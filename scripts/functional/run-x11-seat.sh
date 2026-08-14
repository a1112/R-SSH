#!/usr/bin/env bash
set -euo pipefail

if (($# == 0)); then
  echo "usage: run-x11-seat.sh COMMAND [ARG ...]" >&2
  exit 2
fi

runtime="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rssh-functional-x11-$RANDOM"
mkdir -m 700 "$runtime"
export XDG_RUNTIME_DIR="$runtime"
export GDK_BACKEND=x11
export NO_AT_BRIDGE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1

cleanup() {
  status=$?
  trap - EXIT
  if ((status != 0)) && [[ -s "$runtime/openbox.log" ]]; then
    echo "--- openbox startup log ---" >&2
    sed -n '1,200p' "$runtime/openbox.log" >&2
  fi
  rm -rf -- "$runtime"
  exit "$status"
}
trap cleanup EXIT

dbus-run-session -- xvfb-run --auto-servernum \
  --server-args="-screen 0 1280x800x24 -nolisten tcp" \
  bash -c 'openbox >"$XDG_RUNTIME_DIR/openbox.log" 2>&1 & exec "$@"' bash "$@"
