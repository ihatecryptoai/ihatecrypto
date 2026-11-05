import Foundation
import WalletCore

/// What the chat's model learns about a token before it answers (the «token» lookup): by its contract or mint on any
/// chain, or by its name or ticker. Solana tokens come from Jupiter's token list (holders, safety flags, organic
/// volume), every chain from DexScreener (pools, links). Public APIs, no keys. Every address in a report is one an API
/// returned, never the model's.
public enum TokenResearch {
    public static func report(_ query: String) async -> String {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if let evm = q.firstMatch(of: /0x[0-9a-fA-F]{40}(?![0-9a-fA-F])/).map({ String($0.0) }) {
            return await dex(evm) ?? "DexScreener has no pool for \(evm) on any chain: nothing trades it, or it isn't a token."
        }
        // Named whole: its first 40 hex in the report would be an address the chat could chart or call.
        if let long = q.firstMatch(of: /0x[0-9a-fA-F]{41,}/) {
            return "\(long.0) is no token contract (an EVM one is 0x and 40 hex): a transaction's hash, a Sui address or a bytes32."
        }
        if let raw = q.firstMatch(of: /[1-9A-HJ-NP-Za-km-z]{30,60}/).map({ String($0.0) }) {
            var unanswered = false
            for mint in repairs(raw) {
                let note = mint == raw ? nil : "The pasted «\(raw)» is not a valid Solana address (\(raw.count) characters). Without "
                    + (raw.hasPrefix("2F" + mint) ? "the «2F» in front — a link's %2F («/») copied along —" : "the extra characters")
                    + " it is this token: \(mint)."
                async let jupiter = jupiter(mint)
                async let pools = dex(mint)
                var said: String?
                do { said = try await jupiter } catch { unanswered = true }
                let facts = [note, said, await pools].compactMap { $0 }
                if facts.count > (note == nil ? 0 : 1) { return ((unanswered ? [busy] : []) + facts).joined(separator: "\n") }
            }
            if unanswered { return busy + " DexScreener has no pool for \(raw) either." }
            return valid(raw)
                ? "Neither Jupiter nor DexScreener knows \(raw) as a token: it may be a wallet address, or a token with no pool yet."
                : "«\(raw)» is not a valid Solana address, and no near fix of it is a known token. Ask for the address again, copied from the token's page."
        }
        return await search(q)
    }

    /// The address as pasted when it's a valid Solana one; else what it likely was: a link's «%2F» / «2F» dropped from the
    /// front, or a character or two too many at either end.
    public static func repairs(_ s: String) -> [String] {
        if valid(s) { return [s] }
        var out = ["%2F", "2F", "%3A", "3A"].filter(s.hasPrefix).map { String(s.dropFirst($0.count)) }
        for head in 0...2 { for tail in 0...2 where head + tail > 0 { out.append(String(s.dropFirst(head).dropLast(tail))) } }
        var seen = Set<String>()
        return out.filter { valid($0) && seen.insert($0).inserted }
    }

    static func valid(_ address: String) -> Bool { (try? Base58.decode(address))?.count == 32 }

    /// What buying a Solana token by its mint needs, and what to warn about.
    public struct SolanaToken: Equatable, Sendable {
        public let name: String
        public let symbol: String
        public let decimals: Int
        public let token2022: Bool
        public let verified: Bool
        public let liquidityUSD: Decimal?
        public let freezable: Bool
        public let mintable: Bool
        /// % the top holders own.
        public let topHolders: Decimal?

        /// What is risky about it, plainly, worst first.
        public func warnings(russian: Bool = false) -> [String] {
            var out: [String] = []
            if freezable { out.append(russian ? "создатель может заморозить токены держателей (freeze authority включён) — продать может не получиться"
                                              : "its creator can freeze holders' tokens (freeze authority is on), so you might not be able to sell") }
            if mintable { out.append(russian ? "создатель может выпустить ещё (mint authority включён)" : "its creator can print more of it (mint authority is on)") }
            if let l = liquidityUSD, l < 50_000 { out.append(russian ? "пул тонкий (\(usd(l))): даже небольшая продажа двигает цену" : "its pool is thin (\(usd(l))): a small sale moves the price") }
            if !verified { out.append(russian ? "Jupiter его не проверял" : "Jupiter hasn't verified it") }
            if let top = topHolders, top > 50 { out.append(russian ? "у крупнейших держателей \(pct(top, sign: false))" : "the top holders own \(pct(top, sign: false))") }
            return out
        }
    }

    /// What a Token-2022 mint's extensions let its creator do to a holder, from getAccountInfo's jsonParsed answer for the
    /// mint (research E-8): Jupiter's token list doesn't carry them (checked 2026-11-05). Worst first. A 0 % transfer fee
    /// and a hook with no program (PYUSD's, Ondo's) do nothing today and aren't named.
    public static func extensionWarnings(_ account: Any?, russian: Bool = false) -> [String] {
        let info = (((account as? [String: Any])?["value"] as? [String: Any])?["data"] as? [String: Any]).flatMap { ($0["parsed"] as? [String: Any])?["info"] as? [String: Any] }
        var state: [String: [String: Any]] = [:]
        for e in info?["extensions"] as? [[String: Any]] ?? [] { if let name = e["extension"] as? String { state[name] = e["state"] as? [String: Any] ?? [:] } }
        func set(_ v: Any?) -> Bool { v != nil && !(v is NSNull) }
        var out: [String] = []
        if set(state["permanentDelegate"]?["delegate"]) {
            out.append(russian ? "создатель может забрать или сжечь его в любом кошельке, в твоём тоже (permanent delegate)"
                               : "its creator can take or burn it in any wallet, yours too (permanent delegate)")
        }
        if state["nonTransferable"] != nil { out.append(russian ? "его нельзя перевести, а значит и продать (non-transferable)" : "it can't be transferred, so it can't be sold (non-transferable)") }
        if state["pausableConfig"]?["paused"] as? Bool == true {
            out.append(russian ? "переводы сейчас на паузе: продать не получится (pausable)" : "its transfers are paused now, so it can't be sold (pausable)")
        } else if set(state["pausableConfig"]?["authority"]) {
            out.append(russian ? "эмитент может поставить все переводы на паузу (pausable)" : "its issuer can pause all transfers (pausable)")
        }
        if set(state["transferHook"]?["programId"]) {
            out.append(russian ? "каждый перевод запускает программу создателя, она может не дать продать (transfer hook)"
                               : "every transfer runs its creator's program, which can refuse a sale (transfer hook)")
        }
        // A fee with a cap of 0 takes nothing.
        let fee = ["newerTransferFee", "olderTransferFee"].compactMap { key -> Int? in
            guard let f = state["transferFeeConfig"]?[key] as? [String: Any], (f["maximumFee"] as? NSNumber)?.intValue != 0 else { return nil }
            return (f["transferFeeBasisPoints"] as? NSNumber)?.intValue
        }.max() ?? 0
        if fee > 0 {
            let p = String(format: "%g%%", Double(fee) / 100)
            out.append(russian ? "с каждого перевода создатель берёт \(p) (transfer fee)" : "every transfer pays its creator \(p) (transfer fee)")
        }
        return out
    }

    /// Jupiter's token search: keyless on api.jup.ag (lite-api.jup.ag is being retired), 5 requests in 10 s per IP.
    public static let jupiterSearch = "https://api.jup.ag/tokens/v2/search?query="

    /// The Jupiter-verified token trading under a ticker («BONK»; «WIF» finds «$WIF») or named so («SPX6900», whose
    /// ticker is SPX), the most liquid when several do: its mint. Unverified namesakes — the usual fakes — are never picked.
    /// nil: Jupiter answered and has none. Throws: Jupiter didn't answer (`get`).
    public static func solanaMint(symbol: String) async throws -> String? {
        let query = symbol.trimmingCharacters(in: CharacterSet(charactersIn: "$")).addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? symbol
        return solanaMint(try await get(jupiterSearch + query), symbol: symbol)
    }

    static func solanaMint(_ data: Data, symbol: String) -> String? {
        func plain(_ s: String) -> String { s.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$")) }
        // «SPX6900 (Wormhole)»: the bridge in brackets isn't part of the name people say.
        func name(_ s: String) -> String { plain(s.replacing(/\s*\([^)]*\)\s*$/, with: "")) }
        let tokens = (try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]) ?? []
        return tokens.filter { t in
            (t["isVerified"] as? Bool == true || (t["tags"] as? [String] ?? []).contains("verified"))
                && (plain(t["symbol"] as? String ?? "") == plain(symbol) || name(t["name"] as? String ?? "") == plain(symbol))
        }.max { (dec($0["liquidity"]) ?? 0) < (dec($1["liquidity"]) ?? 0) }?["id"] as? String
    }

    /// nil: Jupiter answered and doesn't know the mint. Throws: Jupiter didn't answer (`get`).
    public static func solanaToken(_ mint: String) async throws -> SolanaToken? {
        solanaToken(try await get(jupiterSearch + mint), mint: mint)
    }

    static func solanaToken(_ data: Data, mint: String) -> SolanaToken? {
        guard let t = (try? JSONSerialization.jsonObject(with: data) as? [[String: Any]])?.first(where: { $0["id"] as? String == mint }),
              let decimals = (t["decimals"] as? NSNumber)?.intValue else { return nil }
        let audit = t["audit"] as? [String: Any] ?? [:]
        let tags = t["tags"] as? [String] ?? []
        let verified = t["isVerified"] as? Bool == true || tags.contains("verified")
        return SolanaToken(name: Holding.plain(t["name"] as? String ?? "?", max: 32), symbol: Holding.plain(t["symbol"] as? String ?? "?"), decimals: decimals,
                           token2022: t["tokenProgram"] as? String == "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" || tags.contains("token-2022"),
                           verified: verified, liquidityUSD: dec(t["liquidity"]), freezable: audit["freezeAuthorityDisabled"] as? Bool == false,
                           mintable: audit["mintAuthorityDisabled"] as? Bool == false, topHolders: dec(audit["topHoldersPercentage"]))
    }

    // MARK: Sources

    static func jupiter(_ mint: String) async throws -> String? {
        jupiterFacts(try await get(jupiterSearch + mint), mint: mint)
    }

    /// For the model when Jupiter's search didn't answer: whether a token is the verified one isn't known.
    static let busy = "Jupiter didn't answer just now, so whether it knows or verified this token isn't known: ask again in a minute."

    /// Jupiter's entry for the mint, as lines.
    static func jupiterFacts(_ data: Data, mint: String, now: Date = Date()) -> String? {
        guard let t = (try? JSONSerialization.jsonObject(with: data) as? [[String: Any]])?.first(where: { $0["id"] as? String == mint }) else { return nil }
        let audit = t["audit"] as? [String: Any] ?? [:]
        let tags = t["tags"] as? [String] ?? []
        let verified = t["isVerified"] as? Bool == true || tags.contains("verified")
        var lines = ["Solana token \(Holding.plain(t["name"] as? String ?? "?", max: 32)) (\(Holding.plain(t["symbol"] as? String ?? "?"))), mint \(mint), chain id solana. Jupiter: "
                     + (verified ? "verified" : "not verified") + (tags.isEmpty ? "" : ", tags \(tags.joined(separator: ", "))") + "."]
        lines.append("Price \(price(dec(t["usdPrice"]))), market cap \(usd(dec(t["mcap"]))), FDV \(usd(dec(t["fdv"]))), liquidity \(usd(dec(t["liquidity"])))"
                     + ((t["holderCount"] as? NSNumber).map { ", \($0.intValue) holders" } ?? "") + ".")
        let windows = [("5m", "stats5m"), ("1h", "stats1h"), ("6h", "stats6h"), ("24h", "stats24h")]
        let changes = windows.compactMap { title, key in dec((t[key] as? [String: Any])?["priceChange"]).map { "\(title) \(pct($0))" } }
        if !changes.isEmpty { lines.append("Price change: " + changes.joined(separator: ", ") + ".") }
        if let day = t["stats24h"] as? [String: Any] {
            let volume = (dec(day["buyVolume"]) ?? 0) + (dec(day["sellVolume"]) ?? 0)
            let organic = (dec(day["buyOrganicVolume"]) ?? 0) + (dec(day["sellOrganicVolume"]) ?? 0)
            lines.append("Last 24h: \(int(day["numBuys"])) buys, \(int(day["numSells"])) sells, \(int(day["numTraders"])) traders, volume \(usd(volume)), "
                         + "of it organic (real users, not bots) \(usd(organic)); organic score \(t["organicScoreLabel"] as? String ?? "?").")
        }
        let created = (t["createdAt"] as? String).flatMap(date)
        let graduated = (t["graduatedAt"] as? String).flatMap(date)
        var origin = created.map { "Created \(age($0, now: now)) ago" } ?? "Creation date unknown"
        if let pad = t["launchpad"] as? String { origin += " on the \(pad) launchpad" }
        if let graduated { origin += "; graduated to an open pool \(age(graduated, now: now)) ago" }
        lines.append(origin + ".")
        var safety: [String] = []
        if let off = audit["mintAuthorityDisabled"] as? Bool { safety.append(off ? "mint authority off (no one can print more)" : "MINT AUTHORITY ON: the creator can print more") }
        if let off = audit["freezeAuthorityDisabled"] as? Bool { safety.append(off ? "freeze authority off" : "FREEZE AUTHORITY ON: the creator can freeze holders' tokens") }
        if let top = dec(audit["topHoldersPercentage"]) { safety.append("top 10 holders own \(pct(top, sign: false))") }
        if let dev = dec(audit["devBalancePercentage"]) { safety.append("the developer holds \(pct(dev, sign: false))") }
        if !safety.isEmpty { lines.append("Safety: " + safety.joined(separator: ", ") + ".") }
        return lines.joined(separator: "\n")
    }

    /// DexScreener: where it trades and its deepest pool, with that pool's last 30 days (the chart on the token's card).
    static func dex(_ address: String) async -> String? {
        guard let data = try? await get("https://api.dexscreener.com/latest/dex/tokens/\(address)"),
              let facts = dexFacts(data, address: address) else { return nil }
        guard let top = TokenLookup.parse(data, address: address).first, let m = TokenMarket.parse(data, address: address, chain: top.chain),
              let points = try? await TokenMarket.history(chain: top.chain, pool: m.pool, range: .month),
              let month = chartFacts(points, range: .month, of: "The deepest pool (GeckoTerminal, as the token's card draws it)") else { return facts }
        return facts + "\n" + month
    }

    /// A chart as the canvas draws it: the move over the range, its low and high, and about thirty closes, so levels
    /// and a buy range come from the line the user sees.
    public static func chartFacts(_ points: [TokenMarket.Point], range: ChartCommand.Range, of what: String, timeZone: TimeZone = .current) -> String? {
        guard let first = points.first, let last = points.last, first.close > 0,
              let low = points.min(by: { $0.close < $1.close }), let high = points.max(by: { $0.close < $1.close }) else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = timeZone
        f.dateFormat = switch range { case .day: "HH:mm"; case .week: "MMM d HH:mm"; case .month: "MMM d" }
        // From the newest back, so the last close is always in: a day's close each on a month of 4-hour bars.
        let step = max(1, points.count / 30)
        let closes = stride(from: points.count - 1, through: 0, by: -step).map { points[$0] }.reversed()
        return "\(what) over \(range.title): \(price(first.close)) → \(price(last.close)) (\(pct((last.close - first.close) / first.close * 100))), "
            + "low \(price(low.close)) on \(f.string(from: low.time)), high \(price(high.close)) on \(f.string(from: high.time)).\nCloses: "
            + closes.map { "\(f.string(from: $0.time)) \(price($0.close))" }.joined(separator: ", ") + "."
    }

    static func dexFacts(_ data: Data, address: String, now: Date = Date()) -> String? {
        let chains = TokenLookup.parse(data, address: address)
        guard let top = chains.first, let m = TokenMarket.parse(data, address: address, chain: top.chain) else { return nil }
        return (["DexScreener: \(Holding.plain(m.name, max: 32)) (\(Holding.plain(m.symbol))), contract \(top.address), chain id \(top.chain)"
                 + (chains.count > 1 ? "; also trades on " + chains.dropFirst().map(\.chain).joined(separator: ", ") : "") + "."]
                + poolFacts(m, now: now)).joined(separator: "\n")
    }

    /// The deepest pool as lines: what the token's card shows under its chart.
    public static func poolFacts(_ m: TokenMarket.Info, now: Date = Date()) -> [String] {
        var lines = ["Deepest pool: \(m.dex) \(m.symbol)/\(m.quote), liquidity \(usd(m.liquidityUSD))" + (m.created.map { ", opened \(age($0, now: now)) ago" } ?? "")
                     + "; price \(price(m.priceUSD)), market cap \(usd(m.marketCap ?? m.fdv)), volume 24h \(usd(m.volume24h))"
                     + (m.buys24h.map { ", \($0) buys" } ?? "") + (m.sells24h.map { " and \($0) sells in 24h" } ?? "") + "."]
        let changes = [("m5", "5m"), ("h1", "1h"), ("h6", "6h"), ("h24", "24h")].compactMap { key, title in m.changes[key].map { "\(title) \(pct($0))" } }
        if !changes.isEmpty { lines.append("Price change: " + changes.joined(separator: ", ") + ".") }
        if !m.links.isEmpty { lines.append("Links: " + m.links.map { "\($0.label) \($0.url.absoluteString)" }.joined(separator: ", ") + ".") }
        return lines
    }

    /// A name or ticker: the tokens called so, deepest pool first, and the deepest one in full (saves the model a round).
    static func search(_ query: String) async -> String {
        // A ticker Jupiter verified on Solana is that token. A pool anyone can open may claim any liquidity (a copy of
        // STONK claimed $6 billion and came first), and the verified list can't be faked that way.
        async let verified = query.count <= 12 && !query.contains(" ") ? solanaMint(symbol: query) : nil
        let q = query.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)
        let data = try? await get("https://api.dexscreener.com/latest/dex/search?q=\(q ?? query)")
        let found = data.flatMap { searchFacts($0, query: query) }
        // Jupiter not answering is no «not verified»: the deepest pool below may be a copy, and the model is told so.
        let mint: String?, head: [String]
        do { (mint, head) = (try await verified, []) } catch {
            (mint, head) = (nil, ["Jupiter didn't answer just now, so which «\(query)» on Solana is the verified one "
                                  + "isn't known: the deepest pool below may be a copy. Ask again in a minute before buying by this name."])
        }
        if let mint {
            async let jupiter = jupiter(mint)
            async let pools = dex(mint)
            return (["«\(query)» on Solana is the Jupiter-verified token with mint \(mint); use this one. Other tokens under the name are copies."]
                    + [try? await jupiter, await pools].compactMap { $0 }
                    + (found.map { ["For comparison, everything DexScreener finds under the name (the copies claim any liquidity):", $0.text] } ?? []))
                .joined(separator: "\n")
        }
        guard data != nil else { return (head + ["DexScreener didn't answer the search."]).joined(separator: "\n") }
        guard let (list, top) = found else { return (head + ["No token called «\(query)» trades anywhere DexScreener sees."]).joined(separator: "\n") }
        async let jupiter = top.hasPrefix("0x") ? nil : jupiter(top)
        async let pools = dex(top)
        return (head + [list, "The deepest one in full:"] + [try? await jupiter, await pools].compactMap { $0 }).joined(separator: "\n")
    }

    /// The matches as lines, and the address of the deepest.
    static func searchFacts(_ data: Data, query: String, now: Date = Date()) -> (text: String, top: String)? {
        guard let pairs = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["pairs"] as? [[String: Any]] else { return nil }
        var best: [String: (liquidity: Decimal, address: String, line: String)] = [:]
        for p in pairs {
            guard let base = p["baseToken"] as? [String: Any], let address = base["address"] as? String, let chain = p["chainId"] as? String else { continue }
            let liquidity = dec((p["liquidity"] as? [String: Any])?["usd"]) ?? 0
            let key = chain + ":" + address
            guard liquidity > (best[key]?.liquidity ?? -1) else { continue }
            let opened = (p["pairCreatedAt"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue / 1000) }
            let change = dec((p["priceChange"] as? [String: Any])?["h24"])
            best[key] = (liquidity, address, "\(Holding.plain(base["symbol"] as? String ?? "?")) (\(Holding.plain(base["name"] as? String ?? "?", max: 32))) on chain id \(chain), contract \(address): "
                         + "price \(price(dec(p["priceUsd"]))), liquidity \(usd(liquidity))" + (change.map { ", 24h \(pct($0))" } ?? "")
                         + (opened.map { ", pool opened \(age($0, now: now)) ago" } ?? ""))
        }
        let ranked = best.values.sorted { $0.liquidity > $1.liquidity }
        guard let top = ranked.first else { return nil }
        return ("Tokens matching «\(query)», deepest pool first. Many tokens copy a famous name: the deep, old pool is usually the real one; "
                + "a thin, fresh one is usually a copy.\n" + ranked.prefix(6).map(\.line).joined(separator: "\n"), top.address)
    }

    // MARK: Formatting

    /// The Dev copy's «Jupiter is busy» (`-jupiterBusyTest YES`, read at every call): each Jupiter request is answered
    /// 429 without going out, for a live run of what the chat says then.
    nonisolated(unsafe) public static var busyTest: () -> Bool = { false }

    /// A GET whose status is read: a throttle (429) or a server error is asked again 1, 2, 4 and 8 s later (RPCRetry),
    /// past Jupiter's 10-second window, and then throws. Jupiter's 429 carries a JSON error, which read as data was
    /// «no such token».
    public static func get(_ url: String) async throws -> Data {
        var request = URLRequest(url: URL(string: url)!, timeoutInterval: 10)
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let jupiter = request.url?.host?.hasSuffix("jup.ag") == true, busy = jupiter && busyTest()
        return try await RPCRetry.run {
            if busy { return (Data(#"{"code":429,"message":"Too many requests"}"#.utf8), HTTPURLResponse(url: request.url!, statusCode: 429, httpVersion: nil, headerFields: nil)! as URLResponse) }
            return try await URLSession.shared.data(for: request)
        } answer: { data, status in try answer(data, status: status, from: jupiter ? "Jupiter" : request.url?.host ?? "The service") }
    }

    /// Data only with a 2xx. 429 and 5xx are worth asking again; anything else is final. Either way it is no answer.
    static func answer(_ data: Data, status: Int, from source: String = "Jupiter") throws -> Data {
        guard (200..<300).contains(status) else {
            let why = status == 429 ? "too many requests right now" : "HTTP \(status)"
            throw RPCRetry.Again(status == 429 || status >= 500, WalletError("\(source) didn't answer (\(why)). Nothing was signed — try again in a minute."))
        }
        return data
    }

    static func dec(_ v: Any?) -> Decimal? { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber).map { $0.decimalValue } }
    static func int(_ v: Any?) -> Int { (v as? NSNumber)?.intValue ?? 0 }
    static func usd(_ d: Decimal?) -> String {
        d.map { $0.formatted(.currency(code: "USD").notation(.compactName).precision(.significantDigits(3)).locale(Locale(identifier: "en_US"))) } ?? "unknown"
    }
    static func price(_ d: Decimal?) -> String { d.map { "$" + String(format: "%.4g", NSDecimalNumber(decimal: $0).doubleValue) } ?? "unknown" }
    static func pct(_ d: Decimal, sign: Bool = true) -> String {
        (sign && d >= 0 ? "+" : "") + String(format: "%.1f", NSDecimalNumber(decimal: d).doubleValue) + "%"
    }
    static func date(_ iso: String) -> Date? { ISO8601DateFormatter().date(from: iso) }
    /// «40 minutes», «5 hours», «23 days», «4 months».
    static func age(_ date: Date, now: Date) -> String {
        let s = max(0, now.timeIntervalSince(date))
        return s < 7200 ? "\(Int(s / 60)) minutes" : s < 172_800 ? "\(Int(s / 3600)) hours" : s < 5_184_000 ? "\(Int(s / 86400)) days" : "\(Int(s / 2_592_000)) months"
    }
}
