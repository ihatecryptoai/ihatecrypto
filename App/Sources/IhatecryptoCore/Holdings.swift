import Foundation
import LLMCore
import RelayBridge
import WalletCore

/// A token the wallet holds, on any network, beyond the balances the app reads one by one (USDC, lending positions,
/// stock tokens…). Found by listing the address on each network; priced where a market knows it.
public struct Holding: Sendable, Identifiable, Equatable {
    /// «solana», «base», «arbitrum», «ethereum», «robinhood», «hyperliquid» (spot) or «near».
    public let chain: String
    /// The mint, the contract, the spot token's name or the NEAR contract; «native» for the network's own coin.
    public let contract: String
    public let amount: Decimal
    public var symbol: String
    public var icon: URL?
    public var price: Decimal?
    /// A market list vouches for it: Jupiter's check, a CoinGecko rate on Blockscout, Ref's price list.
    public var verified = false
    /// The pool behind the price (or, on Hyperliquid's spot book, the day's volume).
    public var liquidity: Decimal?
    /// The network's own coin (ETH on Arbitrum and Ethereum): gas, in the balance but not in the risk rules' capital.
    public var native = false
    /// How far the price moved in 24 hours, in %, where the market that priced it says (Jupiter, DexScreener, Hyperliquid's book).
    public var change24h: Decimal? = nil

    /// The key the user's show/hide choice is kept under: the mint on Solana, «chain:contract» elsewhere.
    public var id: String { chain == "solana" ? contract : "\(chain):\(contract.lowercased())" }

    /// The same token at another amount.
    public func at(_ amount: Decimal) -> Holding {
        Holding(chain: chain, contract: contract, amount: amount, symbol: symbol, icon: icon, price: price, verified: verified,
                liquidity: liquidity, native: native, change24h: change24h)
    }

    public init(chain: String, contract: String, amount: Decimal, symbol: String, icon: URL? = nil, price: Decimal? = nil,
                verified: Bool = false, liquidity: Decimal? = nil, native: Bool = false, change24h: Decimal? = nil) {
        (self.chain, self.contract, self.amount, self.symbol, self.icon, self.price) = (chain, contract, amount, symbol, icon, price)
        (self.verified, self.liquidity, self.native, self.change24h) = (verified, liquidity, native, change24h)
    }
    public var usd: Decimal? { price.map { $0 * amount } }
    /// Airdropped look-alikes are unverified with no real pool behind them: they stay out of sight until the user
    /// asks. A token the user bought has a pool (or a market list's check) and shows.
    public var looksReal: Bool { verified || ((liquidity ?? 0) >= 10_000 && (usd ?? 0) >= 0.01) }
    /// Marked «unverified»: no list vouches for it. Never on Hyperliquid's spot book, where a name belongs to one token,
    /// so nothing there can be a look-alike.
    public var unverified: Bool { !verified && chain != "hyperliquid" }
}

public enum Holdings {
    /// Blockscout turns away clients whose User-Agent doesn't start like a browser's (403 on Base, Arbitrum, Polygon and
    /// Robinhood Chain, 2026-10-23); a crawler's «Mozilla/5.0 (compatible; …)» passes and still says who is asking.
    public static let browser = ["User-Agent": "Mozilla/5.0 (compatible; ihatecrypto; +https://ihatecrypto.ai)"]
    public static let blockscout = ["base": "base.blockscout.com", "arbitrum": "arbitrum.blockscout.com", "polygon": "polygon.blockscout.com",
                             "ethereum": "eth.blockscout.com", "robinhood": "robinhoodchain.blockscout.com", "optimism": "explorer.optimism.io"]
    static let ethIcon = URL(string: "https://assets.coingecko.com/coins/images/279/small/ethereum.png")

    // MARK: EVM networks

    /// Every ERC-20 at the address on one network, and its ETH when `native` (Base's and Robinhood's ETH is already
    /// counted as gas). `skip`: contracts the app counts elsewhere (USDC, Aave's aUSDC, the Morpho vault…), lowercased.
    /// nil when the network couldn't be read in full: the caller keeps what it had instead of showing a shorter list.
    public static func evm(_ chain: String, address: String, native: Bool, skip: Set<String>, extra: [String] = []) async -> [Holding]? {
        if chain == "tempo" { return await tempo(address) }
        if chain == "hyperevm" { return await hyperevm(address) }
        if chain == "bnb" { return await bnb(address, extra: extra) }
        guard let host = blockscout[chain] else { return [] }
        async let pages = tokenPages(host, address: address)
        async let account: [String: Any]?? = native
            ? .some(await retrying { json(try await HTTP.get("https://\(host)/api/v2/addresses/\(address)", headers: browser)) as? [String: Any] })
            : .none
        guard let items = await pages else { return nil }
        var out = evmTokens(chain, items, skip: skip)
        switch await account {
        case .some(nil): return nil
        case .some(let a?): if let eth = nativeCoin(chain, a) { out.append(eth) }
        case .none: break
        }
        // DexScreener gives every token its day's move, and the ones CoinGecko doesn't list their price and pool, so the
        // scam check has something to go on. It takes 30 contracts a call; the priced ones go first.
        // ponytail: 20 calls at most (600 tokens); past that the rest stays listed under «hidden», unpriced.
        let asked = (out.filter { $0.price != nil } + out.filter { $0.price == nil }).filter { !$0.native }.map(\.contract)
        for start in stride(from: 0, to: min(asked.count, 600), by: 30) {
            let chunk = asked[start..<min(start + 30, asked.count)]
            out = dexPrices(out, try? await HTTP.get("https://api.dexscreener.com/tokens/v1/\(chain)/" + chunk.joined(separator: ",")))
        }
        return await llamaPriced(chain, out)
    }

    /// DefiLlama's names for the networks it prices; Robinhood Chain and Tempo it doesn't.
    public static let llamaChain = ["base": "base", "arbitrum": "arbitrum", "ethereum": "ethereum", "polygon": "polygon", "optimism": "optimism",
                                    "bnb": "bsc", "hyperevm": "hyperliquid"]

    /// Dollar prices DefiLlama gives these contracts (lowercased keys), and whether it's sure (confidence ≥ 0.9). It prices
    /// a vault's or a lending market's shares by what's under them — Fluid's fUSDC, Compound's cUSDCv3, Moonwell's mUSDC,
    /// Spark's sUSDC — which no market list or pool does.
    public static func llamaPrices(_ chain: String, _ contracts: [String]) async -> [String: (price: Decimal, sure: Bool)] {
        guard let name = llamaChain[chain], !contracts.isEmpty else { return [:] }
        var out: [String: (price: Decimal, sure: Bool)] = [:]
        for start in stride(from: 0, to: min(contracts.count, 200), by: 50) {
            let chunk = contracts[start..<min(start + 50, contracts.count)]
            guard let coins = (json(try? await HTTP.get("https://coins.llama.fi/prices/current/" + chunk.map { "\(name):\($0)" }.joined(separator: ","), timeout: 15))
                    as? [String: Any])?["coins"] as? [String: [String: Any]] else { continue }
            for (key, c) in coins {
                guard let p = number(c["price"]) else { continue }
                let confidence = (c["confidence"] as? NSNumber)?.doubleValue ?? 0
                guard confidence >= 0.5 else { continue }
                out[String(key.split(separator: ":").last ?? "").lowercased()] = (p, confidence >= 0.9)
            }
        }
        return out
    }

    /// The tokens no list or pool priced, priced by DefiLlama; a sure price makes a position real (not an airdrop's
    /// look-alike), so a deposit in any vault shows with its worth.
    public static func llamaPriced(_ chain: String, _ tokens: [Holding]) async -> [Holding] {
        let unpriced = tokens.filter { $0.price == nil && !$0.native }.map(\.contract)
        let prices = await llamaPrices(chain, unpriced)
        guard !prices.isEmpty else { return tokens }
        return tokens.map { t in
            guard t.price == nil, let p = prices[t.contract.lowercased()] else { return t }
            var priced = t
            priced.price = p.price
            if p.sure { priced.verified = true }
            return priced
        }
    }

    /// The wallet's changes in one token on an EVM network (CostBasis.replay); nil off Blockscout's networks or when it
    /// didn't answer. Any token standard: Robinhood's stock tokens are ERC-8056, not ERC-20.
    // ponytail: the 50 newest transfers, one transaction at a time; what was held before them counts at the price then.
    public static func tokenChanges(_ chain: String, address: String, contract: String) async -> [CostBasis.Change]? {
        guard let host = blockscout[chain],
              let list = await retrying({ json(try await HTTP.get("https://\(host)/api/v2/addresses/\(address)/token-transfers?token=\(contract)", headers: browser)) as? [String: Any] }),
              let items = list["items"] as? [[String: Any]] else { return nil }
        var hashes: [String] = []
        for item in items.reversed() { if let h = item["transaction_hash"] as? String, !hashes.contains(h) { hashes.append(h) } }
        var transactions: [[[String: Any]]] = []
        for h in hashes {
            guard let tx = await retrying({ json(try await HTTP.get("https://\(host)/api/v2/transactions/\(h)/token-transfers", headers: browser)) as? [String: Any] }),
                  let transfers = tx["items"] as? [[String: Any]] else { return nil }
            transactions.append(transfers)
        }
        return CostBasis.replay(wallet: address, contract: contract, transactions: transactions)
    }

    /// The wallet's changes in its network's own coin (ETH, POL), from Blockscout's coin balance history, oldest first:
    /// coin in with stablecoins leaving the wallet in the same transaction was bought for them (a swap for ETH); gas and
    /// sends went out. nil off Blockscout's networks or when it didn't answer.
    // ponytail: the 50 newest balance changes; what was held before them counts at the price then.
    public static func coinChanges(_ chain: String, address: String) async -> [CostBasis.Change]? {
        guard let host = blockscout[chain],
              let list = await retrying({ json(try await HTTP.get("https://\(host)/api/v2/addresses/\(address)/coin-balance-history", headers: browser)) as? [String: Any] }),
              let items = list["items"] as? [[String: Any]] else { return nil }
        var out: [CostBasis.Change] = []
        for item in items.reversed() {
            guard let wei = number(item["delta"]), wei != 0 else { continue }
            let at = (item["block_timestamp"] as? String).flatMap { try? Date($0, strategy: .iso8601) } ?? Date()
            var paid: Decimal?
            if wei > 0, let hash = item["transaction_hash"] as? String,
               let tx = await retrying({ json(try await HTTP.get("https://\(host)/api/v2/transactions/\(hash)/token-transfers", headers: browser)) as? [String: Any] }) {
                let dollars = CostBasis.dollarsOut(tx["items"] as? [[String: Any]] ?? [], wallet: address)
                paid = dollars > 0 ? dollars : nil
            }
            out.append(CostBasis.Change(at: at, tokens: wei / pow(10, 18), paid: paid))
        }
        return out
    }

    /// The NEAR account's changes in each of its tokens, by contract, from NearBlocks' token transfers (the 50 newest),
    /// oldest first: a token in with NEAR's stablecoins leaving in the same transaction was bought for them. nil when it
    /// didn't answer.
    public static func nearTokenChanges(account: String) async -> [String: [CostBasis.Change]]? {
        guard let list = await retrying({ json(try await HTTP.get("https://api.nearblocks.io/v1/account/\(account)/ft-txns?per_page=50")) as? [String: Any] }),
              let txns = list["txns"] as? [[String: Any]] else { return nil }
        let stables = Set((Stablecoins.byNetwork["near"] ?? []).map(\.address))
        let moved: [(hash: String, contract: String, at: Date, amount: Decimal)] = txns.compactMap { t in
            guard t["affected_account_id"] as? String == account, let ft = t["ft"] as? [String: Any], let contract = ft["contract"] as? String,
                  let raw = number(t["delta_amount"]), let decimals = (ft["decimals"] as? NSNumber)?.intValue, let ns = number(t["block_timestamp"]) else { return nil }
            return (t["transaction_hash"] as? String ?? "", contract, Date(timeIntervalSince1970: NSDecimalNumber(decimal: ns / 1_000_000_000).doubleValue), raw / pow(10, decimals))
        }
        var out: [String: [CostBasis.Change]] = [:]
        for m in moved.reversed() where !stables.contains(m.contract) && m.amount != 0 {
            let dollars = -moved.filter { $0.hash == m.hash && stables.contains($0.contract) && $0.amount < 0 }.reduce(0) { $0 + $1.amount }
            out[m.contract, default: []].append(CostBasis.Change(at: m.at, tokens: m.amount, paid: m.amount > 0 && dollars > 0 ? dollars : nil))
        }
        return out
    }

    /// What Hyperliquid itself says each spot token held cost (its `entryNtl`), by the token's name; nil when it
    /// couldn't be read.
    public static func hyperliquidEntries(user: String) async -> [String: CostBasis]? {
        guard let state = await retrying({ try await HTTP.post("https://api.hyperliquid.xyz/info", json: ["type": "spotClearinghouseState", "user": user]) }) else { return nil }
        var out: [String: CostBasis] = [:]
        for b in (json(state) as? [String: Any])?["balances"] as? [[String: Any]] ?? [] {
            guard let coin = b["coin"] as? String, let total = number(b["total"]), let entry = number(b["entryNtl"]) else { continue }
            var basis = CostBasis()
            basis.bought(total, for: entry)
            basis.since = nil   // Hyperliquid doesn't say when
            out[coin] = basis
        }
        return out
    }

    /// DefiLlama's name for a coin, for its past prices: a network's own coin by its CoinGecko id, a token by
    /// «network:address»; nil where it has none (Robinhood Chain, Tempo, Hyperliquid's spot book).
    public static func llamaID(chain: String, contract: String) -> String? {
        switch (chain, contract) {
        case ("solana", CostBasis.wsol): "coingecko:solana"
        case ("solana", _): "solana:" + contract
        case ("near", "native"): "coingecko:near"
        case ("near", _): "near:" + contract
        case ("polygon", "native"): "coingecko:polygon-ecosystem-token"
        case ("bnb", "native"): "coingecko:binancecoin"
        case ("hyperevm", "native"): "coingecko:hyperliquid"
        case ("tempo", _), ("hyperliquid", _): nil
        case (_, "native"): "coingecko:ethereum"
        default: llamaChain[chain].map { "\($0):\(contract.lowercased())" }
        }
    }

    /// What DefiLlama says `coin` was worth on each of `days`: the nearest price within four hours; a day it has none
    /// for is left out. nil when it didn't answer.
    public static func pastPrices(_ coin: String, at days: [Date]) async -> [Date: Decimal]? {
        guard !days.isEmpty, let asked = try? JSONSerialization.data(withJSONObject: [coin: days.map { Int($0.timeIntervalSince1970) }]),
              var url = URLComponents(string: "https://coins.llama.fi/batchHistorical") else { return [:] }
        url.queryItems = [URLQueryItem(name: "coins", value: String(decoding: asked, as: UTF8.self)), URLQueryItem(name: "searchWidth", value: "4h")]
        guard let link = url.url?.absoluteString,
              let coins = (json(await retrying { try await HTTP.get(link, timeout: 20) }) as? [String: Any])?["coins"] as? [String: [String: Any]] else { return nil }
        let prices: [(t: Double, px: Decimal)] = (coins[coin]?["prices"] as? [[String: Any]] ?? []).compactMap { p in
            guard let px = number(p["price"]), let t = (p["timestamp"] as? NSNumber)?.doubleValue else { return nil }
            return (t, px)
        }
        var out: [Date: Decimal] = [:]
        for d in days {
            let t = d.timeIntervalSince1970
            if let near = prices.min(by: { abs($0.t - t) < abs($1.t - t) }), abs(near.t - t) <= 4 * 3600 { out[d] = near.px }
        }
        return out
    }

    /// Tempo's dollars, read from its node (it has no Blockscout): USDC — what Relay brings and services charge — and
    /// pathUSD and USDT0, Tempo's other stablecoins. Tempo has no coin of its own. nil when the node didn't answer.
    /// With the icons Relay publishes for them (its /chains, 2026-09-30).
    static let tempoStables = [("0x20c000000000000000000000b9537d11c60e8b50", "USDC", "https://assets.relay.link/icons/currencies/usdc.png"),
                               ("0x20c0000000000000000000000000000000000000", "pathUSD", "https://assets.relay.link/icons/currencies/pathUSD.png"),
                               ("0x20c00000000000000000000014f22ca97301eb73", "USDT0", "https://coin-images.coingecko.com/coins/images/53705/large/usdt0.jpg")]
    static func tempo(_ address: String) async -> [Holding]? {
        let owner = address.lowercased().replacingOccurrences(of: "0x", with: "")
        guard owner.count == 40 else { return [] }
        var out: [Holding] = []
        for (contract, symbol, icon) in tempoStables {
            let call: [String: Any] = ["jsonrpc": "2.0", "id": 1, "method": "eth_call",
                                       "params": [["to": contract, "data": "0x70a08231" + String(repeating: "0", count: 24) + owner], "latest"]]
            guard let hex = (await retrying({ json(try await HTTP.post("https://rpc.mainnet.tempo.xyz", json: call)) as? [String: Any] }))?["result"] as? String else { return nil }
            let units = hex.dropFirst(2).reduce(Decimal(0)) { $0 * 16 + Decimal(Int(String($1), radix: 16) ?? 0) }
            if units > 0 {
                out.append(Holding(chain: "tempo", contract: contract, amount: units / 1_000_000, symbol: symbol, icon: URL(string: icon), price: 1, verified: true))
            }
        }
        return out
    }

    /// A dollar stablecoin read straight from its network's node (the app's fresh read before a move: Workspace.freshenSource).
    public static func dollar(_ chain: String, contract: String, amount: Decimal, symbol: String = "USDC", icon: URL?) -> Holding {
        Holding(chain: chain, contract: contract, amount: amount, symbol: symbol, icon: icon, price: 1, verified: true)
    }

    /// HyperEVM, read from its node (its explorers have no API ihatecrypto can use): HYPE itself, USDC — what Relay
    /// brings — USD₮0 and WHYPE, HYPE priced by Relay, with the icons Relay publishes (its /chains, 2026-10-06). nil when
    /// the node didn't answer.
    /// Unit's UBTC, UETH and USOL and Kinetiq's kHYPE too, the coins ihatecrypto swaps there (EVMTokens.hyperevm), priced
    /// by DexScreener.
    static let hyperevmTokens = [("0xb88339cb7199b77e23db6e890353e22632ba630f", "USDC", 6, "https://coin-images.coingecko.com/coins/images/6319/large/usdc.png"),
                                 ("0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", "USD₮0", 6, "https://coin-images.coingecko.com/coins/images/53705/large/usdt0.jpg"),
                                 ("0x5d3a1ff2b6bab83b63cd9ad0787074081a52ef34", "USDe", 18, usdeIcon),
                                 ("0x5555555555555555555555555555555555555555", "WHYPE", 18, "https://coin-images.coingecko.com/coins/images/54469/large/_UP3jBsi_400x400.jpg"),
                                 ("0x9fdbda0a5e284c32744d2f17ee5c74b284993463", "UBTC", 8, ""),
                                 ("0xbe6727b535545c67d5caa73dea54865b92cf7907", "UETH", 18, ""),
                                 ("0x068f321fa8fb9f0d135f290ef6a3e2813e1c8a29", "USOL", 9, ""),
                                 ("0xfd739d4e423301ce9385c1fb8850539d657c296d", "kHYPE", 18, "")]
    static let usdeIcon = "https://coin-images.coingecko.com/coins/images/33613/large/usde.png"
    static let hypeIcon = URL(string: "https://assets.coingecko.com/coins/images/50882/standard/hyperliquid.jpg")
    static func hyperevm(_ address: String) async -> [Holding]? {
        await fromNode("hyperevm", node: "https://hyperliquid.drpc.org", coin: "HYPE", icon: hypeIcon, relayChain: Relay.Chain.hyperevm,
                       wrapped: "WHYPE", dollars: Stablecoins.dollars, dex: "hyperevm", tokens: hyperevmTokens, address: address)
    }

    /// BNB Chain, read from its node too (its explorers want an API key): BNB itself, Binance-Peg USDC and USDT (18
    /// decimals), WBNB, the coins ihatecrypto names there (EVMTokens.bnb), and any token bought here (`extra`: an address
    /// with no explorer would lose it otherwise). BNB priced by Relay, the rest by DexScreener (contracts read 2026-10-08).
    static let bnbTokens = [("0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", "USDC", 18, "https://coin-images.coingecko.com/coins/images/6319/large/usdc.png"),
                            ("0x55d398326f99059ff775485246999027b3197955", "USDT", 18, "https://coin-images.coingecko.com/coins/images/39963/large/usdt.png"),
                            ("0x5d3a1ff2b6bab83b63cd9ad0787074081a52ef34", "USDe", 18, usdeIcon),
                            ("0x1af3f329e8be154074d8769d1ffa4ee058b1dbc3", "DAI", 18, "https://coin-images.coingecko.com/coins/images/9956/large/Badge_Dai.png"),
                            ("0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c", "WBNB", 18, "https://assets.relay.link/icons/currencies/bnb.png"),
                            ("0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82", "CAKE", 18, ""),
                            ("0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c", "BTCB", 18, ""),
                            ("0x2170ed0880ac9a755fd29b2688956bd959f933f8", "ETH", 18, "")]
    static let bnbIcon = URL(string: "https://assets.relay.link/icons/currencies/bnb.png")
    static func bnb(_ address: String, extra: [String]) async -> [Holding]? {
        await fromNode("bnb", node: "https://bsc-rpc.publicnode.com", coin: "BNB", icon: bnbIcon, relayChain: Relay.Chain.bnb,
                       wrapped: "WBNB", dollars: Stablecoins.dollars, dex: "bsc", tokens: bnbTokens, extra: extra, address: address)
    }

    /// A network read straight from its node: its coin, a fixed list of tokens, and `extra` contracts (their symbol and
    /// decimals read too). The coin and its wrapped token priced by Relay, dollars at 1, the rest by DexScreener's `dex`.
    /// nil when the node didn't answer.
    static func fromNode(_ chain: String, node: String, coin: String, icon: URL?, relayChain: UInt64, wrapped: String, dollars: Set<String>,
                         dex: String, tokens: [(String, String, Int, String)], extra: [String] = [], address: String) async -> [Holding]? {
        let owner = address.lowercased().replacingOccurrences(of: "0x", with: "")
        guard owner.count == 40 else { return [] }
        func answer(_ call: [String: Any]) async -> String? {
            (await retrying({ json(try await HTTP.post(node, json: call)) as? [String: Any] }))?["result"] as? String
        }
        func number(_ hex: String) -> Decimal { hex.dropFirst(2).reduce(Decimal(0)) { $0 * 16 + Decimal(Int(String($1), radix: 16) ?? 0) } }
        func read(_ contract: String, _ data: String) async -> String? {
            await answer(["jsonrpc": "2.0", "id": 1, "method": "eth_call", "params": [["to": contract, "data": data], "latest"]])
        }
        let price = (try? await Relay.nativePrice(chainId: relayChain)) ?? nil
        guard let wei = await answer(["jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [address, "latest"]]).map(number) else { return nil }
        var out: [Holding] = []
        if wei > 0 { out.append(Holding(chain: chain, contract: "native", amount: wei / pow(10, 18), symbol: coin, icon: icon, price: price, verified: true, native: true)) }
        var list = tokens
        for contract in Set(extra.map { $0.lowercased() }).subtracting(tokens.map(\.0)).sorted() {
            // symbol() as an ABI string: offset, length, bytes.
            guard let d = await read(contract, "0x313ce567").map(number), let raw = await read(contract, "0x95d89b41"),
                  let bytes = Data(hex: String(raw.dropFirst(2))), bytes.count >= 64 else { continue }
            let length = Int(truncating: NSDecimalNumber(decimal: bytes[32..<64].reduce(Decimal(0)) { $0 * 256 + Decimal($1) }))
            let symbol = bytes.count >= 64 + length ? String(decoding: bytes[64..<(64 + length)], as: UTF8.self) : String(contract.prefix(8))
            list.append((contract, symbol, Int(truncating: NSDecimalNumber(decimal: d)), ""))
        }
        for (contract, symbol, decimals, icon) in list {
            guard let units = await read(contract, "0x70a08231" + String(repeating: "0", count: 24) + owner).map(number) else { return nil }
            if units > 0 {
                out.append(Holding(chain: chain, contract: contract, amount: units / pow(10, decimals), symbol: symbol, icon: URL(string: icon),
                                   price: symbol == wrapped ? price : dollars.contains(symbol) ? 1 : nil, verified: true))
            }
        }
        // The swapped coins (UBTC, CAKE…) priced by their deepest pool; nothing asked when none is held.
        let unpriced = out.filter { $0.price == nil }.map(\.contract)
        if !unpriced.isEmpty { out = dexPrices(out, try? await HTTP.get("https://api.dexscreener.com/tokens/v1/\(dex)/" + unpriced.joined(separator: ","))) }
        return out
    }

    /// Every page of the address's ERC-20s. Blockscout serves 50 at a time, the most valuable first, and says where the
    /// next page starts; a page that fails is asked again (the API throws the odd 500). nil when even the first page
    /// never came; a later one that never comes ends the list there, losing only the least valuable tail.
    // ponytail: 20 pages at most (1000 tokens): past that the address is a spam dump and the tail is unpriced dust.
    static func tokenPages(_ host: String, address: String) async -> [[String: Any]]? {
        var items: [[String: Any]] = []
        var next: [String: Any]? = [:]
        for page in 0..<20 {
            guard let params = next else { break }
            var url = URLComponents(string: "https://\(host)/api/v2/addresses/\(address)/tokens")
            url?.queryItems = [URLQueryItem(name: "type", value: "ERC-20")]
                + params.sorted { $0.key < $1.key }.compactMap { k, v in v is NSNull ? nil : URLQueryItem(name: k, value: "\(v)") }
            guard let link = url?.string,
                  let body = await retrying({ json(try await HTTP.get(link, headers: browser)) as? [String: Any] }) else { return page == 0 ? nil : items }
            items += body["items"] as? [[String: Any]] ?? []
            next = body["next_page_params"] as? [String: Any]
        }
        return items
    }

    /// Bridged USDC from before Circle issued its own there: both contracts call themselves «USDC», so the portfolio
    /// names them «USDC.e» (as Uniswap's list does), and offers to swap them for Circle's.
    public static let bridgedUSDC = ["polygon": "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", "arbitrum": "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8"]

    static func evmTokens(_ chain: String, _ items: [[String: Any]], skip: Set<String>) -> [Holding] {
        items.compactMap { item in
            guard let t = item["token"] as? [String: Any], let contract = (t["address_hash"] as? String)?.lowercased(), !skip.contains(contract),
                  let raw = number(item["value"]), raw > 0, let decimals = Int(t["decimals"] as? String ?? "") else { return nil }
            let rate = number(t["exchange_rate"])
            return Holding(chain: chain, contract: contract, amount: raw / pow(10, decimals),
                           symbol: bridgedUSDC[chain] == contract ? "USDC.e" : t["symbol"] as? String ?? "?",
                           icon: (t["icon_url"] as? String).flatMap(URL.init(string:)), price: rate,
                           verified: rate != nil && t["reputation"] as? String != "scam")
        }
    }

    static func nativeCoin(_ chain: String, _ a: [String: Any]) -> Holding? {
        guard let wei = number(a["coin_balance"]), wei > 0 else { return nil }
        // Polygon's own coin is POL; everywhere else it's ETH.
        return Holding(chain: chain, contract: "native", amount: wei / pow(10, 18), symbol: chain == "polygon" ? "POL" : "ETH", icon: chain == "polygon" ? nil : ethIcon,
                       price: number(a["exchange_rate"]), verified: true, native: true)
    }

    /// The deepest pool per token from DexScreener's pairs: its day's move for every token, its price and pool for an
    /// unpriced one (a CoinGecko rate stays).
    static func dexPrices(_ tokens: [Holding], _ data: Data?) -> [Holding] {
        var best: [String: (price: Decimal, liquidity: Decimal, change: Decimal?)] = [:]
        for pair in json(data) as? [[String: Any]] ?? [] {
            guard let base = ((pair["baseToken"] as? [String: Any])?["address"] as? String)?.lowercased(),
                  let price = number(pair["priceUsd"]) else { continue }
            let liquidity = number((pair["liquidity"] as? [String: Any])?["usd"]) ?? 0
            if liquidity >= best[base]?.liquidity ?? -1 { best[base] = (price, liquidity, number((pair["priceChange"] as? [String: Any])?["h24"])) }
        }
        return tokens.map { t in
            guard let b = best[t.contract] else { return t }
            var priced = t
            priced.change24h = t.change24h ?? b.change
            if t.price == nil {
                priced.price = b.price
                priced.liquidity = b.liquidity
            }
            return priced
        }
    }

    // MARK: Hyperliquid spot

    /// Everything on Hyperliquid's spot side, USDC included (the perp account doesn't count it), priced off its own book.
    /// nil when it couldn't be read.
    public static func hyperliquidSpot(user: String) async -> [Holding]? {
        async let state = retrying { try await HTTP.post("https://api.hyperliquid.xyz/info", json: ["type": "spotClearinghouseState", "user": user]) }
        async let meta = retrying { try await HTTP.post("https://api.hyperliquid.xyz/info", json: ["type": "spotMetaAndAssetCtxs"]) }
        guard let s = await state, let m = await meta else { return nil }
        return hyperliquidSpot(s, meta: m)
    }

    static func hyperliquidSpot(_ state: Data?, meta: Data?) -> [Holding] {
        let balances = (json(state) as? [String: Any])?["balances"] as? [[String: Any]] ?? []
        let pair = json(meta) as? [Any] ?? []
        let universe = (pair.first as? [String: Any])?["universe"] as? [[String: Any]] ?? []
        let contexts = pair.count > 1 ? pair[1] as? [[String: Any]] ?? [] : []
        return balances.compactMap { b in
            guard let coin = b["coin"] as? String, let token = b["token"] as? Int, let amount = number(b["total"]), amount > 0 else { return nil }
            if token == 0 { return Holding(chain: "hyperliquid", contract: coin, amount: amount, symbol: "USDC", price: 1, verified: true) }
            // The token's market against USDC (token 0) on the spot book.
            let market = universe.first { ($0["tokens"] as? [Int]) == [token, 0] }
            let context = (market?["index"] as? Int).flatMap { $0 < contexts.count ? contexts[$0] : nil }
            let price = number(context?["midPx"]) ?? number(context?["markPx"]), before = number(context?["prevDayPx"])
            // HYPE is Hyperliquid's own coin, vouched for like ETH on its networks (a spot name belongs to one token).
            return Holding(chain: "hyperliquid", contract: coin, amount: amount, symbol: coin, price: price, verified: coin == "HYPE", liquidity: number(context?["dayNtlVlm"]),
                           change24h: price.flatMap { p in before.flatMap { $0 > 0 ? (p - $0) / $0 * 100 : nil } })
        }
    }

    // MARK: NEAR

    /// Fungible tokens on the NEAR account, wNEAR aside (counted with NEAR). Ref's price list names and prices them;
    /// a token it doesn't list is named by its own contract's metadata, all of them asked at once. nil when the
    /// account's list or the prices couldn't be read.
    public static func near(account: String) async -> [Holding]? {
        async let list = retrying { try await HTTP.get("https://api.fastnear.com/v1/account/\(account)/ft") }
        async let prices = retrying { try await HTTP.get("https://indexer.ref.finance/list-token-price") }
        guard let listed = await list, let priced = await prices else { return nil }
        let ref = json(priced) as? [String: [String: Any]] ?? [:]
        let unlisted = nearBalances(listed).map(\.contract).filter { ref[$0] == nil }
        let metadata = await withTaskGroup(of: (String, (symbol: String, decimals: Int)?).self) { group in
            for contract in unlisted { group.addTask { (contract, await nearMetadata(contract)) } }
            var out: [String: (symbol: String, decimals: Int)] = [:]
            for await (contract, m) in group { out[contract] = m }
            return out
        }
        return near(listed, prices: priced, metadata: metadata)
    }

    static func nearBalances(_ data: Data?) -> [(contract: String, raw: Decimal)] {
        ((json(data) as? [String: Any])?["tokens"] as? [[String: Any]] ?? []).compactMap { t in
            guard let c = t["contract_id"] as? String, c != "wrap.near", let raw = number(t["balance"]), raw > 0 else { return nil }
            return (c, raw)
        }
    }

    static func near(_ list: Data?, prices: Data?, metadata: [String: (symbol: String, decimals: Int)]) -> [Holding] {
        let ref = json(prices) as? [String: [String: Any]] ?? [:]
        return nearBalances(list).compactMap { contract, raw in
            let listed = ref[contract]
            guard let decimals = (listed?["decimal"] as? Int) ?? metadata[contract]?.decimals else { return nil }
            let symbol = (listed?["symbol"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? metadata[contract]?.symbol ?? String(contract.prefix(8))
            let amount = raw / pow(10, decimals), price = number(listed?["price"])
            // Ref lists junk too: its price counts as a check only when the holding is worth a cent.
            return Holding(chain: "near", contract: contract, amount: amount, symbol: symbol, price: price,
                           verified: (price ?? 0) * amount >= 0.01)
        }
    }

    static func nearMetadata(_ contract: String) async -> (symbol: String, decimals: Int)? {
        let params: [String: Any] = ["request_type": "call_function", "finality": "final", "account_id": contract,
                                     "method_name": "ft_metadata", "args_base64": "e30="]
        guard let data = try? await HTTP.post("https://rpc.mainnet.fastnear.com", json: ["jsonrpc": "2.0", "id": 1, "method": "query", "params": params]),
              let bytes = ((json(data) as? [String: Any])?["result"] as? [String: Any])?["result"] as? [Int],
              let m = try? JSONSerialization.jsonObject(with: Data(bytes.map(UInt8.init))) as? [String: Any],
              let decimals = m["decimals"] as? Int else { return nil }
        return (m["symbol"] as? String ?? String(contract.prefix(8)), decimals)
    }

    // MARK: Helpers

    static func json(_ data: Data?) -> Any? { data.flatMap { try? JSONSerialization.jsonObject(with: $0) } }

    /// Up to three tries, a second apart: public APIs drop the odd request.
    static func retrying<T>(_ fetch: () async throws -> T?) async -> T? {
        for attempt in 0..<3 {
            if let value = try? await fetch() { return value }
            if attempt < 2 { try? await Task.sleep(for: .seconds(1)) }
        }
        return nil
    }

    /// Through the number's text: a JSON double's decimalValue turns 1.88 into 1.8799999….
    static func number(_ v: Any?) -> Decimal? { ((v as? NSNumber)?.stringValue ?? v as? String).flatMap { Decimal(string: $0) } }
}

extension Holdings {
    public enum Pick: Equatable, Sendable { case one(Holding), none, several([Holding]) }

    /// The holding a transfer names: by mint or contract (any case on EVM), else by ticker, any case. The network's own
    /// coin wins its ticker («ETH»), and among tokens sharing one — an airdrop copying a real token's name — the only
    /// one that looks real; otherwise `several`, and the address decides.
    public static func pick(_ query: String, in held: [Holding]) -> Pick {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        if let one = held.first(where: { $0.contract.lowercased() == q }) { return .one(one) }
        let named = held.filter { $0.symbol.lowercased() == q }
        if let coin = named.first(where: \.native) { return .one(coin) }
        let real = named.filter(\.looksReal)
        return named.count == 1 ? .one(named[0]) : real.count == 1 ? .one(real[0]) : named.isEmpty ? .none : .several(named)
    }
}
