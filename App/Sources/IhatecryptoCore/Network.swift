import Foundation
import WalletCore
import LLMCore
import OneClickKit
import RelayBridge

func dec(_ s: String?) throws -> Decimal {
    guard let s, let d = Decimal(string: s) else { throw NetError(message: "Unexpected number format: \(s ?? "nil")") }
    return d
}

// MARK: - Hyperliquid (read-only `info`)

public enum Hyperliquid {
    static let info = "https://api.hyperliquid.xyz/info"

    /// An address Hyperliquid has never seen: its first deposit pays a one-time fee for opening the account.
    /// An answer without a role throws: read as «not new», a send to a new account lost its 1 USDC. Any role but «missing»
    /// (user, agent, vault, subAccount, one added later) is an account Hyperliquid knows.
    public static func isNewAccount(_ address: String) async throws -> Bool {
        let data = try await HTTP.post(info, json: ["type": "userRole", "user": address])
        guard let role = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["role"] as? String, !role.isEmpty else {
            throw NetError(message: "Hyperliquid's userRole answered without a role: \(String(decoding: data.prefix(200), as: UTF8.self))")
        }
        return role == "missing"
    }

    /// HIP-3 exchanges whose markets ihatecrypto shows: xyz carries the liquid commodities and stocks (oil, gold,
    /// Tesla…); the others had no volume on 2026-09-25.
    public static let hip3Dexes = ["xyz"]

    /// All live perp markets keyed by name: the main exchange ("ETH") and HIP-3 ones ("xyz:CL").
    public static func markets() async throws -> [String: Market] {
        async let main = markets(dex: nil, dexIndex: 0)
        async let order = try? HTTP.post(info, json: ["type": "perpDexs"])
        var out = try await main
        // Asset ids of HIP-3 markets depend on the exchange's position in perpDexs.
        let dexes = ((await order).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [Any] } ?? [])
            .map { ($0 as? [String: Any])?["name"] as? String }
        for dex in hip3Dexes {
            guard let i = dexes.firstIndex(of: dex), let extra = try? await markets(dex: dex, dexIndex: i) else { continue }
            out.merge(extra) { a, _ in a }
        }
        return out
    }

    static func markets(dex: String?, dexIndex: Int) async throws -> [String: Market] {
        struct Asset: Decodable { let name: String; let szDecimals: Int; let maxLeverage: Int; let isDelisted: Bool? }
        struct Meta: Decodable { let universe: [Asset] }
        struct Ctx: Decodable { let markPx: String?; let prevDayPx: String?; let funding: String?; let dayNtlVlm: String? }
        // The response is a two-element array: [meta, contexts].
        struct Response: Decodable {
            let meta: Meta, ctxs: [Ctx]
            init(from decoder: Decoder) throws {
                var c = try decoder.unkeyedContainer()
                meta = try c.decode(Meta.self)
                ctxs = try c.decode([Ctx].self)
            }
        }
        var body: [String: Any] = ["type": "metaAndAssetCtxs"]
        if let dex { body["dex"] = dex }
        let r = try JSONDecoder().decode(Response.self, from: try await HTTP.post(info, json: body))
        let (meta, ctxs) = (r.meta, r.ctxs)
        var out: [String: Market] = [:]
        for (index, (asset, ctx)) in zip(meta.universe, ctxs).enumerated() where asset.isDelisted != true {
            guard let px = ctx.markPx.flatMap({ Decimal(string: $0) }) else { continue }
            out[asset.name] = Market(name: asset.name, markPx: px, szDecimals: asset.szDecimals, maxLeverage: asset.maxLeverage,
                                     index: dex == nil ? index : 100_000 + dexIndex * 10_000 + index,
                                     prevDayPx: ctx.prevDayPx.flatMap { Decimal(string: $0) }, funding: ctx.funding.flatMap { Decimal(string: $0) }, dex: dex)
            out[asset.name]?.volume24h = ctx.dayNtlVlm.flatMap { Decimal(string: $0) } ?? 0
        }
        return out
    }

    /// ATR(14) of 4-hour candles as % of the last close: the market's ordinary move, for stops.
    public static func atrPct(coin: String, now: Date = Date()) async throws -> Decimal {
        struct Candle: Decodable { let h: String; let l: String; let c: String }
        let end = Int(now.timeIntervalSince1970 * 1000)
        let body: [String: Any] = ["type": "candleSnapshot", "req": ["coin": coin, "interval": "4h", "startTime": end - 16 * 4 * 3_600_000, "endTime": end]]
        let candles = try JSONDecoder().decode([Candle].self, from: try await HTTP.post(info, json: body))
        let bars = try candles.map { (h: try dec($0.h), l: try dec($0.l), c: try dec($0.c)) }
        guard bars.count >= 15 else { throw NetError(message: "Not enough \(coin) candles for ATR.") }
        let ranges = (1..<bars.count).map { i in max(bars[i].h - bars[i].l, abs(bars[i].h - bars[i - 1].c), abs(bars[i].l - bars[i - 1].c)) }
        let atr = ranges.suffix(14).reduce(0, +) / 14
        return atr / bars[bars.count - 1].c * 100
    }

    public struct Balance: Sendable, Equatable {
        public let accountValue: Decimal
        public let withdrawable: Decimal
        public let spotUSDC: Decimal
        /// A unified account or portfolio margin: one USDC balance on spot carries perps, HIP-3 exchanges and spot.
        /// accountValue and withdrawable are read from it, spotUSDC is 0 (the same money), nothing moves between them.
        public var unified = false
    }

    /// The account's abstraction mode (userAbstraction): «unifiedAccount» and «portfolioMargin» keep every balance and
    /// hold in the spot state, where the perp and HIP-3 exchange states mean nothing (account-abstraction-modes doc).
    /// «disabled» (Manual) and «default» keep perp and spot apart. A reply that isn't a mode reads as apart, as before.
    public static func unified(user: String) async throws -> Bool {
        let mode = try JSONSerialization.jsonObject(with: try await HTTP.post(info, json: ["type": "userAbstraction", "user": user]),
                                                    options: .fragmentsAllowed) as? String
        return mode == "unifiedAccount" || mode == "portfolioMargin"
    }

    /// A HIP-3 exchange's own margin balance: account value and what is free. Nothing in a unified account, whose money
    /// is all on the one balance `balance(user:)` reads.
    public static func dexBalance(user: String, dex: String) async throws -> (accountValue: Decimal, withdrawable: Decimal) {
        struct Summary: Decodable { let accountValue: String }
        struct Perp: Decodable { let marginSummary: Summary; let withdrawable: String }
        async let one = unified(user: user)
        let perp = try JSONDecoder().decode(Perp.self, from: try await HTTP.post(info, json: ["type": "clearinghouseState", "user": user, "dex": dex]))
        if try await one { return (0, 0) }
        return (try dec(perp.marginSummary.accountValue), try dec(perp.withdrawable))
    }

    /// Perp and spot are separate balances on HyperCore; they are shown apart, never summed. In a unified account the
    /// spot USDC is the whole account (positions' profit and loss included) and what open positions and orders hold
    /// of it is its `hold`.
    // ponytail: portfolio margin's other collateral (HYPE, BTC) isn't counted here, and a USDC loan reads as 0; the
    // spot tokens show in the portfolio on their own.
    public static func balance(user: String) async throws -> Balance {
        struct Summary: Decodable { let accountValue: String }
        struct Perp: Decodable { let marginSummary: Summary; let withdrawable: String }
        struct SpotRow: Decodable { let coin: String; let total: String; let hold: String? }
        struct Spot: Decodable { let balances: [SpotRow] }
        async let perpData = HTTP.post(info, json: ["type": "clearinghouseState", "user": user])
        async let spotData = HTTP.post(info, json: ["type": "spotClearinghouseState", "user": user])
        async let one = unified(user: user)
        let perp = try JSONDecoder().decode(Perp.self, from: try await perpData)
        let spot = try JSONDecoder().decode(Spot.self, from: try await spotData)
        let usdc = spot.balances.first { $0.coin == "USDC" }
        if try await one {
            let total = max(0, try dec(usdc?.total ?? "0"))
            return Balance(accountValue: total, withdrawable: max(0, total - (try dec(usdc?.hold ?? "0"))), spotUSDC: 0, unified: true)
        }
        return Balance(
            accountValue: try dec(perp.marginSummary.accountValue),
            withdrawable: try dec(perp.withdrawable),
            spotUSDC: try dec(usdc?.total ?? "0")
        )
    }
}

// MARK: - Solana (public RPC)

public enum Solana {
    /// The user's own node when set in Settings, else the public one (SolanaNodes).
    static var rpc: String { SolanaNodes.urls[0].absoluteString }
    static let usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

    public struct Balance: Sendable, Equatable {
        public let sol: Decimal
        public let usdc: Decimal
        /// Every other token in the wallet, the most valuable first; scam airdrops included (see Holding.looksReal).
        public var tokens: [Holding] = []
        /// Jupiter didn't answer: the tokens came by mint, without names or prices, and their money isn't counted.
        public var unpriced = false

        /// Unpriced, the tokens take the last read's names and prices at their new amounts: read as they came, the
        /// automation wallet's 25 USDT dropped out of the balance until the next read. Still unpriced when one is new.
        public func pricing(from last: [Holding]) -> Balance {
            guard unpriced else { return self }
            var b = self
            b.tokens = tokens.map { t in last.first { $0.contract == t.contract }?.at(t.amount) ?? t }
            b.unpriced = tokens.contains { t in !last.contains { $0.contract == t.contract } }
            return b
        }
    }

    /// `lendShares`: Jupiter Lend's share tokens (jlUSDC…) among the tokens too, for a transfer of them as they are.
    public static func balance(owner: String, lendShares: Bool = false) async throws -> Balance {
        // The public node throttles getTokenAccountsByOwner («Too many requests for a specific RPC call», HTTP 429) when a
        // chain's step and its transfer read the wallet a moment apart: the USDT a swap brought then failed to send. Asked
        // again 1, 2, 4 and 8 s later, as RPCRetry asks other nodes.
        for delay in RPCRetry.delays {
            do { return try await read(owner: owner, lendShares: lendShares) } catch let e as NetError where RPCRetry.transient(e.message) {
                try await Task.sleep(for: delay)
            }
        }
        return try await read(owner: owner, lendShares: lendShares)
    }

    private static func read(owner: String, lendShares: Bool) async throws -> Balance {
        struct RPCError: Decodable { let message: String }
        struct Lamports: Decodable { struct R: Decodable { let value: UInt64 }; let result: R?; let error: RPCError? }
        struct Tokens: Decodable {
            struct Amount: Decodable { let amount: String; let decimals: Int }
            struct Info: Decodable { let mint: String; let tokenAmount: Amount }
            struct Parsed: Decodable { let info: Info }
            struct DataField: Decodable { let parsed: Parsed }
            struct Account: Decodable { let data: DataField }
            struct Item: Decodable { let account: Account }
            struct R: Decodable { let value: [Item] }
            let result: R?
            let error: RPCError?
        }
        // Explicitly «finalized»: since Alpenglow (2026-10-21) it is the same slot as «confirmed», which is to be removed.
        // Before, finalized ran ~13 s behind: a bridge's 133.94 USDC had landed, and the chain's next step («lend 150»)
        // still read the 17.77 from before and stopped.
        async let lamportsData = HTTP.post(rpc, json: ["jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [owner, ["commitment": "finalized"]]])
        // Both token programs: meme coins (POKEMON…) are Token-2022 and don't show under the classic one.
        func accounts(_ program: String) -> [String: Any] {
            ["jsonrpc": "2.0", "id": 2, "method": "getTokenAccountsByOwner",
             "params": [owner, ["programId": program], ["encoding": "jsonParsed", "commitment": "finalized"]]]
        }
        async let classicData = HTTP.post(rpc, json: accounts("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"))
        async let token2022Data = HTTP.post(rpc, json: accounts("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"))
        let lamports = try JSONDecoder().decode(Lamports.self, from: try await lamportsData)
        let classic = try JSONDecoder().decode(Tokens.self, from: try await classicData)
        let token2022 = try JSONDecoder().decode(Tokens.self, from: try await token2022Data)
        guard let l = lamports.result, let c = classic.result, let t = token2022.result else {
            throw NetError(message: "Solana RPC: \(lamports.error?.message ?? classic.error?.message ?? token2022.error?.message ?? "empty response")")
        }
        var usdc = Decimal()
        var others: [String: Decimal] = [:]
        for item in c.value + t.value {
            let info = item.account.data.parsed.info
            let amount = try dec(info.tokenAmount.amount) / pow(10, info.tokenAmount.decimals)
            if info.mint == usdcMint { usdc += amount } else if amount > 0 { others[info.mint, default: 0] += amount }
        }
        let held = await holdings(others, lendShares: lendShares)
        return Balance(sol: Decimal(l.value) / 1_000_000_000, usdc: usdc, tokens: held.tokens, unpriced: !held.priced)
    }

    /// Names, icons and prices from Jupiter, 100 mints a request (its search's limit). Jupiter Lend's share tokens are
    /// left out: the Jupiter Lend row counts them. Without Jupiter the tokens still show, by mint and unpriced. A throttled
    /// Jupiter is waited out (TokenResearch.get): three tries in 2 s never outlasted its 10-second window.
    public static func holdings(_ amounts: [String: Decimal], lendShares: Bool = false) async -> (tokens: [Holding], priced: Bool) {
        let mints = amounts.keys.sorted()
        var pages: [Data] = [], priced = true
        for start in stride(from: 0, to: mints.count, by: 100) {
            let chunk = mints[start..<min(start + 100, mints.count)]
            if let data = try? await TokenResearch.get(TokenResearch.jupiterSearch + chunk.joined(separator: ",")) {
                pages.append(data)
            } else {
                priced = false
            }
        }
        return (holdings(amounts, jupiter: pages, lendShares: lendShares), priced)
    }

    static func holdings(_ amounts: [String: Decimal], jupiter pages: [Data], lendShares: Bool = false) -> [Holding] {
        let found = pages.flatMap { Holdings.json($0) as? [[String: Any]] ?? [] }
        let byMint = Dictionary(found.compactMap { t in (t["id"] as? String).map { ($0, t) } }) { a, _ in a }
        return amounts.compactMap { mint, amount -> Holding? in
            let t = byMint[mint] ?? [:]
            let tags = t["tags"] as? [String] ?? []
            if tags.contains("jup-lend-earn"), !lendShares { return nil }
            return Holding(chain: "solana", contract: mint, amount: amount, symbol: t["symbol"] as? String ?? String(mint.prefix(4)) + "…",
                           icon: (t["icon"] as? String).flatMap(URL.init(string:)), price: Holdings.number(t["usdPrice"]),
                           verified: t["isVerified"] as? Bool == true || tags.contains("verified"), liquidity: Holdings.number(t["liquidity"]),
                           change24h: Holdings.number((t["stats24h"] as? [String: Any])?["priceChange"]))
        }
        .sorted { ($0.usd ?? 0, $0.symbol) > ($1.usd ?? 0, $1.symbol) }
    }
}

/// The NEAR chain: native NEAR and wNEAR on an account. 1Click delivers «wrap.near», so both are counted.
public enum Near {
    static let rpc = "https://rpc.mainnet.fastnear.com"

    public static func balance(account: String) async throws -> (near: Decimal, wrapped: Decimal) {
        func query(_ params: [String: Any]) async throws -> [String: Any]? {
            let data = try await HTTP.post(rpc, json: ["jsonrpc": "2.0", "id": 1, "method": "query", "params": params])
            let j = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            if let e = j?["error"] as? [String: Any] {
                // An implicit account exists only after NEAR first arrives; before that it simply holds nothing.
                if "\(e)".contains("UNKNOWN_ACCOUNT") || "\(e)".contains("does not exist") { return nil }
                throw NetError(message: "NEAR RPC: \(e["message"] ?? e)")
            }
            return j?["result"] as? [String: Any]
        }
        let yocto = pow(Decimal(10), 24)
        let view = try await query(["request_type": "view_account", "finality": "final", "account_id": account])
        let native = (view?["amount"] as? String).flatMap { Decimal(string: $0) } ?? 0
        let args = try JSONSerialization.data(withJSONObject: ["account_id": account]).base64EncodedString()
        let call = try await query(["request_type": "call_function", "finality": "final", "account_id": "wrap.near",
                                    "method_name": "ft_balance_of", "args_base64": args])
        // The view returns the JSON string «"123"» as bytes.
        let text = String(decoding: (call?["result"] as? [Int] ?? []).map { UInt8($0) }, as: UTF8.self)
        let wrapped = Decimal(string: text.trimmingCharacters(in: CharacterSet(charactersIn: "\""))) ?? 0
        return (native / yocto, wrapped / yocto)
    }

    /// What a new transaction from `account` needs: the next nonce of its key, a recent final block hash, and the
    /// account's native balance and storage (yoctoNEAR, bytes). The key is the implicit account's own.
    public static func txContext(account: String) async throws -> (nonce: UInt64, blockHash: Data, balance: String, storageBytes: UInt64, gasPrice: String) {
        func call(_ method: String, _ params: [String: Any]) async throws -> [String: Any] {
            let data = try await HTTP.post(rpc, json: ["jsonrpc": "2.0", "id": 1, "method": method, "params": params])
            let j = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            guard let result = j?["result"] as? [String: Any] else {
                let error = "\(j?["error"] ?? "no result")"
                // No NEAR ever came: the account isn't there to sign from.
                if error.contains("UNKNOWN_ACCOUNT") || error.contains("does not exist while viewing") {
                    throw NetError(message: "This NEAR account doesn't exist yet: NEAR opens it with the first NEAR it receives. Send it ≈ 0.1 NEAR, then try again.")
                }
                throw NetError(message: "NEAR RPC \(method): \(error)")
            }
            return result
        }
        guard let raw = Data(hex: account), raw.count == 32 else { throw NetError(message: "Not a NEAR implicit account: \(account).") }
        let publicKey = "ed25519:" + Base58.encode(raw)
        let key = try await call("query", ["request_type": "view_access_key", "finality": "final", "account_id": account, "public_key": publicKey])
        let view = try await call("query", ["request_type": "view_account", "finality": "final", "account_id": account])
        let block = try await call("block", ["finality": "final"])
        guard let nonce = (key["nonce"] as? NSNumber)?.uint64Value, let hash = (block["header"] as? [String: Any])?["hash"] as? String,
              let balance = view["amount"] as? String, let storage = (view["storage_usage"] as? NSNumber)?.uint64Value else {
            throw NetError(message: "NEAR RPC: no nonce, block hash or balance.")
        }
        let gasPrice = (block["header"] as? [String: Any])?["gas_price"] as? String ?? "100000000"
        return (nonce + 1, try Base58.decode(hash), balance, storage, gasPrice)
    }

    /// NEAR's «NotEnoughBalance» (the fee) or «LackBalanceForState» (what the account must keep for its storage) in words,
    /// with what to do; nil for anything else.
    static func shortOfNEAR(_ error: String) -> String? {
        guard error.contains("NotEnoughBalance") || error.contains("LackBalanceForState") else { return nil }
        return "Not enough NEAR: the account keeps NEAR for its storage and pays the fee in NEAR. Send it ≈ 0.1 NEAR, then try again."
    }

    /// Broadcasts a borsh SignedTransaction and waits until it has executed; throws with NEAR's reason on failure.
    /// Returns the transaction hash (base58).
    public static func send(_ signed: Data) async throws -> String {
        let data = try await HTTP.post(rpc, json: ["jsonrpc": "2.0", "id": 1, "method": "send_tx",
                                                   "params": ["signed_tx_base64": signed.base64EncodedString(), "wait_until": "EXECUTED_OPTIMISTIC"]])
        let j = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let result = j?["result"] as? [String: Any] else {
            let error = "\(j?["error"] ?? "no result")"
            throw NetError(message: shortOfNEAR(error) ?? "NEAR RPC send_tx: \(error)")
        }
        if let failure = (result["status"] as? [String: Any])?["Failure"] {
            throw NetError(message: shortOfNEAR("\(failure)") ?? "The NEAR transaction failed: \(failure)")
        }
        return (result["transaction"] as? [String: Any])?["hash"] as? String ?? "?"
    }
}

// MARK: - NEAR Intents 1Click (dry quotes for the plan; execution lives in OneClickKit)

public enum Bridge {
    // Dry quotes do not depend on addresses, so the user's own addresses are not sent.
    static let placeholderRefund = "So11111111111111111111111111111111111111112"
    public static let placeholderRecipient = "0x0000000000000000000000000000000000000001"

    /// `arrive` = exact USDC to land on HyperCore; `send` = exact USDC to spend from Solana.
    public enum Amount: Sendable { case arrive(Decimal), send(Decimal) }

    /// Both bridges in parallel. The app bridges trades through Relay, so Relay's price is the plan's (the signature
    /// asks for it); 1Click is shown for comparison and stands in only when Relay doesn't answer. `newAccount`: the
    /// user's Hyperliquid account doesn't exist yet, so the quote goes to a fresh random address and carries the same
    /// one-time opening fee the real deposit will. `origin`: the USDC leaves an EVM network instead (chain, token) —
    /// Relay only, straight to Hyperliquid. `oneClickFirst`: the user chose to open a new account through 1Click.
    public static func dryQuote(_ amount: Amount, newAccount: Bool = false, from origin: (chain: UInt64, usdc: String, decimals: Int)? = nil,
                                oneClickFirst: Bool = false) async throws -> BridgeQuote {
        let recipient = newAccount ? "0x" + (0..<20).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined() : placeholderRecipient
        async let relay = try? relayQuote(amount, recipient: recipient, from: origin)
        async let oneClick = origin == nil ? try? oneClickQuote(amount, recipient: recipient) : nil
        let options = (oneClickFirst ? [await oneClick, await relay] : [await relay, await oneClick]).compactMap { $0 }
        guard var best = options.first else { throw NetError(message: "No bridge quotes: Relay and 1Click didn't respond.") }
        if let other = options.dropFirst().first {
            best.alternative = "\(other.provider): send \(Fmt.num(other.amountIn)) → receive \(Fmt.num(other.amountOut)), \(other.time.lowercased())"
            best.alternativeFee = other.amountIn - other.reserve - other.amountOut
        }
        best.opening = newAccount
        return best
    }

    static func relayQuote(_ amount: Amount, recipient: String = placeholderRecipient, from origin: (chain: UInt64, usdc: String, decimals: Int)? = nil) async throws -> BridgeQuote {
        let (exactOutput, units): (Bool, Decimal) = switch amount {
        case .arrive(let v): (true, v * 100_000_000) // Hyperliquid perp USDC in Relay: 8 decimals
        case .send(let v): (false, v * pow(10, origin?.decimals ?? 6))   // USDC on Solana and the EVM networks: 6, BNB Chain's 18
        }
        let q = try await Relay.quote(user: origin == nil ? placeholderRefund : placeholderRecipient, recipient: recipient,
                                      originChain: origin?.chain ?? Relay.Chain.solana, destinationChain: Relay.Chain.hyperliquid,
                                      originCurrency: origin?.usdc ?? Relay.Currency.solanaUSDC, destinationCurrency: Relay.Currency.hyperliquidUSDC,
                                      amount: 0, exactOutput: exactOutput, units: "\(units.rounded(0, .up))")
        // Measured 2026-09-25: 4 s from send to credit on Hyperliquid from Solana; from an EVM network, Relay's estimate.
        return BridgeQuote(provider: "Relay", amountIn: try dec(q.amountInFormatted), reserve: 0, amountOut: try dec(q.amountOutFormatted),
                           time: origin == nil ? "Seconds: 4 s to credit in live runs." : "Seconds: ≈ \(max(q.seconds, 1)) s by Relay's estimate.")
    }

    public static func oneClickQuote(_ amount: Amount, recipient: String = placeholderRecipient) async throws -> BridgeQuote {
        let (exactOutput, units): (Bool, Decimal) = switch amount {
        case .arrive(let v): (true, v * 100_000_000) // HyperCore USDC has 8 decimals
        case .send(let v): (false, v * 1_000_000)    // Solana USDC has 6 decimals
        }
        // EXACT_OUTPUT adds the slippage tolerance to amountIn; 0.1% is enough for USDC → USDC.
        let q = try await OneClick.quote(
            dry: true, exactOutput: exactOutput, origin: OneClick.Asset.solanaUSDC, destination: OneClick.Asset.hyperCoreUSDC,
            amount: "\(units.rounded(0, .up))", refundTo: placeholderRefund, recipient: recipient, slippageBps: 10
        )
        let amountIn = try dec(q.amountInFormatted)
        let priceIn = try q.minAmountIn.map { try dec($0) / 1_000_000 } ?? amountIn
        // 1Click estimates ~30 s; live runs took 114–237 s from send to credit.
        return BridgeQuote(provider: "NEAR 1Click", amountIn: amountIn, reserve: max(0, amountIn - priceIn),
                           amountOut: try dec(q.amountOutFormatted), time: "Usually 2–4 min.")
    }
}
