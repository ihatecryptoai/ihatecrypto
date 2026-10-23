import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import RelayBridge
import SolanaChain
import TxGuard
import WalletCore

/// Any token on one network for any token on another through Relay, in one signature: SOL on Solana for ETH on Arbitrum,
/// AERO on Base for SOL on Solana. Relay's solver swaps on the way. What goes out is exactly the amount said; what comes
/// in is judged by value — at most 3% or 15 cents lost, as Relay's own sale. From Solana: Relay's Depository deposit (SOL,
/// USDC, a token it takes as is); a route through other programs is refused by the guard. From an EVM network: an exact
/// approve to Relay's approval proxy and its call, or to its depository for a token it holds as is (USDC) — one EIP-7702
/// transaction where it batches — or the network's coin sent to the depository, an address the signer allows in person
/// once.
extension Executor {
    struct CrossSide {
        let chain: UInt64
        let currency: String
        let decimals: Int
        let symbol: String
        /// The network's own coin: SOL, ETH, POL.
        let coin: Bool
        var token2022 = false
    }

    static let evmCoin = "0x0000000000000000000000000000000000000000"

    func crossSide(_ token: String, on place: String) async throws -> CrossSide {
        if place == "solana" {
            switch token.uppercased() {
            case "SOL": return CrossSide(chain: Relay.Chain.solana, currency: Relay.Currency.solanaSOL, decimals: 9, symbol: "SOL", coin: true)
            case "USDC": return CrossSide(chain: Relay.Chain.solana, currency: Relay.Currency.solanaUSDC, decimals: 6, symbol: "USDC", coin: false)
            default:
                // USDT, USDS and USDe by their mints (Stablecoins): a ticker search can land on a lookalike.
                let stable = Stablecoins.find(token, on: "solana")?.address
                let mint = token.count >= 32 ? token : stable != nil ? stable : await TokenResearch.solanaMint(symbol: token)
                guard let mint, let info = await TokenResearch.solanaToken(mint) else { throw ExecutionError("Jupiter knows no verified \(token) on Solana. Nothing was signed.") }
                return CrossSide(chain: Relay.Chain.solana, currency: mint, decimals: info.decimals, symbol: info.symbol, coin: false, token2022: info.token2022)
            }
        }
        guard let net = EVMNetwork.named(place) else { throw ExecutionError("Relay swaps between Solana, Base, Arbitrum, Ethereum, Polygon and Robinhood Chain.") }
        if Self.isCoin(token, on: net) { return CrossSide(chain: net.relayChain, currency: Self.evmCoin, decimals: 18, symbol: net.gasCoin, coin: true) }
        let t = try await evmToken(token, on: net)
        return CrossSide(chain: net.relayChain, currency: t.address, decimals: t.decimals, symbol: t.symbol, coin: false)
    }

    /// Relay's quote for the swap, checked: from this wallet to this wallet, and no more value lost than allowed.
    /// `amount` nil = all of a token (never all of a coin: its fees come from it).
    /// `capped` false: for the card, which shows the cost instead of stopping at it, priced at the amount asked even when
    /// the wallet holds less (the run checks it).
    func crossQuote(_ a: String, on from: String, for b: String, on to: String, amount: Decimal?, capped: Bool = true)
        async throws -> (quote: Relay.Quote, origin: CrossSide, destination: CrossSide, units: Decimal) {
        guard from != to else { throw ExecutionError("Both sides are on \(WalletCommand.placeName(from)): that's a swap there, not between networks.") }
        async let o = crossSide(a, on: from)
        async let d = crossSide(b, on: to)
        let (origin, destination) = try await (o, d)
        let scale = Decimal(sign: .plus, exponent: origin.decimals, significand: 1)
        let held: Decimal = if from == "solana" {
            origin.coin ? Decimal(try await rpc.balance(wallet.solana)) : Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: origin.currency))
        } else {
            origin.coin ? Decimal(try await EVMNetwork.named(from)!.rpc.balance(wallet.evm)) : try await balance(origin.currency, on: EVMNetwork.named(from)!.rpc)
        }
        guard let amount else {
            guard !origin.coin else { throw ExecutionError("Say how much \(origin.symbol): its network fees come out of it. Nothing was signed.") }
            guard held > 0 else { throw ExecutionError("No \(origin.symbol) on \(WalletCommand.placeName(from)).") }
            return try await crossQuote(a, on: from, for: b, on: to, amount: held / scale)
        }
        let units = (amount * scale).whole
        guard units > 0 else { throw ExecutionError("\(Fmt.num(amount)) \(origin.symbol) is less than its smallest unit.") }
        guard units <= held || !capped else {
            throw ExecutionError("\(Fmt.num(held / scale, maxFraction: 8)) \(origin.symbol) on \(WalletCommand.placeName(from)); \(Fmt.num(amount, maxFraction: 8)) is more. Nothing was signed.")
        }
        let me = from == "solana" ? wallet.solana : wallet.evm, recipient = to == "solana" ? wallet.solana : wallet.evm
        let quote = try await relayQuote(user: me, recipient: recipient, originChain: origin.chain, destinationChain: destination.chain,
                                          originCurrency: origin.currency, destinationCurrency: destination.currency, amount: 0, units: "\(units)")
        let binding = BindingGuard.check(["sender": quote.sender, "recipient": quote.recipient],
                                         expected: ["sender": me, "recipient": recipient], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        let worthIn = Decimal(string: quote.amountInUSD) ?? 0, worthOut = Decimal(string: quote.amountOutUSD) ?? 0
        guard worthOut > 0 else { throw ExecutionError("Relay doesn't quote \(origin.symbol) for \(destination.symbol) now. Nothing was signed.") }
        if capped { try Self.checkLoss(worthIn - worthOut, paid: worthIn, cap: max(worthIn * Decimal(string: "0.03")!, Decimal(string: "0.15")!), what: "Relay's swap") }
        return (quote, origin, destination, units)
    }

    /// The swap's quote for the card: what arrives, its worth, and how long Relay says it takes. Nothing is signed.
    /// NEAR's side, or a pair Relay has no route for, is NEAR Intents' (IntentSwap.swift).
    public func crossQuoteText(_ a: String, on from: String, for b: String, on to: String, amount: Decimal?, near: String? = nil) async throws -> (gets: String, way: String) {
        if from == "near" || to == "near" {
            guard let amount else { throw ExecutionError("Say how much \(a.uppercased()).") }
            return try await intentQuoteText(a, on: from, for: b, on: to, amount: amount, near: near)
        }
        do {
            let (q, _, d, _) = try await crossQuote(a, on: from, for: b, on: to, amount: amount, capped: false)
            return ("\(Fmt.num(Decimal(string: q.amountOutFormatted) ?? 0, maxFraction: 8)) \(d.symbol) ≈ \(Fmt.usd(Decimal(string: q.amountOutUSD) ?? 0))", "≈ \(max(q.seconds, 1)) s")
        } catch {
            guard let amount, let other = try? await intentQuoteText(a, on: from, for: b, on: to, amount: amount, near: near) else { throw error }
            return other
        }
    }

    /// The swap: Relay's quote checked, its one transaction signed on the origin network, then Relay's word that it landed.
    /// NEAR's side goes through NEAR Intents, and so does a pair Relay has no route for when 1Click lists both tokens.
    public func crossSwap(_ a: String, on from: String, for b: String, on to: String, amount: Decimal?, near: String? = nil) async throws -> String {
        if from == "near" || to == "near" { return try await intentSwap(a, on: from, for: b, on: to, amount: amount, near: near) }
        if let why = Modules.current.refusal(.relay) { throw ExecutionError(why) }
        let quoted: (quote: Relay.Quote, origin: CrossSide, destination: CrossSide, units: Decimal)
        do { quoted = try await crossQuote(a, on: from, for: b, on: to, amount: amount) }
        catch where !(error is CostlyMove) {
            guard let amount, (try? await intentQuote(a, on: from, for: b, on: to, amount: amount, near: near, dry: true)) != nil else { throw error }
            report("Relay: \(error.localizedDescription) NEAR Intents has a route: taking it.")
            return try await intentSwap(a, on: from, for: b, on: to, amount: amount, near: near)
        }
        let (quote, origin, destination, units) = quoted
        let scale = Decimal(sign: .plus, exponent: origin.decimals, significand: 1)
        let sent = "\(Fmt.num(units / scale, maxFraction: 8)) \(origin.symbol)"
        let gets = "\(Fmt.num(Decimal(string: quote.amountOutFormatted) ?? 0, maxFraction: 8)) \(destination.symbol)"
        let fromName = WalletCommand.placeName(from), toName = WalletCommand.placeName(to)
        let what = "\(fromName) \(origin.symbol)→\(toName) \(destination.symbol) \(Fmt.num(units / scale, maxFraction: 8))"
        report("Relay: \(sent) on \(fromName) (≈ \(Fmt.usd(Decimal(string: quote.amountInUSD) ?? 0))) → ≈ \(gets) on \(toName) (≈ \(Fmt.usd(Decimal(string: quote.amountOutUSD) ?? 0))), ≈ \(max(quote.seconds, 1)) s.")
        let started = Date()
        let link: String
        if from == "solana" {
            link = "https://solscan.io/tx/" + (try await crossFromSolana(quote, origin, to: destination.chain, units: units, what: what))
        } else {
            guard let net = EVMNetwork.named(from) else { throw ExecutionError("ihatecrypto doesn't move money on \(from).") }
            let hash = try await crossFromEVM(quote, origin, net, units: units, sent: sent)
            try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(what), tx \(hash)"))
            link = "\(net.explorer)/tx/\(hash)"
            while Date().timeIntervalSince(started) < 240 {
                let status = try await Relay.status(requestId: quote.requestId).status
                if status == "success" { try journal.append(.init(chain: "relay", id: quote.requestId, kind: .settled, note: status)); break }
                if ["refund", "failure"].contains(status) {
                    try journal.append(.init(chain: "relay", id: quote.requestId, kind: .failed, note: status))
                    throw ExecutionError("Relay returned \(status): the \(origin.symbol) comes back on \(fromName). \(link)")
                }
                try await Task.sleep(for: .seconds(2))
            }
        }
        let landed = try await Relay.status(requestId: quote.requestId).status == "success"
        return landed ? "Swapped \(sent) on \(fromName) for ≈ \(gets) on \(toName) through Relay, in \(Int(Date().timeIntervalSince(started))) s. \(link)"
                      : "\(sent) left \(fromName); ≈ \(gets) is on its way to \(toName) (Relay order \(quote.requestId), in transit on the portfolio). \(link)"
    }

    /// Solana's side: Relay's instructions compiled, checked by the guard (Relay's Depository only), simulated, then sent
    /// as a Relay bridge is (sendBridge waits for the order to land). Returns the transaction's signature.
    private func crossFromSolana(_ quote: Relay.Quote, _ origin: CrossSide, to chain: UInt64, units: Decimal, what: String) async throws -> String {
        let (blockhash, lastValid) = try await rpc.latestBlockhash()
        let unsigned = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage(quote.solanaInstructions, payer: wallet.solana, blockhash: blockhash))
        let message = try SolanaMessage.parse(transaction: unsigned)
        let account = origin.coin ? nil : try SPL.associatedAccount(owner: wallet.solana, mint: origin.currency,
                                                                    program: origin.token2022 ? SPL.token2022Program : SPL.tokenProgram)
        let owners = account.map { [$0: wallet.solana] } ?? [:]
        var verdict = SolanaGuard.check(message, accounts: message.staticKeys, policy: SolanaPolicy(owner: wallet.solana), tokenOwners: owners)
        guard verdict.ok else {
            throw GuardStop("Relay routes \(origin.symbol) through programs ihatecrypto doesn't sign for (" + verdict.problems.joined(separator: " ")
                                 + "). Sell it for USDC or SOL on Solana first, then swap that. Nothing was signed.")
        }
        let lamports = try await rpc.balance(wallet.solana)
        let tokens = account == nil ? nil : try await rpc.tokenBalance(owner: wallet.solana, mint: origin.currency)
        let simulation = try await rpc.simulate(unsigned, watch: [wallet.solana] + (account.map { [$0] } ?? []), sigVerify: false)
        if let error = simulation.error { throw ExecutionError("The simulation failed, so nothing was signed: \(error)") }
        let spend = NSDecimalNumber(decimal: units).int64Value
        var checks = [SolanaGuard.BalanceRule(label: "lamports", before: Int64(lamports), after: Int64(simulation.after[wallet.solana]?.lamports ?? 0),
                                          atLeast: -(origin.coin ? spend : 0) - 5_000_000)]
        if let account, let tokens {
            checks.append(.init(label: origin.symbol, before: Int64(tokens), after: Int64(simulation.after[account]?.token ?? 0), atLeast: -spend))
        }
        verdict.merge(SolanaGuard.balances(checks))
        guard verdict.ok else { throw GuardStop("The guard stopped the swap: " + verdict.problems.joined(separator: " ")) }
        var step = BridgeStep(requestId: quote.requestId, unsigned: unsigned, accounts: message.staticKeys, tokenOwners: owners, lastValid: lastValid,
                              spend: 0, arriving: 0, feesUSD: quote.feesUSD, chain: chain)
        step.note = what
        return try await sendBridge(step)
    }

    /// An EVM network's side: the coin straight to Relay's depository for exactly the amount, or an exact approve to its
    /// approval proxy and the proxy's call. Returns the transaction's hash.
    private func crossFromEVM(_ quote: Relay.Quote, _ origin: CrossSide, _ net: EVMNetwork, units: Decimal, sent: String) async throws -> String {
        let note = "Relay: \(sent) on \(net.name) → \(quote.recipient == wallet.solana ? "Solana" : "another network")"
        if origin.coin {
            guard let depository = try await Relay.depository(chainId: net.relayChain)?.lowercased(), quote.evm.count == 1, let call = quote.evm.first,
                  call.to.lowercased() == depository, call.chainId == net.relayChain, Decimal(string: call.value) == units, let value = UInt64(exactly: NSDecimalNumber(decimal: units).uint64Value) else {
                throw ExecutionError("Relay's transaction isn't one deposit of exactly \(sent) to the depository it publishes for \(net.name). Nothing was signed.")
            }
            // Coins to Relay's depository pay Relay: the signer asks to allow that address in person the first time.
            return try await sendEVM(net.rpc, to: call.to, data: call.data, note: note, recipients: [depository], value: value)
        }
        // A token Relay swaps goes through its approval proxy; one it holds as is (USDC) straight into its depository,
        // depositErc20(this wallet, the token, exactly the amount, the order's id). Either way the approve is exact.
        let proxy = try await Relay.approvalProxy(chainId: net.relayChain)?.lowercased()
        let depository = try await Relay.depository(chainId: net.relayChain)?.lowercased()
        let token = origin.currency.lowercased()
        guard let call = quote.evm.last, call.chainId == net.relayChain, call.value == "0" || call.value == "<null>" else {
            throw ExecutionError("Relay's transaction isn't a plain call on \(net.name). Nothing was signed.")
        }
        let args = Data(call.data.dropFirst(4))
        let spender: String? = if call.to.lowercased() == proxy { proxy }
            else if call.to.lowercased() == depository, call.data.prefix(4).hex == "e8017952", args.count == 4 * 32,
                    ABI.address(in: args, word: 0)?.lowercased() == wallet.evm.lowercased(), ABI.address(in: args, word: 1)?.lowercased() == token,
                    ABI.decimal(in: args, word: 2) == units { depository }
            else { nil }
        guard let spender, quote.evm.dropLast().allSatisfy({ $0.to.lowercased() == token && $0.data.prefix(4).hex == "095ea7b3"
                  && ABI.address(in: Data($0.data.dropFirst(4)), word: 0)?.lowercased() == spender && ABI.decimal(in: Data($0.data.dropFirst(4)), word: 1) == units }) else {
            throw ExecutionError("Relay's transactions don't go to the approval proxy or the depository it publishes for \(net.name), for exactly \(sent). Nothing was signed.")
        }
        return try await approveAndSend(net.rpc, token: origin.currency, spender: spender, units: units, approveNote: "Approve exactly \(sent) to Relay",
                                        to: call.to, data: call.data, note: note)
    }
}
