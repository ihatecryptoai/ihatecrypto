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

def said(text):
    for a, b in SWAPS: text = text.replace(a, b)
    return text

releases = json.load(open(src, encoding="utf-8"))
for r in releases:
    for s in r["stories"]:
        for k in ("title", "text", "try"):
            if isinstance(s.get(k), str): s[k] = said(s[k])
with open(out, "w", encoding="utf-8") as f:
    f.write("// What's New, from the Mac's WhatsNew/whatsnew.json (newest first), its Mac words said for this PC. Written by\n"
            "// App/Windows/whatsnew.py at release; edit the Mac's file, not this one.\n"
            "export const releases = " + json.dumps(releases, ensure_ascii=False, indent=1) + ";\n")
print(len(releases), "releases →", out)
