#!/bin/zsh
# A live run of a money feature on the Dev copy's test wallet: the phrase goes to the chat of a fresh test canvas, the
# card it draws is printed, its buttons are pressed in order (Confirm by default) as a person would, and what the move
# did is printed when it ends. Only the Dev copy, only while its active wallet is the test wallet: anything else is refused.
#   Tools/live.sh "купи 0.01 SPCX за USDC"                       draws the card, presses Confirm, waits for the outcome
#   Tools/live.sh "deposit 5 USDC to kamino" "Send anyway"     a part of a title is enough for a later button
#   DRY=1 Tools/live.sh "…"                                    only draws the card and prints it, then cancels
# A few dollars at most: the test wallet holds a few dollars.
emulate -L zsh
HERE=${0:A:h} AX=${0:A:h}/../.build/axpress
dev() { $HERE/dev.sh "$@" }
py() { python3 -I -c "import json,sys; d=json.loads(sys.stdin.read(), strict=False); $1" }
[[ -n $1 ]] || { sed -n '2,8p' $0 | sed 's/^# //'; exit 2 }
phrase=$1; shift
buttons=("${@:-Confirm}")

state=$(TIMEOUT=10 dev api GET /state) || { echo "Dev doesn't answer: Tools/dev.sh relaunch" >&2; exit 1 }
echo $state | py "sys.exit(0 if d['wallet'].get('test') else 1)" || { echo "The Dev copy's active wallet isn't the test wallet: switch to it first." >&2; exit 1 }
if why=$(dev busy); then echo "Dev is in use ($why)." >&2; exit 1; fi
if [[ -z $DRY && ( ! -x $AX || $HERE/axpress.swift -nt $AX ) ]]; then
  mkdir -p ${AX:h} && swiftc -O $HERE/axpress.swift -o $AX || exit 1
fi
canvas=$(echo $state | py "print(d.get('canvasID') or '')")

# A fresh test canvas, or nothing: the phrase must never land in the canvas the owner left open.
dev api POST /reset >/dev/null || { echo "/reset failed: not sending anything" >&2; exit 1 }
echo "you: $phrase"
dev api POST /send "$(python3 -I -c 'import json,sys; print(json.dumps({"text": sys.argv[1]}))' $phrase)" >/dev/null
# The card's quote comes a moment after the card.
for i in {1..20}; do
  s=$(TIMEOUT=10 dev api GET /state)
  echo $s | py "p=d['boards'].get('pending') or {}; sys.exit(0 if not p or p.get('quote') or p.get('quoteState','').startswith('ready') or p.get('kind') != 'wallet' else 1)" && break
  sleep 1.5
done
dev state
echo $s | py "sys.exit(0 if d['boards'].get('pending') else 1)" || { echo "No card waits for Confirm: nothing to press."; dev api POST /restore "{\"canvas\":\"$canvas\"}" >/dev/null; exit 1 }
if [[ -n $DRY ]]; then dev api POST /cancel >/dev/null; dev api POST /restore "{\"canvas\":\"$canvas\"}" >/dev/null; exit 0; fi

for b in $buttons; do
  # An exact title, or the first button whose title contains it («Send anyway» for «Send anyway · ≈ $3.10 lost»).
  # The chat's cards fade in one at a time after the reply: up to 10 s for it to show.
  pressed=
  for i in {1..10}; do
    pressed=$($AX press $b 2>/dev/null) && break
    pressed=; sleep 1
  done
  [[ -n $pressed ]] || { echo "no button «$b» on screen; buttons: $($AX list | tr '\n' '|')" >&2; exit 1 }
  echo $pressed
  sleep 2
  dev idle || exit 1
done
TIMEOUT=10 dev api GET /state | py "
for m in d['messages']: print('you:' if m['user'] else 'app:', m['text'])
print('outcome:', d.get('outcome'))"
dev api POST /restore "{\"canvas\":\"$canvas\"}" >/dev/null
