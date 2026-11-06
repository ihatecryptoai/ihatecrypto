import Foundation

/// Any token by its contract: name, symbol, price, 24 h move, liquidity and its icon, from DexScreener's public API
/// (api.dexscreener.com, no key). The chain is the one with the most liquidity for it.
public enum TokenLookup {
    public struct Info: Codable, Equatable, Sendable {
        /// DexScreener's chain id: «robinhood», «base», «arbitrum», «solana», «ethereum»…
        public let chain: String
        public let address: String
        public let name: String
        public let symbol: String
        public let priceUSD: Decimal?
        public let change24h: Decimal?
        public let liquidityUSD: Decimal?
        public let imageURL: URL?
        public let pairURL: URL?
    }

    /// A symbol to warn about: letters outside ASCII (a Cyrillic «С» in «USDС»), or a major coin's ticker on a
    /// contract that isn't one of that coin's `genuine` (lowercased) contracts.
    public static func suspicious(_ symbol: String, contract: String, genuine: Set<String>) -> Bool {
        // A genuine contract's own symbol may be outside ASCII: USD₮0 on its own contract.
        if Stablecoins.byNetwork.values.joined().contains(where: { $0.address.lowercased() == contract.lowercased() && ([$0.symbol] + $0.also).contains(symbol) }) { return false }
        if symbol.unicodeScalars.contains(where: { !$0.isASCII }) { return true }
        let majors: Set = ["USDC", "USDT", "USDT0", "USDG", "DAI", "USDS", "USDE", "ETH", "WETH", "BTC", "WBTC", "SOL"]
        return majors.contains(symbol.uppercased()) && !genuine.contains(contract.lowercased())
    }

    public static func lookup(_ address: String) async throws -> [Info] {
        let (data, _) = try await URLSession.shared.data(from: URL(string: "https://api.dexscreener.com/latest/dex/tokens/\(address)")!)
        return parse(data, address: address)
    }

    /// One entry per chain, from the deepest pair where the token is the base; deepest chain first.
    public static func parse(_ data: Data, address: String) -> [Info] {
        guard let pairs = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["pairs"] as? [[String: Any]] else { return [] }
        func dec(_ v: Any?) -> Decimal? { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber).map { $0.decimalValue } }
        var best: [String: (Decimal, Info)] = [:]
        for p in pairs {
            guard let base = p["baseToken"] as? [String: Any], (base["address"] as? String)?.lowercased() == address.lowercased(),
                  let chain = p["chainId"] as? String else { continue }
            let liquidity = dec((p["liquidity"] as? [String: Any])?["usd"]) ?? 0
            guard liquidity > (best[chain]?.0 ?? -1) else { continue }
            let info = Info(chain: chain, address: base["address"] as? String ?? address, name: base["name"] as? String ?? "?", symbol: base["symbol"] as? String ?? "?",
                            priceUSD: dec(p["priceUsd"]), change24h: dec((p["priceChange"] as? [String: Any])?["h24"]), liquidityUSD: liquidity,
                            imageURL: ((p["info"] as? [String: Any])?["imageUrl"] as? String).flatMap(URL.init), pairURL: (p["url"] as? String).flatMap(URL.init))
            best[chain] = (liquidity, info)
        }
        return best.values.sorted { $0.0 > $1.0 }.map(\.1)
    }
}

/// What a token's detail board shows: its deepest pair on one chain (DexScreener) and that pool's price history
/// (GeckoTerminal's public API, no key, about 30 calls a minute).
public enum TokenMarket {
    public struct Info: Equatable, Sendable {
        public struct Link: Equatable, Sendable {
            public let label: String
            public let url: URL
        }

        public let pool: String
        public let name: String
        public let symbol: String
        /// «uniswap v4», «ramses».
        public let dex: String
        public let quote: String
        public let priceUSD: Decimal?
        /// «m5», «h1», «h6», «h24» → percent.
        public let changes: [String: Decimal]
        public let liquidityUSD: Decimal?
        public let fdv: Decimal?
        public let marketCap: Decimal?
        public let volume24h: Decimal?
        public let buys24h: Int?
        public let sells24h: Int?
        public let created: Date?
        public let imageURL: URL?
        public let pairURL: URL?
        /// The project's website and socials, as the listing gives them.
        public let links: [Link]
    }

    public struct Point: Equatable, Sendable {
        public let time: Date
        public let close: Decimal
        public init(time: Date, close: Decimal) { self.time = time; self.close = close }
    }

    public static func info(_ address: String, chain: String) async throws -> Info? {
        let (data, _) = try await URLSession.shared.data(from: URL(string: "https://api.dexscreener.com/latest/dex/tokens/\(address)")!)
        return parse(data, address: address, chain: chain)
    }

    /// The deepest pair on `chain` where the token is the base.
    public static func parse(_ data: Data, address: String, chain: String) -> Info? {
        guard let pairs = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["pairs"] as? [[String: Any]] else { return nil }
        func dec(_ v: Any?) -> Decimal? { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber).map { $0.decimalValue } }
        let ours = pairs.filter { ($0["baseToken"] as? [String: Any])?["address"].map { "\($0)".lowercased() } == address.lowercased() && $0["chainId"] as? String == chain }
        guard let p = ours.max(by: { (dec(($0["liquidity"] as? [String: Any])?["usd"]) ?? 0) < (dec(($1["liquidity"] as? [String: Any])?["usd"]) ?? 0) }),
              let pool = p["pairAddress"] as? String else { return nil }
        let txns = (p["txns"] as? [String: Any])?["h24"] as? [String: Any]
        let info = p["info"] as? [String: Any]
        var links: [Info.Link] = []
        for w in info?["websites"] as? [[String: Any]] ?? [] {
            if let url = (w["url"] as? String).flatMap(URL.init) { links.append(.init(label: w["label"] as? String ?? "Website", url: url)) }
        }
        for s in info?["socials"] as? [[String: Any]] ?? [] {
            let type = s["type"] as? String ?? "link"
            if let url = (s["url"] as? String).flatMap(URL.init) { links.append(.init(label: type == "twitter" ? "X" : type.capitalized, url: url)) }
        }
        let labels = (p["labels"] as? [String] ?? []).joined(separator: " ")
        let base = p["baseToken"] as? [String: Any]
        return Info(
            pool: pool, name: base?["name"] as? String ?? "?", symbol: base?["symbol"] as? String ?? "?", dex: [p["dexId"] as? String ?? "?", labels].filter { !$0.isEmpty }.joined(separator: " "),
            quote: (p["quoteToken"] as? [String: Any])?["symbol"] as? String ?? "?", priceUSD: dec(p["priceUsd"]),
            changes: ((p["priceChange"] as? [String: Any]) ?? [:]).compactMapValues { dec($0) },
            liquidityUSD: dec((p["liquidity"] as? [String: Any])?["usd"]), fdv: dec(p["fdv"]), marketCap: dec(p["marketCap"]),
            volume24h: dec((p["volume"] as? [String: Any])?["h24"]), buys24h: (txns?["buys"] as? NSNumber)?.intValue, sells24h: (txns?["sells"] as? NSNumber)?.intValue,
            created: (p["pairCreatedAt"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue / 1000) },
            imageURL: (info?["imageUrl"] as? String).flatMap(URL.init), pairURL: (p["url"] as? String).flatMap(URL.init), links: links)
    }

    /// The pool's closes in USD over the range, oldest first: 15-minute bars for a day, hourly for a week, 4-hour for a month.
    public static func history(chain: String, pool: String, range: ChartCommand.Range) async throws -> [Point] {
        let network = chain == "ethereum" ? "eth" : chain
        let (frame, aggregate, limit) = switch range {
        case .day: ("minute", 15, 96)
        case .week: ("hour", 1, 168)
        case .month: ("hour", 4, 180)
        }
        let url = URL(string: "https://api.geckoterminal.com/api/v2/networks/\(network)/pools/\(pool)/ohlcv/\(frame)?aggregate=\(aggregate)&limit=\(limit)&currency=usd&token=base")!
        let (data, response) = try await URLSession.shared.data(from: url)
        if (response as? HTTPURLResponse)?.statusCode == 429 { throw URLError(.resourceUnavailable, userInfo: [NSLocalizedDescriptionKey: "GeckoTerminal is busy, try again in a minute"]) }
        return parseHistory(data)
    }

    /// GeckoTerminal's `ohlcv_list`: [time, open, high, low, close, volume], newest first.
    public static func parseHistory(_ data: Data) -> [Point] {
        guard let list = (((try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["data"] as? [String: Any])?["attributes"] as? [String: Any])?["ohlcv_list"] as? [[NSNumber]] else { return [] }
        return list.compactMap { row in
            row.count >= 5 ? Point(time: Date(timeIntervalSince1970: row[0].doubleValue), close: Decimal(string: row[4].stringValue) ?? row[4].decimalValue) : nil
        }
        .sorted { $0.time < $1.time }
    }
}

/// «добавь в favorite вот этот токен на робингуде 0x…», «watch token 0x… on Base», «remove token AI from favorites»,
/// «chart token B5AN… on solana».
public enum TokenCommand: Equatable, Sendable {
    /// `chain`: as the user named it (DexScreener's id), nil = wherever it trades most.
    case favorite(address: String, chain: String?)
    case unfavorite(symbol: String)
    /// The token's card on the canvas: its chart, market and links.
    case show(address: String, chain: String?)

    public static func parse(_ text: String) -> TokenCommand? {
        let q = text.lowercased()
        if let m = text.firstMatch(of: /(?i)(?:remove|unstar|убери|удали)\s+(?:the\s+)?(?:token|токен)\s+\$?([a-z0-9]{1,12})\s+(?:from|из)\s+(?:my\s+)?(?:favou?rites?|избранн\w*|watchlist)/) {
            return .unfavorite(symbol: String(m.1).uppercased())
        }
        guard let address = AddressBookCommand.address(in: text) else { return nil }
        let chain: String? = q.contains(/robinhood|робинхуд|робингуд/) ? "robinhood" : q.contains(/\bbase\b|бейс/) ? "base"
            : q.contains(/arbitrum|арбитрум/) ? "arbitrum" : q.contains(/ethereum|эфириум|\beth\s+mainnet/) ? "ethereum"
            : q.contains(/solana|солан/) || !address.hasPrefix("0x") ? "solana" : nil
        if q.contains(/^\s*(?:chart|show)\s+token\b/) { return .show(address: address, chain: chain) }
        guard q.contains(/\btoken|токен|\bcoin\b|монет|watch|следи|favou?rite|избранн|watchlist/),
              // «save 0x… as Bybit» is an address for the book, not a token; «купи 0x… токен» is a trade.
              !q.contains(/\bbuy\b|куп\w*|purchase|приобрет\w*|\bsell\b|прода\w*/),
              !q.contains(/\bas\s|\bcalled\b|\bnamed\b|\bкак\s|под\s+именем/) else { return nil }
        return .favorite(address: address, chain: chain)
    }
}
