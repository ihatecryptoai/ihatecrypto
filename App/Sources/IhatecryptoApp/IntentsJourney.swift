import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import SwiftUI
import TradeExecution
import WalletCore

extension Brand {
    /// NEAR's green (N5): the route through NEAR Intents.
    static let near = Color(hex: 0x00EC97)
    static let nearInk = Color(light: 0x00875A, dark: 0x00EC97)
    /// Privacy's violet (P3): a send nobody can link to the sender.
    static let privacy = Color(hex: 0xB9A6FF)
    static let privacyInk = Color(light: 0x5B3AA8, dark: 0xB9A6FF)
    static let privacyFill = Color(light: 0xEFEAFF, dark: 0x2F2846)
}

// MARK: - N5 · the journey after Confirm

/// N5 «Live journey» (owner's pick, 2026-11-06): a move through NEAR Intents after Confirm, as the three places its money
/// passes: paid where it leaves, a solver filling it on NEAR, landed where it goes. Read from 1Click's own states, as the
/// executor reports them (Executor.oneClickSettled).
struct IntentsJourney {
    enum Stage: Int, Comparable {
        case paying, paid, filling, landed, refunded
        static func < (a: Stage, b: Stage) -> Bool { a.rawValue < b.rawValue }
    }

    let title: String
    /// Places: «solana», «base», «bitcoin».
    let from: String, to: String
    let privately: Bool
    let started: Date
    /// 1Click's own estimate, from the deposit to the payout.
    var estimate: Int?
    var stage = Stage.paying
    var paidAt: Date?
    var endedAt: Date?
    /// It ended without 1Click's last word (a timeout, a dropped connection): it settles or refunds on its own.
    var stuck = false
    /// 1Click's deposit address: the explorer's page for this move.
    var deposit: String?
    /// 1Click said REFUNDED: the money went back. FAILED alone doesn't say where it is.
    var refunded = false

    /// The moves that go through NEAR Intents and wait for its solvers: a send paid out there, private or not, and USDC
    /// moved «via NEAR Intents». nil for any other.
    @MainActor init?(_ ws: Workspace, _ c: WalletCommand) {
        switch c {
        case .sendPrivately(let name, _, let x, _), .sendToken(let name, _, let x?, _):
            guard let r = ws.acrossReceipt, r.command == c else { return nil }
            var isPrivate = false
            if case .sendPrivately = c { isPrivate = true }
            self.init(title: "\(Fmt.num(x, maxFraction: 9)) \(r.symbol) → \(name)", from: EVMNetwork.all.first { $0.name == r.from }?.key ?? "solana",
                      to: r.network, privately: isPrivate, estimate: r.seconds)
        case .swapPrivately(let a, let from, let b, let to, let x):
            let way = ws.swapQuote.flatMap { $0.command == c ? $0.ways.first?.way : nil }
            self.init(title: "\(Fmt.num(x, maxFraction: 8)) \(a.uppercased()) → \(b.uppercased())", from: from, to: to, privately: true,
                      estimate: way.flatMap { $0.firstMatch(of: /≈ (\d+) s/) }.flatMap { Int($0.1) })
        case .intents(let a, let b, let x):
            let quoted = ws.swapQuote.flatMap { $0.command == c ? $0.ways.first?.gets : nil }
            // All of it reads as its number: «all · 3.8 USDC».
            let amount = x.map { Fmt.num($0) } ?? (a == "solana" ? ws.solana?.usdc : ws.usdc(on: a)).map { "all · \(Fmt.num($0))" } ?? "all"
            self.init(title: "\(amount) USDC → \(WalletCommand.placeName(b))", from: a, to: b, privately: false,
                      estimate: quoted.flatMap { $0.firstMatch(of: /≈ (\d+) s/) }.flatMap { Int($0.1) })
        default:
            return nil
        }
    }

    init(title: String, from: String, to: String, privately: Bool, estimate: Int?, started: Date = Date()) {
        (self.title, self.from, self.to, self.privately, self.estimate, self.started) = (title, from, to, privately, estimate, started)
    }

    /// A line the executor reported: «1Click · deposit received, swapping · 12 s of ≈ 30».
    mutating func read(_ line: String, now: Date = Date()) {
        let parts = line.components(separatedBy: " · ")
        guard parts.count >= 2, parts[0] == "1Click" else { return }
        if let m = line.firstMatch(of: /of ≈ (\d+)/), let e = Int(m.1) { estimate = e }
        guard let status = Executor.oneClickWords.first(where: { $0.value == parts[1] })?.key else { return }
        let next: Stage = switch status {
        case "PROCESSING": .filling
        case "SUCCESS": .landed
        case "REFUNDED", "FAILED": .refunded
        // INCOMPLETE_DEPOSIT isn't final: 1Click refunds it, and its next word says so.
        default: .paid
        }
        if status == "REFUNDED" { refunded = true }
        if next >= .paid, paidAt == nil { paidAt = now }
        if next > stage { stage = next }
        if next >= .landed { endedAt = now }
    }

    /// The move came back: landed, or it failed. One that failed before anything was paid isn't a journey: nil.
    /// `unknown`: the journal holds a deposit sent with no outcome — paid, and still on its way as far as anyone knows.
    func ended(ok: Bool, unknown: Bool = false, now: Date = Date()) -> IntentsJourney? {
        var j = self
        if j.endedAt == nil { j.endedAt = now }
        if ok { j.stage = .landed; return j }
        if j.stage == .paying, unknown { j.stage = .paid }
        if j.stage == .paying { return nil }
        if j.stage < .landed { j.stuck = true }
        return j
    }

    /// Top right: how long is left, or how it ended.
    func clock(_ now: Date) -> String {
        switch stage {
        case .landed: return "landed in " + Self.duration((endedAt ?? now).timeIntervalSince(started))
        case .refunded: return refunded ? "refunded" : "failed"
        case .paying: return "signing"
        case .paid, .filling:
            if stuck { return "still on its way" }
            guard let estimate, let paidAt else { return Self.duration(now.timeIntervalSince(started)) }
            let left = estimate - Int(now.timeIntervalSince(paidAt))
            return left > 0 ? "≈ \(Self.duration(TimeInterval(left))) left" : "any moment now"
        }
    }

    /// The bar under the three places.
    func progress(_ now: Date) -> Double {
        switch stage {
        case .paying: return 0.06
        case .landed, .refunded: return 1
        case .paid, .filling:
            guard let estimate, let paidAt else { return 0.5 }
            return 0.2 + 0.75 * min(1, now.timeIntervalSince(paidAt) / Double(max(estimate, 1)))
        }
    }

    var explorer: URL? { deposit.flatMap { URL(string: "https://explorer.near-intents.org/transactions/\($0)") } }

    static func duration(_ t: TimeInterval) -> String {
        let s = max(0, Int(t.rounded()))
        return s < 120 ? "\(s) s" : String(format: "%d:%02d", s / 60, s % 60)
    }
}

extension Workspace {
    /// A report line during a NEAR Intents move: the journey's stage, and 1Click's deposit from the journal once paid.
    func journeyRead(_ line: String) {
        guard var j = journey, j.endedAt == nil else { return }
        j.read(line)
        if j.stage >= .paid, j.deposit == nil {
            let entries: [Journal.Entry] = (try? Journal(url: journalURL).entries()) ?? []
            let since = j.started.addingTimeInterval(-1)
            j.deposit = entries.last { $0.chain == "1click" && $0.kind == .signed && $0.at >= since }?.id
        }
        journey = j
    }
}

/// N5 in the chat's field, where the receipt was before Confirm: the three places the money passes, the time left, and
/// NEAR Intents' explorer for this move. It stays after the money lands until the next message.
struct JourneyBar: View {
    @Environment(Workspace.self) private var ws
    let journey: IntentsJourney

    var body: some View {
        Group {
            if let end = journey.endedAt { content(end) }
            else { TimelineView(.periodic(from: journey.started, by: 1)) { content($0.date) } }
        }
        .padding(.vertical, 12).padding(.horizontal, 14)
        .background(Brand.near.opacity(0.06))
    }

    private func content(_ now: Date) -> some View {
        let j = journey, failed = j.stage == .refunded || j.stuck
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if j.privately { Image(systemName: "lock.fill").font(.system(size: 11, weight: .bold)).foregroundStyle(Brand.privacyInk) }
                Text(j.title).font(.ui(15, .bold)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.8)
                Spacer(minLength: 6)
                Text(j.clock(now)).font(.ui(11.5)).foregroundStyle(failed ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary)).monospacedDigit()
            }
            HStack(alignment: .top, spacing: 0) {
                place(logo: logo(j.from), current: j.stage == .paying, done: j.stage >= .paid,
                      title: j.stage >= .paid ? "Paid on \(WalletCommand.placeName(j.from))" : "Paying on \(WalletCommand.placeName(j.from))",
                      line: j.paidAt.map { IntentsJourney.duration($0.timeIntervalSince(j.started)) } ?? "now")
                link(lit: j.stage >= .paid)
                place(logo: "near", current: j.stage == .paid || j.stage == .filling, done: j.stage == .landed, failed: j.stage == .refunded,
                      title: solverTitle(j), line: solverLine(j), ink: Brand.nearInk)
                link(lit: j.stage == .landed, dashed: j.stage < .landed)
                place(logo: logo(j.to), current: false, done: j.stage == .landed,
                      title: "On \(WalletCommand.placeName(j.to))",
                      line: j.stage == .landed ? j.endedAt.map { IntentsJourney.duration($0.timeIntervalSince(j.started)) } ?? "" : j.stage == .refunded ? "didn't go" : "next")
            }
            GeometryReader { g in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.primary.opacity(0.08))
                    Capsule().fill(failed ? Color.orange : Brand.near).frame(width: g.size.width * j.progress(now))
                }
            }
            .frame(height: 6)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(footnote(j)).font(.ui(10.5)).foregroundStyle(.secondary).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 6)
                if let url = j.explorer {
                    Button { ws.openWeb(url) } label: { Text("Track on NEAR ↗").font(.ui(11, .semibold)).foregroundStyle(Brand.nearInk) }
                        .buttonStyle(.plain).pointerStyle(.link).fixedSize()
                        .help("This move on NEAR Intents' explorer")
                }
            }
        }
    }

    private func logo(_ place: String) -> String { AddressCheck.Network(rawValue: place)?.logo ?? "sol" }

    private func solverTitle(_ j: IntentsJourney) -> String {
        switch j.stage {
        case .paying: "Solver"
        case .paid: "Deposit reaching NEAR"
        case .filling: "Solver filling it"
        case .landed: "Filled on NEAR"
        case .refunded: "No fill"
        }
    }

    private func solverLine(_ j: IntentsJourney) -> String {
        switch j.stage {
        case .paying: "on NEAR"
        case .paid, .filling: j.stuck ? "settles on its own" : "now · on NEAR"
        case .landed: j.privately ? "unlinkable" : "by a solver"
        case .refunded: j.refunded ? "money went back" : "failed"
        }
    }

    private func footnote(_ j: IntentsJourney) -> String {
        if j.stage == .landed { return j.privately ? "Delivered privately: nobody can link it to your deposit." : "Delivered through NEAR Intents." }
        if j.stage == .refunded {
            return j.refunded ? "No solver filled it, so the USDC went back to \(WalletCommand.placeName(j.from))."
                              : "NEAR Intents says it failed. The chat says where the money is."
        }
        if j.stuck { return "NEAR Intents settles or refunds it on its own. Don't send it again." }
        return "If no solver fills it, your USDC comes back."
    }

    private func place(logo: String, current: Bool, done: Bool, failed: Bool = false, title: String, line: String, ink: Color? = nil) -> some View {
        VStack(spacing: 5) {
            TokenLogo(name: logo, size: 34, round: true)
                .padding(3)
                .overlay(Circle().strokeBorder(current ? Brand.near : Color.clear, lineWidth: 2.5))
                .overlay(alignment: .bottomTrailing) {
                    if done || failed {
                        Image(systemName: failed ? "xmark" : "checkmark").font(.system(size: 8, weight: .heavy)).foregroundStyle(Color(hex: 0x1D1B19))
                            .frame(width: 16, height: 16)
                            .background(Circle().fill(failed ? Color.orange : Brand.tee))
                            .overlay(Circle().strokeBorder(Brand.card, lineWidth: 1.5))
                            .offset(x: 3, y: 3)
                    }
                }
                .opacity(done || current || failed ? 1 : 0.45)
            Text(title).font(.ui(11, .semibold)).multilineTextAlignment(.center).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                .foregroundStyle(done || current ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
            Text(line).font(.ui(10)).foregroundStyle(current && ink != nil ? AnyShapeStyle(ink!) : AnyShapeStyle(.secondary)).monospacedDigit().lineLimit(1)
        }
        .frame(width: 86)
    }

    private func link(lit: Bool, dashed: Bool = false) -> some View {
        Line().stroke(lit ? Brand.near : Color.primary.opacity(0.2), style: StrokeStyle(lineWidth: 3, lineCap: .round, dash: dashed && !lit ? [5, 5] : []))
            .frame(height: 3).padding(.top, 19).frame(maxWidth: .infinity)
    }

    private nonisolated struct Line: Shape {
        func path(in r: CGRect) -> Path { Path { p in p.move(to: CGPoint(x: r.minX + 2, y: r.midY)); p.addLine(to: CGPoint(x: r.maxX - 2, y: r.midY)) } }
    }
}

// MARK: - P3 · regular or private

/// P3 «Regular or private» (owner's pick, 2026-11-06): a USDC send to a saved address waits for Confirm with the other
/// way beside it. Regular goes on its network as it is; private goes through NEAR Intents' Confidential Intents, where
/// nobody can link the payout to the deposit. Either way it waits for Confirm.
struct PrivacyChoice {
    var isPrivate: Bool
    var rows: [(label: String, value: String, note: String, logo: String?)]
}

extension Workspace {
    /// Where a private send pays USDC (OneClick.Asset.payout), and the networks a plain transfer goes on.
    static let privateUSDCNetworks: Set<String> = ["solana", "base", "arbitrum", "ethereum"]

    /// The send waiting for Confirm and the same send the other way, when both can go: USDC to a saved address on Solana,
    /// Base, Arbitrum or Ethereum, with a NEAR Intents key. nil for anything else.
    func privacyTwin() -> (isPrivate: Bool, other: WalletCommand)? {
        switch pendingAction {
        case .transfer(let entry, let amount)?:
            let net = entry.network.rawValue
            guard Self.privateUSDCNetworks.contains(net), (try? privateRecipient(entry.name, token: "USDC", network: net)) != nil else { return nil }
            return (false, .sendPrivately(name: entry.name, token: "USDC", amount: amount, network: net))
        case .wallet(.sendPrivately(let name, let token, let x, let net))?:
            guard token.uppercased() == "USDC", let r = try? privateRecipient(name, token: token, network: net),
                  Self.privateUSDCNetworks.contains(r.network) else { return nil }
            return (true, .sendTo(name: name, amount: x, network: r.network))
        // A swap through NEAR Intents and its private twin through Confidential Intents (the partner key's).
        case .wallet(let c)?:
            if case .swapPrivately = c { return c.madePublic.map { (true, $0) } }
            guard OneClick.key != nil, let p = c.madePrivate, case .swapPrivately = p else { return nil }
            return (false, p)
        default:
            return nil
        }
    }

    /// Why a private swap can't be drawn: Confidential Intents needs the NEAR Intents partner key.
    func privateSwapRefusal(_ c: WalletCommand) -> String? {
        switch c {
        case .limitOrder, .limitOrders, .limitCancel:
            guard OneClick.key == nil else { return nil }
            return say("Limit orders go through NEAR Intents and belong to its partner key: add one in Settings › General › NEAR Intents key (made at https://partners.near-intents.org → API Keys).",
                       "Лимитные ордера идут через NEAR Intents и привязаны к его партнёрскому ключу: добавь ключ в Settings › General › NEAR Intents key (его делают на https://partners.near-intents.org → API Keys).")
        default: break
        }
        guard case .swapPrivately = c, OneClick.key == nil else { return nil }
        return say("A private swap needs a NEAR Intents partner key: add it in Settings › General › NEAR Intents key.",
                   "Для приватного обмена нужен партнёрский ключ NEAR Intents: добавь его в Settings › General › NEAR Intents key.")
    }

    /// The private card's violet: the field and its head while a private send waits.
    var pendingIsPrivate: Bool {
        switch pendingAction {
        case .wallet(.sendPrivately)?, .wallet(.swapPrivately)?, .wallet(.limitOrder)?: return true
        default: return false
        }
    }

    /// The other way picked over Confirm: its card takes this one's place, with a line in the chat. Nothing is signed.
    func switchPrivacy() async {
        guard !busy, !executing, let twin = privacyTwin() else { return }
        await handleWallet(twin.other)
    }

    /// What the private twin of a regular send waiting for Confirm costs and how long it takes, quoted dry (nothing is
    /// issued), for the regular card's comparison: afresh each time the card is drawn. It's quoted to this wallet's own
    /// address there, never the payee's: a plain transfer tells 1Click nobody it pays.
    func quotePrivateTwin() async {
        privateTwin = nil
        // A swap's twin: what Confidential Intents gives for the same amount, to this wallet's own addresses.
        if let twin = privacyTwin(), !twin.isPrivate, case .swapPrivately(let a, let from, let b, let to, let x) = twin.other {
            guard let wallet = previewWallet,
                  let q = try? await executor(wallet).intentQuoteText(a, on: from, for: b, on: to, amount: x, near: nearAddress, confidential: true),
                  !Task.isCancelled else { return }
            privateTwin = (twin.other, nil, q.way.firstMatch(of: /≈ (\d+) s/).flatMap { Int($0.1) }, q.gets)
            return
        }
        guard let twin = privacyTwin(), !twin.isPrivate, case .sendPrivately(let name, let token, let x, let net) = twin.other,
              let wallet = previewWallet, let r = try? privateRecipient(name, token: token, network: net),
              let q = try? await executor(wallet).acrossQuote(r.symbol, amount: x, network: r.network,
                                                              to: r.network == "solana" ? wallet.solana : wallet.evm, confidential: true),
              !Task.isCancelled else { return }
        let cost = Decimal(string: q.quote.amountInFormatted) ?? 0
        privateTwin = (twin.other, q.quote.amountOutUSD.map { max(0, cost - $0) }, q.quote.timeEstimate, nil)
    }

    /// P3's rows for the send waiting for Confirm: what it pays from where, the fees, when it arrives and what it goes
    /// through, each with the other way's figure beside it when known.
    var privacyChoice: PrivacyChoice? {
        guard let twin = privacyTwin() else { return nil }
        func seconds(_ s: Int) -> String { s >= 120 ? "≈ \(s / 60) min" : "≈ \(s) s" }
        switch pendingAction {
        case .transfer(let entry, let amount)?:
            let net = entry.network.rawValue, name = entry.network.title
            let quick = (usdc(on: net) ?? amount) >= amount || net == "solana"
            let twinQuote = privateTwin.flatMap { $0.command == twin.other ? $0 : nil }
            return PrivacyChoice(isPrivate: false, rows: [
                ("You pay", "\(Fmt.num(amount)) USDC", quick ? "from \(name)" : "the rest from Solana first", nil),
                ("Blockchain fees", "network gas", twinQuote?.fee.map { "private ≈ \(Fmt.usd($0))" } ?? "none to us", nil),
                ("Arrives", quick ? "in seconds" : "after a bridge", twinQuote?.seconds.map { "private \(seconds($0))" } ?? "", nil),
                ("Goes through", "\(name) directly", "anyone can see it's from you", entry.network.logo),
            ])
        case .wallet(let c)? where (twin.isPrivate ? c : twin.other).isPrivateSwap:
            guard case .swapPrivately(let a, let from, _, _, let x) = twin.isPrivate ? c : twin.other else { return nil }
            let own = swapQuote.flatMap { $0.command == c ? $0.ways.first : nil }
            let failed = swapQuote.map { $0.command == c && $0.ways.isEmpty } ?? false
            // What this card gets: its quote, else what its last step says it lands.
            let gets = own.map { "≈ \($0.gets)" } ?? route(c)?.steps.last.map { readable($0.title) } ?? "…"
            let pays = ("You pay", "\(Fmt.num(x, maxFraction: 8)) \(a.uppercased())", "on \(WalletCommand.placeName(from))", nil as String?)
            if twin.isPrivate {
                return PrivacyChoice(isPrivate: true, rows: [
                    pays,
                    ("You get", gets, failed ? "no quote now" : "", nil),
                    ("Arrives", own.flatMap { $0.way.firstMatch(of: /≈ (\d+) s/) }.flatMap { Int($0.1) }.map { "in \(seconds($0))" } ?? "…", "", nil),
                    ("Goes through", "Confidential Intents", "nobody can match the two ends", "near"),
                ])
            }
            let twinQuote = privateTwin.flatMap { $0.command == twin.other ? $0 : nil }
            return PrivacyChoice(isPrivate: false, rows: [
                pays,
                ("You get", gets, twinQuote?.gets.map { "private ≈ \($0)" } ?? "", nil),
                ("Goes through", own?.way ?? "…", "anyone can see the swap", nil),
            ])
        case .wallet(let c)?:
            let r = acrossReceipt.flatMap { $0.command == c ? $0 : nil }
            let fee = r.flatMap { r in r.worth.map { max(0, r.pays - $0) } }
            // A quote that came back empty (no network holds what it costs) says so, not «getting the price».
            let failed = swapQuote.map { $0.command == c && $0.ways.isEmpty } ?? false
            return PrivacyChoice(isPrivate: true, rows: [
                ("You pay", r.map { "≈ \(Fmt.num($0.pays)) USDC" } ?? "…", r.map { "from \($0.from)" } ?? (failed ? "no quote now" : "getting the price"), nil),
                ("Blockchain fees", fee.map { "≈ \(Fmt.usd($0))" } ?? "…", "regular: network gas · none to us", nil),
                ("Arrives", r?.seconds.map { "in \(seconds($0))" } ?? "…", "regular in seconds", nil),
                ("Goes through", "NEAR Intents", "nobody can link it to you", "near"),
            ])
        default:
            return nil
        }
    }
}

/// P3 in the receipt: Regular | Private, then the rows of the one picked.
struct PrivacyChoiceRows: View {
    @Environment(Workspace.self) private var ws
    let choice: PrivacyChoice

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 4) {
                option(false, "Regular", "anyone can see it's from you")
                option(true, "Private", "nobody can link it to you")
            }
            .padding(3)
            .background(RoundedRectangle(cornerRadius: 13, style: .continuous).fill(Color.primary.opacity(0.06)))
            .accessibilityElement(children: .contain)
            .accessibilityLabel("How to send")
            VStack(alignment: .leading, spacing: 5) {
                ForEach(choice.rows.indices, id: \.self) { i in
                    let row = choice.rows[i]
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(row.label).font(.ui(11.5)).foregroundStyle(.secondary).frame(width: 96, alignment: .leading)
                        HStack(spacing: 5) {
                            if let logo = row.logo { TokenLogo(name: logo, size: 14, round: true).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 } }
                            Text(row.value).font(.ui(12, .semibold)).monospacedDigit().lineLimit(1)
                        }
                        Spacer(minLength: 6)
                        Text(row.note).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.85)
                    }
                }
            }
        }
    }

    private func option(_ isPrivate: Bool, _ title: String, _ line: String) -> some View {
        let on = choice.isPrivate == isPrivate
        return Button { if !on { Task { await ws.switchPrivacy() } } } label: {
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    if isPrivate { Image(systemName: "lock.fill").font(.system(size: 10, weight: .bold)).foregroundStyle(Brand.privacyInk) }
                    Text(title).font(.ui(12.5, .semibold)).foregroundStyle(on ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                }
                Text(line).font(.ui(10.5)).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.85)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 9).padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(on ? (isPrivate ? Brand.privacyFill : Brand.glassSelected) : Color.clear))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(on ? (isPrivate ? Brand.privacy : Brand.cardBorder) : Color.clear))
            .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .buttonStyle(.plain)
        .pointerStyle(on ? nil : .link)
        .disabled(ws.busy || ws.executing)
        .accessibilityAddTraits(on ? [.isSelected] : [])
        .help(on ? "" : isPrivate ? "Send it through NEAR Intents' Confidential Intents instead: the card changes, it still waits for Confirm"
                                  : "Send it as a plain transfer instead: the card changes, it still waits for Confirm")
    }
}

extension WalletCommand {
    var isPrivateSwap: Bool { if case .swapPrivately = self { true } else { false } }
}
