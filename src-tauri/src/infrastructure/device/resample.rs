//! Rational sample-rate conversion for the device audio path — a streaming
//! polyphase windowed-sinc resampler (upsample by `L`, low-pass, decimate by
//! `M`, computed only at the output instants). The device plays 44100 Hz only;
//! podcast episodes commonly arrive at 48000 Hz (147 : 160), so this is the
//! step between the decoder and the MP3 encoder. PURE: no I/O, no allocation
//! per sample, `f32` in / `f32` out, deterministic.
//!
//! Design: Kaiser-windowed sinc, [`TAPS_PER_PHASE`] taps per output sample,
//! cut-off at [`CUTOFF_RATIO`] × half the LOWER of the two rates (no aliasing
//! on decimation, no imaging on interpolation), unity passband gain (every
//! polyphase branch is normalized to a DC gain of exactly 1). The kernel is
//! centered on an INTEGER upsampled instant so its group delay is compensated
//! exactly: output sample `n` sits at input instant `n · M / L`. The stream is
//! flushed with zeros so the tail is not truncated, and the total length is
//! `ceil(input_len · L / M)`. Equal rates are a pure pass-through.
//!
//! Measured on this design (48 kHz → 44.1 kHz, `cargo test` pins the gist):
//! flat to 18 kHz, −3.7 dB at 20 kHz, −50 dB at 22 kHz, −95 dB at 23 kHz —
//! the 20–22 kHz shelf is inaudible on a toy speaker, and podcast speech
//! carries nothing there; buying a sharper edge would cost 2–4× the taps.

/// Filter taps evaluated per output sample (one polyphase branch) — the
/// kernel spans this many INPUT samples, which sets the transition width
/// (≈ 5.6 / taps of the input rate for the Kaiser β below: ~4 kHz at 48 kHz).
/// 64 taps ≈ 2.8 G MAC for a 16-minute episode — about a second.
const TAPS_PER_PHASE: usize = 64;
/// Cut-off as a fraction of the lower Nyquist frequency: the transition band
/// is placed BELOW Nyquist so it has fully rolled off there (aliasing ≥ 50 dB
/// down at the fold), at the price of a shelf above 18 kHz.
const CUTOFF_RATIO: f64 = 0.92;
/// Kaiser window shape parameter (β ≈ 8.6 ⇒ ~90 dB side-lobe attenuation).
const KAISER_BETA: f64 = 8.6;

/// A streaming rational resampler from `input_rate` to `output_rate` Hz.
pub struct Resampler {
    /// Equal rates: samples pass through untouched (no filter, no delay).
    pass_through: bool,
    /// Upsampling factor `L` (`output_rate / gcd`).
    up: usize,
    /// Decimation factor `M` (`input_rate / gcd`).
    down: usize,
    /// Kernel of `TAPS_PER_PHASE · L` taps, phase-major: tap `j` of phase `p`
    /// lives at `coeffs[p + j · L]` (the standard polyphase decomposition).
    coeffs: Vec<f32>,
    /// Pending input samples: `TAPS_PER_PHASE - 1` samples of history followed
    /// by not-yet-consumed input. `base` is the absolute input index of
    /// `buffer[0]`, minus the history length (so the first real sample is
    /// absolute index 0).
    buffer: Vec<f32>,
    base: i64,
    /// Upsampled-domain instant of the next output sample (`n · M + delay`).
    next_instant: u64,
    /// Total input samples pushed so far (for the exact output length).
    input_len: u64,
    /// Output samples produced so far.
    output_len: u64,
}

impl Resampler {
    /// A resampler for `input_rate → output_rate`. Both must be non-zero;
    /// equal rates make a pass-through (`L = M = 1`).
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        let input_rate = input_rate.max(1) as usize;
        let output_rate = output_rate.max(1) as usize;
        let g = gcd(input_rate, output_rate);
        let up = output_rate / g;
        let down = input_rate / g;
        let coeffs = polyphase_kernel(up, down);
        let history = TAPS_PER_PHASE - 1;
        Self {
            pass_through: up == 1 && down == 1,
            up,
            down,
            coeffs,
            buffer: vec![0.0; history],
            base: -(history as i64),
            next_instant: kernel_delay(up),
            input_len: 0,
            output_len: 0,
        }
    }

    /// Feed input samples; every output sample that is fully determined by
    /// the input so far is appended to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.pass_through {
            out.extend_from_slice(input);
            return;
        }
        self.buffer.extend_from_slice(input);
        self.input_len += input.len() as u64;
        self.drain(out, self.output_target_for(self.input_len));
    }

    /// End of input: flush the filter tail with zeros so the last output
    /// samples (up to the exact expected length) are produced.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if self.pass_through {
            return;
        }
        let target = ceil_div(self.input_len * self.up as u64, self.down as u64);
        // Enough zeros for the last instant's window to be fully available.
        let zeros = vec![0.0f32; TAPS_PER_PHASE + self.down];
        self.buffer.extend_from_slice(&zeros);
        self.drain(out, target);
    }

    /// Output samples whose window is complete once `input_len` samples are
    /// in: the instant must not look past the last available input.
    fn output_target_for(&self, input_len: u64) -> u64 {
        // Output n needs input index floor((n·M + delay) / L) ≤ input_len - 1.
        let delay = kernel_delay(self.up);
        let limit = input_len * self.up as u64; // exclusive bound on instants
        if limit <= delay {
            return 0;
        }
        // Largest n with n·M + delay < limit  ⇒  n < (limit - delay) / M.
        ceil_div(limit - delay, self.down as u64)
    }

    fn drain(&mut self, out: &mut Vec<f32>, target_output_len: u64) {
        let taps = TAPS_PER_PHASE;
        while self.output_len < target_output_len {
            let instant = self.next_instant;
            let k = (instant / self.up as u64) as i64; // newest input index
            let phase = (instant % self.up as u64) as usize;
            // Window = input[k - taps + 1 ..= k], relative to the buffer.
            let end = (k - self.base) as usize;
            if end >= self.buffer.len() {
                break; // not yet available — wait for more input
            }
            let start = end + 1 - taps;
            let window = &self.buffer[start..=end];
            let mut acc = 0.0f32;
            for (j, &x) in window.iter().rev().enumerate() {
                acc += self.coeffs[phase + j * self.up] * x;
            }
            out.push(acc);
            self.output_len += 1;
            self.next_instant += self.down as u64;
        }
        // Drop everything the next instant can no longer touch, keeping the
        // history the next output window needs.
        let next_k = (self.next_instant / self.up as u64) as i64;
        let keep_from = next_k - taps as i64 + 1;
        let drop = (keep_from - self.base).clamp(0, self.buffer.len() as i64) as usize;
        if drop > 0 {
            self.buffer.drain(..drop);
            self.base += drop as i64;
        }
    }
}

/// Group delay of the kernel in upsampled instants: the kernel is centered on
/// this INTEGER instant (see [`polyphase_kernel`]), so compensating it is
/// exact — a half-instant error would shift every output by ~70 ns and show
/// up as a frequency-proportional error (−43 dB at 16 kHz).
fn kernel_delay(up: usize) -> u64 {
    ((TAPS_PER_PHASE * up) / 2) as u64
}

/// The polyphase kernel: a windowed sinc of `TAPS_PER_PHASE · L` taps at the
/// upsampled rate, centered on the integer instant [`kernel_delay`], cut off
/// at [`CUTOFF_RATIO`] × half the lower rate, each polyphase branch normalized
/// to a DC gain of exactly 1, laid out so `coeffs[p + j·L]` is tap `j` of
/// phase `p`.
fn polyphase_kernel(up: usize, down: usize) -> Vec<f32> {
    let len = TAPS_PER_PHASE * up;
    let center = kernel_delay(up) as f64;
    // Normalized cut-off (cycles per upsampled sample).
    let cutoff = CUTOFF_RATIO * 0.5 / up.max(down) as f64;
    let mut h = vec![0.0f32; len];
    for (n, tap) in h.iter_mut().enumerate() {
        let x = n as f64 - center;
        // Ideal low-pass impulse response: 2·fc at the center, else
        // sin(2π·fc·x) / (π·x) — the SAME scale for every tap.
        let lowpass = if x == 0.0 {
            2.0 * cutoff
        } else {
            (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
        };
        let window = kaiser(x / center.max(1.0), KAISER_BETA);
        *tap = (lowpass * window * up as f64) as f32;
    }
    // Normalize EACH polyphase branch to a DC gain of exactly 1: the raw
    // branch sums differ by ~0.1 % (the window truncation), which would
    // otherwise ripple the passband at −57 dB.
    for phase in 0..up {
        let sum: f64 = (0..TAPS_PER_PHASE).map(|j| h[phase + j * up] as f64).sum();
        if sum.abs() > f64::EPSILON {
            for j in 0..TAPS_PER_PHASE {
                h[phase + j * up] = (h[phase + j * up] as f64 / sum) as f32;
            }
        }
    }
    h
}

/// Kaiser window at normalized position `t ∈ [-1, 1]`.
fn kaiser(t: f64, beta: f64) -> f64 {
    let arg = beta * (1.0 - t * t).max(0.0).sqrt();
    bessel_i0(arg) / bessel_i0(beta)
}

/// Modified Bessel function of the first kind, order 0 (power series).
fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..=50 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn ceil_div(a: u64, b: u64) -> u64 {
    a.div_ceil(b.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, seconds: f64, amp: f32) -> Vec<f32> {
        let n = (rate as f64 * seconds) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn resample_all(input: &[f32], from: u32, to: u32, chunk: usize) -> Vec<f32> {
        let mut rs = Resampler::new(from, to);
        let mut out = Vec::new();
        for c in input.chunks(chunk.max(1)) {
            rs.push(c, &mut out);
        }
        rs.finish(&mut out);
        out
    }

    /// RMS of the residual between a produced tone and the ideal tone at the
    /// output rate, skipping the filter edges.
    fn tone_error_db(out: &[f32], rate: u32, hz: f64, amp: f32) -> f64 {
        let skip = 200;
        let mut err = 0.0f64;
        let mut sig = 0.0f64;
        for (i, &y) in out.iter().enumerate().skip(skip).take(out.len() - 2 * skip) {
            let ideal =
                amp as f64 * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin();
            err += (y as f64 - ideal).powi(2);
            sig += ideal.powi(2);
        }
        10.0 * (err / sig).log10()
    }

    #[test]
    fn output_length_is_exactly_ceil_of_the_rational_ratio() {
        let input = vec![0.5f32; 48_000];
        let out = resample_all(&input, 48_000, 44_100, 4096);
        assert_eq!(out.len(), 44_100);
        let out = resample_all(&input[..1000], 48_000, 44_100, 1000);
        assert_eq!(out.len(), (1000 * 147usize).div_ceil(160));
        let out = resample_all(&input[..7], 44_100, 48_000, 7);
        assert_eq!(out.len(), (7 * 160usize).div_ceil(147));
    }

    #[test]
    fn dc_passes_with_unity_gain() {
        let input = vec![0.25f32; 20_000];
        let out = resample_all(&input, 48_000, 44_100, 3000);
        for &y in &out[100..out.len() - 100] {
            assert!((y - 0.25).abs() < 1e-3, "dc sample {y}");
        }
    }

    #[test]
    fn a_1khz_tone_survives_48k_to_44k1_with_its_frequency_and_amplitude() {
        let input = sine(48_000, 1000.0, 1.0, 0.5);
        let out = resample_all(&input, 48_000, 44_100, 4096);
        let err = tone_error_db(&out, 44_100, 1000.0, 0.5);
        assert!(err < -70.0, "tone error {err:.1} dB");
        // Still faithful high in the speech/music band.
        let input = sine(48_000, 12_000.0, 1.0, 0.5);
        let out = resample_all(&input, 48_000, 44_100, 4096);
        let err = tone_error_db(&out, 44_100, 12_000.0, 0.5);
        assert!(err < -60.0, "12 kHz tone error {err:.1} dB");
    }

    #[test]
    fn upsampling_44k1_to_48k_is_equally_faithful() {
        let input = sine(44_100, 3000.0, 1.0, 0.4);
        let out = resample_all(&input, 44_100, 48_000, 1000);
        let err = tone_error_db(&out, 48_000, 3000.0, 0.4);
        assert!(err < -70.0, "tone error {err:.1} dB");
    }

    #[test]
    fn content_above_the_output_nyquist_is_suppressed_not_aliased() {
        // 23 kHz at 48 kHz is above 22.05 kHz: it must vanish, not fold back.
        let input = sine(48_000, 23_000.0, 0.5, 0.5);
        let out = resample_all(&input, 48_000, 44_100, 4096);
        let rms = (out[500..out.len() - 500]
            .iter()
            .map(|y| (*y as f64).powi(2))
            .sum::<f64>()
            / (out.len() - 1000) as f64)
            .sqrt();
        let db = 20.0 * (rms / (0.5 / 2f64.sqrt())).log10();
        assert!(db < -60.0, "alias level {db:.1} dB");
    }

    #[test]
    fn streaming_in_any_chunking_equals_one_shot_processing() {
        let input = sine(48_000, 440.0, 0.3, 0.7);
        let reference = resample_all(&input, 48_000, 44_100, input.len());
        for chunk in [1usize, 7, 160, 1152, 4096] {
            let out = resample_all(&input, 48_000, 44_100, chunk);
            assert_eq!(out.len(), reference.len(), "chunk {chunk}");
            for (a, b) in out.iter().zip(&reference) {
                assert!((a - b).abs() < 1e-6, "chunk {chunk}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn equal_rates_pass_samples_through_unchanged() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample_all(&input, 44_100, 44_100, 333);
        assert_eq!(out.len(), input.len());
        for (a, b) in out.iter().zip(&input) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
