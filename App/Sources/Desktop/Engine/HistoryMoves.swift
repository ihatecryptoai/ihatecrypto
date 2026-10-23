import Foundation
import IhatecryptoCore

/// One move ihatecrypto made or saw, as the History block lists it (the Mac's HistoryBoard): what it was, how much, where
/// to, when; the request it came from and the reply it was in, opened in place; the canvas it was said on.
nonisolated struct MoveRow: Encodable, Equatable {
    var at: Date
    var kind: String
    var text: String
    var amount: Decimal?
    var symbol: String?
    var to: String?
    /// Sent to one of this computer's own wallets: still the user's money, outside the balance.
    var own: Bool
    var link: String?
    var request: String?
    var reply: String
    var canvas: String
}

/// What the latest day did to the money: «−25 USDT to Automation · 5 sends», and the own wallets it went to.
nonisolated struct MovesDay: Encodable, Equatable {
    var day: Date
    var lines: [String]
    var own: [String]
}

extension Engine {
    /// Every move in the replies, on every canvas, newest first, each with the request it answered.
    private var allMoves: [(row: MoveRow, line: MovedLine)] {
        let own = Set((status?.wallets ?? []).map(\.label))
        var out: [(MoveRow, MovedLine)] = []
        for c in canvases {
            var request: String?
            for m in c.messages {
                if m.fromUser { request = m.text; continue }
                for l in m.text.split(separator: "\n").compactMap({ MovedLine(String($0)) }) {
                    out.append((MoveRow(at: m.at, kind: l.kind.rawValue, text: l.text, amount: l.amount, symbol: l.symbol, to: l.to,
                                        own: l.to.map(own.contains) ?? false, link: l.link?.absoluteString, request: request, reply: m.text, canvas: c.id), l))
                }
            }
        }
        return out.sorted { $0.0.at > $1.0.at }
    }

    /// The block's rows: the newest 14.
    var historyMoves: [MoveRow] { Array(allMoves.prefix(14).map(\.row)) }

    /// The newest move's day, summed up the shared way (MovedLine.summary), which the chat reads too.
    var movesDay: MovesDay? {
        let moves = allMoves
        guard let latest = moves.first else { return nil }
        let day = Calendar.current.startOfDay(for: latest.row.at)
        let that = moves.filter { Calendar.current.startOfDay(for: $0.row.at) == day }
        return MovesDay(day: day, lines: MovedLine.summary(that.map(\.line)), own: Array(Set(that.filter(\.row.own).compactMap(\.row.to))).sorted())
    }
}
