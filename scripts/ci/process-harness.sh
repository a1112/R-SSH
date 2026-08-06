#!/usr/bin/env bash
set -euo pipefail

active_wrapper_pid=""
version_capture=""

cleanup() {
  if [[ -n "$active_wrapper_pid" ]] && kill -0 "$active_wrapper_pid" 2>/dev/null; then
    kill -TERM "$active_wrapper_pid"
    if ! wait "$active_wrapper_pid"; then
      : # The wrapper reports the phase failure before the EXIT trap runs.
    fi
  fi
  if [[ -n "$version_capture" && -f "$version_capture" ]]; then
    rm -f "$version_capture"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

run_bounded() {
  local phase="$1"
  local timeout_seconds="$2"
  shift 2

  python3 - "$phase" "$timeout_seconds" "$@" <<'PY' &
import os
import select
import signal
import subprocess
import sys
import time

phase = sys.argv[1]
timeout_seconds = int(sys.argv[2])
command = sys.argv[3:]
child = None
process_group = None

def group_is_alive():
    if process_group is None:
        return False
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # Darwin can report EPERM after the session leader exits and its last
        # descendant is killed/reparented. The group is no longer one this
        # wrapper can own or wait on; treating that terminal state as alive
        # would turn successful cleanup into a harness failure.
        return False
    return True

def wait_for_leader_until(deadline):
    if child is None or child.poll() is not None:
        return
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return
    try:
        child.wait(timeout=remaining)
    except subprocess.TimeoutExpired:
        pass

def wait_for_group_until(deadline):
    while group_is_alive() and time.monotonic() < deadline:
        select.select([], [], [], 0.01)

def terminate_tree():
    if not group_is_alive():
        wait_for_leader_until(time.monotonic() + 5)
        return
    try:
        os.killpg(process_group, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        pass
    term_deadline = time.monotonic() + 5
    wait_for_leader_until(term_deadline)
    wait_for_group_until(term_deadline)
    if group_is_alive():
        try:
            os.killpg(process_group, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        kill_deadline = time.monotonic() + 5
        wait_for_leader_until(kill_deadline)
        wait_for_group_until(kill_deadline)
    if group_is_alive():
        raise RuntimeError(f"process group {process_group} survived TERM and KILL")

def handle_signal(signum, _frame):
    terminate_tree()
    raise SystemExit(128 + signum)

signal.signal(signal.SIGINT, handle_signal)
signal.signal(signal.SIGTERM, handle_signal)
child = subprocess.Popen(command, start_new_session=True)
process_group = child.pid
try:
    status = child.wait(timeout=timeout_seconds)
except subprocess.TimeoutExpired:
    terminate_tree()
    print(
        f"{phase} exceeded its {timeout_seconds}s timeout; process group was killed",
        file=sys.stderr,
    )
    raise SystemExit(124)
terminate_tree()
raise SystemExit(status)
PY
  active_wrapper_pid=$!
  local status=0
  if wait "$active_wrapper_pid"; then
    status=0
  else
    status=$?
  fi
  active_wrapper_pid=""
  if ((status != 0)); then
    printf '%s failed with exit code %d\n' "$phase" "$status" >&2
    return "$status"
  fi
}

run_harness_self_test() {
  local harness_directory
  harness_directory="$(mktemp -d)"
  local argv_capture="$harness_directory/argv.json"
  local timeout_capture="$harness_directory/timeout.log"
  local sentinel="$harness_directory/grandchild.pid"
  local leader_capture="$harness_directory/leader.log"
  local leader_sentinel="$harness_directory/leader-grandchild.pid"
  local argv_program='import json, pathlib, sys; pathlib.Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]), encoding="utf-8")'

  run_bounded "quoted argv round-trip" 10 \
    python3 -c "$argv_program" "$argv_capture" "" "two words" 'quote"inside' 'path with space/'
  python3 - "$argv_capture" <<'PY'
import json
import pathlib
import sys

observed = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
expected = ["", "two words", 'quote"inside', "path with space/"]
if observed != expected:
    raise SystemExit(f"quoted argv round-trip mismatch: {observed!r} != {expected!r}")
PY

  local timeout_status=0
  local timeout_program
  timeout_program=$'import pathlib, signal, subprocess, sys\ngrandchild = subprocess.Popen([sys.executable, "-c", "import signal; signal.signal(signal.SIGTERM, signal.SIG_IGN); signal.pause()"])\npathlib.Path(sys.argv[1]).write_text(str(grandchild.pid), encoding="utf-8")\nprint("timeout-stdout-marker", flush=True)\nprint("timeout-stderr-marker", file=sys.stderr, flush=True)\nsignal.pause()'
  if run_bounded "process-group timeout self-test" 2 \
    python3 -c "$timeout_program" "$sentinel" >"$timeout_capture" 2>&1
  then
    timeout_status=0
  else
    timeout_status=$?
  fi
  if ((timeout_status != 124)); then
    cat "$timeout_capture" >&2
    printf 'process-group timeout self-test returned %d, expected 124\n' "$timeout_status" >&2
    return 1
  fi
  for marker in timeout-stdout-marker timeout-stderr-marker; do
    if ! grep -Fq "$marker" "$timeout_capture"; then
      printf 'process-group timeout self-test did not drain %s\n' "$marker" >&2
      return 1
    fi
  done
  if [[ ! -s "$sentinel" ]]; then
    printf '%s\n' 'process-group timeout self-test did not record its grandchild PID' >&2
    return 1
  fi
  python3 - "$sentinel" <<'PY'
import os
import pathlib
import select
import sys
import time

pid = int(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
deadline = time.monotonic() + 5
while True:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        break
    if time.monotonic() >= deadline:
        raise SystemExit(f"process-group timeout self-test left grandchild PID {pid} alive")
    select.select([], [], [], 0.01)
PY
  local leader_program
  leader_program=$'import pathlib, signal, subprocess, sys\ngrandchild = subprocess.Popen([sys.executable, "-c", "import signal; signal.signal(signal.SIGTERM, signal.SIG_IGN); signal.pause()"])\npathlib.Path(sys.argv[1]).write_text(str(grandchild.pid), encoding="utf-8")\nprint("leader-exit-marker", flush=True)'
  run_bounded "leader-exit process-group self-test" 10 \
    python3 -c "$leader_program" "$leader_sentinel" >"$leader_capture" 2>&1
  if ! grep -Fq "leader-exit-marker" "$leader_capture"; then
    printf '%s\n' 'leader-exit process-group self-test did not drain stdout' >&2
    return 1
  fi
  python3 - "$leader_sentinel" <<'PY'
import os
import pathlib
import select
import sys
import time

pid = int(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
deadline = time.monotonic() + 5
while True:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        break
    if time.monotonic() >= deadline:
        raise SystemExit(f"leader-exit process-group self-test left grandchild PID {pid} alive")
    select.select([], [], [], 0.01)
PY
  rm -f "$argv_capture" "$timeout_capture" "$sentinel" "$leader_capture" "$leader_sentinel"
  rmdir "$harness_directory"
}
