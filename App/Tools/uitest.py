#!/usr/bin/env python3
"""ihatecrypto UI scenarios against the running app, through its local debug API (Settings → Debugging).

    python3 App/Tools/uitest.py                 # every scenario in uitest-scenarios.json
    python3 App/Tools/uitest.py deposit chart   # only those whose name contains a word
    IHATECRYPTO_APP=work python3 App/Tools/uitest.py   # against the working app, not «ihatecrypto Dev»
    UITEST_SHOTS=<folder> python3 App/Tools/uitest.py  # also a picture of the window after each scenario (done.sh)

A scenario is one or more phrases and what the canvas and the reply must show after the last ("send" + "expect"), or a
conversation checked after every message ("turns": [{"send": "…", "expect": {…}}, …]): a card that waits while it's
asked about, then changed («добавь ещё 300»), then asked about again.

Each scenario runs on a fresh test canvas (/reset), sends its phrases as chat messages, checks the reply and the
canvas boards, and drops what waits for Confirm (/cancel). At the end the test canvases are deleted and the canvas
that was open is reopened (/restore). Nothing can sign through the debug API, so no money moves. It refuses to start
while a move waits for Confirm or runs on the user's own canvas. Exit code 1 when any scenario fails.
"""
import re
import json
import os
import subprocess
import sys
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
# The development copy («ihatecrypto Dev», build-app.sh) by default: tests never open canvases in the working app.
# IHATECRYPTO_APP=work runs them against the working ihatecrypto.app (a release's smoke test).
WORK = os.environ.get("IHATECRYPTO_APP") == "work"
TOKEN = open(os.path.expanduser("~/Library/Application Support/ihatecrypto" + ("" if WORK else "-dev") + "/debug-token")).read().strip()
BASE = "http://127.0.0.1:" + ("51789" if WORK else "51790")
SHOTS = os.environ.get("UITEST_SHOTS")


def call(method, path, body=None, timeout=240):
    data = json.dumps(body).encode() if body is not None else (b"" if method == "POST" else None)
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={"Authorization": "Bearer " + TOKEN, "Content-Type": "application/json"})
    for _ in range(40):  # the app is busy while the model answers: wait, don't fail
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code != 409:
                raise
            time.sleep(1.5)
    raise RuntimeError(f"{path}: still busy")


def check(expect, state, replies):
    """The reasons a scenario failed; empty when it passed."""
    b, why = state["boards"], []
    pending = b.get("pending")
    def text(x):
        return json.dumps(x, ensure_ascii=False).lower() if x is not None else ""
    for key, want in expect.items():
        if key == "pending_kind" and (pending or {}).get("kind") != want:
            why.append(f"pending {text(pending) or 'none'}, expected a {want} card")
        elif key == "drawn_route" and (pending or {}).get("drawnRoute") != want:
            why.append("the card has no drawn route (fallback card)")
        elif key == "steps":
            labels = [s["label"].upper() for s in (pending or {}).get("steps", [])]
            if not all(any(w in l for l in labels) for w in want):
                why.append(f"steps {labels}, expected {want}")
        elif key == "pending_name" and (pending or {}).get("name") != want:
            why.append(f"the card saves «{(pending or {}).get('name')}», expected «{want}»")
        elif key == "title_contains" and want.lower() not in text((pending or {}).get("title")):
            why.append(f"title {text((pending or {}).get('title'))}, expected «{want}»")
        elif key.startswith("command_contains") and want.lower() not in text((pending or {}).get("command")):
            why.append(f"command {text((pending or {}).get('command'))}, expected «{want}»")
        elif key == "no_pending" and pending:
            why.append(f"a card waits: {text(pending)[:160]}")
        elif key == "plan_first" and not ((b.get("plan") or {}).get("steps") or [{}])[0].get("label", "").upper().startswith(want):
            why.append(f"the plan's first card is {text(((b.get('plan') or {}).get('steps') or [None])[0])}, expected «{want}…»")
        elif key == "plan" and (b.get("plan") or {}).get("asset") != want:
            why.append(f"plan {text(b.get('plan')) or 'none'}, expected a plan on {want}")
        elif key == "chart" and (b.get("chart") or {}).get("coin") != want:
            why.append(f"chart {text(b.get('chart'))}, expected {want}")
        elif key == "chart_range" and (b.get("chart") or {}).get("range") != want:
            why.append(f"chart range {text(b.get('chart'))}, expected {want}")
        elif key == "chart_place" and (b.get("chart") or {}).get("place") != want:
            why.append(f"the chart is {(b.get('chart') or {}).get('place')} on the canvas, expected {want}")
        elif key == "yields" and not b.get("yields"):
            why.append("no rate bubbles")
        elif key == "pnl_period" and (b.get("pnl") or {}).get("period") != want:
            why.append(f"P/L board {text(b.get('pnl'))}, expected period {want}")
        elif key == "note" and want.lower() not in text((b.get("note") or {}).get("text")):
            why.append(f"note card {text(b.get('note')) or 'none'}, expected «{want}»")
        elif key == "reply" and not replies:
            why.append("no reply")
        elif key == "reply_contains" and not any(want.lower() in r.lower() for r in replies):
            why.append(f"no reply with «{want}»: {replies[-3:]}")
        elif key == "reply_cyrillic" and bool(re.search("[а-яё]", (replies or [""])[-1].lower())) != want:
            why.append(f"the reply is {'not ' if want else ''}in Russian: {replies[-1:]}")
        elif key == "no_boards":
            present = [k for k in want if b.get(k)]
            if present:
                why.append(f"unexpected boards: {present}")
        elif key == "detail_contains" and want.lower() not in (b.get("detail") or "").lower():
            why.append(f"canvas block {b.get('detail')}, expected «{want}»")
        elif key == "cards_in_chat" and len(next((m.get("cards") or [] for m in reversed(state.get("messages") or []) if not m["user"]), [])) < want:
            why.append(f"the last reply has fewer than {want} cards under it in the chat")
        elif key == "next_steps_at_least" and len(b.get("nextSteps") or []) < want:
            why.append(f"{len(b.get('nextSteps') or [])} cards to go on, expected at least {want}: {text(b.get('nextSteps'))[:200]}")
        elif key == "not_executing" and state.get("executing"):
            why.append("a move is executing")
        elif key == "allocation_free" and abs(float((b.get("allocation") or {}).get("free") or 0) - want) > 0.01:
            why.append(f"spread of {(b.get('allocation') or {}).get('free')}, expected {want}")
        elif key == "allocation_waits" and bool((b.get("allocation") or {}).get("waits")) != want:
            why.append(f"the spread {'no longer waits' if want else 'still waits'} for Confirm")
        elif key == "allocation_skips" and not any(want.lower() in (x["place"] + " " + x["reason"]).lower() for x in (b.get("allocation") or {}).get("skipped", [])):
            why.append(f"skipped {text((b.get('allocation') or {}).get('skipped'))}, expected «{want}»")
        elif key == "reply_lacks" and any(want.lower() in r.lower() for r in replies[-1:]):
            why.append(f"the reply says «{want}»: {replies[-1:]}")
    return why


def main():
    scenarios = json.load(open(os.path.join(HERE, "uitest-scenarios.json")))
    only = [a.lower() for a in sys.argv[1:]]
    if only:
        scenarios = [s for s in scenarios if any(o in s["name"].lower() for o in only)]
    start = call("GET", "/state")
    if start["boards"].get("pending") or start["executing"] or start["boards"].get("running"):
        sys.exit("A move waits for Confirm or runs on the open canvas: not touching it. Run the tests later.")
    back = start.get("canvasID")
    print(f"ihatecrypto · {start['provider']} · {start['model']} · {len(scenarios)} scenarios\n")
    failed = skipped = 0
    try:
        for n, s in enumerate(scenarios, 1):
            cond = s.get("only_if")  # a scenario that depends on what the wallet holds, on which wallet signs, or on Jev
            if cond and "second_check" in cond and cond["second_check"] != bool(start.get("secondCheck")):
                skipped += 1
                print(f"SKIP  {s['name']}  (the second check is {'off' if cond['second_check'] else 'on'})")
                continue
            if cond and "wallet" in cond:
                if (cond["wallet"] == "test") != bool(start["wallet"].get("test")):
                    skipped += 1
                    print(f"SKIP  {s['name']}  (needs the {cond['wallet']} wallet)")
                    continue
            elif cond and "balance" in cond:
                have = float(start["balances"].get(cond["balance"]) or 0)
                if ("at_least" in cond and have < cond["at_least"]) or ("below" in cond and have >= cond["below"]):
                    skipped += 1
                    print(f"SKIP  {s['name']}  ({cond['balance']} = {have})")
                    continue
            t0 = time.time()
            call("POST", "/reset")
            why = []
            for k, turn in enumerate(s.get("turns") or [{"send": s["send"], "expect": s["expect"]}]):
                replies, state = [], None
                for phrase in [turn["send"]] if isinstance(turn["send"], str) else turn["send"]:
                    state = call("POST", "/send", {"text": phrase})
                    replies += [m["text"] for m in state["messages"] if not m["user"]]
                why += [(f"after «{turn['send']}»: " if "turns" in s else "") + w for w in check(turn["expect"], state, replies)]
            if SHOTS and state.get("windowID"):
                time.sleep(2)  # route cards fade in one at a time
                # /screenshot comes out blank (glass chrome): screencapture needs the terminal's Screen Recording right.
                shot = os.path.join(SHOTS, f"{n:02d} {re.sub(r'[/:]', '-', s['name'])[:80]}.png")
                subprocess.run(["screencapture", "-x", "-o", "-l", str(state["windowID"]), shot])
            if state["boards"].get("pending"):
                call("POST", "/cancel")
            ok = not why
            failed += not ok
            print(f"{'PASS' if ok else 'FAIL'}  {s['name']}  ({time.time() - t0:.0f} s)")
            for w in why:
                print("      · " + w)
            if not ok and replies:
                print("      ihatecrypto: " + replies[-1][:200])
    finally:
        call("POST", "/restore", {"canvas": back} if back else {})
    print(f"\n{len(scenarios) - failed - skipped}/{len(scenarios) - skipped} passed" + (f", {skipped} skipped" if skipped else ""))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
