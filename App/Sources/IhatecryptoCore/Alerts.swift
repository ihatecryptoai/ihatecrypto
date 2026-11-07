import Foundation

/// «Tell me when oil drops below 90»: one notification when the mark price crosses the level, then the alert is done.
/// With steps («when ETH is below 2,680: long ETH leverage 3x») it runs them instead, like a task due now.
public struct PriceAlert: Codable, Equatable, Sendable, Identifiable {
    public var id = UUID()
    public let coin: String
    /// true: fires at or above `price`; false: at or below.
    public let above: Bool
    public let price: Decimal
    public var created = Date()
    /// The router's commands to run when it fires; nil: only the notification.
    public var steps: [String]?
    /// The canvas whose trade plan it prepares: drawn as that plan's first step, and when it fires it prepares the plan
    /// as it is on that canvas then, edits included (`steps` holds it as it was when set).
    public var plan: UUID?
    /// Set through the local debug API: its steps never run on the automation wallet by themselves (ScheduledTask).
    public var untrusted: Bool?
    /// Switched off on the Schedule: it waits without firing until switched on again.
    public var paused: Bool?

    public init(coin: String, above: Bool, price: Decimal, steps: [String]? = nil) {
        self.coin = coin
        self.above = above
        self.price = price
        self.steps = steps
    }

    public func fires(at mark: Decimal) -> Bool { above ? mark >= price : mark <= price }
}

public enum AlertCommand: Equatable, Sendable {
    /// `above` nil: whichever side of the current price the level is on. `steps`: what to run when it fires.
    case set(coin: String, price: Decimal, above: Bool?, steps: [String] = [])
    case clear

    public static func parse(_ text: String, markets: Set<String> = []) -> AlertCommand? {
        // «when ETH is below 2,680: long ETH leverage 3x»: the level before the colon, steps after it joined by « then ».
        if let colon = text.firstIndex(of: ":"), text[..<colon].lowercased().contains(/^\s*(?:when|if|once|alert)\b/) {
            let steps = text[text.index(after: colon)...].split(separator: /\s+then\s+|\s*;\s*/)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
            if !steps.isEmpty {
                guard case .set(let coin, let price, let above, _)? = parse("alert me " + text[..<colon], markets: markets) else { return nil }
                return .set(coin: coin, price: price, above: above, steps: steps)
            }
        }
        let q = text.lowercased()
        if q.contains(/(?:cancel|clear|remove|delete|stop)\s+(?:all\s+|my\s+|the\s+)*(?:price\s+)?alerts?|(?:отмени|удали|убери|сними)\w*\s+(?:все\s+)?(?:алерт|уведомлен|оповещен)/) {
            return .clear
        }
        guard q.contains(/\balert|notify|tell\s+me\s+when|let\s+me\s+know|ping\s+me|remind\s+me\s+when|watch\s+for|уведом|оповест|скажи\s+(?:мне\s+)?(?:когда|если)|сообщи|напомни\s+когда|алерт|дай\s+знать/),
              let coin = IntentParser.namedAsset(text, markets: markets), let price = level(q) else { return nil }
        let above: Bool? = q.contains(/above|over|higher|rises?|goes\s+up|climbs?|reach|выше|поднимет|вырастет|больше|превыс|достигн/) ? true
            : q.contains(/below|under|lower|drops?|falls?|goes\s+down|dips?|ниже|упад|опуст|меньше|снизит/) ? false : nil
        return .set(coin: coin, price: price, above: above)
    }

    /// The price level: «2,800» (thousands) or «93,5» (decimal comma); a number followed by % is not a price.
    static func level(_ q: String) -> Decimal? {
        let numbers = q.matches(of: /(\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:[.,]\d+)?)(?!\s*%|\d)/).map { String($0.1) }
        guard let raw = numbers.last else { return nil }
        let normal = raw.contains(/^\d{1,3}(?:,\d{3})+(?:\.\d+)?$/) ? raw.replacingOccurrences(of: ",", with: "") : raw.replacingOccurrences(of: ",", with: ".")
        return Decimal(string: normal).flatMap { $0 > 0 ? $0 : nil }
    }
}
