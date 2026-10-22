import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import TxGuard
import WalletCore

/// Any token for any other through NEAR Intents' 1Click — the ≈ 200 tokens its /v0/tokens lists: on NEAR itself (NEAR for
/// AURORA), between NEAR and any network ihatecrypto has a wallet on, and where Relay has no route. Always to this wallet's
/// own address on the other side, the refund to its own on this one. What goes in is a plain transfer to the deposit
/// address 1Click binds to this quote — read back from 1Click before signing (DepositGuard) — native NEAR through the
/// signer's wrap.near deposit; the live quote stays within 1% of the one shown (Envelope).
extension Executor {
    /// 1Click's name for each place ihatecrypto has a wallet on (its `blockchain`).
    static let intentChains = ["solana": "sol", "base": "base", "arbitrum": "arb", "optimism": "op", "ethereum": "eth",
                               "polygon": "pol", "bnb": "bsc", "near": "near"]

    struct IntentSide {
        let asset: String
        let symbol: String
        let decimals: Int
    }

    /// The token as 1Click lists it on that network, by ticker or contract; NEAR on NEAR is its wNEAR.
    func intentSide(_ token: String, on place: String) async throws -> IntentSide {
        guard let chain = Self.intentChains[place] else {
            throw ExecutionError("NEAR Intents swaps for this wallet on Solana, NEAR and the EVM networks, not on \(WalletCommand.placeName(place)).")
        }
        let want = token.uppercased() == "NEAR" && place == "near" ? "WNEAR" : token.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        guard let t = try await OneClick.tokens().first(where: {
            $0.blockchain == chain && ($0.symbol.uppercased() == want || $0.contract == token.lowercased())
        }) else {
            throw ExecutionError("NEAR Intents has no \(token) on \(WalletCommand.placeName(place)). Nothing was signed.")
        }
        return IntentSide(asset: t.assetId, symbol: t.symbol == "wNEAR" ? "NEAR" : t.symbol, decimals: t.decimals)
    }

    /// This wallet's own address on `place`: Solana's, the EVM one, or its NEAR account (`near`, from the signer).
    private func intentAddress(_ place: String, near: String?) throws -> String {
        if place == "near" {
            guard let near, near.wholeMatch(of: /[0-9a-f]{64}/) != nil else { throw ExecutionError("The signer has no NEAR account for this wallet.") }
            return near
        }
        return place == "solana" ? wallet.solana : wallet.evm
    }

    /// 1Click's quote for the swap: `dry` for the card, else with a deposit address bound to it.
    func intentQuote(_ a: String, on from: String, for b: String, on to: String, amount: Decimal, near: String?, dry: Bool)
        async throws -> (quote: OneClick.Quote, origin: IntentSide, destination: IntentSide, units: String, recipient: String, refund: String) {
        async let o = intentSide(a, on: from)
        async let d = intentSide(b, on: to)
        let (origin, destination) = try await (o, d)
        guard origin.asset != destination.asset else { throw ExecutionError("That's the same token on both sides. Nothing to swap.") }
        let units = "\((amount * pow(10, origin.decimals)).whole)"
        guard units != "0" else { throw ExecutionError("\(Fmt.num(amount)) \(origin.symbol) is less than its smallest unit.") }
        // A dry quote to NEAR before the signer has named the account: any implicit account prices it the same.
        let recipient = try intentAddress(to, near: dry ? near ?? String(repeating: "0", count: 64) : near)
        let refund = try intentAddress(from, near: dry ? near ?? String(repeating: "0", count: 64) : near)
        let quote = try await OneClick.quote(dry: dry, origin: origin.asset, destination: destination.asset, amount: units,
                                             refundTo: refund, recipient: recipient)
        return (quote, origin, destination, units, recipient, refund)
    }

    /// What the swap gets, for the card: «≈ 12.4 AURORA ≈ $1.98», and the way.
    public func intentQuoteText(_ a: String, on from: String, for b: String, on to: String, amount: Decimal, near: String?) async throws -> (gets: String, way: String) {
        let q = try await intentQuote(a, on: from, for: b, on: to, amount: amount, near: near, dry: true)
        let out = Decimal(string: q.quote.amountOutFormatted) ?? 0
        return ("\(Fmt.num(out, maxFraction: 6)) \(q.destination.symbol)\(q.quote.amountOutUSD.map { " ≈ \(Fmt.usd($0))" } ?? "")",
                "NEAR Intents" + (q.quote.timeEstimate.map { " · ≈ \($0) s" } ?? ""))
    }

    /// The swap itself: quoted again with a deposit address, checked, the deposit sent, 1Click followed until it pays.
    public func intentSwap(_ a: String, on from: String, for b: String, on to: String, amount: Decimal?, near: String?) async throws -> String {
        guard let amount else { throw ExecutionError("Say how much \(a.uppercased()): NEAR Intents swaps the amount said. Nothing was signed.") }
        let shown = try await intentQuote(a, on: from, for: b, on: to, amount: amount, near: near, dry: true)
        var envelope = Envelope.approve(input: amount, quotedOutput: Decimal(string: shown.quote.amountOutFormatted) ?? 0)
        let live = try await intentQuote(a, on: from, for: b, on: to, amount: amount, near: near, dry: false)
        guard let deposit = live.quote.depositAddress else { throw ExecutionError("1Click gave no deposit address. Nothing was signed.") }
        envelope.destination = deposit
        let problems = envelope.violations(input: Decimal(string: live.quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                           output: Decimal(string: live.quote.amountOutFormatted) ?? 0, destination: deposit)
        guard problems.isEmpty else { throw ExecutionError("The price moved past what was approved: " + problems.joined(separator: " ")) }
        if let paid = live.quote.amountInUSD, let worth = live.quote.amountOutUSD {
            try Self.checkLoss(paid - worth, paid: paid, cap: max(paid * Decimal(string: "0.03")!, Decimal(string: "0.15")!), what: "NEAR Intents")
        }
        let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: live.recipient, refundTo: live.refund,
                                         origin: live.origin.asset, destination: live.destination.asset, amount: live.units)
        guard binding.ok else { throw GuardStop("The guard stopped the deposit: " + binding.problems.joined(separator: " ")) }
        let gets = Decimal(string: live.quote.amountOutFormatted) ?? 0
        report("NEAR Intents: \(Fmt.num(amount, maxFraction: 8)) \(live.origin.symbol) on \(WalletCommand.placeName(from)) → ≈ \(Fmt.num(gets, maxFraction: 6)) \(live.destination.symbol) on \(WalletCommand.placeName(to)).")
        // The deposit: a transfer to 1Click's address, which the signer asks to allow in person the first time.
        var tx = ""
        if from == "near" {
            guard live.origin.symbol == "NEAR", let account = near else {
                throw ExecutionError("From NEAR, ihatecrypto sends NEAR itself; its other tokens there can't be signed yet. Nothing was signed.")
            }
            let context = try await Near.txContext(account: account)
            let yocto = UInt128(live.units) ?? 0
            let keep = UInt128(context.storageBytes) * 10_000_000_000_000_000_000 + NearTransaction.intentDepositOverhead(gasPrice: UInt128(context.gasPrice) ?? 100_000_000)
            let balance = UInt128(context.balance) ?? 0
            guard yocto + keep <= balance else {
                throw ExecutionError("\(NearTransaction.near(balance > keep ? balance - keep : 0)) NEAR can go: the account keeps \(NearTransaction.near(keep)) for its storage and the transaction.")
            }
            let signed = try await signer.signNearIntentDeposit(amount: yocto, depositAddress: deposit, nonce: context.nonce, blockHash: context.blockHash)
            try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "\(live.origin.symbol)→\(live.destination.symbol) \(amount) near → \(to)"))
            tx = try await Near.send(signed)
        } else {
            let sent = try await sendToken(a, amount: amount, on: from, to: deposit, label: "the 1Click deposit")
            try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "\(live.origin.symbol)→\(live.destination.symbol) \(amount) \(from) → \(to)"))
            tx = String(sent.split(separator: "/").last ?? "").trimmingCharacters(in: .whitespaces)
        }
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: deposit) }
        let started = Date()
        let s = try await oneClickSettled(deposit, refund: "\(live.origin.symbol) went back to \(WalletCommand.placeName(from))", estimate: live.quote.timeEstimate)
        let line = "Swapped \(Fmt.num(amount, maxFraction: 8)) \(live.origin.symbol) on \(WalletCommand.placeName(from)) for "
            + "\(Fmt.num(Decimal(string: s.amountOutFormatted ?? "") ?? gets, maxFraction: 6)) \(live.destination.symbol) on \(WalletCommand.placeName(to)) "
            + "through NEAR Intents in \(Int(Date().timeIntervalSince(started))) s — at your own address there."
        report(line)
        return line
    }
}
