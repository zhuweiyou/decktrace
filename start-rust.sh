#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if [ "$#" -eq 0 ]; then
  set -- run
fi

exec cargo run -q --release -p decktrace-cli -- "$@"
