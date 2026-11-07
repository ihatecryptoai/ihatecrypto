---
name: ship-change
description: Takes a finished change in ihatecrypto to done — the uitest scenario written first, App/done.sh green, a fresh-eye review, and a commit of only your own files in a checkout other agents share. Use when a change is coded and needs to be checked and committed; when the user says "доведи до конца", "закоммить", "прогони done.sh", "commit only my files", "definition of done", or when the Stop hook says App/Sources changed after the last done.sh.
---

# From a working change to a commit

AGENTS.md's definition of done, the way that works in practice.

## 1. The acceptance, written before the code

- **A scenario** in `App/Tools/uitest-scenarios.json` for what the chat or the canvas shows. One object per scenario:
  `{"name": "…", "send": "phrase" | ["phrase", …], "expect": {…}}`, or a conversation
  `{"name": "conversation: …", "turns": [{"send": "…", "expect": {…}}, …]}`, optionally
  `"only_if": {"wallet": "test"} | {"balance": "solanaUSDC", "at_least": 5} | {"second_check": true}`.
  The `expect` keys are the `check()` branches in `App/Tools/uitest.py` (`pending_kind`, `title_contains`,
  `command_contains`, `steps`, `reply_contains`, `reply_cyrillic`, `no_pending`, …); a new kind of check is a new branch
  there. Edit the file with Python and write it back as `json.dumps(s, indent=2, ensure_ascii=False) + "\n"`.
- **Parser tests in English and Russian** for a new command (`App/Tests/IhatecryptoCoreTests`), and the form in
  `ChatRouter.forms` (`Normalize.swift`) — the model writes the form, code parses it.
- **A live run** for a money feature: the `live-run` skill.

## 2. `App/done.sh "<scenario words>"` until green

- Quote phrases: words match scenario names one by one, and «a» matches nearly every one.
- Right before it: `Tools/dev.sh relaunch --build` (it refuses a Dev build older than Sources).
- It takes more than the shell's 10 minutes with UI scenarios: run it with `run_in_background` and read
  `App/.build/done/{build,test,lint,uitest}.log` and `summary.json` when it ends. One run per checkout (a lock).
- «the UI step left no pictures» / «could not create image from window»: the Dev window is on another desktop. Ask the
  owner to bring it over and run again; don't paper over it with `--skip`.
- `./done.sh --skip "why"` only for what none of this covers (docs, the site, a tool script).
- Tests alone, faster: `swift build --build-tests && swift test --skip-build --filter <Suite>`.

## 3. A fresh eye on the diff

A new subagent given only your diff (`git diff -- <your files>`) and the plan, asked for defects in money, signing and
guard paths first. Fix what you confirm, say what you didn't take and why. In a shared checkout name your files: the
others' changes aren't yours to review.

## 4. Commit only your files

Other agents work in the same checkout: never `git add -A`, `git stash`, `git reset --hard` or `git checkout -- .`.

```sh
printf '%s\n' "Subject: what changed, for whom" "" "Body: why." "" "Co-Authored-By: …" > $TMPDIR/msg
App/Tools/commit-own.sh $TMPDIR/msg App/Sources/… App/Tests/…      # a private index; HEAD moves only if nobody moved it
```

- A file another agent also edits: commit only your hunks. `python3 -I App/Tools/hunks.py list <f>` numbers the
  file's hunks against HEAD; `App/Tools/commit-own.sh $TMPDIR/msg <f>@1,3 <other files>` commits HEAD plus hunks 1
  and 3 of it. Place your edits away from theirs (a few lines apart) so the hunks stay separate, and for the review
  build your diff the same way (`hunks.py keep <f> 1,3` against `git show HEAD:<f>`).
- Before committing a big change, build the commit's tree alone: `git archive <tree> | tar -x -C <dir>`, copy
  `App/.build` there (`cp -cR`), `swift build --build-tests && swift test --skip-build`. Others' uncommitted files can
  make your tree build in the checkout and fail alone.
- EPERM from git in the repo directory: run git from `$HOME` with `GIT_DIR`/`GIT_WORK_TREE`, and unset them before
  `swift build`.
- English in code, comments and commits; no wallet addresses, keys, tokens or personal data anywhere in the diff.
- Push only when the owner says.
