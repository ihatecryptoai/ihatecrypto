import ExecutionCore
import Foundation
import IhatecryptoCore
import LLMCatalog
import LLMCore
import SignerAPI
import TradeExecution
import WalletCore

/// One chat message, as the window shows it and the model reads it.
nonisolated struct Message: Codable, Equatable, Identifiable {
    var id = UUID().uuidString
    var fromUser: Bool
    var text: String
    /// Under the reply, small: which model, which guard, which network.
    var note: String?
    var at = Date()
    /// A price chart drawn under the text.
    var chart: ChartLine?
    /// What a block the person pointed at told the model with this message, shown over it.
    var context: String?
}

/// A market's closes over a range, for the window to draw as a line.
nonisolated struct ChartLine: Codable, Equatable {
    var title: String
    var closes: [Double]
    var low: String
    var high: String
    var change: String
}

/// What waits for the Confirm button. Only the button carries one out (`confirm`): a typed «yes» never does.
enum Pending {
    case wallet(WalletCommand)
    /// USDC to an address-book entry (`send N USDC to <name>`).
    case transfer(SignerAddress, Decimal)
    case position(PositionCommand, coin: String)
    case trade(Executor.Prepared)
    case address(SignerAddress)
    /// «withdraw 5 USDC to Solana then deposit 5 USDC to kamino»: one Confirm, the steps in order, the first failure stops them.
    case chain([ChainStep])
    /// The first buy of a recurring one: the schedule exists only once this buy is confirmed and goes through.
    case recurring(WalletCommand, days: Int)
    /// A site in the browser asks for the wallet or a signature: Confirm lets that one request through.
    case site(SiteAsk)
}

enum ChainStep {
    case wallet(WalletCommand)
    case transfer(SignerAddress, Decimal)

    var title: String {
        switch self {
        case .wallet(let c): c.summary
        case .transfer(let e, let amount): "send \(Fmt.num(amount)) USDC to \(e.name)"
        }
    }
}

/// The card the window draws for what's pending: what will happen, in lines, and what to look at first.
nonisolated struct Card: Encodable, Equatable {
    var id: String
    var title: String
    var lines: [String] = []
    var warnings: [String] = []
    var confirm: String
}

/// The desktop app's engine: the window (Tauri, App/Windows/app) sends it what the person did, one JSON line at a time,
/// and gets back the whole state to draw after every change. It reads the chat with the model through ChatRouter, plans
/// and checks moves with the same IhatecryptoCore and TradeExecution as the Mac, and signs only through the signing
/// process, after Confirm.
final class Engine {
    let out: Outbox
    var settings = Store.load(Settings.self, "settings.json") ?? Settings() {
        didSet { Store.save(settings, "settings.json"); changed() }
    }

    // Signer
    var status: SignerReply?
    var signerError: String?
    var book: [SignerAddress] = []
    var signing = 0
    /// Shown once, right after a new wallet was made, until the person says they wrote it down.
    var phrase: [String]?

    // Markets and balances
    var markets: [String: Market] = [:]
    var portfolio = PortfolioView()
    var positions: [HyperliquidAccount.Position] = []
    var refreshing = false
    var refreshingEveryMinute = false
    /// When the user's last move ran: the balance change it made isn't announced as money arriving.
    var lastOwnMove = Date.distantPast
    /// The wallet the balances on screen were read for.
    var portfolioOwner: String?
    /// What each coin cost: read from the chain (by amount held then), recorded by the app's own buys, or first seen.
    var chainCost: [String: (held: Decimal, cost: CostBasis)] = [:]
    var appCosts: [String: CostBasis] = Store.load([String: CostBasis].self, "costs.json") ?? [:] {
        didSet { Store.save(appCosts, "costs.json") }
    }
    var seenCosts: [String: CostBasis] = Store.load([String: CostBasis].self, "seen.json") ?? [:] {
        didSet { Store.save(seenCosts, "seen.json") }
    }
    var costsLoading = false
    /// A service's price waiting for Confirm («pay GET https://…»).
    var payment: PaymentQuote?
    /// Skills the user added, the ones switched off, the ones loaded in this chat, and what they asked it to remember.
    var addedSkills: [Skill] = Store.load([Skill].self, "skills.json") ?? [] { didSet { Store.save(addedSkills, "skills.json") } }
    var skillsOff: Set<String> = Set(Store.load([String].self, "skills-off.json") ?? []) { didSet { Store.save(skillsOff.sorted(), "skills-off.json") } }
    var loadedSkills: [String] = []
    var memory: [String] = Store.load([String].self, "memory.json") ?? [] { didSet { Store.save(memory, "memory.json") } }
    var favorites: [FavoriteToken] = Store.load([FavoriteToken].self, "favorites.json") ?? [] { didSet { Store.save(favorites, "favorites.json"); changed() } }

    // Canvases: each has its own chat and blocks.
    lazy var canvases: [CanvasDoc] = Self.loadCanvases()
    lazy var canvasID: String = settings.canvas.flatMap { id in canvases.contains { $0.id == id } ? id : nil } ?? canvases[0].id
    var messages: [Message] {
        get { canvases[canvasIndex].messages }
        set { canvases[canvasIndex].messages = newValue }
    }

    // Chat
    var thinking = false
    var draft: String?
    var pending: Pending?
    var card: Card?
    var options: [ChatRouter.Option] = []
    var running: (title: String, lines: [String])?
    /// The block of the move that runs now.
    var runningBoard: String?
    /// The block the message being read is about.
    var attached: Board?
    /// Sites the browser connected in this run.
    var sites = ConnectedSites()
    /// Multisigs made here, the set-up card's choices, and a payment out of one waiting for Confirm.
    var multisigs: [Multisig] = Store.load([Multisig].self, "multisigs.json") ?? []
    var multisigDraft: MultisigDraft?
    var multisigPayment: MultisigPayment?
    var awaitingAmount: WalletCommand?
    /// A move the loss cap stopped (CostlyMove): offered again with that loss accepted («Send anyway»), never by itself.
    var costly: (pending: Pending, title: String, lost: Decimal)?
    /// The trade plan the chat is building: a perp on Hyperliquid.
    var intent = TradeIntent()
    /// Price alerts, checked against Hyperliquid's marks at every refresh while the app is open.
    var alerts: [PriceAlert] = Store.load([PriceAlert].self, "alerts.json") ?? [] {
        didSet { Store.save(alerts, "alerts.json") }
    }
    /// Scheduled tasks and recurring buys, run by the minute.
    var tasks: [ScheduledTask] = Store.load([ScheduledTask].self, "tasks.json") ?? [] {
        didSet { Store.save(tasks, "tasks.json") }
    }

    init(out: Outbox) { self.out = out }

    // MARK: State to the window

    private var pushQueued = false

    /// Something the window draws changed: one state goes out after the current step, however many changes it made.
    func changed() {
        guard !pushQueued else { return }
        pushQueued = true
        Task { @MainActor in
            await Task.yield()
            pushQueued = false
            out.event("state", snapshot())
        }
    }

    var activeWallet: SignerWallet? { status?.wallets?.first { $0.id == status?.activeWallet } ?? status?.wallets?.first }
    var solanaAddress: String? { status?.solanaAddress }
    var evmAddress: String? { status?.evmAddress }
    var nearAddress: String? { status?.nearAddress }
    var tradeWallet: TradeWallet? {
        guard let s = solanaAddress, let e = evmAddress else { return nil }
        return TradeWallet(solana: s, evm: e)
    }

    var russian: Bool {
        switch settings.language {
        case "ru": true
        case "en", "es": false
        default: ChatLanguage.auto.isRussian(recentUserMessages: messages.filter(\.fromUser).suffix(4).map(\.text))
        }
    }

    func say(_ english: String, _ russian: String) -> String { self.russian ? russian : english }

    var replyLanguage: String {
        switch settings.language {
        case "ru": "Russian"
        case "en": "English"
        case "es": "Spanish"
        default: ChatLanguage.auto.name(recentUserMessages: messages.filter(\.fromUser).suffix(4).map(\.text))
        }
    }

    func reply(_ text: String, note: String? = nil) {
        messages.append(Message(fromUser: false, text: text, note: note))
        saveChat()
        changed()
    }

    func saveChat() { saveCanvases() }

    nonisolated struct Snapshot: Encodable {
        struct Signer: Encodable {
            var ready: Bool
            var error: String?
            var notes: [String]
            var waiting: Bool
            var sessionLength: Int?
        }
        struct Option: Encodable { var title: String; var comment: String; var recommended: Bool }
        struct Alert: Encodable { var id: String; var text: String }
        struct SkillView: Encodable { var id: String; var name: String; var about: String; var text: String; var builtIn: Bool; var on: Bool }
        struct Running: Encodable { var title: String; var lines: [String] }
        struct Position: Encodable {
            var coin: String
            var side: String
            var size: String
            var entry: String
            var pnl: String
            var liquidation: String?
        }
        var signer: Signer
        var wallets: [SignerWallet]
        var active: String?
        var addresses: [String: String]
        var phrase: [String]?
        var portfolio: PortfolioView
        var positions: [Position]
        var chat: [Message]
        var thinking: Bool
        var draft: String?
        var card: Card?
        var options: [Option]
        var running: Running?
        var plan: String?
        var model: ModelView
        var providers: [ProviderView]
        var secondCheck: Bool
        var jevOpenRouter: Bool
        var language: String
        var russian: Bool
        var book: [SignerAddress]
        var history: [HistoryLine]
        var alerts: [Alert]
        struct TaskView: Encodable { var id: String; var text: String; var paused: Bool; var automatic: Bool }
        var tasks: [TaskView]
        var automation: SignerWallet?
        var skills: [SkillView]
        var memory: [String]
        var favorites: [FavoriteToken]
        struct CanvasRow: Encodable { var id: String; var title: String; var at: Date }
        var canvases: [CanvasRow]
        var canvas: String
        var boards: [Board]
        var runningBoard: String?
        var multisigs: [Multisig]
        var multisigDraft: MultisigDraft?
        var candidates: [Multisig.Key]
    }

    func snapshot() -> Snapshot {
        var addresses: [String: String] = [:]
        if let s = solanaAddress { addresses["solana"] = s }
        if let e = evmAddress { addresses["evm"] = e; addresses["hyperliquid"] = e }
        if let n = nearAddress { addresses["near"] = n }
        return Snapshot(
            signer: .init(ready: status != nil, error: signerError, notes: status?.checks ?? [], waiting: signing > 0, sessionLength: status?.sessionLength),
            wallets: status?.wallets ?? [], active: status?.activeWallet, addresses: addresses, phrase: phrase,
            portfolio: portfolio,
            positions: positions.map { p in
                .init(coin: IntentParser.pretty(p.coin), side: p.size > 0 ? "long" : "short", size: Fmt.num(abs(p.size), maxFraction: 6),
                      entry: Fmt.price(p.entry), pnl: Fmt.usd(p.unrealizedPnl), liquidation: p.liquidation.map(Fmt.price))
            },
            chat: messages, thinking: thinking, draft: draft, card: card,
            options: options.map { .init(title: $0.title, comment: $0.comment, recommended: $0.recommended) },
            running: running.map { .init(title: $0.title, lines: $0.lines) },
            plan: intent == TradeIntent() ? nil : intent.command,
            model: modelView, providers: providerViews, secondCheck: secondCheck, jevOpenRouter: settings.jevOpenRouter == true,
            language: settings.language ?? "auto", russian: russian, book: book, history: history(),
            alerts: alerts.map { .init(id: $0.id.uuidString, text: alertText($0)) },
            tasks: tasks.sorted { $0.next < $1.next }.map { .init(id: $0.id.uuidString, text: taskText($0), paused: $0.paused == true, automatic: usesAutomation($0) && $0.movesMoney) },
            automation: automationWallet,
            skills: (Skill.builtIn + addedSkills).map { .init(id: $0.id, name: $0.name, about: $0.about, text: $0.text, builtIn: $0.builtIn, on: !skillsOff.contains($0.id)) },
            memory: memory, favorites: favorites,
            canvases: canvases.map { .init(id: $0.id, title: $0.title, at: $0.messages.last?.at ?? $0.at) }, canvas: canvasID, boards: boards,
            runningBoard: runningBoard, multisigs: multisigs, multisigDraft: multisigDraft, candidates: multisigCandidates)
    }

    // MARK: Requests from the window

    nonisolated struct Request: Decodable {
        var id: Int?
        var op: String
        var text: String?
        var index: Int?
        var test: Bool?
        var phrase: String?
        var key: String?
        var provider: String?
        var model: String?
        var wallet: String?
        var label: String?
        var name: String?
        var network: String?
        var address: String?
        var memo: String?
        var language: String?
        var seconds: Int?
        var board: String?
        var threshold: Int?
    }

    func handle(_ r: Request) async {
        do {
            let result: [String: String] = try await run(r)
            out.answer(r.id, ok: true, result: result)
        } catch {
            out.answer(r.id, ok: false, error: error.localizedDescription)
        }
        changed()
    }

    private func run(_ r: Request) async throws -> [String: String] {
        switch r.op {
        case "hello":
            await startSigner()
            if !refreshingEveryMinute {
                // The balances follow the chain while the window is open, as on the Mac (there every 30 s).
                refreshingEveryMinute = true
                Task {
                    while true {
                        await refresh()
                        await runDueTasks()
                        try? await Task.sleep(for: .seconds(60))
                    }
                }
            }
            return [:]
        case "send": await send(r.text ?? "", about: r.board)
        case "confirm": await confirm()
        case "cancel": cancelPending()
        case "pick":
            guard let i = r.index, options.indices.contains(i) else { return [:] }
            let o = options[i]
            options = []
            if o.command == Self.sendAnyway { await sendAnyway(); return [:] }
            if o.command == Self.leaveCostly { costly = nil; reply(say("Left as it was: nothing was signed.", "Оставил как есть: ничего не подписано.")); return [:] }
            await send(o.command.isEmpty ? o.title : o.command, shown: o.title)
        case "refresh": await refresh()
        case "createWallet": try await createWallet(test: r.test == true)
        case "phraseSaved": phrase = nil
        case "importPhrase": try await importPhrase(r.phrase ?? "")
        case "importKey": try await importKey(r.key ?? "")
        case "switchWallet": try await signerCall(.setActiveWallet(id: r.wallet ?? ""))
        case "renameWallet": try await signerCall(.renameWallet(id: r.wallet ?? "", label: r.label ?? ""))
        case "removeWallet": try await signerCall(.removeWallet(id: r.wallet ?? ""))
        case "lock": try await signerCall(.lock)
        case "sessionLength": try await signerCall(.setSessionLength(seconds: r.seconds ?? SignerSession.standard))
        case "saveAddress":
            try await saveAddress(SignerAddress(name: r.name ?? "", network: r.network ?? "", address: r.address ?? "", memo: r.memo ?? ""))
        case "removeAddress": try await removeAddress(name: r.name ?? "", network: r.network ?? "")
        case "removeAlert": alerts.removeAll { $0.id.uuidString == r.wallet }
        case "removeTask": tasks.removeAll { $0.id.uuidString == r.wallet }
        case "taskOn": setTask(r.wallet ?? "", on: r.test == true)
        case "createAutomationWallet": await createAutomationWallet()
        case "addSkill": try addSkill(r.text ?? "", fileName: r.name ?? "skill.md")
        case "removeSkill": addedSkills.removeAll { $0.id == r.wallet }; skillsOff.remove(r.wallet ?? "")
        case "skillOn":
            if r.test == true { skillsOff.remove(r.wallet ?? "") } else { skillsOff.insert(r.wallet ?? "") }
        case "forget": memory.removeAll { $0 == r.text }
        case "removeFavorite": favorites.removeAll { $0.id == r.wallet }
        case "connectModel": try await connect(provider: r.provider ?? "", key: r.key, model: r.model)
        case "signIn": try await signIn(provider: r.provider ?? "")
        case "disconnectModel": disconnect(provider: r.provider ?? "")
        case "setModel": settings.model = r.model
        case "language": settings.language = r.language == "auto" ? nil : r.language
        case "secondCheck": settings.secondCheck = r.test ?? true
        case "jevOpenRouter": settings.jevOpenRouter = r.test ?? false
        case "dapp":
            // From the window's browser, which read the origin from the webview itself.
            return ["json": try await dapp(r.name ?? "", params: r.text ?? "{}", origin: r.address ?? "")]
        case "newCanvas": try newCanvas()
        case "openCanvas": try openCanvas(r.wallet ?? "")
        case "renameCanvas": renameCanvas(r.wallet ?? "", r.label ?? "")
        case "removeCanvas": try removeCanvas(r.wallet ?? "")
        case "addBoard":
            guard let kind = r.name, Self.liveBoards.contains(kind) else { throw WalletError("Unknown block «\(r.name ?? "")».") }
            pin(Board(kind: kind, title: kind))
        case "closeBoard":
            guard r.board != card?.id, r.board != runningBoard else { throw WalletError(say("Confirm or cancel the move first.", "Сначала подтверди или отмени перевод.")) }
            boards.removeAll { $0.id == r.board }
        case "multisigKey": try multisigKey(add: r.address, remove: r.wallet)
        case "multisigThreshold": updateMultisigDraft { $0.threshold = r.threshold ?? $0.threshold }
        case "multisigNetwork": updateMultisigDraft { d in
            guard let n = r.network, Self.multisigNetworks.contains(n) else { return }
            if d.networks.contains(n) { if d.networks.count > 1 { d.networks.removeAll { $0 == n } } } else { d.networks.append(n) }
        }
        case "clearChat":
            messages = []
            loadedSkills = []
            cancelPending()
            intent = TradeIntent()
            saveChat()
        case "signerPid":
            // The window lets the signer come forward when Windows Hello asks; only the window may hand that on.
            return ["pid": (await SignerLink.shared.pid).map(String.init) ?? ""]
        default: throw WalletError("Unknown request «\(r.op)».")
        }
        return [:]
    }
}

/// The engine's output: one JSON line per answer or event on standard output, which the window reads.
nonisolated final class Outbox: @unchecked Sendable {
    private let lock = NSLock()

    private func write(_ object: some Encodable) {
        guard var data = try? JSONEncoder.lines.encode(object) else { return }
        data.append(0x0A)
        lock.withLock { FileHandle.standardOutput.write(data) }
    }

    func event<T: Encodable>(_ name: String, _ body: T) {
        write(OutEvent(event: name, data: body))
    }

    func answer(_ id: Int?, ok: Bool, result: [String: String] = [:], error: String? = nil) {
        struct Answer: Encodable { let id: Int?; let ok: Bool; let result: [String: String]; let error: String? }
        write(Answer(id: id, ok: ok, result: result, error: error))
    }
}

nonisolated struct OutEvent<T: Encodable>: Encodable {
    let event: String
    let data: T
}

nonisolated extension JSONEncoder {
    static let lines: JSONEncoder = {
        let e = JSONEncoder()
        e.dateEncodingStrategy = .iso8601
        return e
    }()
}
