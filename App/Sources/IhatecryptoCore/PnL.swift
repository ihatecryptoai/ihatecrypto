import Foundation

/// Profit and loss, from what is known for sure and what is only approximate:
/// - trading (exact, from Hyperliquid itself): realized P/L of the fills in the period after fees, funding paid or
///   received, and the unrealized P/L of the positions open now;
/// - the portfolio's change: its value now minus its value when the period began, minus money that came in or left
///   (top-ups, transfers out) — prices, lending yield and trading all end up in it.
/// Lending yield isn't tracked on its own: it is inside the portfolio change.
public enum PnL {
    public enum Period: String, CaseIterable, Sendable {
        case day, week, month, all

        public var title: String { switch self { case .day: "24 h"; case .week: "7 days"; case .month: "30 days"; case .all: "all time" } }
        public func since(_ now: Date) -> Date? {
            switch self {
            case .day: now.addingTimeInterval(-86_400)
            case .week: now.addingTimeInterval(-7 * 86_400)
            case .month: now.addingTimeInterval(-30 * 86_400)
            case .all: nil
            }
        }
    }

    /// Money that came into the wallet from outside (+) or left it (−), in dollars at the time. Kept per wallet.
    public struct Flow: Codable, Sendable, Equatable {
        public let at: Date
        public let usd: Decimal
        public let note: String
        public let wallet: String
        public init(at: Date, usd: Decimal, note: String, wallet: String) {
            self.at = at; self.usd = usd; self.note = note; self.wallet = wallet
        }
    }

    /// A holding's amount and price at one moment: what the portfolio's P/L line is counted from.
    public struct Mark: Codable, Sendable, Equatable {
        public let amount: Decimal
        public let price: Decimal
        public init(amount: Decimal, price: Decimal) { self.amount = amount; self.price = price }
    }

    /// What prices did to the holdings between two moments: each amount held at the first, times how far its price moved.
    /// Money that came in or left, swaps and bridges change amounts, not prices, so the line doesn't jump with them; a
    /// coin missing at either moment (sold, or not read) adds nothing.
    // ponytail: funding, fees, slippage and lending yield aren't price moves, so they're left out; add them from
    // Hyperliquid's ledger and the vaults' rates if the line drifts from the P/L board.
    public static func priceEffect(from a: [String: Mark], to b: [String: Mark]) -> Decimal {
        a.reduce(0) { sum, held in b[held.key].map { sum + held.value.amount * ($0.price - held.value.price) } ?? sum }
    }

    /// A funding payment on a Hyperliquid position: − paid, + received.
    public struct Funding: Sendable, Equatable {
        public let time: Date
        public let coin: String
        public let usdc: Decimal
        public init(time: Date, coin: String, usdc: Decimal) { self.time = time; self.coin = coin; self.usdc = usdc }
    }

    public struct AssetLine: Sendable, Equatable {
        public let coin: String
        public let realized: Decimal
        public let closedTrades: Int
    }

    public struct TradeLine: Sendable, Equatable {
        public let coin: String
        public let long: Bool
        public let closed: Date
        public let realized: Decimal
    }

    public struct Report: Sendable, Equatable {
        public let period: Period
        public let coin: String?
        /// Closed P/L of the period's fills minus all their fees.
        public let realized: Decimal
        public let funding: Decimal
        /// Positions open now (the period doesn't apply).
        public let unrealized: Decimal
        /// Portfolio value change without money moved in or out; nil without history for the period (or for one asset).
        public let change: Decimal?
        /// From when the change is measured: the period's start, or the oldest point kept when history is shorter.
        public let changeSince: Date?
        /// Net money moved in (+) or out (−) during the change's span.
        public let flows: Decimal
        public let byAsset: [AssetLine]
        public let closedTrades: Int
        public let wins: Int
        public let best: TradeLine?
        public let worst: TradeLine?

        /// What trading made in the period: realized after fees, plus funding.
        public var trading: Decimal { realized + funding }
    }

    public static func report(_ period: Period, coin: String? = nil, now: Date = Date(), fills: [TradeHistory.Fill],
                              funding: [Funding], unrealized: [String: Decimal], equity: [(at: Date, usd: Decimal)], flows: [Flow]) -> Report {
        let since = period.since(now)
        func inPeriod(_ d: Date) -> Bool { since.map { d >= $0 } ?? true }
        func forCoin(_ c: String) -> Bool { coin.map { $0 == c } ?? true }

        let periodFills = fills.filter { inPeriod($0.time) && forCoin($0.coin) }
        let realized = periodFills.reduce(Decimal(0)) { $0 + $1.closedPnl - $1.fee }
        let paid = funding.filter { inPeriod($0.time) && forCoin($0.coin) }.reduce(Decimal(0)) { $0 + $1.usdc }
        let open = unrealized.filter { forCoin($0.key) }.values.reduce(Decimal(0), +)

        let closed = TradeHistory.stories(fills).filter { !$0.isOpen && forCoin($0.coin) && $0.closed.map(inPeriod) == true }
        let lines = closed.map { TradeLine(coin: $0.coin, long: $0.long, closed: $0.closed ?? now, realized: $0.realized) }
        let byAsset = Dictionary(grouping: periodFills, by: \.coin).map { coin, fs in
            AssetLine(coin: coin, realized: fs.reduce(Decimal(0)) { $0 + $1.closedPnl - $1.fee },
                      closedTrades: closed.filter { $0.coin == coin }.count)
        }
        .sorted { abs($0.realized) > abs($1.realized) }

        // The portfolio change needs the whole wallet: not for one asset.
        var change: Decimal?, changeSince: Date?, moved: Decimal = 0
        let points = equity.sorted { $0.at < $1.at }
        if coin == nil, let end = points.last, let start = points.first(where: { inPeriod($0.at) }), start.at < end.at {
            moved = flows.filter { $0.at > start.at && $0.at <= end.at }.reduce(Decimal(0)) { $0 + $1.usd }
            change = end.usd - start.usd - moved
            changeSince = start.at
        }

        return Report(period: period, coin: coin, realized: realized, funding: paid, unrealized: open, change: change,
                      changeSince: changeSince, flows: moved, byAsset: byAsset, closedTrades: lines.count,
                      wins: lines.filter { $0.realized > 0 }.count,
                      best: lines.max { $0.realized < $1.realized }, worst: lines.min { $0.realized < $1.realized })
    }

    /// For the chat's answers: the facts, one line each, plainly labeled.
    public static func facts(_ r: Report, pretty: (String) -> String = { $0 }) -> [String] {
        func s(_ d: Decimal) -> String { d < 0 ? "−$\(Fmt.num(-d))" : "+$\(Fmt.num(d))" }
        var out = ["P/L \(r.period.title)\(r.coin.map { " on \(pretty($0))" } ?? ""): trading \(s(r.trading)) (realized after fees \(s(r.realized)), funding \(s(r.funding))), \(r.closedTrades) closed trades, \(r.wins) in profit; open positions now \(s(r.unrealized))."]
        if let c = r.change, let since = r.changeSince {
            out.append("Portfolio \(r.period.title): \(s(c)) since \(Fmt.date(since)), not counting \(s(r.flows)) of money moved in or out (lending yield and price moves are inside this).")
        }
        if let b = r.best, b.realized > 0 { out.append("Best trade: \(b.long ? "long" : "short") \(pretty(b.coin)) \(s(b.realized)).") }
        if let w = r.worst, w.realized < 0 { out.append("Worst trade: \(w.long ? "long" : "short") \(pretty(w.coin)) \(s(w.realized)).") }
        return out
    }
}

/// «какой у меня P/L», «pnl за неделю», «сколько я заработал на нефти», «profit and loss week for oil».
public struct PnLCommand: Equatable, Sendable {
    public let period: PnL.Period
    public let coin: String?
    public init(period: PnL.Period, coin: String?) { self.period = period; self.coin = coin }

    public static func parse(_ text: String, markets: Set<String> = []) -> PnLCommand? {
        let q = text.lowercased()
        guard q.contains(/profit\s+and\s+loss|\bp\s*[\/&]\s*l\b|\bpnl\b|пнл|прибыл\w*\s+и\s+убыт|сколько\s+(?:я\s+)?(?:заработал|потерял|наторговал)|how\s+much\s+(?:did\s+)?i\s+(?:make|made|earn|earned|lose|lost)/) else {
            return nil
        }
        let period: PnL.Period = q.contains(/today|24\s*h|\bday\b|сегодня|за\s+день|сутк|суток/) ? .day
            : q.contains(/week|7\s*d\b|недел/) ? .week : q.contains(/month|30\s*d\b|месяц/) ? .month : .all
        // A ticker no market or holding knows («pnl по AERO» with no AERO left) is still the asset asked about: the answer
        // says there's none, not the whole portfolio's P/L.
        let named = IntentParser.namedAsset(text, markets: markets) ?? text.firstMatch(of: /(?i)\b(?:for|on|по|на)\s+([a-z][a-z0-9]{1,9})\b/)
            .map { String($0.1).uppercased() }.flatMap { notTickers.contains($0) ? nil : $0 }
        return PnLCommand(period: period, coin: named)
    }

    static let notTickers: Set<String> = ["THE", "MY", "AN", "ALL", "THIS", "LAST", "PAST", "WEEK", "WEEKS", "TODAY", "DAY", "DAYS", "MONTH",
                                          "MONTHS", "YEAR", "TIME", "EVERYTHING", "TOTAL", "IT", "ME", "EACH", "EVERY",
                                          // Networks and places: «pnl on base» is the network, not a token.
                                          "BASE", "SOLANA", "ARBITRUM", "ETHEREUM", "POLYGON", "ROBINHOOD", "TEMPO", "KAMINO", "JUPITER"]
}

/// What a held token cost, for the P/L on its holding row: the average cost. A buy adds its tokens and dollars; tokens
/// that came in unpaid (a transfer, a bridge, an airdrop) add theirs at the price that day (`came`); a sale or a send
/// takes out the same share of all of it. Only tokens no price reached count for nothing in the P/L.
public struct CostBasis: Codable, Equatable, Sendable {
    public var amount: Decimal = 0
    public var usd: Decimal = 0
    /// When the first of what's still held came in («since you bought on Oct 10»); nil in records kept before it.
    public var since: Date?
    /// Of `amount` and `usd`: what came in unpaid, at the price the day it came; nil when all of it was bought.
    public var received: Decimal?
    public var receivedUSD: Decimal?

    public init() {}

    public mutating func bought(_ tokens: Decimal, for dollars: Decimal, at date: Date = Date()) {
        guard tokens > 0, dollars > 0 else { return }
        if amount == 0 { since = date }
        amount += tokens
        usd += dollars
    }

    /// Tokens that came in unpaid, counted at what they were worth that day.
    public mutating func came(_ tokens: Decimal, worth dollars: Decimal, at date: Date = Date()) {
        guard tokens > 0, dollars > 0 else { return }
        bought(tokens, for: dollars, at: date)
        received = (received ?? 0) + tokens
        receivedUSD = (receivedUSD ?? 0) + dollars
    }

    /// What one of it cost on average.
    public var price: Decimal? { amount > 0 ? usd / amount : nil }

    public mutating func sold(_ tokens: Decimal) {
        guard amount > 0, tokens > 0 else { return }
        let left = max(0, amount - tokens), before = amount
        usd = usd * left / before
        received = received.map { $0 * left / before }
        receivedUSD = receivedUSD.map { $0 * left / before }
        amount = left
        if left == 0 { (since, received, receivedUSD) = (nil, nil, nil) }
    }

    /// One change in what a wallet holds of a token: tokens in (bought for `paid` dollars, or unpaid: nil) or out.
    public struct Change: Sendable, Equatable {
        public let at: Date
        public let tokens: Decimal
        public let paid: Decimal?
        public init(at: Date, tokens: Decimal, paid: Decimal? = nil) { (self.at, self.tokens, self.paid) = (at, tokens, paid) }
    }

    /// The cost of `held` from its changes, oldest first: tokens bought, at what was paid; tokens in unpaid, at `worth`
    /// then; tokens out take their share. What was held before the oldest change (the chain gives the newest 50) counts
    /// at `worth` then too.
    public static func fold(_ changes: [Change], held: Decimal, worth: (Date) -> Decimal?) -> CostBasis {
        var basis = CostBasis()
        let before = held - changes.reduce(0) { $0 + $1.tokens }
        if before > 0, let first = changes.first, let px = worth(first.at) { basis.came(before, worth: before * px, at: first.at) }
        for c in changes {
            if c.tokens < 0 { basis.sold(-c.tokens) }
            else if let paid = c.paid, paid > 0 { basis.bought(c.tokens, for: paid, at: c.at) }
            else if let px = worth(c.at) { basis.came(c.tokens, worth: c.tokens * px, at: c.at) }
        }
        return basis
    }

    /// The days `fold` asks a price for: the oldest change's and every unpaid one's.
    public static func unpaid(_ changes: [Change]) -> [Date] {
        Array(Set(changes.prefix(1).map(\.at) + changes.filter { $0.tokens > 0 && ($0.paid ?? 0) <= 0 }.map(\.at))).sorted()
    }

    /// The P/L of what's held now at `price`: only the tokens with a known cost (at most what was bought here).
    public func pnl(held: Decimal, price: Decimal) -> (usd: Decimal, pct: Decimal)? {
        guard amount > 0, usd > 0, held > 0 else { return nil }
        let counted = min(held, amount)
        let cost = usd * counted / amount
        return (counted * price - cost, (counted * price - cost) / cost * 100)
    }

    /// Dollars, as far as a cost goes.
    static let stablecoins = Stablecoins.dollars

    /// An EVM wallet's changes in a token, from Blockscout's token transfers of each transaction that moved it, oldest
    /// first: tokens in with stablecoins leaving the wallet in the same transaction were bought for them; with no
    /// dollars out (a transfer, an airdrop, a cross-chain order's fill) they came in unpaid; tokens out were sold or sent.
    public static func replay(wallet: String, contract: String, transactions: [[[String: Any]]]) -> [Change] {
        let token = contract.lowercased()
        var out: [Change] = []
        for transfers in transactions {
            let tokens = moved(transfers, wallet: wallet).filter { $0.contract == token }.reduce(0) { $0 + $1.amount }
            let dollars = dollarsOut(transfers, wallet: wallet)
            let at = (transfers.first?["timestamp"] as? String).flatMap { try? Date($0, strategy: .iso8601) } ?? Date()
            if tokens != 0 { out.append(Change(at: at, tokens: tokens, paid: tokens > 0 && dollars > 0 ? dollars : nil)) }
        }
        return out
    }

    /// The dollars (stablecoins) that left the wallet in one transaction's Blockscout token transfers, net of any back.
    public static func dollarsOut(_ transfers: [[String: Any]], wallet: String) -> Decimal {
        -moved(transfers, wallet: wallet).filter { stablecoins.contains($0.symbol) }.reduce(0) { $0 + $1.amount }
    }

    /// Each transfer as the wallet saw it: the token (lowercased), its symbol (uppercased), and the amount in (+) or out (−).
    static func moved(_ transfers: [[String: Any]], wallet: String) -> [(contract: String, symbol: String, amount: Decimal)] {
        let me = wallet.lowercased()
        return transfers.compactMap { t in
            guard let info = t["token"] as? [String: Any], let raw = Holdings.number((t["total"] as? [String: Any])?["value"]),
                  let decimals = Int(info["decimals"] as? String ?? "") else { return nil }
            let side = { (key: String) in ((t[key] as? [String: Any])?["hash"] as? String)?.lowercased() }
            let sign: Decimal = side("to") == me ? 1 : side("from") == me ? -1 : 0
            return ((info["address_hash"] as? String ?? "").lowercased(), (info["symbol"] as? String ?? "").uppercased(), sign * raw / pow(10, decimals))
        }
    }

    /// One transaction as a Solana wallet saw it: when, what it gained or lost of each mint, and of SOL (the fee given
    /// back when it paid it). From `getTransaction` (jsonParsed); nil for a failed one.
    public struct SolanaMove: Sendable, Equatable {
        public let at: Date
        public let change: [String: Decimal]
        public let sol: Decimal

        public init(at: Date, change: [String: Decimal], sol: Decimal) { (self.at, self.change, self.sol) = (at, change, sol) }

        public init?(_ tx: [String: Any], owner: String) {
            guard let meta = tx["meta"] as? [String: Any], meta["err"] == nil || meta["err"] is NSNull else { return nil }
            var change: [String: Decimal] = [:]
            for (key, sign) in [("postTokenBalances", Decimal(1)), ("preTokenBalances", Decimal(-1))] {
                for b in meta[key] as? [[String: Any]] ?? [] where b["owner"] as? String == owner {
                    guard let m = b["mint"] as? String, let ui = b["uiTokenAmount"] as? [String: Any],
                          let raw = Holdings.number(ui["amount"]), let decimals = (ui["decimals"] as? NSNumber)?.intValue else { continue }
                    change[m, default: 0] += sign * raw / pow(10, decimals)
                }
            }
            let keys = ((tx["transaction"] as? [String: Any])?["message"] as? [String: Any])?["accountKeys"] as? [[String: Any]] ?? []
            var sol: Decimal = 0
            if let i = keys.firstIndex(where: { $0["pubkey"] as? String == owner }), let pre = meta["preBalances"] as? [Any], let post = meta["postBalances"] as? [Any],
               i < pre.count, i < post.count, let before = Holdings.number(pre[i]), let after = Holdings.number(post[i]) {
                sol = (after - before + (i == 0 ? Holdings.number(meta["fee"]) ?? 0 : 0)) / 1_000_000_000
            }
            self.init(at: (tx["blockTime"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue) } ?? Date(), change: change, sol: sol)
        }
    }

    public static let wsol = "So11111111111111111111111111111111111111112"

    /// A Solana wallet's changes in `mint` from its own transactions, oldest first: tokens in against dollars (Solana's
    /// stablecoins) out in the same transaction, or else SOL out (wrapped too) at `solPrice` then, were bought; with
    /// nothing paid (a transfer, an airdrop) they came in unpaid; tokens out were sold or sent. SOL is `wsol`: the
    /// wallet's own lamports and its wrapped SOL together.
    public static func replaySolana(mint: String, moves: [SolanaMove], solPrice: (Date) -> Decimal?) -> [Change] {
        let stables = Set((Stablecoins.byNetwork["solana"] ?? []).map(\.address))
        return moves.compactMap { m in
            let tokens = (m.change[mint] ?? 0) + (mint == wsol ? m.sol : 0)
            guard tokens > 0 else { return tokens < 0 ? Change(at: m.at, tokens: tokens) : nil }
            let dollars = -m.change.filter { stables.contains($0.key) && $0.value < 0 }.values.reduce(0, +)
            let sol = mint == wsol ? 0 : m.sol + min(0, m.change[wsol] ?? 0)
            let paid = dollars > 0 ? dollars : sol < 0 ? solPrice(m.at).map { -sol * $0 } : nil
            return Change(at: m.at, tokens: tokens, paid: paid)
        }
    }
}
