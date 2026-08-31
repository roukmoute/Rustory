#!/usr/bin/env python3
"""APS runner worker: executes the generated acceptance suite per NDJSON job.

The APS mutator launches this process once and exchanges ONE JSON object
per line on stdin (request) / stdout (response):

    request:  {"id": "m1", "feature_json": "/abs/.../feature.json",
               "generated_dir": "/abs/.../generated",
               "work_dir": "/abs/.../mutations/m1"}
    response: {"id": "m1", "outcome": "test_success|test_failure|infrastructure_error",
               "output": "<tail>", "error": "<tail>", "duration": <nanoseconds>}

Per job the worker:
  1. refreshes the repo slot src-tauri/tests/acceptance/generated/entry_points.rs
     from <generated_dir>/entry_points.rs when the content differs;
  2. runs `cargo test --test acceptance -- --test-threads=1` in src-tauri/
     with RUSTORY_ACCEPTANCE_IR=<feature_json> (a hard 600s timeout kills
     the whole process group);
  3. classifies: exit 0 -> test_success, a cargo compile failure
     (`error[` / `could not compile` on stderr) -> infrastructure_error,
     anything else -> test_failure.

Diagnostics go to stderr, which the mutator inherits; stdout carries the
protocol only.
"""
from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

SRC_TAURI_DIR = Path(__file__).resolve().parents[1]
SLOT_PATH = SRC_TAURI_DIR / "tests" / "acceptance" / "generated" / "entry_points.rs"
JOB_TIMEOUT_SECONDS = 600
OUTPUT_TAIL_BYTES = 4096


def diagnose(message: str) -> None:
    print(f"[aps-worker] {message}", file=sys.stderr, flush=True)


def respond(job_id: str, outcome: str, output: str, error: str, duration_ns: int) -> None:
    line = json.dumps(
        {
            "id": job_id,
            "outcome": outcome,
            "output": output,
            "error": error,
            "duration": duration_ns,
        },
        ensure_ascii=False,
    )
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def tail(text: str) -> str:
    data = text.encode("utf-8", errors="replace")
    if len(data) <= OUTPUT_TAIL_BYTES:
        return text
    return data[-OUTPUT_TAIL_BYTES:].decode("utf-8", errors="replace")


def refresh_slot(generated_dir: str) -> None:
    source = Path(generated_dir) / "entry_points.rs"
    if not source.is_file():
        raise FileNotFoundError(f"missing {source} in the generated dir")
    payload = source.read_bytes()
    if SLOT_PATH.is_file() and SLOT_PATH.read_bytes() == payload:
        return
    SLOT_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp = SLOT_PATH.with_suffix(".rs.tmp")
    tmp.write_bytes(payload)
    os.replace(tmp, SLOT_PATH)
    diagnose(f"slot refreshed from {source}")


def run_job(job: dict) -> tuple[str, str, str, int]:
    started = time.monotonic_ns()
    job_id = str(job.get("id", "unknown"))
    feature_json = job.get("feature_json")
    generated_dir = job.get("generated_dir")
    if not feature_json or not generated_dir:
        return "infrastructure_error", "", "worker request must carry feature_json and generated_dir", time.monotonic_ns() - started
    if not Path(feature_json).is_file():
        return "infrastructure_error", "", f"feature_json not found: {feature_json}", time.monotonic_ns() - started
    refresh_slot(generated_dir)
    env = dict(os.environ)
    env["RUSTORY_ACCEPTANCE_IR"] = os.path.abspath(feature_json)
    cargo_bin = os.path.join(os.path.expanduser("~"), ".cargo", "bin")
    env["PATH"] = cargo_bin + os.pathsep + env.get("PATH", "")
    command = ["cargo", "test", "--test", "acceptance", "--", "--test-threads=1"]
    try:
        proc = subprocess.Popen(
            command,
            cwd=str(SRC_TAURI_DIR),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
    except OSError as err:
        return "infrastructure_error", "", f"cannot launch cargo: {err}", time.monotonic_ns() - started
    try:
        stdout_b, stderr_b = proc.communicate(timeout=JOB_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except OSError:
            proc.kill()
        stdout_b, stderr_b = proc.communicate()
        duration = time.monotonic_ns() - started
        return (
            "infrastructure_error",
            tail(stdout_b.decode("utf-8", errors="replace")),
            f"acceptance run exceeded {JOB_TIMEOUT_SECONDS}s and was killed",
            duration,
        )
    stdout = stdout_b.decode("utf-8", errors="replace")
    stderr = stderr_b.decode("utf-8", errors="replace")
    duration = time.monotonic_ns() - started
    output = tail(stdout + ("\n[stderr]\n" + stderr if stderr.strip() else ""))
    if proc.returncode == 0:
        return "test_success", output, "", duration
    if "could not compile" in stderr or re.search(r"error\[[A-Z0-9]+\]", stderr):
        return "infrastructure_error", output, tail(stderr), duration
    return "test_failure", output, tail(stderr), duration


def main() -> int:
    diagnose(f"worker ready (src-tauri: {SRC_TAURI_DIR})")
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            job = json.loads(line)
            job_id = str(job.get("id", "unknown"))
        except json.JSONDecodeError as err:
            diagnose(f"unparseable request line: {err}")
            respond("unknown", "infrastructure_error", "", f"unparseable request: {err}", 0)
            continue
        diagnose(f"job {job_id} start")
        try:
            outcome, output, error, duration = run_job(job)
        except Exception as err:  # noqa: BLE001 - the protocol must keep flowing
            outcome, output, error, duration = "infrastructure_error", "", repr(err), 0
        diagnose(f"job {job_id} done: {outcome} ({duration} ns)")
        respond(job_id, outcome, output, error, duration)
    return 0


if __name__ == "__main__":
    sys.exit(main())
