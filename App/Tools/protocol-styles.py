#!/usr/bin/env python3
"""The deposit tiles' colors and logos for DefiLlama's top N protocols by TVL (DepositTile, PortfolioNetworks.swift).

Bridges, chains and exchanges are left out: no deposit shows from them. Each protocol's logo goes into
Sources/IhatecryptoApp/Logos as «<key>.png» (128 px; a file already there stays). Its tile takes a dark fill and a light
accent of the logo's own color; a logo without one (black and white) takes the common dark tile. OVERRIDES holds the
colors picked by hand. The table is written to Sources/IhatecryptoApp/ProtocolStyles.swift.

Run from App/:  python3 Tools/protocol-styles.py [N=50] [--json out.json]   then rebuild (build-app.sh)."""
import colorsys, io, json, os, re, sys, urllib.parse, urllib.request

from PIL import Image

N = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 50
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
LOGOS = os.path.join(ROOT, "Sources", "IhatecryptoApp", "Logos")
SWIFT = os.path.join(ROOT, "Sources", "IhatecryptoApp", "ProtocolStyles.swift")
LEFT_OUT = {"CEX", "Chain", "Bridge", "Canonical Bridge"}
# A protocol without a color of its own: a dark grey tile with a mint accent.
COMMON = (0x1A1D24, 0x7FE3B0)
# Colors picked by hand: fill, accent. Jupiter, Kamino and Hyperliquid as the app already draws them; red brands take
# another of their colors, since red on the tile reads as a loss («+$0.21 a day» in red); logos whose color is too
# small a part of them for the count above.
OVERRIDES = {
    "jupiter": (0x131D27, 0xC7F284), "kamino": (0x082A56, 0x8FD3FF), "hyperliquid": (0x072723, 0x97FCE4),
    "lido": (0x0F2233, 0x5CC8FF), "curve": (0x2E2A10, 0xF5D33D), "babylon": (0x331B0F, 0xF59A4A),
    "maple": (0x33190F, 0xF7944F), "meth": COMMON, "kerneldao": COMMON,
    "spark": (0x2E1030, 0xF58BD8), "sky": (0x1A1033, 0xA48BFA), "etherfi": (0x18123A, 0x9C8CFF),
    "steakhousefinancial": (0x1A1033, 0xA98BF5), "grove": (0x2E2610, 0xE8C766), "venus": (0x0F2233, 0x5CC8F5),
    "stakewise": (0x16133A, 0x9D94F7),
}
# The app's own logo names for protocols it already shows.
ALIASES = {"jup": "jupiter"}


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def key(name):
    """«Kamino Finance» → «kamino», «ether.fi» → «etherfi», «Rocket Pool» → «rocketpool»."""
    name = re.sub(r"\s+(Finance|Protocol|Network|DAO|Labs|Exchange|Fi)$", "", name.strip(), flags=re.I)
    return re.sub(r"[^a-z0-9]", "", name.lower())


def luminance(rgb):
    def ch(c):
        c /= 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = rgb
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def hls(h, l, s):
    return tuple(round(c * 255) for c in colorsys.hls_to_rgb(h, l, s))


def brand(png):
    """The logo's own color, as (hue, saturation): the most-covered saturated hue; None for a black-and-white logo."""
    im = Image.open(io.BytesIO(png)).convert("RGBA").resize((64, 64))
    bins, seen = [[] for _ in range(36)], 0
    for r, g, b, a in im.get_flattened_data():
        if a < 200:
            continue
        seen += 1
        h, l, s = colorsys.rgb_to_hls(r / 255, g / 255, b / 255)
        if s > 0.3 and 0.12 < l < 0.9:
            bins[int(h * 36) % 36].append((h, l, s))
    best = max(bins, key=len)
    if seen == 0 or len(best) < seen * 0.06:
        return None
    return sum(p[0] for p in best) / len(best), sum(p[2] for p in best) / len(best)


def style(color):
    """A dark fill white text reads on, and an accent of the same hue that reads on the fill (≥ 6:1) and holds the
    fill's color as the APY pill's text."""
    if color is None:
        return COMMON
    h, s = color
    fill = hls(h, 0.13, min(s, 0.55))
    l, accent = 0.6, None
    while l < 0.92:
        accent = hls(h, l, min(max(s, 0.6), 0.9))
        if contrast(accent, fill) >= 6:
            break
        l += 0.02
    return tuple(int("%02X%02X%02X" % c, 16) for c in (fill, accent))


def top():
    lite = json.loads(get("https://api.llama.fi/lite/protocols2"))
    parents = {p["id"]: p for p in lite["parentProtocols"]}
    groups = {}
    for p in lite["protocols"]:
        if p.get("category") in LEFT_OUT or not p.get("tvl"):
            continue
        parent = parents.get(p.get("parentProtocol") or "")
        g = groups.setdefault(parent["id"] if parent else p["name"],
                              {"name": (parent or p)["name"], "logo": (parent or p).get("logo"), "tvl": 0})
        g["tvl"] += p["tvl"]
    return sorted(groups.values(), key=lambda g: -g["tvl"])[:N]


def main():
    out = []
    for g in top():
        k = key(g["name"])
        png = None
        try:
            png = get(urllib.parse.quote(re.sub(r"\?.*$", "", g["logo"]), safe=":/") + "?w=128&h=128") if g.get("logo") else None
        except Exception as e:
            print("no logo:", g["name"], e, file=sys.stderr)
        path = os.path.join(LOGOS, k + ".png")
        if png and not any(os.path.exists(os.path.join(LOGOS, k + ext)) for ext in (".png", ".svg")):
            Image.open(io.BytesIO(png)).convert("RGBA").resize((128, 128), Image.LANCZOS).save(path)
        fill, accent = OVERRIDES.get(k) or style(brand(png) if png else None)
        out.append({"key": k, "name": g["name"], "tvl": round(g["tvl"]), "fill": fill, "accent": accent})
    lines = [f'        "{p["key"]}": (0x{p["fill"]:06X}, 0x{p["accent"]:06X}),  // {p["name"]}' for p in out]
    lines += [f'        "{a}": (0x{OVERRIDES[k][0]:06X}, 0x{OVERRIDES[k][1]:06X}),' for a, k in ALIASES.items()]
    with open(SWIFT, "w") as f:
        f.write(f"""// Generated by Tools/protocol-styles.py from DefiLlama's top {N} protocols by TVL (bridges, chains and exchanges left
// out). Run it again to refresh; colors picked by hand are its OVERRIDES.

/// A deposit tile's colors by protocol: a dark fill and an accent of the logo's own color (DepositTile). Keys are the
/// protocol's short name in lower case, letters and digits only («kamino», «etherfi», «rocketpool»).
enum ProtocolStyle {{
    static let common: (fill: UInt32, accent: UInt32) = (0x{COMMON[0]:06X}, 0x{COMMON[1]:06X})
    static let colors: [String: (fill: UInt32, accent: UInt32)] = [
{chr(10).join(lines)}
    ]
}}
""")
    if "--json" in sys.argv:
        json.dump(out, open(sys.argv[sys.argv.index("--json") + 1], "w"), indent=1)
    print(f"{len(out)} protocols → {os.path.relpath(SWIFT, ROOT)}")


if __name__ == "__main__":
    main()
