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

## Order

The signer first: it is small, it holds the keys, and everything it does is checkable offline. The engine after it,
from the leaves (pure parsing and math) to the chat.

| Step | Crate | Swift it replaces | State |
| --- | --- | --- | --- |
| 1 | `wallet` | `Wallet/Core`: BIP-39, BIP-32, SLIP-10, addresses, base58, Keccak, EIP-55, EIP-712, ECDSA, typed data, NEAR borsh, address checks | done, 26 tests |
| 2 | `hyperliquid` | `Protocols/Hyperliquid`: msgpack, actions, L1 and user-signed actions | |
| 3 | `solana` | `Chains/Solana`: message parsing, SPL, transactions | |
| 4 | `evm` | the parts of `Chains/EVM` the signer reads: RLP, EIP-1559/7702 transactions, calldata | |
| 5 | `signer-api` | `Signer/API`: requests and replies, byte for byte the Swift JSON | |
| 6 | `guard` | `Guard/Core`: the transaction guard, injection screening | |
| 7 | `signer` | `Signer/Core` + `Signer/Windows`: the signer's decisions, the vault under Windows Hello, the trusted settings; winsec as a Rust library, not a DLL | |
| 8 | — | `ihatecrypto-signer.exe` from Rust in `build.ps1`; the Swift engine talks to it unchanged | |
| 9+ | engine crates | `IhatecryptoCore`, `Protocols`, `DeFi`, `Execution`, `LLM`, `Desktop/Engine`, leaves first | |

The Mac app stays Swift. After a main merge, a change to a ported Swift module is ported here too, with its tests.
