#!/usr/bin/env bash
set -euo pipefail

if (($# == 0)); then
  echo "usage: run-wayland-seat.sh COMMAND [ARG ...]" >&2
  exit 2
fi
runtime="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rssh-functional-wayland-$RANDOM"
mkdir -m 700 "$runtime"
export XDG_RUNTIME_DIR="$runtime"
export DISPLAY=:98
export WAYLAND_DISPLAY=wayland-rssh-functional
Xvfb "$DISPLAY" -screen 0 1280x800x24 >"$runtime/xvfb.log" 2>&1 &
xvfb_pid=$!
weston --backend=x11-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 >"$runtime/weston.log" 2>&1 &
weston_pid=$!
cleanup() {
  status=$?
  trap - EXIT
  kill -TERM "$weston_pid" "$xvfb_pid" 2>/dev/null || true
  wait "$weston_pid" 2>/dev/null || true
  wait "$xvfb_pid" 2>/dev/null || true
  rm -rf -- "$runtime"
  exit "$status"
}
trap cleanup EXIT
for _ in {1..100}; do
  [[ -S "$runtime/$WAYLAND_DISPLAY" ]] && break
  kill -0 "$weston_pid"
  sleep 0.05
done
test -S "$runtime/$WAYLAND_DISPLAY"
export RSSH_FUNCTIONAL_WESTON_BACKEND=x11
export RSSH_FUNCTIONAL_COMPOSITOR_LOG="$runtime/weston.log"
export RSSH_FUNCTIONAL_WESTON_WINDOW
RSSH_FUNCTIONAL_WESTON_WINDOW="$(xdotool search --onlyvisible --class weston | head -n1)"
test -n "$RSSH_FUNCTIONAL_WESTON_WINDOW"
"$@"
