import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import RelayBridge
import TxGuard
import WalletCore

/// USDC between Solana, Base, Arbitrum and Ethereum through NEAR Intents' 1Click, beside Relay: each bridge quotes both
/// and takes the one that lands more (`bridge`, `solanaToEVM`). 1Click is a deposit to an address its quote names; the
/// live quote must stay within 1% of the one compared, and 1Click itself must have that address bound to our recipient,
/// our refund address, the asset and the amount (DepositGuard) before anything is sent.
extension Executor {
    /// 1Click's asset for USDC on a place: an EVM network's key, Hyperliquid's perp balance, or nil for Solana.
    static func oneClickUSDC(_ place: String?) -> String? {
        place == "hyperliquid" ? OneClick.Asset.hyperCoreUSDC : place.map { OneClick.Asset.usdc(onEVM: $0) } ?? OneClick.Asset.solanaUSDC
    }

    /// What 1Click lands for `units` of USDC from `from` to `to` (nil = Solana), dry: nothing is issued. nil when it doesn't
    /// quote the pair.
    func oneClickQuote(from: String?, to: String?, units: UInt64) async -> OneClick.Quote? {
        guard let origin = Self.oneClickUSDC(from), let destination = Self.oneClickUSDC(to) else { return nil }
        return try? await OneClick.quote(dry: true, origin: origin, destination: destination, amount: "\(units)",
                                         refundTo: from == nil ? wallet.solana : wallet.evm, recipient: to == nil ? wallet.solana : wallet.evm)
    }

    /// The move through 1Click: a fresh quote (within 1% of `compared`), the deposit address checked against what 1Click
    /// bound to it, the USDC sent there from `from` (Solana's own transfer, or an exact ERC-20 transfer the signer asks to
    /// allow in person), then 1Click's states on the running card until it lands. Returns what arrived.
    func oneClickMove(from: String?, to: String?, units: UInt64, compared: Decimal, label: String) async throws -> Decimal {
        guard let origin = Self.oneClickUSDC(from), let destination = Self.oneClickUSDC(to) else { throw ExecutionError("1Click doesn't move USDC there.") }
        let refundTo = from == nil ? wallet.solana : wallet.evm, recipient = to == nil ? wallet.solana : wallet.evm
        let quote = try await OneClick.quote(dry: false, origin: origin, destination: destination, amount: "\(units)", refundTo: refundTo, recipient: recipient)
        guard let deposit = quote.depositAddress else { throw ExecutionError("1Click gave no deposit address. Nothing was sent.") }
        var envelope = Envelope.approve(input: Decimal(units) / 1_000_000, quotedOutput: compared)
        envelope.destination = deposit
        let problems = envelope.violations(input: Decimal(string: quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                           output: Decimal(string: quote.amountOutFormatted) ?? 0, destination: deposit)
        guard problems.isEmpty else { throw ExecutionError("1Click's price moved past the one compared: " + problems.joined(separator: " ") + " Nothing was sent.") }
        let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: recipient, refundTo: refundTo,
                                         origin: origin, destination: destination, amount: "\(units)")
        guard binding.ok else { throw GuardStop("The guard stopped the deposit: " + binding.problems.joined(separator: " ")) }
        let tx: String
        if let from, let net = EVMNetwork.named(from) {
            try await ensureEVM(net, usdc: units)
            tx = try await sendEVM(net.rpc, to: net.usdc, data: try ERC20.transfer(to: deposit, amount: units),
                                   note: "Send \(label) to the 1Click deposit", recipients: [deposit])
        } else {
            tx = String(try await sendSolanaUSDC(units, to: deposit, label: "the 1Click deposit").split(separator: "/").last ?? "")
        }
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "USDC \(from ?? "solana")→\(to ?? "solana") \(label)"))
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: deposit) }
        let s = try await oneClickSettled(deposit, refund: "USDC went back to \(from.map(WalletCommand.placeName) ?? "Solana")", estimate: quote.timeEstimate)
        return Decimal(string: s.amountOutFormatted ?? "") ?? Decimal(string: quote.amountOutFormatted) ?? 0
    }

    /// USDC through 1Click asked for by name («… via NEAR Intents»), whatever Relay would land: `from` and `to` are
    /// «solana» or an EVM network 1Click pays USDC on. nil `usdc` = all of it on `from`.
    /// What `usdc` from `from` lands on `to` through 1Click now, for the card; nil when it doesn't quote. Nothing is issued.
    public func intentsQuote(from: String, to: String, usdc: Decimal) async -> (out: Decimal, seconds: Int?)? {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units > 0, let q = await oneClickQuote(from: from == "solana" ? nil : from, to: to == "solana" ? nil : to, units: units),
              let out = Decimal(string: q.amountOutFormatted) else { return nil }
        return (out, q.timeEstimate)
    }

    /// `shown`: what the card said arrives; the move stops when 1Click now lands more than 1% under it.
    public func intentsBridge(from: String, to: String, usdc: Decimal?, shown: Decimal? = nil) async throws -> String {
        let src = from == "solana" ? nil : from, dst = to == "solana" ? nil : to
        let a = WalletCommand.placeName(from), b = WalletCommand.placeName(to)
        guard src != dst, src.map({ OneClick.Asset.usdc(onEVM: $0) != nil }) ?? true, dst.map({ OneClick.Asset.usdc(onEVM: $0) != nil }) ?? true else {
            throw ExecutionError("NEAR Intents doesn't move USDC from \(a) to \(b). Nothing was signed.")
        }
        let held = if let src, let net = EVMNetwork.named(src) { try await net.usdcBalance(wallet.evm) }
                   else { try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC) }
        let units = usdc.map { NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).uint64Value } ?? held
        guard units > 0, units <= held else { throw ExecutionError("\(Fmt.num(Decimal(held) / 1_000_000)) USDC on \(a). Nothing was signed.") }
        guard let q = await oneClickQuote(from: src, to: dst, units: units), let out = Decimal(string: q.amountOutFormatted) else {
            throw ExecutionError("NEAR Intents didn't quote USDC from \(a) to \(b) now. Nothing was signed.")
        }
        let sent = Fmt.num(Decimal(units) / 1_000_000)
        report("NEAR Intents' 1Click: \(sent) USDC from \(a) → ≈ \(Fmt.num(out, maxFraction: 4)) on \(b)\(q.timeEstimate.map { " in ≈ \($0) s" } ?? "").")
        let started = Date()
        let got = try await oneClickMove(from: src, to: dst, units: units, compared: shown ?? out, label: "\(sent) USDC")
        return "Moved \(sent) USDC from \(a) to \(b) through NEAR Intents' 1Click in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(got)) USDC arrived."
    }

    /// Whether 1Click is worth taking over Relay for this move: it must land more than Relay's `relayOut` by at least
    /// `oneClickEdge` — its fresh deposit address is one more address to allow by Touch ID, and it takes ≈ 20–30 s against
    /// Relay's few (live 2026-10-05: 1Click landed 1–3 cents more on 5 to 200 USDC). Said either way on the card.
    func oneClickBeats(from: String?, to: String?, units: UInt64, relayOut: Decimal) async -> OneClick.Quote? {
        guard let q = await oneClickQuote(from: from, to: to, units: units), let out = Decimal(string: q.amountOutFormatted) else { return nil }
        let edge = Self.oneClickEdge(Decimal(units) / 1_000_000), better = out - relayOut >= edge
        report("Relay lands ≈ \(Fmt.num(relayOut, maxFraction: 4)) USDC, NEAR Intents' 1Click ≈ \(Fmt.num(out, maxFraction: 4))\(q.timeEstimate.map { " in ≈ \($0) s" } ?? ""): taking "
               + (better ? "1Click — its fresh deposit address needs \(Presence.name) first." : "Relay\(out > relayOut ? " (1Click's \(Fmt.usd(out - relayOut)) more isn't worth another \(Presence.short) and the wait)" : "")."))
        return better ? q : nil
    }

    /// What sending `amount` of `symbol` on a network ihatecrypto has no wallet on costs in USDC, and where that USDC comes
    /// from: Solana, or an EVM network 1Click takes USDC from when Solana is short (nearSource). Dry: nothing is issued.
    /// `confidential`: through Confidential Intents (a private send), where USDC on Solana and the EVM networks is paid too.
    public func acrossQuote(_ symbol: String, amount: Decimal, network: String, to recipient: String, confidential: Bool = false)
        async throws -> (quote: OneClick.Quote, asset: String, units: Decimal, source: EVMNetwork?) {
        let found = confidential ? OneClick.Asset.payout(symbol, on: network) : OneClick.Asset.across[network]?.first(where: { $0.symbol == symbol.uppercased() })
        guard let asset = found else {
            throw ExecutionError("NEAR Intents doesn't pay out \(symbol.uppercased()) on \(AddressCheck.Network(rawValue: network)?.title ?? network).")
        }
        let units = (amount * Decimal(sign: .plus, exponent: asset.decimals, significand: 1)).whole
        guard units > 0 else { throw ExecutionError("\(Fmt.num(amount)) \(asset.symbol) is less than its smallest unit.") }
        let solanaQuote = try await OneClick.quote(dry: true, exactOutput: true, origin: OneClick.Asset.solanaUSDC, destination: asset.id,
                                                   amount: "\(units)", refundTo: wallet.solana, recipient: recipient, confidential: confidential)
        let cost = Decimal(string: solanaQuote.amountInFormatted) ?? 0
        let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        let idle = Dictionary(await idleUSDC().map { ($0.net.key, $0.usdc) }, uniquingKeysWith: +)
        guard let src = Self.nearSource(usdc: cost, onSolana: onSolana, held: { idle[$0] }), let origin = OneClick.Asset.usdc(onEVM: src.key) else {
            guard onSolana >= cost else {
                throw ExecutionError("\(Fmt.num(amount)) \(asset.symbol) costs ≈ \(Fmt.num(cost)) USDC; Solana has \(Fmt.num(onSolana)) and no network 1Click takes USDC from has that much. Nothing was signed.")
            }
            return (solanaQuote, asset.id, units, nil)
        }
        let q = try await OneClick.quote(dry: true, exactOutput: true, origin: origin, destination: asset.id, amount: "\(units)", refundTo: wallet.evm,
                                         recipient: recipient, confidential: confidential)
        return (q, asset.id, units, src)
    }

    /// `amount` of a coin on a network ihatecrypto has no wallet on — BTC, ZEC, TRX or USDT on TRON, XRP, DOGE, SUI — to
    /// a saved address there, paid in USDC through 1Click: exact output, so the address gets `amount` and 1Click returns
    /// what's left of the USDC sent. The live quote must stay within 1% of the one compared, and 1Click itself must have
    /// the deposit address bound to this recipient, our refund address, the assets and the amount before anything is sent.
    /// The USDC goes to 1Click's fresh deposit address, which the signer asks to allow in person.
    /// `confidential`: a private send through Confidential Intents — the deposit and the payout can't be matched on chain.
    public func sendAcross(_ symbol: String, amount: Decimal, network: String, to recipient: String, label: String,
                           confidential: Bool = false) async throws -> String {
        let (shown, asset, units, src) = try await acrossQuote(symbol, amount: amount, network: network, to: recipient, confidential: confidential)
        let origin = src.flatMap { OneClick.Asset.usdc(onEVM: $0.key) } ?? OneClick.Asset.solanaUSDC
        let refundTo = src == nil ? wallet.solana : wallet.evm
        let quote = try await OneClick.quote(dry: false, exactOutput: true, origin: origin, destination: asset, amount: "\(units)",
                                             refundTo: refundTo, recipient: recipient, confidential: confidential)
        guard let deposit = quote.depositAddress, let sendUnits = UInt64(quote.amountIn), sendUnits > 0 else { throw ExecutionError("1Click gave no deposit address. Nothing was sent.") }
        let cost = Decimal(sendUnits) / 1_000_000, shownCost = Decimal(string: shown.amountInFormatted) ?? 0
        guard cost <= shownCost * Decimal(string: "1.01")! + Decimal(string: "0.01")! else {
            throw ExecutionError("1Click now wants \(Fmt.num(cost)) USDC, more than the \(Fmt.num(shownCost)) quoted. Nothing was sent.")
        }
        let bound = try await OneClick.boundRequest(depositAddress: deposit)
        let binding = DepositGuard.check(bound: bound, recipient: recipient, refundTo: refundTo, origin: origin, destination: asset, amount: "\(units)")
        guard binding.ok else { throw GuardStop("The guard stopped the deposit: " + binding.problems.joined(separator: " ")) }
        // A private send only when 1Click itself holds this deposit as confidential: else it would go in the open.
        if confidential, case let v = DepositGuard.confidential(bound: bound), !v.ok {
            throw GuardStop("The guard stopped the deposit: " + v.problems.joined(separator: " ") + " Nothing was sent.")
        }
        let from = src?.name ?? "Solana", what = "\(Fmt.num(amount, maxFraction: 9)) \(symbol.uppercased())"
        report("NEAR Intents\(confidential ? " (private)" : ""): \(Fmt.num(cost)) USDC from \(from) → \(what) to «\(label)»; what isn't used comes back.")
        let tx: String
        if let src {
            try await ensureEVM(src, usdc: sendUnits)
            tx = try await sendEVM(src.rpc, to: src.usdc, data: try ERC20.transfer(to: deposit, amount: sendUnits),
                                   note: "Send \(Fmt.num(cost)) USDC to the 1Click deposit for \(what)", recipients: [deposit])
        } else {
            tx = String(try await sendSolanaUSDC(sendUnits, to: deposit, label: "the 1Click deposit").split(separator: "/").last ?? "")
        }
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "\(from) USDC→\(what) to \(label)\(confidential ? " (private)" : "")"))
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: deposit) }
        let started = Date()
        let s = try await oneClickSettled(deposit, refund: "USDC went back to \(from)", estimate: quote.timeEstimate)
        return "Sent \(Fmt.num(Decimal(string: s.amountOutFormatted ?? "") ?? amount, maxFraction: 9)) \(symbol.uppercased()) to «\(label)» for \(Fmt.num(cost)) USDC from \(from), through NEAR Intents\(confidential ? "' Confidential Intents: the deposit and the payout can't be matched on chain" : ""), in \(Int(Date().timeIntervalSince(started))) s."
    }

    /// USDC from Solana to an EVM network's own USDC through NEAR Intents' 1Click, as `bridge` does it: for a payment
    /// there that its USDC can't cover. What arrived.
    public func usdcByIntents(to network: String, usdc: Decimal) async throws -> Decimal {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .up)).uint64Value
        guard let q = await oneClickQuote(from: nil, to: network, units: units), let out = Decimal(string: q.amountOutFormatted) else {
            throw ExecutionError("NEAR Intents doesn't quote USDC from Solana to \(network) now. Nothing was sent.")
        }
        return try await oneClickMove(from: nil, to: network, units: units, compared: out, label: "USDC to \(network) through NEAR Intents")
    }

    /// USDC from Solana into an agent wallet on NEAR Intents (Agent API). The deposit address the API gave must be
    /// 1Click's, bound by 1Click itself to the agent's own NEAR Intents account and to Solana USDC, before anything is sent.
    public func fundAgent(units: UInt64, depositAddress: String, agentAccount: String) async throws -> String {
        let bound = try await OneClick.boundRequest(depositAddress: depositAddress)
        guard bound["recipient"] == agentAccount else {
            throw GuardStop("The guard stopped the deposit: 1Click has it paying «\(bound["recipient"] ?? "nobody")», not the agent wallet. Nothing was sent.")
        }
        guard bound["originAsset"] == OneClick.Asset.solanaUSDC else {
            throw GuardStop("The guard stopped the deposit: it doesn't take USDC on Solana. Nothing was sent.")
        }
        // And it stays USDC: the deposit address is the Agent API's, and a swap into another token would be at its rate.
        guard bound["destinationAsset"] == OneClick.Asset.solanaUSDC else {
            throw GuardStop("The guard stopped the deposit: 1Click would turn the USDC into «\(bound["destinationAsset"] ?? "something else")» for the agent wallet. Nothing was sent.")
        }
        // Exactly this amount, and a refund back to the agent itself: a short deposit can't be refunded elsewhere.
        guard bound["amount"] == "\(units)" else {
            throw GuardStop("The guard stopped the deposit: 1Click expects \(bound["amount"] ?? "another amount"), not \(units). Nothing was sent.")
        }
        guard let refund = bound["refundTo"], refund == agentAccount else {
            throw GuardStop("The guard stopped the deposit: a refund would go to «\(bound["refundTo"] ?? "nobody")», not the agent wallet. Nothing was sent.")
        }
        report("NEAR Intents: \(Self.usdc(units)) USDC from Solana into the agent wallet.")
        let tx = String(try await sendSolanaUSDC(units, to: depositAddress, label: "the agent wallet's deposit").split(separator: "/").last ?? "")
        try journal.append(.init(chain: "1click", id: depositAddress, kind: .signed, note: "Solana USDC → agent wallet"))
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: depositAddress) }
        return tx
    }

    /// A deposit address on a network ihatecrypto has no wallet on — Sui, Bitcoin, TRON… — for about `amount` of `symbol`
    /// sent there from an exchange or another wallet: 1Click swaps what arrives into USDC on Solana, to this wallet.
    /// Nothing is signed. The address is 1Click's, checked to be bound (DepositGuard) to our Solana address, the assets and
    /// the amount; any deposit from the quote's minimum is taken (FLEX_INPUT) for an hour. A refund goes to the wallet's
    /// NEAR Intents account (`nearAccount`, its own NEAR key) — ihatecrypto has no address of the user's there.
    // ponytail: getting a refund out of NEAR Intents isn't built; it waits in that account, under the wallet's NEAR key.
    public func receiveAcross(_ symbol: String, amount: Decimal, network: String, nearAccount: String)
        async throws -> (deposit: String, quote: OneClick.Quote, minimum: Decimal) {
        guard let asset = OneClick.Asset.across[network]?.first(where: { $0.symbol == symbol.uppercased() }) else {
            throw ExecutionError("NEAR Intents doesn't take \(symbol.uppercased()) on \(AddressCheck.Network(rawValue: network)?.title ?? network).")
        }
        guard nearAccount.wholeMatch(of: /[0-9a-f]{64}/) != nil else { throw ExecutionError("A refund needs the wallet's NEAR account, and it has none.") }
        let units = (amount * Decimal(sign: .plus, exponent: asset.decimals, significand: 1)).whole
        guard units > 0 else { throw ExecutionError("\(Fmt.num(amount)) \(asset.symbol) is less than its smallest unit.") }
        let quote = try await OneClick.quote(dry: false, origin: asset.id, destination: OneClick.Asset.solanaUSDC, amount: "\(units)",
                                             refundTo: nearAccount, recipient: wallet.solana, flexible: true, refundToIntents: true, minutes: 60)
        guard let deposit = quote.depositAddress else { throw ExecutionError("1Click gave no deposit address.") }
        let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: wallet.solana, refundTo: nearAccount,
                                         origin: asset.id, destination: OneClick.Asset.solanaUSDC, amount: "\(units)")
        guard binding.ok else { throw GuardStop("The guard refused 1Click's address: " + binding.problems.joined(separator: " ")) }
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed,
                                 note: "Receive: \(AddressCheck.Network(rawValue: network)?.title ?? network) \(asset.symbol)→Solana USDC \(Fmt.num(amount))"))
        let minimum = (Decimal(string: quote.minAmountIn ?? "") ?? units) / Decimal(sign: .plus, exponent: asset.decimals, significand: 1)
        return (deposit, quote, minimum)
    }

    /// An MPP charge through NEAR Intents (paymentauth.org draft-nearintents-charge-01): `units` of USDC on `from` (nil =
    /// Solana) to the 1Click deposit address the service's 402 named. 1Click itself must hold that address for the
    /// service's recipient, its refund address, exactly `amountOut` and, when known, the asset it receives, at no more
    /// than 1Click's own price (DepositGuard.charge) before anything is sent. The signer asks to allow the deposit address
    /// in person. Returns the deposit's transaction hash: the payment's proof.
    public func payIntentsCharge(from: String?, units: UInt64, deposit: String, recipient: String, refundTo: String, amountOut: String,
                                 destination: String?, service: String) async throws -> String {
        guard let origin = Self.oneClickUSDC(from) else { throw ExecutionError("NEAR Intents doesn't take USDC on \(from ?? "solana"). Nothing was paid.") }
        // Gas and USDC there first: the card says so. The binding is read after, right before the signature.
        if let from, let net = EVMNetwork.named(from) { try await ensureEVM(net, usdc: units) }
        let verdict = DepositGuard.charge(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: recipient, refundTo: refundTo,
                                          origin: origin, destination: destination, amountOut: amountOut, deposit: units)
        guard verdict.ok else { throw GuardStop("The guard stopped the payment: " + verdict.problems.joined(separator: " ") + " Nothing was paid.") }
        let name = from.map(WalletCommand.placeName) ?? "Solana", usdc = Self.usdc(units)
        report("NEAR Intents: \(usdc) USDC from \(name) to the 1Click deposit \(service)'s payment names.")
        let tx: String
        if let from, let net = EVMNetwork.named(from) {
            tx = try await sendEVM(net.rpc, to: net.usdc, data: try ERC20.transfer(to: deposit, amount: units),
                                   note: "Pay \(usdc) USDC to \(service) through NEAR Intents", recipients: [deposit])
        } else {
            tx = String(try await sendSolanaUSDC(units, to: deposit, label: "\(service)'s 1Click deposit").split(separator: "/").last ?? "")
        }
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "MPP \(service) · \(usdc) USDC from \(name)"))
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: deposit) }
        return tx
    }

    /// What 1Click must land beyond Relay to be taken: 10 cents, or 0.05% of a bigger move.
    static func oneClickEdge(_ usdc: Decimal) -> Decimal { max(Decimal(string: "0.10")!, usdc * Decimal(string: "0.0005")!) }
}
