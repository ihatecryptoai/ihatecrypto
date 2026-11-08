import Foundation
import SolanaChain
import WalletCore

/// Swaps inside Solana through Jupiter's free API. Jupiter builds the transaction; the wallet only checks
/// that it is the fee payer and signs (`SolanaTransaction.sign`).
public enum Jupiter {
    static let base = "https://api.jup.ag/swap/v1"
    public static let solMint = "So11111111111111111111111111111111111111112"
    public static let usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

    public struct Quote: Sendable {
        /// The quote exactly as Jupiter returned it; /swap needs it back unchanged.
        public let raw: Data
        public let inAmount: UInt64
        public let outAmount: UInt64
        /// Least output after slippage: the floor the user agrees to.
        public let minOut: UInt64
        public let route: String
        /// The route's pools by Jupiter's label, e.g. ["HumidiFi", "Hadron"]: what `excludeDexes` takes.
        public let dexes: [String]
        /// How far the swap moves the price, 0…1 (Jupiter's priceImpactPct): 0.12 for 2M WIF sold at once (2026-10-28).
        public var impact: Decimal = 0
        /// What comes out, in USD by Jupiter's prices (swapUsdValue); 0 when it doesn't say.
        public var usd: Decimal = 0
        /// What it was asked: the transaction (`swapTransaction`) is built for the same.
        public var input = "", output = "", slippageBps = 50
    }

    /// /build's route doesn't fit in one transaction: its pools, to ask again without them.
    public struct TooBig: LocalizedError {
        public let dexes: [String]
        public let message: String
        public var errorDescription: String? { message }
    }

    /// A swap ready for the signer: its own route and amounts, which can differ a little from the quote's.
    public struct Built: Sendable {
        public let wire: Data
        public let lastValidBlockHeight: UInt64
        public let outAmount: UInt64
        public let minOut: UInt64
        public let route: String
        public let dexes: [String]
    }

    /// Jupiter's price impact (its sign dropped: v2 says it negative) and the swap's USD worth, from a quote or an order.
    static func impactAndWorth(_ j: [String: Any]) -> (impact: Decimal, usd: Decimal) {
        func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue ?? 0 }
        return (abs(dec(j["priceImpactPct"])), dec(j["swapUsdValue"]))
    }

    /// `excludeDexes`: labels Jupiter must route around (a route that failed before signing).
    public static func quote(input: String, output: String, amount: UInt64, slippageBps: Int = 50, excludeDexes: [String] = []) async throws -> Quote {
        var url = URL(string: "\(base)/quote?inputMint=\(input)&outputMint=\(output)&amount=\(amount)&slippageBps=\(slippageBps)")!
        if !excludeDexes.isEmpty { url.append(queryItems: [URLQueryItem(name: "excludeDexes", value: excludeDexes.joined(separator: ","))]) }
        let ask = url
        // A throttle or a gateway's 5xx is asked again (RPCRetry): one 503 here stopped a chain of moves at its swap.
        return try await RPCRetry.run({ try await URLSession.shared.data(from: ask) }) { data, status in
            guard status == 200,
                  let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let inAmount = (j["inAmount"] as? String).flatMap(UInt64.init),
                  let outAmount = (j["outAmount"] as? String).flatMap(UInt64.init),
                  let minOut = (j["otherAmountThreshold"] as? String).flatMap(UInt64.init) else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            let dexes = (j["routePlan"] as? [[String: Any]] ?? []).compactMap { ($0["swapInfo"] as? [String: Any])?["label"] as? String }
            let (impact, usd) = impactAndWorth(j)
            var quote = Quote(raw: data, inAmount: inAmount, outAmount: outAmount, minOut: minOut, route: dexes.joined(separator: " → "), dexes: dexes,
                              impact: impact, usd: usd)
            (quote.input, quote.output, quote.slippageBps) = (input, output, slippageBps)
            return quote
        }
    }

    /// The unsigned v0 transaction for `quote`'s swap, paid by `user`, from Swap API v2's /build: its swap is route_v2 or
    /// shared_accounts_route_v2, whose amounts open the instruction, the only Jupiter swaps the signer reads (the
    /// 2026-11-08 review); /swap v1 builds v1 routes, whose amounts sit after the route. `excludeDexes` as for the quote.
    public static func swapTransaction(_ quote: Quote, user: String, excludeDexes: [String] = []) async throws -> Built {
        var url = URL(string: "\(v2)/build?inputMint=\(quote.input)&outputMint=\(quote.output)&amount=\(quote.inAmount)&slippageBps=\(quote.slippageBps)&taker=\(user)")!
        if !excludeDexes.isEmpty { url.append(queryItems: [URLQueryItem(name: "excludeDexes", value: excludeDexes.joined(separator: ","))]) }
        let ask = url
        // Nothing is signed yet, so this is asked again like the quote.
        let j = try await RPCRetry.run({ try await URLSession.shared.data(from: ask) }) { data, status -> [String: Any] in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], j["swapInstruction"] != nil else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            return j
        }
        func instruction(_ any: Any?) throws -> SolanaInstruction? {
            guard let o = any as? [String: Any] else { return nil }
            guard let program = o["programId"] as? String, let accounts = o["accounts"] as? [[String: Any]],
                  let data = (o["data"] as? String).flatMap({ Data(base64Encoded: $0) }) else { throw JupiterError(message: "an instruction Jupiter's /build sent can't be read") }
            return SolanaInstruction(program: program, accounts: try accounts.map {
                guard let key = $0["pubkey"] as? String else { throw JupiterError(message: "an account Jupiter's /build sent can't be read") }
                return .init(pubkey: key, isSigner: $0["isSigner"] as? Bool ?? false, isWritable: $0["isWritable"] as? Bool ?? false)
            }, data: data)
        }
        var all: [SolanaInstruction] = []
        for key in ["computeBudgetInstructions", "setupInstructions", "otherInstructions"] {
            for o in j[key] as? [Any] ?? [] { if let ix = try instruction(o) { all.append(ix) } }
        }
        guard let swap = try instruction(j["swapInstruction"]) else { throw JupiterError(message: "Jupiter's /build sent no swap") }
        all.append(swap)
        if let cleanup = try instruction(j["cleanupInstruction"]) { all.append(cleanup) }
        guard let meta = j["blockhashWithMetadata"] as? [String: Any], let hash = meta["blockhash"] as? [Int], hash.count == 32,
              let height = (meta["lastValidBlockHeight"] as? NSNumber)?.uint64Value,
              let outAmount = (j["outAmount"] as? String).flatMap(UInt64.init), let minOut = (j["otherAmountThreshold"] as? String).flatMap(UInt64.init) else {
            throw JupiterError(message: "Jupiter's /build answer is missing its blockhash or amounts")
        }
        let tables = (j["addressesByLookupTableAddress"] as? [String: [String]] ?? [:]).sorted { $0.key < $1.key }.map { (address: $0.key, entries: $0.value) }
        // The wallet's own token accounts for both coins, and both mints, stay in the message itself: the signer lets the
        // swap pay only an account the message names, and names the coins on its prompt from the message's own keys.
        let own = Set([quote.input, quote.output].flatMap { mint in [SPL.tokenProgram, SPL.token2022Program].compactMap { try? SPL.associatedAccount(owner: user, mint: mint, program: $0) } }
                      + [quote.input, quote.output])
        let dexes = (j["routePlan"] as? [[String: Any]] ?? []).compactMap { ($0["swapInfo"] as? [String: Any])?["label"] as? String }
        let message: Data
        do {
            message = try SolanaTransaction.compileV0(all, payer: user, blockhash: Base58.encode(Data(hash.map { UInt8(truncatingIfNeeded: $0) })),
                                                      tables: tables, keepStatic: own)
        } catch let e as WalletError where e.message.contains("over Solana's limit of 1232") { throw TooBig(dexes: dexes, message: e.message) }
        return Built(wire: SolanaTransaction.unsigned(message), lastValidBlockHeight: height, outAmount: outAmount, minOut: minOut,
                     route: dexes.joined(separator: " → "), dexes: dexes)
    }
}

/// Jupiter's gasless swap (Swap API v2, keyless on api.jup.ag): for a wallet with under 0.01 SOL, Jupiter's own wallet pays
/// the fee and the rent and takes them out of the output (`feeBps` grows). Its transaction comes with Jupiter's wallet as
/// the fee payer and ours as the second signer; it goes back through Jupiter's /execute, which adds its signature and sends.
extension Jupiter {
    static let v2 = "https://api.jup.ag/swap/v2"

    public struct Order: Sendable {
        public let requestId: String
        public let wire: Data
        public let inAmount: UInt64
        public let outAmount: UInt64
        /// The least the swap gives after slippage.
        public let minOut: UInt64
        /// Whoever pays the fee: Jupiter's wallet when `gasless`, else the taker.
        public let gasless: Bool
        public let feeBps: Int
        public let route: String
        /// The block its blockhash is good until (0 when Jupiter doesn't say).
        public let lastValidBlockHeight: UInt64
        /// As on a Quote: how far it moves the price, and what comes out in USD.
        public var impact: Decimal = 0
        public var usd: Decimal = 0
        /// Who fills it: «metis» (Jupiter's own routing) or «jupiterz» (a market maker, which pays the fee and signs first).
        public var router = ""
    }

    /// What /order gives for `amount` without a taker: its output, price impact and router — for the card, which compares
    /// it with /quote's (JupiterZ's market makers beat the pools on big swaps: 2M WIF $362k against $321k, 2026-10-28).
    public static func orderQuote(input: String, output: String, amount: UInt64) async throws -> (out: UInt64, impact: Decimal, router: String) {
        let url = URL(string: "\(v2)/order?inputMint=\(input)&outputMint=\(output)&amount=\(amount)")!
        return try await RPCRetry.run({ try await URLSession.shared.data(from: url) }) { data, status in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let out = (j["outAmount"] as? String).flatMap(UInt64.init) else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            return (out, impactAndWorth(j).impact, j["router"] as? String ?? "")
        }
    }

    /// The swap for `taker` as Jupiter builds it; nil transaction when it can't (too little, no route): its words.
    /// `excludeRouters`: routers Jupiter must leave out («jupiterz»: its market makers, through a program of their own).
    public static func order(input: String, output: String, amount: UInt64, taker: String, excludeRouters: [String] = []) async throws -> Order {
        let skip = excludeRouters.isEmpty ? "" : "&excludeRouters=" + excludeRouters.joined(separator: ",")
        let url = URL(string: "\(v2)/order?inputMint=\(input)&outputMint=\(output)&amount=\(amount)&taker=\(taker)\(skip)")!
        return try await RPCRetry.run({ try await URLSession.shared.data(from: url) }) { data, status in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            guard let requestId = j["requestId"] as? String, let b64 = j["transaction"] as? String, let wire = Data(base64Encoded: b64), !wire.isEmpty,
                  let inAmount = (j["inAmount"] as? String).flatMap(UInt64.init), let outAmount = (j["outAmount"] as? String).flatMap(UInt64.init),
                  let minOut = (j["otherAmountThreshold"] as? String).flatMap(UInt64.init) else {
                throw JupiterError(message: j["errorMessage"] as? String ?? j["error"] as? String ?? "no transaction for this swap")
            }
            let dexes = (j["routePlan"] as? [[String: Any]] ?? []).compactMap { ($0["swapInfo"] as? [String: Any])?["label"] as? String }
            let (impact, usd) = impactAndWorth(j)
            return Order(requestId: requestId, wire: wire, inAmount: inAmount, outAmount: outAmount, minOut: minOut,
                         gasless: j["gasless"] as? Bool ?? false, feeBps: (j["feeBps"] as? NSNumber)?.intValue ?? 0,
                         route: dexes.joined(separator: " → "),
                         lastValidBlockHeight: (j["lastValidBlockHeight"] as? NSNumber)?.uint64Value ?? UInt64(j["lastValidBlockHeight"] as? String ?? "") ?? 0,
                         impact: impact, usd: usd, router: j["router"] as? String ?? "")
        }
    }

    /// How long one /execute waits for its answer. Only the Dev copy's `-jupiterExecuteTimeout 0.02` sets it, for a live
    /// run of a swap whose answer doesn't come (Executor.settleSponsored).
    nonisolated(unsafe) public static var executeTimeout: TimeInterval = 60

    /// Hands our signed transaction to Jupiter, which signs as the fee payer and sends it. The same transaction and id may
    /// be sent again (Jupiter answers for the first one), so a dropped answer is asked again. Returns the signature.
    public static func execute(_ signed: Data, requestId: String) async throws -> String {
        var request = URLRequest(url: URL(string: "\(v2)/execute")!, timeoutInterval: executeTimeout)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["signedTransaction": signed.base64EncodedString(), "requestId": requestId])
        let ask = request
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            guard status == 200, let j, j["status"] as? String == "Success", let signature = j["signature"] as? String else {
                throw RPCRetry.Again(status == 429 || status >= 500,
                                     JupiterError(message: "\(j?["error"] as? String ?? RPCRetry.said(data, status: status)) (code \(j?["code"] ?? status))"))
            }
            return signature
        }
    }
}

public struct JupiterError: LocalizedError {
    public let message: String
    /// Jupiter's own words when it answers JSON («Each order valued at 30.00 USDC, minimum is 50.00 USDC»), else the
    /// body as text; a gateway's page is only its status (RPCRetry.said).
    init(message: String) { self.message = message }
    init(_ body: Data, _ status: Int = 0) {
        message = ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["error"] as? String ?? RPCRetry.said(body, status: status)
    }
    public var errorDescription: String? { "Jupiter: \(message)" }
}
