import Foundation
import CryptoKit
import WalletCore

public enum Side: String, Codable, Sendable {
    case long, short
}

/// What the user wants. Only words and numbers from the chat; never addresses or keys.
public struct TradeIntent: Codable, Equatable, Sendable {
    public var asset = "ETH"
    public var side = Side.long
    public var riskUSD: Decimal?
    public var stopPct: Decimal?
    public var takePct: Decimal?
    public var leverage: Decimal?
    /// USDC to send from Solana. nil = exactly what the position needs.
    public var amountUSDC: Decimal?
    /// Take as a multiple of the stop distance («тейк 2R», «1:3»); used when takePct is not set.
    public var takeR: Decimal?
    /// «как учат»: what the user left out is filled by the risk rules (stop from volatility, 2R take, 1% risk).
    public var autoRules: Bool?
    /// Limit entry price («лимит 2650»); nil = market entry.
    public var entryPx: Decimal?
    /// Limit entry this far from the mark in the trade's favour («вход на откате 2%»); resolved to entryPx.
    public var pullbackPct: Decimal?
    /// «тейк частями»: half at 1R, then the stop goes to breakeven and the rest runs to the take.
    public var partialTake: Bool?
    /// «трейлинг 2%»: the stop follows the best price at this distance while the app is open.
    public var trailingPct: Decimal?
    /// «hedge 0.1 SOL»: a short of this many of the coins the wallet holds (Hedge); sized by them, not by a risk.
    public var hedge: Decimal?
    /// «hedge SOL» = 1, «hedge SOL half» = 0.5: the share of what the wallet holds, turned into `hedge` by Plan.resolve.
    public var hedgeShare: Decimal?

    public var isHedge: Bool { hedge != nil || hedgeShare != nil }

    public init() {}

    /// Human-readable list of fields that differ from `old`.
    public func changes(from old: TradeIntent) -> [String] {
        func opt(_ v: Decimal?, _ f: (Decimal) -> String) -> String { v.map(f) ?? "—" }
        var out: [String] = []
        if asset != old.asset { out.append("market \(asset)") }
        if side != old.side { out.append(side == .long ? "long" : "short") }
        if riskUSD != old.riskUSD { out.append("risk \(opt(riskUSD, Fmt.usd))") }
        if stopPct != old.stopPct { out.append("stop \(opt(stopPct, Fmt.pct))") }
        if takePct != old.takePct { out.append("take \(opt(takePct, Fmt.pct))") }
        if leverage != old.leverage { out.append("leverage \(opt(leverage) { Fmt.num($0) + "×" })") }
        if amountUSDC != old.amountUSDC { out.append("money in the trade \(opt(amountUSDC) { Fmt.num($0) + " USDC" })") }
        if takeR != old.takeR { out.append("take \(opt(takeR) { Fmt.num($0) + "R" })") }
        if autoRules != old.autoRules, autoRules == true { out.append("the rest by the risk rules") }
        if partialTake != old.partialTake, partialTake == true { out.append("take in parts: half at 1R, then stop to breakeven") }
        if trailingPct != old.trailingPct { out.append("trailing \(opt(trailingPct, Fmt.pct))") }
        if isHedge, hedge != old.hedge || hedgeShare != old.hedgeShare {
            out.append(hedge.map { "hedge \(Fmt.num($0, maxFraction: 6)) \(asset)" }
                       ?? hedgeShare.map { $0 < 1 ? "hedge \(Fmt.pct($0 * 100)) of the \(asset) held" : "hedge all the \(asset) held" } ?? "")
        }
        if entryPx != old.entryPx || pullbackPct != old.pullbackPct {
            out.append(entryPx.map { "limit entry at \(Fmt.price($0))" } ?? pullbackPct.map { "entry on a \(Fmt.pct($0)) pullback" } ?? "market entry")
        }
        return out
    }

    /// The plan in the router's own form («long SOL risk $0.3 stop 3% take 6% leverage 2x»), which IntentParser.apply
    /// reads back to the same plan: what a price alert tied to a plan runs to prepare it again.
    /// A plan missing its risk or its stop («long ETH», «лонг соланы х3 на 5 юсдс», «short BTC risk half a buck») is
    /// filled by the risk rules, as «by the book» is; what the user named stays. «long ETH» alone would otherwise equal
    /// the empty plan (TradeIntent's defaults are ETH, long), which isn't drawn.
    public var started: TradeIntent {
        var next = self
        // A hedge is sized by the coins held: the rules have no risk to fill.
        if next.autoRules == nil, !isHedge, next.riskUSD == nil || next.stopPct == nil { next.autoRules = true }
        return next
    }

    public var command: String {
        func n(_ d: Decimal) -> String { NSDecimalNumber(decimal: d).stringValue }
        let name = asset.hasPrefix("xyz:") ? String(asset.dropFirst(4)) : asset
        if isHedge {
            var parts = [hedge.map { "hedge \(n($0)) \(name)" } ?? "hedge \(name)\((hedgeShare ?? 1) < 1 ? " \(n((hedgeShare ?? 1) * 100))%" : "")"]
            if let v = leverage { parts.append("leverage \(n(v))x") }
            if let v = stopPct { parts.append("stop \(n(v))%") }
            return parts.joined(separator: " ")
        }
        var parts = ["\(side == .long ? "long" : "short") \(name)"]
        if let v = riskUSD { parts.append("risk $\(n(v))") }
        if let v = stopPct { parts.append("stop \(n(v))%") }
        if let v = takePct { parts.append("take \(n(v))%") } else if let v = takeR { parts.append("take \(n(v))R") }
        if let v = leverage { parts.append("leverage \(n(v))x") }
        if let v = amountUSDC { parts.append("\(n(v)) USDC") }
        if autoRules == true { parts.append("by the book") }
        // «pullback N%»: the form the parser reads first.
        if let v = pullbackPct { parts.append("pullback \(n(v))%") } else if let v = entryPx { parts.append("limit \(n(v))") }
        if let v = trailingPct { parts.append("trailing \(n(v))%") }
        if partialTake == true { parts.append("take in parts") }
        return parts.joined(separator: " ")
    }
}

/// Deterministic parsing of the phrases a trader actually types. The cloud model only sees what this misses.
public enum IntentParser {
    static let fallbackMarkets: Set<String> = ["BTC", "ETH", "SOL"]
    /// Words for markets that are not tickers: commodities on HIP-3 exchanges.
    /// Brent before oil: «brent oil» is Brent.
    nonisolated(unsafe) static let aliases: [(Regex<Substring>, String)] = [
        (try! Regex("брент|brent"), "BRENTOIL"), (try! Regex("нефт|\\boil\\b|\\bwti\\b|crude"), "CL"),
        (try! Regex("золот|\\bgold\\b"), "GOLD"), (try! Regex("серебр|silver"), "SILVER"),
        // Names people say instead of tickers. Not «Solana»: «с соланы» in a trade names the network.
        (try! Regex("\\bэфир\\w*|\\bethereum\\b|\\bэфириум\\w*"), "ETH"), (try! Regex("биткоин\\w*|биткойн\\w*|\\bбиток\\w*|\\bbitcoin\\b"), "BTC"),
    ]

    /// «xyz:CL» → «WTI oil (xyz:CL)» for the plan's headings.
    public static func pretty(_ asset: String) -> String {
        let names = ["CL": "WTI oil", "BRENTOIL": "Brent oil", "GOLD": "gold", "SILVER": "silver", "TSLA": "Tesla", "NVDA": "Nvidia",
                     "GOOGL": "Google", "META": "Meta", "AAPL": "Apple", "MSFT": "Microsoft", "AMZN": "Amazon", "MU": "Micron",
                     "COIN": "Coinbase", "CRCL": "Circle", "SNDK": "Sandisk", "SP500": "S&P 500", "XYZ100": "tech 100 index"]
        guard let colon = asset.firstIndex(of: ":"), let name = names[String(asset[asset.index(after: colon)...])] else { return asset }
        return "\(name) (\(asset))"
    }
    static let notTickers: Set<String> = ["LONG", "SHORT", "STOP", "TAKE", "RISK", "SL", "TP", "USDC", "USD", "LEV"]
    /// Hyperliquid tickers that are everyday words: in lowercase they are words («move stop», «not now»), not markets.
    static let everydayWords: Set<String> = [
        "ACE", "ALT", "APE", "AR", "BABY", "BANANA", "BLUR", "CAKE", "CHIP", "DASH", "DOT", "GAS", "GOAT", "GRASS", "HYPER", "IO",
        "LAYER", "LINK", "LIT", "ME", "MET", "MON", "MOVE", "NEAR", "NIL", "NOT", "PEOPLE", "PROVE", "PUMP", "RENDER", "SAND", "SKY",
        "STABLE", "SUPER", "TURBO", "USELESS", "USUAL", "VIRTUAL", "ZEN",
    ]

    /// Returns the updated intent, or nil when nothing in `text` was recognised.
    public static func apply(_ text: String, to intent: TradeIntent, markets: Set<String> = []) -> TradeIntent? {
        let q = plainNumbers(text.lowercased())
        // «sell SOL for 10 USDC», «продай сол на 10 баксов» (a sale in dollars the wallet parser had no coin count for),
        // «buy SOL for 5 USDC every 0 days» (a recurring buy it refused), «buy USDT for 750 USDC» (a swap with its network
        // left out): never a long of that many USDC — the last one drew a 5× ETH long for a USDC → USDT swap.
        if q.contains(/\bsell\b|прода[йтв]|\bbuy\b|куп|покупа/),
           !q.contains(/\b(?:лонг|шорт|long|short)/), q.contains(/\$\s*\d|\d\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)/) { return nil }
        // A money move the wallet parser didn't take («move 5 USDC from Solana to BNB», «swap 0.01 BNB for USDT on bsc»,
        // «buy CAKE on bnb for 5 USDC»): never a trade plan of the ticker it names — the router asks again or says it can't.
        // A stop or a take being moved stays the plan's.
        if !q.contains(/\b(?:лонг|шорт|long|short)/), !q.contains(/\b(?:stop|take|tp|sl|стоп\w*|тейк\w*)\b/),
           // An amount, or «all»: a bare «MOVE» is the ticker being looked up (namedAsset), not a move.
           q.contains(/^\s*(?:move|swap|exchange|convert|bridge|send|transfer|deposit|withdraw|lend|stake|receive|top\s*up|переве\w*|перевод\w*|отправ\w*|помен\w*|обмен\w*|свап\w*|своп\w*|конверт\w*|выведи|вывод\w*|полож\w*|внеси|пополн\w*|задепоз\w*|бридж\w*)\b/)
               && q.contains(/\d|\ball\b|\bвс[её]\b|\bвесь\b/)
            || q.contains(/^\s*(?:buy|sell|купи\w*|прода\w*)\b.*\b(?:on|на|в|во)\s+(?:the\s+)?(?:solana|base|arbitrum|polygon|ethereum|mainnet|optimism|hyperevm|robinhood|tempo|bnb|bsc|binance|avalanche|avax|sui|tron|солан\w*|бейс\w*|арбитрум\w*|полигон\w*|эфириум\w*|оптимизм\w*|хайперевм\w*|робинхуд\w*|темпо\w*|бнб|бсц|бинанс\w*|аваланч\w*)\b/) { return nil }
        var next = intent
        var hit = false
        // «hedge SOL», «захеджируй половину эфира»: a plan of its own, sized by what the wallet holds, never a change to the
        // plan on the canvas; a long or a short after it is a trade again.
        // «не хеджируй», «сними хедж»: never a new hedge, and nothing else either; «long SOL, no hedge» is the long.
        let unhedge = q.contains(/\bне\s+(?:за)?хедж|сним\w*\s+хедж|убери\s+хедж|без\s+хедж|\bno\s+hedg|\bdon'?t\s+hedg|\bunhedg|\bwithout\s+(?:a\s+)?hedg/)
        if unhedge, !q.contains(/\b(?:лонг|шорт|long|short)/) { return nil }
        let hedging = !unhedge && q.contains(/\bhedg|хедж/)
        // A dollar amount isn't a hedge's size: it counts coins, so the model asks.
        if hedging, q.contains(/\$\s*\d|\d\s*(?:\$|usdc|usd\b|долл|бакс)/) { return nil }
        if hedging { next = TradeIntent() }
        var named = false

        if q.contains(/\b(?:лонг|long)/) { next.side = .long; hit = true }
        if q.contains(/\b(?:шорт|short)/) { next.side = .short; hit = true }
        if !hedging, q.contains(/\b(?:лонг|шорт|long|short)/) { next.hedge = nil; next.hedgeShare = nil }

        let known = markets.isEmpty ? fallbackMarkets : markets
        // A ticker on the main exchange, else the same ticker on a HIP-3 one («xyz:TSLA»).
        func market(_ ticker: String) -> String? {
            if known.contains(ticker) { return ticker }
            // Coins priced in fractions of a cent trade by the thousand: PEPE is «kPEPE», BONK «kBONK».
            if known.contains("k" + ticker) { return "k" + ticker }
            if ticker.hasPrefix("K"), known.contains("k" + ticker.dropFirst()) { return "k" + ticker.dropFirst() }   // «KPEPE»
            return known.sorted().first { $0.hasSuffix(":" + ticker) }
        }
        if let m = q.firstMatch(of: /\$([a-z][a-z0-9]{1,9})\b/) {
            next.asset = market(m.1.uppercased()) ?? m.1.uppercased(); hit = true; named = true
        } else if let alias = Self.aliases.first(where: { q.contains($0.0) }) {
            next.asset = market(alias.1) ?? "xyz:" + alias.1; hit = true; named = true
        } else {
            // Typed as a ticker («ETH», «NOW») a word counts on any exchange; in lowercase only a main-exchange ticker
            // that isn't an everyday word, so «what is open now» or «move stop» never name a market.
            for word in text.split(whereSeparator: { !$0.isLetter && !$0.isNumber }) {
                let ticker = word.uppercased()
                guard ticker.count >= 2, !notTickers.contains(ticker) else { continue }
                // «kPEPE» as Hyperliquid spells it, «kpepe», «KPEPE»: the thousand-unit market.
                let thousand = ticker.hasPrefix("K") && known.contains("k" + ticker.dropFirst()) ? "k" + ticker.dropFirst() : nil
                let name = word == ticker ? market(ticker) : known.contains(ticker) && !everydayWords.contains(ticker) ? ticker : thousand
                if let name {
                    next.asset = name; hit = true; named = true
                    break
                }
            }
        }

        // «риск $10», «риск на сделку — не больше 10$». A percent after the number is not dollars.
        if let v = number(q, /(?:риск|risk)[^\d%]{0,30}?(\d+(?:[.,]\d+)?)(?!\d|[.,]\d|\s*%)/) { next.riskUSD = v; hit = true }
        if let v = number(q, /(?:стоп|stop|\bsl\b)[^\d]{0,15}?(\d+(?:[.,]\d+)?)\s*%/) { next.stopPct = v; hit = true }
        // Between the word and the number at most one short word («тейк на 6%», «take profit at 6%»): «take in parts on
        // a 2% pullback» has no take percent.
        if let v = number(q, /(?:тейк|take|\btp\b)(?:[- ]?(?:profit|профит)\w*)?[^\d\p{L}]*(?:\p{L}{1,3}[^\d\p{L}]+)?(\d+(?:[.,]\d+)?)\s*%/) { next.takePct = v; hit = true }
        // «плечо 2», «2x», «3х» (Cyrillic х), «2×»
        if let v = number(q, /(?:плеч|leverage|\blev\b)[^\d]{0,10}?(\d+(?:[.,]\d+)?)/)
            ?? number(q, /(\d+(?:[.,]\d+)?)\s*[xх×](?!\p{L})/) { next.leverage = v; hit = true }
        if let v = number(q, /(\d+(?:[.,]\d+)?)\s*usdc/) { next.amountUSDC = v; hit = true }
        // «тейк 2R», «1:3», «один к трём» are ratios to the stop, not percents.
        if let v = number(q, /(?:^|[^\d.,])(\d+(?:[.,]\d+)?)\s*r\b/) ?? number(q, /\b1\s*[:к]\s*(\d+(?:[.,]\d+)?)/) {
            next.takeR = v; next.takePct = nil; hit = true
        }
        if q.contains(/как\s+учат|как\s+советуют|по\s+правилам|как\s+(?:лучше|правильно|положено)|по\s+уму|грамотн|риск[- ]?менеджмент|by\s+the\s+(?:book|rules)|best\s+practice|properly|risk[- ]?management|like\s+(?:the\s+)?pros/) {
            next.autoRules = true; hit = true
        }
        // «вход на откате 2%», «подожди откат 2%»; «лимит 2650», «вход 2650»; «по рынку» clears both. After the word at
        // most one short word, so «on a 2% pullback trailing 3%» is a 2% pullback and «stop 2% pullback 3%» a 3% one.
        if let v = number(q, /(?:откат\w*|pullback|dip|retrace\w*)[^\d\p{L}]*(?:\p{L}{1,3}[^\d\p{L}]+)?(\d+(?:[.,]\d+)?)\s*%/)
            ?? number(q, /(\d+(?:[.,]\d+)?)\s*%\s*(?:pullback|dip|retrace\w*)/) {
            next.pullbackPct = v; next.entryPx = nil; hit = true
        } else if let v = number(q, /(?:лимит\w*|вход\w*|limit|entry|enter)\s*(?:по|на|at)?\s*\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)/) {
            next.entryPx = v; next.pullbackPct = nil; hit = true
        }
        if q.contains(/по\s+рынку|market/) { next.entryPx = nil; next.pullbackPct = nil; hit = true }
        if q.contains(/частям|частичн|scale\s*out|in\s+parts|partial/) { next.partialTake = true; hit = true }
        if let v = number(q, /(?:трейл\w*|скользящ\w*|trail\w*)(?:[- ]?(?:стоп\w*|stop))?[^\d]{0,6}?(\d+(?:[.,]\d+)?)\s*%/) { next.trailingPct = v; hit = true }
        if hedging {
            // Only a coin named: «хедж мне что-нибудь» is the model's to pick from what the wallet holds.
            guard named else { return nil }
            next.side = .short
            // «hedge SOL 30%», «хеджируй 30% эфира»: a share; «hedge 0.1 SOL»: coins; «half», else all of it.
            if let v = number(q, /(?:hedg|хедж)\w*\s+(?:\S+\s+)?(\d+(?:[.,]\d+)?)\s*%/), v > 0, v <= 100 { next.hedgeShare = v / 100 }
            else if let v = number(q, /(?:hedg|хедж)\w*\s+(\d+(?:[.,]\d+)?)(?![\d.,]|\s*%)/) { next.hedge = v }
            else { next.hedgeShare = q.contains(/\bhalf\b|половин/) ? Decimal(string: "0.5")! : 1 }
            hit = true
        }
        // A hedge is sized by the coins and has no take: a risk, a take, money in the trade or a limit said for one would
        // be listed as a change and then do nothing.
        if next.isHedge {
            next.riskUSD = nil; next.takePct = nil; next.takeR = nil; next.amountUSDC = nil; next.autoRules = nil
            next.entryPx = nil; next.pullbackPct = nil; next.partialTake = nil; next.trailingPct = nil
        }

        return hit ? next : nil
    }

    /// Wording the parser cannot turn into numbers («тейк ближе», «плечо поменьше»). A message with it goes to
    /// the model whole, even if the parser recognised another part, so no part of it is silently dropped.
    public static func needsModel(_ text: String) -> Bool {
        text.lowercased().contains(/ближе|дальше|больше|меньше|выше|ниже|шире|увелич|уменьш|удво|вдвое|в \d+ раза|половин|чуть|немного|closer|further|wider|tighter|double|half|bigger|smaller|higher|lower|a bit|slightly/)
    }

    /// The market the text names («close ETH», «закрой нефть»), or nil when it names none.
    public static func namedAsset(_ text: String, markets: Set<String> = []) -> String? {
        var probe = TradeIntent()
        probe.asset = ""
        guard let found = apply(text, to: probe, markets: markets)?.asset, !found.isEmpty else { return nil }
        return found
    }

    /// A question to answer, not an order to carry out: «а где наш шорт по нефти?», «what did you do with ETH».
    public static func isQuestion(_ text: String) -> Bool {
        let q = text.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        if q.hasSuffix("?") { return true }
        return q.contains(/^(?:а\s+|и\s+|so\s+|and\s+)?(?:где|что|сколько|почему|зачем|когда|какой|какая|какие|каким|куда\s+дел|кто|откуда|what|where|why|when|which|who|how\s+(?:much|many|is|are|did|was|were|do|come)|is\s+(?:my|the|there)|are\s+(?:my|there)|did\s+(?:you|i|we)|do\s+i|have\s+(?:i|you|we))\b/)
    }

    /// Whether the text has a trading word in Russian or English. Without one, a ticker and an amount alone
    /// («compra SOL por 1 USDC») are too weak to trust: the message may be another language asking for something else.
    public static func hasTradeWords(_ text: String) -> Bool {
        text.lowercased().contains(/лонг|шорт|\blong\b|\bshort\b|\bhedg|хедж|риск|\brisk\b|стоп|\bstop|тейк|\btake\b|\btp\b|\bsl\b|плеч|leverage|\d\s*[xх×]\b|откат|pullback|лимит|limit|по\s+рынку|market|как\s+учат|by\s+the\s+(?:book|rules)|трейл|trail|частям|in\s+parts|scale\s*out|\d\s*r\b|\b1\s*[:к]\s*\d/)
    }

    /// Numbers as the parsers read them: «2,500» and «1,234.56» lose their thousands commas and «.5» becomes «0.5»; a
    /// decimal comma («1,5», «0,500», «33,333%») stays for the parsers to read as a point, as AlertCommand.level does.
    static func plainNumbers(_ text: String) -> String {
        text.replacing(/(^|[^\d.,])([1-9]\d{0,2}(?:,\d{3})+)(?!,?\d|\s*%)/) { $0.1 + $0.2.replacingOccurrences(of: ",", with: "") }
            .replacing(/(^|[^\w.,])\.(?=\d)/) { $0.1 + "0." }
    }

    static func number(_ q: String, _ regex: Regex<(Substring, Substring)>) -> Decimal? {
        guard let m = q.firstMatch(of: regex) else { return nil }
        return Decimal(string: m.1.replacingOccurrences(of: ",", with: "."))
    }
}

/// Blocks a recovery phrase before it reaches the model, the chat history or a log.
public enum SeedGuard {
    /// True when the text has 12+ consecutive BIP-39 words. Numbers and punctuation between them
    /// («1. abandon 2. ability …») do not break the run; any other word does.
    public static func containsMnemonic(_ text: String) -> Bool {
        var run = 0
        for word in text.lowercased().split(whereSeparator: { !$0.isLetter }) {
            run = BIP39.index[String(word)] != nil ? run + 1 : 0
            if run >= 12 { return true }
        }
        return false
    }

    /// True when the text holds a private key: a Solana keypair — 64 bytes in Base58 or as a JSON array, its second half the
    /// first half's public key, so a transaction's signature (64 bytes too) isn't one — or 32 bytes of hex (an EVM key)
    /// with a word for a key beside it: alone, 64 hex digits are as likely a transaction hash or a NEAR account.
    /// ponytail: an EVM key pasted with no such word passes; nothing in the hex itself tells it from a hash.
    public static func containsPrivateKey(_ text: String) -> Bool {
        func keypair(_ b: Data) -> Bool {
            b.count == 64 && (try? Curve25519.Signing.PrivateKey(rawRepresentation: b.prefix(32)))?.publicKey.rawRepresentation == b.suffix(32)
        }
        for m in text.matches(of: /\[\s*\d{1,3}(?:\s*,\s*\d{1,3}){63}\s*\]/) {
            let bytes = m.output.split(whereSeparator: { !$0.isNumber }).compactMap { UInt8($0) }
            if bytes.count == 64, keypair(Data(bytes)) { return true }
        }
        let keyWord = text.lowercased().contains(/private|secret|\bkey\b|приват|секрет|ключ/)
        for word in text.split(whereSeparator: { !$0.isLetter && !$0.isNumber }) {
            let hex = word.hasPrefix("0x") ? word.dropFirst(2) : word
            if keyWord, hex.count == 64, hex.allSatisfy(\.isHexDigit) { return true }
            if (86...88).contains(word.count), let b = try? Base58.decode(String(word)), keypair(b) { return true }
        }
        return false
    }
}

public enum Fmt {
    static let locale = Locale(identifier: "en_US")
    public static func usd(_ d: Decimal) -> String {
        let text = abs(d).formatted(.number.precision(.fractionLength(2)).locale(locale))
        return (d < 0 && text != "0.00" ? "−$" : "$") + text
    }
    public static func num(_ d: Decimal, maxFraction: Int = 2) -> String {
        // A coin amount (6+ places) keeps four significant digits however small: 0.00001196 UBTC, not 0.000012 — the
        // card's rate, worked out from the shown amount, came out 9% off.
        var fraction = maxFraction
        if maxFraction >= 6, d != 0 {
            var scaled = abs(d) * pow(10, maxFraction)
            while scaled < 1000, fraction < 18 { scaled *= 10; fraction += 1 }
        }
        return d.formatted(.number.precision(.fractionLength(0...fraction)).locale(locale))
    }
    /// Said on a swap's card when fees take more than a tenth of what's paid (2026-10-08: $0.18 of a $1 UBTC buy went to
    /// Relay): «fees take ≈ $0.18 (18%)». `worth` is what arrives in dollars after the fees and gas.
    public static func feeWarning(paid: Decimal, worth: Decimal) -> String? {
        guard paid > 0, worth > 0, paid - worth > paid / 10 else { return nil }
        return "⚠️ fees take ≈ \(usd(paid - worth)) (\(num((paid - worth) / paid * 100, maxFraction: 0))%): a bigger amount loses less"
    }
    public static func price(_ d: Decimal) -> String { d.formatted(.number.precision(.significantDigits(1...6)).locale(locale)) }
    public static func pct(_ d: Decimal) -> String { num(d) + "%" }
    /// «28 сент., 22:38»: no year, so a non-Gregorian system calendar does not show «2569 г. BE».
    public static func date(_ d: Date) -> String { d.formatted(Date.FormatStyle(locale: locale).day().month(.abbreviated).hour().minute()) }
    /// «23:48» — for the second end of a span on the same day.
    public static func time(_ d: Date) -> String { d.formatted(Date.FormatStyle(locale: locale).hour().minute()) }
    /// A shown line with every number in it but a percentage as dots («worth ••• · +4.2%»): the balance hidden.
    public static func dots(_ text: String) -> String { text.replacing(/[−+-]?\$?\d[\d,]*(?:\.\d+)?(?![\d.,]*%)/, with: "•••") }
}

extension Decimal {
    public func rounded(_ scale: Int, _ mode: NSDecimalNumber.RoundingMode) -> Decimal {
        var value = self, result = Decimal()
        NSDecimalRound(&result, &value, scale, mode)
        return result
    }
}
