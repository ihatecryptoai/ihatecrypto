# Notes for coding agents

ihatecrypto is a native macOS 26 wallet (SwiftUI, SwiftPM). A chat turns a request into a plan, the plan is drawn as
cards, and a separate signing process signs only after the user presses Confirm. This file is for Claude Code, Codex
and other agents working in this repo. Read [docs/architecture.md](docs/architecture.md) before a change that crosses
modules.

## Layout

Everything is under `App/`. One folder under `App/Sources` is one SwiftPM target (`App/Package.swift`), and
dependencies point inward: chains and protocols know nothing about the window or the models.

| Folder | What it holds |
| --- | --- |
| `IhatecryptoApp` | The window: chat, canvas, cards, Settings, the dApp browser, the debug API |
| `IhatecryptoCore` | Commands and their parsers, the chat router's forms, plans, risk rules, holdings, scheduled tasks |
| `LLM/*` | One target per way to reach a model; see `App/Sources/LLM/README.md` |
| `Wallet/Core`, `Wallet/Ledger`, `Wallet/Check` | Keys and signatures, the Ledger, the `WalletCheck` CLI |
| `Chains/*`, `Protocols/*`, `DeFi/*` | Solana and EVM; Hyperliquid, Jupiter, Relay, 1Click; Jupiter Lend, Kamino, yields |
| `Execution`, `Execution/Trade` | The write-before-send journal; running an approved plan |
| `Guard/Core`, `Guard/Jev` | The transaction guard and injection screening; the optional Jev classifier |
| `Signer/API`, `Signer/Service` | The XPC protocol and the signing process |

Tests are in `App/Tests/<Target>Tests`. Tools are in `App/Tools`. `App/Xcode/project.yml` (XcodeGen) packages the app
and `Signer.xpc`; the SwiftPM package stays the source of the modules.

## Commands

Run from `App/`:

```sh
swift build && swift build --build-tests && swift test --skip-build
./lint.sh
./done.sh [scenario words…]          # the definition of done (below): build, tests, lint, UI scenarios, design review
./run.sh                             # no signing process: balances only
TEAM=<team id> ./build-app.sh        # «ihatecrypto Dev.app»; add Debug for a build in .build/xcode
./verify-release.sh <version>        # rebuild a release from its tag and compare it with the DMG (~5 min)
swift run WalletCheck <command>      # live checks on ~/.ihatecrypto/test-wallet.txt; nothing sent without --yes
swift run LLMCheck <provider> "text" # one model provider; on a subscription it spends the user's limits
open -W --stdout /dev/stdout ".build/xcode/Build/Products/Debug/ihatecrypto Dev.app" --args -signerSelfTest YES
```

The signer self-test needs a Debug build and `open`: launched directly, the app can't reach its XPC service.

## Two copies of the app

`build-app.sh` makes «ihatecrypto Dev.app» (bundle id `app.ihatecrypto.mac.dev`). It has its own Keychain group and
wallets, settings, model logins, data folder (`~/Library/Application Support/ihatecrypto-dev`) and debug API port
(51790). An installed ihatecrypto.app uses `app.ihatecrypto.mac`, `Application Support/ihatecrypto` and port 51789.
Build, test and run only the Dev copy. Never replace, quit or drive an installed ihatecrypto.app unless the user asks.

## Debug API and UI tests

Only the copies `build-app.sh` makes (and `swift run`) have it: a release is built without it (`DEBUG_API`, project.yml).
The user turns it on in Settings › Debugging › Local debug API. It listens on 127.0.0.1, and the token is in
`debug-token` in the copy's data folder. Endpoints are listed at the top of `App/Sources/IhatecryptoApp/DebugAPI.swift`
(`/state`, `/send`, `/screenshot`, `/reset`, `/cancel`, `/pick`, `/restore`). It never signs.

`python3 App/Tools/uitest.py [word…]` runs the scenarios in `App/Tools/uitest-scenarios.json` against the Dev copy.
Each one gets a fresh test canvas and is cleaned up afterwards. Add a scenario for a change in what the chat or the
canvas shows. `IHATECRYPTO_APP=work` points it at the installed app; use that only when the user asks.

## Definition of done

1. Before the code, write the acceptance: a scenario in `App/Tools/uitest-scenarios.json` for what the chat or the
   canvas shows, parser tests in English and Russian for a new command, and for a money feature the live run on the
   Dev copy's test wallet.
2. Run `App/done.sh <scenario words>` until it says green. It builds, runs the tests and `lint.sh`, runs the scenarios
   whose names contain the words on the Dev copy (start it and turn on its debug API first), and has a separate model
   review the window after each one. Green writes `App/.build/done/summary.json`. A change that none of this covers
   (docs, the site): `App/done.sh --skip "why"`. The Stop hook in `.claude/settings.json` reminds you while App/Sources
   or App/Tests changed after the last green.
3. Have the diff reviewed by a fresh eye: a new subagent given only your diff and the plan, and Codex too when it's
   installed (`codex review --uncommitted`; in a shared checkout tell it which files are yours). Fix what you confirm.
4. Commit only your own files.

## Conventions

- A new network: follow `.claude/skills/add-network/SKILL.md`, after an issue says the network is wanted.
- Dollars without gas are a dead end: on any network, and in any new feature that brings money somewhere, the first
  move with only USDC there must work, or the app must say what to add before it's sent (Top up's gas line,
  `Workspace.stuckDollars`; the add-network skill, «The first move with dollars only»).
- A new model provider: `App/Sources/LLM/README.md`.
- A new Solana DeFi protocol: a folder `DeFi/<Protocol>/`, a target in `Package.swift`, its program in
  `SolanaGuard.knownPrograms`, and balance rules for the simulation check.
- The chat model rewrites a request as one English form (`ChatRouter.forms` in `IhatecryptoCore/Normalize.swift`);
  code parses the form. A new command gets a form there and parser tests in English and Russian.
- UI text never says «Touch ID» directly: use `Presence.name`, `Presence.short` and `Presence.icon`, since a Mac
  without Touch ID asks for its password.
- Launch flags that drive the app (`-signerSelfTest`, `-openBrowser`) stay inside `#if DEBUG`.

## Rules

- Money moves only after Confirm. Never add a path that signs without it. An automated live run presses Confirm only
  in the Dev copy, on its test wallet.
- Live runs use the test wallet with a few dollars. Never sign from a main wallet yourself.
- Tests use public test phrases («abandon … about», Hardhat's «test … junk») and the addresses they derive. No real
  wallet addresses, keys, seed phrases or API tokens in the repo, in logs or in commit messages.
- Safety lives in code: the Guard and the signer's checks. A prompt can ask the model to be careful, but a check that
  matters goes in `Guard` or `Signer`, and the signer must not trust what the app says it can verify itself.
- Other agents may work in the same checkout. Commit only the files you changed, and don't stash or reset others' work.
- English in code, comments and commits.
