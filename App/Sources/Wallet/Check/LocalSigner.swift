import Foundation
import HyperliquidSigning
import JupiterSwap
import DeFiKamino
import DeFiYields
import EVMChain
import IhatecryptoCore
import SolanaChain
import TradeExecution
import TxGuard
import WalletCore

/// The test wallet's keys in this process, behind the same guard the app's signing process runs.
struct LocalSigner: TradeSigner {
    let keys: (solana: Data, evm: Data)
    let wallet: TradeWallet

    init() throws {
        keys = try TestWallet.keys()
        let a = try TestWallet.addresses()
        wallet = TradeWallet(solana: a.solana, evm: a.evm)
    }

    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data {
        let verdict = SolanaGuard.check(try SolanaMessage.parse(transaction: wire), accounts: accounts,
                                        policy: SolanaPolicy(owner: wallet.solana, recipients: Set(recipients)), tokenOwners: tokenOwners)
        guard verdict.ok else { throw ExecutionError(verdict.problems.joined(separator: " ")) }
        return try SolanaTransaction.sign(wire, privateKey: keys.solana).wire
    }

    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature {
        let verdict = HLGuard.check(action, owner: wallet.evm, recipients: [])
        guard verdict.ok else { throw ExecutionError(verdict.problems.joined(separator: " ")) }
        return try HLSigner.signL1(action, privateKey: keys.evm, nonce: nonce, mainnet: true)
    }

    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature {
        let verdict = HLGuard.check(action, owner: wallet.evm, recipients: Set(recipients))
        guard verdict.ok else { throw ExecutionError(verdict.problems.joined(separator: " ")) }
        // As the signing process does: sendAsset and usdSend (the sweep's transfer to the new wallet) alike.
        let (type, fields) = try HLSigner.userAction(action)
        return try HLSigner.signUserAction(type, fields: fields, privateKey: keys.evm, mainnet: true)
    }

    func signRelayMapping(wallet w: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature {
        guard w.lowercased() == wallet.evm.lowercased(), depositor.lowercased() == wallet.evm.lowercased() else { throw ExecutionError("Mapping is not for our wallet.") }
        return try EVMSigner.sign(digest: HLSigner.relayNonceMappingDigest(wallet: w, depositor: depositor, id: id, nonce: nonce), privateKey: keys.evm)
    }

    /// The same calldata rules as the signing process: known networks, no unlimited approvals, tokens only to us.
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data {
        guard EVMRPC.signable.contains(chainId) else { throw ExecutionError("Not signing for network \(chainId).") }
        let call = EVMCall.decode(data)
        if let refusal = call.refusal(sender: wallet.evm, chainId: chainId) ?? Batch7702.selfCall(to: to, data: data, owner: wallet.evm) { throw ExecutionError(refusal) }
        if let payee = call.payees(to: to, value: 0, chainId: chainId).first(where: { p in !(recipients + [wallet.evm]).contains { $0.lowercased() == p.lowercased() } }) {
            throw ExecutionError("Tokens or the right to spend them would go to \(payee).")
        }
        return try EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                  gasLimit: gasLimit, to: to, value: 0, data: data).signed(privateKey: keys.evm).raw
    }

    /// The same rules with coins sent along: they may only come back to this wallet (a wrap, a native v4 swap) or go to `recipients`.
    func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, value: UInt64,
                        data: Data, recipients: [String]) async throws -> Data {
        guard EVMRPC.signable.contains(chainId) else { throw ExecutionError("Not signing for network \(chainId).") }
        let call = EVMCall.decode(data)
        if let refusal = call.refusal(sender: wallet.evm, chainId: chainId) ?? Batch7702.selfCall(to: to, data: data, owner: wallet.evm) { throw ExecutionError(refusal) }
        if let payee = call.payees(to: to, value: value, chainId: chainId).first(where: { p in !(recipients + [wallet.evm]).contains { $0.lowercased() == p.lowercased() } }) {
            throw ExecutionError("Coins or tokens would go to \(payee).")
        }
        return try EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                  gasLimit: gasLimit, to: to, value: value, data: data).signed(privateKey: keys.evm).raw
    }

    var signsBatches: Bool { true }

    /// The same batch rules as the signing process: each call's own rules, no call into the wallet, payees approved.
    func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                      calls: [Batch7702.Call], recipients: [String]) async throws -> Data {
        if let problem = Batch7702.problems(calls, owner: wallet.evm, chainId: chainId).first { throw ExecutionError(problem) }
        for c in calls {
            if let payee = EVMCall.decode(c.data).payees(to: c.to, value: 0, chainId: chainId)
                .first(where: { p in !(recipients + [wallet.evm]).contains { $0.lowercased() == p.lowercased() } }) {
                throw ExecutionError("Tokens or the right to spend them would go to \(payee).")
            }
        }
        return try Batch7702.signed(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee, gasLimit: gasLimit,
                                    owner: wallet.evm, calls: calls, privateKey: keys.evm).raw
    }

    /// The same fixed wrap.near transaction the signing process builds.
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data {
        guard amount > 0, depositAddress.wholeMatch(of: /[0-9a-f]{64}/) != nil, blockHash.count == 32 else { throw ExecutionError("Not a 1Click NEAR deposit.") }
        let key = try TestWallet.nearKey()
        return try NearTransaction.sign(receiver: NearTransaction.wrapContract, nonce: nonce, blockHash: blockHash,
                                        actions: NearTransaction.intentDeposit(account: try NearAccount.implicit(privateKey: key), depositAddress: depositAddress, amount: amount),
                                        privateKey: key).signed
    }
}

/// `execute "<phrase>"`: builds the plan from the phrase, prepares and prints what would be signed; `--yes` runs it.
func executePlan(_ phrase: String) async throws {
    let signer = try LocalSigner()
    let markets = try await Hyperliquid.markets()
    guard let intent = IntentParser.apply(phrase, to: TradeIntent(), markets: Set(markets.keys)) else { throw WalletError("Could not parse the phrase.") }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    // The test wallet holds about $15: under the usual 2% nothing clears Hyperliquid's $10 minimum.
    var policy = RiskPolicy()
    policy.maxRiskPct = 10
    let prepared = try await executor.prepare(intent, policy: policy)
    print("Will sign:")
    for (n, s) in prepared.steps.enumerated() { print("  \(n + 1). \(s)") }
    print("What changes:")
    for c in prepared.changes { print("  · \(c)") }
    guard send else { print("Add --yes to execute."); return }
    let opened = try await executor.execute(prepared)
    print("\(opened.pending ? "Limit pending" : "Opened"): \(opened.size) \(opened.asset) at \(opened.entry), stop \(opened.stop), take-profit \(opened.take.map { "\($0)" } ?? "none")")
    try await printPosition()
}

func closePosition(_ asset: String) async throws {
    let signer = try LocalSigner()
    guard send else { print("Close \(asset): add --yes."); return }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    print(try await executor.close(asset: asset))
    try await printPosition()
}

/// `move "выведи 2 USDC в Solana"`, `move "положи 1 USDC в хранилище"`, `move "сними из хранилища"`.
func moveMoney(_ text: String) async throws {
    guard let command = WalletCommand.parse(text) else { throw WalletError("Could not parse the command: \(text)") }
    print("Command: \(command)")
    // Read-only commands and the allocation plan run without --yes; the plan executes only with it.
    switch command {
    // Jupiter DCA without --yes: built, guarded and simulated, not signed.
    case .yields, .allocate, .rebalance, .pay, .jupiterDCA, .jupiterDCACancel, .jupiterDCAOrders: break
    default: guard send else { print("Add --yes."); return }
    }
    // `DRY_SOLANA=<address>`: a dry run for a wallet whose key lives in the signer (the test wallet does now).
    let dry = ProcessInfo.processInfo.environment["DRY_SOLANA"].map { DryRunSigner(wallet: TradeWallet(solana: $0, evm: "")) }
    let signer: any TradeSigner = try dry ?? LocalSigner()
    let wallet = dry?.wallet ?? (signer as! LocalSigner).wallet
    let executor = Executor(wallet: wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    switch command {
    case .jupiterDCA(let asset, let usdc, let orders, let every):
        print(try await executor.jupiterDCA(asset, usdc: usdc ?? 0, orders: orders ?? WalletCommand.jupiterDCAOrders(for: usdc ?? 0), every: every, dryRun: !send))
    case .jupiterDCACancel(let asset): print(try await executor.cancelJupiterDCA(asset, dryRun: !send))
    case .jupiterDCAOrders:
        for o in try await executor.jupiterDCAOrders() {
            print("\(o.key): \(Fmt.num(o.used))/\(Fmt.num(o.deposited)) spent, \(Fmt.num(o.perCycle)) every \(WalletCommand.everyText(o.every)), \(o.fills) fills, received \(Fmt.num(o.received, maxFraction: 9))")
        }
    case .withdrawToSolana(let usdc): _ = try await executor.withdrawToSolana(usdc: usdc)
    case .lendDeposit(let symbol, let amount): _ = try await executor.lend(symbol, amount: amount, deposit: true)
    case .lendWithdraw(let symbol, let amount): _ = try await executor.lend(symbol, amount: amount, deposit: false)
    case .kamino(let deposit, let symbol, let amount): _ = try await executor.kamino(symbol, amount: amount, deposit: deposit)
    case .stake(let sol): _ = try await executor.convert(from: SpotAsset.all[0], to: .jitoSOL, amount: sol)
    case .unstake(let jito): _ = try await executor.convert(from: .jitoSOL, to: SpotAsset.all[0], amount: jito)
    case .hlp(let deposit, let amount): _ = try await executor.hlp(deposit: deposit, amount: amount)
    case .buyNear(let usdc):
        _ = try await executor.buyNear(usdc: usdc ?? 0, account: try TestWallet.nearAccount())
        if let b = try? await Near.balance(account: try TestWallet.nearAccount()) { print("NEAR account: \(Fmt.num(b.near, maxFraction: 6)) NEAR, \(Fmt.num(b.wrapped, maxFraction: 6)) wNEAR") }
    case .sellNear(let amount, let to):
        print(try await executor.sellNear(amount: amount, account: try TestWallet.nearAccount(), to: to.flatMap(EVMNetwork.named)))
        if let b = try? await Near.balance(account: try TestWallet.nearAccount()) { print("NEAR account: \(Fmt.num(b.near, maxFraction: 6)) NEAR, \(Fmt.num(b.wrapped, maxFraction: 6)) wNEAR") }
    case .aave(let deposit, let amount, let net):
        guard let m = AaveV3.markets.first(where: { $0.chain == net }) else { throw ExecutionError("No Aave market on \(net).") }
        print(try await executor.aave(deposit: deposit, usdc: amount, market: m))
    case .morpho(let deposit, let amount, let net):
        guard let v = MorphoVault.vaults.first(where: { $0.chain == net }) else { throw ExecutionError("No Morpho vault on \(net).") }
        print(try await executor.morpho(deposit: deposit, usdc: amount, vault: v))
    case .baseToSolana(let amount): print(try await executor.baseToSolana(usdc: amount))
    case .arbitrumToSolana(let amount): print(try await executor.evmToSolana(.arbitrum, usdc: amount))
    case .evmToSolana(let net, let amount):
        guard let n = EVMNetwork.named(net) else { throw ExecutionError("ihatecrypto doesn't move money on \(net).") }
        print(try await executor.evmToSolana(n, usdc: amount))
    case .sendTo, .sendNear, .closeNear, .sendSol, .sendToken: throw ExecutionError("Sending to a saved address works in the ihatecrypto app, where the address book is.")
    case .buyStock(let symbol, let usdc): print(try await executor.buyStock(symbol, usdc: usdc ?? 1))
    case .sellStock(let symbol, let amount): print(try await executor.sellStock(symbol, amount: amount))
    case .buyOn(let place, let token, let usdc, let from): print(try await executor.buyOn(EVMNetwork.named(place) ?? .base, token, usdc: usdc ?? 1, from: from))
    case .sellOn(let place, let token, let amount, let keep): print(try await executor.sellOn(EVMNetwork.named(place) ?? .base, token, amount: amount, keep: keep))
    case .hlSpotBuy(let symbol, let usdc): print(try await executor.hlSpot(buy: true, symbol: symbol, usdc: usdc ?? 10, amount: nil))
    case .hlSpotSell(let symbol, let amount): print(try await executor.hlSpot(buy: false, symbol: symbol, usdc: nil, amount: amount))
    case .contractCall: print("A contract call is checked, simulated and confirmed in the app.")
    case .multisigCreate, .multisigSend, .multisigShow: print("A multisig is set up, signed and sent in the app.")
    case .showApprovals: for a in await executor.approvals().list { print(a.chain, a.symbol, a.spenderName ?? a.spender, a.unlimited ? "unlimited" : "\(a.amount)", a.permit2 ? "Permit2" : "") }
    case .revokeApproval(let place, let token, let spender, let permit2):
        print(try await executor.revokeApproval(EVMNetwork.named(place) ?? .base, token: token, spender: spender, permit2: permit2))
    case .sweepDust: throw ExecutionError("Sweeping dust works in the ihatecrypto app, where the portfolio is.")
    case .swapOn(let place, let from, let to, let amount): print(try await executor.swapOn(EVMNetwork.named(place) ?? .base, from: from, to: to, amount: amount))
    case .cctp(let from, let to, let amount): print(try await executor.cctpBridge(from: EVMNetwork.named(from) ?? .base, to: EVMNetwork.named(to) ?? .arbitrum, usdc: amount))
    case .usdt(let from, let to, let amount, let layerZero):
        guard let a = EVMNetwork.named(from), let b = EVMNetwork.named(to) else { throw ExecutionError("ihatecrypto doesn't move USDT between \(from) and \(to).") }
        print(try await executor.usdtMove(from: a, to: b, usdt: amount, layerZero: layerZero))
    case .aaveBorrow(let x, let net): print(try await executor.aaveBorrow(AaveV3.markets.first { $0.chain == net } ?? AaveV3.base, usdc: x ?? 0))
    case .kaminoBorrow(let x, let stock, let adding): print(try await executor.kaminoBorrow(usdc: x ?? 0, against: stock, adding: adding))
    case .kaminoRepay(let x): print(try await executor.kaminoRepay(usdc: x))
    case .aaveRepay(let x, let net): print(try await executor.aaveRepay(AaveV3.markets.first { $0.chain == net } ?? AaveV3.base, usdc: x))
    case .aaveCollateral(let x, let net): print(try await executor.aaveSupplyETH(AaveV3.markets.first { $0.chain == net } ?? AaveV3.base, eth: x ?? 0))
    case .receiveAcross: print("Receiving through NEAR Intents opens a deposit address in the app.")
    case .crossSwap(let a, let from, let b, let to, let amount): print(try await executor.crossSwap(a, on: from, for: b, on: to, amount: amount))
    case .robinhoodToSolana(let amount): print(try await executor.evmToSolana(.robinhood, usdc: amount))
    case .solanaTo(let place, let amount):
        guard let n = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't move money to \(place).") }
        print(try await executor.solanaToEVM(n, usdc: amount ?? 1))
    case .gas(let place, let usd):
        guard let n = EVMNetwork.named(place) else { throw ExecutionError("No network \(place).") }
        print(try await executor.topUpGas(n, usd: usd ?? 1))
    case .bridge(let from, let to, let amount):
        let hyperliquid = to == "hyperliquid", b = EVMNetwork.named(to)
        guard let a = EVMNetwork.named(from), hyperliquid || b != nil else { throw ExecutionError("No network \(from) or \(to).") }
        print(try await executor.bridge(a, to: b, hyperliquid: hyperliquid, usdc: amount))
    case .pay(let method, let url, let body):
        // Without --yes: the service's price only. With it: paid on Tempo, then the answer.
        let (status, headers, data) = try await MPP.call(method, url, body)
        guard status == 402 else { print("\(status), unpaid:\n\(MPP.shown(data))"); return }
        let c = try MPP.challenge(headers["www-authenticate"] ?? "").get()
        print("Price: \(Fmt.num(c.usdc, maxFraction: 6)) USDC to \(c.payee) (\(c.realm))\(c.purpose.map { " for «\($0)»" } ?? ""), memo \(c.memo.map { "0x" + $0.hex } ?? "none")")
        guard send else { print("Add --yes to pay it."); return }
        let hash = try await executor.payTempo(c.amount, to: c.payee, memo: c.memo, label: c.realm)
        let (paid, _, answer) = try await MPP.call(method, url, body, credential: (c.field, MPP.credential(c, hash: hash, payer: wallet.evm)))
        print("Paid: \(EVMNetwork.tempo.explorer)/tx/\(hash)\n\(paid):\n\(MPP.shown(answer))")
    case .claimRewards:
        // Without --yes: what Merkl owes. With it: claimed on each chain.
        let owed = try await Merkl.rewards(user: wallet.evm)
        for r in owed { print("chain \(r.chainId): \(Fmt.num(r.claimable, maxFraction: 6)) \(r.symbol)\(r.worthUSD.map { " ≈ \(Fmt.usd($0))" } ?? "")") }
        guard !owed.isEmpty else { print("Nothing to claim on Merkl."); return }
        guard send else { print("Add --yes to claim it."); return }
        print(try await executor.claimRewards())
    case .sweepRewards(let min, let into):
        guard send else { print("Add --yes to claim and sell."); return }
        print(try await executor.sweepRewards(minUSD: min, into: into))
    case .receive: print("Solana (USDC, SOL): \(wallet.solana)\nBase / EVM (USDC, ETH): \(wallet.evm)")
    case .rebalance(let share):
        let (text, ready, _) = try await executor.rebalance(maxShare: share ?? 50, execute: send)
        print(text + (ready && !send ? "\nWith --yes I'll make these moves." : ""))
    case .allocate(let symbol, let share, let limit, let without, let from):
        let (text, ready, _) = try await executor.allocate(symbol, maxShare: share ?? 50, limit: limit, without: without, from: from, execute: send)
        if !send { print(text + (ready ? "\nWith --yes I'll allocate by this plan." : "")) }
    case .yields(let asset):
        for o in await Yields.options(for: asset) { print("\(o.asset) · \(o.place): \(Fmt.num(o.apy.rounded(2, .plain)))%  \(o.note)") }
        return
    case .buy(let asset, let usdc): _ = try await executor.swap(try await lookedUp(asset), buy: true, amount: usdc)
    case .sell(let asset, let amount): _ = try await executor.swap(try await lookedUp(asset), buy: false, amount: amount)
    case .schedule, .stopSchedules: print("Scheduled buys are run by the ihatecrypto app while it is open.")
    }
    try await printLendPositions()
    try await printBalances()
}

/// A token named by its ticker or mint alone, looked up on Jupiter as the app does (TokensUI.spotToken): its decimals and
/// token program, so a Token-2022 coin (Ondo's stocks) reads from its own account.
func lookedUp(_ asset: SpotAsset) async throws -> SpotAsset {
    guard !asset.resolved else { return asset }
    let mint = asset.mint.isEmpty ? await TokenResearch.solanaMint(symbol: asset.symbol) : asset.mint
    guard let mint, let t = await TokenResearch.solanaToken(mint) else { throw WalletError("Jupiter has no verified token \(asset.symbol) on Solana.") }
    return SpotAsset(symbol: t.symbol, mint: mint, decimals: t.decimals, token2022: t.token2022)
}

/// `manage "стоп в безубыток"` and the like, on the position of the named or ETH asset.
func managePosition(_ text: String) async throws {
    guard let command = PositionCommand.parse(text) else { throw WalletError("Could not parse the command: \(text)") }
    let signer = try LocalSigner()
    let markets = try await Hyperliquid.markets()
    let asset = text.uppercased().split(whereSeparator: { !$0.isLetter }).map(String.init).first { markets[$0] != nil } ?? "ETH"
    print("Command: \(command) for \(asset)")
    guard send else { print("Add --yes."); return }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    _ = try await executor.manage(command, asset: asset)
    try await printPosition()
}

func printPosition() async throws {
    let evm = try TestWallet.addresses().evm
    let positions = try await HyperliquidAccount.positions(evm)
    if positions.isEmpty { print("No positions.") }
    for p in positions {
        print("Position \(p.coin): \(p.size) at \(p.entry), PnL \(Fmt.usd(p.unrealizedPnl)), liquidation \(p.liquidation.map { Fmt.price($0) } ?? "—")")
    }
    for o in try await HyperliquidAccount.openOrders(evm) {
        print("Order \(o.coin) #\(o.oid) \(o.side ?? "?") \(o.size.map { "\($0)" } ?? "?")\(o.isTrigger ? " trigger \(o.triggerPx.map { "\($0)" } ?? "?")" : " limit \(o.limitPx.map { "\($0)" } ?? "?")")")
    }
    try await printBalances()
}

/// `browser-test-page <path>`: a local dApp page for the in-app browser test, with fresh unsigned transactions
/// for the test wallet: a Memo (a program outside ihatecrypto's list) and a token Approve to a stranger (a drainer).
func writeBrowserTestPage(_ path: String) async throws {
    let signer = try LocalSigner()
    let owner = signer.wallet.solana
    let (hash, _) = try await rpc.latestBlockhash()
    let memo = SolanaInstruction(program: "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
                                 accounts: [.init(pubkey: owner, isSigner: true, isWritable: false)], data: Data("ihatecrypto browser test".utf8))
    let memoMessage = try SolanaTransaction.compileMessage([memo], payer: owner, blockhash: hash)
    let stranger = Base58.encode(Data((0..<32).map { _ in UInt8.random(in: 0...255) }))
    let myUSDC = try SPL.associatedAccount(owner: owner, mint: Jupiter.usdcMint)
    let approve = SolanaInstruction(program: SPL.tokenProgram, accounts: [
        .init(pubkey: myUSDC, isSigner: false, isWritable: true), .init(pubkey: stranger, isSigner: false, isWritable: false),
        .init(pubkey: owner, isSigner: true, isWritable: false),
    ], data: Data([4]) + withUnsafeBytes(of: UInt64(1_000_000).littleEndian) { Data($0) })
    let approveWire = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([approve], payer: owner, blockhash: hash))
    // EVM: reference signatures made here with the same key, which the page compares with ihatecrypto's.
    let evm = signer.wallet.evm
    func hex(_ s: EVMSignature) -> String { "0x" + s.r.hex + s.s.hex + String(format: "%02x", s.v) }
    let login = "Sign in to the ihatecrypto test page (EVM)"
    let personal = hex(try EVMSigner.sign(digest: PersonalMessage.digest(Data(login.utf8)), privateKey: signer.keys.evm))
    // A site's typed login signs like the reference; Hyperliquid's own ApproveAgent never signs for a site (the main key is
    // the Hyperliquid account's key: the app signs its actions only in its own guarded steps).
    let typedJSON = #"{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"}],"Login":[{"name":"contents","type":"string"},{"name":"nonce","type":"uint256"}]},"primaryType":"Login","domain":{"name":"ihatecrypto test","version":"1","chainId":42161},"message":{"contents":"Sign in to the ihatecrypto test page","nonce":1}}"#
    let typedSig = hex(try EVMSigner.sign(digest: TypedData.parse(Data(typedJSON.utf8)).digest, privateKey: signer.keys.evm))
    let agentJSON = #"{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],"HyperliquidTransaction:ApproveAgent":[{"name":"hyperliquidChain","type":"string"},{"name":"agentAddress","type":"address"},{"name":"agentName","type":"string"},{"name":"nonce","type":"uint64"}]},"primaryType":"HyperliquidTransaction:ApproveAgent","domain":{"name":"HyperliquidSignTransaction","version":"1","chainId":42161,"verifyingContract":"0x0000000000000000000000000000000000000000"},"message":{"hyperliquidChain":"Mainnet","agentAddress":"0x00000000000000000000000000000000000000aa","agentName":"test","nonce":1700000000000}}"#
    let permitJSON = #"{"types":{"Permit":[{"name":"owner","type":"address"},{"name":"spender","type":"address"},{"name":"value","type":"uint256"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}]},"primaryType":"Permit","domain":{"name":"USD Coin","version":"2","chainId":42161,"verifyingContract":"0xaf88d065e77c8cC2239327C5EDb3A432268e5831"},"message":{"owner":"\#(evm)","spender":"0x000000000000000000000000000000000000dEaD","value":"115792089237316195423570985008687907853269984665640564039457584007913129639935","nonce":0,"deadline":"99999999999"}}"#
    let html = """
    <!doctype html><meta charset="utf-8"><title>ihatecrypto test dApp</title><pre id="out">waiting</pre>
    <script>
    const MEMO_TX = "\(SolanaTransaction.unsigned(memoMessage).base64EncodedString())";
    const MEMO_MESSAGE = "\(memoMessage.base64EncodedString())";
    const APPROVE_TX = "\(approveWire.base64EncodedString())";
    const b64d = (t) => Uint8Array.from(atob(t), (c) => c.charCodeAt(0));
    const results = []; const log = (s) => { results.push(s); document.getElementById('out').textContent = results.join('\\n'); };
    let wallet;
    const api = { register: (w) => { wallet = wallet || w; return () => {}; } };
    window.addEventListener('wallet-standard:register-wallet', ({ detail }) => detail(api));
    window.dispatchEvent(new CustomEvent('wallet-standard:app-ready', { detail: api }));
    (async () => {
      if (!wallet) return log('FAIL: no wallet registered');
      log('wallet: ' + wallet.name + ', features: ' + Object.keys(wallet.features).length);
      const { accounts } = await wallet.features['standard:connect'].connect();
      const account = accounts[0];
      log('connect: ' + account.address);
      const [m] = await wallet.features['solana:signMessage'].signMessage({ account, message: new TextEncoder().encode('Sign in to the ihatecrypto test page') });
      log('signMessage: ' + m.signature.length + '-byte signature');
      try { await wallet.features['solana:signMessage'].signMessage({ account, message: b64d(MEMO_MESSAGE) }); log('FAIL: transaction signed as a message'); }
      catch (e) { log('transaction disguised as text: refused (' + e.message + ')'); }
      try {
        const [s] = await wallet.features['solana:signAndSendTransaction'].signAndSendTransaction({ account, transaction: b64d(MEMO_TX), chain: 'solana:mainnet' });
        log('memo (unknown program): signed and sent, signature ' + s.signature.length + ' bytes, ' + btoa(String.fromCharCode(...s.signature)).slice(0, 16));
      } catch (e) { log('memo: FAIL ' + e.message); }
      try { await wallet.features['solana:signTransaction'].signTransaction({ account, transaction: b64d(APPROVE_TX) }); log('FAIL: token approval signed'); }
      catch (e) { log('token approval to a stranger: refused (' + e.message + ')'); }

      // EVM through EIP-6963, the way wallet pickers discover providers.
      let eth = null;
      window.addEventListener('eip6963:announceProvider', ({ detail }) => { if (detail.info.name === 'ihatecrypto') eth = detail.provider; });
      window.dispatchEvent(new Event('eip6963:requestProvider'));
      if (!eth) { log('FAIL: no EIP-6963 provider'); log('DONE'); return; }
      const req = (method, params) => eth.request({ method, params });
      const refused = async (label, method, params) => { try { await req(method, params); log('FAIL: ' + label + ' signed'); } catch (e) { log(label + ': refused, code ' + e.code); } };
      log('EIP-6963: ihatecrypto, chain ' + await req('eth_chainId'));
      const [addr] = await req('eth_requestAccounts');
      log('evm account: ' + (addr === '\(evm)' ? 'ok ' : 'FAIL ') + addr);
      const sig = await req('personal_sign', ['0x' + [...new TextEncoder().encode('\(login)')].map(b => b.toString(16).padStart(2, '0')).join(''), addr]);
      log('personal_sign: ' + (sig === '\(personal)' ? 'matches reference' : 'FAIL ' + sig));
      const typed = await req('eth_signTypedData_v4', [addr, JSON.stringify(\(typedJSON))]);
      log('typed data (site login): ' + (typed === '\(typedSig)' ? 'matches reference' : 'FAIL ' + typed));
      await refused('Hyperliquid ApproveAgent for a site', 'eth_signTypedData_v4', [addr, JSON.stringify(\(agentJSON))]);
      await refused('Permit (unlimited USDC to a stranger)', 'eth_signTypedData_v4', [addr, JSON.stringify(\(permitJSON))]);
      await refused('blind 32-byte hash', 'personal_sign', ['0x' + 'aa'.repeat(32), addr]);
      // EVM transactions on Arbitrum; the account has no ETH, so a signed one is refused by the node for funds,
      // which names the address it recovered from the signature.
      const pad = (h) => h.replace(/^0x/, '').toLowerCase().padStart(64, '0');
      const USDC = '0xaf88d065e77c8cC2239327C5EDb3A432268e5831';
      const spender = '0x' + '5c'.repeat(20);
      try { await req('eth_sendTransaction', [{ from: addr, to: USDC, data: '0x095ea7b3' + pad(spender) + pad((1000000).toString(16)) }]); log('FAIL: exact approve sent'); }
      // A stranger as spender is a new address: signed only after the tick and Touch ID for it.
      catch (e) { log('exact approve 1 USDC to a stranger: ' + (e.message.toLowerCase().includes(addr.toLowerCase()) ? 'signed, node recovered our address' : 'not signed') + ' — ' + e.message.slice(0, 120)); }
      await refused('unlimited approve', 'eth_sendTransaction', [{ from: addr, to: USDC, data: '0x095ea7b3' + pad(spender) + 'f'.repeat(64) }]);
      await refused('setApprovalForAll', 'eth_sendTransaction', [{ from: addr, to: USDC, data: '0xa22cb465' + pad(spender) + pad('1') }]);
      try { await req('eth_sendTransaction', [{ from: addr, to: USDC, data: '0xa9059cbb' + pad(spender) + pad((1000000).toString(16)) }]); log('FAIL: transfer sent'); }
      catch (e) { log('USDC transfer without balance: refused, code ' + e.code + ' — ' + e.message.slice(0, 90)); }
      let changed = null; eth.on('chainChanged', (c) => { changed = c; });
      await req('wallet_switchEthereumChain', [{ chainId: '0x1' }]);
      log('switch to Ethereum: chainChanged ' + changed + ', balance ' + await req('eth_getBalance', [addr, 'latest']));
      log('DONE');
    })();
    </script>
    """
    try html.write(toFile: path, atomically: true, encoding: .utf8)
    print("Page: \(path)")
}

/// `plan "<phrase>"`: how a phrase is parsed and what plan the rules make of it, on live prices. Signs nothing.
func explainPlan(_ phrase: String) async throws {
    let markets = try await Hyperliquid.markets()
    guard let intent = IntentParser.apply(phrase, to: TradeIntent(), markets: Set(markets.keys)) else { throw WalletError("Could not parse the phrase.") }
    var market = markets[intent.asset]
    market?.atrPct = try? await Hyperliquid.atrPct(coin: intent.asset)
    let a = try TestWallet.addresses()
    let hl = try await Hyperliquid.balance(user: a.evm)
    let equity = hl.accountValue + Decimal(try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)) / 1_000_000
    let (resolved, notes) = Plan.resolve(intent, market: market, equity: equity, policy: RiskPolicy())
    print("Parsed: \(intent.side) \(intent.asset), risk \(intent.riskUSD.map { "\($0)" } ?? "—"), stop \(intent.stopPct.map { "\($0)%" } ?? "—"), by the rules \(intent.autoRules == true)")
    print("Market: \(market.map { "\($0.name) mark \($0.markPx), szDecimals \($0.szDecimals), leverage up to \($0.maxLeverage)×, id \($0.index), dex \($0.dex ?? "main"), ATR 4h \($0.atrPct.map { Fmt.pct($0.rounded(2, .plain)) } ?? "—")" } ?? "none")")
    for n in notes { print("  · \(n)") }
    let e = Plan.evaluate(resolved, market: market)
    if let s = e.sizing { print("Plan: \(s.size) at \(s.entry), notional \(Fmt.usd(s.notional)), margin \(Fmt.usd(s.margin)), stop \(s.stopPrice), take-profit \(s.takePrice.map { "\($0)" } ?? "—"), at stop −\(Fmt.usd(s.lossAtStopUSD))") }
    if let p = e.problem { print("Problem: \(p)") }
    if !e.missing.isEmpty { print("Missing: \(e.missing.joined(separator: ", "))") }
}

/// `kamino <deposit|withdraw> <SYMBOL> [amount] [--yes] [--accept-rent]` — without --yes only simulates (shows rent and
/// changes); --accept-rent lets a first deposit pay Kamino's account rent even when it is over 10% of the amount.
/// `kamino reserves`, `kamino position`.
func kaminoCommand(_ args: [String]) async throws {
    let signer = try LocalSigner()
    switch args.first ?? "reserves" {
    case "reserves":
        for r in try await Kamino.reserves().prefix(12) { print("\(r.symbol)  \(Fmt.num(r.supplyAPY.rounded(2, .plain)))% APY  pool \(Fmt.usd(r.tvlUSD))  \(r.address)") }
    case "position":
        let p = try await Kamino.positions(wallet: signer.wallet.solana)
        if p.isEmpty { print("No Kamino positions.") }
        for x in p { print("\(x.symbol): \(Fmt.usd(x.valueUSD))  (shares \(x.amount))") }
    case let action where action == "deposit" || action == "withdraw":
        let symbol = args.dropFirst().first ?? "USDC"
        let amount = args.dropFirst(2).first.flatMap { Decimal(string: $0) }
        let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
        _ = try await executor.kamino(symbol, amount: amount, deposit: action == "deposit", dryRun: !send,
                                      acceptRent: CommandLine.arguments.contains("--accept-rent"))
        try await printBalances()
    default:
        print("kamino reserves | position | deposit USDC 1 [--yes] | withdraw USDC [amount] [--yes]")
    }
}

/// `sweep-solana <new Solana address> --yes`: the move to a new wallet, for this (old) test wallet. All USDC goes to
/// the new address through the executor's checked send; empty token accounts are closed for their rent; then all SOL
/// follows, the account left at exactly zero. Only the given address may receive anything (the guard checks it).
func sweepSolana(to destination: String) async throws {
    let signer = try LocalSigner()
    let me = signer.wallet.solana
    guard (try? Base58.decode(destination))?.count == 32, destination != me else { throw WalletError("Not a Solana address to move to: \(destination)") }
    guard send else { print("Moves everything on Solana from \(me) to \(destination). Add --yes."); return }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    let usdc = try await rpc.tokenBalance(owner: me, mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
    if usdc > 0 { print(try await executor.sendUSDC(Decimal(usdc) / 1_000_000, to: .solana(destination), label: "the new wallet")) }

    // Empty token accounts hold ≈ 0.002 SOL of rent each: closing them brings it back before the SOL leaves.
    // «confirmed», not the default «finalized»: right after the USDC left, finalized still shows it there.
    let accounts = (try await rpc.call("getTokenAccountsByOwner", [me, ["programId": SPL.tokenProgram], ["encoding": "jsonParsed", "commitment": "confirmed"]]) as? [String: Any])?["value"] as? [[String: Any]] ?? []
    let empty = accounts.compactMap { a -> String? in
        let amount = ((((a["account"] as? [String: Any])?["data"] as? [String: Any])?["parsed"] as? [String: Any])?["info"] as? [String: Any])
            .flatMap { ($0["tokenAmount"] as? [String: Any])?["amount"] as? String }
        return amount == "0" ? a["pubkey"] as? String : nil
    }
    for account in empty {
        let close = SolanaInstruction(program: SPL.tokenProgram, accounts: [
            .init(pubkey: account, isSigner: false, isWritable: true), .init(pubkey: me, isSigner: false, isWritable: true),
            .init(pubkey: me, isSigner: true, isWritable: false),
        ], data: Data([9])) // CloseAccount
        let (hash, lastValid) = try await rpc.latestBlockhash()
        let wire = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([close], payer: me, blockhash: hash))
        let signed = try await signer.signSolana(wire, accounts: try SolanaMessage.parse(transaction: wire).staticKeys, recipients: [], tokenOwners: [account: me])
        let signature = try await rpc.send(signed)
        print("Closed empty token account \(account.prefix(6))…: \(try await rpc.waitForConfirmation(signature, lastValidBlockHeight: lastValid))")
    }

    // All SOL: the fee is asked from the network for this very message, so the old account ends at exactly zero
    // (anything between zero and the rent minimum would be refused).
    let lamports = try await rpc.balance(me)
    func transfer(_ amount: UInt64, _ hash: String) throws -> Data {
        let data = Data([2, 0, 0, 0]) + withUnsafeBytes(of: amount.littleEndian) { Data($0) }
        return try SolanaTransaction.compileMessage([SolanaInstruction(program: SolanaGuard.system, accounts: [
            .init(pubkey: me, isSigner: true, isWritable: true), .init(pubkey: destination, isSigner: false, isWritable: true),
        ], data: data)], payer: me, blockhash: hash, microLamportsPerCU: 0)
    }
    let (hash, lastValid) = try await rpc.latestBlockhash()
    let feeReply = try await rpc.call("getFeeForMessage", [try transfer(lamports, hash).base64EncodedString(), ["commitment": "confirmed"]]) as? [String: Any]
    guard let fee = (feeReply?["value"] as? NSNumber)?.uint64Value, lamports > fee else { throw WalletError("No fee quote, or nothing to move (\(lamports) lamports).") }
    let wire = SolanaTransaction.unsigned(try transfer(lamports - fee, hash))
    let signed = try await signer.signSolana(wire, accounts: try SolanaMessage.parse(transaction: wire).staticKeys, recipients: [destination], tokenOwners: [:])
    let signature = try await rpc.send(signed)
    print("Sent \(Fmt.num(Decimal(lamports - fee) / 1_000_000_000, maxFraction: 9)) SOL (fee \(fee) lamports): \(try await rpc.waitForConfirmation(signature, lastValidBlockHeight: lastValid)) https://solscan.io/tx/\(signature)")
    print("Left on \(me): \(try await rpc.balance(me)) lamports, \(try await rpc.tokenBalance(owner: me, mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")) USDC units.")
}

/// `sweep-near <new NEAR implicit account> --yes`: the old test wallet's NEAR to the new wallet's account in one plain
/// transfer. It keeps what the old account's own storage needs and a margin for the fee (a transfer's gas is tiny).
func sweepNear(to destination: String) async throws {
    let key = try TestWallet.nearKey()
    let me = try TestWallet.nearAccount()
    guard destination.wholeMatch(of: /[0-9a-f]{64}/) != nil, destination != me else { throw WalletError("Not a NEAR implicit account to move to: \(destination)") }
    let context = try await Near.txContext(account: me)
    let balance = UInt128(context.balance) ?? 0
    // Storage, and 0.01 NEAR: a transfer that creates the new implicit account prepays ≈ 0.0076 NEAR of gas at the
    // pessimistic price (measured 2026-09-28); the unused part comes back to the old account.
    let keep = UInt128(context.storageBytes) * 10_000_000_000_000_000_000 + 10_000_000_000_000_000_000_000
    guard balance > keep else { throw WalletError("Nothing to move: \(NearTransaction.near(balance)) NEAR, \(NearTransaction.near(keep)) stays for storage.") }
    let amount = (balance - keep) / 1_000_000_000_000_000_000 * 1_000_000_000_000_000_000
    guard send else { print("Moves \(NearTransaction.near(amount)) NEAR from \(me) to \(destination). Add --yes."); return }
    let signed = try NearTransaction.sign(receiver: destination, nonce: context.nonce, blockHash: context.blockHash,
                                          actions: [.transfer(deposit: amount)], privateKey: key).signed
    let hash = try await Near.send(signed)
    print("Sent \(NearTransaction.near(amount)) NEAR to \(destination.prefix(8))…: https://nearblocks.io/txns/\(hash)")
}

/// `sweep-hyperliquid <new EVM address> --yes`: the old test wallet's HLP (once its 4-day lock is over) back to its
/// perp balance, then everything there to the new wallet's Hyperliquid account with usdSend — no bridge, no Solana
/// fee. Only that address may receive it (the guard and the signer check).
func sweepHyperliquid(to destination: String) async throws {
    let signer = try LocalSigner()
    guard destination.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil, destination.lowercased() != signer.wallet.evm.lowercased() else { throw WalletError("Not an EVM address to move to: \(destination)") }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    let vault = try await HyperliquidAccount.vaultEquity(user: signer.wallet.evm, vault: HLGuard.hlpVault)
    guard send else {
        print("HLP \(Fmt.usd(vault.equity))\(vault.lockedUntil.map { ", locked until \(Fmt.date($0))" } ?? ""); then all perp USDC to \(destination) on Hyperliquid. Add --yes.")
        return
    }
    // Withdrawn already, HLP keeps a dust of equity (0.000003) that Hyperliquid refuses to withdraw.
    if vault.equity >= 0.01 { print(try await executor.hlp(deposit: false, amount: nil)) }
    // Margin left on a HIP-3 exchange (xyz) joins the main balance first, so nothing stays behind.
    for dex in Hyperliquid.hip3Dexes {
        let idle = (try await Hyperliquid.dexBalance(user: signer.wallet.evm, dex: dex).withdrawable).rounded(6, .down)
        if idle > 0 { try await executor.moveMargin(from: dex, to: "", usdc: idle) }
    }
    let free = try await Hyperliquid.balance(user: signer.wallet.evm).withdrawable
    print(try await executor.sendUSDC(free, to: .hyperliquid(destination), label: "the new wallet"))
}

/// `close-near <new NEAR implicit account> --yes`: deletes the old test wallet's NEAR account; NEAR sends everything
/// left on it, its storage deposit included, to the new account. The new account must exist, or NEAR would burn it.
func closeNear(to beneficiary: String) async throws {
    let key = try TestWallet.nearKey()
    let me = try TestWallet.nearAccount()
    guard beneficiary.wholeMatch(of: /[0-9a-f]{64}/) != nil, beneficiary != me else { throw WalletError("Not a NEAR implicit account: \(beneficiary)") }
    guard let there = try? await Near.balance(account: beneficiary), there.near > 0 else {
        throw WalletError("\(beneficiary.prefix(8))… doesn't exist on NEAR yet: deleting into it would burn the balance.")
    }
    let context = try await Near.txContext(account: me)
    guard send else { print("Deletes \(me.prefix(8))… (\(NearTransaction.near(UInt128(context.balance) ?? 0)) NEAR) into \(beneficiary.prefix(8))…. Add --yes."); return }
    let signed = try NearTransaction.sign(receiver: me, nonce: context.nonce, blockHash: context.blockHash,
                                          actions: [.deleteAccount(beneficiary: beneficiary)], privateKey: key).signed
    let hash = try await Near.send(signed)
    print("Deleted the old NEAR account; its NEAR went to \(beneficiary.prefix(8))…: https://nearblocks.io/txns/\(hash)")
}

/// `sweep-evm <new EVM address> --yes`: the old test wallet's leftovers on Base, Robinhood Chain and Arbitrum — its
/// USDC/USDG first (while there is ETH to pay for it), then its ETH — to that address on each chain. What a transfer
/// doesn't burn of its gas stays behind, a few wei; Base's L1 data fee comes from its GasPriceOracle, with a margin.
func sweepEVM(to destination: String) async throws {
    let signer = try LocalSigner()
    let me = signer.wallet.evm
    guard destination.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil, destination.lowercased() != me.lowercased() else {
        throw WalletError("Not an EVM address to move to: \(destination)")
    }
    let executor = Executor(wallet: signer.wallet, signer: signer, journal: journal, rpc: rpc) { print("[\(stamp())] \($0)") }
    for net in [EVMNetwork.base, .robinhood, .arbitrum] {
        let chain = net.rpc
        let stable = try await net.usdcBalance(me)
        let wei = try await chain.balance(me)
        print("\(net.name): \(stable) stable units, \(wei) wei")
        guard send else { continue }
        if stable > 0 {
            let to: Executor.Destination = net.key == "base" ? .base(destination) : net.key == "arbitrum" ? .arbitrum(destination) : .robinhood(destination)
            do { print(try await executor.sendUSDC(Decimal(stable) / 1_000_000, to: to, label: "the new wallet")) } catch { print("\(net.name) stablecoin stays: \(error)") }
        }
        let balance = try await chain.balance(me)
        guard balance > 0 else { continue }
        let (tip, maxFee) = try await chain.fees()
        let gas = try await chain.estimateGas(from: me, to: destination, value: 1, data: Data())   // Arbitrum's includes its L1 part
        var fee = gas * maxFee
        if net.key == "base" {
            // OP stack: the L1 data fee is charged on top; ask the oracle for this very transaction's size.
            let sample = try EVMTransaction(chainId: chain.chainId, nonce: try await chain.nonce(me), maxPriorityFeePerGas: tip, maxFeePerGas: maxFee,
                                            gasLimit: gas, to: destination, value: balance, data: Data()).signed(privateKey: signer.keys.evm).raw
            var call = Data([0x49, 0x94, 0x8e, 0x0e])   // getL1Fee(bytes)
            call += ABI.uint(32) + ABI.uint(UInt64(sample.count)) + sample + Data(count: (32 - sample.count % 32) % 32)
            let l1 = ABI.uint(in: try await chain.read("0x420000000000000000000000000000000000000F", call), word: 0) ?? 0
            fee += l1 + l1 / 2
        }
        guard balance > fee else { print("\(net.name): \(balance) wei is less than the fee (\(fee)); it stays."); continue }
        let tx = EVMTransaction(chainId: chain.chainId, nonce: try await chain.nonce(me), maxPriorityFeePerGas: tip, maxFeePerGas: maxFee,
                                gasLimit: gas, to: destination, value: balance - fee, data: Data())
        let (raw, hash) = try tx.signed(privateKey: signer.keys.evm)
        _ = try await chain.send(raw)
        let ok = try await chain.waitForReceipt(hash)
        print("\(net.name): sent \(balance - fee) wei → \(ok ? "confirmed" : "FAILED") \(net.explorer)/tx/\(hash); left \(try await chain.balance(me)) wei")
    }
}

/// Signs nothing: dry runs (`DRY_SOLANA`) build, guard and simulate a move for an address alone.
struct DryRunSigner: TradeSigner {
    let wallet: TradeWallet
    private func refuse() -> Error { WalletError("A dry run signs nothing.") }
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw refuse() }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw refuse() }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw refuse() }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw refuse() }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                 to: String, data: Data, recipients: [String]) async throws -> Data { throw refuse() }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw refuse() }
}
