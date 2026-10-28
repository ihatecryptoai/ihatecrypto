---
name: dev-copy
description: Builds, relaunches and drives «ihatecrypto Dev» (the development copy, debug API on port 51790) from a terminal — state, chat messages, cards, cancel, restore — and shares it with other sessions without breaking their runs. Use before any build, UI check, uitest or live run on the app; when the user says "rebuild Dev", "relaunch the app", "send this to the chat", "what does the app show", "пересобери Dev", "перезапусти приложение", "что сейчас на канвасе"; or when the debug token, port or /state is needed.
---

# The Dev copy from a terminal

Only «ihatecrypto Dev.app» (bundle `app.ihatecrypto.mac.dev`, data in `~/Library/Application Support/ihatecrypto-dev`,
debug API on 127.0.0.1:51790). Never build over, quit or drive an installed ihatecrypto.app (port 51789) unless the
user asks. `App/Tools/dev.sh` names only the Dev copy:

```sh
cd App
Tools/dev.sh state                 # busy/idle, executing, the card waiting for Confirm and its steps, balances, last messages
Tools/dev.sh api GET /state        # any endpoint (DebugAPI.swift lists them); a body third:
Tools/dev.sh api POST /send '{"text":"займи 1 USDC под SPCX на камино"}'   # waits for the model's answer (≤ 240 s)
Tools/dev.sh api POST /cancel      # drop the card · /reset a fresh test canvas · /restore '{"canvas":"<id>"}' delete test canvases
Tools/dev.sh busy                  # exit 0 while a card waits, a move runs, the model answers, or uitest/done.sh runs
Tools/dev.sh relaunch --build      # refuses while busy; quit, build-app.sh, open, wait until the new token answers
Tools/dev.sh relaunch -- -jevOpenRouter YES   # launch flags after --; plain `relaunch` reopens without building
```

## Rules that save the most time

- **The token changes on every launch.** `dev.sh` reads `debug-token` for each call; never keep it in a variable
  across a relaunch. Right after launch `/state` can be half-written JSON: parse with `json.loads(…, strict=False)`.
- **Other sessions share this copy.** Before a relaunch or a long run, `Tools/dev.sh busy` must say nothing. If another
  session uses it, message that session once (ListAgents / SendMessage) and wait with `Tools/dev.sh idle`. Don't
  quit Dev while it's busy: a quit in the middle of a move leaves it unsettled.
- **done.sh refuses a stale build** («the Dev copy runs a build older than Sources»): another agent's edit is enough.
  `Tools/dev.sh relaunch --build` right before `done.sh`, not an hour before.
- `build-app.sh` needs no `TEAM`: project.yml has it. It builds Release into `~/Applications`. A Debug build
  (`./build-app.sh Debug`) stays in `.build/xcode` and is only for `-signerSelfTest` through `open -W --stdout`.
- `kill <pid>` then `open` fails with `-600`: quit through osascript (dev.sh does) and wait for the process to end.
- Pictures: `/screenshot` comes out blank (glass chrome). `screencapture -x -o -l <windowID from /state> out.png` needs
  the window on the current desktop: «could not create image from window» means it is on another Space or the screen
  is locked — ask the owner to bring it over; nothing in code fixes that.
- A scenario or test that leaves `agentMemory` behind: check `GET /context`, clear with
  `defaults write app.ihatecrypto.mac.dev agentMemory -array`.
- zsh: quote globs — `grep -rn x Sources --include='*.swift'`; unquoted it fails with «no matches found».
- Jev (the second check) is off in Dev unless launched with `-jevOpenRouter YES`.

## The model behind the chat

/state says which provider and model answer. A subscription (Claude Code, ChatGPT) spends the owner's limits: a few
messages per check, never a bulk run. Never start Ollama or a local model for a check.
