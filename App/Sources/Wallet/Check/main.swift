import Foundation
import HyperliquidSigning
import JupiterSwap
import IhatecryptoCore
import DeFiJupiterLend
import EVMChain
import ExecutionCore
import GuardJev
import OneClickKit
import RelayBridge
import SolanaChain
import TradeExecution
import TxGuard
import WalletCore

// Live checks with real services and the throwaway test wallet (~/.ihatecrypto/test-wallet.txt).
// Nothing is sent without --yes; amounts are capped.
//   new-wallet | address | balances
//   simulate                      build and simulate a SOL transfer and a Jupiter swap, send nothing
//   swap <SOL> [--yes]            SOL → USDC on Solana through Jupiter
//   bridge <SOL> [--yes]          SOL → USDC on HyperCore through NEAR 1Click, timed end to end
//   bridge-usdc <USDC> [--yes]    USDC on Solana → USDC on HyperCore: the app's main route (SPL transfer to the deposit)
//   hl-perp <USDC> [--yes]        HyperCore spot → perp balance
//   hl-trade <USD> [--yes]        open and close an ETH long of about <USD> notional (min $10)
//   hl-agent <USD> [--yes]        approve a fresh agent key once, then the same round trip signed by the agent
//   hl-spot <USDC> [--yes]        HyperCore perp → spot balance (needed before `return`)
//   hl-protect <USD> [--yes]      open a small long, place SL and TP, read them back from the exchange, cancel, close
//   hl-agent-limits               the agent key tries to move funds; Hyperliquid must refuse
//   vaults | lend-position        Jupiter Lend Earn vaults and our position
//   lend <USDC> [--yes]           deposit USDC into Jupiter Lend Earn (jlUSDC)
//   lend-withdraw <USDC> [--yes]  withdraw USDC from it
//   jev-screen <text>             screen untrusted text: local rules, plus Jev if OPENROUTER_API_KEY is set
//   jev-eval <cases.jsonl>        the second check on labelled cases, one JSON line each (Tools/jevcheck)
//   lookup <tool> <arg…>          what one of the model's lookups brings back, as the model gets it: token, services, service, call, rpc, hl, fetch
//   relay <USDC> [--yes]          USDC on Solana → Hyperliquid perp balance through Relay, timed
//   relay-back <USDC> [--yes]     Hyperliquid perp USDC → USDC on Solana through Relay, timed
//   evm-sig                       sign an Arbitrum transfer with a random key; the node must name our address
//   guard-demo                    feed the guard a real transaction with a swapped-in scam address
//   return <USDC> [--yes]         HyperCore spot USDC → Solana USDC through 1Click (spotSend to the deposit address)
//   reconcile                     check every signed-but-unfinished entry of the journal on chain
//   hl-sig                        testnet signature check with a random key
//   stables [network]             the big five stablecoins on every network, quoted live both ways against USDC
//   ledger [sign] [--speculos|--ble] a Ledger's app and address (USB, the emulator or Bluetooth); `sign`: the moves' requests signed on it (see LedgerCheck.swift)

setvbuf(stdout, nil, _IONBF, 0) // show every line even if the process dies
let args = Array(CommandLine.arguments.dropFirst())
let command = args.first ?? "hl-sig"
let amount = args.dropFirst().first.flatMap { Decimal(string: $0) }
let send = args.contains("--yes")
let rpc = SolanaRPC()
let lamportsPerSOL: Decimal = 1_000_000_000
let journal = Journal(url: TestWallet.folder.appendingPathComponent("journal.jsonl"))

do {
    switch command {
    case "new-wallet": try TestWallet.create(); try printAddresses()
    case "address": try printAddresses()
    case "balances": try await printBalances()
    case "simulate": try await simulateAll()
    case "swap": try await swap(sol: try require(amount, max: Decimal(string: "0.05")!))
    case "bridge": try await bridge(sol: try require(amount, max: Decimal(string: "0.15")!))
    case "bridge-usdc": try await bridge(usdc: try require(amount, max: 20))
    case "near-buy": try await bridge(usdc: try require(amount, max: 5), to: OneClick.Asset.near, recipient: try TestWallet.nearAccount(), output: "NEAR")
    case "hl-perp": try await moveToPerp(usdc: try require(amount, max: 50))
    case "hl-trade": try await roundTrip(usd: try require(amount, max: 20))
    case "hl-agent": try await agentRoundTrip(usd: try require(amount, max: 20))
    case "return": try await bridgeBack(usdc: try require(amount, max: 50))
    case "hl-spot": try await moveToSpot(usdc: try require(amount, max: 50))
    case "hl-protect": try await protectionCheck(usd: try require(amount, max: 20))
    case "hl-agent-limits": try await agentLimitsCheck()
    case "vaults": try await printVaults()
    case "lend": try await lend(usdc: try require(amount, max: 5), deposit: true)
    case "lend-withdraw": try await lend(usdc: try require(amount, max: 5), deposit: false)
    case "lend-position": try await printLendPositions()
    case "guard-demo": try await guardDemo()
    case "relay": try await relay(usdc: try require(amount, max: 10))
    case "relay-back": try await relayBack(usdc: try require(amount, max: 10))
    case "evm-sig": try await evmSignatureCheck()
    case "jev-screen":
        let text = args.dropFirst().filter { $0 != "--yes" }.joined(separator: " ")
        let key = ProcessInfo.processInfo.environment["OPENROUTER_API_KEY"]
        printVerdict(await InjectionGuard.screen(text, source: "terminal check", classifier: key.map { JevClassifier(apiKey: $0) }).verdict)
    case "jev-eval": try await jevEval(args.dropFirst().first ?? "")
    case "lookup":
        let tool = args.dropFirst().first ?? "", arg = args.dropFirst(2).joined(separator: " ")
        print(tool == "token" ? await TokenResearch.report(arg) : tool == "services" ? await MPP.search(arg) : tool == "service" ? await MPP.service(arg)
              : await AgentReads.answer(tool, arg, me: .init(evm: nil, solana: nil)))
    case "reconcile": try await reconcile()
    case "hl-sig": try await hyperliquidSignatureCheck()
    case "execute": try await executePlan(args.dropFirst().filter { $0 != "--yes" }.joined(separator: " "))
    case "close": try await closePosition(args.dropFirst().first { $0 != "--yes" } ?? "ETH")
    case "position": try await printPosition()
    case "manage": try await managePosition(args.dropFirst().filter { $0 != "--yes" }.joined(separator: " "))
    case "stables": await stablesCheck(args.dropFirst().first)
    case "move": try await moveMoney(args.dropFirst().filter { $0 != "--yes" }.joined(separator: " "))
    case "plan": try await explainPlan(args.dropFirst().joined(separator: " "))
    case "kamino": try await kaminoCommand(Array(args.dropFirst().filter { !$0.hasPrefix("--") }))
    case "sweep-solana": try await sweepSolana(to: args.dropFirst().first { !$0.hasPrefix("--") } ?? "")
    case "sweep-near": try await sweepNear(to: args.dropFirst().first { !$0.hasPrefix("--") } ?? "")
    case "close-near": try await closeNear(to: args.dropFirst().first { !$0.hasPrefix("--") } ?? "")
    case "sweep-evm": try await sweepEVM(to: args.dropFirst().first { !$0.hasPrefix("--") } ?? "")
    case "sweep-hyperliquid": try await sweepHyperliquid(to: args.dropFirst().first { !$0.hasPrefix("--") } ?? "")
    case "ledger": try await ledgerCheck(Array(args.dropFirst()))
    case "browser-test-page": try await writeBrowserTestPage(args.dropFirst().first ?? "/tmp/ihatecrypto-test.html")
    default: print("Unknown command: \(command)"); exit(2)
    }
} catch {
    fputs("Error: \(error.localizedDescription)\n", stderr)
    exit(1)
}

func require(_ value: Decimal?, max: Decimal) throws -> Decimal {
    guard let value, value > 0 else { throw WalletError("Enter an amount.") }
    guard value <= max else { throw WalletError("Test limit: at most \(max).") }
    return value
}

func lamports(_ sol: Decimal) -> UInt64 { NSDecimalNumber(decimal: sol * lamportsPerSOL).uint64Value }
func sol(_ lamports: UInt64) -> String { Fmt.num(Decimal(lamports) / lamportsPerSOL, maxFraction: 6) }
func stamp() -> String { Date().formatted(date: .omitted, time: .standard) }

func printAddresses() throws {
    let a = try TestWallet.addresses()
    print("Solana (SOL, USDC):  \(a.solana)")
    print("EVM / Hyperliquid:   \(a.evm)")
    print("NEAR (implicit):     \(try TestWallet.nearAccount())")
    print("Phrase: \(TestWallet.file.path) (not printed)")
}

func printBalances() async throws {
    let a = try TestWallet.addresses()
    let lamports = try await rpc.balance(a.solana)
    let usdc = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
    print("Solana       \(sol(lamports)) SOL · \(Fmt.num(Decimal(usdc) / 1_000_000, maxFraction: 6)) USDC")
    if let hl = try? await Hyperliquid.balance(user: a.evm) {
        print("Hyperliquid  perp \(Fmt.usd(hl.accountValue)) (withdrawable \(Fmt.usd(hl.withdrawable))) · spot \(Fmt.num(hl.spotUSDC, maxFraction: 6)) USDC")
    } else {
        print("Hyperliquid  no account yet")
    }
}

/// Keep enough SOL for fees and a token account after any send.
func ensureSOL(spending: UInt64) async throws {
    let reserve: UInt64 = 10_000_000 // 0.01 SOL
    let have = try await rpc.balance(try TestWallet.addresses().solana)
    guard have >= spending + reserve else {
        throw WalletError("Wallet has \(sol(have)) SOL, needs \(sol(spending)) + a 0.01 SOL reserve for fees.")
    }
}

/// Only `guardSolana` can make one: nothing reaches the network without the guard's approval.
struct GuardedTransaction {
    let wire: Data
    let signature: String
    let lastValid: UInt64
    fileprivate init(wire: Data, signature: String, lastValid: UInt64) {
        self.wire = wire
        self.signature = signature
        self.lastValid = lastValid
    }
}

struct Watch {
    let address: String
    let label: String
    let token: Bool
    let atLeast: Int64
}

/// Decode → allowed programs and recipients → simulate → balances against what was approved.
func guardSolana(_ signed: (signature: String, wire: Data), lastValid: UInt64, recipients: Set<String> = [],
                 tokenOwners: [String: String], watch: [Watch]) async throws -> GuardedTransaction {
    let a = try TestWallet.addresses()
    let message = try SolanaMessage.parse(transaction: signed.wire)
    let accounts = try await rpc.resolveAccounts(message)
    var verdict = SolanaGuard.check(message, accounts: accounts, policy: SolanaPolicy(owner: a.solana, recipients: recipients), tokenOwners: tokenOwners)

    var before: [String: Int64] = [:]
    for w in watch {
        if w.token {
            let r = try? await rpc.call("getTokenAccountBalance", [w.address, ["commitment": "confirmed"]]) as? [String: Any]
            before[w.address] = ((r?["value"] as? [String: Any])?["amount"] as? String).flatMap(Int64.init) ?? 0
        } else {
            before[w.address] = Int64(try await rpc.balance(w.address))
        }
    }
    let simulation = try await rpc.simulate(signed.wire, watch: watch.map(\.address))
    if let error = simulation.error { verdict.problems.append("Simulation failed: \(error)") }
    else {
        verdict.checks += SolanaGuard.balances(watch.map { w in
            let after = simulation.after[w.address]
            let value = w.token ? Int64(after?.token ?? 0) : Int64(after?.lamports ?? 0)
            return .init(label: w.label, before: before[w.address] ?? 0, after: value, atLeast: w.atLeast)
        }).checks
        verdict.problems += SolanaGuard.balances(watch.map { w in
            let after = simulation.after[w.address]
            let value = w.token ? Int64(after?.token ?? 0) : Int64(after?.lamports ?? 0)
            return .init(label: w.label, before: before[w.address] ?? 0, after: value, atLeast: w.atLeast)
        }).problems
    }
    printVerdict(verdict)
    guard verdict.ok else { throw WalletError("The guard stopped the transaction.") }
    return GuardedTransaction(wire: signed.wire, signature: signed.signature, lastValid: lastValid)
}

func printVerdict(_ v: Verdict) {
    for c in v.checks { print("  ✓ \(c)") }
    for p in v.problems { print("  ✗ \(p)") }
}

func submit(_ tx: GuardedTransaction, note: String) async throws -> TimeInterval {
    let (wire, signature, lastValidBlockHeight) = (tx.wire, tx.signature, tx.lastValid)
    // Written and flushed before the broadcast: after a crash `reconcile` finds it by signature.
    try journal.append(.init(chain: "solana", id: signature, kind: .signed, note: "\(note); valid until block \(lastValidBlockHeight)"))
    let started = Date()
    _ = try await rpc.send(wire)
    print("[\(stamp())] sent: https://solscan.io/tx/\(signature)")
    switch try await rpc.waitForConfirmation(signature, lastValidBlockHeight: lastValidBlockHeight) {
    case .confirmed: try journal.append(.init(chain: "solana", id: signature, kind: .confirmed))
    case .failed(let e):
        try journal.append(.init(chain: "solana", id: signature, kind: .failed, note: e))
        throw WalletError("Transaction failed: \(e)")
    case .expired:
        try journal.append(.init(chain: "solana", id: signature, kind: .expired))
        throw WalletError("Blockhash expired: the transaction definitely did not land. Safe to retry.")
    }
    let elapsed = Date().timeIntervalSince(started)
    print("[\(stamp())] confirmed in \(String(format: "%.1f", elapsed)) s")
    return elapsed
}

func simulateAll() async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let (hash, _) = try await rpc.latestBlockhash()
    let other = Base58.encode(Data((0..<32).map { _ in UInt8.random(in: 0...255) }))
    let transfer = try SolanaTransaction.transfer(from: keys.solana, to: other, lamports: 1_000_000, blockhash: hash)
    print("SOL transfer:  \(try await rpc.simulate(transfer.wire).error ?? "would pass")")
    let usdc = try SPL.transfer(from: keys.solana, to: other, mint: Jupiter.usdcMint, amount: 1_000_000, decimals: 6, blockhash: hash)
    print("USDC transfer: \(try await rpc.simulate(usdc.wire).error ?? "would pass")")
    let quote = try await Jupiter.quote(input: Jupiter.solMint, output: Jupiter.usdcMint, amount: 20_000_000)
    let tx = try await Jupiter.swapTransaction(quote, user: a.solana)
    let signed = try SolanaTransaction.sign(tx.wire, privateKey: keys.solana)
    print("Jupiter swap:  \(try await rpc.simulate(signed.wire).error ?? "would pass") · route \(quote.route)")
    print("Errors about insufficient funds or a missing account mean the format and signature were accepted.")
}

func swap(sol amount: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let input = lamports(amount)
    let quote = try await Jupiter.quote(input: Jupiter.solMint, output: Jupiter.usdcMint, amount: input)
    print("Jupiter: \(sol(quote.inAmount)) SOL → \(Fmt.num(Decimal(quote.outAmount) / 1_000_000, maxFraction: 6)) USDC, at least \(Fmt.num(Decimal(quote.minOut) / 1_000_000, maxFraction: 6)) · \(quote.route)")
    guard send else { print("Add --yes to send."); return }
    try await ensureSOL(spending: input)
    let usdcBefore = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
    let tx = try await Jupiter.swapTransaction(quote, user: a.solana)
    let signed = try SolanaTransaction.sign(tx.wire, privateKey: keys.solana)
    let myUSDC = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.usdcMint)
    let myWSOL = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.solMint)
    let guarded = try await guardSolana(signed, lastValid: tx.lastValidBlockHeight, tokenOwners: [myUSDC: a.solana, myWSOL: a.solana], watch: [
        Watch(address: myUSDC, label: "USDC (at least the quote minus slippage)", token: true, atLeast: Int64(quote.minOut)),
        Watch(address: a.solana, label: "lamports SOL (no more than the swap amount plus fees)", token: false, atLeast: -Int64(input + 10_000_000)),
    ])
    _ = try await submit(guarded, note: "Jupiter SOL→USDC")
    let got = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint) - usdcBefore
    print("Received \(Fmt.num(Decimal(got) / 1_000_000, maxFraction: 6)) USDC (quoted \(Fmt.num(Decimal(quote.outAmount) / 1_000_000, maxFraction: 6)))")
    try await printBalances()
}

func bridge(sol amount: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let input = lamports(amount)
    try await bridgeToHyperCore(origin: OneClick.Asset.solanaSOL, label: "SOL", amount: amount, units: input) { deposit in
        try await ensureSOL(spending: input)
        let (hash, lastValid) = try await rpc.latestBlockhash()
        let tx = try SolanaTransaction.transfer(from: keys.solana, to: deposit, lamports: input, blockhash: hash)
        return try await guardSolana(tx, lastValid: lastValid, recipients: [deposit], tokenOwners: [:], watch: [
            Watch(address: a.solana, label: "lamports SOL (deposit amount and fee)", token: false, atLeast: -Int64(input + 5_000_000)),
        ])
    }
}

/// Solana USDC through 1Click: to HyperCore USDC by default, or to another asset and recipient (NEAR to our NEAR account).
func bridge(usdc amount: Decimal, to destination: String = OneClick.Asset.hyperCoreUSDC, recipient: String? = nil, output: String = "USDC on HyperCore") async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let units = NSDecimalNumber(decimal: amount * 1_000_000).uint64Value
    try await bridgeToHyperCore(origin: OneClick.Asset.solanaUSDC, label: "USDC", amount: amount, units: units,
                                destination: destination, recipient: recipient ?? a.evm, output: output) { deposit in
        try await ensureSOL(spending: 3_000_000) // the deposit's token account may need creating: ~0.002 SOL rent
        let have = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
        guard have >= units else { throw WalletError("Wallet has \(Decimal(have) / 1_000_000) USDC, needs \(amount).") }
        let (hash, lastValid) = try await rpc.latestBlockhash()
        let tx = try SPL.transfer(from: keys.solana, to: deposit, mint: Jupiter.usdcMint, amount: units, decimals: 6, blockhash: hash)
        let myUSDC = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.usdcMint)
        let theirUSDC = try SPL.associatedAccount(owner: deposit, mint: Jupiter.usdcMint)
        return try await guardSolana(tx, lastValid: lastValid, recipients: [deposit], tokenOwners: [myUSDC: a.solana, theirUSDC: deposit], watch: [
            Watch(address: myUSDC, label: "USDC (exactly the deposit amount)", token: true, atLeast: -Int64(units)),
            Watch(address: a.solana, label: "lamports SOL (fee and the recipient's account)", token: false, atLeast: -5_000_000),
        ])
    }
}

/// Quote the user sees → envelope → live quote checked against it → deposit (written to the journal first)
/// → 1Click status until final. Timed end to end.
func bridgeToHyperCore(origin: String, label: String, amount: Decimal, units: UInt64,
                       destination: String = OneClick.Asset.hyperCoreUSDC, recipient: String? = nil, output: String = "USDC on HyperCore",
                       deposit makeTransaction: (String) async throws -> GuardedTransaction) async throws {
    let a = try TestWallet.addresses()
    let recipient = recipient ?? a.evm
    let shown = try await OneClick.quote(dry: true, origin: origin, destination: destination,
                                         amount: "\(units)", refundTo: a.solana, recipient: recipient)
    print("1Click: \(shown.amountInFormatted) \(label) → \(shown.amountOutFormatted) \(output) (to \(recipient)), ≈ \(shown.timeEstimate ?? 0) s after deposit")
    guard send else { print("Add --yes to get a deposit address and send."); return }
    // The user approved what they saw, with 1% room: the live quote must fit, or nothing is sent.
    var envelope = Envelope.approve(input: amount, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
    let quote = try await OneClick.quote(dry: false, origin: origin, destination: destination,
                                         amount: "\(units)", refundTo: a.solana, recipient: recipient)
    guard let deposit = quote.depositAddress else { throw WalletError("Quote has no deposit address.") }
    envelope.destination = deposit
    let problems = envelope.violations(input: Decimal(string: quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                       output: Decimal(string: quote.amountOutFormatted) ?? 0, destination: deposit)
    guard problems.isEmpty else { throw WalletError("Outside the approved envelope: " + problems.joined(separator: " ")) }
    print("Live quote within the envelope: \(quote.amountOutFormatted) \(output) arrives (at least \(envelope.minOutput)), deposit \(deposit)")
    // The deposit address must be bound, by 1Click itself, to a quote with our recipient and refund address.
    let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: recipient, refundTo: a.solana,
                                     origin: origin, destination: destination, amount: "\(units)")
    printVerdict(binding)
    guard binding.ok else { throw WalletError("The guard stopped the deposit: the address is not bound to our quote.") }

    let tx = try await makeTransaction(deposit)
    let started = Date()
    try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "\(label)→\(output), deposit tx \(tx.signature)"))
    _ = try await submit(tx, note: "1Click deposit \(deposit)")
    try? await OneClick.submitDeposit(txHash: tx.signature, depositAddress: deposit)

    var last = ""
    while true {
        let s = try await OneClick.status(depositAddress: deposit)
        if s.status != last {
            print("[\(stamp())] +\(Int(Date().timeIntervalSince(started))) s  \(s.status)")
            last = s.status
        }
        if s.isFinal {
            try journal.append(.init(chain: "1click", id: deposit, kind: .settled, note: s.status))
            if s.status == "SUCCESS" { print("Credited \(s.amountOutFormatted ?? "?") \(output) · \(s.destinationTxs.joined(separator: ", "))") }
            if s.status == "REFUNDED" { print("Refunded \(s.refundedFormatted ?? "?") to \(a.solana)") }
            break
        }
        guard Date().timeIntervalSince(started) < 900 else { print("15 minutes without a final status, exiting. Status: \(s.status)"); break }
        try await Task.sleep(for: .seconds(3))
    }
    print("Total from send to final status: \(Int(Date().timeIntervalSince(started))) s")
    if destination == OneClick.Asset.near, let b = try? await Near.balance(account: recipient) {
        print("NEAR account: \(Fmt.num(b.near, maxFraction: 6)) NEAR native, \(Fmt.num(b.wrapped, maxFraction: 6)) wNEAR")
    }
    try await printBalances()
}

func postExchange(_ action: MsgPack, nonce: UInt64, signature: EVMSignature, recipients: Set<String> = [], agent: String? = nil) async throws -> String {
    let verdict = HLGuard.check(action, owner: try TestWallet.addresses().evm, recipients: recipients, agent: agent)
    guard verdict.ok else { printVerdict(verdict); throw WalletError("The guard stopped the Hyperliquid action.") }
    return String(decoding: try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true), as: UTF8.self)
}

func moveToPerp(usdc: Decimal) async throws {
    let keys = try TestWallet.keys()
    print("Moving \(usdc) USDC from spot to the Hyperliquid perp balance")
    guard send else { print("Add --yes."); return }
    let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
    let (action, signature) = try HLSigner.usdClassTransfer(amount: usdc, toPerp: true, nonce: nonce, privateKey: keys.evm, mainnet: true)
    print("Response: \(try await postExchange(action, nonce: nonce, signature: signature))")
    try await printBalances()
}

func roundTrip(usd: Decimal, signingKey: Data? = nil) async throws {
    let keys = try TestWallet.keys()
    let signer = signingKey ?? keys.evm
    struct Asset: Decodable { let name: String; let szDecimals: Int }
    struct Meta: Decodable { let universe: [Asset] }
    let meta = try JSONDecoder().decode(Meta.self, from: try await infoRequest(#"{"type":"meta"}"#))
    guard let index = meta.universe.firstIndex(where: { $0.name == "ETH" }) else { throw WalletError("No ETH") }
    let szDecimals = meta.universe[index].szDecimals
    guard let mark = try await Hyperliquid.markets()["ETH"]?.markPx else { throw WalletError("No ETH price") }
    let size = HLAction.size(usd / mark + Decimal(string: "0.0001")!, szDecimals: szDecimals)
    let buyPx = HLAction.price(mark * Decimal(string: "1.01")!, szDecimals: szDecimals)
    let sellPx = HLAction.price(mark * Decimal(string: "0.99")!, szDecimals: szDecimals)
    print("ETH mark \(mark): buy \(size) ETH (≈ \(Fmt.usd(size * mark))) with IOC ≤ \(buyPx), then close with IOC ≥ \(sellPx)")
    guard send else { print("Add --yes."); return }

    for (label, isBuy, px, reduce) in [("Open", true, buyPx, false), ("Close", false, sellPx, true)] {
        let action = try HLAction.order([HLOrder(asset: index, isBuy: isBuy, price: px, size: size, reduceOnly: reduce, kind: .limit(tif: "Ioc"))])
        let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
        let signature = try HLSigner.signL1(action, privateKey: signer, nonce: nonce, mainnet: true)
        print("[\(stamp())] \(label): \(try await postExchange(action, nonce: nonce, signature: signature))")
        try await Task.sleep(for: .seconds(2))
    }
    try await printBalances()
}

func infoRequest(_ body: String, testnet: Bool = false) async throws -> Data {
    var request = URLRequest(url: URL(string: testnet ? "https://api.hyperliquid-testnet.xyz/info" : "https://api.hyperliquid.xyz/info")!)
    request.httpMethod = "POST"
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = Data(body.utf8)
    return try await URLSession.shared.data(for: request).0
}

func hyperliquidSignatureCheck() async throws {
    var generator = SystemRandomNumberGenerator()
    let key = Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &generator) })
    let address = try EVMAccount.address(privateKey: key)
    struct Asset: Decodable { let name: String }
    struct Meta: Decodable { let universe: [Asset] }
    let meta = try JSONDecoder().decode(Meta.self, from: try await infoRequest(#"{"type":"meta"}"#, testnet: true))
    guard let eth = meta.universe.firstIndex(where: { $0.name == "ETH" }) else { throw WalletError("No ETH on testnet") }
    let action = try HLAction.order([HLOrder(asset: eth, isBuy: true, price: 100, size: Decimal(string: "0.01")!, reduceOnly: false, kind: .limit(tif: "Gtc"))])
    let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
    let signature = try HLSigner.signL1(action, privateKey: key, nonce: nonce, mainnet: false)
    print("Our address: \(address)")
    func check(_ label: String, _ action: MsgPack, _ nonce: UInt64, _ signature: EVMSignature) async throws {
        let reply = String(decoding: try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: false), as: UTF8.self)
        let ok = reply.lowercased().contains(address.lowercased().dropFirst(2))
        print("\(ok ? "✓" : "✗") \(label): \(reply)")
    }
    try await check("order", action, nonce, signature)
    let t = UInt64(Date().timeIntervalSince1970 * 1000)
    let (toPerp, s1) = try HLSigner.usdClassTransfer(amount: 1, toPerp: true, nonce: t, privateKey: key, mainnet: false)
    try await check("spot → perp", toPerp, t, s1)
    let (spot, s2) = try HLSigner.spotSend(destination: "0x0000000000000000000000000000000000000001", token: "USDC:0xeb62eee3685fc4c43992febcd9e75443",
                                           amount: 1, time: t + 1, privateKey: key, mainnet: false)
    try await check("spotSend", spot, t + 1, s2)
    let (approve, s3) = try HLSigner.approveAgent(agent: "0x0000000000000000000000000000000000000002", name: "ihatecrypto", nonce: t + 2, privateKey: key, mainnet: false)
    try await check("agent approval", approve, t + 2, s3)
}

func reconcile() async throws {
    let open = try journal.unresolved()
    if open.isEmpty { print("No unfinished sends in the journal."); return }
    for entry in open {
        switch entry.chain {
        case "solana":
            let r = try await rpc.call("getSignatureStatuses", [[entry.id], ["searchTransactionHistory": true]]) as? [String: Any]
            let status = (r?["value"] as? [Any])?.first as? [String: Any]
            let kind: Journal.Entry.Kind? = status == nil ? nil : (status?["err"] is NSNull || status?["err"] == nil ? .confirmed : .failed)
            print("solana \(entry.id.prefix(12))…  \(kind?.rawValue ?? "not found on chain")  (\(entry.note))")
            if let kind { try journal.append(.init(chain: "solana", id: entry.id, kind: kind, note: "reconcile")) }
        case "1click":
            let s = try await OneClick.status(depositAddress: entry.id)
            print("1click \(entry.id.prefix(12))…  \(s.status)")
            if s.isFinal { try journal.append(.init(chain: "1click", id: entry.id, kind: .settled, note: s.status)) }
        default:
            print("\(entry.chain) \(entry.id): check manually")
        }
    }
}

/// Agent wallet: the main key approves a separate trading key once. The agent can trade, not withdraw.
func agentRoundTrip(usd: Decimal) async throws {
    let keys = try TestWallet.keys()
    let file = TestWallet.folder.appendingPathComponent("hl-agent-key.txt")
    let agentKey: Data
    if let hex = try? String(contentsOf: file, encoding: .utf8), let k = Data(hex: hex.trimmingCharacters(in: .whitespacesAndNewlines)) {
        agentKey = k
    } else {
        var g = SystemRandomNumberGenerator()
        agentKey = Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &g) })
        guard send else { print("An agent key will be created and approved by the main key. Add --yes."); return }
        FileManager.default.createFile(atPath: file.path, contents: Data(agentKey.hex.utf8), attributes: [.posixPermissions: 0o600])
    }
    let agent = try EVMAccount.address(privateKey: agentKey)
    print("Agent: \(agent)")
    guard send else { print("Add --yes."); return }
    let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
    let (action, signature) = try HLSigner.approveAgent(agent: agent, name: "ihatecrypto", nonce: nonce, privateKey: keys.evm, mainnet: true)
    print("[\(stamp())] Agent approval: \(try await postExchange(action, nonce: nonce, signature: signature, agent: agent))")
    try await Task.sleep(for: .seconds(2))
    try await roundTrip(usd: usd, signingKey: agentKey)
}

/// The way back: HyperCore spot USDC → 1Click deposit address (spotSend, gasless) → USDC on Solana.
func bridgeBack(usdc: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let units = "\(NSDecimalNumber(decimal: usdc * 100_000_000).uint64Value)" // HyperCore USDC: 8 decimals
    let shown = try await OneClick.quote(dry: true, origin: OneClick.Asset.hyperCoreUSDC, destination: OneClick.Asset.solanaUSDC,
                                         amount: units, refundTo: a.evm, recipient: a.solana)
    print("1Click: \(shown.amountInFormatted) USDC from HyperCore → \(shown.amountOutFormatted) USDC on Solana, ≈ \(shown.timeEstimate ?? 0) s")
    guard send else { print("Add --yes."); return }
    var envelope = Envelope.approve(input: usdc, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
    let quote = try await OneClick.quote(dry: false, origin: OneClick.Asset.hyperCoreUSDC, destination: OneClick.Asset.solanaUSDC,
                                         amount: units, refundTo: a.evm, recipient: a.solana)
    guard let deposit = quote.depositAddress else { throw WalletError("Quote has no deposit address.") }
    envelope.destination = deposit
    let problems = envelope.violations(input: Decimal(string: quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                       output: Decimal(string: quote.amountOutFormatted) ?? 0, destination: deposit)
    guard problems.isEmpty else { throw WalletError("Outside the approved envelope: " + problems.joined(separator: " ")) }

    let time = UInt64(Date().timeIntervalSince1970 * 1000)
    let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: a.solana, refundTo: a.evm,
                                     origin: OneClick.Asset.hyperCoreUSDC, destination: OneClick.Asset.solanaUSDC, amount: units)
    printVerdict(binding)
    guard binding.ok else { throw WalletError("The guard stopped the return: the address is not bound to our quote.") }
    let (action, signature) = try HLSigner.spotSend(destination: deposit, token: "USDC:0x6d1e7cde53ba9467b783cb7c530ce054",
                                                    amount: usdc, time: time, privateKey: keys.evm, mainnet: true)
    try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "HyperCore→Solana USDC, spotSend \(usdc)"))
    let started = Date()
    print("[\(stamp())] spotSend to \(deposit): \(try await postExchange(action, nonce: time, signature: signature, recipients: [deposit]))")
    var last = ""
    while Date().timeIntervalSince(started) < 900 {
        let s = try await OneClick.status(depositAddress: deposit)
        if s.status != last { print("[\(stamp())] +\(Int(Date().timeIntervalSince(started))) s  \(s.status)"); last = s.status }
        if s.isFinal {
            try journal.append(.init(chain: "1click", id: deposit, kind: .settled, note: s.status))
            print("Result: \(s.status), received \(s.amountOutFormatted ?? "?") USDC")
            break
        }
        try await Task.sleep(for: .seconds(3))
    }
    try await printBalances()
}

func moveToSpot(usdc: Decimal) async throws {
    let keys = try TestWallet.keys()
    print("Moving \(usdc) USDC from the Hyperliquid perp balance to spot")
    guard send else { print("Add --yes."); return }
    let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
    let (action, signature) = try HLSigner.usdClassTransfer(amount: usdc, toPerp: false, nonce: nonce, privateKey: keys.evm, mainnet: true)
    print("Response: \(try await postExchange(action, nonce: nonce, signature: signature))")
    try await printBalances()
}

func agentKey() throws -> Data {
    let file = TestWallet.folder.appendingPathComponent("hl-agent-key.txt")
    guard let hex = try? String(contentsOf: file, encoding: .utf8), let key = Data(hex: hex.trimmingCharacters(in: .whitespacesAndNewlines)) else {
        throw WalletError("Run hl-agent first: there is no agent key.")
    }
    return key
}

/// The point of an agent wallet: it trades for the account but cannot move money out.
func agentLimitsCheck() async throws {
    let agent = try agentKey()
    let a = try TestWallet.addresses()
    let t = UInt64(Date().timeIntervalSince1970 * 1000)
    let (toSpot, s1) = try HLSigner.usdClassTransfer(amount: 1, toPerp: false, nonce: t, privateKey: agent, mainnet: true)
    print("Agent moves perp → spot: \(String(decoding: try await HLExchange.send(action: toSpot, nonce: t, signature: s1, mainnet: true), as: UTF8.self))")
    let (spot, s2) = try HLSigner.spotSend(destination: "0x0000000000000000000000000000000000000001", token: "USDC:0x6d1e7cde53ba9467b783cb7c530ce054",
                                           amount: 1, time: t + 1, privateKey: agent, mainnet: true)
    print("Agent sends USDC out: \(String(decoding: try await HLExchange.send(action: spot, nonce: t + 1, signature: s2, mainnet: true), as: UTF8.self))")
    print("The local guard would block this too: \(HLGuard.check(spot, owner: a.evm, recipients: []).problems.joined())")
    try await printBalances()
    print("Expected: both refused, balance of \(a.evm.prefix(8))… unchanged.")
}

struct OpenOrder: Decodable {
    let coin: String; let side: String; let sz: String; let oid: UInt64
    let isTrigger: Bool?; let triggerPx: String?; let orderType: String?; let reduceOnly: Bool?; let isPositionTpsl: Bool?
}

func openOrders(_ user: String) async throws -> [OpenOrder] {
    try JSONDecoder().decode([OpenOrder].self, from: try await infoRequest(#"{"type":"frontendOpenOrders","user":"\#(user)"}"#))
}

/// The product's promise: a position is shown as protected only when SL and TP are actually on the exchange.
func protectionCheck(usd: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    struct Asset: Decodable { let name: String; let szDecimals: Int }
    struct Meta: Decodable { let universe: [Asset] }
    let meta = try JSONDecoder().decode(Meta.self, from: try await infoRequest(#"{"type":"meta"}"#))
    guard let index = meta.universe.firstIndex(where: { $0.name == "ETH" }), let mark = try await Hyperliquid.markets()["ETH"]?.markPx else {
        throw WalletError("No ETH data")
    }
    let szd = meta.universe[index].szDecimals
    let size = HLAction.size(usd / mark + Decimal(string: "0.0001")!, szDecimals: szd)
    let stop = HLAction.price(mark * Decimal(string: "0.97")!, szDecimals: szd)
    let take = HLAction.price(mark * Decimal(string: "1.06")!, szDecimals: szd)
    print("Long \(size) ETH at ≈ \(mark), stop \(stop) (−3%), take-profit \(take) (+6%)")
    guard send else { print("Add --yes."); return }

    func post(_ label: String, _ action: MsgPack) async throws -> String {
        let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
        let reply = try await postExchange(action, nonce: nonce, signature: try HLSigner.signL1(action, privateKey: keys.evm, nonce: nonce, mainnet: true))
        print("[\(stamp())] \(label): \(reply)")
        try await Task.sleep(for: .seconds(1))
        return reply
    }

    _ = try await post("Open", try HLAction.order([HLOrder(asset: index, isBuy: true, price: HLAction.price(mark * Decimal(string: "1.01")!, szDecimals: szd),
                                                            size: size, reduceOnly: false, kind: .limit(tif: "Ioc"))]))
    _ = try await post("Stop and take-profit", try HLAction.order([
        HLOrder(asset: index, isBuy: false, price: stop, size: size, reduceOnly: true, kind: .trigger(price: stop, isMarket: true, tpsl: "sl")),
        HLOrder(asset: index, isBuy: false, price: take, size: size, reduceOnly: true, kind: .trigger(price: take, isMarket: true, tpsl: "tp")),
    ], grouping: "positionTpsl"))

    let orders = try await openOrders(a.evm).filter { $0.coin == "ETH" && $0.isTrigger == true }
    for o in orders { print("  on the exchange: \(o.orderType ?? "?") \(o.side) \(o.sz) trigger \(o.triggerPx ?? "?") reduceOnly=\(o.reduceOnly ?? false)") }
    let hasStop = orders.contains { Decimal(string: $0.triggerPx ?? "") == stop }
    let hasTake = orders.contains { Decimal(string: $0.triggerPx ?? "") == take }
    print(hasStop && hasTake ? "✓ Position protected: stop and take-profit confirmed by the exchange." : "✗ Position NOT protected: no stop or take-profit on the exchange.")

    if !orders.isEmpty { _ = try await post("Cancel stop and take-profit", HLAction.cancel(orders.map { (index, $0.oid) })) }
    _ = try await post("Close", try HLAction.order([HLOrder(asset: index, isBuy: false, price: HLAction.price(mark * Decimal(string: "0.99")!, szDecimals: szd),
                                                            size: size, reduceOnly: true, kind: .limit(tif: "Ioc"))]))
    print("Open ETH orders afterwards: \(try await openOrders(a.evm).filter { $0.coin == "ETH" }.count)")
    try await printBalances()
}

func printVaults() async throws {
    for v in try await JupiterLend.vaults() {
        print("\(v.symbol.padding(toLength: 9, withPad: " ", startingAt: 0)) \(v.assetSymbol.padding(toLength: 7, withPad: " ", startingAt: 0)) \(Fmt.num(v.apy))% APY  \(v.address)")
    }
}

func printLendPositions() async throws {
    let positions = try await JupiterLend.positions(user: try TestWallet.addresses().solana).filter { $0.shares > 0 }
    if positions.isEmpty { print("No Jupiter Lend positions.") }
    for p in positions { print("\(p.vault): \(Fmt.num(Decimal(p.assets) / 1_000_000, maxFraction: 6)) USDC (shares \(p.shares))") }
}

/// DeFi, first protocol: Jupiter Lend Earn. The same guard as swaps: allowed programs, simulated balances.
func lend(usdc: Decimal, deposit: Bool) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    guard let vault = try await JupiterLend.vaults().first(where: { $0.assetMint == Jupiter.usdcMint }) else { throw WalletError("No USDC vault") }
    let units = NSDecimalNumber(decimal: usdc * 1_000_000).uint64Value
    print("\(deposit ? "Deposit" : "Withdraw") \(usdc) USDC \(deposit ? "into" : "from") \(vault.symbol) (\(Fmt.num(vault.apy))% APY)")
    guard send else { print("Add --yes."); return }
    try await ensureSOL(spending: 0)
    let myShares = try SPL.associatedAccount(owner: a.solana, mint: vault.address)
    if deposit, (try? await rpc.call("getAccountInfo", [myShares, ["encoding": "base64"]]) as? [String: Any])?["value"] is NSNull {
        // The API's deposit expects our share account to exist; Jupiter's own site creates it first.
        print("Creating your \(vault.symbol) share account: \(myShares)")
        let (hash, lastValid) = try await rpc.latestBlockhash()
        let create = try SPL.createOwnAccount(privateKey: keys.solana, mint: vault.address, blockhash: hash)
        let guarded = try await guardSolana(create, lastValid: lastValid, tokenOwners: [myShares: a.solana], watch: [
            Watch(address: a.solana, label: "lamports SOL (account rent and fee)", token: false, atLeast: -4_000_000),
        ])
        _ = try await submit(guarded, note: "create \(vault.symbol) account")
    }
    let wire = deposit
        ? try await JupiterLend.depositTransaction(asset: Jupiter.usdcMint, user: a.solana, amount: units)
        : try await JupiterLend.withdrawTransaction(asset: Jupiter.usdcMint, user: a.solana, amount: units)
    let signed = try SolanaTransaction.sign(wire, privateKey: keys.solana)
    let myUSDC = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.usdcMint)
    let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
    let guarded = try await guardSolana(signed, lastValid: lastValid, tokenOwners: [myUSDC: a.solana, myShares: a.solana], watch: deposit ? [
        Watch(address: myUSDC, label: "USDC (exactly the deposit amount)", token: true, atLeast: -Int64(units)),
        Watch(address: myShares, label: "\(vault.symbol) shares (must come to you)", token: true, atLeast: 1),
        Watch(address: a.solana, label: "lamports SOL (fee and share account)", token: false, atLeast: -5_000_000),
    ] : [
        Watch(address: myUSDC, label: "USDC (must come back to you)", token: true, atLeast: Int64(units) - 1),
        Watch(address: a.solana, label: "lamports SOL (fee)", token: false, atLeast: -5_000_000),
    ])
    _ = try await submit(guarded, note: "Jupiter Lend \(deposit ? "deposit" : "withdraw") \(usdc) USDC")
    try await printLendPositions()
    try await printBalances()
}

/// Takes a real Jupiter swap for our wallet, swaps one account for a scam address the way an injected page
/// or a tampered API would, and shows the guard's verdict. Nothing is sent.
func guardDemo() async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let (hash, lastValid) = try await rpc.latestBlockhash()
    let scam = Base58.encode(Data((0..<32).map { _ in UInt8.random(in: 0...255) }))
    print("1) A SOL transfer to a deposit address, with the recipient swapped in the transaction:")
    let bad = try SolanaTransaction.transfer(from: keys.solana, to: scam, lamports: 1_000_000, blockhash: hash)
    _ = try? await guardSolana(bad, lastValid: lastValid, recipients: ["5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw"], tokenOwners: [:], watch: [])
    print("2) A genuine Jupiter swap, untampered:")
    let quote = try await Jupiter.quote(input: Jupiter.solMint, output: Jupiter.usdcMint, amount: 5_000_000)
    let tx = try await Jupiter.swapTransaction(quote, user: a.solana)
    let signed = try SolanaTransaction.sign(tx.wire, privateKey: keys.solana)
    let myUSDC = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.usdcMint)
    let myWSOL = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.solMint)
    _ = try? await guardSolana(signed, lastValid: tx.lastValidBlockHeight, tokenOwners: [myUSDC: a.solana, myWSOL: a.solana], watch: [
        Watch(address: myUSDC, label: "USDC", token: true, atLeast: Int64(quote.minOut)),
    ])
    print("3) A 1Click deposit address from someone else's quote (the recipient is not us):")
    let foreign = try await OneClick.boundRequest(depositAddress: "9Ra5yK3PdSZkrfxujnnyF7nbMBz9HxUE9mzXPRJg7uA9")
    printVerdict(DepositGuard.check(bound: foreign, recipient: a.evm, refundTo: a.solana, origin: OneClick.Asset.solanaUSDC,
                                    destination: OneClick.Asset.hyperCoreUSDC, amount: "1000000000"))
}

/// Relay: a solver pays out on Hyperliquid within seconds; our USDC goes to Relay's Depository on Solana.
func relay(usdc: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let units = NSDecimalNumber(decimal: usdc * 1_000_000).uint64Value
    func quote() async throws -> Relay.Quote {
        try await Relay.quote(user: a.solana, recipient: a.evm, originChain: Relay.Chain.solana, destinationChain: Relay.Chain.hyperliquid,
                              originCurrency: Relay.Currency.solanaUSDC, destinationCurrency: Relay.Currency.hyperliquidUSDC, amount: units)
    }
    let shown = try await quote()
    print("Relay: \(shown.amountInFormatted) USDC → \(shown.amountOutFormatted) USDC on Hyperliquid, fee $\(shown.feesUSD), estimate \(shown.seconds) s")
    guard send else { print("Add --yes."); return }
    let have = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
    guard have >= units else { throw WalletError("Wallet has \(Decimal(have) / 1_000_000) USDC, needs \(usdc).") }

    let envelope = Envelope.approve(input: usdc, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
    let live = try await quote()
    let binding = BindingGuard.check(["sender": live.sender, "recipient": live.recipient, "amountIn": live.amountIn],
                                     expected: ["sender": a.solana, "recipient": a.evm, "amountIn": "\(units)"], subject: "Relay quote")
    printVerdict(binding)
    let problems = envelope.violations(input: usdc, output: Decimal(string: live.amountOutFormatted) ?? 0)
    guard binding.ok, problems.isEmpty else { throw WalletError("The guard stopped Relay: " + (binding.problems + problems).joined(separator: " ")) }

    let (hash, lastValid) = try await rpc.latestBlockhash()
    let tx = try SolanaTransaction.compile(live.solanaInstructions, privateKey: keys.solana, blockhash: hash)
    let myUSDC = try SPL.associatedAccount(owner: a.solana, mint: Jupiter.usdcMint)
    let guarded = try await guardSolana(tx, lastValid: lastValid, tokenOwners: [myUSDC: a.solana], watch: [
        Watch(address: myUSDC, label: "USDC (exactly the deposit amount)", token: true, atLeast: -Int64(units)),
        Watch(address: a.solana, label: "lamports SOL (fee)", token: false, atLeast: -5_000_000),
    ])
    let hlBefore = try? await Hyperliquid.balance(user: a.evm).accountValue
    try journal.append(.init(chain: "relay", id: live.requestId, kind: .signed, note: "Solana USDC→Hyperliquid \(usdc), tx \(tx.signature)"))
    let started = Date()
    _ = try await submit(guarded, note: "Relay deposit \(live.requestId)")
    var last = ""
    while Date().timeIntervalSince(started) < 300 {
        let s = try await Relay.status(requestId: live.requestId)
        if s.status != last { print("[\(stamp())] +\(String(format: "%.1f", Date().timeIntervalSince(started))) s  \(s.status)"); last = s.status }
        if ["success", "refund", "failure"].contains(s.status) {
            try journal.append(.init(chain: "relay", id: live.requestId, kind: .settled, note: s.status))
            print("Result: \(s.status) \(s.destinationTxs.joined(separator: ", "))")
            break
        }
        try await Task.sleep(for: .seconds(1))
    }
    if let before = hlBefore, let after = try? await Hyperliquid.balance(user: a.evm).accountValue {
        print("Hyperliquid perp: \(Fmt.usd(before)) → \(Fmt.usd(after)) (+\(Fmt.num(after - before, maxFraction: 6)))")
    }
    try await printBalances()
}

func relayBack(usdc: Decimal) async throws {
    let keys = try TestWallet.keys()
    let a = try TestWallet.addresses()
    let units = NSDecimalNumber(decimal: usdc * 100_000_000).uint64Value // Hyperliquid perp USDC: 8 decimals in Relay
    func quote() async throws -> Relay.Quote {
        try await Relay.quote(user: a.evm, recipient: a.solana, originChain: Relay.Chain.hyperliquid, destinationChain: Relay.Chain.solana,
                              originCurrency: Relay.Currency.hyperliquidUSDC, destinationCurrency: Relay.Currency.solanaUSDC, amount: units)
    }
    let shown = try await quote()
    print("Relay: \(shown.amountInFormatted) USDC from Hyperliquid → \(shown.amountOutFormatted) USDC on Solana, fee $\(shown.feesUSD), estimate \(shown.seconds) s")
    guard send else { print("Add --yes."); return }
    let envelope = Envelope.approve(input: usdc, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
    let live = try await quote()
    guard let hl = live.hyperliquid else { throw WalletError("Quote has no Hyperliquid steps.") }
    let published = try await Relay.depository(chainId: Relay.Chain.hyperliquid)?.lowercased()
    var verdict = BindingGuard.check(
        ["sender": live.sender, "recipient": live.recipient, "amountIn": live.amountIn, "wallet": hl.wallet, "depositor": hl.depositor,
         "destination": hl.destination, "published depository": published ?? "none", "token": hl.token],
        expected: ["sender": a.evm, "recipient": a.solana, "amountIn": "\(units)", "wallet": a.evm, "depositor": a.evm,
                   "destination": Relay.hyperliquidDepository, "published depository": Relay.hyperliquidDepository,
                   "token": "USDC:0x6d1e7cde53ba9467b783cb7c530ce054"],
        subject: "Relay quote")
    if Decimal(string: hl.amount) != usdc { verdict.problems.append("sendAsset amount \(hl.amount) instead of \(usdc).") }
    verdict.problems += envelope.violations(input: usdc, output: Decimal(string: live.amountOutFormatted) ?? 0)
    printVerdict(verdict)
    guard verdict.ok else { throw WalletError("The guard stopped the return through Relay.") }

    // 1) Tie the deposit to the order: EIP-712 RelayNonceMapping, posted to Relay.
    guard let id = Data(hex: hl.mappingId), id.count == 32 else { throw WalletError("Invalid mapping id") }
    let digest = try EIP712.digest(
        domain: [.init("name", "string", .string("RelayNonceMapping")), .init("version", "string", .string("2")),
                 .init("chainId", "uint256", .uint(1)), .init("verifyingContract", "address", .address("0x0000000000000000000000000000000000000000"))],
        type: "NonceMapping",
        message: [.init("chainId", "string", .string("hyperliquid")), .init("wallet", "address", .address(hl.wallet)),
                  .init("depositor", "address", .address(hl.depositor)), .init("id", "bytes32", .bytes32(id)),
                  .init("nonce", "uint256", .uint(hl.mappingNonce))])
    let mappingSignature = try EVMSigner.sign(digest: digest, privateKey: keys.evm)
    try await Relay.authorize(body: hl.authorizeBody, signature: "0x" + mappingSignature.r.hex + mappingSignature.s.hex + String(format: "%02x", mappingSignature.v))
    print("[\(stamp())] deposit-to-order mapping signed")

    // 2) sendAsset from the perp balance to Relay's depository, through the Hyperliquid guard.
    let (action, signature) = try HLSigner.sendAsset(destination: hl.destination, sourceDex: hl.sourceDex, destinationDex: hl.destinationDex,
                                                     token: hl.token, amount: hl.amount, nonce: hl.nonce, privateKey: keys.evm, mainnet: true)
    let usdcBefore = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
    try journal.append(.init(chain: "relay", id: live.requestId, kind: .signed, note: "Hyperliquid→Solana USDC \(usdc)"))
    let started = Date()
    print("[\(stamp())] sendAsset: \(try await postExchange(action, nonce: hl.nonce, signature: signature, recipients: [Relay.hyperliquidDepository]))")
    var last = ""
    while Date().timeIntervalSince(started) < 300 {
        let s = try await Relay.status(requestId: live.requestId)
        if s.status != last { print("[\(stamp())] +\(String(format: "%.1f", Date().timeIntervalSince(started))) s  \(s.status)"); last = s.status }
        if ["success", "refund", "failure"].contains(s.status) {
            try journal.append(.init(chain: "relay", id: live.requestId, kind: .settled, note: s.status))
            print("Result: \(s.status) \(s.destinationTxs.joined(separator: ", "))")
            break
        }
        try await Task.sleep(for: .seconds(1))
    }
    let got = try await rpc.tokenBalance(owner: a.solana, mint: Jupiter.usdcMint)
    print("Solana USDC: +\(Fmt.num(Decimal(Int64(got) - Int64(usdcBefore)) / 1_000_000, maxFraction: 6))")
    try await printBalances()
}

/// Like hl-sig for Arbitrum: an unfunded key's signed EIP-1559 transfer is refused for lack of funds,
/// and the node names the sender it recovered from the signature.
func evmSignatureCheck() async throws {
    var g = SystemRandomNumberGenerator()
    let key = Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &g) })
    let address = try EVMAccount.address(privateKey: key)
    let chain = EVMRPC.arbitrum
    let fees = try await chain.fees()
    for (label, to, value, data) in [
        ("ETH transfer", "0x0000000000000000000000000000000000000001", UInt64(1_000_000_000_000), Data()),
        ("ERC-20 USDC", EVMRPC.arbitrumUSDC, UInt64(0), try ERC20.transfer(to: "0x0000000000000000000000000000000000000001", amount: 1_000_000)),
    ] {
        let tx = EVMTransaction(chainId: chain.chainId, nonce: 0, maxPriorityFeePerGas: fees.maxPriority, maxFeePerGas: fees.maxFee,
                                gasLimit: 100_000, to: to, value: value, data: data)
        let (raw, hash) = try tx.signed(privateKey: key)
        do {
            _ = try await chain.send(raw)
            print("✗ \(label): unexpectedly accepted \(hash)")
        } catch {
            let text = error.localizedDescription
            print("\(text.lowercased().contains(address.lowercased().dropFirst(2)) ? "✓" : "?") \(label): \(text)")
        }
    }
    print("Our address: \(address)")
}
