# The Windows app in Rust

The Windows app is three programs (docs/windows.md): the window (Rust, Tauri), the engine and the signer. The engine
and the signer were Swift, the Mac's core built for Windows; since step 14 both are Rust, so Windows needs no Swift
toolchain and no Swift runtime beside the programs, and the Mac's code can drop its `#if os(Windows)`.

The rule for every step: a Rust crate in `App/Windows/rust` takes the place of one Swift module, gives the same
result for the same input (its tests carry the Swift tests' vectors and the same error words, since the window shows
them), and nothing switches to it until it does. The pipes don't change: the window ↔ engine JSON lines and the
engine ↔ signer framed `SignerRequest`/`SignerReply` stay as they are, so either program can be swapped alone.

```sh
cd App/Windows/rust && cargo test
```

## Found while porting

- The Swift Windows signer kept the Hyperliquid trading key's week as a Double, which its trusted settings can't hold:
  the week never started, so every order that opens or adds went to the main key with a prompt. The Rust signer keeps
  whole seconds.
- The EVM client read a node's refusal with a JSON body of its own (`{"error": "forbidden"}`, HTTP 403) as an empty
  result. The Rust client takes it as a refusal and asks the next node.
- main's `sellStock` (Stocks.swift) says what the sale brought with `Decimal(after - before)` on two `UInt64`s: a USDG
  balance read lower after the sale than before (a node behind, USDG leaving at the same time) traps, after the sale went
  through. The line above it guards the same subtraction. The Rust port says the change as it is.
- The Swift Windows engine counted a contract call's approve of a token no one prices as $0 against the cap, and took
  18 decimals for a token whose `decimals()` doesn't answer; main had since made both refusals. The Rust engine takes
  main's rules (`ContractCallGuard::at_risk`, decimals read or refused).
- The Swift Windows engine took a Bluetooth Ledger as reachable whatever the search answered (a tuple compared with nil).
  The Rust engine asks for a real answer.
- The engine has no TWAP guardian yet (main's Mac stops a TWAP whose stop fired, c308a92): a TWAP is turned down before
  any plan is drawn, rather than started with nothing to stop it.
- About 120 of the engine's words still carry a «;» from the Swift Windows engine; main dropped them from its own UI.

Checked on a real Windows 11 ARM64 (the VM) at 62937d95: `cargo test --workspace --locked` passes 304 tests there, among them DPAPI, Windows Hello not faked, and the signer refusing a parent that isn't ihatecrypto-engine.exe from both its own side and the engine's. Again at 9431cab2, with ledger, trade and core round 5: 630 tests.

## For the engine

- `rust_decimal`'s operators panic on overflow where Swift's Decimal doesn't, and the release profile aborts on a panic:
  sums and products of numbers from someone's API (Kamino, Jupiter, a node) must not be able to take the engine down —
  checked arithmetic on outside numbers, or the engine's tasks isolated so one bad answer fails one read.

## Order

The signer first: it is small, it holds the keys, and everything it does is checkable offline. The engine after it,
from the leaves (pure parsing and math) to the chat.

| Step | Crate | Swift it replaces | State |
| --- | --- | --- | --- |
| 1 | `wallet` | `Wallet/Core`: BIP-39, BIP-32, SLIP-10, addresses, base58, Keccak, EIP-55, EIP-712, ECDSA, typed data, NEAR borsh, address checks | done, 26 tests |
| 2 | `hyperliquid` | `Protocols/Hyperliquid`: msgpack, actions, L1 and user-signed actions | done, 10 tests (the SDK's vectors) |
| 3 | `solana` | `Chains/Solana` offline: messages (v1 refused), transactions, SPL, program addresses, .sol names, account answers | done, 12 tests (mainnet accounts, a fuzz) |
| 4 | `evm` | `Chains/EVM` offline: RLP, EIP-1559/7702 transactions, ABI, the signable networks, calldata read back with its payees and refusals, Safe, CCTP, USDT0, Merkl, KyberSwap, Uniswap v2/v3/v4 with the vetted hooks, Permit3009, Relay's multicall permit, 7702 batches, eth_simulateV1 answers | done, 45 tests (EVMTests, the review's cases, KyberSwap and Uniswap calldata from cast, three live Relay order ids) |
| 5 | `signer-api` | `Signer/API`: the 42 requests and the replies, the 4-byte frame; shapes checked against JSON a Swift program wrote (tests/golden.json) | done, 7 tests |
| 6 | `guard` | `Guard/Core`: the Solana guard (balance rules, account rights, known programs, sponsor refunds), Hyperliquid actions, quote bindings, injection screening | done, 39 tests (GuardTests' real transactions) |
| 7 | `signer` | `Signer/Core` + `Signer/Windows`: every request's decisions (main's Signer.swift with the Windows lines), the vault under Windows Hello, the trusted settings, the door over pipes, the binary `ihatecrypto-signer`; winsec as a Rust library, not a DLL | done, 23 tests (VaultTests, AgentKeyTests, AgentFallbackTests, CodeTests, LedgerAgentKeyTests, the binary over its pipes); checks for `x86_64-pc-windows-msvc` |
| 8 | — | `ihatecrypto-signer.exe` from Rust in `build.ps1` and CI; the Swift engine talks to it unchanged | done: on this Mac the Swift engine imported a phrase, renamed the wallet and saved an address through the Rust signer; on the Windows 11 ARM64 VM the workspace builds and 151 tests pass, the three real-Windows ones too (DPAPI, Windows Hello not faked, the parent check) |
| 9 | `net` | `RPCRetry`, `SolanaRPC`, `EVMRPC`, the node lists, ENS, SNS: one HTTP client (the system's TLS), retries, the walk to the next node | done, 8 tests against a stand-in node + a live read-only check |
| 10 | `protocols` | `Protocols/Relay`, `OneClick`, `Jupiter` + `JupiterRecurring`: quotes, the payment-by-signature and v3.2 steps, 1Click tokens, quotes and statuses, Jupiter quotes, orders, execute, recurring orders | done, 16 tests (GaslessTests' Relay steps, OneClickTests, Jupiter's impact) + 1 live read-only |
| 10 | `llm` | `LLM/Core`, `APIKey`, `Anthropic`, `OpenRouter`, `ChatGPT`, `ClaudeCode`, `Catalog` (not Apple): requests, streaming, usage and context, OAuth and refresh, the fallback beta; logins handed in and out, never stored here, the Swift `credentials.bin` JSON read as is | done, 33 tests (LLMTests, stand-in servers, a fake `claude`) |
| 11 | `execution` | `Execution` without `Trade`: the envelope a person approves (limits, not a snapshot), the write-before-send journal (synced to disk before a send, 0600, unresolved sends, what went through, money in transit) | done, 7 tests (EnvelopeTests, JournalTests); reads the Swift envelope's JSON |
| 11 | `defi` | `DeFi`: Jupiter Lend Earn, Kamino (vaults, loans, obligations, klend's fixed point), yields, allocation and rebalance | done, 29 tests (AllocationTests, RebalanceTests, KaminoVaultTests, KaminoBorrowTests' Kamino parts, trimmed real answers) + 1 live read-only |
| 12 | `ihatecrypto-core` | all of `IhatecryptoCore`, in five rounds. 1: ChatLanguage, Folders, SiteCheck, Skills, TopUp, Guardian, AddressBookCommand, TakeStyle, FundingPlan, AgentCommands, ScheduledTask, MovedLine, Stablecoins, Tokens, StockNames, Hedge, Alerts, `foundation` (Swift's regexes and word bounds, Decimal printing, dates and UUIDs in Swift's JSON, checked math). 2: Intent, PositionCommand, ModelEdit. 3: Plan, Risk, Network, Modules, TokenResearch, Charts. 4: Normalize (the chat router's forms and prompts), ModelExam, ChatRecall, MPP, Blocks. 5, by the other session: Holdings, PnL, TradeHistory, TradeStudy, TradeJournal | done, 347 tests. main's own Swift parsers built beside the Rust ones: Intent and PositionCommand answered 4,213 test phrases identically, the router's form parsing a 300-phrase sample identically, and its prompts match the Windows branch's to the byte (this PC for this Mac); a number past the arithmetic refuses a plan (`plan::OUT_OF_RANGE`) where Swift got NaN |
| 13 | `engine` | `Desktop/Engine`, all of it. Its leaves: the winsec calls (the signer started hardened in the engine's job object, Windows Hello for the app's lock, time away, a locked session, letting the signer come forward) and `SignerLink`; `guard::jev` is the second check's request and answer, its POST the engine's. Layer 1: every file's types with their JSON, the engine's state (`State` in a `RefCell`, `Rc<Engine>` on one thread as Swift's MainActor), its files in the Swift engine's own shapes, the lock, the window's contract (`Snapshot`, `Request`, `Outbox`). Layer 2: each file's `extension Engine` as `impl Engine` in its module — the moves side over `trade` (moves, routes, checks, chains, the trade plan and guardian, the Ledger and its trading key, top up, earn, payments, multisigs, contract calls, the portfolio and blocks, schedule and automation, alerts, favorites, modules) and the chat side over core and llm (chat, its turns and commands, the agent, models, skills, canvases, the browser, the history, folders, notices). `dispatch` answers all 140 of the window's requests; `Engine::snapshot` draws the 83 keys the window reads; the binary `ihatecrypto-engine` | done, 333 tests (the data folder a person switching keeps reads and writes back the same; every op family; the binary answering a piped request) |
| 13 | `ledger` | `Wallet/Ledger`: APDUs, the dashboard, the Ethereum app (addresses, transactions in 150-byte pieces, EIP-712 field by field or by its two hashes, blind signing, token descriptors), the Solana app (addresses, messages in marked pieces, trusted names), Ledger's signed descriptors, Speculos over TCP, `LedgerSigner` (the guard first, then the device, then the signature checked to be this Ledger's), and Windows' USB and Bluetooth transports through winsec (its `ledger` and `ble` modules now public to Rust) | done, 15 tests (LedgerTests, the apps' commands against a scripted device, LedgerSigner against a software Ledger) |
| 13 | `trade` | `Execution/Trade`, all 26 files: the executor (Hyperliquid orders, closes, limits with their stop and take, half at 1R, trailing stops, TWAPs and the guardian), the price-impact and loss stops (CostlyMove, «part / anyway / cancel»), swaps by the best way (JupiterZ, Uniswap v2/v3/v4, Relay, KyberSwap; Robinhood stocks, Ondo, Tempo, NEAR Intents, cross-chain), sends (USDC, SPL, SOL, NEAR, any token, Tempo payments, zero-fee and gasless first moves), bridges (Relay, CCTP, 1Click, Across, USDT0 over LayerZero; money in transit), Aave, Morpho, Kamino loans and vaults, Jupiter Lend, HLP, rewards, dust, approvals, Hyperliquid spot, contract calls with their cap, the agent's reads, rebalance, the Safe multisig | done, 86 tests (ExecutorTests, ExecutorCloseTests, LimitFillTests, PartialTakeTests, TrailingStopTests, TwapExecutionTests, HedgeRiskTests, the swap, send, Kamino, approvals, contract-call and spot cases; a stand-in Hyperliquid, Solana node and web); a number past the arithmetic stops the move where Swift reached NaN; live and anvil-fork cases are left to the VM and the Dev copy |
| 14 | — | `build.ps1` and CI build `ihatecrypto-engine.exe` and `ihatecrypto-signer.exe` from Rust (no Swift runtime, no winsec.dll); the window driven by the Rust engine; the whole app on the Windows VM | done: a second check of e41fe655 on a clean copy, 1044 tests (one engine test failed every second run: trade kept Settings › Modules process-wide, and parallel test engines overwrote it; per engine thread since adbaccf6); the engine's first state has the Swift engine's shape, path for path, fresh and with a wallet (only Apple's model, which the Swift engine has on a Mac alone, and main's newer OpenAI list differ); the window on this Mac, hidden, imported the test phrase from its page and answered in the chat; on the Windows 11 ARM64 VM the window started the Rust engine and signer, the phrase went in through a real Windows Hello PIN prompt, the portfolio loaded and «chart SOL» drew its card; `build.ps1` there made `ihatecrypto_0.7.7_arm64-setup.exe` (11.7 MB, two programs in `bin\`) |

Shared modules are ported from `main`, not from this branch's older copy: steps 1–2 and on match main at `bc90340`
(trailing stops and TWAPs, Morpho's Authorization refused, address twins). The Mac app stays Swift; to catch up,
`git diff bc90340 main -- <the module>` shows what to port, with its tests, and the baseline here moves with it.
