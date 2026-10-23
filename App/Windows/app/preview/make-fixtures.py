"""Writes the preview's states (fixtures/*.json): what the engine would send, with demo balances on the public test
addresses (Hardhat's #0, «abandon … about»), for drawing the window without the engine. Run: python3 make-fixtures.py"""
import json, math, os, datetime as dt

now = dt.datetime(2026, 10, 23, 13, 40, tzinfo=dt.timezone.utc)
iso = lambda d: d.strftime("%Y-%m-%dT%H:%M:%SZ")
SOL = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
EVM = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
AUTO_EVM = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"

def row(symbol, amount, usd, ch=None, pnl=None, gain=None, verified=None, section="tokens", note=None, icon=None):
    return {"symbol": symbol, "amount": amount, "value": float(amount.replace(",", "")), "usd": usd, "icon": icon,
            "pnl": pnl, "gain": gain, "change24h": ch, "verified": verified, "section": section, "note": note}

networks = [
    {"key": "solana", "name": "Solana", "usd": 787.11, "note": "idle $741.30 · earning $2.51", "rows": [
        row("USDC", "741.30", 741.30, verified=True),
        row("SPCX", "0.1201", 18.25, -3.7, "−$1.74 (−8.7%)", -1.74, True),
        row("HIGGS", "41,250.75", 3.86, -23.9, "−$125.10 (−97.4%)", -125.10, False),
        row("SOL", "0.0215", 2.38, 0.6, "−$0.39 (−14.2%)", -0.39, True),
        row("RAY", "1.0412", 1.98, -0.7, "−$0.02 (−0.9%)", -0.02, True),
        row("STONK", "4.0518", 0.53, 11.1, "−$0.48 (−47.5%)", -0.48, True),
        row("Kamino", "2.51", 2.51, section="defi", note="Allez USDC · 5.42%"),
    ]},
    {"key": "hyperliquid", "name": "Hyperliquid", "usd": 301.45, "note": "$0.01 · margin", "rows": [
        row("HYPE", "3.7124", 298.48, 0.4, "+$1.49 (+0.5%)", 1.49, True),
        row("USDC", "2.871", 2.96, verified=True),
    ]},
    {"key": "robinhood", "name": "Robinhood", "usd": 359.83, "note": "idle $0.89", "rows": [
        row("AI", "4,012.5", 326.91, 1.2),
        row("SPCX", "0.1287", 19.69, -3.9),
        row("TSLA", "0.0139", 5.33, 3.1),
        row("MOO", "1,050.2", 2.54, -10.1),
        row("ETH", "0.00095", 2.37, -0.3, "−$0.27 (−10.3%)", -0.27),
        row("SHROOM", "548.1", 2.10, -18.4),
        row("USDG", "0.89", 0.89),
    ]},
    {"key": "near", "name": "NEAR", "usd": 1196.30, "note": None, "rows": [row("NEAR", "455.6", 1196.30, 2.4)]},
    {"key": "base", "name": "Base", "usd": 563.40, "note": "idle $0.92", "rows": [row("ETH", "0.2044", 562.48, -0.3), row("USDC", "0.9017", 0.92)]},
    {"key": "arbitrum", "name": "Arbitrum", "usd": 99.65, "note": None, "rows": [row("USDT", "2.39", 2.46), row("ARB", "301.2", 97.19, -1.2)]},
    {"key": "tempo", "name": "Tempo", "usd": 0.12, "note": None, "rows": [row("USDC", "0.12", 0.12)]},
]
total = round(sum(n["usd"] for n in networks), 2)
equity = []
for i in range(0, 7 * 24 * 6, 6):  # a point an hour for a week
    d = now - dt.timedelta(minutes=10 * (7 * 24 * 6 - i))
    x = i / (7 * 24 * 6)
    usd = total + 318.4 * (1 - x) + 60 * math.sin(x * 19) * (1 - x) + 25 * math.sin(x * 61)
    equity.append({"at": iso(d), "usd": round(usd, 2)})
equity.append({"at": iso(now), "usd": total})

main_wallet = {"id": "w-main", "label": "Main", "kind": "phrase", "solana": SOL, "evm": EVM}
auto_wallet = {"id": "w-auto", "label": "Automation", "kind": "phrase", "solana": None, "evm": AUTO_EVM, "automation": True}

move = {"id": "b-move", "kind": "move", "title": "Send 150 USDC to HL wallet", "state": "done", "at": iso(now - dt.timedelta(hours=15)),
        "lines": [],
        "steps": [
            {"label": "FROM · Hyperliquid", "title": "150 USDC", "lines": ["146.04 USDC short: it comes from Solana first", "Fee 1 USDC: Hyperliquid's once for opening the account"],
             "money": {"place": "Solana", "symbol": "USDC", "was": "890.4", "after": "744.36"}},
            {"label": "TO · Saved Address", "title": "HL wallet", "lines": ["0x3c44…93BC", "Saved Oct 22 with Windows Hello"]},
        ]}
pills = [
    {"id": "b-p1", "kind": "move", "title": "Send 150 USDC to HL wallet", "state": "failed", "at": iso(now - dt.timedelta(hours=17)), "lines": [], "steps": []},
    {"id": "b-p2", "kind": "move", "title": "Save “HL wallet” to Addresses", "state": "done", "at": iso(now - dt.timedelta(hours=17, minutes=1)), "lines": [], "steps": []},
]
waiting_move = {"id": "b-wait", "kind": "move", "title": "Buy SOL for 10 USDC", "state": "waiting", "at": iso(now), "lines": ["Jupiter · price impact 0.01% · fee $0.00"],
    "steps": [
        {"label": "BUY · Solana", "title": "≈ 0.0548 SOL", "lines": ["Jupiter route: USDC → SOL", "Slippage 0.5%"], "money": {"place": "Solana", "symbol": "USDC", "was": "741.30", "after": "747.59"}},
    ]}
chart = {"id": "b-chart", "kind": "chart", "title": "SOL · week", "at": iso(now), "lines": [],
         "chart": {"title": "SOL", "closes": [182 + 9 * math.sin(i / 4) + i * 0.3 for i in range(48)], "low": "$176.20", "high": "$199.80", "change": "+4.2%"}}

chat = [
    {"id": "m1", "fromUser": True, "text": "отправь 150 USDC на HL wallet", "at": iso(now - dt.timedelta(hours=15, minutes=3))},
    {"id": "m2", "fromUser": False, "text": "Sent 150 USDC to HL wallet on Hyperliquid, plus 1 USDC Hyperliquid kept once for opening their account.", "note": "transfer", "at": iso(now - dt.timedelta(hours=15))},
    {"id": "m3", "fromUser": True, "text": "отправь газ на хайпер " + EVM, "at": iso(now - dt.timedelta(hours=14, minutes=59))},
    {"id": "m4", "fromUser": False, "text": "Торговому счёту Hyperliquid газ не нужен: переводы и сделки там идут без него, и 150 USDC на HL wallet уже дошли. Газ в HyperEVM (сеть 999) платится в HYPE. Могу докинуть на HL wallet ещё USDC: от 10 USDC на нём можно купить HYPE на споте. Что тебе нужно?",
     "note": "claude-opus-5-5", "at": iso(now - dt.timedelta(hours=14, minutes=58))},
]
options = [{"title": "Send 10 USDC to HL wallet", "comment": "Enough for a spot HYPE buy", "recommended": True},
           {"title": "Nothing for now", "comment": "", "recommended": False}]

providers = [
    {"id": "claude-code", "name": "Claude (subscription via Claude Code)", "kind": "subscription", "takesKey": False, "existingLogin": "Claude Code", "signIn": None, "note": "", "unavailable": None, "connected": True},
    {"id": "chatgpt", "name": "ChatGPT (subscription)", "kind": "subscription", "takesKey": False, "existingLogin": "Codex", "signIn": "chatgpt", "note": "", "unavailable": None, "connected": True},
    {"id": "anthropic", "name": "Anthropic API", "kind": "apiKey", "takesKey": True, "existingLogin": None, "signIn": None, "note": "", "unavailable": None, "connected": False},
    {"id": "openrouter", "name": "OpenRouter", "kind": "apiKey", "takesKey": True, "existingLogin": None, "signIn": None, "note": "", "unavailable": None, "connected": True},
    {"id": "openai", "name": "OpenAI API", "kind": "apiKey", "takesKey": True, "existingLogin": None, "signIn": None, "note": "", "unavailable": None, "connected": False},
    {"id": "ollama", "name": "Ollama on this PC", "kind": "local", "takesKey": False, "existingLogin": None, "signIn": None, "note": "", "unavailable": None, "connected": True},
]

def state(**over):
    s = {
        "signer": {"ready": True, "error": None, "notes": ["Keys under Windows Hello with this PC's TPM", "Signer checks passed"], "waiting": False, "sessionLength": 300, "ledger": None},
        "wallets": [main_wallet, auto_wallet], "active": "w-main",
        "addresses": {"solana": SOL, "evm": EVM, "hyperliquid": EVM, "near": "ihatecrypto-demo.near"},
        "phrase": None,
        "portfolio": {"networks": networks, "total": total, "updatedAt": iso(now), "errors": [], "equity": equity, "change7d": round(total - equity[0]["usd"], 2)},
        "positions": [], "positionsPnl": None, "automationUSD": 4.24,
        "chat": chat, "thinking": False, "draft": None, "card": None, "options": options, "running": None, "plan": None,
        "model": {"provider": "claude-code", "name": "Claude (subscription via Claude Code)", "model": "claude-opus-5-5", "connected": True, "models": ["claude-opus-5-5", "claude-sonnet-5-5"]},
        "providers": providers, "secondCheck": True, "jevOpenRouter": False, "language": "auto", "russian": False,
        "book": [{"name": "HL wallet", "network": "hyperliquid", "address": "0x3c44CdDdB6a900fa2b585dd299e03d12FA4293BC", "memo": ""},
                 {"name": "Mom", "network": "base", "address": "0x90F79bf6EB2c4f870365E785982E1f101E93b906", "memo": ""}],
        "history": [{"at": iso(now - dt.timedelta(hours=15)), "chain": "hyperliquid", "kind": "transfer", "note": "Sent 150 USDC to HL wallet", "link": None},
                    {"at": iso(now - dt.timedelta(days=1)), "chain": "solana", "kind": "swap", "note": "Bought SOL for 10 USDC", "link": "https://solscan.io/tx/demo"}],
        "alerts": [{"id": "a1", "text": "BTC above $120,000"}],
        "tasks": [{"id": "t1", "text": "Every day at 9:00: buy SOL for 5 USDC", "paused": True, "automatic": True, "next": iso(now + dt.timedelta(hours=19))},
                  {"id": "t2", "text": "Every Monday: check the yields", "paused": True, "automatic": False, "next": iso(now + dt.timedelta(days=3))}],
        "automation": auto_wallet,
        "skills": [{"id": "trading-study", "name": "Trading study", "about": "Keeps a journal of your trades and studies them with you", "text": "", "builtIn": True, "on": True}],
        "memory": ["The user's HL wallet is their Hyperliquid account"],
        "favorites": [{"chain": "solana", "address": "DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump", "name": "Higgs", "symbol": "HIGGS", "image": None, "price": 0.0000866, "change24h": -23.9, "liquidity": 51000}],
        "canvases": [{"id": "c1", "title": "Withdraw · 23 USDC to Solana", "at": iso(now - dt.timedelta(hours=1))},
                     {"id": "c2", "title": "Send 150 USDC to HL wallet", "at": iso(now - dt.timedelta(hours=15))},
                     {"id": "c3", "title": "Gas · $1.00 of BNB on BNB Chain", "at": iso(now - dt.timedelta(days=2))}],
        "canvas": "c2",
        "boards": [move] + pills, "runningBoard": None,
        "multisigs": [], "multisigDraft": None, "candidates": [], "ledgerConnect": None, "starred": [],
        "watching": False, "watched": {},
        "context": {"used": 21380, "window": 1000000},
    }
    s.update(over)
    return s

os.makedirs("fixtures", exist_ok=True)
fixtures = {
    "main": state(),
    "waiting": state(boards=[waiting_move, move] + pills, card={"id": "b-wait", "title": "Buy SOL for 10 USDC", "lines": ["≈ 0.0548 SOL"], "warnings": [], "confirm": "Confirm"},
                     options=[], chat=chat + [{"id": "m5", "fromUser": True, "text": "buy SOL for 10 USDC", "at": iso(now)}]),
    "thinking": state(thinking=True, draft=None, options=[], chat=chat + [{"id": "m5", "fromUser": True, "text": "chart SOL week", "at": iso(now)}], boards=[chart, move] + pills),
    "empty": state(boards=[], chat=[], options=[], canvases=[{"id": "c9", "title": "Canvas", "at": iso(now)}], canvas="c9"),
    "russian": state(russian=True),
    "welcome": state(wallets=[], active=None, addresses={}, portfolio={"networks": [], "total": 0, "updatedAt": None, "errors": [], "equity": [], "change7d": None}, boards=[], chat=[], options=[]),
    "phrase": state(phrase="legal winner thank year wave sausage worth useful legal winner thank yellow".split()),
}
for name, s in fixtures.items():
    with open(f"fixtures/{name}.json", "w") as f:
        json.dump(s, f, ensure_ascii=False, indent=1)
print(", ".join(fixtures))
