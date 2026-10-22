# Contributing

Thanks for helping. For a big change or a new network, open an issue first: maintainers decide which networks and
protocols the app takes on, and a short talk before the code saves a rewrite.

## Requirements

- Xcode 27 (the macOS 27 SDK; the app itself runs on macOS 26 and later).
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) and an Apple developer team, only for the full app.

## Build and test

```sh
cd App
swift build                         # every module
swift test                          # the tests; no Apple account needed
./lint.sh                           # swift-format rules from .swift-format
./run.sh                            # the app without its signing process: it shows balances but can't sign
TEAM=<your team id> ./build-app.sh  # «ihatecrypto Dev.app» with Signer.xpc, signed by your team
```

Tests that call real services run only with `IHATECRYPTO_NET=1`. Fork tests run only with
`IHATECRYPTO_ANVIL=<anvil url>`; `Tools/fork7702.sh` starts a fork with Foundry and runs the EIP-7702 ones.

`build-app.sh` makes a separate copy, «ihatecrypto Dev.app», with its own bundle id, Keychain group (so its own
wallets), settings and data folder. It never touches an installed ihatecrypto.app. A Release build goes to
`~/Applications/ihatecrypto Dev.app`; `./build-app.sh Debug` stays in `App/.build/xcode`.

## Live checks

`swift run WalletCheck <command>` runs against real services. It signs with a throwaway wallet whose phrase sits in
`~/.ihatecrypto/test-wallet.txt` (`swift run WalletCheck new-wallet` makes one). Nothing is sent without `--yes`, and
amounts are capped. Put only a few dollars on that wallet. The list of commands is at the top of
`App/Sources/Wallet/Check/main.swift`.

`swift run LLMCheck <provider> "text"` checks one model provider from the terminal. On a subscription it spends your
plan's limits, so run a few phrases, not a whole set.

## Rules

- Never commit keys, seed phrases, API tokens or your own wallet addresses. Tests use the public test phrases
  («abandon … about», Hardhat's «test … junk») and the addresses they derive.
- A money move happens only after Confirm. Don't add a path that signs without it.
- Keep safety checks in code (`Guard`, `Signer`), not in model prompts.
- English in code, comments and commits. A commit message says what the user can now do and how you checked it.
- Run `swift test` and `./lint.sh` before you open a pull request. For a UI change, add a screenshot.

Security issues go through [SECURITY.md](SECURITY.md), not issues or pull requests.
