#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
  echo "usage: x11-set-clipboard.sh TEXT" >&2
  exit 2
fi

printf '%s' "$1" | xclip -selection clipboard -loops 1
