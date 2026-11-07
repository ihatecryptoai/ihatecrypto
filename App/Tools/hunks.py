#!/usr/bin/env python3
"""A file's changes against HEAD as numbered hunks, and the file with only some of them: for committing your own
hunks of a file another agent also edits (Tools/commit-own.sh takes «file@1,3»).
    python3 -I Tools/hunks.py list <file>          the hunks, numbered from 1, with their first changed lines
    python3 -I Tools/hunks.py keep <file> 1,3      the file as HEAD has it plus hunks 1 and 3 only, on stdout
Paths relative to the current directory. A new file (not in HEAD) is one hunk."""
import difflib, os, subprocess, sys


def versions(path):
    top = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    rel = os.path.relpath(os.path.abspath(path), top)
    head = subprocess.run(["git", "-C", top, "show", f"HEAD:{rel}"], capture_output=True, text=True)
    old = head.stdout.splitlines(keepends=True) if head.returncode == 0 else []
    return old, open(path, encoding="utf-8").read().splitlines(keepends=True)


def hunks(old, new):
    return [op for op in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes() if op[0] != "equal"]


def main():
    if len(sys.argv) < 3 or sys.argv[1] not in ("list", "keep") or (sys.argv[1] == "keep" and len(sys.argv) < 4):
        sys.exit(__doc__)
    old, new = versions(sys.argv[2])
    ops = hunks(old, new)
    if sys.argv[1] == "list":
        for n, (tag, i1, i2, j1, j2) in enumerate(ops, 1):
            first = (new[j1] if j2 > j1 else "- " + old[i1]).strip()[:110]
            print(f"{n:3}  {tag:7}  HEAD {i1 + 1}-{i2}  now {j1 + 1}-{j2}  {first}")
        return
    keep = {int(x) for x in sys.argv[3].split(",") if x}
    if not keep <= set(range(1, len(ops) + 1)):
        sys.exit(f"hunks are 1…{len(ops)}")
    out, at = [], 0
    for n, (tag, i1, i2, j1, j2) in enumerate(ops, 1):
        out += old[at:i1]
        out += new[j1:j2] if n in keep else old[i1:i2]
        at = i2
    sys.stdout.write("".join(out + old[at:]))


if __name__ == "__main__":
    main()
