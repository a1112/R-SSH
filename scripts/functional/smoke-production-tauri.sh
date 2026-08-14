#!/usr/bin/env bash
set -euo pipefail

binary=""
evidence_directory=""
input_backend=""
while (($#)); do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --evidence-directory) evidence_directory="$2"; shift 2 ;;
    --input-backend) input_backend="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -n "$binary" && -n "$evidence_directory" && -n "$input_backend" ]] || {
  echo "usage: smoke-production-tauri.sh --binary PATH --evidence-directory PATH --input-backend x11|macos" >&2
  exit 2
}

binary="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
mkdir -p "$evidence_directory"
evidence_directory="$(cd "$evidence_directory" && pwd)"
stdout="$evidence_directory/stdout"
stderr="$evidence_directory/stderr"
process_tree="$evidence_directory/process-tree.json"
screenshot="$evidence_directory/failure-screenshot.png"
root_pid=""
owned_pids=()

descendants() {
  local parent child
  local -a pending=("$1")
  while ((${#pending[@]})); do
    parent="${pending[0]}"
    pending=("${pending[@]:1}")
    while IFS= read -r child; do
      [[ -n "$child" ]] || continue
      printf '%s\n' "$child"
      pending+=("$child")
    done < <(pgrep -P "$parent" 2>/dev/null || true)
  done
}

session_descendants() {
  local pid command
  while IFS= read -r pid; do
    command="$(ps -p "$pid" -o comm= 2>/dev/null || true)"
    case "$command" in
      *WebKit*|*bwrap*|*xdg-dbus-proxy*) ;;
      *) printf '%s\n' "$pid" ;;
    esac
  done < <(descendants "$1")
}

wait_condition() {
  local seconds="$1" failure="$2"
  shift 2
  local deadline=$((SECONDS + seconds))
  while ((SECONDS < deadline)); do
    "$@" && return 0
    sleep 0.05
  done
  echo "$failure" >&2
  return 1
}

root_alive() { kill -0 "$root_pid" 2>/dev/null; }
root_exited() { ! kill -0 "$root_pid" 2>/dev/null; }
session_started() { [[ -n "$(session_descendants "$root_pid")" ]]; }
session_stopped() { [[ -z "$(session_descendants "$root_pid")" ]]; }
all_owned_exited() {
  local pid
  for pid in "${owned_pids[@]}"; do
    kill -0 "$pid" 2>/dev/null && return 1
  done
}

input() {
  case "$input_backend" in
    x11) bash scripts/functional/x11-xtest-input.sh "$root_pid" "$@" ;;
    macos)
      [[ "${RSSH_FUNCTIONAL_MACOS_ACCESSIBILITY:-}" == authorized ]] || {
        echo "macOS Accessibility authorization is required" >&2
        return 3
      }
      /usr/bin/xcrun swift "${RSSH_FUNCTIONAL_MACOS_CGEVENT_HELPER:?missing CGEvent helper}" --pid "$root_pid" "$@"
      ;;
    *) echo "unsupported input backend: $input_backend" >&2; return 2 ;;
  esac
}

save_process_tree() {
  local remaining=0 pid separator=""
  printf '{"schema":1,"root_process_id":%s,"owned_process_ids":[' "$root_pid" >"$process_tree"
  for pid in "${owned_pids[@]}"; do
    printf '%s%s' "$separator" "$pid" >>"$process_tree"
    separator=,
    kill -0 "$pid" 2>/dev/null && remaining=$((remaining + 1))
  done
  root_exited || remaining=$((remaining + 1))
  printf '],"remaining_owned_processes":%s,"reaped":%s,"pty_interaction":"exit 7"}\n' \
    "$remaining" "$([[ "$remaining" == 0 ]] && echo true || echo false)" >>"$process_tree"
}

save_screenshot() {
  if [[ "$input_backend" == x11 ]] && command -v import >/dev/null; then
    import -window root "$screenshot" || true
  elif [[ "$input_backend" == macos ]] && command -v screencapture >/dev/null; then
    screencapture -x "$screenshot" || true
  fi
}

cleanup() {
  status=$?
  trap - EXIT
  if [[ -n "$root_pid" ]]; then
    root_exited || kill -TERM "$root_pid" 2>/dev/null || true
    wait "$root_pid" 2>/dev/null || true
    save_process_tree
  fi
  ((status == 0)) || save_screenshot
  exit "$status"
}
trap cleanup EXIT

"$binary" >"$stdout" 2>"$stderr" &
root_pid=$!
wait_condition 30 "production Tauri process exited before its window became interactive" root_alive
wait_condition 30 "production Tauri did not start a PTY child" session_started
while IFS= read -r owned_pid; do
  [[ -n "$owned_pid" ]] && owned_pids+=("$owned_pid")
done < <(descendants "$root_pid")

input focus
input type "exit 7"
input key enter
wait_condition 15 "production Tauri PTY child did not exit after OS keyboard input" session_stopped
input window close
wait_condition 10 "production Tauri did not exit after its OS close action" root_exited
wait_condition 10 "production Tauri left an owned helper process" all_owned_exited
wait "$root_pid"
