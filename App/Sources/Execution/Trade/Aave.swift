import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import RelayBridge
import SolanaChain
import TxGuard
import WalletCore

/// An EVM network ihatecrypto moves USDC on: where to read it, where Relay reaches it, how much gas to keep there.
public struct EVMNetwork: Sendable {
    public let name: String
    /// The journal's chain name.
    public let key: String
    public let rpc: EVMRPC
    public let usdc: String
    public let relayChain: UInt64
    public let explorer: String
    /// ETH to keep for several transactions; below it a bridge brings a top-up along.
    public let gasFloor: UInt64
    /// What the top-up is worth in USD (it must land above the floor).
    public let topupUSD: Decimal
    /// The coin gas is paid in.
    public var gasCoin = "ETH"
    /// A network with no coin of its own (Tempo): fees come out of this stablecoin, so there's no gas to bring.
    public var feeToken: String?
    /// Its dollar's decimals: 6 everywhere but BNB Chain, whose Binance-Peg USDC has 18. The app counts dollars in
    /// millionths on every network (UInt64); `raw`, `micro` and `usdcBalance` cross to the token's own units at the chain.
    public var usdcDecimals = 6

    public static let base = EVMNetwork(name: "Base", key: "base", rpc: .base, usdc: EVMRPC.baseUSDC, relayChain: Relay.Chain.base,
                                        explorer: "https://basescan.org", gasFloor: 5_000_000_000_000, topupUSD: Decimal(string: "0.03")!)
    /// Gas on Arbitrum costs a few times Base's (its L1 data share): a larger floor and top-up.
    public static let arbitrum = EVMNetwork(name: "Arbitrum", key: "arbitrum", rpc: .arbitrum, usdc: EVMRPC.arbitrumUSDC, relayChain: Relay.Chain.arbitrum,
                                            explorer: "https://arbiscan.io", gasFloor: 10_000_000_000_000, topupUSD: Decimal(string: "0.05")!)

    /// Robinhood Chain: its dollar is USDG (Relay delivers it; Stock Tokens trade against it on Uniswap).
    public static let robinhood = EVMNetwork(name: "Robinhood Chain", key: "robinhood", rpc: .robinhood, usdc: EVMRPC.robinhoodUSDG,
                                             relayChain: Relay.Chain.robinhood, explorer: "https://robinhoodchain.blockscout.com",
                                             gasFloor: 15_000_000_000_000, topupUSD: Decimal(string: "0.08")!)

    /// Polygon's gas is POL at ≈ $0.12: 0.3 POL covers several calls at today's 274 gwei, and a $0.06 top-up lands above it.
    public static let polygon = EVMNetwork(name: "Polygon", key: "polygon", rpc: .polygon, usdc: EVMRPC.polygonUSDC, relayChain: Relay.Chain.polygon,
                                           explorer: "https://polygonscan.com", gasFloor: 300_000_000_000_000_000,
                                           topupUSD: Decimal(string: "0.06")!, gasCoin: "POL")
    /// Ethereum: an Aave supply is ≈ 250k gas, $0.50 at 0.74 gwei (2026-09-29); the floor covers a supply and a
    /// withdraw at twice that, and a $2.50 top-up lands above it.
    // ponytail: fixed floor; derive it from eth_gasPrice if mainnet fees jump past ≈ 2 gwei.
    public static let ethereum = EVMNetwork(name: "Ethereum", key: "ethereum", rpc: .ethereum, usdc: EVMRPC.ethereumUSDC, relayChain: Relay.Chain.ethereum,
                                            explorer: "https://etherscan.io", gasFloor: 800_000_000_000_000, topupUSD: Decimal(string: "2.5")!)

    /// Tempo: fees in USDC itself (≈ $0.001 a call at 0.6 gwei-attodollars, 2026-09-30); a transfer is the only thing
    /// ihatecrypto does there besides the bridge — to a saved address, or to pay a service (MPP).
    public static let tempo = EVMNetwork(name: "Tempo", key: "tempo", rpc: .tempo, usdc: EVMRPC.tempoUSDC, relayChain: Relay.Chain.tempo,
                                         explorer: "https://explore.tempo.xyz", gasFloor: 0, topupUSD: 0, gasCoin: "USDC",
                                         feeToken: EVMRPC.tempoUSDC)

    /// HyperEVM: HYPE at ≈ $95 pays 0.1 gwei, ≈ $0.001 a call (2026-10-06); 0.0005 HYPE covers dozens, and Relay's $0.15
    /// top-up lands ≈ 0.0016 above it.
    public static let hyperevm = EVMNetwork(name: "HyperEVM", key: "hyperevm", rpc: .hyperevm, usdc: EVMRPC.hyperevmUSDC, relayChain: Relay.Chain.hyperevm,
                                            explorer: "https://hyperevmscan.io", gasFloor: 500_000_000_000_000, topupUSD: Decimal(string: "0.15")!,
                                            gasCoin: "HYPE")

    /// Optimism: an OP Stack network like Base, its calls cents of ETH; the same floor and top-up.
    public static let optimism = EVMNetwork(name: "Optimism", key: "optimism", rpc: .optimism, usdc: EVMRPC.optimismUSDC, relayChain: Relay.Chain.optimism,
                                            explorer: "https://optimistic.etherscan.io", gasFloor: 5_000_000_000_000, topupUSD: Decimal(string: "0.03")!)

    /// BNB Chain: gas in BNB at 0.05 gwei, a swap ≈ a cent (2026-10-08); 0.0001 BNB covers several calls, and Relay's
    /// $0.15 top-up lands ≈ 0.0002 above it. Its USDC is Binance-Peg, 18 decimals.
    public static let bnb = EVMNetwork(name: "BNB Chain", key: "bnb", rpc: .bnb, usdc: EVMRPC.bnbUSDC, relayChain: Relay.Chain.bnb,
                                       explorer: "https://bscscan.com", gasFloor: 100_000_000_000_000, topupUSD: Decimal(string: "0.15")!,
                                       gasCoin: "BNB", usdcDecimals: 18)

    public static let all = [base, arbitrum, robinhood, polygon, ethereum, tempo, hyperevm, optimism, bnb]

    /// Its USDC's own units for millionths of a dollar: the same number, but a trillion times it on BNB Chain.
    public func raw(_ micro: UInt64) -> Decimal { Decimal(micro) * pow(10, usdcDecimals - 6) }
    /// Millionths of a dollar for its USDC's own units, rounded down.
    public func micro(_ raw: Decimal) -> UInt64 { NSDecimalNumber(decimal: (raw / pow(10, usdcDecimals - 6)).rounded(0, .down)).uint64Value }
    /// Its USDC at `owner`, in millionths of a dollar (an 18-decimal balance doesn't fit the node's 64-bit read).
    public func usdcBalance(_ owner: String) async throws -> UInt64 {
        usdcDecimals == 6 ? try await rpc.tokenBalance(usdc, owner: owner)
            : micro(ABI.decimal(in: try await rpc.read(usdc, try ERC20.balanceOf(owner)), word: 0) ?? 0)
    }

    /// What to leave for fees when everything goes: a cent pays ten Tempo calls.
    // ponytail: fixed cent; read the base fee if Tempo's fees ever pass $0.001 a call.
    var feeReserve: UInt64 { feeToken == nil ? 0 : 10_000 }

    /// nil for a network ihatecrypto doesn't move money on: never guessed.
    public static func of(chainId: UInt64) -> EVMNetwork? { all.first { $0.rpc.chainId == chainId } }
    public static func named(_ key: String) -> EVMNetwork? { all.first { $0.key == key } }

    /// Where a trade's money comes from when Solana is short: the first network holding `send` and a dime over for Relay's
    /// fee, straight to Hyperliquid. Ethereum is asked last (its gas costs dollars), never left out. nil = none has it.
    public static func funding(_ send: Decimal, held: (String) -> Decimal?) -> EVMNetwork? {
        searched.first { (held($0.key) ?? 0) >= send + Decimal(string: "0.1")! }
    }

    /// Where the USDC a move needs on `target` comes from — the one rule the executor signs and the card shows: Solana
    /// pays when it holds what must arrive plus 10 cents; else the EVM network holding it (Ethereum last) sends it
    /// straight, a dollar over for Relay's fee. nil: Solana pays (or nobody holds it, and the move says so).
    public static func source(arrive: Decimal, onSolana: Decimal, target: String, held: (String) -> Decimal?) -> (net: EVMNetwork, send: Decimal)? {
        guard onSolana < arrive + Decimal(string: "0.1")! else { return nil }
        let send = (arrive + 1).rounded(2, .up)
        return funding(send, held: { $0 == target ? nil : held($0) }).map { ($0, send) }
    }

    /// Every network a search for money looks at: all but the ones switched off in Settings › Modules — the user's rule,
    /// never a network left out by us. Ethereum comes last, so a cheaper network pays first when both hold enough.
    public static var searched: [EVMNetwork] {
        all.filter { ModuleNetwork(place: $0.key).map(Modules.current.isOn) ?? true }.sorted { $0.key != "ethereum" && $1.key == "ethereum" }
    }
}

/// Aave v3 on any network ihatecrypto knows: USDC in and out. USDC missing there comes from Solana over Relay, with a little
/// of the network's gas coin; a withdrawal goes back to Solana.
extension Executor {
    static var baseGasFloorForPlan: UInt64 { EVMNetwork.base.gasFloor }

    public func aave(deposit: Bool, usdc requested: Decimal?, market m: AaveV3.Market = AaveV3.base) async throws -> String {
        guard let net = EVMNetwork.named(m.chain) else { throw ExecutionError("ihatecrypto doesn't move money on \(m.chainName). Nothing was signed.") }
        let rpc = m.rpc
        // Pool and aToken are re-read from the market's own network before anything is signed.
        let apy = try await AaveV3.verify(m)
        let usdc = m.usdc, pool = m.pool, me = wallet.evm
        func units(_ d: Decimal) -> UInt64 { NSDecimalNumber(decimal: (d * 1_000_000).rounded(0, .down)).uint64Value }
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }

        guard deposit else {
            let supplied = try await rpc.tokenBalance(m.aUSDC, owner: me)
            guard supplied > 0 else { throw ExecutionError("Nothing is supplied to Aave on \(m.chainName).") }
            // nil = everything, interest included.
            let amount = requested.map(units).flatMap { $0 < supplied ? $0 : nil }
            try await ensureEVM(net, usdc: 0)
            let before = try await rpc.tokenBalance(usdc, owner: me)
            _ = try await sendEVM(rpc, to: pool, data: try AaveV3.withdraw(asset: usdc, amount: amount, to: me),
                                  note: "Aave withdraw \(amount.map(show) ?? "all") USDC on \(m.chainName)")
            return try await bridgeBack(net, since: before, expected: amount ?? supplied)
        }

        guard let requested, requested > 0 else { throw ExecutionError("Say how much, e.g. “lend 2 USDC on Aave”.") }
        let want = units(requested)
        try await ensureEVM(net, usdc: want)
        let before = try await rpc.tokenBalance(m.aUSDC, owner: me)
        let hash = try await approveAndSend(rpc, token: usdc, spender: pool, units: Decimal(want), approveNote: "Approve exactly \(show(want)) USDC to Aave on \(m.chainName)",
                                            to: pool, data: try AaveV3.supply(asset: usdc, amount: want, onBehalfOf: me),
                                            note: "Aave supply \(show(want)) USDC on \(m.chainName)")
        // A node can answer from a block before the receipt's for a moment: give it up to ten seconds.
        var after = try await rpc.tokenBalance(m.aUSDC, owner: me)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await rpc.tokenBalance(m.aUSDC, owner: me)
        }
        guard after > before else { throw ExecutionError("The supply went through but aUSDC didn't grow. \(net.explorer)/tx/\(hash)") }
        return "Supplied \(show(want)) USDC to Aave on \(m.chainName) at ≈ \(Fmt.num(apy.rounded(2, .plain)))% a year: \(show(after)) USDC there now. \(net.explorer)/tx/\(hash)"
    }

    /// After a withdrawal on `net`: back where the money lives, Solana — only the USDC the withdrawal brought, never what
    /// already lay there (a card that says 10 bridged all 2,970). `expected`: what it was for, while a node is a block behind.
    func bridgeBack(_ net: EVMNetwork, since before: UInt64, expected: UInt64) async throws -> String {
        var there = try await net.usdcBalance(wallet.evm)
        for _ in 0..<10 where there <= before {
            try await Task.sleep(for: .seconds(1))
            there = try await net.usdcBalance(wallet.evm)
        }
        let got = there > before ? there - before : expected
        guard got >= 500_000 else {
            return "\(Fmt.num(Decimal(got) / 1_000_000, maxFraction: 6)) USDC stays on \(net.name): under the half-dollar a bridge to Solana is worth."
        }
        return try await evmToSolana(net, usdc: Decimal(got) / 1_000_000)
    }

    func ensureBase(usdc want: UInt64) async throws { try await ensureEVM(.base, usdc: want) }

    /// At least `want` USDC and some ETH for gas on the network: what is missing comes from Solana over Relay.
    /// `gas`: a floor and a top-up for calls heavier than a supply (a Morpho vault deposit is ≈ 450k gas);
    /// the top-up must land above the floor, or the wait below runs out.
    func ensureEVM(_ net: EVMNetwork, usdc want: UInt64, gas heavier: (floor: UInt64, topupUSD: Decimal)? = nil) async throws {
        let me = wallet.evm
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        let floor = max(net.gasFloor, heavier?.floor ?? 0), topupUSD = max(net.topupUSD, heavier?.topupUSD ?? 0)
        // Where fees come out of the stablecoin, the fees ride on the USDC wanted and there's no coin to read.
        let want = want + net.feeReserve
        func gasThere() async throws -> UInt64 { net.feeToken == nil ? try await net.rpc.balance(me) : floor }
        var there = try await net.usdcBalance(me)
        var gas = try await gasThere()
        guard there < want || gas < floor else { return }
        let short = Decimal(want > there ? want - there : 0) / 1_000_000
        let needGas = gas < floor
        // Relay has minimums: a gas top-up rides on at least half a dollar of USDC.
        let arrive = max(short, needGas ? Decimal(string: "0.5")! : 0)
        // Solana pays when it holds enough. Else the EVM network that holds the USDC sends it straight (Ethereum asked
        // last); its bridge brings gas along and waits until the money has landed. Before, an empty Solana failed the move
        // while thousands sat on Ethereum.
        let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        if onSolana < arrive + Decimal(string: "0.1")! {
            let idle = Dictionary(await idleUSDC().map { ($0.net.key, $0.usdc) }, uniquingKeysWith: +)
            if let (from, send) = EVMNetwork.source(arrive: arrive, onSolana: onSolana, target: net.key, held: { idle[$0] }) {
                report("\(net.name) needs \(Fmt.num(arrive)) USDC and Solana has \(Fmt.num(onSolana)): bringing \(Fmt.num(send)) straight from \(from.name).")
                _ = try await bridge(from, to: net, usdc: send)
                return
            }
        }
        let (step, changes) = try await prepareBridge(.arrive(arrive), to: net.relayChain, currency: net.usdc, decimals: net.usdcDecimals,
                                                      topupGasUSD: needGas ? topupUSD : nil)
        let stable = net.key == "robinhood" ? "USDG" : "USDC"
        report("Bridge to \(net.name): \(Fmt.num(step.spend)) USDC from Solana → \(Fmt.num(step.arriving)) \(stable)\(needGas ? " plus \(net.gasCoin) for gas" : ""). "
               + changes.joined(separator: " "))
        try await sendBridge(step)
        let started = Date()
        while there < want || gas < floor {
            guard Date().timeIntervalSince(started) < 240 else {
                throw ExecutionError("The USDC didn't reach \(net.name) within 4 minutes. Relay order \(step.requestId) is in the journal; nothing else was signed.")
            }
            try await Task.sleep(for: .seconds(2))
            there = try await net.usdcBalance(me)
            gas = try await gasThere()
        }
        report("On \(net.name) in \(Int(Date().timeIntervalSince(started))) s: \(show(there)) \(stable) and gas.")
    }

    /// USDC on Base → our Solana address over Relay. Relay's two transactions (an exact approve and the deposit into its
    /// depository) are checked field by field before signing: the depository is the one Relay publishes, the depositor
    /// is us, the token is USDC, the amount is the quote's, the recipient our Solana address. nil = everything on Base.
    public func baseToSolana(usdc requested: Decimal?) async throws -> String { try await evmToSolana(.base, usdc: requested) }

    /// USDC from Solana to our own address on Base, Arbitrum or Robinhood Chain (USDG there) over Relay, with a little
    /// ETH for gas when there is none. `usdc` is what arrives.
    public func solanaToEVM(_ net: EVMNetwork, usdc: Decimal) async throws -> String {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units >= 500_000 else { throw ExecutionError("At least 0.5 USDC: less isn't worth the bridge.") }
        let stable = net.key == "robinhood" ? "USDG" : "USDC"
        let before = try await net.usdcBalance(wallet.evm)
        // With gas already there, NEAR Intents' 1Click may land more than Relay: both are quoted, the better one goes.
        if OneClick.Asset.usdc(onEVM: net.key) != nil, try await net.rpc.balance(wallet.evm) >= net.gasFloor,
           let relay = try? await Relay.quote(user: wallet.solana, recipient: wallet.evm, originChain: Relay.Chain.solana, destinationChain: net.relayChain,
                                              originCurrency: Relay.Currency.solanaUSDC, destinationCurrency: net.usdc, amount: units),
           let relayOut = Decimal(string: relay.amountOutFormatted),
           let better = await oneClickBeats(from: nil, to: net.key, units: units, relayOut: relayOut) {
            _ = try await oneClickMove(from: nil, to: net.key, units: units, compared: Decimal(string: better.amountOutFormatted) ?? relayOut,
                                       label: "\(Fmt.num(usdc)) USDC")
        } else {
            try await ensureEVM(net, usdc: before + units)
        }
        let after = try await net.usdcBalance(wallet.evm)
        return "Moved \(Fmt.num(Decimal(after > before ? after - before : 0) / 1_000_000)) \(stable) to \(net.name): \(Fmt.num(Decimal(after) / 1_000_000)) \(stable) there now. "
            + "Say “move \(stable) from \(net.name) to Solana” to bring it back."
    }

    /// The same from any EVM network ihatecrypto knows (Base, Arbitrum).
    public func evmToSolana(_ net: EVMNetwork, usdc requested: Decimal?) async throws -> String { try await bridge(net, to: nil, usdc: requested) }

    /// USDC from one EVM network straight to another at our own address (Base → Robinhood Chain arrives as USDG there),
    /// to our Hyperliquid trading balance (`hyperliquid`), or to Solana when `dest` is nil — over Relay, the same two
    /// checked transactions each way. A destination with a gas coin and none of it gets a little along. nil `usdc` =
    /// everything on `net`.
    public func bridge(_ net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool = false, usdc requested: Decimal?) async throws -> String {
        let base = net.rpc, usdc = net.usdc, me = wallet.evm, chain = net.relayChain
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        guard dest?.key != net.key else { throw ExecutionError("The money is already on \(net.name).") }
        let toSolana = dest == nil && !hyperliquid
        let there = hyperliquid ? "Hyperliquid" : dest?.name ?? "Solana", stable = dest?.key == "robinhood" ? "USDG" : "USDC"
        let have = try await net.usdcBalance(me)
        // On Tempo the fees come out of this USDC: a cent of it stays.
        let spendable = have > net.feeReserve ? have - net.feeReserve : 0
        let amount = requested.map { min(NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).uint64Value, spendable) } ?? spendable
        guard amount >= 500_000 else { throw ExecutionError("\(show(have)) USDC on \(net.name) — below the half-dollar a bridge is worth.") }
        // Gas for what comes next there (a buy on Robinhood Chain), when the destination has a gas coin and too little of it.
        var topup: Decimal?
        if let dest, dest.feeToken == nil, try await dest.rpc.balance(me) < dest.gasFloor { topup = dest.topupUSD }
        // No gas here for an approve and a deposit: where the dollar pays by signature, Relay takes it so and its solver pays
        // the gas; on BNB Chain a plain transfer to Relay's deposit address goes with BNB Chain paying. CCTP and 1Click both
        // start with a transaction here, so they're not asked.
        if net.feeToken == nil, Self.leavesWithoutGas(net), try await base.balance(me) < net.gasFloor {
            return try await bridgeWithoutGas(net, to: dest, hyperliquid: hyperliquid, amount: amount, topup: topup)
        }

        let recipient = toSolana ? wallet.solana : me
        let quote = try await Relay.quote(user: me, recipient: recipient, originChain: chain,
                                          destinationChain: hyperliquid ? Relay.Chain.hyperliquid : dest?.relayChain ?? Relay.Chain.solana, originCurrency: usdc,
                                          destinationCurrency: hyperliquid ? Relay.Currency.hyperliquidUSDC : dest?.usdc ?? Relay.Currency.solanaUSDC,
                                          amount: amount, topupGasUSD: topup, units: "\(net.raw(amount))")
        let binding = BindingGuard.check(["sender": quote.sender.lowercased(), "recipient": toSolana ? quote.recipient : quote.recipient.lowercased()],
                                         expected: ["sender": me.lowercased(), "recipient": toSolana ? recipient : recipient.lowercased()], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        guard let depository = try await Relay.depository(chainId: chain)?.lowercased(),
              let deposit = quote.evm.first(where: { $0.id == "deposit" }), deposit.to.lowercased() == depository, deposit.chainId == chain,
              deposit.value == "0" || deposit.value == "<null>" else {
            throw GuardStop("Relay's deposit isn't to its published depository on \(net.name). Nothing was signed.")
        }
        // depositErc20(address depositor, address token, uint256 amount, bytes32 id)
        let args = Data(deposit.data.dropFirst(4))
        guard deposit.data.prefix(4).hex == "e8017952", args.count == 4 * 32,
              ABI.address(in: args, word: 0)?.lowercased() == me.lowercased(),
              ABI.address(in: args, word: 1)?.lowercased() == usdc,
              ABI.decimal(in: args, word: 2) == net.raw(amount) else {
            throw GuardStop("Relay's deposit asks for something else than \(show(amount)) USDC from this wallet. Nothing was signed.")
        }
        let arriving = Decimal(string: quote.amountOutFormatted) ?? 0
        let spend = Decimal(amount) / 1_000_000
        // Circle's CCTP between EVM networks, when no gas has to come along and it lands more than Relay by the margin
        // 1Click must beat too: one signature, Circle mints on the other side itself.
        if !hyperliquid, topup == nil, let dest, let cctp = await cctpQuote(from: net, to: dest, units: amount) {
            let better = cctp.arrives - arriving >= Self.oneClickEdge(spend)
            report("Relay lands ≈ \(Fmt.num(arriving, maxFraction: 4)) USDC, Circle's CCTP ≈ \(Fmt.num(cctp.arrives, maxFraction: 4)): "
                   + (better ? "taking CCTP." : "Relay\(cctp.arrives > arriving ? " (CCTP's \(Fmt.usd(cctp.arrives - arriving)) more isn't worth the wait)" : "")."))
            if better {
                let started = Date()
                let got = try await cctpMove(from: net, to: dest, units: amount, maxFee: cctp.maxFee)
                return "Moved \(show(amount)) USDC from \(net.name) to \(there) through Circle's CCTP in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(got)) USDC arrived."
            }
        }
        // NEAR Intents' 1Click for the same USDC, when it serves both ends and no gas has to come along: the better lands.
        if !hyperliquid, topup == nil, OneClick.Asset.usdc(onEVM: net.key) != nil, dest == nil || OneClick.Asset.usdc(onEVM: dest!.key) != nil,
           let better = await oneClickBeats(from: net.key, to: dest?.key, units: amount, relayOut: arriving) {
            let started = Date()
            let got = try await oneClickMove(from: net.key, to: dest?.key, units: amount, compared: Decimal(string: better.amountOutFormatted) ?? arriving,
                                             label: "\(show(amount)) USDC")
            return "Moved \(show(amount)) USDC from \(net.name) to \(there) through NEAR Intents' 1Click in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(got)) USDC arrived."
        }
        try Self.checkLoss(spend - arriving - (topup ?? 0), paid: spend, cap: min(max(arriving * Decimal(string: "0.03")!, Decimal(string: "0.15")!), 1),
                           what: "Relay")
        let before = try await relayLanded(dest, hyperliquid: hyperliquid)
        let hash = try await approveAndSend(base, token: usdc, spender: depository, units: net.raw(amount), approveNote: "Approve exactly \(show(amount)) USDC to Relay",
                                            to: deposit.to, data: deposit.data, note: "Relay deposit \(show(amount)) USDC on \(net.name)")
        try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(net.name) USDC→\(there) \(show(amount)), tx \(hash)"))
        let requestId = quote.requestId, gas = topup != nil, sent = show(amount)
        let landing: @Sendable () async throws -> String = { [self] in
            try await relayLanding(requestId, link: "\(net.explorer)/tx/\(hash)", from: net, to: dest, hyperliquid: hyperliquid, before: before, arriving: arriving,
                                   sent: sent, stable: stable, there: there, gas: gas)
        }
        // Inside a chain the next step is signed while this lands; a step spending where it lands waits for it (Landings).
        if let landings = Self.landings {
            landings.add(hyperliquid ? "hyperliquid" : dest?.key ?? "solana") { try await landing() }
            return "Sent \(sent) USDC from \(net.name) to \(there): ≈ \(Fmt.num(arriving)) \(stable) lands there in seconds, and what needs it waits for it."
        }
        return try await landing()
    }

    /// The USDC at our address where a Relay order from an EVM network lands: Hyperliquid's withdrawable, an EVM
    /// network's USDC, or Solana's.
    private func relayLanded(_ dest: EVMNetwork?, hyperliquid: Bool) async throws -> UInt64 {
        if hyperliquid { return NSDecimalNumber(decimal: try await Hyperliquid.balance(user: wallet.evm).withdrawable * 1_000_000).uint64Value }
        if let dest { return try await dest.usdcBalance(wallet.evm) }
        return try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)
    }

    /// A Relay order from an EVM network, signed: Relay's word until it settles, then the USDC on the other side.
    private func relayLanding(_ requestId: String, link: String, from net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool, before: UInt64,
                              arriving: Decimal, sent: String, stable: String, there: String, gas: Bool) async throws -> String {
        let started = Date()
        while Date().timeIntervalSince(started) < 180 {
            let status = try await Relay.status(requestId: requestId).status
            if status == "success" { try journal.append(.init(chain: "relay", id: requestId, kind: .settled, note: status)); break }
            if ["refund", "failure"].contains(status) {
                try journal.append(.init(chain: "relay", id: requestId, kind: .failed, note: status))
                throw ExecutionError("Relay returned \(status). Check USDC on \(net.name). \(link)")
            }
            try await Task.sleep(for: .seconds(2))
        }
        let target = before + NSDecimalNumber(decimal: arriving * 990_000).uint64Value
        while Date().timeIntervalSince(started) < 240 {
            let now = try await relayLanded(dest, hyperliquid: hyperliquid)
            if now >= target {
                return "Moved \(sent) USDC from \(net.name) to \(there) in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(arriving)) \(stable) arrived, "
                    + "\(Fmt.num(Decimal(now) / 1_000_000, maxFraction: 6)) \(stable) on \(there) now." + (gas ? " A little gas came along." : "")
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw ExecutionError("Relay didn't credit \(there) within 4 minutes. Order \(requestId) is in the journal. \(link)")
    }

    /// `amount` of `net`'s USDC to Solana (as SOL when `sol`), to Hyperliquid or to another EVM network, paid by signature:
    /// Relay's approval proxy takes it (EIP-3009, Permit3009) and its solver pays the gas, so a wallet with no gas coin here
    /// still moves it. The quote is checked like a deposit: from and to this wallet, our token and amount, Relay's proxy.
    func bridgeByPermit(_ net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool = false, amount: UInt64, topup: Decimal? = nil,
                        sol: Bool = false) async throws -> String {
        let me = wallet.evm, chain = net.relayChain, toSolana = dest == nil && !hyperliquid
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        let there = hyperliquid ? "Hyperliquid" : dest?.name ?? "Solana", stable = sol ? "SOL" : dest?.key == "robinhood" ? "USDG" : "USDC"
        let recipient = toSolana ? wallet.solana : me
        let quote = try await Relay.quote(user: me, recipient: recipient, originChain: chain,
                                          destinationChain: hyperliquid ? Relay.Chain.hyperliquid : dest?.relayChain ?? Relay.Chain.solana, originCurrency: net.usdc,
                                          destinationCurrency: sol ? Relay.Currency.solanaSOL : hyperliquid ? Relay.Currency.hyperliquidUSDC : dest?.usdc ?? Relay.Currency.solanaUSDC,
                                          amount: amount, topupGasUSD: topup, units: "\(net.raw(amount))", usePermit: true)
        let binding = BindingGuard.check(["sender": quote.sender.lowercased(), "recipient": toSolana ? quote.recipient : quote.recipient.lowercased()],
                                         expected: ["sender": me.lowercased(), "recipient": toSolana ? recipient : recipient.lowercased()], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        let now = UInt64(Date().timeIntervalSince1970), units = "\(net.raw(amount))"
        func hex(_ s: EVMSignature) -> String { "0x" + s.r.hex + s.s.hex + String(format: "%02x", s.v) }
        let known = Permit3009.tokens["\(net.rpc.chainId):\(net.usdc)"]
        // Relay's v3.2 proxy: the MulticallAuthorization the signer checks and signs with the EIP-3009 item it binds; else v3's
        // single EIP-3009 item. Each is held to what this wallet asked for before anything is signed.
        let v32: RelayMulticallPermit? = quote.multicall.flatMap { m in
            let f = m.funding.first, calls = m.calls.compactMap { c in UInt64(c.value).map {
                RelayMulticallPermit.Call(target: c.target, allowFailure: c.allowFailure, value: $0, callData: c.callData) } }
            guard m.chainId == net.rpc.chainId, m.domainName == "RelayApprovalProxy", m.domainVersion == "3.1",
                  m.verifyingContract.lowercased() == RelayMulticallPermit.proxy, m.from.lowercased() == me.lowercased(), m.msgValue == "0",
                  m.fundingIndex == 0, m.funding.count == 1, let f, f.token.lowercased() == net.usdc, f.from.lowercased() == me.lowercased(),
                  f.value == units, f.validAfter == 0, calls.count == m.calls.count,
                  m.permit.to.lowercased() == RelayMulticallPermit.proxy, m.permit.value == units, m.permit.validBefore == f.validBefore,
                  m.permit.domainName == known?.name, m.permit.domainVersion == known?.version else { return nil }
            let mine = RelayMulticallPermit(chainId: m.chainId, token: net.usdc, from: me, value: amount, validBefore: f.validBefore, relayer: m.relayer,
                                            refundTo: m.refundTo, nftRecipient: m.nftRecipient, metadata: m.metadata, calls: calls)
            // Relay's nonce must be the digest computed here: else the two signatures wouldn't pair.
            guard mine.refusal(owner: me, now: now) == nil, (try? mine.digest()) == m.permit.nonce else { return nil }
            return mine
        }
        let single = v32 != nil ? nil : quote.permit.flatMap { p -> (Relay.Permit, Permit3009)? in
            let permit = Permit3009(chainId: p.chainId, token: p.token.lowercased(), from: p.from, to: p.to, value: amount, validBefore: p.validBefore, nonce: p.nonce)
            guard p.chainId == net.rpc.chainId, permit.token == net.usdc, p.value == units, p.validAfter == 0, permit.refusal(owner: me, now: now) == nil,
                  p.domainName == known?.name, p.domainVersion == known?.version else { return nil }
            return (p, permit)
        }
        guard v32 != nil || single != nil else {
            throw ExecutionError("There's no \(net.gasCoin) on \(net.name) for gas, and Relay won't take its USDC by signature right now (it asks for a transaction). "
                                 + "Add a little \(net.gasCoin) to \(net.name); nothing was signed.")
        }
        let arriving = Decimal(string: quote.amountOutFormatted) ?? 0, spend = Decimal(amount) / 1_000_000
        let worth = sol ? Decimal(string: quote.amountOutUSD) ?? 0 : arriving
        try Self.checkLoss(spend - worth - (topup ?? 0), paid: spend, cap: min(max(worth * Decimal(string: "0.03")!, Decimal(string: "0.15")!), 1), what: "Relay")
        let before = sol ? try await rpc.balance(wallet.solana) : try await relayLanded(dest, hyperliquid: hyperliquid)
        if let v32, let m = quote.multicall {
            let signatures = try await signer.signRelayMulticallPermit(v32)
            try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(net.name) USDC→\(there) \(show(amount)) by signature (v3.2)"))
            // Relay takes the MulticallAuthorization first; the EIP-3009 one before it is refused.
            try await Relay.postSignature(endpoint: m.endpoint, body: m.body, signature: hex(signatures[0]))
            try await Relay.postPermit(m.permit, signature: hex(signatures[1]))
        } else if let (p, permit) = single {
            let signature = try await signer.signReceiveAuthorization(permit)
            try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(net.name) USDC→\(there) \(show(amount)) by signature"))
            try await Relay.postPermit(p, signature: hex(signature))
        }
        report("Signed: Relay takes \(show(amount)) USDC on \(net.name) by signature and pays the gas there; ≈ \(Fmt.num(arriving, maxFraction: 6)) \(stable) to \(there).")
        return try await relayArrival(quote.requestId, link: "https://relay.link/transaction/\(quote.requestId)", from: net, to: dest, hyperliquid: hyperliquid,
                                      before: before, arriving: arriving, sent: show(amount), stable: stable, there: there, gas: topup != nil, sol: sol)
    }

    /// Where a network's dollars can leave with no gas coin there: by signature (Relay, Permit3009) or as a transfer BNB
    /// Chain pays the gas for (MegaFuel, SponsoredTransfer).
    static func leavesWithoutGas(_ net: EVMNetwork) -> Bool {
        Permit3009.supports(chainId: net.rpc.chainId, token: net.usdc) || SponsoredTransfer.supports(chainId: net.rpc.chainId, token: net.usdc)
    }

    /// `amount` of `net`'s USDC out with no gas coin there, whichever way the network has.
    func bridgeWithoutGas(_ net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool = false, amount: UInt64, topup: Decimal? = nil,
                          sol: Bool = false) async throws -> String {
        if Permit3009.supports(chainId: net.rpc.chainId, token: net.usdc) {
            return try await bridgeByPermit(net, to: dest, hyperliquid: hyperliquid, amount: amount, topup: topup, sol: sol)
        }
        return try await bridgeBySponsor(net, to: dest, hyperliquid: hyperliquid, amount: amount, topup: topup, sol: sol)
    }

    /// `amount` of BNB Chain's USDC to Relay's deposit address for the order, as a zero-fee transfer BNB Chain's paymaster
    /// pays the gas for (MegaFuel). The quote is checked like a deposit: from and to this wallet, the transfer exactly this
    /// amount of our token to the deposit address the quote names; the signer then holds the address to the new-recipient
    /// rule.
    func bridgeBySponsor(_ net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool = false, amount: UInt64, topup: Decimal? = nil,
                         sol: Bool = false) async throws -> String {
        let me = wallet.evm, chain = net.relayChain, toSolana = dest == nil && !hyperliquid
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        let there = hyperliquid ? "Hyperliquid" : dest?.name ?? "Solana", stable = sol ? "SOL" : dest?.key == "robinhood" ? "USDG" : "USDC"
        let recipient = toSolana ? wallet.solana : me
        let quote = try await Relay.quote(user: me, recipient: recipient, originChain: chain,
                                          destinationChain: hyperliquid ? Relay.Chain.hyperliquid : dest?.relayChain ?? Relay.Chain.solana, originCurrency: net.usdc,
                                          destinationCurrency: sol ? Relay.Currency.solanaSOL : hyperliquid ? Relay.Currency.hyperliquidUSDC : dest?.usdc ?? Relay.Currency.solanaUSDC,
                                          amount: amount, topupGasUSD: topup, units: "\(net.raw(amount))", useDepositAddress: true, refundTo: me)
        let binding = BindingGuard.check(["recipient": toSolana ? quote.recipient : quote.recipient.lowercased()],
                                         expected: ["recipient": toSolana ? recipient : recipient.lowercased()], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        guard let deposit = quote.depositAddress, (try? ABI.address(deposit)) != nil, let step = quote.evm.first, quote.evm.count == 1,
              step.to.lowercased() == net.usdc, step.chainId == chain, step.value == "0" || step.value == "<null>",
              step.data == (try ERC20.transfer(to: deposit, units: net.raw(amount))) else {
            throw GuardStop("Relay's deposit on \(net.name) isn't a plain transfer of \(show(amount)) USDC to the deposit address it names. Nothing was signed.")
        }
        let arriving = Decimal(string: quote.amountOutFormatted) ?? 0, spend = Decimal(amount) / 1_000_000
        let worth = sol ? Decimal(string: quote.amountOutUSD) ?? 0 : arriving
        try Self.checkLoss(spend - worth - (topup ?? 0), paid: spend, cap: min(max(worth * Decimal(string: "0.03")!, Decimal(string: "0.15")!), 1), what: "Relay")
        let transfer = SponsoredTransfer(chainId: net.rpc.chainId, nonce: try await MegaFuel.nonce(me), gasLimit: 80_000, token: net.usdc, to: deposit,
                                         amount: "\(net.raw(amount))")
        guard try await MegaFuel.sponsors(transfer, from: me) else {
            throw ExecutionError("There's no \(net.gasCoin) on \(net.name) for gas, and BNB Chain won't pay the gas for this transfer now. Add a little \(net.gasCoin); nothing was signed.")
        }
        let before = sol ? try await rpc.balance(wallet.solana) : try await relayLanded(dest, hyperliquid: hyperliquid)
        let raw = try await signer.signSponsoredTransfer(transfer, recipients: [deposit])
        let hash = "0x" + Keccak.hash256(raw).hex, note = "\(net.name) USDC→\(there) \(show(amount)), BNB Chain paying the gas"
        try journal.append(.init(chain: net.key, id: hash, kind: .signed, note: note))
        _ = try await MegaFuel.send(raw)
        guard try await net.rpc.waitForReceipt(hash) else {
            try journal.append(.init(chain: net.key, id: hash, kind: .failed, note: note))
            throw ExecutionError("The transfer to Relay failed on \(net.name): \(net.explorer)/tx/\(hash)")
        }
        try journal.append(.init(chain: net.key, id: hash, kind: .confirmed))
        try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(note), tx \(hash)"))
        report("Sent \(show(amount)) USDC on \(net.name) to Relay's deposit address, BNB Chain paying the gas; ≈ \(Fmt.num(arriving, maxFraction: 6)) \(stable) to \(there).")
        return try await relayArrival(quote.requestId, link: "\(net.explorer)/tx/\(hash)", from: net, to: dest, hyperliquid: hyperliquid, before: before,
                                      arriving: arriving, sent: show(amount), stable: stable, there: there, gas: topup != nil, sol: sol)
    }

    /// A Relay order sent with no gas of ours: its landing (SOL on Solana, or the dollar where it goes), waited for here or,
    /// inside a chain of moves, by the step that needs it.
    private func relayArrival(_ requestId: String, link: String, from net: EVMNetwork, to dest: EVMNetwork?, hyperliquid: Bool, before: UInt64,
                              arriving: Decimal, sent: String, stable: String, there: String, gas: Bool, sol: Bool) async throws -> String {
        let landing: @Sendable () async throws -> String = { [self] in
            guard sol else {
                return try await relayLanding(requestId, link: link, from: net, to: dest, hyperliquid: hyperliquid, before: before, arriving: arriving,
                                              sent: sent, stable: stable, there: there, gas: gas)
            }
            let started = Date()
            while Date().timeIntervalSince(started) < 180 {
                let status = try await Relay.status(requestId: requestId).status
                if ["refund", "failure"].contains(status) {
                    try journal.append(.init(chain: "relay", id: requestId, kind: .failed, note: status))
                    throw ExecutionError("Relay returned \(status). Check USDC on \(net.name). \(link)")
                }
                let now = try await rpc.balance(wallet.solana)
                if status == "success", now > before {
                    try journal.append(.init(chain: "relay", id: requestId, kind: .settled, note: status))
                    return "Moved \(sent) USDC from \(net.name) to Solana as SOL in \(Int(Date().timeIntervalSince(started))) s: +\(Fmt.num(Decimal(now - before) / 1_000_000_000, maxFraction: 6)) SOL."
                }
                try await Task.sleep(for: .seconds(2))
            }
            throw ExecutionError("Relay didn't credit Solana within 3 minutes. Order \(requestId) is in the journal. \(link)")
        }
        if let landings = Self.landings {
            landings.add(hyperliquid ? "hyperliquid" : dest?.key ?? "solana") { try await landing() }
            return "Sent \(sent) USDC from \(net.name) to \(there) with no gas of yours: ≈ \(Fmt.num(arriving)) \(stable) lands there in seconds, and what needs it waits for it."
        }
        return try await landing()
    }

    /// An exact approve when the allowance is short, then a wait until the chain shows it: the next call's dry run
    /// on a node that hasn't seen the approve yet would revert.
    func approve(_ rpc: EVMRPC, token: String, spender: String, amount: UInt64, note: String) async throws {
        try await approve(rpc, token: token, spender: spender, units: Decimal(amount), note: note)
    }

    /// The same for any amount (256-bit words): a thousand tokens of 18 decimals don't fit 64 bits.
    func approve(_ rpc: EVMRPC, token: String, spender: String, units amount: Decimal, note: String, recipients: [String] = []) async throws {
        func allowance() async throws -> Decimal {
            ABI.decimal(in: try await rpc.read(token, try ERC20.allowance(owner: wallet.evm, spender: spender)), word: 0) ?? 0
        }
        guard try await allowance() < amount else { return }
        _ = try await sendEVM(rpc, to: token, data: try ERC20.approve(spender: spender, units: amount), note: note, recipients: recipients)
        for _ in 0..<15 {
            if try await allowance() >= amount { return }
            try await Task.sleep(for: .seconds(1))
        }
        throw ExecutionError("The approve went through but the network doesn't show it yet. Try again in a minute; nothing else was signed.")
    }

    /// Settings › Wallet › «One transaction for approve and swap». Off unless the user turns it on: the first batched move
    /// on a network leaves the wallet there running the audited batch account's code (EIP-7702), a change they choose.
    public static let batchKey = "batch7702"
    public static var batchesOn: Bool { UserDefaults.standard.bool(forKey: batchKey) }

    /// Whether calls on this network can go as one EIP-7702 transaction: the user's switch on, a network in
    /// Batch7702.chains that takes fees in its own coin, and a signer that signs batches (not a Ledger).
    func canBatch(_ rpc: EVMRPC) -> Bool {
        Self.batchesOn && signer.signsBatches && Batch7702.chains.contains(rpc.chainId) && EVMNetwork.of(chainId: rpc.chainId)?.feeToken == nil
    }

    /// An exact approve and the call that spends it. Where they can batch (`canBatch`) they go as one transaction under one
    /// Touch ID, dry-run together first, so a call that would fail stops before anything is signed; elsewhere the approve,
    /// its wait, then the call, as before.
    func approveAndSend(_ rpc: EVMRPC, token: String, spender: String, units: Decimal, approveNote: String,
                        to: String, data: Data, note: String, recipients: [String] = []) async throws -> String {
        if canBatch(rpc) {
            let allowance = ABI.decimal(in: try await rpc.read(token, try ERC20.allowance(owner: wallet.evm, spender: spender)), word: 0) ?? 0
            if allowance < units {
                let calls = [Batch7702.Call(to: token, data: try ERC20.approve(spender: spender, units: units)), Batch7702.Call(to: to, data: data)]
                return try await sendEVM(rpc, to: wallet.evm, data: Data(), note: "\(approveNote) + \(note)", recipients: recipients, batch: calls)
            }
        } else {
            try await approve(rpc, token: token, spender: spender, units: units, note: approveNote, recipients: recipients)
        }
        return try await sendEVM(rpc, to: to, data: data, note: note, recipients: recipients)
    }

    /// One EVM transaction: dry run by eth_estimateGas (a call that would revert stops here), the fee within the ETH
    /// there, the journal written before sending.
    /// `batch`: these calls instead, as one EIP-7702 transaction from and to the wallet (`to` and `data` unused); the dry
    /// run sees the wallet already running Batch7702.delegate, and the gas limit adds the authorization's own.
    /// `value`: wei of the network's coin sent with the call (a wrap, a native v4 swap); never with `batch`.
    func sendEVM(_ rpc: EVMRPC, to: String, data: Data, note: String, recipients: [String] = [], batch: [Batch7702.Call]? = nil,
                 value: UInt64 = 0) async throws -> String {
        guard let net = EVMNetwork.of(chainId: rpc.chainId) else { throw ExecutionError("ihatecrypto doesn't move money on network \(rpc.chainId). Nothing was signed.") }
        guard value == 0 || (batch == nil && net.feeToken == nil) else { throw ExecutionError("Coins go only with a single call on a network that pays its fees in them. Nothing was signed.") }
        if let token = net.feeToken, to.lowercased() != Tempo.feeManager {
            // Only a plain transfer of the fee coin pays its fee in that coin by itself.
            let paysInToken = if case .transfer = EVMCall.decode(data) { to.lowercased() == token } else { false }
            if !paysInToken { try await chooseFeeToken(net, token) }
        }
        let estimate = if let batch {
            try await rpc.estimateGas(from: wallet.evm, to: wallet.evm, value: 0, data: try Batch7702.executeBatch(batch), runningCodeOf: Batch7702.delegate)
        } else {
            try await rpc.estimateGas(from: wallet.evm, to: to, value: value, data: data)
        }
        let (tip, maxFee) = try await rpc.fees()
        let gasLimit = estimate + estimate / 4 + (batch == nil ? 0 : Batch7702.authorizationGas)
        if let token = net.feeToken {
            guard Tempo.feeUnits(gasLimit * maxFee) <= (try await rpc.tokenBalance(token, owner: wallet.evm)) else {
                throw ExecutionError("Not enough \(net.gasCoin) on \(net.name) for the fee: \(note).")
            }
        } else {
            // Short of gas is never a dead end: a top-up of three times what this call may cost comes over first. Coins sent
            // with the call are the user's amount: when they and the fee don't fit, it's said, not topped up.
            let fee = gasLimit * maxFee
            if value > 0, fee + value > (try await rpc.balance(wallet.evm)) {
                throw ExecutionError("\(Fmt.num(Decimal(value) / 1e18, maxFraction: 8)) \(net.gasCoin) and the fee don't both fit in the \(net.gasCoin) on \(net.name): \(note). Nothing was signed.")
            }
            if fee > (try await rpc.balance(wallet.evm)) {
                // A top-up that itself runs short (Relay's swap for gas with none there) stops instead of looping.
                guard !Self.toppingUp else { throw ExecutionError("Not enough \(net.gasCoin) on \(net.name) even to buy gas there: \(note). Top up Solana's USDC and try again.") }
                let price = try await Relay.nativePrice(chainId: net.rpc.chainId) ?? 0
                let usd = price > 0 ? min(max(Decimal(fee) / 1e18 * price * 3, net.topupUSD, Decimal(string: "0.5")!), 10) : max(net.topupUSD, 1)
                report("Not enough \(net.gasCoin) on \(net.name) for «\(note)»: bringing \(Fmt.usd(usd)) of it first.")
                _ = try await Self.$toppingUp.withValue(true) { try await topUpGas(net, usd: usd) }
                guard fee <= (try await rpc.balance(wallet.evm)) else { throw ExecutionError("Not enough \(net.gasCoin) on \(net.name) for gas even after a top-up: \(note).") }
            }
        }
        let nonce = try await nextNonce(rpc)
        let raw = if let batch {
            try await signer.signEVMBatch(chainId: rpc.chainId, nonce: nonce, maxPriorityFee: tip, maxFee: maxFee, gasLimit: gasLimit, calls: batch,
                                          recipients: recipients)
        } else if value > 0 {
            try await signer.signEVMPayable(chainId: rpc.chainId, nonce: nonce, maxPriorityFee: tip, maxFee: maxFee, gasLimit: gasLimit, to: to,
                                            value: value, data: data, recipients: recipients)
        } else {
            try await signer.signEVM(chainId: rpc.chainId, nonce: nonce, maxPriorityFee: tip, maxFee: maxFee, gasLimit: gasLimit, to: to, data: data,
                                     recipients: recipients)
        }
        let hash = "0x" + Keccak.hash256(raw).hex
        try journal.append(.init(chain: net.key, id: hash, kind: .signed, note: note))
        _ = try await rpc.send(raw)
        guard try await rpc.waitForReceipt(hash) else {
            try journal.append(.init(chain: net.key, id: hash, kind: .failed, note: note))
            throw ExecutionError("\(note) failed on \(net.name): \(net.explorer)/tx/\(hash)")
        }
        try journal.append(.init(chain: net.key, id: hash, kind: .confirmed))
        // A batch's own authorization takes the next nonce too (Batch7702.sign).
        confirmed(nonce + (batch == nil ? 1 : 2), rpc)
        report("\(note): done. \(net.explorer)/tx/\(hash)")
        return hash
    }

    /// The nonce for this wallet's next transaction. A node can answer behind right after our own: after an EIP-7702 batch
    /// at nonce 0 one said 1 (the batch's authorization had taken it), and the unwrap that came next was refused as «nonce
    /// too low». So it's never below what follows our last confirmed transaction there.
    func nextNonce(_ rpc: EVMRPC) async throws -> UInt64 {
        let node = try await rpc.nonce(wallet.evm), key = "\(rpc.chainId):\(wallet.evm.lowercased())"
        return Self.evmNonces.withLock { max(node, $0[key] ?? 0) }
    }

    /// Only after a receipt: a transaction that never landed would leave a gap no later one could get past.
    func confirmed(_ next: UInt64, _ rpc: EVMRPC) {
        let key = "\(rpc.chainId):\(wallet.evm.lowercased())"
        Self.evmNonces.withLock { $0[key] = max($0[key] ?? 0, next) }
    }

    /// Set while a gas top-up runs: its own transactions don't start another.
    @TaskLocal static var toppingUp = false
    /// Set by a chain of moves (Workspace.runSteps): a bridge leg signs and goes on, its landing waited for by the step
    /// that spends where it lands.
    @TaskLocal public static var landings: Landings?

    /// `usd` of the network's own coin for gas, at our address there: over Relay from Solana's USDC (it rides on half a
    /// dollar of USDC, which arrives too); when Solana is short, Relay swaps the network's own USDC for it on the chain.
    public func topUpGas(_ net: EVMNetwork, usd: Decimal) async throws -> String {
        guard net.feeToken == nil else { return "\(net.name) takes its fees in \(net.gasCoin): there's no gas coin to top up." }
        guard usd >= Decimal(string: "0.1")!, usd <= 20 else { throw ExecutionError("A gas top-up is between $0.10 and $20.") }
        let me = wallet.evm, rpc = net.rpc
        let before = try await rpc.balance(me)
        let price = try await Relay.nativePrice(chainId: rpc.chainId) ?? 0
        let expected = Self.gasExpected(usd: usd, price: price)
        let solanaUSDC = Decimal(try await self.rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        let dollars = NSDecimalNumber(decimal: ((usd + 1) * 1_000_000).rounded(0, .up)).uint64Value
        if solanaUSDC >= usd + 1 {
            try await ensureEVM(net, usdc: 0, gas: (floor: before + expected, topupUSD: usd))
        } else if try await Hyperliquid.balance(user: me).withdrawable >= usd.rounded(2, .up) + 1 {
            // Hyperliquid's balance brings it with no gas anywhere: the way for BNB Chain and HyperEVM, whose dollars can't pay
            // by signature.
            report(try await hyperliquidGas(net, usd: usd))
        } else if before < net.gasFloor, Self.leavesWithoutGas(net), try await net.usdcBalance(me) >= dollars + 1_000_000 {
            // No gas anywhere: this network's USDC goes to Solana by signature (Relay pays the gas here) — a dollar of it as SOL
            // when Solana has none for its own fee — and the gas comes back from there.
            report("No \(net.gasCoin) on \(net.name) and too little USDC on Solana to bring it: \(net.name)'s USDC goes to Solana by signature first, then the gas comes back.")
            try await Self.$landings.withValue(nil) {
                if try await self.rpc.balance(wallet.solana) < TopUpOrder.solanaFeeFloor {
                    report(try await bridgeWithoutGas(net, to: nil, amount: 1_000_000, sol: true))
                }
                report(try await bridgeWithoutGas(net, to: nil, amount: dollars))
            }
            try await ensureEVM(net, usdc: 0, gas: (floor: before + expected, topupUSD: usd))
        } else {
            try await swapForGas(net, usd: usd)
        }
        let after = try await rpc.balance(me)
        return "Topped up gas on \(net.name): \(Fmt.num(Decimal(before) / 1e18, maxFraction: 8)) → \(Fmt.num(Decimal(after) / 1e18, maxFraction: 8)) \(net.gasCoin) (≈ \(Fmt.usd(usd)))."
    }

    /// Most of the wei `usd` buys at `price`: a top-up's wait ends when 80% of it is there. Rounded first: a quotient's
    /// 38 digits read as uint64 gave 0 (price 3000), and the top-up did nothing.
    static func gasExpected(usd: Decimal, price: Decimal) -> UInt64 {
        price > 0 ? NSDecimalNumber(decimal: (usd / price * 1e18 * Decimal(string: "0.8")!).rounded(0, .down)).uint64Value : 1
    }

    /// Gas bought with the network's own USDC by Relay's swap on the chain: an exact approve to the approval proxy Relay
    /// publishes for it, then its call to that proxy, from and to this wallet, at most 5% (or $0.15) lost.
    private func swapForGas(_ net: EVMNetwork, usd: Decimal) async throws {
        let me = wallet.evm, rpc = net.rpc, chain = net.relayChain
        let units = NSDecimalNumber(decimal: (usd * 1_000_000).rounded(0, .up)).uint64Value
        guard try await net.usdcBalance(me) >= units else {
            throw ExecutionError("No gas on \(net.name), and neither Solana nor \(net.name) has \(Fmt.usd(usd)) of USDC to buy it. Top up either first.")
        }
        guard let proxy = try await Relay.approvalProxy(chainId: chain)?.lowercased() else { throw ExecutionError("Relay publishes no approval proxy for \(net.name).") }
        /// Relay's quote with every check: from and to this wallet, its call to the proxy after an exact approve, what it gives.
        func checked() async throws -> Relay.EVMStep {
            let quote = try await Relay.quote(user: me, recipient: me, originChain: chain, destinationChain: chain, originCurrency: net.usdc,
                                              destinationCurrency: "0x0000000000000000000000000000000000000000", amount: units, units: "\(net.raw(units))")
            let binding = BindingGuard.check(["sender": quote.sender.lowercased(), "recipient": quote.recipient.lowercased()],
                                             expected: ["sender": me.lowercased(), "recipient": me.lowercased()], subject: "Relay quote")
            guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
            guard let call = quote.evm.last, call.to.lowercased() == proxy, call.chainId == chain, call.value == "0" || call.value == "<null>",
                  quote.evm.dropLast().allSatisfy({ step in
                      let args = Data(step.data.dropFirst(4))
                      return step.to.lowercased() == net.usdc && step.data.prefix(4).hex == "095ea7b3"
                          && ABI.address(in: args, word: 0)?.lowercased() == proxy && ABI.decimal(in: args, word: 1) == net.raw(units)
                  }) else {
                throw ExecutionError("Relay's transactions don't go to the approval proxy it publishes for \(net.name), for exactly this amount. Nothing was signed.")
            }
            let worthOut = Decimal(string: quote.amountOutUSD) ?? 0
            guard worthOut > 0 else { throw ExecutionError("Relay doesn't quote \(net.gasCoin) for \(Fmt.usd(usd)) now. Nothing was signed.") }
            try Self.checkLoss(usd - worthOut, paid: usd, cap: max(usd * Decimal(string: "0.05")!, Decimal(string: "0.15")!), what: "Relay's swap")
            return call
        }
        _ = try await checked()
        let before = try await rpc.balance(me)
        // Batched (EIP-7702), the approve goes with the swap; otherwise first, on its own.
        let approveNote = "Approve exactly \(Fmt.num(usd)) USDC to Relay for gas"
        if !canBatch(rpc) { try await approve(rpc, token: net.usdc, spender: proxy, units: net.raw(units), note: approveNote) }
        // Quoted again after the approve's wait, and once more if its dry run reverts: a route is priced for its moment.
        for attempt in 0..<2 {
            let call = try await checked()
            do {
                _ = try await approveAndSend(rpc, token: net.usdc, spender: proxy, units: net.raw(units), approveNote: approveNote,
                                             to: call.to, data: call.data, note: "Relay: \(Fmt.usd(usd)) of USDC into \(net.gasCoin) on \(net.name)")
                break
            } catch where attempt == 0 && "\(error.localizedDescription)".contains("eth_estimateGas") {
                try await Task.sleep(for: .seconds(2))
            }
        }
        for _ in 0..<30 where try await rpc.balance(me) <= before { try await Task.sleep(for: .seconds(1)) }
    }

    /// Tempo: the wallet's fee token set to `token` once, before a call that isn't a plain transfer of it.
    private func chooseFeeToken(_ net: EVMNetwork, _ token: String) async throws {
        guard ABI.address(in: try await net.rpc.read(Tempo.feeManager, try Tempo.userToken(wallet.evm)), word: 0)?.lowercased() != token else { return }
        _ = try await sendEVM(net.rpc, to: Tempo.feeManager, data: try Tempo.setUserToken(token), note: "Pay \(net.name) fees in \(net.gasCoin) from now on")
    }
}
