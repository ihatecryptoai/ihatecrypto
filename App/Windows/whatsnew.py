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
# The PC's own releases, newest first, before the Mac's: what only the Windows app changed.
PC_ONLY = [
    {"version": "0.9.0", "date": "2026-11-07", "stories": [
        {"title": "Rebuilt in Rust", "symbol": "wrench.and.screwdriver.fill", "color": "FF9F0A",
         "text": "The engine and the signer are Rust programs now, each part checked against the Mac's code with its own tests. "
                 "A plain chat command is answered in about 8 ms instead of 120, the app starts faster, and what's installed is "
                 "two programs of 20 MB instead of 38 files of 155 MB."},
        {"title": "Windows Hello as Windows draws it", "symbol": "checkmark.shield.fill", "color": "30D158",
         "text": "Confirm, the lock screen and a site's request show the PIN keypad Windows Hello asks with. Every move still "
                 "asks Windows Hello before it's signed, and the app's passcode only opens the window."},
    ]},
]
releases = PC_ONLY + releases
for r in releases[len(PC_ONLY):]:
    for s in r["stories"]:
        for k in ("title", "text", "try"):
            if isinstance(s.get(k), str): s[k] = said(s[k])
# A phrase that no longer matches would leave the Mac's words on the PC: the Mac's text changed, so say it again here.
missing = [a for a, _ in PHRASES if a not in used]
if missing: raise SystemExit("whatsnew.py: these phrases no longer match the Mac's text:\n" + "\n".join(missing))
with open(out, "w", encoding="utf-8") as f:
    f.write("// What's New, from the Mac's WhatsNew/whatsnew.json (newest first), its Mac words said for this PC. Written by\n"
            "// App/Windows/whatsnew.py at release; edit the Mac's file, not this one.\n"
            "export const releases = " + json.dumps(releases, ensure_ascii=False, indent=1) + ";\n")
print(len(releases), "releases →", out)
