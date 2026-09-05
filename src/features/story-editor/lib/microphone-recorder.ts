import { encodeWav, trimSilence } from "./wav";

/** A finished take: mono 16-bit WAV bytes, its length, its sample rate. */
export interface RecordedTake {
  wav: Uint8Array;
  durationMs: number;
  sampleRate: number;
}

/** One capture session: started, then stopped into a take (or aborted). */
export interface RecordingSession {
  /** Stop capturing and produce the take (silence trimmed). */
  stop(): Promise<RecordedTake>;
  /** Abort without a take, releasing the microphone. */
  cancel(): void;
}

/** Why a recording could not start. Closed set, worded by the UI. */
export type RecorderFailure = "unsupported" | "denied" | "noDevice" | "failed";

export class RecorderError extends Error {
  readonly failure: RecorderFailure;
  constructor(failure: RecorderFailure, message: string) {
    super(message);
    this.name = "RecorderError";
    this.failure = failure;
  }
}

/** Starts capture sessions. Injectable so components are testable. */
export interface Recorder {
  start(): Promise<RecordingSession>;
}

/** Upper bound on a take: a spoken title is seconds long. */
export const MAX_TAKE_SECONDS = 60;

/**
 * The production recorder: the webview's microphone (getUserMedia) read as
 * raw PCM through Web Audio — no browser codec — and encoded as WAV by
 * [`encodeWav`]. Mono, at the audio context's native rate; Rust transcodes
 * to the device format at send time.
 */
export class MicrophoneRecorder implements Recorder {
  async start(): Promise<RecordingSession> {
    const media = globalThis.navigator?.mediaDevices;
    const AudioContextCtor = (globalThis as { AudioContext?: typeof AudioContext }).AudioContext;
    if (!media?.getUserMedia || !AudioContextCtor) {
      throw new RecorderError("unsupported", "Le micro n'est pas disponible dans cette version.");
    }
    let stream: MediaStream;
    try {
      stream = await media.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
      });
    } catch (err) {
      const name = (err as { name?: string })?.name ?? "";
      if (name === "NotAllowedError" || name === "SecurityError") {
        throw new RecorderError("denied", "L'accès au micro a été refusé.");
      }
      if (name === "NotFoundError" || name === "OverconstrainedError") {
        throw new RecorderError("noDevice", "Aucun micro n'a été trouvé.");
      }
      throw new RecorderError("failed", "Le micro n'a pas pu démarrer.");
    }
    const context = new AudioContextCtor();
    const source = context.createMediaStreamSource(stream);
    // ScriptProcessorNode: deprecated but universal across the three
    // webviews, and the simplest raw-PCM tap without a worklet file.
    const processor = context.createScriptProcessor(4096, 1, 1);
    const chunks: Float32Array[] = [];
    let total = 0;
    const maxSamples = MAX_TAKE_SECONDS * context.sampleRate;
    processor.onaudioprocess = (event) => {
      if (total >= maxSamples) return;
      const input = event.inputBuffer.getChannelData(0);
      const copy = new Float32Array(input);
      chunks.push(copy);
      total += copy.length;
    };
    source.connect(processor);
    processor.connect(context.destination);

    const release = (): void => {
      processor.disconnect();
      source.disconnect();
      for (const track of stream.getTracks()) track.stop();
      void context.close().catch(() => undefined);
    };

    return {
      async stop() {
        release();
        const samples = new Float32Array(total);
        let offset = 0;
        for (const chunk of chunks) {
          samples.set(chunk, offset);
          offset += chunk.length;
        }
        const trimmed = trimSilence(samples, context.sampleRate);
        return {
          wav: encodeWav(trimmed, context.sampleRate),
          durationMs: Math.round((trimmed.length / context.sampleRate) * 1000),
          sampleRate: context.sampleRate,
        };
      },
      cancel() {
        release();
      },
    };
  }
}
