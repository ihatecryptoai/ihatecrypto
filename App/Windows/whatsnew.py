"""Writes app/ui/whatsnew-data.js from the Mac's WhatsNew/whatsnew.json, its Mac words said for a PC (Touch ID → Windows
Hello, the menu bar → the tray, ⌘ → Ctrl). Run from App/Windows at release: python3 whatsnew.py"""
import json, os

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, "..", "Sources", "IhatecryptoApp", "WhatsNew", "whatsnew.json")
out = os.path.join(here, "app", "ui", "whatsnew-data.js")
SWAPS = [("Touch ID", "Windows Hello"), ("this Mac", "this PC"), ("the Mac's", "the PC's"), ("your Mac", "your PC"),
         ("when the Mac sleeps", "when the PC sleeps"), ("On a Mac without", "On a PC without"), ("so the Mac does", "so the PC does"),
         ("the menu bar", "the tray"), ("menu bar", "tray"), ("⌥⌘Q", "Ctrl+Shift+Q"), ("⌘Q", "Ctrl+Q"), ("⌘L", "Ctrl+L"),
         ("⌘,", "Ctrl+,"), ("⌘", "Ctrl+")]

# Whole phrases that are only the Mac's, said as the PC does it (after the swaps above): a PC has no «PC password» way in
# and no Keychain; only the Mac's release is rebuilt and compared byte for byte (verify-release.yml); the PC's updater
# looks every six hours and waits in Settings › About (system.rs), with no Sparkle window; its mascot is drawn, not a film.
PHRASES = [(", Windows Hello or your PC password.", " or Windows Hello."),
           ("On a PC without Windows Hello, prompts say «your PC password».", "Prompts name Windows Hello, the way this PC asks."),
           ("and the key stays in this PC's Keychain.", "and its key is kept encrypted for your Windows user."),
           ("Open source, rebuilt byte for byte", "Open source, the Mac app rebuilt byte for byte"),
           ("and every release rebuilds from its tag byte for byte apart from the signatures, so anyone can check the app is that code.",
            "and every Mac release rebuilds from its tag byte for byte apart from the signatures, so anyone can check that app is that code. "
            "The Windows installer isn't rebuilt and compared that way."),
           ("Every release also rebuilds byte for byte on GitHub's Mac now, not only on ours.",
            "Every Mac release also rebuilds byte for byte on GitHub's Mac now, not only on ours."),
           ("A new version shows a small window — Install Update, Remind Me Later or Skip This Version.",
            "A new version shows a notification, and Settings › About has Install and Later."),
           ("This copy looks at updates.ihatecrypto.ai once a day. A new version downloads in the background and goes in at «Restart to update»;",
            "This copy looks for a new version every six hours. It goes in when you press Install in Settings › About;"),
           ("ihatecrypto looks for a new version once a day. When one is ready you get a notification; Install shows what changed first.",
            "ihatecrypto looks for a new version every six hours. When one is ready you get a notification; Install is in Settings › About, "
            "and What's New shows what changed once it restarts."),
           ("Pinch and Ctrl+-scroll zoom stay at full frame rate. The glowing edge of a busy canvas breathes a few times and then stays lit, "
            "and the coin's face is played as a film, so the PC does far less work while the chat thinks.",
            "Pinch and Ctrl+scroll zoom stay at full frame rate.")]
used = set()

def said(text):
    for a, b in SWAPS: text = text.replace(a, b)
    for a, b in PHRASES:
        if a in text: used.add(a); text = text.replace(a, b)
    return text

releases = json.load(open(src, encoding="utf-8"))
# The PC's releases, newest first, before the Mac's older ones. Since 0.8.6 the PC's version is the Mac's (the owner,
# 2026-11-10: «синхронизируй версии, чтобы они были одинаковые»): what the previews 0.9.0–0.9.2 brought is listed under
# the Mac versions it matched (0.9.2 = 0.8.4, 0.9.1 = 0.8.3, 0.9.0 with 0.8.2), and 0.8.6 holds the Mac's 0.8.5 and
# 0.8.6 as the PC has them. The Mac's file on this branch ends at 0.8.2: newer entries are written here.
PC_ONLY = [
    {"version": "0.8.7", "date": "2026-11-13", "stories": [
        {"title": "Go past a risk limit when you choose", "symbol": "slider.horizontal.3", "color": "FF7A2F",
         "text": "Settings › Trading agent › Limits has «Over a limit»: Block, as before, Ask me or Just warn. With Ask me a plan "
                 "past your risk per trade isn't stopped. Its card shows the loss at the stop, and Confirm waits until you tick "
                 "«I accept losing $40.00 at the stop on this trade». Risk per trade and all positions together go up to «No "
                 "limit», leverage up to 50×.",
         "try": "long ETH risk $5 stop 3% take 6%"},
        {"title": "Limits change only with you", "symbol": "lock.shield.fill", "color": "34C759",
         "text": "Every change of a trading limit or of leverage asks for Windows Hello, and the signer keeps the limits itself. "
                 "An order with more leverage than you saved is signed with the main key, after asking you again."},
        {"title": "Limit orders, always private", "symbol": "clock.fill", "color": "0A84FF",
         "text": "«limit buy SOL at 120 for 20 USDC» or «limit sell 0.1 SOL at 200» leaves an order with NEAR Intents' 1Click "
                 "for 7 days, or as many as you say. It fills when the market reaches the price, in parts too, with nothing "
                 "more to sign, and every order is private. «show limit orders» lists them with the market beside each and a "
                 "Cancel. The chat and a notification say when one fills or runs out, with what came and its price after the fees.",
         "try": "show limit orders"},
    ]},
    {"version": "0.8.6", "date": "2026-11-10", "stories": [
        {"title": "A model that didn't answer says why", "symbol": "exclamationmark.bubble.fill", "color": "FF9F0A",
         "text": "When a model's service refuses, the chat says what happened and what to do, not the service's error text: "
                 "«NEAR AI Cloud says this account has no credits, or no spending limit is set for the key. Add credits or set "
                 "the limit in your account at cloud.near.ai». It knows a used-up plan, too many requests, a key that didn't "
                 "fit, a sign-in that ran out, a model that isn't there, a service that is down and a program that isn't running."},
        {"title": "A bridge's Windows Hello says what you get", "symbol": "text.badge.checkmark", "color": "0A84FF",
         "text": "Windows Hello for a Relay bridge now carries a line the signer reads from Relay's order itself: «Relay then "
                 "pays at least 4.878026 USDC to …», with the address whole and its network. Until now what arrives was only "
                 "on the card, which is the app's word. The amount on that line is the one the order is signed for, digit for digit.",
         "try": "move 5 USDC from Solana to Base"},
        {"title": "Another review, every finding fixed", "symbol": "checkmark.shield.fill", "color": "30D158",
         "text": "An address written without its «0x» is never signed, so nothing gets past the checks that know your wallet "
                 "and Relay by their addresses. A site asking for all your NFTs is refused whatever it puts in the yes, where a "
                 "2 used to read as a revoke. A task's bridge counts toward its daily limit by what doesn't come back. A site's "
                 "text asks Windows Hello of its own, whatever session is open."},
        {"title": "Steps wait for the money", "symbol": "list.number", "color": "FF7A2F",
         "text": "A step that follows a withdrawal waits until the balance shows it: «withdraw 1 USDC from Jupiter then deposit "
                 "1 USDC into Jupiter» deposits 1, where it could deposit the 0.1 that was already there. A step that names no "
                 "amount waits the same way, and one that asks for more than came waits only until what came shows.",
         "try": "withdraw 1 USDC from Jupiter then deposit 1 USDC into Jupiter"},
    ]},
    {"version": "0.8.4", "date": "2026-11-09", "stories": [
        {"title": "Solana keeps answering when its public node is busy", "symbol": "antenna.radiowaves.left.and.right", "color": "9945FF",
         "text": "Without a node of your own, the app asked only Solana's public node, and when it said «too many requests» a "
                 "balance or a move could stop after a few tries. Now those tries go to a second free node, Pocket, so reads and "
                 "sends keep going. Settings › General › Your own nodes also names FluxRPC, which has a free plan like Helius."},
    ]},
    {"version": "0.8.3", "date": "2026-11-09", "stories": [
        {"title": "Your own limits", "symbol": "gearshape.fill", "color": "0A84FF",
         "text": "Settings › Wallet › Limits holds what may sign without Windows Hello: the trading key's cap per order and per "
                 "day, how many days it keeps opening positions, and the automation wallet's cap per day and per week. The "
                 "separate signer keeps these numbers and what was spent, in a log no other program can reset. A lower number "
                 "holds at once, a higher one asks Windows Hello, and a move whose spending can't be written down isn't signed."},
        {"title": "The engine in a box of its own", "symbol": "lock.shield.fill", "color": "30D158",
         "text": "The part that reads the chat, the models, sites and networks runs in a Windows AppContainer now. It can't "
                 "open your files, the signer's folder or a USB device, and can't start a program. What it needs beyond that, "
                 "such as Claude Code or a sign-in page, the window does for it, one fixed request at a time. Your data moves "
                 "into the container's folder on the first start."},
    ]},
]
# With the Mac's 0.8.2 (NEAR) the PC got its Rust engine and signer: listed after the Mac's own stories there.
PC_082 = [
    {"title": "Rebuilt in Rust", "symbol": "wrench.and.screwdriver.fill", "color": "FF9F0A",
     "text": "The engine and the signer are Rust programs now, each part checked against the Mac's code with its own tests. "
             "A plain chat command is answered in about 8 ms instead of 120, the app starts faster, and what's installed is "
             "two programs of 20 MB instead of 38 files of 155 MB."},
    {"title": "Windows Hello as Windows draws it", "symbol": "checkmark.shield.fill", "color": "30D158",
     "text": "Confirm, the lock screen and a site's request show the PIN keypad Windows Hello asks with. Every move still "
             "asks Windows Hello before it's signed, and the app's passcode only opens the window."},
]
releases = PC_ONLY + releases
for r in releases[len(PC_ONLY):]:
    for s in r["stories"]:
        for k in ("title", "text", "try"):
            if isinstance(s.get(k), str): s[k] = said(s[k])
mac = next(r for r in releases if r["version"] == "0.8.2")
mac["stories"] = mac["stories"] + PC_082
# Newest first, each version once: a Mac entry newer than the PC's own list would repeat one written above.
versions = [r["version"] for r in releases]
if len(set(versions)) != len(versions): raise SystemExit("whatsnew.py: a version is listed twice: " + ", ".join(versions[:8]))
# A phrase that no longer matches would leave the Mac's words on the PC: the Mac's text changed, so say it again here.
missing = [a for a, _ in PHRASES if a not in used]
if missing: raise SystemExit("whatsnew.py: these phrases no longer match the Mac's text:\n" + "\n".join(missing))
with open(out, "w", encoding="utf-8") as f:
    f.write("// What's New, from the Mac's WhatsNew/whatsnew.json (newest first), its Mac words said for this PC. Written by\n"
            "// App/Windows/whatsnew.py at release; edit the Mac's file, not this one.\n"
            "export const releases = " + json.dumps(releases, ensure_ascii=False, indent=1) + ";\n")
print(len(releases), "releases →", out)
