#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if [ "${1:-}" = "--dev" ]; then
  shift
  exec cargo run -q --release -p decktrace -- "$@"
fi

./scripts/bundle-macos.sh >/dev/null
if [ "$#" -gt 0 ]; then
  open -a "$(pwd)/dist/decktrace.app" --args "$@"
else
  open "$(pwd)/dist/decktrace.app"
fi
