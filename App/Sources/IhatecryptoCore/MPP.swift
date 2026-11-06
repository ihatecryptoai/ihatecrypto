import Foundation
import LLMCore
import WalletCore

/// Paying for an API on Tempo — MPP, the «Payment» HTTP auth scheme (paymentauth.org, the x402 flow with receipts).
/// The service answers 402 with a challenge: this much USDC on Tempo to this payee. ihatecrypto pays it itself (push
/// mode: its own transfer, signed after Confirm) and asks again with the transaction's hash as the credential.
/// Pull mode (a signed Tempo 0x76 transaction the service sends) and sessions aren't supported: the signer signs only
/// its own EIP-1559 transfers, so a service that takes only those is refused with the reason.
public enum MPP {
    /// The one token ihatecrypto pays in: USDC on Tempo, 6 decimals.
    public static let usdc = "0x20c000000000000000000000b9537d11c60e8b50"
    public static let chainId: UInt64 = 4217

    public struct Challenge: Equatable, Sendable {
        /// The auth-params as the service sent them: the credential echoes them unchanged.
        public let params: [String: String]
        /// USDC units.
        public let amount: UInt64
        public let payee: String
        public let memo: Data?
        public let expires: Date?

        public var realm: String { params["realm"] ?? "" }
        public var purpose: String? { params["description"] }
        /// «Payment-Authorization» when the service keeps Authorization for its own login.
        public var field: String { params["header"] ?? "Authorization" }
        public var usdc: Decimal { Decimal(amount) / 1_000_000 }
    }

    /// The Payment challenges in a WWW-Authenticate value, each as its auth-params (a value may list several schemes).
    static func challenges(_ header: String) -> [[String: String]] {
        var out: [[String: String]] = [], current: [String: String]?, scheme = ""
        for m in header.matches(of: /(?:([A-Za-z][\w-]*)\s+)?([A-Za-z][\w-]*)=("(?:[^"\\]|\\.)*"|[^\s,]+)/) {
            if let s = m.1 {
                if let c = current, scheme == "payment" { out.append(c) }
                scheme = s.lowercased()
                current = [:]
            }
            var value = String(m.3)
            if value.hasPrefix("\"") {
                value = String(value.dropFirst().dropLast()).replacing(/\\(.)/) { String($0.1) }
            }
            current?[String(m.2).lowercased()] = value
        }
        if let c = current, scheme == "payment" { out.append(c) }
        return out
    }

    /// The challenge ihatecrypto can pay, or why none: USDC on Tempo, a charge of more than zero, push mode allowed.
    public static func challenge(_ header: String, now: Date = Date()) -> Result<Challenge, Refusal> {
        let all = challenges(header)
        guard let p = all.first(where: { $0["method"] == "tempo" && $0["intent"] == "charge" }) else {
            let offered = all.map { "\($0["method"] ?? "?") \($0["intent"] ?? "?")" }.joined(separator: ", ")
            return .failure(Refusal(offered.isEmpty ? "The service asked for payment without an MPP challenge." :
                "The service takes \(offered); ihatecrypto pays only a one-time charge in USDC on Tempo."))
        }
        guard let id = p["id"], !id.isEmpty, p["realm"] != nil, let encoded = p["request"],
              let raw = base64url(encoded), let request = try? JSONSerialization.jsonObject(with: raw) as? [String: Any] else {
            return .failure(Refusal("The service's payment request can't be read."))
        }
        let details = request["methodDetails"] as? [String: Any] ?? [:]
        guard let amount = (request["amount"] as? String).flatMap({ UInt64($0) }), amount > 0 else {
            return .failure(Refusal("The service asks for a zero-amount proof or an amount that can't be read; ihatecrypto only pays real charges."))
        }
        guard (request["currency"] as? String)?.lowercased() == usdc else {
            return .failure(Refusal("The service charges in \(request["currency"] as? String ?? "an unknown token"); ihatecrypto pays in USDC on Tempo."))
        }
        if let chain = (details["chainId"] as? NSNumber)?.uint64Value, chain != chainId {
            return .failure(Refusal("The service wants payment on network \(chain), not Tempo mainnet."))
        }
        guard let payee = request["recipient"] as? String, payee.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else {
            return .failure(Refusal("The service's payee isn't an address."))
        }
        if let splits = details["splits"] as? [Any], !splits.isEmpty {
            return .failure(Refusal("The service splits the payment between several payees; ihatecrypto pays one."))
        }
        if let modes = details["supportedModes"] as? [String], !modes.contains("push") {
            return .failure(Refusal("The service takes only a transaction it sends itself (pull mode); ihatecrypto sends its own and proves it with the hash."))
        }
        if details["feePayer"] as? Bool == true {
            return .failure(Refusal("The service pays the fee itself (sponsored mode), which needs a transaction it sends; ihatecrypto sends its own."))
        }
        var memo: Data?
        if let m = details["memo"] as? String {
            guard m.wholeMatch(of: /0x[0-9a-fA-F]{64}/) != nil, let d = Data(hex: m) else {
                return .failure(Refusal("The service's memo isn't 32 bytes."))
            }
            memo = d
        }
        let expires = p["expires"].flatMap(date)
        if let expires, expires <= now { return .failure(Refusal("The service's price quote has expired; ask again.")) }
        return .success(Challenge(params: p, amount: amount, payee: payee, memo: memo, expires: expires))
    }

    public struct Refusal: LocalizedError, Equatable, Sendable {
        public let reason: String
        init(_ reason: String) { self.reason = reason }
        public var errorDescription: String? { reason }
    }

    /// The header value that proves the payment: «Payment <base64url JSON>» with the challenge echoed, the transaction's
    /// hash and the payer as a did:pkh.
    public static func credential(_ c: Challenge, hash: String, payer: String) -> String {
        let echoed = c.params.filter { ["id", "realm", "method", "intent", "request", "description", "opaque", "digest", "expires", "header"].contains($0.key) }
        let body: [String: Any] = ["challenge": echoed, "payload": ["type": "hash", "hash": hash], "source": "did:pkh:eip155:\(chainId):\(payer)"]
        let json = (try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data()
        return "Payment " + json.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    static func base64url(_ s: String) -> Data? {
        var b = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        b += String(repeating: "=", count: (4 - b.count % 4) % 4)
        return Data(base64Encoded: b)
    }

    static func date(_ s: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime]
        return f.date(from: s)
    }

    /// The request as the service documents it; `credential` goes in the field its challenge named.
    public static func call(_ method: String, _ url: String, _ body: String?, credential: (field: String, value: String)? = nil) async throws -> (Int, [String: String], Data) {
        guard let u = URL(string: url), u.scheme == "https" else { throw URLError(.unsupportedURL) }
        var request = URLRequest(url: u, timeoutInterval: 60)
        request.httpMethod = method
        if let body {
            request.httpBody = Data(body.utf8)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let credential { request.setValue(credential.value, forHTTPHeaderField: credential.field) }
        let (data, response) = try await URLSession.shared.data(for: request)
        let http = response as? HTTPURLResponse
        let headers = (http?.allHeaderFields ?? [:]).reduce(into: [String: String]()) { $0["\($1.key)".lowercased()] = "\($1.value)" }
        return (http?.statusCode ?? 0, headers, data)
    }

    /// A service's answer for the chat: JSON indented, cut at `limit` characters.
    public static func shown(_ data: Data, limit: Int = 1500) -> String {
        let text = (try? JSONSerialization.jsonObject(with: data)).flatMap { try? JSONSerialization.data(withJSONObject: $0, options: [.prettyPrinted, .withoutEscapingSlashes]) }
            .map { String(decoding: $0, as: UTF8.self) } ?? String(decoding: data, as: UTF8.self)
        return text.count > limit ? String(text.prefix(limit)) + "…" : text
    }

    // MARK: The service's answer

    /// What a service answered, for the canvas: its list of results (search hits, flights, images) when it has one, else
    /// its plain values, else its text.
    public struct Answer: Codable, Hashable, Sendable {
        public struct Item: Codable, Hashable, Sendable {
            public let title: String
            public let url: String?
            public let detail: String?
            public init(title: String, url: String?, detail: String?) { (self.title, self.url, self.detail) = (title, url, detail) }
            public var isImage: Bool { url.map { $0.lowercased().contains(/\.(?:png|jpe?g|webp|gif)(?:\?|$)/) } ?? false }
        }
        public struct Fact: Codable, Hashable, Sendable { public let key: String; public let value: String }

        public var items: [Item] = []
        public var facts: [Fact] = []
        public var text: String?
        /// What an AI service wrote (Grok, Perplexity: the answer itself), and the links it cites. Its JSON puts the
        /// answer after its tool calls, so a cut of the start showed only «x_keyword_search» rows.
        public var writing: String?
        public var links: [String] = []

        public init(_ data: Data) {
            guard let json = try? JSONSerialization.jsonObject(with: data, options: .fragmentsAllowed) else {
                let t = String(decoding: data.prefix(1500), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                text = t.isEmpty ? nil : t
                return
            }
            var texts: [String] = []
            func collect(_ x: Any, _ key: String, depth: Int) {
                guard depth < 14 else { return }
                if let s = x as? String {
                    if s.hasPrefix("http"), s.count < 400 {
                        if !links.contains(s), links.count < 15 { links.append(s) }
                    } else if Self.writingKeys.contains(key), s.count >= 80, s.filter({ $0 == " " }).count >= 8, !texts.contains(s) {
                        texts.append(s)
                    }
                } else if let list = x as? [Any] {
                    list.forEach { collect($0, key, depth: depth + 1) }
                } else if let d = x as? [String: Any] {
                    d.keys.sorted().forEach { collect(d[$0]!, $0, depth: depth + 1) }
                }
            }
            collect(json, "", depth: 0)
            if !texts.isEmpty {
                writing = String(texts.joined(separator: "\n\n").prefix(6000))
                return
            }
            // The list is the array of objects with the most titles and links in it, wherever it sits («data.web.results»).
            var best: [[String: Any]] = [], bestScore = 0
            func walk(_ x: Any, depth: Int) {
                guard depth < 7 else { return }
                if let list = x as? [Any] {
                    let rows = list.compactMap { $0 as? [String: Any] }
                    let score = rows.filter { Self.pick($0, Self.titleKeys) != nil || Self.link($0) != nil }.count
                    if score > bestScore { (best, bestScore) = (rows, score) }
                    list.forEach { walk($0, depth: depth + 1) }
                } else if let d = x as? [String: Any] {
                    d.keys.sorted().forEach { walk(d[$0]!, depth: depth + 1) }
                }
            }
            walk(json, depth: 0)
            items = best.prefix(10).compactMap { row in
                let url = Self.link(row)
                // A row without a title or text (a flight: price, airline, times) shows its plain values instead.
                let shown = Set(Self.titleKeys + Self.detailKeys + Self.linkKeys)
                let facts = row.keys.sorted().filter { !shown.contains($0) }.compactMap { k in Self.scalar(row[k]).map { "\(k) \($0)" } }
                let named = Self.pick(row, Self.titleKeys) ?? url.flatMap { URL(string: $0)?.host() }
                guard let title = named ?? facts.first else { return nil }
                let rest = named == nil ? facts.dropFirst() : facts[...]
                return Item(title: title, url: url, detail: Self.pick(row, Self.detailKeys) ?? (rest.isEmpty ? nil : rest.prefix(3).joined(separator: " · ")))
            }
            guard items.isEmpty else { return }
            func flatten(_ x: Any, _ path: String, depth: Int) {
                guard facts.count < 12, depth < 4 else { return }
                if let d = x as? [String: Any] {
                    for k in d.keys.sorted() where !["success", "type", "object", "status"].contains(k) { flatten(d[k]!, path.isEmpty ? k : "\(path).\(k)", depth: depth + 1) }
                } else if let v = Self.scalar(x) {
                    facts.append(Fact(key: path, value: v))
                }
            }
            flatten(json, "", depth: 0)
            if facts.isEmpty { text = MPP.shown(data, limit: 1500) }
        }

        static let titleKeys = ["title", "name", "headline", "label", "subject", "display_name", "full_name", "question"]
        /// Where services put the words they wrote: xAI's and OpenAI's output_text, a chat reply's content, an answer.
        static let writingKeys: Set<String> = ["output_text", "text", "content", "answer", "response", "completion"]
        static let detailKeys = ["description", "snippet", "summary", "content", "text", "body", "answer", "extra_snippets"]

        static func pick(_ row: [String: Any], _ keys: [String]) -> String? {
            keys.lazy.compactMap { k in (row[k] as? String).map(Self.plain) ?? (row[k] as? [String])?.first.map(Self.plain) }.first { !$0.isEmpty }
        }

        static let linkKeys = ["url", "link", "href", "permalink", "profile_url", "image_url"]
        static func link(_ row: [String: Any]) -> String? { linkKeys.lazy.compactMap { row[$0] as? String }.first { $0.hasPrefix("http") } }

        /// A number, a short string or a yes/no; nil for objects, lists and long text.
        static func scalar(_ x: Any?) -> String? {
            switch x {
            case let n as NSNumber: CFGetTypeID(n) == CFBooleanGetTypeID() ? (n.boolValue ? "yes" : "no") : n.stringValue
            case let s as String where !s.isEmpty && s.count <= 120: plain(s)
            default: nil
            }
        }

        /// Search engines mark matches with <strong>: the canvas shows the words.
        public static func plain(_ s: String) -> String {
            s.replacing(/<[^>]+>/, with: "").replacingOccurrences(of: "&amp;", with: "&").replacingOccurrences(of: "&quot;", with: "\"")
                .replacingOccurrences(of: "&#39;", with: "'").replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }

    /// «Paid … ↗\n\n{ "success": true, … }» → the words, and the JSON after them (whole or cut short).
    public static func splitData(_ text: String) -> (String, String?) {
        guard let r = text.range(of: #"\n\s*\n(?=\s*[\{\[]\s*\n?\s*")"#, options: .regularExpression) else { return (text, nil) }
        let data = String(text[r.upperBound...]).trimmingCharacters(in: .whitespacesAndNewlines)
        guard data.count > 40, data.contains("\":") || data.contains("\" :") else { return (text, nil) }
        return (String(text[..<r.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines), data)
    }

    /// What the folded card lists: the results' titles with their links when the JSON has them, else its first
    /// non-empty text values («original: site:x.com …»). At most four.
    public static func rows(_ json: String) -> [(title: String, url: String?)] {
        func values(_ keys: String) -> [String] {
            json.matches(of: try! Regex(#""(?:"# + keys + #")"\s*:\s*"((?:[^"\\]|\\.)*)""#)).compactMap { m in
                (m.output[1].substring).map { Answer.plain(String($0)) }
            }.filter { !$0.isEmpty }
        }
        let titles = values("title|name|headline"), urls = values("url|link").filter { $0.hasPrefix("http") }
        if !titles.isEmpty { return titles.prefix(4).enumerated().map { ($0.element, $0.offset < urls.count ? urls[$0.offset] : nil) } }
        return json.matches(of: #/"(\w+)"\s*:\s*"((?:[^"\\]|\\.)+)"/#).prefix(4).map { ("\($0.output.1): \(Answer.plain(String($0.output.2)))", nil) }
    }

    /// The user's question answered from what the service sent: 1–3 sentences in their language, the best results as
    /// links. The model sees only the service's data, cut to 6,000 characters.
    public static func reply(to ask: String, service: String, data: Data, language: String, client: any LLMClient, model: String) async throws -> String {
        let prompt = """
        The user asked their crypto app: «\(ask.prefix(600))»
        To answer it, the app paid a web service, \(service), which sent back:
        \(digest(data))
        Answer the user in \(language) in 1–3 short sentences, from this data only: what they wanted to know, naming the one \
        to three most relevant results as markdown links [title](url) when there are any. If the data doesn't answer it (no \
        results, an error), say so plainly and what to ask instead. Plain text: no JSON, no headings, no preamble.
        """
        let text = try await client.complete(LLMRequest(model: model, messages: [LLMMessage(.user, prompt)]))
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// What the chat's model reads of an answer: an AI service's own words and its links; else the JSON, cut.
    static func digest(_ data: Data) -> String {
        let a = Answer(data)
        guard let w = a.writing else { return shown(data, limit: 6000) }
        return "Its answer:\n\(w)" + (a.links.isEmpty ? "" : "\n\nLinks it cites:\n" + a.links.joined(separator: "\n"))
    }

    /// The service's name in the directory («Brave Search») for its address; nil when it isn't listed.
    public static func serviceName(_ url: String) async -> String? {
        guard let host = URL(string: url)?.host()?.lowercased(), let services = await directory() else { return nil }
        return services.first { (($0["serviceUrl"] as? String).flatMap { URL(string: $0)?.host()?.lowercased() }) == host }?["name"] as? String
    }

    // MARK: Directory

    /// The services that take MPP payments (mpp.dev's directory) matching the words, with their endpoints and prices,
    /// for the model's «services» lookup.
    public static func search(_ query: String) async -> String {
        guard let services = await directory() else { return "The MPP directory (mpp.dev) didn't answer." }
        let hits = rank(services, query)
        guard !hits.isEmpty else { return "No MPP service matches «\(query)». Try broader words (email, search, image, flights, phone, mail)." }
        return hits.map { describe($0, endpoints: 6) }.joined(separator: "\n\n")
            + "\n\nTo pay one: the form «pay <METHOD> <service URL + path> <JSON body>». For the exact body fields look up «service <id> <path>»."
    }

    /// The six services that fit the words best.
    static func rank(_ services: [[String: Any]], _ query: String) -> [[String: Any]] {
        var words = tokens(query).filter { $0.count > 1 && !["api", "apis", "service", "services", "pay", "paid", "request", "the", "for", "via", "tempo", "mpp",
                                                             "on", "about", "of", "in", "to", "and", "or", "what", "how", "is", "are", "with", "from"].contains($0) }
        // A coin's name is the word «crypto» to the directory: «BTC price» is CoinGecko's.
        // ponytail: a short list; the model usually writes «crypto» itself.
        if words.contains(where: ["bitcoin", "btc", "ethereum", "eth", "solana", "sol", "coin", "coins", "token", "tokens"].contains) { words.append("crypto") }
        // Twitter is «X» there («Chat with X Search»); a one-letter word matches only itself, not «x402» or «xai».
        if words.contains(where: ["twitter", "tweet", "tweets", "твиттер", "твит", "твиты"].contains) || tokens(query).contains("x") { words.append("x") }
        // A word in the name counts most, then in the tags, then anywhere in what it does; a word matches the start of
        // one there («price» — «prices»). Plain substrings ranked a fax service «priced per page» over CoinGecko.
        func score(_ s: [String: Any]) -> Int {
            let name = tokens("\(s["id"] ?? "") \(s["name"] ?? "")")
            let tags = tokens(((s["tags"] as? [String] ?? []) + (s["categories"] as? [String] ?? [])).joined(separator: " "))
            let rest = tokens("\(s["description"] ?? "") " + ((s["endpoints"] as? [[String: Any]])?.compactMap { $0["description"] as? String }.joined(separator: " ") ?? ""))
            return words.reduce(0) { sum, w in
                let hit = { (t: String) in w.count == 1 ? t == w : t.hasPrefix(w) }
                return sum + (name.contains(where: hit) ? 5 : tags.contains(where: hit) ? 2 : rest.contains(where: hit) ? 1 : 0)
            }
        }
        return services.filter { !unpayable($0) }.map { ($0, score($0)) }.filter { words.isEmpty || $0.1 > 0 }.sorted { $0.1 > $1.1 }.prefix(6).map(\.0)
    }

    /// Services whose 402 ihatecrypto can't pay, measured 2026-09-30: Tempo's own proxies and Orthogonal's take only a
    /// transaction they send and pay the fee for (fal, OpenAI, Anthropic, Fiber, ScrapeCreators), Exa only a transaction
    /// it sends (pull), Nansen only x402 on Base — a signed spending authorization the signer refuses as a drainer's tool.
    /// Left out of the search so the model never offers what can't be paid.
    // ponytail: by host, as measured; a service's 402 is still checked before any card (MPP.challenge).
    static func unpayable(_ s: [String: Any]) -> Bool {
        let host = (s["serviceUrl"] as? String).flatMap { URL(string: $0)?.host()?.lowercased() } ?? ""
        return host.hasSuffix(".mpp.tempo.xyz") || ["mpp.orthogonal.com", "api.exa.ai", "api.nansen.ai"].contains(host) || refusesProof.contains(host)
    }

    /// Services that take the payment and then refuse its proof, measured live 2026-10-23 on the Dev test wallet: AIsa
    /// answers «credential request does not match this route's requirements» to every proof (GET and POST), Alchemy's
    /// own receipt check fails (403 from its node), StableEnrich answers 402 again to a push proof. Their 402 looks
    /// payable, so they're never paid, even when the chat names the address.
    public static let refusesProof: Set<String> = ["api.aisa.one", "mpp.alchemy.com", "stableenrich.dev"]

    static func tokens(_ text: String) -> [String] { text.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber }).map(String.init) }

    /// One service in full: every endpoint and price, and the start of its docs for the request fields.
    /// `arg`: «grok» or «grok /grok/x-search»: with an endpoint the docs start at that endpoint's section — a long doc
    /// cut at its start never reached Grok's X search, and the model fell back to a web search.
    public static func service(_ arg: String) async -> String {
        guard let services = await directory() else { return "The MPP directory (mpp.dev) didn't answer." }
        let words = arg.split(separator: " ").map(String.init)
        guard let id = words.first?.lowercased(), let s = services.first(where: { ($0["id"] as? String)?.lowercased() == id }) else {
            return "No MPP service «\(arg)». Search with «services <words>» first."
        }
        var text = describe(s, endpoints: 25)
        if let docs = (s["docs"] as? [String: Any])?["llmsTxt"] as? String, let data = try? await HTTP.get(docs, timeout: 10) {
            text += "\nDocs (\(docs)):\n" + section(String(decoding: data, as: UTF8.self), about: Array(words.dropFirst()))
        }
        return text
    }

    /// 2,500 characters of the docs from the heading of the section that names the endpoint (its path's last part,
    /// «x-search»); the start when none is named or found.
    static func section(_ docs: String, about hint: [String]) -> String {
        let keys = hint.compactMap { $0.split(separator: "/").last.map(String.init) }.filter { $0.count > 2 }
        guard let hit = keys.lazy.compactMap({ docs.range(of: $0, options: .caseInsensitive) }).first else { return String(docs.prefix(2500)) }
        let head = docs[..<hit.lowerBound].range(of: "\n#", options: .backwards)?.lowerBound ?? hit.lowerBound
        return String(docs[head...].prefix(2500))
    }

    static func describe(_ s: [String: Any], endpoints limit: Int) -> String {
        let base = s["serviceUrl"] as? String ?? ""
        let endpoints = (s["endpoints"] as? [[String: Any]] ?? []).prefix(limit).map { e -> String in
            let pay = e["payment"] as? [String: Any]
            let price: String = if let a = (pay?["amount"] as? String).flatMap({ Decimal(string: $0) }), a > 0 {
                "$" + Fmt.num(a / 1_000_000, maxFraction: 6)
            } else if let hint = pay?["amountHint"] as? String {
                "priced by use (\(hint)); the 402 names the most it charges"
            } else if pay?["intent"] as? String == "session" { "per use (session: ihatecrypto can't pay these yet)" } else if pay == nil { "free" } else { "price in its 402 answer" }
            return "  \(e["method"] as? String ?? "GET") \(base)\(e["path"] as? String ?? "") — \(e["description"] as? String ?? ""), \(price)"
        }
        return "\(s["name"] as? String ?? "?") (id \(s["id"] as? String ?? "?")): \(s["description"] as? String ?? "")\n" + endpoints.joined(separator: "\n")
    }

    // ponytail: the directory is read on every lookup (≈ 400 KB); cache it if lookups get frequent.
    static func directory() async -> [[String: Any]]? {
        guard let data = try? await HTTP.get("https://mpp.dev/api/services", timeout: 15),
              let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return j["services"] as? [[String: Any]]
    }
}
