# The Windows app in Rust

The Windows app is three programs (docs/windows.md): the window (Rust, Tauri), the engine and the signer (Swift, the
Mac's core built for Windows). This rewrites the engine and the signer in Rust, so Windows needs no Swift toolchain,
no Swift runtime beside the programs and no `#if os(Windows)` in the Mac's code.

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
| 12 | `ihatecrypto-core` | `IhatecryptoCore`, round 1 of 5: ChatLanguage, Folders, SiteCheck, Skills, TopUp, Guardian, AddressBookCommand, TakeStyle, FundingPlan, AgentCommands, ScheduledTask, MovedLine, Stablecoins, Tokens, StockNames, Hedge, Alerts, and `foundation` (Swift's regexes, word bounds, Decimal printing, dates and UUIDs in Swift's JSON) | round 1 done, 64 tests; next: Intent, PositionCommand, ModelEdit; then Plan, Risk, Network, Modules, TokenResearch; then Normalize; then Holdings, PnL, trade history |
| 12+ | engine crates | `Execution/Trade` (after core's round 3), `Desktop/Engine`, leaves first | |

Shared modules are ported from `main`, not from this branch's older copy: steps 1–2 and on match main at `bc90340`
(trailing stops and TWAPs, Morpho's Authorization refused, address twins). The Mac app stays Swift; to catch up,
`git diff bc90340 main -- <the module>` shows what to port, with its tests, and the baseline here moves with it.
