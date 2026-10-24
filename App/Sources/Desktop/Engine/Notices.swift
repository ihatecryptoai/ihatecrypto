import Foundation
import IhatecryptoCore
import TradeExecution
import TxGuard

/// What a Windows notification is about, as the Mac's Notice: each kind can be turned off in Settings › General ›
/// Notifications. The window shows them (main.rs); the engine decides which go out.
nonisolated enum Notice: String, CaseIterable, Codable {
    case incoming, positions, orders, alerts, moves, hlp, signature, confirm, update

    func title(_ say: (String, String) -> String) -> String {
        switch self {
        case .incoming: say("Money arrived", "Пришли деньги")
        case .positions: say("A position closed, or its stop moved", "Позиция закрылась или её стоп сдвинулся")
        case .orders: say("A limit order filled", "Исполнилась лимитная заявка")
        case .alerts: say("A price alert fired", "Сработало ценовое оповещение")
        case .moves: say("A confirmed move finished or failed", "Подтверждённое действие завершилось или не прошло")
        case .hlp: say("The HLP deposit unlocked", "Депозит в HLP разблокирован")
        case .signature: say("A scheduled move waits for Windows Hello", "Действие по расписанию ждёт Windows Hello")
        case .confirm: say("A move or a trade waits for your Confirm", "Действие или сделка ждёт твоего «Подтвердить»")
        case .update: say("A new version of ihatecrypto is ready", "Готова новая версия ihatecrypto")
        }
    }

    /// The Mac's sounds (App/Tools/make-sounds.py): a coin when money arrived or moved, a chime when something wants a
    /// look, a low knock when a move failed. The window plays it (main.rs), its toast silent.
    func sound(failed: Bool = false) -> String {
        failed ? "knock" : [.incoming, .orders, .moves, .hlp].contains(self) ? "coin" : "chime"
    }
}

/// The last notices' memory: Hyperliquid's positions and resting entries at the last read, and when one last went out.
@MainActor enum NoticeMemory {
    static var positions: Set<String>?
    static var entries: Set<String> = []
    static var last = Date.distantPast
}

extension Engine {
    struct NoticeView: Encodable, Equatable { var id: String; var title: String; var on: Bool }

    var noticeViews: [NoticeView] {
        let off = Set(settings.noticesOff ?? [])
        return Notice.allCases.map { .init(id: $0.rawValue, title: $0.title { say($0, $1) }, on: !off.contains($0.rawValue)) }
    }

    /// One Windows notification, unless its kind is off. `whenAway`: only while the window isn't in front (a card the
    /// person may not be looking at); main.rs checks that. `record`: what ihatecrypto did by itself (money arrived, an
    /// alert, the guardian, a limit filled, HLP unlocked) is a History record too, notified or not, as the Mac's
    /// notify(record:); a confirmed move has its own, and a card waiting or an update isn't one.
    /// `schedule`: the Schedule item it's about («perp:BTC»), as the Mac's notify(schedule:).
    func notify(_ kind: Notice, _ title: String, _ body: String, failed: Bool = false, whenAway: Bool = false, record: Bool? = nil, schedule: String? = nil) {
        if record ?? ![.moves, .confirm, .update].contains(kind) { recordAutomatic(title, body, failed: failed, schedule: schedule) }
        guard !(settings.noticesOff ?? []).contains(kind.rawValue) else { return }
        NoticeMemory.last = Date()
        var event = ["kind": kind.rawValue, "title": title, "body": body, "sound": kind.sound(failed: failed)]
        if failed { event["failed"] = "1" }
        if whenAway { event["whenAway"] = "1" }
        out.event("notify", event)
    }

    func setNotice(_ id: String, on: Bool) {
        guard Notice(rawValue: id) != nil else { return }
        var off = Set(settings.noticesOff ?? [])
        if on { off.remove(id) } else { off.insert(id) }
        settings.noticesOff = off.isEmpty ? nil : off.sorted()
    }

    /// Settings' «Send a test»: always goes out, whatever is switched off.
    func noticeTest() {
        out.event("notify", ["kind": "test", "title": "ihatecrypto", "sound": Notice.incoming.sound(),
                             "body": say("Notifications work: scheduled tasks, stops, alerts and moves waiting for Confirm show up here.",
                                         "Уведомления работают: задачи по расписанию, стопы, оповещения и действия, ждущие подтверждения, появятся здесь.")])
    }

    /// A confirmed move's end: done or failed, with the chat saying why.
    func noticeMove(_ title: String, state: String?) {
        let ok = state != "failed" && state != "unknown"
        notify(.moves, title, ok ? say("Done — checked, signed and confirmed.", "Готово — проверено, подписано и подтверждено сетью.")
                                 : state == "unknown" ? say("Sent, status unknown — check the links in the chat before doing it again.", "Отправлено, статус неизвестен — проверь ссылки в чате, прежде чем повторять.")
                                 : say("It didn’t go through; the chat says why.", "Не прошло — причина в чате."), failed: !ok)
    }

    /// A card on the canvas while the person looks elsewhere. Not right after another notice (a scheduled move says so
    /// itself): one tap is enough.
    func noticeCard(_ title: String) {
        guard Date().timeIntervalSince(NoticeMemory.last) > 5 else { return }
        notify(.confirm, say("Waiting for your Confirm", "Ждёт твоего «Подтвердить»"), title, whenAway: true)
    }

    /// Open at login, from Settings or the onboarding: kept, and main.rs told to set Windows' own entry (it answers with
    /// `loginState`, so the switch shows what Windows has).
    func setOpenAtLogin(_ on: Bool) {
        settings.openAtLogin = on
        out.event("openAtLogin", ["on": on ? "1" : "0"])
    }

    /// The updater (main.rs) found a newer version.
    /// The version the updater found, until this copy restarts into it.
    nonisolated(unsafe) static var availableUpdate: String?

    func updateReady(_ version: String) {
        Self.availableUpdate = version
        changed()
        notify(.update, say("ihatecrypto \(version) is ready", "Готова ihatecrypto \(version)"),
               say("Click to open ihatecrypto and install it from Settings › About.", "Нажми, чтобы открыть ihatecrypto и поставить её в «Настройки › О приложении»."))
    }

    /// The window or the engine stopped unexpectedly last time (main.rs keeps the report): one line in the chat.
    func crashed(at date: String, summary: String, folder: String) {
        // main.rs passes the report's time in seconds since 1970.
        let when = Double(date).map { Fmt.date(Date(timeIntervalSince1970: $0)) } ?? date
        reply(say("ihatecrypto closed unexpectedly on \(when) — \(summary). The report is kept in \(folder).",
                  "ihatecrypto упал \(when) — \(summary). Отчёт сохранён в \(folder)."))
    }

    /// What happened on Hyperliquid since the last read, as the Mac's refresh says it: a resting limit entry that filled,
    /// the HLP deposit's lock that ended. (A position closed outside ihatecrypto is the guardian's: PositionGuard.swift.)
    func noticeHyperliquid() async {
        guard !watching, let user = evmAddress else { return }
        let now = Set(positions.map(\.coin))
        let before = NoticeMemory.positions
        NoticeMemory.positions = now
        // Not while one of ours runs: opening it is the move itself.
        if let before, running == nil, Date().timeIntervalSince(lastOwnMove) > 60 {
            for p in positions where !before.contains(p.coin) && NoticeMemory.entries.contains(p.coin) {
                notify(.orders, say("Limit filled: \(IntentParser.pretty(p.coin))", "Лимитка исполнилась: \(IntentParser.pretty(p.coin))"),
                       say("\(p.size > 0 ? "Long" : "Short") \(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)); its stop and take are on the exchange.",
                           "\(p.size > 0 ? "Лонг" : "Шорт") \(Fmt.num(abs(p.size), maxFraction: 6)) по \(Fmt.price(p.entry)); стоп и тейк стоят на бирже."),
                       schedule: "perp:" + p.coin)
            }
        }
        // ponytail: its own read of the open orders, one more request a minute; the guardian's `hlOrders` can feed it.
        if let orders = try? await HyperliquidAccount.openOrders(user), evmAddress == user {
            NoticeMemory.entries = Set(orders.filter { !$0.isTrigger && !$0.reduceOnly }.map(\.coin))
        }
        // The HLP deposit's 4-day lock ended: said once per lock.
        if let v = try? await HyperliquidAccount.vaultEquity(user: user, vault: HLGuard.hlpVault), evmAddress == user,
           let until = v.lockedUntil, until <= Date(), v.equity > 0 {
            let key = until.timeIntervalSince1970
            if Store.load(Double.self, "hlp-unlock-noticed.json") != key {
                Store.save(key, "hlp-unlock-noticed.json")
                notify(.hlp, say("HLP unlocked", "HLP разблокирован"),
                       say("\(Fmt.usd(v.equity)) in the HLP vault can be withdrawn now.", "\(Fmt.usd(v.equity)) в хранилище HLP теперь можно вывести."))
            }
        }
    }
}
