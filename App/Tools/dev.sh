#!/bin/zsh
# The development copy («ihatecrypto Dev», bundle app.ihatecrypto.mac.dev, debug API on 51790) from a terminal.
# Never the installed ihatecrypto.app: everything here names the Dev copy's bundle id, port and data folder.
#   Tools/dev.sh state                     what /state says, short: busy, executing, pending card, wallet kind, balances
#   Tools/dev.sh api GET /state            any debug API call; a body as the third argument: api POST /send '{"text":"…"}'
#   Tools/dev.sh busy                      exit 0 when someone uses the copy now (a move, a model answer, uitest, done.sh)
#   Tools/dev.sh relaunch [--build] [args] quit it, rebuild it with build-app.sh (--build), open it with the args, wait for its API
#   Tools/dev.sh idle                      wait until it's neither busy nor executing (10 min at most)
# The token in debug-token changes on every launch: it's read for each call, never kept.
emulate -L zsh
BUNDLE=app.ihatecrypto.mac.dev PORT=51790 DATA=~/Library/Application\ Support/ihatecrypto-dev APP=~/Applications/"ihatecrypto Dev.app"
HERE=${0:A:h}

api() {  # METHOD PATH [JSON]
  local token body=(); token=$(cat $DATA/debug-token 2>/dev/null) || { echo "no debug token: is Dev running with Settings › Debugging › Local debug API on?" >&2; return 1 }
  [[ $1 == GET ]] || body=(-d "${3-}")   # a POST without a body still sends an empty one
  curl -sf -m ${TIMEOUT:-240} -X $1 -H "Authorization: Bearer $token" -H 'Content-Type: application/json' $body http://127.0.0.1:$PORT$2
}
py() { python3 -I -c "import json,sys; d=json.loads(sys.stdin.read(), strict=False); $1" }

busy() {
  pgrep -f 'uitest.py|done.sh' >/dev/null && { echo "uitest.py or done.sh runs"; return 0 }
  local s; s=$(TIMEOUT=5 api GET /state) || return 1
  echo $s | py "b=d['boards']; why=[k for k in ('busy','executing') if d.get(k)] + (['a card waits for Confirm'] if b.get('pending') else []) + (['a move runs'] if b.get('running') else []); print(', '.join(why)); sys.exit(0 if why else 1)"
}

case $1 in
api) shift; api "$@"; echo;;
state)
  api GET /state | py "b=d['boards']; p=b.get('pending') or {}
print('busy' if d['busy'] else 'idle', '· executing' if d['executing'] else '', '·', d['provider'], d['model'], '· test wallet' if d['wallet'].get('test') else '· NOT the test wallet')
print('pending:', p.get('title') or p.get('kind') or '—', '·', p.get('quote') or '')
for s in p.get('steps') or []: print('   ', s['label'], '·', s['title'], '·', ' / '.join(s.get('lines') or []))
print('outcome:', d.get('outcome'), '· balances:', {k: v for k, v in d['balances'].items() if v not in (None, '0')})
for m in d['messages'][-3:]: print('  ', 'you:' if m['user'] else 'app:', m['text'][:300].replace(chr(10), ' '))";;
busy) busy;;
idle)
  for i in {1..300}; do busy >/dev/null || exit 0; sleep 2; done
  echo "still busy after 10 min: $(busy)" >&2; exit 1;;
relaunch)
  shift
  if why=$(busy); then echo "Dev is in use ($why): not relaunching. Ask the session using it, or wait: Tools/dev.sh idle" >&2; exit 1; fi
  osascript -e "tell application id \"$BUNDLE\" to quit" 2>/dev/null
  for i in {1..40}; do pgrep -x 'ihatecrypto Dev' >/dev/null || break; sleep 0.5; done
  pgrep -x 'ihatecrypto Dev' >/dev/null && { echo "Dev didn't quit (a sheet open?)" >&2; exit 1 }
  if [[ $1 == --build ]]; then
    shift
    df -h /System/Volumes/Data | awk 'NR==2 && $5+0 > 85 { print "disk " $5 " full: builds fail without a clear error below 15% free" > "/dev/stderr" }'
    (cd $HERE/.. && unset GIT_DIR GIT_WORK_TREE && ./build-app.sh 2>&1 | grep -E '^\*\* BUILD|error:|installed') || exit 1
  fi
  old=$(stat -f %m $DATA/debug-token 2>/dev/null || echo 0)
  open $APP ${1:+--args} "$@"
  # The new token is written when the API starts; /state may answer half-written JSON for a moment (strict=False above).
  for i in {1..60}; do
    sleep 1
    [[ $(stat -f %m $DATA/debug-token 2>/dev/null || echo 0) -gt $old ]] && TIMEOUT=3 api GET /state >/dev/null 2>&1 && { echo "Dev is up on $PORT"; exit 0 }
  done
  echo "Dev opened but its debug API doesn't answer: Settings › Debugging › Local debug API" >&2; exit 1;;
*) sed -n '2,9p' $0 | sed 's/^# //'; exit 2;;
esac
