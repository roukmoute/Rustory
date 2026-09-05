//! Running an external speech program with a budget: the system engines
//! (`say`, `pico2wave`, `espeak-ng`) and the embedded Piper runtime are all
//! separate processes writing a WAV file. One helper waits with a deadline
//! (a hung engine is killed, never waited on forever), and one locates a
//! program on the PATH without spawning a shell.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::SpeechError;

/// Default budget for one synthesis (a few seconds of speech).
pub const SYNTHESIS_BUDGET: Duration = Duration::from_secs(60);

/// Run `command` to completion within `budget`. Stdout/stderr are dropped
/// (the engines write their result to a file). A non-zero exit is an
/// engine failure; a deadline overrun kills the process and is a timeout.
pub fn run_with_budget(
    mut command: Command,
    budget: Duration,
    stage: &'static str,
) -> Result<(), SpeechError> {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let child = command
        .spawn()
        .map_err(|_| SpeechError::EngineFailed(stage))?;
    wait_with_budget(child, budget, stage)
}

/// Like [`run_with_budget`] for an already spawned child (stdin fed by the
/// caller).
pub fn wait_with_budget(
    mut child: Child,
    budget: Duration,
    stage: &'static str,
) -> Result<(), SpeechError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(SpeechError::EngineFailed(stage))
                };
            }
            Ok(None) => {
                if started.elapsed() > budget {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SpeechError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return Err(SpeechError::EngineFailed(stage)),
        }
    }
}

/// The first executable named `program` on the PATH, if any. A plain PATH
/// walk (no shell), honoring `PATHEXT`-less names on Unix and `.exe` on
/// Windows.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{program}.exe"));
            if is_executable(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The output WAV of an engine run: read, validated, and the temp file
/// removed whatever happens.
pub fn take_wav_output(path: &Path) -> Result<Vec<u8>, SpeechError> {
    let bytes = std::fs::read(path);
    let _ = std::fs::remove_file(path);
    let bytes = bytes.map_err(|_| SpeechError::EngineFailed("output_missing"))?;
    if !super::wav::is_wav(&bytes) {
        return Err(SpeechError::InvalidOutput);
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    /// Write an executable shell script named `name` into `dir` (Unix) so a
    /// test can stand in for a speech program found on the PATH.
    #[cfg(unix)]
    pub fn fake_program(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// The PATH value that puts `dir` first (so fakes shadow real programs).
    pub fn path_with(dir: &Path) -> std::ffi::OsString {
        let current = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.to_path_buf()];
        paths.extend(std::env::split_paths(&current));
        std::env::join_paths(paths).expect("join paths")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_hung_program_is_killed_at_the_deadline() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        let started = Instant::now();
        assert_eq!(
            run_with_budget(cmd, Duration::from_millis(200), "hang"),
            Err(SpeechError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_failing_program_is_an_engine_failure_and_a_good_one_succeeds() {
        let mut bad = Command::new("sh");
        bad.args(["-c", "exit 3"]);
        assert_eq!(
            run_with_budget(bad, Duration::from_secs(5), "bad"),
            Err(SpeechError::EngineFailed("bad"))
        );
        let mut good = Command::new("sh");
        good.args(["-c", "exit 0"]);
        assert_eq!(
            run_with_budget(good, Duration::from_secs(5), "good"),
            Ok(())
        );
    }

    #[test]
    fn finds_a_program_on_the_path_without_a_shell() {
        assert!(find_on_path("sh").is_some());
        assert!(find_on_path("definitely-not-a-program-xyz").is_none());
    }

    #[test]
    fn a_non_wav_output_is_refused_and_the_file_removed() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.wav");
        std::fs::write(&out, b"not a wav").unwrap();
        assert_eq!(take_wav_output(&out), Err(SpeechError::InvalidOutput));
        assert!(!out.exists());
        assert_eq!(
            take_wav_output(&out),
            Err(SpeechError::EngineFailed("output_missing"))
        );
    }
}
