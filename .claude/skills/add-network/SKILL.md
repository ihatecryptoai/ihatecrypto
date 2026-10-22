---
name: add-network
description: Adds a new network (blockchain) to ihatecrypto, from checking the network's facts to its icon, its row in Settings › Modules, the Relay bridge, transfers, the portfolio, chat commands and a Release build. Only for a network an issue or the maintainers asked for. Use when the user says "add a network", "add chain", "support <chain>", "add network X", "подключи сеть", "добавь сеть X", or asks to make a new EVM chain work in the app (bridge, balance, transfers, payments).
---

# A new network in ihatecrypto

**Open an issue first.** Maintainers decide which networks to add. Don't propose or start one on your own.

Done means: the network is in Settings › Modules with its own icon, money moves Solana ⇄ the network through Relay,
the balance shows in the portfolio, addresses on the network can be saved and take transfers, the chat understands the
network in English and Russian, tests cover it, and a Release build from committed code works.

Past examples in the history: the commits that added Polygon and Ethereum (RPC, USDC, gas, the signer), Tempo (gas
paid in a stablecoin, MPP payments, icons, the switch in Modules) and BNB Chain. Find them with
`git log --oneline --grep 'Polygon and Ethereum as networks' --grep 'Tempo, Stripe' --grep '^BNB Chain'` and read their
diffs before you start: `git show --stat <sha>`, then `git show <sha> -- <file>`.

## 1. Facts about the network first (guess nothing)

Check everything with live requests and write the date of the check in a comment next to the constant.

```bash
R=<rpc>
cast chain-id --rpc-url $R; cast gas-price --rpc-url $R
cast balance <any address> --rpc-url $R   # the native balance; on Tempo it is a placeholder 4242…42
curl -s https://api.relay.link/chains | python3 -c "import json,sys; [print(c['id'],c['name'],c.get('httpRpcUrl'),c.get('explorerUrl'),c.get('currency',{}).get('symbol'),[(t['symbol'],t['address']) for t in c.get('erc20Currencies') or []],[(t['symbol'],(t.get('metadata') or {}).get('logoURI')) for t in c.get('featuredTokens') or []]) for c in json.load(sys.stdin)['chains'] if '<name>' in c['name'].lower()]"
```

Answer for yourself:
- **Chain id, RPC, explorer.** The RPC must serve `eth_call`, `eth_estimateGas` and receipts without a key. Public
  nodes have rate limits; drpc.org usually works.
- **Which dollar.** The USDC address (or USDG, USDC.e) that Relay delivers. Check `decimals()` and `symbol()` with
  `cast call`.
- **What pays for gas.** A native coin (ETH, POL) needs `gasFloor`, `topupUSD` and `gasCoin`. No native coin (Tempo)
  needs `feeToken`: how the network picks the fee token, what `eth_getBalance` returns, and whether a one-time call
  such as `FeeManager.setUserToken` is needed.
- **The bridge.** A Relay quote both ways (`/quote/v2`, 2 USDC): which steps go toward Solana (`approve` and
  `depositErc20` to the depository from `/chains`), the fee and the time. A few lines of Python against `/quote/v2`
  are enough.
- **Blockscout.** Whether the network has an instance. If not, read the balance over RPC, as `Holdings.tempo` does.

## 2. Checklist (every place each network touched)

**Network and signer**
- [ ] `App/Sources/Chains/EVM/EVM.swift`: `EVMRPC.<net>` and `EVMRPC.<net>USDC`, and the chain id in
  `EVMRPC.signable`. Without it the signer refuses: «ihatecrypto doesn't sign for network N».
- [ ] `App/Tests/WalletTests/EVMTests.swift` → `signableNetworks()`: update the expected set.
- [ ] `App/Sources/Protocols/Relay/Relay.swift`: `Relay.Chain.<net>`.
- [ ] `App/Sources/Execution/Trade/Aave.swift`: `EVMNetwork.<net>` (name, key, rpc, usdc, relayChain, explorer,
  gasFloor, topupUSD, gasCoin, feeToken), added to `EVMNetwork.all`. `ensureEVM`, `sendEVM`, `solanaToEVM` and
  `evmToSolana` depend on it. If gas isn't paid in a native coin, check three places: `ensureEVM` (don't read
  `eth_getBalance`), `sendEVM` (the fee check in the token) and `evmToSolana` («all» keeps a `feeReserve`).
- [ ] New selectors in calldata (for example `transferWithMemo`) are decoded in `EVMCall.decode`, so the signer checks
  the recipient. Otherwise the call goes as `.contract` with no recipient check.

**The network list and icons (always an icon)**
- [ ] `App/Sources/IhatecryptoApp/Logos/<net>.png`: 256×256 RGB with no transparency (a transparent black logo
  disappears on the dark theme). Take Relay's logo (`https://assets.relay.link/icons/<chainId>/light.png`) and put a
  white background under it with PIL. You don't add the file to Xcode yourself: `build-app.sh` runs `xcodegen`.
- [ ] `App/Sources/IhatecryptoCore/Modules.swift`: `ModuleNetwork` (the case, `name`, `uses`, `logo`). Add the network
  to the `ModuleProtocol.relay.place` line. If the network has a feature of its own (like MPP on Tempo), add a
  `ModuleKind`/`ModuleProtocol` with a logo and route `refusal(_ command:)` through it.
- [ ] `App/Sources/IhatecryptoApp/ModulesUI.swift` → `icon(_ k:)`: an SF Symbol for a new `ModuleKind`, if there is one.
- [ ] `App/Sources/Wallet/Core/AddressCheck.swift`: `Network` (the case, `title`, the address format in `problem`).
- [ ] `App/Sources/IhatecryptoApp/AddressBook.swift`: `AddressCheck.Network.logo`, the question «Which network is this
  0x address on…», the `Destination` in `transfer(...)`, and the transfer card's «from» balance.
- [ ] `App/Sources/IhatecryptoApp/Studio.swift` → `HoldingRow.chainLogos`: the network badge on a token in the portfolio.
- [ ] `App/Sources/IhatecryptoApp/Canvases.swift`: `RouteFill` (the case and its `color`), `outRoute`/`backRoute` (take
  the logo through `ModuleNetwork(place:)?.logo`, never a hardcoded «eth»), the gas text.
- [ ] `App/Sources/IhatecryptoApp/RouteCards.swift`: `RouteFill.accent` for the new color, and `places` (a «TO · <NET>»
  label means the user's own wallet, not a payment to someone else).
- [ ] Token icons in the portfolio: `Holding.icon` needs a URL, or the token shows a lettered circle. Take
  `featuredTokens[].metadata.logoURI` from Relay's `/chains`.

**Gas is never a dead end**
- [ ] For a network with a native coin (`feeToken == nil`), `EVMNetwork` sets `gasFloor` and `topupUSD`. Check that
  `Relay.nativePrice(chainId:)` returns the coin's price and that a Solana → network quote with `topupGas` delivers
  gas. `sendEVM` buys gas by itself before a transaction that lacks it (`topUpGas`: from Solana, or else a Relay swap
  of the network's USDC into the coin).
- [ ] The network is in the `add gas on <…>` form (PositionCommand: the network's words in the gas parser, Normalize,
  the «GAS · TOP UP» block). Check: `WalletCheck move "add gas on <network> for 1 USDC"` and a quote with `topupGas`,
  both without errors.
- [ ] A network without a native coin (like Tempo) uses `feeToken`, not gas; `add gas` there answers that there is
  nothing to top up.

**Money and balance**
- [ ] `App/Sources/Execution/Trade/Send.swift`: `Destination.<net>` → `sendEVMUSDC(units, on: .<net>, …)`.
- [ ] `App/Sources/IhatecryptoCore/Holdings.swift`: the host in `blockscout[...]`, or a branch that reads over RPC.
  Then add it to `Workspace.loadHoldings` (the `read` tuple).
- [ ] If the cards need it, `Workspace.<net>USDC` (like `tempoUSDC`).

**Blocks (always)**
- [ ] `App/Sources/IhatecryptoCore/Blocks.swift`: the network gets its own group in `BlockCatalog.groups` (named after
  the network) with every variant in it: the bridge there and back, a transfer to a saved address, a monthly transfer
  («every month on the 1st: …»), and each protocol or service on the network (swaps, deposits, paid APIs). The
  «Tempo» group is the model.
- [ ] Each block has `fill` (the network's `RouteFill`), `logo` (the network's or the protocol's), `form` (what it does
  and what it costs) and `example` (a command that works).
- [ ] Each `example` is checked live without moving money: a quote or a gas estimate for a money move, and for a paid
  API `WalletCheck move "pay …"` without `--yes` must print `Price:`. An example that fails stays out of the blocks
  (on Tempo that dropped fal and OpenAI, whose fees were sponsored, and StableStudio, whose reply came later).
- [ ] Add the network to the shared «BRIDGE · RELAY» form (the Move money group). `BlocksTests.everyBlockExampleParses`
  passes.

**Chat and commands**
- [ ] `App/Sources/IhatecryptoCore/PositionCommand.swift`: the network's words in `network(_:)` and `network(in:)`
  (English and Russian: «темпо», «полигон»), the `solanaTo` pattern, the `evmToSolana` condition, `placeName`.
- [ ] `App/Sources/IhatecryptoCore/Normalize.swift`: the «MOVING MONEY» forms, `save … on <net>`, the `places` text
  (where money lives and how it moves). New lookups go into the system prompt.
- [ ] `App/Sources/Wallet/Check/LocalSigner.swift` (the `WalletCheck move` CLI): the network through
  `EVMNetwork.named`, with no fallback to another network.
- [ ] `App/Sources/LLM/Check/main.swift`: new lookups in `lookup` for `--route`.

## 3. Checks

1. `cd App && swift build && swift build --build-tests && swift test --skip-build`: every suite passes. New tests go
   into `App/Tests/IhatecryptoCoreTests/<Net>Tests.swift` (parsing commands both ways, in English and Russian,
   `ModuleNetwork(place:)`, `refusal`) and into `EVMTests` (selector and decoder).
2. The model on real phrases: `swift run -q LLMCheck <provider> --route "move 5 USDC from Solana to <network>" "bring
   everything from <network> back to Solana" "save 0x… as Shop on <network>"`. Each line must give `✓ parses` with
   the right direction. On a subscription this spends the user's limits: run only these few.
3. Live checks without money: `cast estimate` on transfer, approve and the other calls from an address with a
   balance; a Relay quote both ways; `WalletCheck move "<command>"` without `--yes`.
4. The block library: open «Add block» and see the network's group with its logos (a screenshot).
5. A UI scenario in `App/Tools/uitest-scenarios.json`: the bridge draws the route `SOLANA → RELAY → <NET>`. Run it with
   `python3 App/Tools/uitest.py <word>` (needs Settings › Debugging › Local debug API).
6. A screenshot of Settings › Modules: `open -g ~/Applications/"ihatecrypto Dev.app" --args -openPlace settings
   -settingsTab modules`, then `screencapture -l <windowID>` (the window id from `CGWindowListCopyWindowInfo`). Make
   sure the icon shows on the dark theme.
7. Real money moves only after the user's Confirm and Touch ID, or from the test wallet within the budget the user
   gave. Never sign from a main wallet yourself.

## 4. Commit and build

- Other agents may work in the same checkout. Commit only your files, through a private index, and pass the file
  list through bash (zsh doesn't split `$FILES` into words):
  ```bash
  S=<scratch dir>; cat > $S/commit.sh <<'EOF'
  set -e; S=$1; shift; rm -f $S/idx
  GIT_INDEX_FILE=$S/idx git read-tree HEAD
  GIT_INDEX_FILE=$S/idx git add "$@"
  GIT_INDEX_FILE=$S/idx git commit -q -F $S/msg.txt
  git reset -q HEAD -- "$@"
  EOF
  bash $S/commit.sh $S <files…>
  ```
- Check that HEAD builds on its own: `git archive HEAD | tar -x -C $S/head`, `cp -cR App/.build $S/head/App/`, then
  `swift build --target IhatecryptoApp` there. The working tree may not build because of someone else's unfinished
  edits; that isn't your commit.
- Build Release only from HEAD: `TEAM=<team id> $S/head/App/build-app.sh Release` (check `pgrep -fl "xcodebuild
  -project"` first). Then `osascript -e 'tell application id "app.ihatecrypto.mac.dev" to quit'` and
  `open ~/Applications/"ihatecrypto Dev.app"`. This is the separate «ihatecrypto Dev» copy (its own wallets, data and
  debug API port 51790). Never quit or replace an installed ihatecrypto.app.
- A commit message reads like the history: what the user can do now, how it was checked, and whether money moved.

## Traps others fell into

- `eth_getBalance` on a network without a native coin (Tempo) returns a huge placeholder, and `EVMRPC.quantity` fails
  with «not a number». Don't read that balance.
- Tempo takes a plain transaction's fee from the token only for that token's `transfer`/`transferWithMemo`. Other
  calls pay from `FeeManager.userTokens`, or else from pathUSD, which the wallet doesn't hold. Hence `chooseFeeToken`
  before the first approve.
- The `LocalSigner.moveMoney` CLI used to send any unknown network to Robinhood Chain.
- A hardcoded logo (`"eth"` by default in `backRoute`) gives a new network someone else's icon.
- The networks' `HStack` in Modules is one row: with 10+ networks, check its width on a screenshot.
