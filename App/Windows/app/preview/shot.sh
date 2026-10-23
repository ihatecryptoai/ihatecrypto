#!/bin/bash
# Draws the preview in headless Chrome and saves a picture: ./shot.sh out.png "fixture=main&theme=dark" [width height]
# Serves App/Windows/app on a free port for the moment it takes.
set -e
out=$1; query=${2:-fixture=main}; w=${3:-1440}; h=${4:-900}
here=$(cd "$(dirname "$0")" && pwd)
port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 "$here/serve.py" "$port" "$here/.." >/dev/null 2>&1 &
server=$!
trap 'kill $server 2>/dev/null' EXIT
for _ in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:$port/preview/index.html" && break; sleep 0.1; done
profile=$(mktemp -d)
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu --hide-scrollbars --no-first-run \
  --user-data-dir="$profile" --force-device-scale-factor=2 --window-size="$w,$h" --virtual-time-budget=9000 \
  --screenshot="$out" "http://127.0.0.1:$port/preview/index.html?$query" >/dev/null 2>&1
rm -rf "$profile"
echo "$out"
