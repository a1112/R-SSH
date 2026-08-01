#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -x "$root/R-SSH.app/Contents/MacOS/rssh-app" ]; then
  executable="$root/R-SSH.app/Contents/MacOS/rssh-app"
else
  executable="$root/rssh-app"
fi

exec "$executable" console "$@"
