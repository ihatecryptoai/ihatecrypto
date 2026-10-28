---
name: add-defi-protocol
description: Adds a DeFi protocol or a new money command to ihatecrypto end to end — the protocol's API and transactions checked first, the guard and the signer, the executor with its simulation rules, the chat form and parser in English and Russian, the card, the portfolio, tests, a uitest scenario and a live run on the Dev test wallet. Use when the user says "add <protocol>", "support lending/borrowing on X", "сделай чтобы можно было <move> на X", "подключи протокол", "new market on Kamino/Jupiter/Aave", or forwards news about a protocol feature to make work in the app.
---

# A protocol or a money command, end to end

Templates in the history — read their diffs first (`git show --stat <sha>`, then `git show <sha> -- <file>`):
`git log --oneline --grep 'Aave' --grep 'Kamino' --grep 'Morpho' --grep 'JupiterZ' --grep 'KyberSwap'`.
The Aave borrow (`AaveBorrow.swift`, `aaveDebtRoute`) is the template for a loan; Kamino's deposit (`Executor.kamino`)
for a Solana lending protocol; JupiterZ/KyberSwap for a new swap way.

## 1. Facts first, from the protocol itself (guess nothing)

- Find the protocol's API spec: Swagger pages load a JSON one (Kamino: `GET /openapi/json?openapi=3.0.0`). Undocumented
  transaction builders exist too (Kamino `POST /ktx/klend/{deposit,withdraw,borrow,repay}` with
  `{"wallet","market","reserve","amount"}`): probe them with the real test wallet's public address.
- Cloudflare answers 403 to Python's default user agent: send `User-Agent: curl/8`.
- Decode what the builder returns before any code signs it: `python3 -I App/Tools/soltx.py tx.b64 <owner>
  [anchor_instruction_name …]` lists programs, Anchor instructions by name and accounts (LUT#n = from a lookup table).
- Read the program's verified source for each instruction you'll allow: who it can pay (`token::authority = owner`,
  `has_one = owner`), which account pins the user's own position (an obligation PDA, a user account), and what an
  attacker-built variant could do (pay another account, act on another market, a referrer taking fees).
- Token facts: SPL Token or Token-2022 (`getAccountInfo` on the mint: its owner), extensions (permanent delegate,
  pausable, transfer hook, scaled UI amount — UI amount = raw × multiplier), decimals; can a plain wallet hold it
  (no KYC allow-list: `defaultAccountState` initialized), and is there a route to buy it (`lite-api.jup.ag/swap/v1/quote`).
- Risk numbers for the card: max LTV, liquidation LTV, borrow factor, caps (Kamino `GET /reserves/<reserve>/stats`),
  rates, rent of the first use, minimums. Write the date of the check next to each constant.

## 2. The guard and the signer (safety lives in code)

`App/Sources/Guard/Core/Guard.swift`, `SolanaGuard`:
- The program goes in `knownPrograms`; only the instructions ihatecrypto uses go in an allow-list by discriminator,
  each with the account index that must be the user's own position or token account (`kaminoLendAllowed`,
  `ownTokenAccount`, a PDA derived here from the owner — never an address the app passes). A new market is a new
  PDA the guard derives, not a looser check.
- The signer must not trust the app: whatever it can verify itself (a PDA, an ATA, a payee) it derives. Tests in
  `App/Tests/WalletTests/GuardTests.swift`: the allowed transaction passes; the same one paying someone else, acting
  on another position or with an unknown instruction is refused.
- EVM: an `EVMCall` case and its `refusal(sender:)` in `Chains/EVM/EVM.swift`, the signer's check in
  `Signer/Service/Signer.swift`, a fork test (`Tools/fork7702.sh`).

## 3. The protocol module and the executor

- `App/Sources/DeFi/<Protocol>/` (a target in `Package.swift` for a new folder): the API client — reads, unsigned
  transactions. Plain `Decimal`s and `String`s across modules.
- `App/Sources/Execution/Trade/…`: the move. Read the position, refuse before signing what can't work (say why and
  the working way, «Nothing was signed»), then `signAndSend(wire, …, rules:, lamportsAtLeast:)`: the simulation rules
  say what must arrive and the most that may leave (fees, first-use rent). A Token-2022 token's account derives from
  that program: `SPL.associatedAccount(owner:mint:program:)`. Report what happened with the explorer link.
- A cost worth asking about (rent over 10% of the amount, price impact) is a `CostlyMove`: the chat asks «Send anyway?»
  instead of refusing.
- A loan: ihatecrypto's own limit above the protocol's (Aave: health ≥ `Executor.minimumHealth` 1.5), the
  liquidation price on the card, repay of the user's own debt only, and every way out (repay, withdraw) built in the
  same change — dollars that can't come back are a dead end.

## 4. The chat, the card, the portfolio

- `IhatecryptoCore/Normalize.swift` `ChatRouter.forms`: one English form per move, with Russian examples
  («займи 2 USDC под SPCX на камино» = …). `IhatecryptoCore/PositionCommand.swift`: the `WalletCommand` case, its
  parser (English and Russian regexes — order matters: a specific protocol before a generic «borrow»), the question
  when the amount is missing, `with(amount:)`, the form text it prints back. Tests in both languages.
- Build: the «switch must be exhaustive» errors list the rest of the places (Modules, LocalSigner, Canvases,
  Execution, Blocks, Browser, Wallets).
- `IhatecryptoApp/Execution.swift`: the dispatch to the executor; `refusalBeforeCard` for what the balances already
  show can't work. `IhatecryptoApp/Canvases.swift`: the route card — steps FROM → protocol → TO, numbers quoted while
  the card waits (the `swapQuote` loop, every 30 s), a warning line for the risk. Design rules: everything as cards
  in the chat, nothing disappears from the canvas.
- The portfolio: money put into the protocol stays visible — `Workspace` refresh and `PortfolioNetworks` lines (a
  debt shown and subtracted, so the total stays true).
- Settings › Modules (`IhatecryptoCore/Modules.swift`) when it's a new protocol to switch off.

## 5. Done

The `ship-change` skill (scenario first, `App/done.sh`, review, commit own files) and the `live-run` skill: the move,
then its undo, on the Dev test wallet with a few dollars.
