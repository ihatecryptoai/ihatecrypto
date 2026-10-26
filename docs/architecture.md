# Architecture

ihatecrypto is a SwiftUI app for macOS 26 built from a SwiftPM package (`App/Package.swift`). The chat model reads
what the user wants and says it back as a command; code turns the command into a plan, checks it, draws it as cards
and runs it after Confirm. Keys live in a separate signing process that checks each transaction again before it
signs. The model never signs and never decides whether a move is allowed.

## Modules

One folder under `App/Sources` is one SwiftPM target. Dependencies point inward: chains, protocols and the signer know
nothing about the window or the models.

```
IhatecryptoApp/          the window: chat, canvas, cards, Settings, address book, dApp browser, debug API
IhatecryptoCore/         commands and their parsers, the chat router's forms, plans, risk rules, holdings,
                         scheduled tasks, modules (what the assistant may use)
LLM/*                    model access, one target per way to connect (see LLM/README.md)

Wallet/Core              BIP-39, key derivation for EVM, Solana and NEAR, Keccak, EIP-712, address checks
Wallet/Ledger            a Ledger over USB or Bluetooth (or the Speculos emulator)
Wallet/Check             `swift run WalletCheck …`: live checks on a throwaway test wallet
Chains/Solana            RPC, SOL and SPL transfers, token accounts, transaction parsing, lookup tables
Chains/EVM               RLP, EIP-1559 and EIP-7702 transactions, ERC-20, gas, calldata decoding; Uniswap v2/v3/v4,
                         Aave v3, Morpho, CCTP, USDT0, Safe, Merkl, ENS
Protocols/Hyperliquid    MessagePack, order and transfer signatures
Protocols/Jupiter        Solana swaps and Jupiter's recurring orders
Protocols/OneClick       NEAR Intents 1Click: quote, deposit, status, the address binding
Protocols/Relay          the Relay bridge: quotes, the depository, order binding
DeFi/JupiterLend         Jupiter Lend Earn vaults: deposit, withdraw, positions
DeFi/Kamino              Kamino lending and vaults
DeFi/Yields              where money earns now across the DeFi modules, and rebalancing
Execution                the approval envelope and the write-before-send journal
Execution/Trade          running an approved plan: bridges, swaps, transfers, lending, trades
Guard/Core               the last check before signing, and injection screening of untrusted text
Guard/Jev                an optional classifier for untrusted text and plans (TypeSafe Jev through OpenRouter)
Signer/API               the XPC protocol between the app and the signing process
Signer/Service           the signing process: seed storage, parsing, the Guard, signatures; sandboxed, no network
```

Networks: Solana, Hyperliquid, NEAR and the EVM networks in `EVMNetwork.all` (`Execution/Trade/Aave.swift`): Base,
Arbitrum, Robinhood Chain, Polygon, Ethereum, Tempo, HyperEVM, Optimism and BNB Chain. Coins on networks without a
wallet here (Bitcoin, Zcash, Tron, XRP, Dogecoin, Sui) go out and come in through NEAR Intents. Settings › Modules
turns networks, kinds of moves and protocols on and off (`IhatecryptoCore/Modules.swift`); the assistant never plans a
move through something that is off.

## Builds

- `./run.sh`: a quick development build with `swift build`, without the signing process. It shows balances of
  addresses you add, but can't make a wallet or sign.
- `TEAM=<team id> ./build-app.sh [Release|Debug]`: «ihatecrypto Dev.app» with `Signer.xpc` embedded, from
  `Xcode/project.yml` (XcodeGen). It sits beside an installed ihatecrypto.app and shares nothing with it: the bundle
  id `app.ihatecrypto.mac.dev` gives it its own Keychain group (its own wallets), settings and model logins, and
  `Flavor` (`IhatecryptoApp/Canvases.swift`) gives it its own data folder (`Application Support/ihatecrypto-dev`), debug
  API port (51790) and Keep running agent. Release goes to `~/Applications/ihatecrypto Dev.app`.
- `release.sh`: the signed, notarized release with its Sparkle update archive and appcast.
- `verify-release.sh <version>`: rebuilds a release's tag with `Tools/build-release.sh`, the build `release.sh` makes, in
  the same folder (`/private/tmp/ihatecrypto-release`, since its path goes into the binaries) and compares it with the
  published DMG file by file, code signatures taken off. The Verify release workflow runs it for every release.
- Signer self-test on the public test phrase, from `App/`:
  `open -W --stdout /dev/stdout ".build/xcode/Build/Products/Debug/ihatecrypto Dev.app" --args -signerSelfTest YES`.
  Launch flags work only in Debug builds. Use `open`: launched directly, the app can't reach its XPC service.

## From a message to a move

1. **Screening.** Every message, and everything the model's lookups bring back, passes `InjectionGuard` first (see
   «Untrusted text» below).
2. **Reading.** The model reads every message in the context of the chat, the canvas and the portfolio
   (`ChatRouter` in `IhatecryptoCore/Normalize.swift`). It may run read-only lookups first (the portfolio, a market,
   a token, rates, a contract call). It answers with a route: a command written as one English line in the router's
   forms, a question, a plan change, a reply, a question back to the user, or cancel. When two readings would move
   different money, it asks.
3. **Parsing.** Code parses the command (`PositionCommand.swift`, `WalletCommand`) exactly as if the user had typed it.
   An address can come only from the user's message or their address book. If the second check (Jev) doubts the
   reading, nothing runs and the user picks.
4. **Planning.** Code builds the move from fresh prices and balances. For a trade, `Executor.prepare`
   (`Execution/Trade/Executor.swift`) applies the risk rules (`IhatecryptoCore/Risk.swift`), builds the bridge
   transaction and simulates it, so the card shows what each balance will be. A move the wallet can't pay for is
   turned down before a card is drawn.
5. **Confirm.** Only the Confirm button runs a move; a typed «yes» never does. For a quoted bridge or swap the user
   approves an envelope (`Execution/Envelope.swift`): limits, not a price snapshot. Live values may move inside them;
   anything outside stops the route and needs a new approval.
6. **Signing and sending.** `execute` signs through `TradeSigner` (the signing process in the app, a local test key
   in `WalletCheck`). The journal (`Execution/Journal.swift`) writes each entry to disk before the transaction is
   sent, keyed by an id known in advance. After a crash, `unresolved()` lists what to check on chain instead of
   sending it again (`WalletCheck reconcile`).

For a perp: bridge with Relay if the trading balance is short, set leverage, enter, place the stop and the take, then
read the position back from the exchange. A limit entry carries its stop and take in one `normalTpsl` group, so the
exchange arms them when it fills. If the stop can't be placed, the position is closed at once. In the terminal:
`WalletCheck execute "<phrase>" [--yes]`, `close ETH --yes`, `position`.

Jev (`Guard/Jev`, `JevReviewer`) checks the model's command against the message, the plan against the user's words,
and the list of signatures with their balance changes against the plan. It can only warn: when the answers other than
«all correct» hold half the probability or more. The user then confirms the warning with a separate checkbox. Code
decides. Jev gets the router's forms for the command (`ChatRouter.forms(for:)`); without them it didn't know that
`lend` means Jupiter Lend. Measurements and labelled sets are in `App/Tools/jevcheck` (`WalletCheck jev-eval`).

## Commands in the chat

Examples of what `PositionCommand.swift` parses after the model's rewrite:

- An open position: «stop to breakeven», «stop at X», «take at X», «close half», «close», «cancel the limit»; after
  entry, «stop 2%» and «take closer» move the orders on the exchange.
- Money: «withdraw N USDC to Solana», «deposit / withdraw N USDC from the vault», «buy SOL for N USDC», «sell all my
  ETH», «buy SOL for N USDC every week», «stop the buys».
- Scheduled tasks (`ScheduledTask.swift`): «in 1 week: …», «every day at 09:00: …», price alerts. Each step runs as
  if typed, on a canvas of its own. A money move stops at its Confirm card unless the user set up an automation
  wallet, which signs only a task's moves.

Trailing stops, breakeven after a partial take (`Guardian.swift`) and scheduled tasks run while ihatecrypto is open,
including from the menu bar with the window closed.

The Hyperliquid trading key (an agent key) is made and kept by the signing process. It signs orders without Touch ID
and can't withdraw.

## dApp browser

`IhatecryptoApp/Browser` is a WKWebView. Before the page's own scripts, it injects a Solana wallet (Wallet Standard
and the older `navigator.wallets`) and an EVM provider (EIP-1193, announced through EIP-6963). Every request goes
through a confirmation window.

- Solana transactions are parsed and simulated. Programs outside the built-in list sign only after a separate
  permission (`extraPrograms` to the signer), named on the Touch ID prompt.
- EVM sign-in (`personal_sign`) and flat typed data: the signer hashes the JSON itself (`Wallet/Core/TypedData.swift`)
  and refuses Permit and Permit2, nested types and bare hashes.
- EVM transactions (`eth_sendTransaction`): calldata is decoded (`EVMCall` in `Chains/EVM/EVM.swift`). Unlimited
  approvals, `setApprovalForAll`, permits and `transferFrom` out of the user's wallet are refused in the app and in
  the signer. The call is checked with `eth_call`, gas is estimated, and the signer builds and signs the transaction.
- `SiteCheck` flags hosts that look like known dApps, and the browser checks MetaMask's phishing list. Jev
  (`JevReviewer.site`) gives a second opinion on the site and the transaction.

Test page: `WalletCheck browser-test-page <path>`, then a Debug build with
`-openBrowser file://… -browserAutoApprove YES -browserResultAfter 40`.

## Risk rules

`RiskPolicy` in `IhatecryptoCore/Risk.swift`, for Hyperliquid perps (crypto, and oil and stocks through HIP-3
markets). Hard limits that block a trade: at most 2% of capital lost at the stop, and at most 6% across all open
positions at their stops. Both change in Settings › Trading agent. Warnings: reward to risk below 1:1.5, a stop
inside the 4-hour ATR, leverage above 5×, liquidation closer than two stops, a trade against a big 24-hour move,
expensive funding, margin over half the capital. «By the book» means a stop at 1.5 × the 4-hour ATR, a take at 2R
and 1% of capital at risk.

## The signing process

The app never gets a key. It sends a `SignerRequest` (`Signer/API/SignerAPI.swift`: JSON in one XPC call), and the
signing process parses the transaction itself and runs the Guard. For Hyperliquid transfers it picks the EIP-712
schema from the action type. Token-account owners the app names are derived again in the signer, and a made-up claim
is dropped.

The seed is encrypted to a Secure Enclave key (P-256, ECIES); the ciphertext sits in the data-protection Keychain,
this device only, in the signer's own access group. The Secure Enclave can't hold secp256k1 or Ed25519 keys, so it
guards the seed instead. Opening the seed asks for Touch ID or the Mac password; the Touch ID prompt shows the amounts
and addresses from the signer's own parsing. One prompt covers one move, for a session of one minute by default.
After each signature the seed and the derived key are overwritten (`SeedStore.wipe`).

Derivation paths: EVM `m/44'/60'/0'/0/i` (MetaMask, Rabby), Solana `m/44'/501'/i'/0'` (Phantom, Solflare), NEAR
`m/44'/397'/i'`.

The signer has no network, so the app tells it the approved recipients and Solana lookup-table contents. A recipient
outside the wallet still needs Touch ID once, with the address on the prompt, and the signer keeps it in its own
Keychain group (`RecipientStore`). The address book lives there too, so a changed file or a chat message can't put a
scam address under a saved name.

## The Guard

Plain code, not a model. A model can be fooled by text on a page; a check against a list can't.

- **Solana** (`SolanaGuard`): the transaction is parsed instruction by instruction. Only listed programs run
  (`SolanaGuard.knownPrograms`: System, Compute Budget, SPL Token and Token-2022, Associated Token, Jupiter Swap,
  Jupiter Lend, Jupiter DCA, Relay Depository, Kamino Lend, Vault and Farms, Address Lookup Table). Money goes only to
  the user's own accounts or to an address bound to an approved quote. Token approvals, owner changes and other fee
  payers are refused, and the priority fee has a cap.
- **Simulation:** before sending, the transaction runs on the network, and the balances after are compared with what
  the user approved. This catches a route that pays its output to someone else inside a program.
- **1Click** (`DepositGuard`): the deposit address must be the one 1Click bound to the user's recipient, refund
  address, assets and amount.
- **Relay:** only `deposit_token` and `deposit_native` of the Relay Depository, from the user's wallet. The quote must
  name the user as sender and recipient. The depository address on Hyperliquid is fixed in code and compared with
  Relay's `/chains`.
- **Hyperliquid** (`HLGuard`): trading inside the account is allowed; transfers out go only to an approved address,
  and only the user's own agent key can be approved.
- **EVM:** the calldata rules above, an exact amount on every approve, and Permit2 approvals for one day at most.

**Untrusted text** (`Guard/Core/Injection.swift`): before the parser and the model, local rules look for commands to
the assistant and «send it to this address». If the user turned on the second check, Jev reads it too. Jev can only
stop a text: it never clears what the rules found and never allows a signature. It is told who is speaking: the user
(whose orders to the assistant are normal; it looks for pasted text from someone else) or a lookup's result, such as a
token's description, a service's docs, an API reply or a contract's string (`Workspace.screenedLookup`,
`InjectionGuard.lookupProblem`). A lookup result that addresses the assistant or pushes money to an address never
reaches the model. Jev reads every chat message but the Confirm button's own, and the results of the `token`,
`services`, `service`, `call`, `rpc`, `hl` and `fetch` lookups. Other text gets the local rules only. The local rules
stop «ты теперь» and «you are now» only with a new role or mode («ты теперь в режиме разработчика»), so the user's own
«ты теперь всегда отвечай по-русски» reaches the model. Why a message was stopped comes back as data
(`InjectionGuard.Stop`), and the chat says it in the chat's language.

## Data on the Mac

- Seeds, the trading key, approved recipients and the address book: the signer's Keychain group.
- Model keys and tokens: the app's Keychain. For other apps' logins (Codex CLI, Claude Code) only the path is kept.
- The user's own RPC node URLs (they carry the provider's key): the login Keychain.
- Chats, canvases, settings and kept crash reports: owner-only files in `~/Library/Application Support/<copy>`.

Every balance read tells the RPC operator the address and the Mac's IP.

## Adding things

- A network: `.claude/skills/add-network/SKILL.md`, after an issue says it's wanted.
- A model provider: [`App/Sources/LLM/README.md`](../App/Sources/LLM/README.md).
- A Solana DeFi protocol: a folder `DeFi/<Protocol>/`, a target in `Package.swift`, its program in
  `SolanaGuard.knownPrograms`, and balance rules for the simulation check.

## Known limits

- A native EVM amount is a `UInt64` of wei, which caps one native transfer at about 18 ETH.
- The journal is one JSON line per event in a file, with no queries over history.
- Trailing stops, breakeven moves and scheduled tasks run only while the app runs.
- A key is in the signer's memory while it signs; copies inside the derivation and secp256k1 code can't be wiped from
  Swift. Release builds have no `get-task-allow`, so no debugger can attach.
