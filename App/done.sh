#!/bin/zsh
# The definition of done: every agent runs it before it says «done». It stops at the first failure and says what failed.
#   1. swift build, the tests' build and swift test (passed/failed counts), then ./lint.sh
#   2. the uitest scenarios the words name, on the Dev copy's debug API (port 51790; never the installed app on 51789),
#      with a picture of the window after each scenario
#   3. a separate Claude reviews each picture against the design rules below: a fresh eye, not the author's
# Green = everything passed: .build/done/summary.json (what ran, counts, verdicts, pictures), then .build/done/stamp.
#   ./done.sh [scenario words…]   no words = no UI step (it says so); logs, pictures and reviews in .build/done/
#   ./done.sh --skip "why"        the change didn't touch what this checks: only record that
#   ./done.sh --stale             the project's Stop hook: exit 2 while Sources or Tests changed after the last stamp
cd "${0:A:h}"
OUT=.build/done STAMP=.build/done/stamp
mkdir -p $OUT
case $1 in
--stale)
  grep -qE '"stop_hook_active": ?true' && exit 0   # stdin is the hook's JSON; remind once, never loop
  [[ -f $STAMP && -z $(find Sources Tests -type f -newer $STAMP -print -quit) ]] && exit 0
  echo 'App/Sources or App/Tests changed after the last App/done.sh. Run App/done.sh <uitest scenario words> until it is green (the change needs none of it: App/done.sh --skip "why"), then say done.' >&2
  exit 2;;
--skip)
  [[ -n $2 ]] || { echo 'Say why: ./done.sh --skip "why"' >&2; exit 1 }
  echo "$(date '+%F %R')  skip: $2" >> $OUT/history; touch $STAMP; exit 0;;
esac

words=("$@")
fail() { echo "\nNOT DONE: $1" >&2; echo "$(date '+%F %R')  ${words[*]}  failed: ${1%%:*}" >> $OUT/history; exit 1 }
# One run at a time per checkout: a second one would wipe this one's pictures and drive the same Dev window at once.
zmodload zsh/system
: >> $OUT/lock   # flock doesn't create it
zsystem flock -t 0 -f lock $OUT/lock 2>/dev/null \
  || { echo "another ./done.sh runs in this checkout: waiting for it"; zsystem flock -f lock $OUT/lock || fail "the lock: $OUT/lock" }
rm -rf $OUT/*.log(N) $OUT/shots $OUT/small $OUT/review $OUT/summary.json

echo "── build"
{ swift build && swift build --build-tests } > $OUT/build.log 2>&1 \
  || { grep -m 20 'error:' $OUT/build.log || tail -20 $OUT/build.log; fail "swift build: $PWD/$OUT/build.log" }

echo "── tests"
swift test --skip-build > $OUT/test.log 2>&1; rc=$?
# Swift Testing: a «✘ Test … failed after» line per failed test, and a «Test run with N tests» line per run (several).
failed=$(grep -E '^[^ ]+ Test .* failed after' $OUT/test.log | grep -vc ' Test run with ')
total=$(awk '/ Test run with [0-9]+ tests? /{for (i = 1; i < NF; i++) if ($i == "with") { s += $(i + 1); break }} END {print s + 0}' $OUT/test.log)
passed=$(( total - failed ))
echo "$passed passed, $failed failed"
(( rc == 0 )) || { grep -E '^[^ ]+ Test .* failed after|error:' $OUT/test.log | grep -v ' Test run with ' | head -20
                   fail "swift test ($failed failed): $PWD/$OUT/test.log" }

echo "── lint"
./lint.sh > $OUT/lint.log 2>&1 || { head -30 $OUT/lint.log; fail "./lint.sh: $PWD/$OUT/lint.log" }

echo "── UI"
api() { curl -sf -m 3 -o /dev/null -H "Authorization: Bearer $(cat ~/Library/Application\ Support/ihatecrypto-dev/debug-token 2>/dev/null)" http://127.0.0.1:51790/state }
if ! api; then
  [[ -n $(git status --porcelain -- Sources/IhatecryptoApp) || ! -f $STAMP || -n $(find Sources/IhatecryptoApp -type f -newer $STAMP -print -quit) ]] \
    && fail "the window's code (Sources/IhatecryptoApp) changed and the Dev copy's debug API doesn't answer on 51790. Start the Dev copy: TEAM=<team id> ./build-app.sh (or open ~/Applications/'ihatecrypto Dev.app'), turn on Settings › Debugging › Local debug API, run again."
  ui="skipped: the Dev copy's debug API doesn't answer and Sources/IhatecryptoApp didn't change"
elif (( ! $#words )); then
  ui="skipped: no scenario words (./done.sh <words> runs the uitest scenarios whose names contain them)"
else
  # The Dev copy must run a binary built after the newest source file, and have started after that build.
  pid=$(lsof -nP -iTCP:51790 -sTCP:LISTEN -t | head -1) newest=(Sources/**/*(.om[1]))
  bin=$(ps -o comm= -p $pid) started=$(date -j -f '%a %b %d %T %Y' "$(LC_ALL=C ps -o lstart= -p $pid)" +%s 2>/dev/null)
  (( $(stat -f %m "$bin") >= $(stat -f %m $newest) && ${started:-0} >= $(stat -f %m "$bin") )) \
    || fail "the Dev copy runs a build older than Sources ($newest changed after it). Rebuild with TEAM=<team id> ./build-app.sh, quit Dev when /state shows nothing busy, open it again, run again."
  mkdir -p $OUT/shots
  env -u IHATECRYPTO_APP UITEST_SHOTS=$PWD/$OUT/shots python3 Tools/uitest.py $words | tee $OUT/uitest.log
  (( pipestatus[1] == 0 )) || fail "uitest: $PWD/$OUT/uitest.log"
  ui=$(tail -1 $OUT/uitest.log)
fi
echo "$ui"

shots=($OUT/shots/*.png(N))
if [[ $ui != skipped* ]]; then
  (( $#shots )) || fail "the UI step left no pictures: no scenario name contains «${words[*]}», all were skipped, or screencapture lacks the Screen Recording right"
  echo "── design review"
  read -r -d '' RULES <<'EOF'
You are a strict design reviewer for ihatecrypto, a macOS wallet. The picture is its main window right after a UI test sent the chat message of the scenario named above. Layout: a wallet rail on the left, a canvas with framed boards and cards, a floating chat panel. The look is monochrome with one orange accent.

Check the window against these rules:
1. Nothing sticks out: no text, button, chip, logo, badge or shadow crosses its card's edge, nothing overlaps, and important text (amounts, titles, addresses, button labels) is not cut off or ended with an ellipsis.
2. Text wraps well in every language (Russian is the longest): no lonely last word on a line of a title, pill, button or card title; button labels stay on one line; a pill or bubble hugs its text instead of stretching across.
3. Orange is rare: the one main button, a selected edge, switches and small links. Never orange text or icons on an orange fill; secondary buttons, menus and pickers are muted (grey fill, normal text). No system-blue buttons, no red buttons.
4. What the chat made is a card on the canvas, and the chat says one short line about it. The model's plain answer has no frame; a framed card in the chat only for a result, a failure or a checklist. Ways to go on are selectable cards inside the chat, under the reply that offered them, at most one marked recommended.
5. A money move is drawn as route step cards with an arrow between every pair: the state in words («Done», «Now») at the picture's top left and nothing in the top-right corner, a kind and a place with its logo («FROM · Solana», «INTO · Jupiter Lend», «BUY · ON BASE»), the amount or title, and a grey «MONEY · NOW → AFTER» (or «WAS → NOW») box with real amounts. A card shows the amount itself, never only «all <coin>».
6. One thing is selected at a time: only the selected or open item has the accent edge. The current wallet or account has a check mark, the others say «Switch», never «In use» or «Use». No bare «Any» on chips.
7. An address is shown whole (two lines are fine) with a Copy button; a copy action is an icon and a word. Buttons carry icons.
8. No card repeats what its neighbour already shows; no empty, placeholder or broken element; the newest board is on top.
9. Readable: enough contrast, labels no smaller than about 11 pt, section headers in one style (capitals, grey).

Intended, not errors: the DEV and Test tags and the orange «Debug API» badge by the zoom; the portfolio is a board on the canvas that runs past the window's bottom edge, and the floating zoom bar (100 %, Fit) sits on top of whatever is under it, so rows it covers there are not an error; a route card's picture is a coin scene whose shapes may be cut by the picture's own frame; the canvas is bigger than the window, so a board cut by the window's edge is normal; a card may wait with Confirm and Cancel inside the chat field; amounts are tiny (a test wallet); the window may be inactive, so switches and tints can look grey.

Answer: the first line is exactly «ОЦЕНКА: хорошо» or «ОЦЕНКА: плохо». «плохо» only if a person would say at first glance that the window looks sloppy or broken, or rule 1 or 3 is clearly broken. Then up to five of the most visible problems, one per line: «- [rule N] where → what is wrong → how to fix», in English. No praise, no nitpicks.
EOF
  mkdir -p $OUT/small $OUT/review
  review() {  # $1 = a picture, named «NN <scenario>»
    local r=$OUT/review/${1:t:r}
    python3 -I - "Scenario: ${${1:t:r}#?? }"$'\n\n'"$RULES" $1 > $r.jsonl <<'EOF'
import base64, json, sys
text, *images = sys.argv[1:]
content = [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64.b64encode(open(p, "rb").read()).decode()}}
           for p in images] + [{"type": "text", "text": text}]
print(json.dumps({"type": "user", "message": {"role": "user", "content": content}}))
EOF
    # Model and effort from the verify tier in ~/.claude/workflows/swarm.js.
    claude -p --input-format stream-json --output-format stream-json --verbose --model claude-sonnet-5-5 --effort medium \
      --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence < $r.jsonl 2> $r.err \
      | python3 -I -c 'import json, sys
for line in sys.stdin:
    try: m = json.loads(line)
    except ValueError: continue
    if m.get("type") == "result": print(m.get("result") or "")' > $r.txt
  }
  n=0
  for f in $shots; do
    sips -Z 1600 $f --out $OUT/small/${f:t} > /dev/null
    review $OUT/small/${f:t} &
    (( ++n % 4 )) || wait   # four reviewers at a time: each is a node process of ~200 MB
  done
  wait
  bad=0
  for f in $shots; do
    t=$OUT/review/${f:t:r}.txt
    echo "── ${f:t:r}"
    if [[ -s $t ]]; then cat $t; else echo "the reviewer didn't answer: $PWD/${t:r}.err"; fi
    grep -q 'ОЦЕНКА: хорошо' $t 2>/dev/null || bad=1
  done
  (( bad )) && fail "design review said «плохо» or didn't answer. Look at the pictures yourself ($PWD/$OUT/shots), fix, run again."
fi

python3 -I - $PWD/$OUT $passed $failed "$ui" $(git rev-parse --short HEAD) $words <<'EOF'
import glob, json, os, sys, time
out, passed, failed, ui, head, *words = sys.argv[1:]
reviews = []
for shot in sorted(glob.glob(os.path.join(out, "shots", "*.png"))):
    txt = os.path.join(out, "review", os.path.basename(shot)[:-4] + ".txt")
    reviews.append({"screenshot": shot, "verdict": "хорошо" if "ОЦЕНКА: хорошо" in open(txt).read() else "плохо", "review": txt})
json.dump({"at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "head": head, "words": words,
           "ran": ["swift build", "swift build --build-tests", "swift test --skip-build", "./lint.sh"]
                  + ([] if ui.startswith("skipped") else ["Tools/uitest.py", "design review"]),
           "tests": {"passed": int(passed), "failed": int(failed)}, "lint": "clean", "ui": ui, "reviews": reviews},
          open(os.path.join(out, "summary.json"), "w"), ensure_ascii=False, indent=2)
EOF
touch $STAMP
echo "$(date '+%F %R')  ${words[*]}  green" >> $OUT/history
echo "\nDONE: green. $PWD/$OUT/summary.json"
