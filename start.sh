#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if [ -x ".venv/bin/python" ]; then
  PY=".venv/bin/python"
else
  PY="python3"
fi

if [ "$#" -eq 0 ]; then
  set -- run
elif [ "${1#-}" != "$1" ]; then
  # 允许 ./start.sh --once 这类写法，默认补上 run
  set -- run "$@"
fi

exec "$PY" -m jjcounter "$@"
