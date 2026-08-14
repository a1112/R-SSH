#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
  echo "usage: x11-set-clipboard.sh TEXT" >&2
  exit 2
fi

if [[ "${RSSH_FUNCTIONAL_WAYLAND_CLIPBOARD:-0}" == 1 ]]; then
  printf '%s' "$1" | wl-copy --paste-once --type 'text/plain;charset=utf-8'
else
  printf '%s' "$1" | xclip -selection clipboard -loops 1
fi
