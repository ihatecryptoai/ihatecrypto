import Foundation

/// A cheap phishing check: the host against dApps the user is likely to mean. A host one or two letters away
/// from a known one is the classic fake («jup.ag» → «jupp.ag»).
public enum SiteCheck {
    public static let known = ["jup.ag", "raydium.io", "orca.so", "kamino.finance", "kamino.com", "drift.trade", "meteora.ag", "marginfi.com",
                        "app.hyperliquid.xyz", "hyperliquid.xyz", "relay.link", "solscan.io", "phantom.app", "pump.fun", "tensor.trade"]

    public enum Verdict: Equatable, Sendable { case known, lookalike(String), unknown, local }

    public static func check(host: String) -> Verdict {
        // By its labels, as `listed` reads it (the 2026-11-13 review): «jup.ag.» is jup.ag's own server and read as a fake
        // of it, and the dot counted as a letter, so a fake two letters off a known address read as an unfamiliar site.
        let h = host.lowercased().split(separator: ".").joined(separator: ".")
        if h.isEmpty { return .local }
        if known.contains(where: { h == $0 || h.hasSuffix("." + $0) }) { return .known }
        let base = h.split(separator: ".").suffix(2).joined(separator: ".")
        // A known name as a part of a label anywhere else (jup.ag.claim-airdrop.com, hyperliquid-app.xyz, kamino-finance.app,
        // jup-ag.com) is the other classic fake, and so is any punycode label: letters from another alphabet that look like ours.
        let parts = Set(h.split(whereSeparator: { $0 == "." || $0 == "-" }).map(String.init))
        if let named = known.first(where: { parts.contains(brand($0)) }) { return .lookalike(named) }
        if h.split(separator: ".").contains(where: { $0.hasPrefix("xn--") }) {
            return .lookalike(known.min { distance(base, $0) < distance(base, $1) } ?? known[0])
        }
        if let near = known.first(where: { distance(base, $0) <= 2 || distance(h, $0) <= 2 }) { return .lookalike(near) }
        return .unknown
    }

    /// The name a known site goes by: the label before its top-level domain («hyperliquid» of app.hyperliquid.xyz).
    static func brand(_ domain: String) -> String { String(domain.split(separator: ".").dropLast().last ?? "") }

    /// On MetaMask's phishing list (eth-phishing-detect): the host or any domain above it is blocked, unless the list's
    /// allowlist names one of them («evil.github.io» is blocked by its own entry, not by github.io). Capitals and a
    /// trailing dot («evil.com.») change nothing: `split` drops the empty label.
    public static func listed(host: String, blocked: Set<String>, allowed: Set<String>) -> Bool {
        let parts = host.lowercased().split(separator: ".")
        let domains = (0..<max(parts.count - 1, 0)).map { parts[$0...].joined(separator: ".") }
        return !domains.contains(where: allowed.contains) && domains.contains(where: blocked.contains)
    }

    // MARK: What a site's request is held to (the 2026-11-13 review)

    /// The origin a site's connection is kept under: scheme, host and port as WebKit names them. A `file:` page has no
    /// host, so every local page was one origin and connecting one connected them all: a local page's origin is its file.
    public static func origin(scheme: String, host: String, port: Int?, path: String) -> String {
        host.isEmpty ? "\(scheme)://\(scheme == "file" ? path : "")" : "\(scheme)://\(host)\(port.map { ":\($0)" } ?? "")"
    }

    /// The fee is read again after Sign, and that one is signed: held so the whole of it stays within 1.5 times the
    /// «Network fee up to» the window showed (`shown`, in wei, for `gas` units). The most a unit of gas may cost, and the
    /// tip within it. Held, not refused: a tip that jumps between two reads of a cent's fee would stop ordinary
    /// transactions. nil: nothing to hold it to.
    public static func feeHeld(shown: Decimal, gas: UInt64, maxFee: UInt64, maxPriority: UInt64) -> (maxPriority: UInt64, maxFee: UInt64)? {
        guard gas > 0, shown > 0 else { return nil }
        let most = NSDecimalNumber(decimal: shown * 3 / 2 / Decimal(gas)).uint64Value
        let fee = min(maxFee, most)
        return fee > 0 ? (min(maxPriority, fee), fee) : nil
    }

    /// What a Solana transaction pays the network, in lamports: 5,000 a signature, and the price its Compute Budget steps
    /// name (tag 3, micro-lamports a unit) for their unit limit (tag 2, 1.4 million at most), rounded up. 0 past 64 bits:
    /// the guard refuses that.
    /// ponytail: no limit named counts as 200,000 units, the guard's number. Solana gives 200,000 a program step, so a
    /// transaction with a price and no limit pays more and its SOL reads as sent — count the steps if sites send those.
    public static func solanaFee(signatures: Int, budget: [Data]) -> UInt64 {
        var limit: UInt64 = 200_000, price: UInt64 = 0
        func le(_ d: Data, _ bytes: Int) -> UInt64 { d.dropFirst().prefix(bytes).reversed().reduce(0) { $0 << 8 | UInt64($1) } }
        for d in budget {
            if d.first == 2, d.count >= 5 { limit = min(le(d, 4), 1_400_000) }
            if d.first == 3, d.count >= 9 { price = le(d, 8) }
        }
        let (product, overflow) = limit.multipliedReportingOverflow(by: price)
        return overflow ? 0 : 5_000 * UInt64(UInt8(clamping: signatures)) + product / 1_000_000 + (product % 1_000_000 == 0 ? 0 : 1)
    }

    static func distance(_ a: String, _ b: String) -> Int {
        let a = Array(a), b = Array(b)
        var row = Array(0...b.count)
        for i in 1...max(a.count, 1) where !a.isEmpty {
            var prev = row[0]
            row[0] = i
            for j in 1...max(b.count, 1) where !b.isEmpty {
                let cur = row[j]
                row[j] = min(row[j] + 1, row[j - 1] + 1, prev + (a[i - 1] == b[j - 1] ? 0 : 1))
                prev = cur
            }
        }
        return a.isEmpty ? b.count : b.isEmpty ? a.count : row[b.count]
    }
}
