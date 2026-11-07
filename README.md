<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="images/hero-dark.png">
    <img src="images/hero-light.png" width="800" alt="ihatecrypto. Even crypto hates crypto. Say it. See the move. You confirm. Below, the app window: the portfolio on the left, «buy $10 of Tesla» laid out as cards on the canvas (10 USDC from Solana, bridged with Relay, approved and swapped for TSLA on Robinhood Chain) and the chat below">
  </picture>
</p>

<p align="center">
  Say what you want done with your crypto. ihatecrypto lays the move out as cards, and nothing moves until you confirm with Touch ID.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue"></a>
  <img alt="Platform: macOS 26" src="https://img.shields.io/badge/platform-macOS%2026-lightgrey">
  <a href="https://ihatecrypto.ai"><img alt="Site: ihatecrypto.ai" src="https://img.shields.io/badge/site-ihatecrypto.ai-FF7A2F"></a>
</p>

ihatecrypto is a self-custody Mac wallet you talk to. Write «buy $10 of Tesla» or «put my USDC where it earns most»
in English, Spanish or Russian. It looks up prices and balances, picks a route across networks and shows where the
money comes from, how it gets there and what you end up with. The model never holds a key: a separate signing
process checks every transaction in code and signs only after you press Confirm.

## NEAR HACKATHON (November 1–14, 2026)

ihatecrypto existed before the hackathon: swaps, bridges and sends through NEAR Intents' 1Click and a NEAR account
derived from the wallet's phrase are baseline (tag `pre-hackathon`). Everything below was built in the window, on the
branch `near-hackathon`: [the whole diff](../../compare/pre-hackathon...near-hackathon). Tracks: Privacy & Private AI,
AI Security & Defense, Agent Economy, Real-World Money, Unstoppable DeFi.

| What | NEAR's role | Try it (Dev build, see Install) | Check without a wallet |
| --- | --- | --- | --- |
| **Private sends** | Confidential Intents: on chain the deposit and the payout can't be matched | «send 2 USDC privately to Mom» → a card «PRIVATE · CONFIDENTIAL INTENTS» → Confirm → Mom gets exactly 2 USDC. Any USDC send to a saved address offers «Regular \| Private» over Confirm, with both ways' fees and times | `swift test --filter privateSend` |
| **Private swaps** | Confidential Intents swaps between this wallet's own addresses | «buy NEAR for 1 USDC privately» or «обменяй приватно 1 USDC на NEAR» → a card «PRIVATE SWAP · CONFIDENTIAL INTENTS» → Confirm → NEAR on your NEAR account. Refused before anything is sent unless 1Click itself holds the deposit as private | `swift test --filter privateSwap` |
| **Private chat model** | NEAR AI Cloud runs the model in Intel TDX + NVIDIA enclaves | Pick «NEAR AI Cloud (private)» → every message waits for the enclave check → each reply's exact bytes are checked against the model key the attestation verified → the chat's header reads «Private assistant» with a lock, and Proof shows what was checked | `swift run LLMCheck nearai --attest` (no key: checks the live enclaves down to Intel's root) |
| **«Won't let you get liquidated»** | The USDC a repay lacks comes only through NEAR Intents, from whichever network has it | «guard my loan on kamino below 1.6» → under the line «move N USDC from Base to Solana via NEAR Intents» and a repay back to 1.8 wait for one Confirm | `swift test --filter 'LoanGuard\|aGuardsTopUp\|viaNEARIntents'` |
| **Pay any API: MPP through NEAR Intents, and x402** | MPP's NEAR Intents charge ([draft](https://paymentauth.org/draft-nearintents-charge-01)): the service's 402 names a 1Click deposit address, and 1Click delivers exactly what the service asked on its own network | «pay GET <a service that takes MPP's nearintents charge>» → a card «PAY · NEAR INTENTS» saying what the service receives, where, and where a refund goes → Confirm. Before signing, 1Click itself must hold the deposit for the service's recipient, its exact output and refund address, at no more than 1Click's own price. Then the request goes again with the deposit's hash. x402: «pay GET https://api.402rates.com/v1/ping» → an EIP-3009 signature, Base's USDC brought from Solana through NEAR Intents when short | `swift test --filter 'nearIntentsCharge\|anMPPCharge\|tempoComesFirst\|x402\|X402'` |
| **Agent wallet** | Agent API: NEAR Intents enforces the owner-signed policy | «create an agent wallet with $20 a day» → your NEAR key signs the rules (NEP-366) and a grant (NEP-413) → «send 1 USDC from the agent wallet to Mom» goes without your key; over the limit NEAR Intents refuses | `swift test --filter 'agentWallet\|NearOwnerSigning\|agentOwner'` |
| **Every move through NEAR Intents, after Confirm** | 1Click's own states | The chat field shows the money's way: paid on the source network, a solver filling it on NEAR, landed, with the time left and the move on NEAR Intents' explorer. Refunds and «still on its way» are said as such | `swift test --filter 'Journey\|aRefundIsSaid'` |

The signer checks what it signs itself: a private send or swap goes out only when 1Click itself holds the deposit as
confidential. The agent wallet's policy goes on chain encrypted for NEAR Intents' custody (`store_wallet_policy` on
`outlayer.near`), so the signer can't read it there: it signs only that method, only to that contract, with at most
0.05 NEAR for storage (said in words, and that its refund goes to NEAR Intents' relayer), and the app signs only when
the policy the API previews is the one asked, under the hash the API names (sha256 of its JSON with sorted keys); after
it's applied, the applied hash must be that one.

Run live on mainnet (2026-11-02…07): a private send (2.02 Solana USDC → exactly 2 USDC on Base in 89 s), an x402 call
paid by signature, the NEAR AI reply signature check (`teeReply: model` in the UI test), a move «via NEAR Intents»
(6.19 USDC Solana → Base in 99 s), the loan guard's repay, the agent wallet funded, paying its own wallet and frozen,
a private swap (1 USDC on Solana → 0.26964 NEAR in 100 s), and an MPP NEAR Intents charge to a test merchant on
NEAR's own SDK (`@defuse-protocol/nearintents-mpp-sdk`): 1.020228 USDC from Base, 1 USDC delivered on Solana, the
service's receipt back.

**Not yet:** the agent wallet's grant lives on this Mac only. The on-chain `agent_policy_hash` is the custody's own hash
of the encrypted policy and isn't recomputed here. MPP's directory lists no service taking the NEAR Intents charge yet,
so the live run used a merchant of our own.

## What it does

<table>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="images/route-dark.png">
        <img src="images/route-light.png" width="100%" alt="A move as a row of cards: the source, the bridge, an exact approve and the buy, each with the balance now and after">
      </picture>
    </td>
    <td width="50%" valign="top">
      <h3>Say it, see the route</h3>
      <p>Every move is a row of cards: the source, each bridge and swap with its fee, and the balance after. Set the
      amount with the slider over the chat field. A move the wallet can't pay for is turned down before any card
      appears.</p>
      <h3>Keys stay on the Mac</h3>
      <p>The wallet is made on the Mac and sealed with a Secure Enclave key. Signing runs in its own process with no
      network: it parses each transaction, refuses unlimited approvals, permits and blind signatures, and asks for
      Touch ID. A Ledger and a Safe multisig work too.</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="images/trade-dark.png">
        <img src="images/trade-light.png" width="100%" alt="A Hyperliquid trade plan with entry, stop, take and the loss at the stop">
      </picture>
    </td>
    <td width="50%" valign="top">
      <h3>Trading with rules</h3>
      <p>Hyperliquid perps on crypto, oil and stocks, sized so a stop costs at most 2% of your capital (you can change
      the limit in Settings). The stop and the take go in with the order. Price alerts and scheduled buys keep
      running from the menu bar when the window is closed.</p>
      <h3>Bring your own model</h3>
      <p>A ChatGPT Plus or Pro plan, a Claude plan through Claude Code, OpenRouter, an API key, or a model on your Mac
      through Ollama. ihatecrypto has no model of its own, and your chat goes from the Mac to the model you pick.</p>
    </td>
  </tr>
</table>

**Networks:** Solana, Hyperliquid, Ethereum, Base, Arbitrum, Optimism, Polygon, BNB Chain, HyperEVM, Robinhood Chain,
Tempo and NEAR; Bitcoin and others through NEAR Intents.
**Protocols:** Jupiter, Uniswap, Relay, NEAR Intents, CCTP, USDT0, Aave, Kamino, Jupiter Lend, JitoSOL, Morpho and
Hyperliquid's HLP.

## Install

**Download:** get the DMG from [Releases](../../releases/latest) or [ihatecrypto.ai](https://ihatecrypto.ai) and drag
ihatecrypto into Applications. It's free and runs on macOS 26 or later. Every version is on the
[Releases](../../releases) page with its notes and DMG. Updates come through Sparkle from updates.ihatecrypto.ai,
signed with the project's EdDSA key.

**Build from source** (Xcode 27; the app runs on macOS 26 and later):

```sh
cd App
swift build && swift test             # every module and its tests; no Apple account needed
./run.sh                              # the app without its signing process: it shows balances but can't sign
TEAM=<your team id> ./build-app.sh    # the full app with Signer.xpc, signed by your team (needs XcodeGen)
```

`build-app.sh` makes «ihatecrypto Dev.app» with its own bundle id, Keychain group and data folder, so it never
touches an installed ihatecrypto.app or its wallets.

**Check a release** against the code: `App/verify-release.sh <version>` rebuilds that tag and compares it with the
release's DMG file by file, apart from code signatures; the **Verify release** workflow does the same on GitHub for
every release. See [SECURITY.md](SECURITY.md#how-it-is-built).

**Windows** (a preview on the `windows` branch): the same core ported to Rust under a Tauri window, with keys sealed by
Windows Hello in a separate, hardened signing process. How it works, how to build it and what it doesn't do yet:
[docs/windows.md](docs/windows.md).

<details>
<summary>Details for contributors</summary>

**Layout:** one folder per SwiftPM module under `App/Sources`: `Wallet/Core` (BIP-39, keys, EIP-712), `Chains/*`,
`Protocols/*`, `DeFi/*`, `Guard` (the last check before signing), `Signer` (the XPC signing process), `Execution`
(runs an approved plan), `IhatecryptoCore` (commands, plans and risk rules), `LLM/*` (one folder per way to reach a
model) and `IhatecryptoApp` (the window). See [docs/architecture.md](docs/architecture.md).

**Live checks:** `swift run WalletCheck …` runs against real services with a throwaway wallet whose phrase sits in
`~/.ihatecrypto/test-wallet.txt`. Nothing is sent without `--yes`. Keep small amounts there.

**Debug API** (development builds only, never in a release): Settings › Debugging › Local debug API turns on a local
HTTP API (127.0.0.1, a bearer token that changes on every start) that the UI tests drive (`Tools/uitest.py`). It reads
the state and types into the chat; it never signs, so a move still waits for Confirm and Touch ID.

Run `./lint.sh` and `swift test` before sending changes. See [CONTRIBUTING.md](CONTRIBUTING.md), and
[AGENTS.md](AGENTS.md) if you work with a coding agent.

</details>

## Before you use it

ihatecrypto moves real money and has not been audited. Start with small amounts. Nothing in it is financial advice.

Three things sign without Touch ID, and each is set up once with Touch ID: a test wallet, an automation wallet for
scheduled tasks, and the Hyperliquid trading key, which places orders but can't withdraw. Keep only small amounts on
the first two.

The app talks to RPC nodes, to the services a move goes through (Relay, Jupiter, NEAR Intents, Hyperliquid and
others), to price and token sources such as DexScreener, DefiLlama and CoinGecko, to the update feed, and to the model
you connect. Each of them sees your IP address, and the nodes see the addresses they're asked about. The app has no
analytics. The site ihatecrypto.ai counts page visits and downloads by day, language and device, without IP addresses,
cookies or ids.

Report security issues privately: see [SECURITY.md](SECURITY.md).

## Credits

- [Sparkle](https://github.com/sparkle-project/Sparkle) for updates and
  [swift-secp256k1](https://github.com/21-DOT-DEV/swift-secp256k1) (libsecp256k1) for signatures, both MIT.
- [DynaPuff](https://fonts.google.com/specimen/DynaPuff) and [Onest](https://fonts.google.com/specimen/Onest), SIL Open
  Font License; the licenses are in `App/Sources/IhatecryptoApp/Fonts`.
- [three.js](https://github.com/mrdoob/three.js) on the landing page, MIT.
- Token logos from CoinGecko. Each token and network logo belongs to its project.

## License

The code is under the [GNU GPL v3](LICENSE). The ihatecrypto name, logo and coin mascot are not covered by it. A fork
needs its own name and icon.
