# NEAR Hackathon (November 1–14, 2026)

ihatecrypto existed before the hackathon: swaps, bridges and sends through NEAR Intents' 1Click and a NEAR account
derived from the wallet's phrase are baseline (tag `pre-hackathon`). Everything below was built in the window, on the
branch `near-hackathon`: [the whole diff](https://github.com/ihatecryptoai/ihatecrypto/compare/pre-hackathon...near-hackathon). The branch
carries `main` up to the day of the latest merge and the Windows port, so the same NEAR pieces run on Windows too. Tracks: Privacy & Private AI,
AI Security & Defense, Agent Economy, Real-World Money, Unstoppable DeFi.

| What | NEAR's role | Try it (Dev build, see [Install](../README.md#install)) | Check without a wallet |
| --- | --- | --- | --- |
| **Private sends** | Confidential Intents: on chain the deposit and the payout can't be matched | «send 2 USDC privately to Mom» → a card «PRIVATE · CONFIDENTIAL INTENTS» → Confirm → Mom gets exactly 2 USDC. Any USDC send to a saved address offers «Regular \| Private» over Confirm, with both ways' fees and times | `swift test --filter privateSend` |
| **Private swaps** | Confidential Intents swaps between this wallet's own addresses | «buy NEAR for 1 USDC privately» or «обменяй приватно 1 USDC на NEAR» → a card «PRIVATE SWAP · CONFIDENTIAL INTENTS» → Confirm → NEAR on your NEAR account. Refused before anything is sent unless 1Click itself holds the deposit as private | `swift test --filter privateSwap` |
| **Private chat model** | NEAR AI Cloud runs the model in Intel TDX + NVIDIA enclaves | Pick «NEAR AI» (marked Private) in the model menu → every message waits for the enclave check → each reply's exact bytes are checked against the model key the attestation verified → the chat's header reads «Private assistant» with a lock, and Proof shows what was checked | `swift run LLMCheck nearai --attest` (no key: checks the live enclaves down to Intel's root) |
| **«Won't let you get liquidated»** | The USDC a repay lacks comes only through NEAR Intents, from whichever network has it | «guard my loan on kamino below 1.6» → under the line «move N USDC from Base to Solana via NEAR Intents» and a repay back to 1.8 wait for one Confirm | `swift test --filter 'LoanGuard\|aGuardsTopUp\|viaNEARIntents'` |
| **Pay any API: MPP through NEAR Intents, and x402** | MPP's NEAR Intents charge ([draft](https://paymentauth.org/draft-nearintents-charge-01)): the service's 402 names a 1Click deposit address, and 1Click delivers exactly what the service asked on its own network | «pay GET <a service that takes MPP's nearintents charge>» → a card «PAY · NEAR INTENTS» saying what the service receives, where, and where a refund goes → Confirm. Before signing, 1Click itself must hold the deposit for the service's recipient, its exact output and refund address, at no more than 1Click's own price. Then the request goes again with the deposit's hash. x402: «pay GET https://api.402rates.com/v1/ping» → an EIP-3009 signature, Base's USDC brought from Solana through NEAR Intents when short | `swift test --filter 'nearIntentsCharge\|anMPPCharge\|tempoComesFirst\|x402\|X402'` |
| **Agent wallet** | Agent API: NEAR Intents enforces the owner-signed policy | «create an agent wallet with $20 a day» → your NEAR key signs the rules (NEP-366) and a grant (NEP-413) → «send 1 USDC from the agent wallet to Mom» goes without your key; over the limit NEAR Intents refuses | `swift test --filter 'agentWallet\|NearOwnerSigning\|agentOwner'` |
| **Every move through NEAR Intents, after Confirm** | 1Click's own states | The chat field shows the money's way: paid on the source network, a solver filling it on NEAR, landed, with the time left and the move on NEAR Intents' explorer. Refunds and «still on its way» are said as such | `swift test --filter 'Journey\|aRefundIsSaid'` |
| **Private limit orders** | 1Click's orders (`/v0/orders`): an order rests with NEAR Intents and fills with no signature then; 1Click takes only confidential orders | «limit buy SOL at 120 for 20 USDC» or «выставь лимитку: купи SOL по 120 на 20 USDC» → a card with the market beside the price → Confirm → one deposit, and the order waits up to 7 days (or «for 3 days») and fills when the market gets there, in parts too. «show limit orders» draws a board with each order against 1Click's own price and Cancel per order; a fill or an expiry is said in the chat and as a notification. Before the deposit the order is read back from 1Click and checked whole (pair, side, amount, price, our payout and refund address, private, fees) | `swift test --filter LimitOrder` |
| **NEAR on Windows** | The same as on the Mac | The Windows preview has private sends and swaps, the NEAR AI enclave check, the agent wallet with its owner signatures, the loan guard, x402 and MPP through NEAR Intents, and the money's way after Confirm | [docs/windows.md](https://github.com/ihatecryptoai/ihatecrypto/blob/near-hackathon/docs/windows.md) |

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

Run live again for the demo video and after it (2026-11-10…12, the Dev copy's test wallet): borrow 1 USDC on Kamino
against SOL and guard the loan in one Confirm; under its line the guard brought 1.45 USDC from Base to Solana through
NEAR Intents in 103 s and repaid. The agent wallet tried to send $25 against its $20 rule and NEAR Intents refused it
(`policy_transaction_limit_exceeded`). A private send of 2.03 USDC from Solana paid out exactly 2 USDC to another wallet
of the owner's in 158 s. A limit buy of SOL rested with 1Click and filled: 0.6979 USDC in, 0.008247 SOL arrived. The
guard now watches Kamino's main market (the loan against SOL) as well as Backpack's, and an agent send that timed out is
asked again under the same key instead of reading «status unknown».

**Not yet:** the agent wallet's grant lives on this Mac only. The on-chain `agent_policy_hash` is the custody's own hash
of the encrypted policy and isn't recomputed here. MPP's directory lists no service taking the NEAR Intents charge yet,
so the live run used a merchant of our own.
