---
name: live-run
description: Runs a money feature for real on the Dev copy's test wallet — the chat phrase, the card it draws, Confirm pressed through Accessibility, the outcome and the balances after — with the few-dollar rules. Use for the live run every money feature needs before it's done; when the user says "проверь вживую", "live run", "прогони на тестовом кошельке", "check it really works", or after a new swap, deposit, borrow, bridge or transfer path.
---

# A live run on the Dev test wallet

Money moves only after Confirm, and an automated run presses Confirm **only in the Dev copy, only on its test wallet**.
`App/Tools/live.sh` refuses anything else (another app, another wallet, a busy copy); `App/Tools/axpress.swift` finds
the window by the Dev copy's bundle id alone. Never sign from a main wallet; never press Confirm in the installed app.

## Steps

1. `Tools/dev.sh state` — the Dev copy runs a build with your change (`Tools/dev.sh relaunch --build` if not), the
   active wallet is the test wallet, nobody else uses it. Note what the test wallet holds: a few dollars, so size the
   run at $1–5 and leave the wallet able to pay its fees.
2. Draw the card first, without pressing anything: `DRY=1 Tools/live.sh "<phrase the user would type>"`. Read the
   title, steps and quote: they are what the user will Confirm. A wrong card is a bug to fix before money moves.
3. The run: `Tools/live.sh "<phrase>"` presses Confirm, waits until nothing executes, prints every new chat message and
   the outcome. More buttons after it, in order, by a part of their title:
   `Tools/live.sh "deposit 5 USDC to kamino" Confirm "Send anyway"` (a CostlyMove asks «Send anyway · ≈ $3.10 lost»).
4. Check on chain, not only in the chat: the reply's explorer link, the balances in `Tools/dev.sh state`, the
   protocol's own API for the position (e.g. Kamino `GET /kamino-market/<market>/users/<wallet>/obligations`).
5. Undo what can be undone (withdraw, repay, sell back) with the app itself, the same way, so the test wallet keeps
   its dollars where they were. Say in the final report what stayed (rent, a fee, dust) and where.

## Pitfalls

- `axpress` needs the terminal's Accessibility right (System Settings › Privacy & Security › Accessibility). Several
  buttons can be titled «Confirm» (under the chat's reply and on the canvas): they are the same card.
- `swift run WalletCheck …` uses `~/.ihatecrypto/test-wallet.txt`, an older wallet that may not exist on this Mac.
  Live runs go through the Dev copy; WalletCheck stays for simulations (`kamino …` without `--yes` only simulates).
- A run longer than the shell's 10-minute limit: one phrase per call, or `run_in_background`.
- Nothing in the run goes into the repository: no wallet addresses, signatures or balances in code, tests, commits or
  docs. Tests use the public test phrases («abandon … about», Hardhat's «test … junk»).
