import Foundation

/// What happened on Hyperliquid, from the exchange itself: every fill, grouped into position stories
/// (open → changes → close), and the money that came in or went out.
public enum TradeHistory {
    public struct Fill: Decodable, Sendable, Equatable {
        public let coin: String
        public let px: Decimal
        public let sz: Decimal
        /// "B" buy, "A" sell.
        public let side: String
        public let time: Date
        /// Signed position size before this fill.
        public let startPosition: Decimal
        /// «Open Long», «Close Short», …
        public let dir: String
        public let closedPnl: Decimal
        public let fee: Decimal
        public let hash: String?

        enum Keys: String, CodingKey { case coin, px, sz, side, time, startPosition, dir, closedPnl, fee, hash }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            func dec(_ k: Keys) throws -> Decimal { Decimal(string: try c.decode(String.self, forKey: k)) ?? 0 }
            coin = try c.decode(String.self, forKey: .coin)
            px = try dec(.px); sz = try dec(.sz); startPosition = try dec(.startPosition); closedPnl = try dec(.closedPnl); fee = try dec(.fee)
            side = try c.decode(String.self, forKey: .side)
            dir = try c.decode(String.self, forKey: .dir)
            time = Date(timeIntervalSince1970: TimeInterval(try c.decode(Int64.self, forKey: .time)) / 1000)
            hash = try c.decodeIfPresent(String.self, forKey: .hash)
        }

        public init(coin: String, px: Decimal, sz: Decimal, side: String, time: Date, startPosition: Decimal, dir: String,
                    closedPnl: Decimal = 0, fee: Decimal = 0, hash: String? = nil) {
            self.coin = coin; self.px = px; self.sz = sz; self.side = side; self.time = time
            self.startPosition = startPosition; self.dir = dir; self.closedPnl = closedPnl; self.fee = fee; self.hash = hash
        }

        /// Position size after this fill.
        public var endPosition: Decimal { startPosition + (side == "B" ? sz : -sz) }
    }

    /// One position from the first fill to flat again (or still open).
    public struct Story: Identifiable, Sendable, Equatable {
        public var id: String { "\(coin)-\(opened.timeIntervalSince1970)" }
        public let coin: String
        public let long: Bool
        public let fills: [Fill]
        public var opened: Date { fills.first?.time ?? .distantPast }
        public var closed: Date? { isOpen ? nil : fills.last?.time }
        /// A flip's fill (long straight to short) closes it: what's left of that fill opens the next story.
        public var isOpen: Bool {
            guard let last = fills.last else { return false }
            let end = last.endPosition
            return end != 0 && !(last.startPosition != 0 && (last.startPosition > 0) != (end > 0))
        }
        /// Realized result after fees.
        public var realized: Decimal { fills.reduce(0) { $0 + $1.closedPnl } - fees }
        public var fees: Decimal { fills.reduce(0) { $0 + $1.fee } }
        /// The largest size the position reached.
        public var peakSize: Decimal { fills.map { abs($0.endPosition) }.max() ?? 0 }
    }

    /// Fills in any order → stories, newest first. A story starts when a coin's position leaves zero and ends when
    /// it comes back to zero; a flip (long straight to short) closes one story and opens the next.
    public static func stories(_ fills: [Fill]) -> [Story] {
        var open: [String: [Fill]] = [:]
        var out: [Story] = []
        for f in fills.sorted(by: { $0.time < $1.time }) {
            var current = open[f.coin] ?? []
            current.append(f)
            let end = f.endPosition
            if end == 0 || (f.startPosition != 0 && (f.startPosition > 0) != (end > 0)) {
                let long = (current.first?.endPosition ?? 0) > 0
                out.append(Story(coin: f.coin, long: long, fills: current))
                open[f.coin] = end == 0 ? nil : []
                if end != 0 { open[f.coin] = [Fill(coin: f.coin, px: f.px, sz: abs(end), side: f.side, time: f.time, startPosition: 0, dir: end > 0 ? "Open Long" : "Open Short")] }
            } else {
                open[f.coin] = current
            }
        }
        for (coin, fills) in open where !fills.isEmpty {
            out.append(Story(coin: coin, long: (fills.first?.endPosition ?? 0) > 0, fills: fills))
        }
        return out.sorted { $0.opened > $1.opened }
    }

    /// Money in and out of the Hyperliquid account, in words.
    public struct Transfer: Sendable, Identifiable {
        public var id: String { "\(time.timeIntervalSince1970)-\(text)" }
        public let time: Date
        public let text: String
        /// + in, − out, 0 for moves between the account's own balances.
        public let amount: Decimal
    }

    public static func transfers(_ json: Data, user: String) -> [Transfer] {
        guard let rows = try? JSONSerialization.jsonObject(with: json) as? [[String: Any]] else { return [] }
        let me = user.lowercased()
        func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? 0 }
        return rows.compactMap { row in
            guard let delta = row["delta"] as? [String: Any], let type = delta["type"] as? String,
                  let ms = row["time"] as? Double ?? (row["time"] as? Int).map(Double.init) else { return nil }
            let time = Date(timeIntervalSince1970: ms / 1000)
            let from = (delta["user"] as? String)?.lowercased(), to = (delta["destination"] as? String)?.lowercased()
            switch type {
            case "deposit": let a = dec(delta["usdc"]); return Transfer(time: time, text: "Deposit \(Fmt.usd(a))", amount: a)
            case "withdraw": let a = dec(delta["usdc"]); return Transfer(time: time, text: "Withdrawal \(Fmt.usd(a))", amount: -a)
            case "send", "spotTransfer":
                let a = dec(delta["usdcValue"] ?? delta["amount"])
                let sourceDex = delta["sourceDex"] as? String ?? "", destDex = delta["destinationDex"] as? String ?? ""
                if from == me && to == me {
                    let where_ = destDex.isEmpty ? "the main balance" : "the \(destDex) exchange"
                    return Transfer(time: time, text: "Moved \(Fmt.usd(a)) to \(where_)\(sourceDex.isEmpty || sourceDex == "spot" ? "" : " from \(sourceDex)")", amount: 0)
                }
                if to == me { return Transfer(time: time, text: "Received \(Fmt.usd(a)) (bridge in)", amount: a) }
                let fee = dec(delta["fee"])
                return Transfer(time: time, text: "Sent \(Fmt.usd(a)) out (bridge to another chain)\(fee > 0 ? ", fee \(Fmt.usd(fee))" : "")", amount: -a)
            case "accountClassTransfer":
                let a = dec(delta["usdc"])
                return Transfer(time: time, text: "Moved \(Fmt.usd(a)) \((delta["toPerp"] as? Bool) == true ? "from spot to perps" : "from perps to spot")", amount: 0)
            case "vaultDeposit": let a = dec(delta["usdc"]); return Transfer(time: time, text: "Deposited \(Fmt.usd(a)) into a vault", amount: 0)
            case "vaultWithdraw": let a = dec(delta["usdc"] ?? delta["netWithdrawnUsd"]); return Transfer(time: time, text: "Withdrew \(Fmt.usd(a)) from a vault", amount: 0)
            default: return Transfer(time: time, text: type, amount: 0)
            }
        }
        .sorted { $0.time > $1.time }
    }

    public static func fills(user: String) async throws -> [Fill] {
        try JSONDecoder().decode([Fill].self, from: try await info(["type": "userFills", "user": user, "aggregateByTime": true]))
    }

    /// Every fill Hyperliquid still serves (its last 10,000), oldest first, page by page: `userFills` alone gives 2,000.
    public static func allFills(user: String) async throws -> [Fill] {
        var out: [Fill] = [], start: Int64 = 0
        for _ in 0..<6 {
            let page = try JSONDecoder().decode([Fill].self, from: try await info(["type": "userFillsByTime", "user": user, "startTime": start, "aggregateByTime": true]))
            out += page
            guard page.count >= 2000, let last = page.last else { break }
            start = Int64(last.time.timeIntervalSince1970 * 1000) + 1
        }
        return out
    }

    /// All funding payments, 500 a page (`funding(user:)` reads the first page only).
    public static func allFunding(user: String) async throws -> [PnL.Funding] {
        var out: [PnL.Funding] = [], start: Int64 = 0
        for _ in 0..<40 {
            let page = funding(try await info(["type": "userFunding", "user": user, "startTime": start]))
            out += page
            guard page.count >= 500, let last = page.last else { break }
            start = Int64(last.time.timeIntervalSince1970 * 1000) + 1
        }
        return out
    }

    /// Funding paid (−) or received (+) on positions, HIP-3 markets included (userFunding).
    public static func funding(user: String) async throws -> [PnL.Funding] {
        funding(try await info(["type": "userFunding", "user": user, "startTime": 0]))
    }

    public static func funding(_ json: Data) -> [PnL.Funding] {
        guard let rows = try? JSONSerialization.jsonObject(with: json) as? [[String: Any]] else { return [] }
        return rows.compactMap { row in
            guard let delta = row["delta"] as? [String: Any], let coin = delta["coin"] as? String,
                  let usdc = (delta["usdc"] as? String).flatMap({ Decimal(string: $0) }),
                  let ms = row["time"] as? Double ?? (row["time"] as? Int).map(Double.init) else { return nil }
            return PnL.Funding(time: Date(timeIntervalSince1970: ms / 1000), coin: coin, usdc: usdc)
        }
    }

    public static func transfers(user: String) async throws -> [Transfer] {
        transfers(try await info(["type": "userNonFundingLedgerUpdates", "user": user, "startTime": 0]), user: user)
    }

    static func info(_ body: [String: Any]) async throws -> Data {
        var request = URLRequest(url: URL(string: "https://api.hyperliquid.xyz/info")!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        request.timeoutInterval = 20
        return try await URLSession.shared.data(for: request).0
    }
}
