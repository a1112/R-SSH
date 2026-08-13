#!/usr/bin/env bash
set -euo pipefail

if (($# < 2)); then
  echo "usage: x11-xtest-input.sh PID ACTION [ARG ...]" >&2
  exit 2
fi
pid="$1"
action="$2"
shift 2
mapfile -t matches < <(xdotool search --onlyvisible --pid "$pid")
if ((${#matches[@]} != 1)); then
  echo "expected one visible X11 window for PID $pid; observed ${#matches[@]}" >&2
  exit 3
fi
window="${matches[0]}"
xdotool windowactivate --sync "$window"

case "$action" in
  focus) ;;
  type) xdotool type --clearmodifiers --delay 0 --window "$window" -- "$*" ;;
  key) xdotool key --clearmodifiers --window "$window" "$*" ;;
  click)
    xdotool mousemove --window "$window" "$1" "$2"
    case "$3" in left) button=1;; middle) button=2;; right) button=3;; *) exit 2;; esac
    xdotool click --window "$window" "$button"
    ;;
  drag)
    case "$5" in left) button=1;; middle) button=2;; right) button=3;; *) exit 2;; esac
    xdotool mousemove --window "$window" "$1" "$2"
    xdotool mousedown "$button"
    xdotool mousemove --window "$window" "$3" "$4"
    xdotool mouseup "$button"
    ;;
  wheel)
    x="$1"; y="$2"
    for ((i=0; i<${y#-}; i++)); do ((y < 0)) && button=5 || button=4; xdotool click "$button"; done
    for ((i=0; i<${x#-}; i++)); do ((x < 0)) && button=7 || button=6; xdotool click "$button"; done
    ;;
  paste)
    command -v xclip >/dev/null
    printf %s "$*" | xclip -selection clipboard
    xdotool key --clearmodifiers --window "$window" ctrl+v
    ;;
  resize) xdotool windowsize "$window" "$1" "$2" ;;
  window)
    case "$1" in
      minimize) xdotool windowminimize "$window" ;;
      maximize|restore) xdotool key --clearmodifiers --window "$window" alt+F10 ;;
      close) xdotool windowclose "$window" ;;
      *) exit 2 ;;
    esac
    ;;
  *) echo "unsupported XTEST action: $action" >&2; exit 2 ;;
esac
