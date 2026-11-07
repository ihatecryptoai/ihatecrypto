import Foundation

/// Company names people say, to Robinhood Stock Token symbols. The token address itself always comes from
/// Robinhood's registry and is checked on-chain; this only turns «SpaceX» or «тесла» into «SPCX» / «TSLA».
public enum StockNames {
    nonisolated(unsafe) static let names: [(Regex<Substring>, String)] = [
        (try! Regex(#"spacex|space\s*x\b|спейс\s*икс|спейсикс|спэйс"#), "SPCX"), (try! Regex(#"\btesla\b|тесл\w*"#), "TSLA"),
        (try! Regex(#"nvidia|нвиди\w*|нвидия"#), "NVDA"), (try! Regex(#"\bapple\b|эппл|эпл\b"#), "AAPL"), (try! Regex(#"microsoft|майкрософт"#), "MSFT"),
        (try! Regex(#"amazon|амазон"#), "AMZN"), (try! Regex(#"google|alphabet|гугл"#), "GOOGL"), (try! Regex(#"\bmeta\b|facebook|фейсбук"#), "META"),
        (try! Regex(#"coinbase|коинбейс"#), "COIN"), (try! Regex(#"palantir|палантир"#), "PLTR"), (try! Regex(#"netflix|нетфликс"#), "NFLX"),
        (try! Regex(#"\bamd\b"#), "AMD"), (try! Regex(#"robinhood\s+(?:stock|shares?|акци)|акци\w*\s+робинхуд"#), "HOOD"), (try! Regex(#"\bcircle\b"#), "CRCL"),
    ]
    /// Words that look like tickers but aren't stocks here.
    static let notStocks: Set<String> = Set(["USDC", "USDG", "USD", "ETH", "SOL", "BTC", "JUP", "HYPE", "USDT", "SPOT", "BUY", "SELL", "FOR", "ON", "THE", "ALL"]).union(Stablecoins.dollars)

    public static func symbol(in text: String) -> String? {
        let q = text.lowercased()
        if let hit = names.first(where: { q.contains($0.0) }) { return hit.1 }
        // A ticker written as one: «buy SPCX stock», «$NVDA».
        for word in text.split(whereSeparator: { !$0.isLetter && $0 != "$" }) {
            let w = word.hasPrefix("$") ? String(word.dropFirst()) : String(word)
            if (2...5).contains(w.count), w == w.uppercased(), w.allSatisfy({ $0.isASCII && $0.isLetter }), !notStocks.contains(w) { return w }
        }
        return nil
    }
}

/// Ondo Global Markets' tokenized US stocks and ETFs on Solana (NVDAon, SPYon…: Token-2022 mints ending in «ondo», each
/// Jupiter-verified and filled by JupiterZ's market makers, whom Ondo mints and redeems for) and USDY, its dollar earning
/// the US Treasury rate. Checked 2026-10-29 against Jupiter's list and docs.ondo.finance; on chain the stocks' mint
/// authority is 9foMHsSDq7nMg4WPusSz9eY7tyxyukqborA8GyU5cUxD, USDY's 8n2uiRUd…, all frozen by 51QVCuHf…. The suffix is a
/// vanity anyone can grind: a token counts as Ondo's only Jupiter-verified too (SpotAsset.ondo).
public enum Ondo {
    public static let usdy = "A1KLoBrKBde8Ty9qtNQUtq3C2ortoC3u7twggz7sEto6"
    public static func issued(_ mint: String) -> Bool { mint == usdy || mint.hasSuffix("ondo") }
    /// Ondo's terms, said before Confirm. Its freeze and mint authorities are an issuer's, not a scam's sign.
    public static func terms(russian: Bool) -> String {
        russian ? "Ondo не продаёт свои токены жителям США, Канады, России и Беларуси, а в ЕЭЗ, Британии, Сингапуре, Гонконге, Швейцарии, Бразилии и Малайзии — только квалифицированным инвесторам; погашение у самого Ondo — после KYC."
                : "Ondo doesn't offer its tokens to residents of the US, Canada, Russia or Belarus, and in the EEA, the UK, Singapore, Hong Kong, Switzerland, Brazil and Malaysia only to qualified investors; redeeming at Ondo itself needs KYC."
    }
}
