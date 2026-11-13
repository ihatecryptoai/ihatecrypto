#!/usr/bin/env python3
"""perf.py — the rendering A/B on the Metal copy («ihatecrypto Metal», debug API on 51791).

Each scenario runs twice in turn, as before the metal branch ({"classic": true}) and as now, on the route canvas:
frames from the app's FrameMeter (/measure: fps, dropped refreshes, p99 and longest gap, main-thread busy %) and the
GPU time WindowServer and the app spent meanwhile (the AGX user clients' accumulatedGPUTime, ms per second). The
system-wide «Device Utilization %» isn't used: Notification Center alone once read 60 % of it.

    python3 App/Tools/perf.py [rounds] [scenario …]

Scenarios: pan pinch zoom chat idle thinking newcanvas, and thinking-late / newcanvas-late (GPU from 5.5 s of a 9 s
run: after the face rests and the edge stops breathing). The window comes to the front for each run, without the
keyboard, and goes back behind after. Settings › Debugging › Local debug API must be on in the Metal copy.
"""
import json, os, re, subprocess, sys, threading, time, urllib.request
from collections import Counter

PORT = 51791
# In the App Sandbox's container, or outside it for a copy from before the sandbox.
TOKEN = open(next(p for p in map(os.path.expanduser, ['~/Library/Containers/app.ihatecrypto.mac.metal/Data/Library/Application Support/ihatecrypto-metal/debug-token',
                                                     '~/Library/Application Support/ihatecrypto-metal/debug-token']) if os.path.exists(p))).read().strip()
CANVAS = '5A7FD128-98EF-4BB8-BB68-C6260F1C7ADA'  # «Spread USDC across the best rates»: a route of 12 cards


def api(path, body=None):
    req = urllib.request.Request(f'http://127.0.0.1:{PORT}/{path}', data=json.dumps(body or {}).encode(),
                                 headers={'Authorization': f'Bearer {TOKEN}'}, method='POST')
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def gpu_ns():
    out = subprocess.run(['ioreg', '-r', '-c', 'AGXDeviceUserClient', '-w', '0', '-l'], capture_output=True, text=True).stdout
    t = Counter()
    for block in out.split('+-o ')[1:]:
        m = re.search(r'"IOUserClientCreator" = "pid (\d+),', block)
        if m:
            t[m.group(1)] += sum(int(g) for g in re.findall(r'"accumulatedGPUTime"=(\d+)', block))
    return t


def pid(name):
    return subprocess.run(['pgrep', '-x', name], capture_output=True, text=True).stdout.split()[0]


def run(what, classic, ws, app):
    late = what.endswith('-late')
    scenario, seconds, delay = what.removesuffix('-late'), (9 if late else 4), (5.5 if late else 0.6)
    api('zoom', {'fit': True})
    time.sleep(0.8 + (4 if scenario == 'thinking' else 0))
    result = {}
    t = threading.Thread(target=lambda: result.update(json.loads(api('measure', {'what': scenario, 'seconds': seconds, 'classic': classic}))))
    t.start()
    time.sleep(delay)
    a = gpu_ns(); time.sleep(3); b = gpu_ns()
    t.join()
    ms = lambda p: (b[p] - a.get(p, 0)) / 3e6
    r = result
    print(f"{what:15} {'old' if classic else 'new'}  WindowServer {ms(ws):6.0f}  app {ms(app):4.0f} ms GPU/s | "
          f"fps {r['fps']:5.1f} dropped {r['dropped']:3d} p99 {r['p99ms']:4.1f} max {r['maxms']:5.1f} ms main {r['mainBusy']:4.1f} %"
          + ('' if r.get('visible') else '  (window not visible)'), flush=True)


if __name__ == '__main__':
    rounds = int(sys.argv[1]) if len(sys.argv) > 1 else 2
    scenarios = sys.argv[2:] or ['idle', 'pan', 'pinch', 'zoom', 'chat', 'thinking', 'thinking-late', 'newcanvas-late']
    ws, app = pid('WindowServer'), pid('ihatecrypto Metal')
    api('restore', {'canvas': CANVAS}); time.sleep(3)
    for _ in range(rounds):
        for what in scenarios:
            for classic in (True, False):
                run(what, classic, ws, app)
    api('zoom', {'fit': True})
