#!/usr/bin/env python3
"""Runs phrases through `LLMCheck <provider> --route` in parallel and checks each answer against its expectation.

    swift build --product LLMCheck && rm -f LLMCheck && cp ../../.build/debug/LLMCheck .   # a copy: a rebuild mid-run breaks the original
    python3 gen_phrases.py 2400 4 > cases.jsonl      # and/or: python3 followups.py > followups.jsonl
    python3 run_route.py cases.jsonl results.jsonl --provider claude-code --workers 8 --batch 6
    python3 summary.py results.jsonl
Resumable: cases already answered in results.jsonl are skipped; errors (a timeout, Claude Code reinstalling itself) run again.
cp over the old copy gets it killed by macOS (exit 137): remove it first. ~0.8 phrases/s with 8 workers; a Claude call ≈ 9 s."""
import argparse, json, os, re, subprocess, sys, threading, time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
APP = os.path.abspath(os.path.join(HERE, "..", ".."))
MARKETS = open(os.path.join(HERE, "markets-main.txt")).read().strip() + "," + open(os.path.join(HERE, "markets-xyz.txt")).read().strip()


def norm(c):
    c = (c or "").lower().replace("“", '"').replace("”", '"').replace("’", "'")
    c = re.sub(r"\s+", " ", c).strip()
    return c[:-1] if c.endswith(".") else c


def judge(case, out):
    return judge_route(case, out) + judge_agent(case["expect"], out)


def judge_agent(e, out):
    """The agent's side: what it looked up, whether it goes on after the command and says what's next, whether a check
    stays quiet, what the reply says, what the cards must not offer, a number in the command."""
    why, looks = [], out.get("lookups") or []
    if "lookups_any" in e and not any(re.search(r, l, re.I) for r in e["lookups_any"] for l in looks):
        why.append(f"lookups {looks}, expected one matching {e['lookups_any']}")
    for key in ("continue", "quiet"):
        if key in e and bool(out.get(key)) != e[key]:
            why.append(f"{key} = {bool(out.get(key))}, expected {e[key]}")
    if "next" in e and bool(out.get("next")) != e["next"]:
        why.append(f"next = {out.get('next')!r}, expected {'one' if e['next'] else 'none'}")
    if "reply_re" in e and not any(re.search(r, out.get("reply") or "", re.I) for r in e["reply_re"]):
        why.append(f"reply {(out.get('reply') or '')[:140]!r} matches none of {e['reply_re']}")
    texts = [norm(o) for o in out.get("options") or []] + [norm(out.get("command"))]
    for r in e.get("options_not_re", []):
        hit = next((t for t in texts if t and re.search(r, t)), None)
        if hit:
            why.append(f"offers {hit!r}, which matches {r}")
    if "number" in e:
        n = e["number"]
        m = re.search(n["re"], norm(out.get("command")))
        if not m or abs(float(m.group(1)) - n["value"]) > n["tol"]:
            why.append(f"number in {out.get('command')!r} isn't {n['value']} ± {n['tol']}")
    return why


def judge_route(case, out):
    e, why = case["expect"], []
    route = out.get("route")
    want = e.get("route")
    if want == "any":
        hits = [norm(out.get("command")), norm(out.get("show"))]
        if not any(re.search(r, h) for r in e["cmd_or_show"] for h in hits if h):
            why.append(f"neither command nor show matches {e['cmd_or_show']}: {out.get('command')!r} / {out.get('show')!r}")
        return why
    if isinstance(want, list):
        if route not in want:
            why.append(f"route {route}, expected one of {want}")
        return why
    if route != want:
        why.append(f"route {route}, expected {want}" + (f": {out.get('reply', '')[:120]!r}" if route in ("chat", "clarify") else ""))
        return why
    if want == "command":
        c = norm(out.get("command"))
        if not out.get("parses"):
            why.append("doesn't parse")
        if "cmd" in e and not any(re.search(r, c) for r in e["cmd"]):
            why.append(f"command {c!r} matches none of {e['cmd']}")
        for k, v in (e.get("intent") or {}).items():
            got = (out.get("intent") or {}).get(k)
            if isinstance(v, (int, float)) and not isinstance(v, bool):
                if got is None or abs(float(got) - float(v)) > 1e-9:
                    why.append(f"intent.{k} = {got}, expected {v}")
            elif got != v:
                why.append(f"intent.{k} = {got}, expected {v}")
        if out.get("doubt"):
            why.append(f"second reader doubts: {out['doubt'][:120]}")
    return why


def run_batch(batch, provider, env_extra):
    # The folders the phrases name, as the app knows the user's: a typo of one lands there (FolderCommand.existing).
    env = dict(os.environ, IHATECRYPTO_ROUTE_JSON="1", IHATECRYPTO_MARKETS=MARKETS, IHATECRYPTO_ROUTE_FOLDERS="Research,Ресерч,Ideas,Идеи", **env_extra)
    # LLMCHECK: another copy of the binary (night.sh runs its own, so a rebuild here doesn't touch it).
    cmd = [os.environ.get("LLMCHECK") or os.path.join(HERE, "LLMCheck"), provider, "--route"] + [c["message"] for c in batch]
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=120 * len(batch))
        lines = p.stdout.splitlines()
    except subprocess.TimeoutExpired as t:
        lines = (t.stdout or b"").decode(errors="replace").splitlines() if isinstance(t.stdout, bytes) else (t.stdout or "").splitlines()
    outs = []
    for line in lines:
        line = line.strip()
        if line.startswith("{"):
            try:
                outs.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        elif "→ error:" in line:
            outs.append({"route": "error", "error": line[-300:], "message": line.split("» →")[0].lstrip("«")})
    results = []
    for c in batch:
        o = next((x for x in outs if x.get("message") == c["message"]), None)
        if o is None:
            o = {"route": "error", "error": "no output"}
        why = judge(c, o) if o.get("route") != "error" else ["error: " + o.get("error", "")[:200]]
        results.append({**c, "out": o, "ok": not why, "why": why})
    return results


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cases"); ap.add_argument("results")
    ap.add_argument("--provider", default="claude-code"); ap.add_argument("--workers", type=int, default=6); ap.add_argument("--batch", type=int, default=8)
    ap.add_argument("--limit", type=int, default=0)
    a = ap.parse_args()
    cases = [json.loads(l) for l in open(a.cases)]
    done = set()
    if os.path.exists(a.results):
        done = {r["id"] for r in map(json.loads, filter(str.strip, open(a.results))) if r["out"].get("route") != "error"}
    todo = [c for c in cases if c["id"] not in done]
    if a.limit:
        todo = todo[: a.limit]
    # One LLMCheck per batch, so a batch shares its chat, canvas and plan («env»).
    groups = {}
    for c in todo:
        groups.setdefault(json.dumps(c.get("env", {}), sort_keys=True), []).append(c)
    batches = [g[i:i + a.batch] for g in groups.values() for i in range(0, len(g), a.batch)]
    lock, started, n = threading.Lock(), time.time(), [0, 0]
    with open(a.results, "a") as f, ThreadPoolExecutor(a.workers) as pool:
        for res in pool.map(lambda b: run_batch(b, a.provider, b[0].get("env", {})), batches):
            with lock:
                for r in res:
                    f.write(json.dumps(r, ensure_ascii=False) + "\n")
                    n[0] += 1; n[1] += r["ok"]
                    if not r["ok"]:
                        print(f"✗ [{r['lang']} {r['kind']}] «{r['message']}» → {'; '.join(r['why'])}", flush=True)
                f.flush()
                print(f"… {n[0]}/{len(todo)} done, {n[1]} ok, {time.time() - started:.0f} s", flush=True)
    print(f"done: {n[1]}/{n[0]} ok")


main()
