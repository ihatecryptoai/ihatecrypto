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
    /// The form the chat's model read a request as (the History's «understood as»).
    var understood: String?
    /// The cards this reply offered and the one picked, a move's steps kept as one card, the blocks a message went with
    /// (ChatTurns.swift).
    var options: [OfferView]?
    var picked: Int?
    var steps: [String]?
    var contexts: [SentContext]?
}

/// A market's closes over a range, for the window to draw as a line.
nonisolated struct ChartLine: Codable, Equatable {
    var title: String
    var closes: [Double]
    var low: String
    var high: String
    var change: String
    /// Hyperliquid's name of the coin («ETH», «xyz:TSLA»): a trade plan of it is drawn on this chart (RouteEdit.swift).
    var coin: String? = nil
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
    /// «stop 2%» after a trade: that position's stop and take moved on the exchange, the traded plan kept (TradePlan.swift).
    case orders([PositionCommand], coin: String, intent: TradeIntent)
}

enum ChainStep {
    case wallet(WalletCommand)
    case transfer(SignerAddress, Decimal)
    /// A step run by its words once the moves before it are through: a chart, a position, a move asked «How much?» then (Chains.swift).
    case said(String)

    var title: String {
        switch self {
        case .wallet(let c): c.summary
        case .transfer(let e, let amount): "send \(Fmt.num(amount)) USDC to \(e.name)"
        case .said(let text): text
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
        didSet { Store.save(settings, "settings.json"); settings.mirror(); changed() }
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
    /// Hyperliquid's open orders: the positions' stops and takes, a limit waiting (PositionGuard.swift).
    var hlOrders: [HyperliquidAccount.Order] = []
    /// A place's details beyond the portfolio's rows (PlaceFacts.swift).
    var placeFacts = PlaceFacts()
    var ratesAt: Date?
    var guarding = false
    var ledgerGuardSaid = false
    /// What the automation wallet holds, in dollars, read with the balances; nil without it.
    var automationUSD: Decimal?
    /// What each canvas's chat filled of its model's window with its last message (the Mac's ContextMeter), this run.
    var meters: [String: Snapshot.Context] = [:]
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
    var starred: Set<String> = Set(Store.load([String].self, "starred.json") ?? []) { didSet { Store.save(starred.sorted(), "starred.json"); changed() } }

    // Canvases: each has its own chat and blocks.
    lazy var canvases: [CanvasDoc] = Self.loadCanvases()
    /// The canvas list's folders and their pictures (Folders.swift).
    var folderStore = Store.load(FolderStore.self, "folders.json") ?? FolderStore() { didSet { Store.save(folderStore, "folders.json"); changed() } }
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
    /// Hyperliquid's trades closed in the last 7 days (Blocks.swift).
    var recentlyClosed: [ClosedTrade] = []
    /// A Solana swap's output as its card showed it, in the token's units (Quotes.swift).
    var jupiterShown: (command: WalletCommand, out: UInt64)? { didSet { if jupiterShown != nil { quotedAt = Date() } } }
    var options: [ChatRouter.Option] = [] { didSet { pinOptions() } }
    /// The chat's turn (ChatTurns.swift): the model call Stop can cancel, the lookup under way, the reply the live cards
    /// belong to, the failed move «Try again» lays out, when the waiting swap was priced, the other blocks a message goes with.
    var modelRunning = false
    var stopRequested = false
    var cancelModel: (() -> Void)?
    var agentStep: String?
    var optionsMessage: String?
    var retry: (board: String, command: WalletCommand)?
    var quotedAt: Date?
    var alsoAttached: [Board] = []
    /// The model menu's lists: a subscription's and the local servers' models, whether those answer, sizes, downloads.
    var liveModels: [String: [String]] = Store.load([String: [String]].self, "live-models.json") ?? [:] {
        didSet { Store.save(liveModels, "live-models.json") }
    }
    var localServers: [String: Bool] = [:]
    /// Settings › AI model's model check and installers, while they run (LocalModels.swift).
    var localSetup = LocalSetup()
    var modelSizes: [String: Double] = [:]
    var modelDownloads: [String: Double] = [:]
    var modelFacts: [String: ModelFacts] = [:]
    var modelFactsAsked = Date.distantPast
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
    /// A request the model carries on with after its step, and whether the last check found nothing to tell.
    var agentGoal: AgentGoal?
    var quietCheck = false
    /// How the last move ended («done», «failed»), for the request it was a step of.
    var lastMove: (at: Date, state: String)?
    /// A contract call checked and simulated, waiting for Confirm.
    var callReview: CallReview?
    /// The Ledger's addresses, its connect card, and what it waits for on its screen now.
    var ledger: LedgerAccount? = Store.load(LedgerAccount.self, "ledger.json") {
        didSet { Store.save(ledger, "ledger.json"); changed() }
    }
    var ledgerConnect: LedgerConnect?
    var ledgerPrompt: String?
    var awaitingAmount: WalletCommand?
    /// A chain's «How much?» and the steps it holds (Chains.swift).
    var chainState = ChainState()
    /// A recurring buy whose two ways were offered: picked again, it goes straight (MoveChecks.swift).
    var dcaChoice: WalletCommand?
    /// A new Hyperliquid account's way in, as picked on its cards: NEAR Intents (true) or Relay (false); nil not asked yet.
    var openVia1Click: Bool?
    /// A card left waiting on a canvas the user switched away from: it waits there a quarter of an hour.
    var parked: [String: (pending: Pending, card: Card, intent: TradeIntent, at: Date)] = [:]
    /// The way picked in a route step's details for the waiting trade (RouteEdit), taken at its Confirm.
    var routeWay: (key: String, way: String)?
    /// Both ways of the waiting EVM trade as quoted for its step's details (RouteEdit.quoteWays), the better first.
    var wayQuotes: (command: WalletCommand, ways: [RouteEdit.Way])? { didSet { if wayQuotes?.ways.isEmpty == false { quotedAt = Date() } } }
    /// The Ledger's Hyperliquid trading key, and whether the account approved ihatecrypto's fee (TradingKey.swift).
    var ledgerAgent: SignerReply?
    var feeApproved: Bool?
    /// A move the loss cap stopped (CostlyMove): offered again with that loss accepted («Send anyway»), never by itself.
    var costly: (pending: Pending, title: String, lost: Decimal)?
    /// Bridge orders on their way, read with the balances (HiddenTokens.swift).
    var inTransit: [TransitView] = []
    /// The trade plan the chat is building: a perp on Hyperliquid.
    var intent = TradeIntent()
    /// The plan's board on the canvas, stopped, previewed or the card (TradePlan.swift); the plan of the trade last opened,
    /// which «stop 2%» edits while its position is open.
    var planBoard: String?
    var tradedPlan: TradeIntent?
    /// Price alerts, checked against Hyperliquid's marks at every refresh while the app is open.
    var alerts: [PriceAlert] = Store.load([PriceAlert].self, "alerts.json") ?? [] {
        didSet { Store.save(alerts, "alerts.json") }
    }
    /// Scheduled tasks and recurring buys, run by the minute.
    var tasks: [ScheduledTask] = TaskSeal.load().tasks {
        didSet { TaskSeal.save(tasks) }
    }
    /// What each scheduled run did, for the Schedule's History tab.
    var scheduleRuns: [ScheduleRun] = Store.load([ScheduleRun].self, "schedule-runs.json") ?? [] {
        didSet { Store.save(scheduleRuns, "schedule-runs.json") }
    }
    /// Each multisig's money by network (address → networks), read with the balances.
    var multisigHeld: [String: [MultisigNetHolding]] = [:]
    /// The automation wallet's money as last read, for the Schedule's budget.
    var automationBudget: AutomationBudget?
    /// The app's lock (Lock.swift), and whether the signer has said if a code is set since launch.
    var lock = LockView()
    var lockHeardSigner = false

    init(out: Outbox) { self.out = out; settings.mirror(); Self.applyNodes(); lockAtLaunch() }

    // MARK: State to the window

    private var pushQueued = false

    /// Something the window draws changed: one state goes out after the current step, however many changes it made.
    func changed() {
        guard !pushQueued else { return }
        pushQueued = true
        Task { @MainActor in
            await Task.yield()
            pushQueued = false
            // Locked: the window gets the lock alone, never the balances, the chat or the history.
            if lock.locked { out.event("state", lockedSnapshot) } else { out.event("state", snapshot()) }
        }
    }

    /// The wallet that signs now: the connected Ledger when it's on, else the signer's.
    var activeWallet: SignerWallet? {
        if ledgerActive, let l = ledger { return l.wallet }
        return status?.wallets?.first { $0.id == status?.activeWallet } ?? status?.wallets?.first
    }
    /// No wallet here, only addresses watched for their balances: they're read, and nothing signs for them.
    var watching: Bool { !ledgerActive && status?.hasWallet != true && (settings.watchSolana != nil || settings.watchEVM != nil) }
    var solanaAddress: String? { ledgerActive ? ledger?.solana : watching ? settings.watchSolana : status?.solanaAddress }
    var evmAddress: String? { ledgerActive ? ledger?.evm : watching ? settings.watchEVM : status?.evmAddress }
    var nearAddress: String? { ledgerActive ? nil : status?.nearAddress }
    var tradeWallet: TradeWallet? {
        guard let s = solanaAddress, let e = evmAddress else { return nil }
        return TradeWallet(solana: s, evm: e)
    }
    /// The addresses balances are read for: a chain without one ("") isn't read.
    var readWallet: TradeWallet? {
        solanaAddress == nil && evmAddress == nil ? nil : TradeWallet(solana: solanaAddress ?? "", evm: evmAddress ?? "")
    }
    var watchOnly: String {
        say("These addresses are only watched: moving money or signing needs a wallet here (Settings › Wallets).",
            "Эти адреса только для просмотра: чтобы переводить или подписывать, добавь кошелёк (Настройки › Кошельки).")
    }

    /// «Addresses for balances»: each checked as an address of its chain; an empty one stops watching it.
    func watch(solana: String?, evm: String?) throws {
        let s = solana?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "", e = evm?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !s.isEmpty, let problem = AddressCheck.problem(s, network: .solana) { throw WalletError(problem) }
        if !e.isEmpty, let problem = AddressCheck.problem(e, network: .base) { throw WalletError(problem) }
        settings.watchSolana = s.isEmpty ? nil : s
        settings.watchEVM = e.isEmpty ? nil : e
        portfolio = PortfolioView()
        positions = []
        loadEquity()
        Task { await refresh() }
    }

    var russian: Bool {
        switch settings.language {
        case "ru": true
        case "en", "es": false
        default: ChatLanguage.auto.isRussian(recentUserMessages: messages.filter(\.fromUser).suffix(4).map(\.text))
        }
    }

    func say(_ english: String, _ russian: String) -> String { self.russian ? russian : english }

    /// The window's own texts (Settings › General › App language), apart from the replies' language: System follows
    /// Windows' language, as the Mac's follows the Mac's.
    var appRussian: Bool {
        switch settings.appLanguage {
        case "ru": true
        case "en": false
        default: (Locale.preferredLanguages.first ?? "").lowercased().hasPrefix("ru")
        }
    }

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
            /// What the Ledger waits for on its screen.
            var ledger: String?
        }
        struct Option: Encodable { var title: String; var comment: String; var recommended: Bool }
        struct Alert: Encodable {
            var id: String; var text: String
            /// The block's parts (AlertsBoard): the coin, «BTC above $120,000», «now $98,000 · 22.4% away», its THEN steps.
            var coin: String? = nil; var when: String? = nil; var now: String? = nil; var steps: [String]? = nil
            /// Each step as its block draws it: a trade plan («THEN · PLAN ON HYPERLIQUID», Long ETH ×3) or a command.
            var blocks: [AlertStep]? = nil
        }
        struct AlertStep: Encodable {
            var plan: Bool; var coin: String?; var title: String; var line: String?
        }
        struct SkillView: Encodable { var id: String; var name: String; var about: String; var text: String; var builtIn: Bool; var on: Bool
            /// The Trading study's journal is kept (Skills.swift): its «Journal» button opens it.
            var journal: Bool? = nil }
        struct Running: Encodable { var title: String; var lines: [String] }
        struct Position: Encodable {
            var coin: String
            var side: String
            var size: String
            var entry: String
            var pnl: String
            var liquidation: String?
            /// «0.05 at $2,412.50 · stop $2,300 · take $2,600» and the guardian's line (PositionGuard.swift).
            var summary: String? = nil
            var guardian: String? = nil
            /// The position board's price ruler and guardian line (TradePlan.swift).
            var ruler: PositionRuler? = nil
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
        struct TaskView: Encodable {
            var id: String; var text: String; var paused: Bool; var automatic: Bool; var next: Date?
            /// Its steps one by one, and how often («3 days»; nil: once), for its own canvas's block.
            var steps: [String] = []; var every: String? = nil
            /// It can pay from the wallet that's on or the automation wallet («Pays from» on its block).
            var pick = false
        }
        /// Tokens the model read with the canvas's last message, and its window when the service or the catalog says.
        /// Of `used`: the instructions and the chat's lines; how many lines went of how many; dollars an API key paid.
        struct Context: Encodable, Equatable {
            var used: Int; var window: Int?
            var instructions = 0, chat = 0, sent = 0, lines = 0
            var spent = 0.0, last: Double? = nil, billed = false
        }
        var tasks: [TaskView]
        var automation: SignerWallet?
        var skills: [SkillView]
        var memory: [String]
        var favorites: [FavoriteToken]
        struct CanvasRow: Encodable {
            var id: String; var title: String; var at: Date
            var pinnedAt: Date?; var folder: String?; var icon: CanvasIcon?; var provider: String?; var model: String?
            /// Nothing on it yet: the list shows it as the new canvas, kept once written on. `blocks`: something on its canvas.
            var empty = false
            var blocks = false
        }
        var canvases: [CanvasRow]
        var folders: [FolderView]
        var canvas: String
        var boards: [Board]
        var runningBoard: String?
        var multisigs: [Multisig]
        var multisigDraft: MultisigDraft?
        var candidates: [Multisig.Key]
        var ledgerConnect: LedgerConnect?
        var starred: [String]
        var watching: Bool
        var watched: [String: String]
        var automationUSD: Decimal?
        /// The open positions' unrealized P/L together; nil with none open.
        var positionsPnl: Decimal?
        /// Bridge orders signed and not landed yet (HiddenTokens.swift).
        var inTransit: [TransitView]
        var context: Context?
        var modelRunning: Bool
        var agentStep: String?
        var optionsMessage: String?
        var retryBoard: String?
        var quote: QuoteView?
        var models: ModelsView
        /// The app's lock (Lock.swift): whether a code is set, how soon it locks, a new code to offer.
        var lock: LockView
        /// Settings › Modules and Settings › Trading agent (Modules.swift, TradingAgent.swift).
        var modules: ModulesView
        var trading: TradingView
        var nodes: NodesView
        /// What the waiting move's step lets change (RouteEdit.Edit); nil when nothing can.
        var stepEdit: RouteEdit.Edit?
        var schedulePaused: Bool
        var scheduleRuns: [ScheduleRun]
        var multisigHeld: [String: [MultisigNetHolding]]
        /// The open canvas's scheduled item («task:<id>», «perp:<coin>»), when it's that item's own canvas.
        var scheduleKey: String?
        /// The History block's moves, newest first, and what the latest day did to the money (HistoryMoves.swift).
        var moves: [MoveRow]
        var movesDay: MovesDay?
        var automationBudget: AutomationBudget?
        /// Dollars without the fee coin (WalletOps.swift), for the onboarding's Top up.
        var stuck: [StuckView]
        /// A place's details board beyond the rows: Hyperliquid in full, the deposits' rates (PlaceFacts.swift).
        var places: PlaceFacts
        /// Settings › General › Notifications: each kind and whether it's on (Notices.swift).
        var notices: [NoticeView]
        /// Settings › Background and the onboarding's «Stay in touch»: ihatecrypto starts in the tray at login (Notices.swift).
        var openAtLogin: Bool
        /// Top up's balance, gas and the money it watches for (TopUp.swift); Settings › General › App language.
        var topUp: TopUpView?
        var appLanguage: String
        var localSetup: LocalSetupView
        /// Hyperliquid's trades closed in the last 7 days, newest first (Blocks.swift), and the address book's pictures,
        /// categories and notes by «network:name».
        var recentlyClosed: [ClosedTrade]
        var addressExtras: [String: AddressExtra]
    }

    func snapshot() -> Snapshot {
        var addresses: [String: String] = [:]
        if let s = solanaAddress { addresses["solana"] = s }
        if let e = evmAddress { addresses["evm"] = e; addresses["hyperliquid"] = e }
        if let n = nearAddress { addresses["near"] = n }
        return Snapshot(
            signer: .init(ready: status != nil, error: signerError, notes: status?.checks ?? [], waiting: signing > 0 || ledgerPrompt != nil,
                          sessionLength: status?.sessionLength, ledger: ledgerPrompt),
            wallets: (status?.wallets ?? []) + (ledger?.evm != nil ? [ledger!.wallet] : []), active: ledgerActive ? LedgerAccount.id : status?.activeWallet,
            addresses: addresses, phrase: phrase,
            portfolio: portfolio,
            positions: positionViews(),
            chat: messages, thinking: thinking, draft: draft, card: card,
            options: options.map { .init(title: $0.title, comment: $0.comment, recommended: $0.recommended) },
            running: running.map { .init(title: $0.title, lines: $0.lines) },
            plan: intent == TradeIntent() ? nil : intent.command,
            model: modelView, providers: providerViews, secondCheck: secondCheck, jevOpenRouter: settings.jevOpenRouter == true,
            language: settings.language ?? "auto", russian: appRussian, book: book, history: history(),
            alerts: alerts.filter { $0.id != planAlert?.id }.map(alertView),
            tasks: tasks.sorted { $0.next < $1.next }.map { .init(id: $0.id.uuidString, text: taskText($0), paused: $0.paused == true, automatic: usesAutomation($0) && $0.movesMoney, next: $0.next,
                                                                steps: $0.steps, every: $0.every?.words, pick: canPickPays($0)) },
            automation: automationWallet,
            skills: (Skill.builtIn + addedSkills).map { .init(id: $0.id, name: $0.name, about: $0.about, text: $0.text, builtIn: $0.builtIn, on: !skillsOff.contains($0.id),
                                                              journal: $0.id == Skill.tradingStudy.id ? Self.journalKept : nil) },
            memory: memory, favorites: favorites,
            canvases: canvasRows, folders: folderViews, canvas: canvasID, boards: boardsShown,
            runningBoard: runningBoard, multisigs: multisigs, multisigDraft: multisigDraft, candidates: multisigCandidates, ledgerConnect: ledgerConnect, starred: starred.sorted(),
            watching: watching, watched: ["solana": settings.watchSolana, "evm": settings.watchEVM].compactMapValues { $0 },
            automationUSD: automationWallet == nil ? nil : automationUSD,
            positionsPnl: positions.isEmpty ? nil : positions.reduce(0) { $0 + $1.unrealizedPnl },
            inTransit: inTransit,
            context: meters[canvasID],
            modelRunning: modelRunning, agentStep: thinking ? agentStep : nil, optionsMessage: optionsMessage, retryBoard: retry?.board,
            quote: quoteView, models: modelsView,
            lock: lockView,
            modules: modulesView(), trading: tradingView(), nodes: nodesView(),
            stepEdit: stepEdit,
            schedulePaused: settings.schedulePaused == true, scheduleRuns: recentRuns, multisigHeld: multisigHeld,
            scheduleKey: scheduleKey, moves: historyMoves, movesDay: movesDay,
            automationBudget: automationBudgetNow,
            stuck: stuckView, places: placeFacts, notices: noticeViews, openAtLogin: settings.openAtLogin == true,
            topUp: TopUpPage.shared.view, appLanguage: settings.appLanguage ?? "system", localSetup: localSetupView,
            recentlyClosed: recentlyClosed, addressExtras: addressExtras)
    }

    // MARK: Requests from the window

    nonisolated struct Request: Decodable {
        var id: Int?
        var op: String
        var text: String?
        var index: Int?
        var test: Bool?
        var phrase: String?
        /// A recovery phrase's BIP-39 passphrase, if its wallet has one.
        var passphrase: String?
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
        var solana: String?
        var evm: String?
        /// Every block the message is about, the newest last (the chat's chips).
        var boards: [String]?
        /// Settings › Trading agent's rules, and a number (the contract cap in dollars).
        var policy: RiskPolicy?
        var value: Double?
    }

    func handle(_ r: Request) async {
        do {
            let result: [String: String] = try await run(r)
            out.answer(r.id, ok: true, result: result)
        } catch {
            out.answer(r.id, ok: false, error: error.localizedDescription)
        }
        // A question that only reads changes nothing: a new state after it would make the browser's toolbar ask again
        // (each state redraws it, and a redraw asks siteState), six times a second, dropping what's being typed.
        if !Self.answersOnly.contains(r.op) { changed() }
    }

    nonisolated static let answersOnly: Set<String> = ["siteState", "historyTabs", "transitState", "blockCatalog", "hlFunded", "checkAddress", "planMarkets", "windowFocus", "openStudyFiles"]

    private func run(_ r: Request) async throws -> [String: String] {
        if lock.locked, !Self.whileLocked.contains(r.op) { throw WalletError(say("ihatecrypto is locked.", "ihatecrypto заблокирован.")) }
        switch r.op {
        case "unlock": await unlock(code: r.text ?? "")
        case "unlockHello": await unlockWithHello()
        case "lockApp": await lockApp()
        case "setCode": try await makeCode(r.text ?? "")
        case "clearCode": try await turnCodeOff()
        case "lockAfter": setLockAfter((r.seconds ?? 300) / 60)
        case "hello":
            await startSigner()
            if !refreshingEveryMinute {
                // The tasks were changed outside ihatecrypto and loaded paused: said once.
                if TaskSeal.load().tampered && tasks.allSatisfy({ $0.paused == true }) {
                    reply(say("The scheduled tasks were changed outside ihatecrypto, so every one is paused. Check them in the Schedule and switch on only the ones you made.",
                              "Задачи по расписанию изменили вне ihatecrypto, поэтому все они на паузе. Проверь их в «Расписании» и включи только свои."))
                }
                offerNewModules()
                watchAlerts()
                // The balances follow the chain while the window is open, as on the Mac (there every 30 s).
                refreshingEveryMinute = true
                Task {
                    while true {
                        await refresh()
                        offerTradingKey()
                        await refreshLive()
                        await readClosed()
                        await runDueTasks()
                        try? await Task.sleep(for: .seconds(60))
                    }
                }
            }
            return [:]
        case "send":
            alsoAttached = (r.boards ?? []).filter { $0 != r.board }.compactMap { id in boards.first { $0.id == id } }
            // The model and the parsers read «SOL»: the «$» only made the chip; the chat shows it as typed (the Mac's send).
            let typed = r.text ?? "", plain = TokenMentions.plain(typed)
            await send(plain, shown: plain == typed ? nil : typed, about: r.board)
        case "stopModel": stopModel()
        case "retry": await retryMove()
        case "requote": requote()
        case "refreshModels": await refreshModels()
        case "downloadModel": Task { await downloadModel(r.model ?? "") }
        case "startOllama": startOllama()
        case "getOllama": getOllama()
        case "lookUpContract": return await lookUpContract(r.address ?? "")
        case "blockCatalog": return blockCatalog()
        case "confirm": await confirm()
        case "cancel": cancelPending()
        case "pick":
            guard let i = r.index, options.indices.contains(i) else { return [:] }
            markPicked(i)
            let o = options[i]
            options = []
            if answerNewModules(o.command) { return [:] }
            if await answerTradingKey(o.command) { return [:] }
            if o.command == Self.sendAnyway { await sendAnyway(); return [:] }
            if o.command == Self.leaveCostly { costly = nil; reply(say("Left as it was: nothing was signed.", "Оставил как есть: ничего не подписано.")); return [:] }
            await send(o.command.isEmpty ? o.title : o.command, shown: o.title)
        case "refresh": await refresh()
        case "createWallet": try await createWallet(test: r.test == true)
        case "phraseSaved": phrase = nil
        case "importPhrase": return try await importPhrase(r.phrase ?? "", passphrase: r.passphrase ?? "")
        case "hlFunded": return await hlFunded()
        case "importKey": try await importKey(r.key ?? "")
        case "switchWallet":
            if r.wallet == LedgerAccount.id { useLedger(true) } else {
                useLedger(false)
                try await signerCall(.setActiveWallet(id: r.wallet ?? ""))
            }
        case "connectLedger": Task { await connectLedger() }
        case "showOnLedger": await showOnLedger(solana: r.test == true)
        case "renameWallet": try await signerCall(.renameWallet(id: r.wallet ?? "", label: r.label ?? ""))
        case "removeWallet":
            if r.wallet == LedgerAccount.id { forgetLedger() } else { try await signerCall(.removeWallet(id: r.wallet ?? "")) }
        case "lock": try await signerCall(.lock)
        case "sessionLength": try await signerCall(.setSessionLength(seconds: r.seconds ?? SignerSession.standard))
        case "saveAddress":
            try await saveAddress(SignerAddress(name: r.name ?? "", network: r.network ?? "", address: r.address ?? "", memo: r.memo ?? ""))
        case "removeAddress": try await removeAddress(name: r.name ?? "", network: r.network ?? "")
        case "removeAlert": alerts.removeAll { $0.id.uuidString == r.wallet }
        case "removeTask": tasks.removeAll { $0.id.uuidString == r.wallet }
        case "taskOn": setTask(r.wallet ?? "", on: r.test == true)
        case "windowFocus":
            if Self.windowFront != (r.test ?? true) { Self.windowFront = r.test ?? true; if !Self.windowFront { Self.windowLeft = Date() } }
            return [:]
        case "taskPays": setFromActive(r.wallet ?? "", r.test == true)
        case "createAutomationWallet": await createAutomationWallet()
        case "addSkill": try addSkill(r.text ?? "", fileName: r.name ?? "skill.md")
        case "removeSkill": addedSkills.removeAll { $0.id == r.wallet }; skillsOff.remove(r.wallet ?? "")
        case "openStudyFiles": try openStudyFiles(journal: r.test == true)
        case "skillOn":
            if r.test == true { skillsOff.remove(r.wallet ?? "") } else { skillsOff.insert(r.wallet ?? "") }
        case "forget": memory.removeAll { $0 == r.text }
        case "removeFavorite": favorites.removeAll { $0.id == r.wallet }
        case "showToken": showToken(r.address ?? "", shown: r.test == true)
        case "openToken": return await openToken(r.address ?? "", chain: r.network ?? "")
        case "tokenRange": if let id = r.board { await loadToken(id, range: ChartCommand.Range(rawValue: r.text ?? "") ?? .day) }
        case "openChart": return try await openChart(r.name ?? "", range: ChartCommand.Range(rawValue: r.text ?? "") ?? .day)
        case "refreshApprovals": if let id = r.board { await refreshApprovals(id) }
        case "checkAddress": return checkAddress(r.address ?? "", network: r.network ?? "", editing: r.label)
        case "editAddress":
            try await editAddress(from: (r.label ?? "", r.text ?? ""), to: SignerAddress(name: r.name ?? "", network: r.network ?? "", address: r.address ?? "", memo: r.memo ?? ""))
        case "addressExtra": try setAddressExtra(name: r.name ?? "", network: r.network ?? "", icon: r.key, category: r.label, note: r.text)
        case "addressIcon": try setAddressExtra(name: r.name ?? "", network: r.network ?? "", icon: r.text ?? "", category: nil, note: nil)
        case "transitState": return ["state": await transitState(chain: r.name ?? "", id: r.address ?? "")]
        case "lookToken": return await lookToken(r.address ?? "")
        case "addFavorite": await tokenCommand(.favorite(address: r.address ?? "", chain: r.network), note: nil)
        case "schedulePause": pauseSchedule(r.test == true)
        case "notice": setNotice(r.name ?? "", on: r.test == true)
        case "noticeTest": noticeTest()
        case "openAtLogin": setOpenAtLogin(r.test == true)
        case "thinking", "checkModel", "installClaudeCode", "claudeSignIn", "installOllama", "ollamaStatus", "setUpLocal", "keepRunning", "keepRunningState": return try await localOp(r)
        // main.rs, at launch and after each change: how Windows has it now.
        case "loginState": settings.openAtLogin = r.test == true
        case "updateReady": updateReady(r.text ?? "")
        case "crashed": crashed(at: r.name ?? "", summary: r.text ?? "", folder: r.address ?? "")
        case "openSchedule": try openSchedule(r.name ?? "")
        case "closeSchedule": try closeSchedule()
        case "runNow": await runNow(r.name ?? "")
        case "star":
            guard let e = book.first(where: { $0.name == r.name && $0.network == r.network }) else { return [:] }
            star(e, r.test == true)
        case "connectModel": try await connect(provider: r.provider ?? "", key: r.key, model: r.model)
        case "signIn": try await signIn(provider: r.provider ?? "")
        case "disconnectModel": disconnect(provider: r.provider ?? "")
        case "setModel": settings.model = r.model
        case "language": settings.language = r.language == "auto" ? nil : r.language
        case "secondCheck": settings.secondCheck = r.test ?? true
        case "jevOpenRouter": settings.jevOpenRouter = r.test ?? false
        case "module": try setModule(r.name, on: r.test == true)
        case "policy": try setPolicy(r.policy)
        case "safeSearch": settings.safeSearch = r.test == false ? false : nil
        case "batches": settings.batches = r.test == true ? true : nil
        case "contractCap": try setContractCap(r.value)
        case "tradingKey": Task { await turnOnTradingKey() }
        case "revokeFee": Task { await revokeFee() }
        case "readFee": await readFeeApproval()
        case "node": try await setNode(r.network, address: r.address)
        case "oneClickKey": try await setOneClickKey(r.key)
        case "watch": try watch(solana: r.solana, evm: r.evm)
        case "dapp":
            // From the window's browser, which read the origin from the webview itself.
            return ["json": try await dapp(r.name ?? "", params: r.text ?? "{}", origin: r.address ?? "")]
        case "siteState": return await siteState(r.address ?? "")
        case "newCanvas": try newCanvas()
        case "openCanvas": try openCanvas(r.wallet ?? "")
        case "renameCanvas": renameCanvas(r.wallet ?? "", r.label ?? "")
        case "removeCanvas": try removeCanvas(r.wallet ?? "")
        case "pinCanvas", "moveCanvas", "createFolder", "renameFolder", "deleteFolder", "newCanvasIn", "setIcon", "siteIcon": return try await folderOp(r)
        case "addBoard":
            guard let kind = r.name, Self.liveBoards.contains(kind) else { throw WalletError("Unknown block «\(r.name ?? "")».") }
            pin(Board(kind: kind, title: kind))
        case "editStep": try await editStep(board: r.board, amount: r.text, way: r.name)
        case "whyStep": try whyStep(board: r.board, index: r.index)
        case "planMarkets", "planMarket", "planTake", "planCancel": return await planOp(r)
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
        case "topUp": try topUp(network: r.network, coin: r.text, watch: r.test == true)
        case "topUpOpen": return ["url": try await topUpOpen(network: r.network, coin: r.text, amount: r.label)]
        case "topUpStop": topUpStop()
        case "historyTabs": return ["json": await historyTabs()]
        case "appLanguage": settings.appLanguage = r.language == "system" ? nil : r.language
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
