import AppKit
import SwiftUI
import DeFiJupiterLend
import DeFiKamino
import DeFiYields
import EVMChain
import ExecutionCore
import Foundation
import GuardJev
import IhatecryptoCore
import JupiterSwap
import LLMCatalog
import LLMCore
import Observation
import os
import Security
import SignerAPI
import TradeExecution
import TxGuard
import WalletCore

nonisolated struct Message: Identifiable, Codable, Equatable {
    var id = UUID()
    let fromUser: Bool
    let text: String
    var note: String?
    /// A message as the user typed it, «$SOL» and «$<mint>» kept: the chat draws them as chips. `text` is what's read.
    var shown: String?
    /// The cards to go on this reply offered, drawn under it in the chat (ChatReplies' Answers); `picked`, the one chosen.
    var options: [ChatRouter.Option]?
    var picked: Int?
    /// When it was said, for the reply's header; nil on messages saved before it was kept.
    var at: Date? = Date()
    /// What went to the model with the user's message (blocks, steps, tokens, addresses), drawn over it as context chips.
    var context: [StepContext]?

    /// The guard's notes on its reply to a message it stopped as steering (send()); that message never reaches a model.
    static let guardLocal = "guard: local rules", guardBoth = "guard: rules and the second check"
    static func stopsSteering(_ note: String?) -> Bool { note == guardLocal || note == guardBoth }
}

@Observable
final class Workspace {
    var intent = TradeIntent()
    var messages = [Message(fromUser: false, text: "Tell me what to do, e.g. “long ETH risk $10 stop 3% take 6% leverage 2x”. Blocks with live numbers appear on the right. Nothing is executed without your signature.")]
    var busy = false
    /// From a sent message until its first reply: the chat shows ihatecrypto reading it.
    var thinking = false {
        // A step's line belongs to the message it was said for: «Asking openweather… for its price», set after the
        // router had read the message, stayed under the next one («через 2 минуты купи SOL») until the router answered.
        didSet { if !thinking, agentStep != nil { agentStep = nil } }
    }
    /// The model's answer while it's still being written (ChatRouter's `draft`): shown in place of the dots, gone once the
    /// finished reply lands.
    var draftReply: String?
    /// When the draft was last shown: a model streams tens of pieces a second, and each shown one lays out the chat.
    @ObservationIgnored private var draftShownAt = Date.distantPast

    /// The model's answer so far, at most ten times a second; the finished reply replaces it whatever was skipped.
    func showDraft(_ words: String) {
        guard thinking, Date().timeIntervalSince(draftShownAt) >= 0.1 else { return }
        draftShownAt = Date()
        draftReply = words
    }

    var markets: [String: Market] = [:]
    /// The market picker's lists, names by group, most traded first. Set only when they change, so the picker
    /// doesn't rebuild its menus with every price.
    var marketNames: [Market.Group: [String]] = [:]
    var marketsError: String?
    var solana: Solana.Balance? { didSet { sortHoldings() } }
    var solanaError: String?
    /// The user's say on a token, by Holding.id: true shows it, false hides it; no entry follows Holding.looksReal.
    /// Not per wallet: a scam token hidden once stays hidden on every wallet.
    var tokenChoices: [String: Bool] = UserDefaults.standard.dictionary(forKey: "tokenChoices") as? [String: Bool] ?? [:] {
        didSet { UserDefaults.standard.set(tokenChoices, forKey: "tokenChoices"); sortHoldings() }
    }
    func tokenShown(_ t: Holding) -> Bool { tokenChoices[t.id] ?? t.looksReal }
    /// Tokens on the other networks (Holdings.swift): ERC-20s on Base, Arbitrum, Ethereum and Robinhood Chain, ETH on
    /// Arbitrum and Ethereum, Hyperliquid's spot side and NEAR tokens.
    var otherTokens: [Holding] = [] { didSet { sortHoldings() } }
    /// Aave and Morpho off Base, with money in them (Lending.swift).
    var lendingElsewhere: [LendingPosition] = []
    /// Deposits made on a site: Kamino's vaults (withdrawn by the kamino form), Jupiter's other platforms (Protocols.swift).
    var protocolPositions: [ProtocolPosition] = []
    @ObservationIgnored var protocolsReadAt: Date?
    /// A read of the other networks' tokens is under way (refresh starts one and doesn't wait for it).
    @ObservationIgnored var holdingsLoading = false
    /// The scheduled buys' and tasks' run, beside the watch loop: one at a time, and a Touch ID left unanswered holds only it.
    @ObservationIgnored var scheduledRun: Task<Void, Never>?
    /// Solana tokens a swap named, as Jupiter resolved them (TokensUI.solanaAsset): the card and the run read them again.
    @ObservationIgnored var spotAssets: [String: SpotAsset] = [:]
    /// Every token beyond the balances read one by one, Solana's first. Stock tokens the Robinhood card already shows
    /// are left out here. Kept, not worked out on each read: the portfolio's cards read these tens of times a refresh.
    private(set) var holdings: [Holding] = []
    /// The tokens on show, and the ones tucked away (hidden by the user or looking like scam airdrops).
    private(set) var shownTokens: [Holding] = []
    private(set) var tuckedTokens: [Holding] = []

    /// Sorts the tokens again when what they're made of changes; an unchanged list isn't assigned, so nothing redraws.
    private func sortHoldings() {
        let stocks = Set((robinhood?.stocks ?? []).map { $0.address.lowercased() })
        let all = (solana?.tokens ?? []) + otherTokens.filter { !($0.chain == "robinhood" && stocks.contains($0.contract)) }
        if all != holdings { holdings = all }
        let shown = all.filter(tokenShown), tucked = all.filter { !tokenShown($0) }
        if shown != shownTokens { shownTokens = shown }
        if tucked != tuckedTokens { tuckedTokens = tucked }
    }
    /// What the tokens on show are worth, the networks' own coins apart: those are gas, like SOL.
    var shownTokensUSD: Decimal { shownTokens.filter { !$0.native }.reduce(0) { $0 + ($1.usd ?? 0) } }
    var shownNativeUSD: Decimal { shownTokens.filter(\.native).reduce(0) { $0 + ($1.usd ?? 0) } }
    var hl: Hyperliquid.Balance?
    var hlError: String?
    var quote: BridgeQuote?
    var quoteError: String?
    /// How a new Hyperliquid account opens, as the user chose on the cards: true = NEAR Intents, false = Relay, nil = not asked.
    var openVia1Click: Bool?
    /// The EVM network the plan's USDC leaves from, straight to Hyperliquid, when Solana is short; nil = Solana.
    var fundingNet: EVMNetwork?
    var updatedAt: Date? {
        didSet { if balancesLoaded != (updatedAt != nil) { balancesLoaded = updatedAt != nil } }
    }
    /// The balances were read: what the rail and the portfolio ask instead of `updatedAt`, which is new every refresh
    /// and redrew them both each time.
    private(set) var balancesLoaded = false
    /// The tokens on the other networks were read for this wallet (loadHoldings): the balance waits for them too.
    var holdingsLoaded = false
    /// Each wallet's last full balance, by its id: shown faded while a new one is read, after a switch or at launch.
    var lastBalance: [String: Decimal] = LocalStore.load("balances.json") ?? [:] {
        didSet { LocalStore.save(lastBalance, "balances.json") }
    }
    /// Whether the signing process holds a key; nil outside ihatecrypto.app, where there is no signer.
    var signerHasWallet: Bool?
    /// The wallet whose key the signing process holds; only then can a plan be executed.
    var signerWallet: TradeWallet?
    /// Every wallet in the signer (public parts) and the one that signs; state kept on this Mac per wallet (exit rules,
    /// recurring buys, the portfolio curve) sits under its id (Wallets.swift).
    var wallets: [SignerWallet] = []
    var activeWalletID: String? = UserDefaults.standard.string(forKey: "activeWalletID") {
        didSet { UserDefaults.standard.set(activeWalletID, forKey: "activeWalletID") }
    }
    /// «Connect your Ledger» on the canvas while it runs or until closed (Ledger.swift).
    var ledgerConnect: LedgerConnect?
    /// What already said it waits for the Ledger (a scheduled buy's symbol, «guardian»): said once, not every tick.
    @ObservationIgnored var ledgerWaitNotified: Set<String> = []
    /// The in-app dApp browser; its wallet is the signer's.
    @ObservationIgnored let browser = Browser()
    /// A chart, an explorer or a transaction opened beside the canvas (WebPanel.swift), not in a browser.
    @ObservationIgnored let sidePage = SidePage()
    var showWeb = false
    /// The panel shows the dApp browser, with the wallet, instead of a plain page.
    var webWallet = false
    /// The signer's Hyperliquid agent: orders without Touch ID, no withdrawals.
    var agentAddress: String?
    var agentActive = false
    /// The Hyperliquid account approved ihatecrypto's fee (HLBuilder); nil until read (Settings reads it).
    var feeApproved: Bool?
    /// Seconds one Touch ID confirmation keeps signing open (the signer's setting).
    var sessionLength = SignerSession.standard

    // Execution
    var prepared: Executor.Prepared? {
        didSet { if prepared != nil, oldValue == nil { remindToConfirm(say("The trade is ready to sign.", "Сделка готова к подписи.")) } }
    }
    var preparing = false
    var executing = false {
        didSet {
            // A new move: a failure before it that wasn't reasoned about is old news.
            if executing, !oldValue { failure = nil }
            guard oldValue, !executing else { return }
            lastOwnMove = Date(); endSigningSession(); continueTask()
            Task { await afterMove() }
        }
    }

    /// One Touch ID covers one move: once it's done the next signature asks again, unless the user chose 5–30 minutes.
    func endSigningSession() {
        guard sessionLength == SignerSession.standard, !executing, SignerClient.isAvailable else { return }
        Task { _ = try? await SignerClient().send(.endSession) }
    }
    var executionError: String?
    /// The last money move or trade that didn't go through, until the model has reasoned about it (explainFailure).
    @ObservationIgnored var failure: (what: String, why: String, at: Date)?
    /// A confirmed money move while it runs: its route stays on the canvas with the latest step, until the outcome.
    var runningAction: (title: String, steps: [ActivityBubble], lines: [String], started: Date)? {
        didSet { if moveRunning != (runningAction != nil) { moveRunning = runningAction != nil } }
    }
    /// A move is running: what the canvas and the canvas list ask, not `runningAction`, whose every log line would redraw
    /// them (only RunningBoard shows the lines).
    private(set) var moveRunning = false
    var planReview: JevReview?
    /// The second check of the plan under way (startPlanReview).
    var planReviewTask: Task<Void, Never>?
    var txReview: JevReview?
    var jevNote: String?
    var positions: [HyperliquidAccount.Position] = []
    /// Positions closed in the last 7 days, newest first, from the exchange: a close shows up even if it happened
    /// while ihatecrypto was shut or another run closed it. nil until loaded.
    var recentlyClosed: [TradeHistory.Story]?
    /// Margin balances on HIP-3 exchanges («xyz»), separate from the main one.
    var dexBalances: [String: (accountValue: Decimal, withdrawable: Decimal)] = [:]
    /// The plan last carried out; while the intent equals it, the card shows the position, not a new trade.
    var executed: TradeIntent?
    /// Regular purchases, carried out while the app is open.
    /// An allocation plan shown and waiting for its Sign button.
    var pendingAllocation: WalletCommand? {
        didSet { if pendingAllocation != nil, oldValue == nil { remindToConfirm() } }
    }
    /// A command the model read from another language: it waits for «do it», so a wrong reading moves nothing.
    var pendingAction: PendingAction? {
        didSet {
            if pendingAction != nil, oldValue == nil { remindToConfirm() }
            // A way picked on a route's step belongs to that trade: another move waiting for Confirm drops it.
            if case .wallet(let command)? = pendingAction { routeWays = routeWays.filter { $0.key == command.wayKey } }
            if let p = pendingAction, !isFirstBuy(p) { firstBuyOf = nil }
        }
    }
    /// The recurring buy whose first buy is the card waiting for Confirm: it's kept only when that card is confirmed
    /// (carryOut), so a Cancel or another card in its place leaves no schedule behind.
    var firstBuyOf: Schedule?

    func isFirstBuy(_ p: PendingAction) -> Bool {
        guard let s = firstBuyOf, case .wallet(.buy(let asset, _)) = p else { return false }
        return asset.symbol == s.symbol
    }

    /// A card waits for Confirm while the window isn't in front (the model took a few seconds and the user looked
    /// away): a notification says so. Read on the next turn of the run loop, when the card's text is set.
    func remindToConfirm(_ what: String? = nil) {
        Task { @MainActor in
            // A test through the debug API makes cards too: they don't call the user over.
            guard !NSApp.isActive, !SignerClient.untrusted else { return }
            notify(.confirm, say("Waiting for your Confirm", "Ждёт твоего Confirm"),
                   what ?? pendingText ?? say("A move is on the canvas.", "Действие на холсте."), record: false)
        }
    }
    /// The last money move carried out on this canvas: whether and when it went through, and its route as confirmed.
    /// The canvas keeps them in place of the «waiting for the signature» they replace.
    var finishedAction: (title: String, outcome: CanvasOutcome, route: [ActivityBubble])?
    /// The move that just failed on a canvas: its board's «Try again» lays the same card out again, quoted afresh.
    var retryCommand: (canvas: UUID?, command: WalletCommand)?
    /// `orders`: a model's new stop or take for an open position, moved on the exchange after Confirm; `intent` is the
    /// plan with them, which becomes the canvas's then.
    enum PendingAction {
        case wallet(WalletCommand), position(PositionCommand, coin: String), orders([PositionCommand], coin: String, intent: TradeIntent)
        case address(SavedAddress), transfer(SavedAddress, Decimal)
    }
    /// The English command the pending action was understood as, for the confirm button.
    var pendingText: String?
    /// A paid API's price as its 402 named it, for the «pay …» card waiting for Confirm (Payments.swift).
    var payment: PaymentQuote?
    /// A contract call the chat wrote, checked and simulated, for its card (ContractCalls.swift).
    var callReview: CallReview?
    /// The multisigs made here (Multisig.swift), the one being set up on its card, and a payment out of one, read for its card.
    var multisigs: [Multisig] = Workspace.loadMultisigs()
    var multisigDraft: MultisigDraft?
    var multisigPayment: MultisigPayment?
    /// What each multisig holds in dollars, as last read (its board and its portfolio card read it).
    var multisigUSD: [String: Decimal] = [:]
    /// What the paid (or free) service sent back, on the canvas.
    var serviceAnswer: ServiceAnswer?
    /// What Merkl owes the wallet, asked for before a «claim rewards» card (Rewards.swift).
    var rewards: [Merkl.Reward]?
    /// A move left waiting on a canvas the user switched away from: it waits there, for a quarter of an hour.
    var parkedPending: [UUID: (action: PendingAction, text: String?, at: Date)] = [:]
    /// A deposit or buy that came without an amount: the next reply that is just a number fills it.
    var awaitingAmount: WalletCommand?
    /// The «Top up ihatecrypto» board on the canvas: our addresses and how balances update.
    var showReceive = false
    /// Bumped after a revoke: the approvals board reads them again.
    var approvalsChanged = 0
    /// «receive 50 USDC from Sui»: 1Click's deposit address there, on the canvas (ReceiveAcrossBoard).
    var acrossDeposit: AcrossDeposit?
    /// A market's price chart on the canvas (Modules.swift). Opened from a coin's row it takes the place of the
    /// token cards (`detail` «chart»), so closing it frees that place too.
    var chart: ChartState? { didSet { if chart == nil, detail == "chart" { detail = nil } } }
    /// The day's biggest movers on the canvas.
    var showMovers = false
    /// The price alerts as a board on the canvas, opened when one is set.
    var showAlerts = false
    /// The latest of the history and the favorites on the canvas, opened by the rail's buttons.
    var showHistory = false
    var showFavorites = false
    /// The Schedule frame on the canvas beside the portfolio (ScheduleFrame.swift), and the item last opened on its own
    /// canvas: «task:<id>», «buy:SOL», «perp:BTC».
    var showSchedule = false
    var scheduleItem: String?
    /// The canvas a scheduled item's canvas was opened from: its Close goes back there.
    @ObservationIgnored var scheduleReturn: UUID?
    /// A canvas switch that leaves the view where it is (an item's canvas opened or closed): no scroll back to the start.
    @ObservationIgnored var keepViewport = false
    /// «Pause schedule»: no task or recurring buy runs until it's resumed; stops and takes stay on the exchange.
    var schedulePaused = UserDefaults.standard.bool(forKey: "schedulePaused") {
        didSet { UserDefaults.standard.set(schedulePaused, forKey: "schedulePaused") }
    }
    /// The automation wallet's USDC, read while the Schedule frame shows.
    var automationBudget: AutomationBudget?
    /// Where the chat is in the window, so scrolling over it isn't taken by the canvas.
    @ObservationIgnored var chatFrame: CGRect = .zero
    /// The same for the rail, which floats over the canvas too.
    @ObservationIgnored var railFrame: CGRect = .zero
    /// The plan being assembled on the canvas, block by block (S02: the chat shows the count). Read only by the count
    /// itself (AssemblyCount): it changes with every card put down, and the canvas and the chat ask `assembling`.
    var assembly: (shown: Int, total: Int)? {
        didSet { if assembling != (assembly != nil) { assembling = assembly != nil } }
    }
    private(set) var assembling = false
    /// A place opened from the rail or the portfolio: «solana», «hyperliquid», «jupiter», «aave»; «chart» the chart of a
    /// network's coin, opened from its row where a token's card opens. Another place opened there replaces that chart.
    var detail: String? { didSet { if oldValue == "chart", detail != "chart" { chart = nil } } }
    /// What the token card in `detail` shows, written by the card as its numbers load: the chat's model reads it.
    @ObservationIgnored var tokenCard: (place: String, text: String)?
    /// When ihatecrypto last finished moving money itself; incoming money right after it is its own, not a top-up.
    var lastOwnMove = Date.distantPast
    /// Everything ihatecrypto did, with the canvas bubbles of each moment; saved on every change.
    var activities: [Activity] = LocalStore.load("activity.json") ?? [] {
        didSet {
            // The file keeps the last 1000; memory too, or a long session copies and walks an ever longer list.
            if activities.count > 1100 { activities.removeFirst(activities.count - 1000) }
            LocalStore.save(Array(activities.suffix(1000)), "activity.json")
        }
    }
    /// Every canvas with its chat, plan and model (see Canvases.swift); saved on every change.
    var canvases: [CanvasSession] = LocalStore.load("canvases.json") ?? [] {
        didSet { LocalStore.save(canvases, "canvases.json") }
    }
    /// Canvas folders made or used in the sidebar; they stay while empty.
    var folders: [String] = UserDefaults.standard.stringArray(forKey: "canvasFolders") ?? [] {
        didSet { UserDefaults.standard.set(folders, forKey: "canvasFolders") }
    }
    /// A folder's picture by its name («Change icon…» on the folder); none = the folder sign.
    var folderIcons: [String: AddressIcon] = LocalStore.load("folder-icons.json") ?? [:] {
        didSet { LocalStore.save(folderIcons, "folder-icons.json") }
    }
    /// The folder a fresh canvas goes into once it's kept («New canvas here» on a folder).
    var newCanvasFolder: String?
    /// The portfolio's value every ten minutes for 30 days (Alerts.swift).
    var equityHistory: [EquityPoint] = LocalStore.load(Workspace.scoped("equity") + ".json") ?? [] {
        didSet { LocalStore.save(equityHistory, Workspace.scoped("equity", activeWalletID) + ".json") }
    }
    /// What each held token cost as the chain shows it (`loadChainCosts`), by the same keys, with the amount held when it
    /// was read: tokens bought outside the app have a P/L too. Read again when that amount changes; not kept.
    var chainCost: [String: (held: Decimal, cost: CostBasis)] = [:]
    /// What the Solana tokens bought here cost, by mint: the P/L on their portfolio rows. Kept per wallet.
    var costBasis: [String: CostBasis] = LocalStore.load(Workspace.scoped("costbasis") + ".json") ?? [:] {
        didSet { LocalStore.save(costBasis, Workspace.scoped("costbasis", activeWalletID) + ".json") }
    }
    /// Tokens the user watches (TokensUI.swift), their DexScreener data and what the wallet holds of them.
    var favoriteTokens: [FavoriteToken] = LocalStore.load("tokens.json") ?? [] {
        didSet { LocalStore.save(favoriteTokens, "tokens.json") }
    }
    /// Behind setters that skip an unchanged value: a `merge` in place (TokensUI, StocksUI) otherwise redraws the
    /// window on every refresh, even with nothing new.
    var tokenInfo: [String: TokenLookup.Info] {
        get { tokenInfoRead }
        set { if newValue != tokenInfoRead { tokenInfoRead = newValue } }
    }
    private var tokenInfoRead: [String: TokenLookup.Info] = [:]
    /// Symbols of contracts that aren't favorites, by lowercased contract, learned when one is shown (Canvases.swift).
    var tokenNames: [String: String] = LocalStore.load("tokennames.json") ?? [:] {
        didSet { LocalStore.save(tokenNames, "tokennames.json") }
    }
    @ObservationIgnored var tokenNamesAsked: Set<String> = []
    /// Contracts and mints typed after «@» in the chat's field, with what DexScreener knows of each (lookUpContract).
    var contractLookups: [String: [TokenLookup.Info]] = [:]
    var tokenBalances: [String: Decimal] {
        get { tokenBalancesRead }
        set { if newValue != tokenBalancesRead { tokenBalancesRead = newValue } }
    }
    private var tokenBalancesRead: [String: Decimal] = [:]
    var tokensCheckedAt = Date.distantPast
    /// Price alerts, one notification each; checked with every refresh (Alerts.swift).
    var alerts: [PriceAlert] = LocalStore.load("alerts.json") ?? [] {
        didSet { LocalStore.save(alerts, "alerts.json") }
    }
    /// Saved addresses — exchanges, other wallets — entered by hand in the address book only (AddressBook.swift).
    var addressBook: [SavedAddress] = LocalStore.load("addressbook.json") ?? [] {
        didSet { LocalStore.save(addressBook, "addressbook.json") }
    }
    /// The names and addresses confirmed with Touch ID, as the signer keeps them: money goes to a saved name only
    /// at the address here (isConfirmed), never at what the file says.
    var confirmedAddresses: [SignerAddress] = []
    /// The canvas on screen; nil for a fresh one nobody has written in yet. Remembered, so it opens again at launch.
    var canvasID: UUID? {
        didSet { UserDefaults.standard.set(canvasID?.uuidString, forKey: "openCanvas") }
    }
    /// The skills the model loaded on this canvas (the `skill` lookup): their text comes with every message here.
    var activeSkills: [String] = []
    /// A reopened canvas whose board isn't a live plan: drawn as it was.
    var savedBoard: CanvasSession?
    /// A note's card: on its own canvas, or where it was just written until the next message.
    var noteCard: NoteCard?
    /// The open canvas once its money move is carried out: its route stays on screen, marked Done or Failed with the
    /// time, until a new board replaces it.
    var finishedCanvas: CanvasSession? { canvases.first { $0.id == canvasID && $0.outcome != nil } }
    /// The activity replies are added to.
    var currentActivity: UUID?
    /// What the user is typing in the studio chat; templates on the canvas fill it.
    var draft = ""
    /// A part of the draft to select in the field, to type over (UTF-16 in `draft`); cleared once it's selected.
    var draftSelection: NSRange?
    /// What the next message is about: canvas blocks, route steps, tokens, addresses handed to the chat — a chip each over
    /// the field, an attached block's edge lit. Several at once; the message takes them all.
    var attached: [StepContext] = []
    /// The way a Base or Arbitrum trade takes, picked in its route step's details (Workspace.wayKey → "relay" or
    /// "uniswap"); none = whichever leaves more. Used once, by the move it was picked for.
    var routeWays: [String: String] = [:]
    /// Both ways of the Base or Arbitrum trade waiting for Confirm, quoted now, the better first (Workspace.quoteSwap):
    /// its card shows the one that leaves more. Empty = neither quotes.
    /// USDT just read from a network's node for a move of it (freshenSource): its card's balance while the explorer the
    /// tokens are read through is still behind — a refresh from it put «Move 0 USDT» on a card of 0.7 that had landed.
    var usdtRead: (place: String, amount: Decimal, at: Date)?
    var swapQuote: (command: WalletCommand, ways: [(way: String, gets: String, worth: Decimal)])? {
        didSet { if swapQuote?.ways.isEmpty == false { swapQuotedAt = Date() } }
    }
    /// When the swap waiting for Confirm was quoted: its Confirm holds `quoteLife` after that, then Refresh asks again.
    var swapQuotedAt: Date?
    /// The swap whose card shows a price (Workspace.quoteSwap): the one Confirm waits on a fresh quote for.
    var pricedCommand: WalletCommand?
    /// A Solana swap's output as its card showed it, in the token's units: the swap won't go for 1% less (Executor.convert).
    var jupiterShown: (command: WalletCommand, out: UInt64)?
    /// Bumped by Refresh: the waiting card's quote starts over.
    var quoteRound = 0
    static let quoteLife: TimeInterval = 15
    /// Speaking into the chat's field.
    let voice = VoiceInput()
    /// A menu or Settings the person has open ("Picking a model", "Choosing networks", "In Settings"): the coin raises
    /// its brows and looks at it, and the chat's header says so.
    var tuning: String?
    /// Settings fill the main window in place of the canvas and the chat.
    var settingsOpen = false

    /// Shows Settings, on `tab` (a `SettingsView.Tab`) when given.
    func openSettings(_ tab: SettingsView.Tab? = nil) {
        if let tab { UserDefaults.standard.set(tab.rawValue, forKey: SettingsView.tabKey) }
        settingsOpen = true
    }
    /// «Spread … across the best rates» waiting for a signature, drawn on the studio canvas.
    var allocationPreview: AllocationPreview?
    /// «Переложи в лучшие ставки» waiting for its signature: the plan, drawn on the canvas it was made on (RebalanceBoard).
    var rebalancePlan: (canvas: UUID?, plan: Rebalance.Plan, maxShare: Decimal)?
    /// Yields shown as bubbles on the studio canvas; nil when none.
    var yieldOptions: [Yields.Option]?
    /// Take-profit styles offered on the canvas after «take profit like the best investors»; nil when none.
    var takeChoices: [TakeStyle]?
    /// The ways to go on offered with the last reply, as cards under it in the chat (Agent.swift `offerCards`).
    var nextSteps: NextSteps? { didSet { if cardKey != nil { cardKey = nil } } }
    /// The cards under the last reply while they may still be picked: 30 minutes from the reply, then their numbers are
    /// old and they fold away (the owner, 2026-10-19: «не хранить долго»). A reply saved before times were kept: never.
    var liveSteps: NextSteps? {
        guard let s = nextSteps, let at = messages.last(where: { $0.id == s.message })?.at, Date().timeIntervalSince(at) < 30 * 60 else { return nil }
        return s
    }
    /// Cards folded or opened by hand, by their reply: true folded. Otherwise live ones are open and old ones folded.
    var cardFold: [UUID: Bool] = [:]
    /// The live card ↑↓ moved to from the empty field (ChatComposer): Return picks it; typing or Esc lets it go.
    var cardKey: Int?
    /// A move the loss cap stopped (CostlyMove) and what it would lose: «Send anyway» runs it again with that accepted.
    @ObservationIgnored var costlyMove: (command: WalletCommand, lost: Decimal)?
    /// The last answer's boards while the next message or picked card is handled: drawn as they were, not clickable,
    /// until the answer is on the canvas. Cleared at once for the logic, they'd leave the canvas blank while the model thinks.
    var heldAnswer: (yields: [Yields.Option]?, takes: [TakeStyle]?)?
    /// What the model is looking up right now («Looking at the token»), under the chat's thinking dots.
    var agentStep: String?
    /// A request the model carries out over several commands (Agent.swift `continueAgent`).
    var agentGoal: AgentGoal?
    /// The last scheduled check found nothing worth telling (Agent.swift `runCheck`): no notification.
    var checkQuiet = false
    /// The model is answering or looking things up (`modelCall`): the send button is Stop.
    var modelRunning = false
    /// Stop was pressed while the model read this message: nothing it would lead to runs.
    var stopRequested = false
    /// Cancels the model call running now; nil when none runs.
    @ObservationIgnored var cancelModel: (() -> Void)?
    var schedules: [Schedule] = (UserDefaults.standard.data(forKey: Workspace.scoped("schedules")).flatMap { try? JSONDecoder().decode([Schedule].self, from: $0) }) ?? [] {
        didSet { UserDefaults.standard.set(try? JSONEncoder().encode(schedules), forKey: Workspace.scoped("schedules", activeWalletID)) }
    }
    /// The running Jupiter DCA orders as last read, drawn on the canvas once asked for («show Jupiter DCA») or after one
    /// is placed or cancelled; nil = not shown.
    var jupiterDCAs: [JupiterRecurring.Order]?
    /// Models being downloaded into Ollama from the model menu: name → 0…1.
    var modelDownloads: [String: Double] = [:]
    /// Whether Ollama and LM Studio answered when the menu last asked (provider id → up), and the download sizes, GB,
    /// of the suggested models they haven't got.
    var localServers: [String: Bool] = [:]
    var modelSizes: [String: Double] = [:]
    /// Ollama's version, and whether a download said it's too old for a model («requires a newer version of Ollama»).
    var ollamaVersion: String?
    var ollamaOutdated = false
    /// Ollama's newest release, read once a launch; and what installing it is doing right now (nil = not running).
    var ollamaLatest: String?
    var ollamaInstall: String?
    /// What installing Claude Code is doing now; nil when it isn't.
    var claudeCodeInstall: String?
    /// Ollama was installed or updated this launch: Homebrew's may stay a patch behind GitHub's, and asking again would loop.
    @ObservationIgnored var ollamaUpdated = false
    /// The recurring buy the two ways were offered for: picking «in the app» brings it back and it goes ahead.
    @ObservationIgnored var dcaChoice: WalletCommand?
    /// Commands scheduled for later or on repeat («in 1 week: rebalance USDC»), per wallet; `watch` runs them (Tasks.swift).
    var tasks: [ScheduledTask] = TaskSeal.tasks(forKey: Workspace.scoped("tasks")).tasks {
        didSet { TaskSeal.save(try? JSONEncoder().encode(tasks), forKey: Workspace.scoped("tasks", activeWalletID)) }
    }
    /// The tasks were changed outside ihatecrypto (TaskSeal) and loaded paused: said once in the chat.
    @ObservationIgnored var tasksTampered = TaskSeal.tasks(forKey: Workspace.scoped("tasks")).tampered
    /// A running task's steps after one that waits for Confirm, and the canvas it runs on: they go when that move is done.
    /// `brought`: networks the steps before delivered USDC to, so a step there waits for the money (runSteps).
    var taskQueue: (canvas: UUID?, steps: [String], since: Date, brought: Set<String>)?
    /// While a confirmed chain runs, its steps after the one under way: drawn after its cards (Workspace.chainSteps).
    /// Only for the canvas; `taskQueue` is what runs.
    var chainAhead: (canvas: UUID?, steps: [String])?
    /// Exit rules the app keeps for open positions (breakeven after the partial take, trailing), by coin.
    var exitRules: [String: ExitRules] = (UserDefaults.standard.data(forKey: Workspace.scoped("exitRules")).flatMap { try? JSONDecoder().decode([String: ExitRules].self, from: $0) }) ?? [:] {
        didSet { UserDefaults.standard.set(try? JSONEncoder().encode(exitRules), forKey: Workspace.scoped("exitRules", activeWalletID)) }
    }
    /// The position the plan card shows: the plan's asset if open, otherwise the first one.
    var position: HyperliquidAccount.Position? { positions.first { $0.coin == intent.asset } ?? positions.first }

    /// For the «Расписание» tab: regular purchases and positions the guardian watches.
    var scheduledCount: Int { schedules.count + exitRules.count }

    /// «0,369 по 93,417 · стоп 96,033 · тейк 90,801 и 88,186»: size, entry and the exchange's trigger orders.
    func positionSummary(_ p: HyperliquidAccount.Position) -> String {
        let triggers = orders.filter { $0.coin == p.coin && $0.isTrigger }
        let stop = triggers.first { ($0.orderType ?? "").contains("Stop") }?.triggerPx
        // Nearest take first: a partial one fills before the full one.
        let takes = triggers.filter { ($0.orderType ?? "").contains("Take") }.compactMap(\.triggerPx)
            .sorted { p.size > 0 ? $0 < $1 : $0 > $1 }
        return "\(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)) · stop \(stop.map(Fmt.price) ?? "none")"
            + (takes.isEmpty ? "" : " · take " + takes.map(Fmt.price).joined(separator: " and "))
    }
    var orders: [HyperliquidAccount.Order] = []
    /// A limit entry waiting on the book (its stop and take are the trigger orders next to it).
    var pendingEntry: HyperliquidAccount.Order? { orders.first { !$0.isTrigger } }
    var lend: [(vault: String, usdc: Decimal)] = []
    /// Kamino Lend deposits, in dollars.
    var kamino: Decimal = 0
    /// The NEAR implicit account the signer derived from the same phrase; nil until it has been asked once.
    var nearAddress: String?
    /// NEAR on that account: native and wNEAR, in NEAR.
    var near: Decimal = 0
    /// Its value in dollars, at Hyperliquid's NEAR price.
    var nearUSD: Decimal { near * (markets["NEAR"]?.markPx ?? 0) }
    /// Gas money: SOL on Solana, ETH on Base and Robinhood Chain, at Hyperliquid's prices. It is the user's money, so
    /// the balance shows it (`total`); the risk rules leave it out (`equity`), since no trade may spend it.
    var solUSD: Decimal { (solana?.sol ?? 0) * (markets["SOL"]?.markPx ?? 0) }
    var gasETH: Decimal { (base?.eth ?? 0) + (robinhood?.eth ?? 0) }
    var gasETHUSD: Decimal { gasETH * (markets["ETH"]?.markPx ?? 0) }
    /// Everything the wallet holds, gas included: the balance on screen.
    var total: Decimal? { equity.map { $0 + solUSD + gasETHUSD + shownNativeUSD } }
    /// Every read the balance adds up came back for this wallet: the networks, the tokens on the others, the automation
    /// wallet. Until then the screen shows the last full balance, never the sum of what came so far: after a wallet
    /// switch that read $0 for seconds, and the money looked gone.
    var balanceReady: Bool {
        balancesLoaded && holdingsLoaded && (solana.map { !$0.unpriced } ?? !isSolanaAddress(solanaAddress)) && (hl != nil || !isEVMAddress(hlAddress))
            && (automationWallet == nil || automationBudget != nil)
    }
    /// Both wallets, the main one and Automation: the one balance the sidebar, the portfolio and the menu bar show.
    var balance: Decimal? { balanceReady ? total.map { $0 + (automationUSD ?? 0) } : nil }
    func rememberBalance() {
        guard let balance, lastBalance[activeWalletID ?? ""] != balance else { return }
        lastBalance[activeWalletID ?? ""] = balance
    }
    /// Hyperliquid's HLP vault: what is in it and, while a deposit is locked, until when.
    var hlp: (equity: Decimal, lockedUntil: Date?) = (0, nil)
    /// On Base, at the Hyperliquid (EVM) address: idle USDC, USDC supplied to Aave, ETH for gas.
    var base: (usdc: Decimal, aave: Decimal, morpho: Decimal, eth: Decimal)?
    /// USDC on Arbitrum at our EVM address (money in transit or left from a transfer).
    var arbitrumUSDC: Decimal?
    /// Bridge orders on their way (Executor.resolveTransit): shown above the network cards until they land.
    var inTransit: [Journal.Entry] = []
    /// USDC on Tempo, read with the other networks' tokens: what pays for services (MPP) and Tempo transfers.
    var tempoUSDC: Decimal? { heldUSDC("tempo", EVMRPC.tempoUSDC) }
    var hyperevmUSDC: Decimal? { heldUSDC("hyperevm", EVMRPC.hyperevmUSDC) }
    var optimismUSDC: Decimal? { heldUSDC("optimism", EVMRPC.optimismUSDC) }
    var bnbUSDC: Decimal? { heldUSDC("bnb", EVMRPC.bnbUSDC) }
    /// Robinhood Chain at our EVM address: USDG, ETH for gas, the Stock Tokens bought through ihatecrypto (StocksUI.swift).
    /// A tuple isn't Equatable, so @Observable would redraw on every refresh's assignment: the setter skips an unchanged one.
    var robinhood: (usdg: Decimal, eth: Decimal, stocks: [StockHolding])? {
        get { robinhoodRead }
        set {
            if let n = newValue, let o = robinhoodRead, n == o { return }
            if newValue == nil, robinhoodRead == nil { return }
            robinhoodRead = newValue
        }
    }
    private var robinhoodRead: (usdg: Decimal, eth: Decimal, stocks: [StockHolding])? { didSet { sortHoldings() } }
    /// Aave v3 USDC supply APY on Base, %, read from the pool itself.
    var aaveAPY: Decimal?
    /// Morpho's Gauntlet USDC Prime vault on Base, net of its fee.
    var morphoAPY: Decimal?
    private var aaveCheckedAt: Date?
    var reviewedIntent: TradeIntent?

    init() {
        // The user's own nodes, from the Keychain, before anything reads a network.
        NodeStore.activate()
        restoreLastCanvas()
        // The chat names a contract it knows; it never looks one up itself (a wallet's address isn't DexScreener's business).
        ChatText.tokenName = { [weak self] contract in self.flatMap { $0.favoriteToken(contract)?.symbol ?? $0.tokenNames[contract.lowercased()] } }
        // What is on the open canvas at quit (a chart opened by a click, a board that finished building) is kept too.
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: .main) { [weak self] _ in
            // Then every file saved so far is written before the app goes (LocalStore writes off the main thread).
            MainActor.assumeIsolated { self?.saveCanvas(); LocalStore.flush() }
        }
        // A locked screen or sleep ends the Touch ID session, however long the user let it be.
        let endSession: @Sendable (Notification) -> Void = { _ in
            Task { if SignerClient.isAvailable { _ = try? await SignerClient().send(.endSession) } }
        }
        DistributedNotificationCenter.default().addObserver(forName: .init("com.apple.screenIsLocked"), object: nil, queue: .main, using: endSession)
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main, using: endSession)
    }
    private var quotedKey: String?

    var solanaAddress = UserDefaults.standard.string(forKey: "solanaAddress") ?? "" {
        didSet { UserDefaults.standard.set(solanaAddress, forKey: "solanaAddress"); Task { await loadSolana() } }
    }
    var hlAddress = UserDefaults.standard.string(forKey: "hlAddress") ?? "" {
        didSet { UserDefaults.standard.set(hlAddress, forKey: "hlAddress"); Task { await loadHL(); await refreshQuote() } }
    }

    // MARK: Model connection. Which provider and model live in UserDefaults; keys and tokens only in Keychain.

    var providerID = UserDefaults.standard.string(forKey: "llmProvider") ?? "chatgpt" {
        didSet { UserDefaults.standard.set(providerID, forKey: "llmProvider"); llmStatus = nil; availableModels = [] }
    }
    var modelChoices = UserDefaults.standard.dictionary(forKey: "llmModels") as? [String: String] ?? [:] {
        didSet { UserDefaults.standard.set(modelChoices, forKey: "llmModels") }
    }
    /// Second opinion on incoming text from Jev via OpenRouter. On by default (product decision, 2026-09-25);
    /// it needs an OpenRouter login and sends the message text there. Local rules run regardless.
    var policy: RiskPolicy = (UserDefaults.standard.data(forKey: "riskPolicy").flatMap { try? JSONDecoder().decode(RiskPolicy.self, from: $0) }) ?? {
        var p = RiskPolicy()
        // `-maxRiskPct 10`: a looser limit for tests on a tiny wallet.
        #if DEBUG
        if let v = UserDefaults.standard.string(forKey: "maxRiskPct").flatMap({ Decimal(string: $0) }) { p.maxRiskPct = v }
        #endif
        return p
    }() {
        didSet { UserDefaults.standard.set(try? JSONEncoder().encode(policy), forKey: "riskPolicy") }
    }
    /// 4-hour ATR per asset, % of price.
    var atr: [String: Decimal] = [:]
    var jevScreening = UserDefaults.standard.object(forKey: "jevScreening") as? Bool ?? true {
        didSet { UserDefaults.standard.set(jevScreening, forKey: "jevScreening") }
    }
    /// The second check may use OpenRouter while the chat runs elsewhere: only when the user turned it on (Settings ›
    /// Second check, or «Connect» there). A key left from an earlier OpenRouter chat doesn't send texts there by itself.
    var jevOpenRouter = UserDefaults.standard.bool(forKey: "jevOpenRouter") {
        didSet { UserDefaults.standard.set(jevOpenRouter, forKey: "jevOpenRouter") }
    }
    /// The key the second check sends texts with: OpenRouter's, when the chat runs there or the user turned it on.
    var jevKey: String? { providerID == "openrouter" || jevOpenRouter ? openRouterKey : nil }
    /// On: Solana buys only the fixed verified coins. Off: any token by its mint, with its risks said first (TokensUI.swift).
    var safeSearch = UserDefaults.standard.object(forKey: "safeSearch") as? Bool ?? true {
        didSet { UserDefaults.standard.set(safeSearch, forKey: "safeSearch") }
    }
    var openRouterKey: String? {
        _ = credentialVersion
        if case .apiKey(let key)? = Keychain.load("openrouter") { return key }
        // A debug build takes it from the environment too, as WalletCheck's jev-screen does: the Dev copy has its own
        // model logins and no OpenRouter one, so the second check runs there without a sign-in in the browser
        // (`open --env OPENROUTER_API_KEY=… "ihatecrypto Dev.app"`).
        #if DEBUG
        if let key = ProcessInfo.processInfo.environment["OPENROUTER_API_KEY"], !key.isEmpty { return key }
        #endif
        return nil
    }
    private var credentialVersion = 0
    var llmBusy = false
    var llmStatus: String?
    var availableModels: [String] = []

    // The account the chat uses is `providerID`; the functions that take an `id` work on any account, so Settings can
    // connect one without the chat switching to it.
    var provider: any LLMProvider { provider(providerID) }
    func provider(_ id: String) -> any LLMProvider { LLMCatalog.provider(id: id) ?? LLMCatalog.all[0] }
    var model: String {
        get { model(providerID) }
        set { modelChoices[providerID] = newValue }
    }
    func model(_ id: String) -> String {
        let info = provider(id).info, chosen = modelChoices[id] ?? info.defaultModel
        // A subscription that doesn't offer the chosen model now answers 400 for it (a lapsed ChatGPT plan lists only
        // gpt-6-luna and 5.6): its first listed model stands in, and the choice comes back once the list has it again.
        // ponytail: the list kept is its first four; a choice further down would be stood in for too.
        if info.kind == .subscription, let live = liveModels[id], let first = live.first, !live.contains(chosen) { return first }
        return chosen
    }
    var credential: Credential? { credential(providerID) }
    func credential(_ id: String) -> Credential? {
        _ = credentialVersion
        return Keychain.load(id)
    }
    /// Whether the chosen provider has a login, for views: a Keychain read is a round trip to securityd (≈ 4 ms) and
    /// the rail redraws with every balance refresh. A login found is remembered (a token refresh never removes one);
    /// «none» is read again, since a read can fail once (a fresh build's Keychain prompt) and must not stick.
    var signedIn: Bool { signedIn(providerID) }
    func signedIn(_ id: String) -> Bool {
        _ = credentialVersion
        if signedInCache.contains(id) { return true }
        guard Keychain.load(id) != nil else { return false }
        signedInCache.insert(id)
        return true
    }
    @ObservationIgnored private var signedInCache: Set<String> = []
    /// A model can read the chat: a subscription or a key signed in, or on this Mac a downloaded model of a server that
    /// answers. Without one the chat isn't offered: keyword guesses misread free words, and people took that for the product.
    var hasModel: Bool { hasModel(providerID) }
    func hasModel(_ id: String) -> Bool {
        let info = provider(id).info, model = self.model(id)
        guard info.unavailable == nil, LLMCatalog.chatRefusal(model: model) == nil else { return false }
        guard info.kind == .local else { return signedIn(id) }
        return localServers[info.id] != false && liveModels[info.id].map { $0.contains(model) } != false
    }

    /// The chat goes on with this account, as the profile menu and Settings' «Switch» do it; its model stays as
    /// chosen for it.
    func useAccount(_ id: String) {
        providerID = id
        // A server on this Mac runs only what it has downloaded: its first chat model stands in for one that isn't there.
        if provider.info.kind == .local, let live = liveModels[id], !live.contains(model),
           let first = live.first(where: { LLMCatalog.chatRefusal(model: $0) == nil }) { model = first }
        // Loaded into memory now, so the first answer doesn't wait for it.
        if id == "ollama" { OllamaModels.preload(model) }
        saveCanvas()
    }

    /// How each model did in the chat's exam (Settings › AI model › Check), by «provider/model»; kept between launches.
    var modelScores: [String: ModelExam.Result] = (UserDefaults.standard.data(forKey: "modelScores").flatMap { try? JSONDecoder().decode([String: ModelExam.Result].self, from: $0) }) ?? [:] {
        didSet { UserDefaults.standard.set(try? JSONEncoder().encode(modelScores), forKey: "modelScores") }
    }
    /// Questions of the exam answered so far while it runs; nil when it doesn't.
    var examProgress: Int?

    func score(_ provider: String, _ model: String) -> ModelExam.Result? { modelScores["\(provider)/\(model)"] }

    /// Runs the exam on an account's chosen model (the chat's by default): its score shows in Settings and in the model menu.
    func checkModel(_ id: String? = nil) async {
        let id = id ?? providerID
        guard let client = llmClient(id), examProgress == nil else { llmStatus = "Connect \(provider(id).info.name) first."; return }
        let model = self.model(id), key = "\(id)/\(model)"
        examProgress = 0
        defer { examProgress = nil }
        let result = await ModelExam.run(client: client, model: model) { n in await MainActor.run { self.examProgress = n } }
        modelScores[key] = result
        llmStatus = result.good
            ? "\(LLMCatalog.displayName(model: model)) understood \(result.passed) of \(result.total) requests, \(String(format: "%.1f", result.seconds)) s a reply — good for the chat."
            : "\(LLMCatalog.displayName(model: model)) understood only \(result.passed) of \(result.total) — it will misread requests. A bigger model or a subscription reads better. Missed: " + result.misses.prefix(3).joined(separator: "; ")
    }

    /// Saves an account's login (the chat's by default). Adding one doesn't switch the chat to it (the owner, 2026-10-20:
    /// «я просто хотел подключить другой аккаунт»), unless the chat has no model to go on with.
    func setCredential(_ credential: Credential?, for id: String? = nil) {
        let id = id ?? providerID
        Keychain.save(credential, for: id)
        signedInCache.remove(id)
        credentialVersion += 1
        if credential != nil, id != providerID, !hasModel { useAccount(id) }
    }

    /// Signs out of one account, in use or not: its login leaves the Keychain. A chat that used it asks for a model
    /// again (ConnectModelBar), as with no login at all.
    func signOut(_ id: String) {
        Keychain.save(nil, for: id)
        signedInCache.remove(id)
        credentialVersion += 1
    }

    /// The chat accounts connected now, in the catalog's order (the profile menu): a subscription or key with a saved
    /// login, or a server on this Mac with a model downloaded that isn't known to be down (as `hasModel` reads it).
    var accounts: [ProviderInfo] {
        _ = credentialVersion
        return Self.accounts(LLMCatalog.all.map(\.info), hasLogin: { Keychain.load($0) != nil }, localServers: localServers, liveModels: liveModels)
    }

    /// `accounts` without the Keychain, for tests.
    static func accounts(_ infos: [ProviderInfo], hasLogin: (String) -> Bool, localServers: [String: Bool], liveModels: [String: [String]]) -> [ProviderInfo] {
        infos.filter { info in
            guard info.unavailable == nil else { return false }
            guard info.kind == .local else { return hasLogin(info.id) }
            return localServers[info.id] != false && liveModels[info.id]?.isEmpty == false
        }
    }

    /// nil until the account (the chat's by default) is connected.
    func llmClient(_ id: String? = nil) -> (any LLMClient)? {
        let p = provider(id ?? providerID)
        let id = p.info.id, credential = self.credential(id)
        guard p.info.unavailable == nil, credential != nil || p.info.kind == .local,
              let client = try? p.client(credential, save: { Keychain.save($0, for: id) }) else { return nil }
        // Settings › Models › «Think before answering»: every request reasons first.
        let asked: any LLMClient = UserDefaults.standard.bool(forKey: "modelThinking") ? ThinkingClient(client) : client
        // Every answer says what it read and cost: the chip over the chat's field (ContextMeter.swift).
        return MeteredClient(asked) { [weak self] usage, request in
            Task { @MainActor in await self?.count(usage, for: request, provider: id) }
        }
    }

    /// What the open canvas's chat fills of its model's window, and what its answers cost.
    var meter = ContextMeter()
    /// Windows and list prices of the models a key pays for, by `ModelFacts.key`; read when one first answers.
    @ObservationIgnored var modelFacts: [String: ModelFacts] = [:]
    @ObservationIgnored var modelFactsAsked = Date.distantPast

    func signIn(_ id: String? = nil) async {
        let id = id ?? providerID
        llmBusy = true
        defer { llmBusy = false }
        llmStatus = "Waiting for the sign-in in the browser…"
        do {
            setCredential(try await provider(id).signIn(open: { url in Task { @MainActor in NSWorkspace.shared.open(url) } }), for: id)
            llmStatus = "Signed in."
        } catch {
            llmStatus = error.localizedDescription
        }
    }

    /// Jev's own sign-in: an OpenRouter key, without making OpenRouter the chat's model.
    func signInJev() async {
        guard let openRouter = LLMCatalog.provider(id: "openrouter") else { return }
        jevStatus = say("Waiting for the sign-in in the browser…", "Жду входа в браузере…")
        do {
            Keychain.save(try await openRouter.signIn(open: { url in Task { @MainActor in NSWorkspace.shared.open(url) } }), for: "openrouter")
            jevOpenRouter = true
            signedInCache.remove("openrouter")
            credentialVersion += 1
            jevStatus = say("Connected: a separate model now checks every command and plan too.", "Подключено: теперь каждую команду и план проверяет ещё и отдельная модель.")
            jevNote = nil
            planReview = nil
            reviewedIntent = nil
        } catch {
            jevStatus = error.localizedDescription
        }
    }
    /// How Jev's sign-in went, for Settings.
    var jevStatus: String?

    func useExistingLogin(_ id: String? = nil) {
        let id = id ?? providerID
        guard let login = provider(id).existingLogin() else { llmStatus = "No sign-in found on this Mac."; return }
        setCredential(login, for: id)
        llmStatus = "Connected."
    }

    func testModel() async {
        guard let client = llmClient() else { llmStatus = "Connect \(provider.info.name) first."; return }
        llmBusy = true
        defer { llmBusy = false }
        do {
            let started = Date()
            let reply = try await client.complete(LLMRequest(model: model, messages: [LLMMessage(.user, "Reply with one word: works")]))
            llmStatus = "\(model) answered in \(String(format: "%.1f", Date().timeIntervalSince(started))) s: \(reply.prefix(80))"
        } catch {
            llmStatus = error.localizedDescription
        }
    }

    /// A subscription's models as it lists them now (ChatGPT's Codex backend), for the chat's menu. API keys keep the
    /// checked list there: their /models lists everything, image and speech models too.
    /// Kept between launches, so the menu opens with the list it will show and never swaps rows under the pointer.
    var liveModels: [String: [String]] = UserDefaults.standard.dictionary(forKey: "liveModels") as? [String: [String]] ?? [:] {
        didSet { UserDefaults.standard.set(liveModels, forKey: "liveModels") }
    }

    func refreshLiveModels() async {
        let p = provider, id = p.info.id
        guard p.info.kind == .subscription, let list = try? await p.models(credential), !list.isEmpty else { return }
        if liveModels[id] != Array(list.prefix(4)) { liveModels[id] = Array(list.prefix(4)) }
    }

    /// The chat models downloaded in Ollama and LM Studio, for the menu: a listed one that isn't downloaded answers 404.
    /// A server that isn't running leaves its list as it was; embedding models can't chat and stay out.
    func refreshLocalModels() async {
        for p in LLMCatalog.all where p.info.kind == .local && p.info.unavailable == nil && p.info.id != "apple" {
            // Ollama's own list says how big each downloaded model is: what it takes in memory to run.
            if p.info.id == "ollama", let tags = await OllamaModels.downloaded() {
                for (name, gb) in tags { modelSizes[name] = gb }
                // An updated Ollama may take the model now.
                let version = await OllamaModels.version()
                if version != ollamaVersion { ollamaVersion = version; ollamaOutdated = false }
                if ollamaLatest == nil { ollamaLatest = await OllamaInstaller.latest() }
            }
            guard let list = try? await p.models(nil) else { localServers[p.info.id] = false; continue }
            localServers[p.info.id] = true
            let chat = list.filter { !$0.lowercased().contains("embed") }
            if liveModels[p.info.id] != chat { liveModels[p.info.id] = chat }
            // What a download would take, for the suggested Ollama models not there yet.
            if p.info.id == "ollama" {
                for m in p.info.models where !chat.contains(m) && modelSizes[m] == nil { modelSizes[m] = await OllamaModels.sizeGB(m) }
            }
        }
    }

    func loadModels(_ id: String? = nil) async {
        let p = provider(id ?? providerID)
        do {
            availableModels = try await p.models(credential(p.info.id))
        } catch {
            llmStatus = error.localizedDescription
        }
        // Not listed live (no key yet, a subscription): the models checked for it.
        if availableModels.isEmpty { availableModels = p.info.models }
        if availableModels.isEmpty { llmStatus = "This provider does not list its models; type the name." }
    }

    var market: Market? {
        var m = markets[intent.asset]
        m?.atrPct = atr[intent.asset]
        return m
    }
    /// Capital the risk rules measure against: USDC on Solana and the tokens on show there, the Hyperliquid account and vaults.
    var equity: Decimal? {
        guard solana != nil || hl != nil else { return nil }
        return (solana?.usdc ?? 0) + shownTokensUSD + (hl?.accountValue ?? 0) + lend.reduce(0) { $0 + $1.usdc } + dexBalances.values.reduce(0) { $0 + $1.accountValue }
            + (base.map { $0.usdc + $0.aave + $0.morpho } ?? 0) + lendingElsewhereUSD + (arbitrumUSDC ?? 0) + robinhoodTotal + kamino + hlp.equity + nearUSD + protocolUSD
    }
    /// The plan with what the user left to the rules filled in.
    var resolved: (intent: TradeIntent, notes: [String]) { planned().resolved }
    var evaluation: Evaluation { planned().evaluation }
    var blocks: [Block] {
        var live = Live()
        live.equity = equity
        live.policy = policy
        live.openRisk = HyperliquidAccount.riskAtStops(positions: positions, orders: orders)
        live.solanaAddressSet = isSolanaAddress(solanaAddress)
        live.solanaUSDC = solana?.usdc
        live.hlAddressSet = isEVMAddress(hlAddress)
        // A HIP-3 market can use what is free on its own exchange as well as the main balance.
        live.hlWithdrawable = hl.map { $0.withdrawable + (market?.dex.flatMap { dexBalances[$0]?.withdrawable } ?? 0) }
        live.marketsLoaded = !markets.isEmpty
        live.market = market
        live.quote = quote
        live.from = fundingNet.map { ($0.name, usdc(on: $0.key) ?? 0) }
        let key = BlocksKey(intent: intent, live: live)
        if let memo = blocksMemo, memo.key == key { return memo.blocks }
        let (resolvedIntent, notes) = resolved
        live.notes = notes
        let b = Plan.blocks(resolvedIntent, evaluation, live)
        blocksMemo = (key, b)
        return b
    }

    // The plan's numbers are asked for some ten times an update (the status, the buttons, the chat's actions), each a
    // whole resolve and evaluate, and Plan.fix runs both 60 times. Each keeps its last answer by its inputs: reading the
    // inputs to make the key still ties a view to them, so it redraws when they change.
    private struct PlanInputs: Equatable { let intent: TradeIntent, market: Market?, equity: Decimal?, policy: RiskPolicy }
    @ObservationIgnored private var planMemo: (inputs: PlanInputs, resolved: (intent: TradeIntent, notes: [String]), evaluation: Evaluation)?
    private func planned() -> (resolved: (intent: TradeIntent, notes: [String]), evaluation: Evaluation) {
        let inputs = PlanInputs(intent: intent, market: market, equity: equity, policy: policy)
        if let memo = planMemo, memo.inputs == inputs { return (memo.resolved, memo.evaluation) }
        let r = Plan.resolve(inputs.intent, market: inputs.market, equity: inputs.equity, policy: inputs.policy)
        let e = Plan.evaluate(r.intent, market: inputs.market)
        planMemo = (inputs, r, e)
        return (r, e)
    }

    /// Everything Plan.blocks reads but the notes, which follow from the intent and the live numbers.
    private struct BlocksKey: Equatable {
        let intent: TradeIntent, equity: Decimal?, policy: RiskPolicy, openRisk: Decimal?, solanaSet: Bool, solanaUSDC: Decimal?
        let hlSet: Bool, hlWithdrawable: Decimal?, marketsLoaded: Bool, market: Market?, quote: BridgeQuote?, from: String?, fromUSDC: Decimal?
        init(intent: TradeIntent, live: Live) {
            self.intent = intent
            equity = live.equity; policy = live.policy; openRisk = live.openRisk
            solanaSet = live.solanaAddressSet; solanaUSDC = live.solanaUSDC
            hlSet = live.hlAddressSet; hlWithdrawable = live.hlWithdrawable; marketsLoaded = live.marketsLoaded
            market = live.market; quote = live.quote; from = live.from?.name; fromUSDC = live.from?.usdc
        }
    }
    @ObservationIgnored private var blocksMemo: (key: BlocksKey, blocks: [Block])?

    /// Plan.fix's inputs: the plan's and what the account can still put in.
    struct FixKey: Equatable { let intent: TradeIntent, market: Market, equity: Decimal?, policy: RiskPolicy, openRisk: Decimal?, available: Decimal }
    @ObservationIgnored var fixMemo: (key: FixKey, fix: (command: String, sizing: Sizing)?)?

    // MARK: Chat

    /// The message with each address already saved in Addresses written as its name, when it asks to send: a saved
    /// address was confirmed on its card, so writing it is the same as writing the name. On several networks, the one the
    /// message names («по эфир сети») wins.
    func savedNames(in text: String) -> String {
        let found = InjectionHeuristics.findings(text)
        guard found.transferVerb else { return text }
        let want = AddressBookCommand.network(text.lowercased())
        return found.addresses.reduce(text) { out, address in
            let saved = addressBook.filter { address.hasPrefix("0x") ? $0.address.lowercased() == address.lowercased() : $0.address == address }
            guard let entry = saved.first(where: { $0.network.rawValue == want }) ?? saved.first else { return out }
            return out.replacingOccurrences(of: address, with: entry.name)
        }
    }

    /// Set while a Confirm or Sign button's own «do it» goes through `send`: only a press carries out a move waiting for
    /// Confirm. A typed «yes» never does — text on the canvas, a site or a pasted note can't agree for the user.
    @TaskLocal static var pressed = false

    /// The Confirm (or Sign) button: the move waiting on the card goes ahead, as «do it» in the chat.
    func pressConfirm() async {
        await Self.$pressed.withValue(true) { await send("do it") }
    }

    /// `shown`: the message as typed, when its «$» mentions became plain text (ChatComposer.send).
    /// `about`: what this message alone is about («What's going on here?» from a block's menu); the attached chips then
    /// stay for the next one.
    func send(_ raw: String, shown: String? = nil, about: [StepContext]? = nil) async {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !busy else { return }
        // A move the message asked for that was refused before its card: reasoned about once the message is through.
        defer { Task { await explainFailure() } }
        // Anything they write takes over from what the model was carrying out; the Confirm button carries it on.
        if !Self.pressed { agentGoal = nil }
        if SeedGuard.containsMnemonic(text) {
            reply(say("That looks like a recovery phrase. I don’t show it, store it or send it anywhere. Import a phrase in its own window.",
                      "Похоже на фразу восстановления. Я её не показываю, не храню и никуда не отправляю. Импортируй фразу в отдельном окне."))
            return
        }
        if SeedGuard.containsPrivateKey(text) {
            reply(say("That looks like a private key. I don’t show it, store it or send it anywhere. Import a key in its own window.",
                      "Похоже на приватный ключ. Я его не показываю, не храню и никуда не отправляю. Импортируй ключ в отдельном окне. Если это хеш транзакции, напиши его с «0x»."))
            return
        }
        // A step handed over from its card: the message is about it, and a move waiting for Confirm keeps waiting.
        // A block goes with what it shows right now; the chat keeps it over the message, the model's history its name.
        let step = (about ?? attached).map { c in c.about == nil ? StepContext(title: c.title, logo: c.logo, about: StepContext.block(c.title, shows: blockFacts(c.title))) : c }
        if about == nil, !attached.isEmpty { attached = [] }
        let heading = step.isEmpty ? "" : step.map { "«\($0.title)»" }.joined(separator: " ") + "\n"
        messages.append(Message(fromUser: true, text: heading + text, shown: step.isEmpty ? shown : shown ?? text, context: step.isEmpty ? nil : step))
        thinking = true
        draftReply = nil
        defer { thinking = false; draftReply = nil }
        heldAnswer = (yieldOptions, takeChoices)
        defer { heldAnswer = nil }
        // Cleared only when set: a tuple and NextSteps aren't Equatable, so even nil over nil redraws what reads them.
        if finishedAction != nil { finishedAction = nil }
        // The cards of the last answer go with any new message; the model still knows them («да», «второй»).
        let offered = liveSteps?.options ?? []
        if nextSteps != nil { nextSteps = nil }
        if noteCard?.canvas != canvasID { noteCard = nil }
        // Every message starts an activity; whatever is on the canvas when it's handled is frozen into it.
        beginActivity(.request, request: text)
        defer { snapshotCanvas(); saveCanvas() }

        // Before the parser or any model sees it: local rules, plus Jev if the user turned it on.
        // The classifier reads what may hold a paste, where an injection could hide: an address or a link, or a message
        // longer than anyone types a command (the longest of 262 typed ones was 85 characters, the shortest injected paste
        // 105; a pasted paragraph with no address in it used to go unread). Local rules read everything.
        // It is told the message is the user's own (JevClassifier.own), so their «не хочу, отмени», «save 0x… as test
        // wallet for ai agent» and «купи токен 0x2E8c… на 1 доллар» read as what they are: the three workarounds those
        // false alarms brought — never on a save, a redirect only noted — are gone.
        // An address already saved in Addresses is that entry, as if its name were written: «переведи 2 USDC на 0x5333…»
        // goes to «main wallet». Only a saved (confirmed) address: any other still never becomes a recipient.
        text = savedNames(in: text)
        // ponytail: «pasted» by its look, not by the pasteboard; a short paste with no address or link is read by the local rules only.
        let pasted = text.count >= 100 || text.contains(/0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|https?:\/\/|www\./)
        let classifier: (any TextClassifier)? = jevScreening && pasted ? jevKey.map { JevClassifier(apiKey: $0, own: true) } : nil
        busy = true
        let screen = await InjectionGuard.screen(text, source: "chat message", classifier: classifier)
        busy = false
        FileHandle.standardError.write(Data("ihatecrypto screen: ok=\(screen.ok) \(screen.checks + screen.problems)\n".utf8))
        guard screen.ok else {
            // The user's own «переведи 2 USDC на 0x…»: no attack, just the rule that money goes to saved addresses only.
            // The chat offers the save instead, ready in the field; the address becomes a recipient only on its card.
            let found = InjectionHeuristics.findings(text)
            if screen.problems.count == 1, found.commands.isEmpty, found.transferVerb, let address = found.recipients.first {
                let lower = text.lowercased()
                let net = AddressBookCommand.network(lower) ?? (address.hasPrefix("0x") ? "base" : "solana")
                suggest(say("save \(address) as NAME on \(net)", "сохрани \(address) как ИМЯ на \(net)"))
                reply(say("Money goes only to saved addresses, so a swapped address can't receive it. Save this one first: the phrase is in the field — put a name instead of NAME (and another network if it isn't \(net)), send it, and check the address on the card. Then say “send <amount> USDC to <name>”.",
                          "Деньги уходят только на сохранённые адреса — так подменённый адрес ничего не получит. Сначала сохрани этот: фраза уже в поле — впиши имя вместо ИМЯ (и другую сеть, если не \(net)), отправь и сверь адрес на карточке. Потом скажи «переведи <сумма> USDC <имя>»."),
                      note: "guard: sends go to saved addresses")
                return
            }
            let pastedAddress = text.contains(/0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}/)
            updateActivity { $0.stopped = true }
            reply(say("This looks like an attempt to steer the assistant or swap an address. ", "Похоже на попытку управлять ассистентом или подменить адрес. ")
                  + screen.problems.joined(separator: " ")
                  + say(" The message was not passed to the model and changed nothing.", " Сообщение не ушло модели и ничего не изменило.")
                  + (pastedAddress ? say(" To keep an address, save it in Addresses — the book button in the rail.", " Чтобы сохранить адрес, добавь его в адресную книгу — кнопка с книгой слева.") : ""),
                  note: classifier == nil ? Message.guardLocal : Message.guardBoth)
            return
        }
        let before = intent

        // A confirmation from the debug API would only fail at the signer and lose the card: keep it for the window.
        if SignerClient.untrusted, activeWallet?.isTest != true, pendingAction != nil || pendingAllocation != nil, ChatWords.confirms(text) {
            reply(say("That «do it» came from the debug API, which can’t confirm money moves. In the window, press Confirm on the card.",
                      "Это «do it» пришло из отладочного API, а он не подтверждает переводы. В окне нажми Confirm на карточке."), note: "debug API")
            return
        }
        // Only the button carries out a move: a typed «yes», «do it», «делай» leaves the card waiting.
        if ChatWords.typedApproval(text, pressed: Self.pressed, waiting: pendingAction != nil || pendingAllocation != nil) {
            reply(say("Press Confirm on the card to go ahead — a typed «yes» never moves money, so text from a site, a note or a pasted message can't confirm for you. The card keeps waiting.",
                      "Нажми Confirm на карточке — напечатанное «да» деньги не двигает, чтобы текст с сайта, из заметки или вставленного сообщения не мог подтвердить за тебя. Карточка ждёт."),
                  note: "Confirm is a button")
            return
        }

        // The button's «do it» right after a plan that waits for it carries it out; anything else drops it.
        let confirms = ChatWords.confirms(text)
        let declines = ChatWords.declines(text)
        // Read once per message: each read is a Keychain round trip and a JSON decode.
        let client = llmClient()
        // Anything but a confirmation, or the amount a step asked for, takes over from the steps that waited for it
        // (with a model, once it reads a command: a question about the card keeps them, see below). An amount in the
        // wrong unit counts too: it is asked again below, and the steps keep waiting for the answer.
        if !confirms, client == nil || declines, awaitingAmount == nil || WalletCommand.amountAnswer(text) == nil { taskQueue = nil }
        // With a model, a card waiting for Confirm stays while the model reads the message with it in view: a question
        // about it («это точно лучший маршрут?», «а комиссия какая?») is answered and the card keeps waiting; only a new
        // command or plan takes its place (routeWithModel). Before, any message dropped it first, and the model answered
        // «there's no plan on the canvas».
        let asksModel = !confirms && !declines && client != nil
        if let pending = pendingAllocation, !asksModel {
            pendingAllocation = nil
            if confirms { await move(pending, confirmed: true); return }
            if declines {
                allocationPreview = nil
                rebalancePlan = nil
                reply(say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada."))
                return
            }
        }
        if let pending = pendingAction, step.isEmpty || confirms || declines, !asksModel {
            pendingAction = nil
            pendingText = nil
            if declines {
                firstBuyOf = nil
                reply(say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada."))
                return
            }
            if confirms {
                await carryOut(pending)
                return
            }
        }
        // The answer to «How much …?»: a bare number fills the waiting command.
        if let waiting = awaitingAmount {
            if declines {
                awaitingAmount = nil
                reply(say("OK, not doing that.", "Хорошо, не делаю."))
                return
            }
            if let value = WalletCommand.amountAnswer(text, for: waiting) {
                awaitingAmount = nil
                await handleWallet(waiting.with(amount: value))
                return
            }
            // An amount in another unit («$5» to a SOL stake, «1 sol» to a USDC deposit) or under the coin's smallest one:
            // asked again in the unit the question is in, never read as that coin.
            if let value = WalletCommand.amountAnswer(text) {
                let unit = waiting.amountAsset, least = unit == "SOL" ? "0.000000001" : "0.000001", example = unit == "SOL" ? "0.01" : "1"
                reply(value < Decimal(string: least)!
                      ? say("That's less than the smallest \(unit) amount (\(least)). How much \(unit)? Write a number in \(unit), for example “\(example)”.",
                            "Это меньше самой малой суммы \(unit) (\(least)). Сколько \(unit)? Напиши число в \(unit), например «\(example)».")
                      : say("The question is in \(unit). How much \(unit)? Write a number in \(unit), for example “\(example)”.",
                            "Вопрос в \(unit). Сколько \(unit)? Напиши число в \(unit), например «\(example)»."))
                return
            }
            if ChatWords.confirms(text) {
                reply(say("How much? Just the number, for example “1”.", "Сколько? Просто число, например «1»."))
                return
            }
            awaitingAmount = nil
        }
        if declines {
            withAnimation(StudioView.spring) { allocationPreview = nil; rebalancePlan = nil; yieldOptions = nil; takeChoices = nil }
            reply(say("OK — nothing was waiting, nothing moved.", "Хорошо — ничего не ждало подтверждения, ничего не ушло."))
            return
        }
        // «да» with nothing waiting answers ihatecrypto's last question: the model reads it with the chat. Without a model
        // there is nothing to answer.
        if client == nil, ChatWords.confirms(text) {
            reply(say("Nothing is waiting for a confirmation right now. Tell me what to do — for example “deposit 1 USDC into Jupiter”.",
                      "Сейчас нечего подтверждать. Скажи, что сделать — например «положи 1 USDC в Jupiter»."))
            return
        }
        takeChoices = nil
        yieldOptions = nil
        if pendingAllocation == nil { allocationPreview = nil; rebalancePlan = nil }
        if TakeStyle.asksForChoice(text) {
            if intent == TradeIntent() {
                reply(say("Describe the trade first — then I’ll show the ways to take profit on it.",
                          "Сначала опиши сделку — потом покажу, как на ней взять прибыль."))
            } else {
                takeChoices = TakeStyle.allCases
                reply(say("Three ways to take profit are on the canvas. The stop stays the same; pick one or say your own, e.g. “take 3R”.",
                          "На холсте три способа взять прибыль. Стоп остаётся прежним; выбери один или скажи свой, например «тейк 3R»."))
            }
            return
        }
        let lower = text.lowercased()
        if lower.wholeMatch(of: /\s*(?:new\s+canvas|новый\s+(?:канвас|холст)|start\s+over|начн[иём]\s+заново)\s*[.!]?\s*/) != nil {
            messages.removeLast()   // the command itself doesn't belong to the canvas being closed
            newCanvas()
            return
        }
        if lower.wholeMatch(of: /\s*(?:clear|reset|remove|drop)\s+(?:the\s+)?plan|(?:очисти|сбрось|убери|удали)\s+план\w*\s*[.!]?\s*/) != nil {
            clearPlan()
            return
        }
        if text.lowercased().wholeMatch(of: /\s*(?:lock|lock\s+(?:it|the\s+wallet|signing)|заблокируй\w*|блокир\w*)\s*[.!]?\s*/) != nil {
            await lockSigner()
            return
        }
        // «open jup.ag», «открой сайт raydium.io»: the site with the wallet, in the panel beside the chat (WebPanel.swift).
        if let m = text.wholeMatch(of: /\s*(?:open|go\s+to|открой|зайди\s+на|перейди\s+на)\s+(?:the\s+site\s+|site\s+|сайт\s+)?((?:https?:\/\/)?[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:\/\S*)?)\s*/.ignoresCase()) {
            let site = String(m.1)
            openSite(site)
            reply(say("\(site) is open beside the chat with your wallet: the site asks to connect first, and every signature shows here before it's made.",
                      "\(site) открыт рядом с чатом, с кошельком: сайт сначала попросит подключиться, и каждую подпись ты увидишь до того, как она сделана."))
            return
        }
        // «what's new»: the versions by date and their stories, over the window (WhatsNew.swift).
        if lower.wholeMatch(of: /\s*(?:what'?s\s+new(?:\s+in\s+(?:the\s+)?(?:app|ihatecrypto))?|что\s+нового\s+в\s+(?:приложении|ihatecrypto|апп?\w*))\s*[.!?]?\s*/) != nil {
            WhatsNew.shared.open = true
            reply(say("What's new is on screen; it's also in the Profile menu.", "Что нового — на экране; оно же в меню профиля."))
            return
        }
        // «check for updates», «обнови приложение»: Sparkle's window (Updater.swift), in the copy from the site.
        if lower.wholeMatch(of: /\s*(?:check\s+for\s+updates?|update\s+(?:the\s+)?app|any\s+updates\??|(?:проверь|есть|нет\s+ли)\s+обновлени\w*|обнови(?:сь|\s+(?:приложение|ihatecrypto|апп?ку?)))\s*[.!?]?\s*/) != nil {
            if Updater.shared.available {
                Updater.shared.check()
                reply(say("Checking for a new version: Sparkle's window shows what it finds.", "Проверяю новую версию: окно Sparkle покажет, что нашлось."))
            } else {
                reply(say("This copy was built on this Mac, so it doesn't look for updates. The copy from the site looks once a day.",
                          "Эта копия собрана на этом Маке, поэтому обновлений не ищет. Копия с сайта проверяет их раз в день."))
            }
            return
        }
        // «connect ledger», «use my Ledger»: an exact app command, read before the model like a wallet switch.
        if await ledgerCommand(text) { return }
        // «переключись на кошелёк Main»: the wallet by its name; the signer asks for Touch ID.
        if let target = walletSwitch(text) {
            if let target { await switchWallet(target) } else {
                let yours = wallets.map { "«\($0.label)»" }.joined(separator: ", ")
                reply(say("No wallet has that name. Yours: \(yours).", "Кошелька с таким именем нет. Твои: \(yours)."))
            }
            return
        }
        // «turn on the trading key»: one that was never approved is approved now (Hyperliquid has to be open for it); a paused
        // one is resumed.
        if text.lowercased().contains(/(?:resume|unpause|turn\s+on|enable|approve|включи|верни|одобри)\s+(?:the\s+)?(?:hyperliquid\s+)?(?:trading\s+key|торгов\w*\s+ключ)/) {
            if agentAddress == nil { await enableAgent() } else { await resumeTradingKey() }
            return
        }
        // A connected model reads every message first: its meaning, not keywords. What it understood goes through
        // the same parsers, checks, Jev and confirmations as a typed command. The local parsers below read the message
        // as typed when no model is connected or it can't be reached.
        if let client {
            await routeWithModel(step.isEmpty ? text : step.compactMap(\.about).joined(separator: "; ") + ": " + text,
                                 client: client, before: before, offered: offered, typed: step.isEmpty ? nil : text)
            return
        }
        // No model connected: the local parsers read the message as typed.
        if ScheduledTask.cancelsAll(text) { cancelTasks(); return }
        if let task = ScheduledTask.parse(text) { schedule(task); return }
        if case .pay? = WalletCommand.parse(text), let command = WalletCommand.parse(text) { await handleWallet(command); return }
        if let command = TokenCommand.parse(text) {
            await tokenCommand(command)
            return
        }
        if let command = FolderCommand.parse(text) { folderCommand(command); return }
        if let command = AddressBookCommand.parse(text) {
            await addressBookCommand(command)
            return
        }
        if let command = AlertCommand.parse(text, markets: Set(markets.keys)) {
            alertCommand(command)
            return
        }
        if let command = ChartCommand.parse(text, markets: Set(markets.keys)) {
            await chartCommand(command)
            return
        }
        if let command = PnLCommand.parse(text, markets: pnlNames) { await showPnL(command); return }
        if case .sendTo(let name, let amount, let net)? = WalletCommand.parse(text) {
            prepareTransfer(amount, to: name, network: net)
            return
        }
        if let command = WalletCommand.parse(text) {
            await handleWallet(command)
            return
        }
        // With a position or a waiting limit, «стоп в безубыток», «закрой половину» act on it, not on the plan.
        if !positions.isEmpty || pendingEntry != nil, let command = PositionCommand.parse(text) {
            if let coin = targetCoin(for: text) {
                switch command {
                case .close, .cancelOrders:
                    // A close or a cancel can't be taken back: it waits for Confirm, as when a model reads it.
                    pendingAction = .position(command, coin: coin)
                    pendingText = text
                    reply(say("“\(text)” for \(IntentParser.pretty(coin)): press Confirm to go ahead.",
                              "«\(text)» для \(IntentParser.pretty(coin)): нажми Confirm."))
                default:
                    await manage(command, coin: coin)
                }
            } else {
                let open = positions.map { "\($0.size > 0 ? "long" : "short") \(IntentParser.pretty($0.coin))" }.joined(separator: ", ")
                let example = "\(text) \(positions.first.map { IntentParser.pretty($0.coin).components(separatedBy: " (").first ?? $0.coin } ?? "")"
                reply(say("Which position? Open: \(open). Name it, e.g. “\(example)”.", "Какую позицию? Открыты: \(open). Назови её, например «\(example)»."))
            }
            return
        }

        // A question gets an answer from the portfolio and the history; it never becomes a plan.
        if IntentParser.isQuestion(text) {
            await answer(text)
            return
        }

        // With no plan on the canvas, only a market or a direction starts one: «1 USDC» alone is not «long ETH».
        let startsPlan = intent != TradeIntent() || IntentParser.startsPlan(text, markets: Set(markets.keys))
        let parsed = startsPlan ? IntentParser.apply(text, to: intent, markets: Set(markets.keys)).map(Self.started) : nil
        let confident = parsed != nil && (client == nil || (!IntentParser.needsModel(text) && IntentParser.hasTradeWords(text)))
        if let next = parsed, confident {
            if await editExecuted(from: before, to: next) { return }
            intent = next
            let changes = next.changes(from: before)
            await refreshQuote()
            reply(planUpdated(changes) + outcomeLine)
        } else if let client {
            // No plan and nothing about trading: the plan editor would only invent one.
            guard startsPlan || IntentParser.hasTradeWords(text) else {
                reply(Self.help)
                return
            }
            await editPlanWithModel(text, client: client, before: before)
            return
        } else {
            reply(say("I didn’t get that. No model is connected, so I read commands, not free talk: “long ETH risk $5”, “stop 3%”, “take 6%”, “leverage 2x”, “send 5 USDC to Bybit”, “deposit 1 USDC into Jupiter” — in English or Russian. With or without a model, nothing moves until you press Confirm and \(Presence.touchID ? "touch Touch ID" : "type your Mac password"). For free wording in any language, connect a model in Settings › Sign-ins (⌘,): a ChatGPT or Claude subscription, OpenRouter or an API key.",
                      "Не понял. Модель не подключена, поэтому я понимаю команды, а не свободный разговор: «лонг ETH риск $5», «стоп 3%», «тейк 6%», «плечо 2x», «отправь 5 USDC на Bybit», «положи 1 USDC в Jupiter» — по-русски и по-английски. С моделью или без, ничего не выполняется, пока ты не нажмёшь Confirm и \(Presence.touchID ? "не приложишь Touch ID" : "не введёшь пароль от Mac"). Чтобы писать свободно на любом языке, подключи модель в Settings › Sign-ins (⌘,): подписку ChatGPT или Claude, OpenRouter или API-ключ."))
        }
        await refreshQuote()
        startPlanReview()
    }

    /// A move the user confirmed, carried out.
    func carryOut(_ pending: PendingAction) async {
        switch pending {
        case .wallet(let command):
            if let s = firstBuyOf, isFirstBuy(pending) {
                schedules.removeAll { $0.symbol == s.symbol }
                schedules.append(s)
            }
            firstBuyOf = nil
            await move(command)
        case .position(let command, let coin): await manage(command, coin: coin)
        case .orders(let commands, let coin, let next):
            for command in commands { await manage(command, coin: coin) }
            intent = next
            executed = next
        case .transfer(let entry, let amount): await transfer(amount, to: entry)
        case .address(let entry):
            // Touch ID with the whole address on the prompt; only then is it in the book.
            let bubble = Self.saveStep(entry)
            do {
                try await confirmAddresses([entry])
                addressBook.append(entry)
                // Its card stays on the canvas, as Done.
                finishedAction = ("Save “\(entry.name)” to Addresses", CanvasOutcome(at: Date(), ok: true), [bubble])
                reply(say("Saved “\(entry.name)” in Addresses (\(entry.network.title)), confirmed with \(Presence.name). Star it there or say “add \(entry.name) to favorites”.",
                          "«\(entry.name)» сохранён в адресной книге (\(entry.network.title)), подтверждено \(Presence.ruIns). Отметь его звёздочкой там или скажи «добавь \(entry.name) в избранное»."))
            } catch {
                finishedAction = ("Save “\(entry.name)” to Addresses", CanvasOutcome(at: Date(), ok: false), [bubble])
                reply(say("Not saved: ", "Не сохранено: ") + error.localizedDescription)
            }
        }
    }

    /// Which position a position command is about. Never a guess: the asset the message names, else the plan's asset
    /// if it has a position, else the only position when no plan is on screen. nil means ask.
    func targetCoin(for text: String) -> String? {
        let open = Set(positions.map(\.coin) + (pendingEntry.map { [$0.coin] } ?? []))
        if let named = IntentParser.namedAsset(text, markets: Set(markets.keys)) { return open.contains(named) ? named : nil }
        if intent != TradeIntent(), open.contains(intent.asset) { return intent.asset }
        if intent == TradeIntent(), open.count == 1 { return open.first }
        return nil
    }

    /// A take-profit style picked on the canvas.
    func chooseTake(_ style: TakeStyle) async {
        let before = intent
        intent = style.apply(to: intent)
        takeChoices = nil
        let changes = intent.changes(from: before)
        await refreshQuote()
        reply(say("Take: ", "Тейк: ") + style.title.lowercased() + (changes.isEmpty ? "." : " — " + changes.joined(separator: ", ") + ".") + outcomeLine)
        startPlanReview()
    }

    nonisolated static let routerLog = Logger(subsystem: "app.ihatecrypto.mac", category: "router")

    /// The model can't be reached: what reads the same without it still shows — a token, a chart, P/L. Anything that could
    /// move money waits for a model: a keyword guess about money is worse than a question.
    private func readWithoutModel(_ text: String) async -> Bool {
        if let command = TokenCommand.parse(text) { await tokenCommand(command); return true }
        if let command = ChartCommand.parse(text, markets: Set(markets.keys)) { await chartCommand(command); return true }
        if let command = PnLCommand.parse(text, markets: pnlNames) { await showPnL(command); return true }
        // «покажи чарт spx6900»: a Solana token by its ticker or name, Jupiter-verified only.
        if let word = text.lowercased().firstMatch(of: /(?:charts?|график\w*|чарт\w*|show|покажи)\s+(?:(?:the|token|токен\w*|чарт\w*|график\w*)\s+)*\$?([a-z][a-z0-9]{1,15})\b/)?.1,
           let mint = await TokenResearch.solanaMint(symbol: String(word)) {
            await tokenCommand(.show(address: mint, chain: "solana"))
            return true
        }
        return false
    }

    /// The model's reading of the message (ChatRouter) — the only reading while a model is connected: keywords guess,
    /// and a wrong guess about money is worse than a question. The model is asked twice; if it still can't be reached,
    /// nothing is done and the chat says so.
    /// `said`: the user's own words when `text` is ihatecrypto telling the model how its last step went (`continueAgent`);
    /// addresses still count only from them. `round`: how many steps of their request the model has taken.
    /// `typed`: the user's words alone, when what they attached goes before them in `text`.
    func routeWithModel(_ text: String, client: any LLMClient, before: TradeIntent, offered: [ChatRouter.Option] = [], typed: String? = nil,
                        said: String? = nil, round: Int = 0) async {
        busy = true
        stopRequested = false
        var reading: ChatRouter.Reading?
        var failure = "", unreadable = false
        let context = routerContext(offered: offered, messageShown: said == nil)
        // The model's lookups run here, on this canvas's data; the chat shows which one is under way.
        let lookup: @Sendable (ChatRouter.Lookup) async -> String = { [weak self] l in await self?.screenedLookup(l) ?? "" }
        // A second reader of every command: Jev when OpenRouter is connected, else the model reading it again.
        let key = jevScreening ? jevKey : nil, recent = context.recent, model = model, known = Set(markets.keys)
        @Sendable func modelReading(_ command: String) async -> (answered: Bool, why: String?) {
            do { return (true, try await ChatRouter.verify(text, command: command, context: context, client: client, model: model, markets: known)) } catch { return (false, nil) }
        }
        // What a follow-up is about: the plan, the blocks with a card waiting for Confirm among them, the cards offered.
        let seen = (context.plan.map { ["plan on the canvas: " + $0] } ?? []) + context.canvas
            + (context.offered.isEmpty ? [] : ["cards offered after the last reply: " + context.offered.joined(separator: "; ")])
        @Sendable func classifierReading(_ command: String) async -> JevReview? {
            guard let key else { return nil }
            return try? await JevReviewer.reading(message: text, recent: recent, canvas: seen, command: command, forms: ChatRouter.forms(for: command), apiKey: key)
        }
        // Both at once. The model's reading decides and says what differs. The OpenRouter classifier used to raise false
        // alarms on right commands with no reason given, so it counted only when the model couldn't answer; since
        // 2026-10-13 it is told what the forms mean and names what differs (JevReviewer.reading: no false alarm on 284
        // right commands), so it also stops a command the model passed when it is strongly against it. Neither answering
        // is no agreement: the command or the «yes» waits for the user.
        let unchecked = say("it couldn't run this time (no answer to it).", "в этот раз она не прошла — ответа не было.")
        @Sendable func secondReading(_ command: String) async -> String? {
            async let byModel = modelReading(command)
            async let other = classifierReading(command)
            let (m, review) = await (byModel, other)
            if let review { FileHandle.standardError.write(Data("ihatecrypto second check: «\(command)» \(review.label) \(review.confidence) model=\(m.answered ? (m.why ?? "ok") : "no answer")\n".utf8)) }
            if m.answered { return m.why ?? review.flatMap { $0.concern && $0.confidence >= JevReviewer.strongDoubt ? $0.text : nil } }
            guard let review else { return unchecked }
            return review.concern ? review.text : nil
        }
        var check: (@Sendable (String) async -> String?)?
        if jevScreening { check = secondReading }
        let routeMarkets = markets.isEmpty ? nil : Set(markets.keys), checker = check
        for _ in 0..<2 where reading == nil && !stopRequested {
            do {
                // Before the markets load a trade can't be checked: the model's command goes through as it is.
                reading = try await modelCall {
                    try await ChatRouter.route(text, context: context, client: client, model: model, markets: routeMarkets, lookup: lookup, check: checker,
                                               draft: { [weak self] words in Task { @MainActor in self?.showDraft(words) } })
                }
            } catch {
                if stopRequested { break }
                failure = error.localizedDescription
                unreadable = error is ChatRouter.Unreadable
                FileHandle.standardError.write(Data("ihatecrypto router: \(error)\n".utf8))
                // In the system log too, where an app started from the Dock has no stderr: what a model answered that
                // couldn't be read (`log show --predicate 'subsystem == "app.ihatecrypto.mac" AND category == "router"'`).
                Self.routerLog.error("\(String(describing: error), privacy: .public)")
            }
        }
        busy = false
        agentStep = nil
        // Stop: the model was cut off; nothing it would have led to runs.
        if stopRequested {
            draftReply = nil
            reply(say("Stopped — nothing ran. Write what you'd like instead.", "Остановил — ничего не запускал. Напиши, что сделать вместо этого."), note: model)
            return
        }
        if let reading, !reading.lookups.isEmpty {
            updateActivity { $0.understood = "looked up " + reading.lookups.map { "\($0.tool) \($0.arg)" }.joined(separator: ", ") }
        }
        // The block the answer points to opens with it; the cards follow the reply.
        if let reading { await show(reading, userText: said ?? text) }
        defer { if let reading { offer(reading, userText: said ?? text) } }
        var note = reading.map { $0.lookups.isEmpty ? model : "\(model) · looked at \(Set($0.lookups.map(\.tool)).sorted().joined(separator: ", "))" } ?? model
        if let why = reading?.corrected, why != unchecked { note += " · corrected after a second reading" }
        // The second reader still disagrees after the model tried again, or couldn't be read: nothing runs, the user decides.
        if var r = reading, case .command(let c) = r.route, let doubt = r.doubt {
            r.options = [ChatRouter.Option(title: c, comment: doubt, command: c, recommended: true)]
            reading = r
            reply(say("I read that as “\(c)”, but a second reading doesn't confirm it: \(doubt) Nothing ran. If that’s what you meant, pick the card; otherwise say it another way.",
                      "Я понял это как «\(c)», но вторая проверка этого не подтверждает: \(doubt) Я ничего не запускал. Если ты это и имел в виду — выбери карточку, иначе скажи по-другому."), note: note)
            return
        }
        // The model read an approval, but the second reading isn't sure: nothing is signed, the card keeps waiting.
        if reading?.route == .confirm, let doubt = reading?.doubt {
            reply(say("That reads like a yes, but the second check isn't sure: \(doubt) Nothing was signed. Press Confirm on the card if you meant it.",
                      "Похоже на «да», но вторая проверка не уверена: \(doubt) Я ничего не подписывал. Если ты это и имел в виду — нажми Confirm на карточке."), note: note)
            return
        }
        switch reading?.route {
        case .confirm?:
            // The model read an approval: only the button carries out a move, so the card keeps waiting for it. A number or
            // a change in the message («add another 300», which Apple's model once read as a yes) is said to be one.
            let said = typed ?? text
            if ChatWords.mayChange(said) {
                reply(say("That reads like a yes, but it has a number or a change in it, so nothing was signed and the card keeps waiting. If it's a change, say it as one («make it 800»); if not, press Confirm on the card.",
                          "Похоже на «да», но в сообщении есть число или изменение — я ничего не подписывал, карточка ждёт. Если это изменение, скажи его так («сделай 800»); если нет — нажми Confirm на карточке."), note: note)
            } else if pendingAction != nil || pendingAllocation != nil {
                reply(say("Press Confirm on the card to go ahead — a typed «yes» never moves money. The card keeps waiting.",
                          "Нажми Confirm на карточке — напечатанное «да» деньги не двигает. Карточка ждёт."), note: note)
            } else {
                reply(say("Nothing is waiting for a confirmation right now.", "Сейчас нечего подтверждать."), note: note)
            }
        case .cancel?:
            let waited = pendingAction != nil || pendingAllocation != nil
            dropWaiting()
            withAnimation(StudioView.spring) { allocationPreview = nil; rebalancePlan = nil }
            reply(waited ? say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada.")
                         : say("OK — nothing was waiting, nothing moved.", "Хорошо — ничего не ждало подтверждения, ничего не ушло."), note: note)
        case nil where unreadable:
            reply(say("\(model) answered, but not in a form ihatecrypto can read, so nothing ran. Say it another way, or pick another model above.",
                      "\(model) ответил, но не в том виде, который ihatecrypto может прочитать, поэтому ничего не запускал. Скажи иначе или выбери другую модель сверху."), note: model)
        case nil where LLMCatalog.chatRefusal(model: model) != nil:
            reply((LLMCatalog.chatRefusal(model: model) ?? "") + say(" Nothing ran. Pick another model above.", " Ничего не запускал. Выбери другую модель сверху."), note: model)
        case nil:
            // A model on this Mac that isn't downloaded answers 404 «model … not found»: say how to get it.
            let missing = provider.info.kind == .local && failure.lowercased().contains("not found")
            let why = missing
                ? (provider.info.id == "ollama" ? say("it isn't downloaded — run «ollama pull \(model)» in Terminal", "она не скачана — выполни в Терминале «ollama pull \(model)»")
                                                 : say("it isn't downloaded in \(provider.info.name)", "она не скачана в \(provider.info.name)"))
                : String(failure.prefix(140))
            // What reads the same without a model — a chart, a token, P/L — still shows; nothing that could move money.
            if typed == nil, said == nil, await readWithoutModel(text) {
                reply(say("\(model) isn't answering (\(why)), so I read this one myself. Pick another model above for the rest.",
                          "\(model) не отвечает (\(why)), поэтому это я прочитал сам. Для остального выбери другую модель сверху."), note: model)
            } else {
                reply(say("I couldn’t reach \(model) (\(why)), so I didn’t act on this — nothing moved. Send it again, or pick another model above.",
                          "Не достучался до \(model) (\(why)), поэтому ничего не сделал — ничего не ушло. Отправь ещё раз или выбери другую модель сверху."), note: model)
            }
        case .question?: await answer(text)
        case .plan?:
            dropWaiting()
            // With both perps and xyz markets off there's no trade to build; otherwise Execute checks the market.
            if let off = Modules.current.refusal(trade: false, bridges: false, ru: modulesRussian),
               Modules.current.refusal(trade: true, bridges: false) != nil {
                reply(off, note: model)
            } else {
                await editPlanWithModel(text, client: client, before: before)
                if let off = tradeRefusal { reply(off, note: model) }
            }
        case .chat(let r)? where reading?.quiet == true:
            // A scheduled check with nothing to tell: said on its own canvas, no notification (runDueTask).
            checkQuiet = true
            reply(say("Checked: nothing to tell you now.", "Проверил: сейчас сообщать нечего.") + (r.isEmpty ? "" : " " + r), note: note)
        case .chat(let r)?, .clarify(let r)?: reply(r, note: note)
        case .command(let canonical)?:
            // A new command takes the place of the card that waited; a chart or the rates beside it don't.
            if !Self.displays(canonical) { dropWaiting(keepAction: typed != nil) }
            guard ChatRouter.addressesAllowed(canonical, userText: said ?? text, known: knownContracts, found: reading?.found ?? [],
                                              displays: Self.foundAllowed, markets: Set(markets.keys)) else {
                reply(say("I read that as “\(canonical)”, but its address is neither in your message nor among your favorites, so I won’t use it. Paste the address itself.",
                          "Я понял это как «\(canonical)», но этого адреса нет ни в твоём сообщении, ни в избранном, поэтому я его не использую. Вставь сам адрес."), note: model)
                return
            }
            let mark = messages.count
            if await !runCanonical(canonical, userText: said ?? text, before: before) {
                reply(say("I read that as “\(canonical)”, but ihatecrypto can’t do that yet — nothing moved. Tell me another way, or ask what ihatecrypto can do.",
                          "Я понял это как «\(canonical)», но ihatecrypto пока так не умеет — ничего не ушло. Скажи иначе или спроси, что ihatecrypto умеет."), note: model)
            } else if reading?.continues == true {
                // Their request goes on after this step: the model hears how it went now, or after the Confirm when it moves money.
                agentGoal = AgentGoal(request: said ?? text, command: canonical, mark: mark, canvas: canvasID, since: Date(), round: round, next: reading?.next)
                if let next = reading?.next { reply(say("Next, once this is through: ", "Дальше, когда это пройдёт: ") + next, note: model) }
                Task { await continueAgent() }
            }
        }
    }

    /// The card waiting for Confirm, carried out because the model read the message as approving it — the same path as
    /// a typed «да». Never from the debug API (it can't confirm money moves), and nothing when no card waits.
    /// A new command or plan read from the message takes the place of the card that waited for Confirm, and of the
    /// chain's steps after it. A question about the card (chat, clarify) leaves them. Asked about one route step
    /// («Why?» on its card), the move keeps waiting as before.
    private func dropWaiting(keepAction: Bool = false) {
        if pendingAllocation != nil { pendingAllocation = nil; allocationPreview = nil; rebalancePlan = nil }
        if !keepAction { pendingAction = nil; pendingText = nil; firstBuyOf = nil }
        taskQueue = nil
    }

    /// What the model may know besides the message: the chat, the plan, positions, favorites, address book names and
    /// the main balances. Never keys or the user's own addresses.
    /// What the model is given besides the message, for the debug API's /context.
    var debugContext: ChatRouter.Context { routerContext(messageShown: false) }

    /// A block's own lines of what the model reads about the canvas (routerContext), found by its BlockMenu name; nil when
    /// that has nothing of it, and only its name goes.
    func blockFacts(_ title: String) -> String? {
        let c = routerContext()
        let parts = title.components(separatedBy: " · "), name = parts.count > 1 ? parts[1] : ""
        func lines(_ prefix: String) -> String? {
            let found = c.canvas.filter { $0.hasPrefix(prefix) }
            return found.isEmpty ? nil : found.joined(separator: "; ")
        }
        switch parts[0] {
        case "Portfolio": return ([c.capital.map { "total \($0)" }].compactMap { $0 } + c.balances).joined(separator: "; ")
        case "Chart": return lines("chart ")
        case "Position": return c.positions.first { $0.contains(" \(name) ") }
        case "Token": return lines("token card: ")
        case "Profit and loss": return lines("P/L board")
        case "Movers": return lines("movers board")
        case "History": return lines("history board")
        case "Schedule": return lines("schedule: ")
        case "Price alerts": return lines("price alert")
        case "Rates": return lines("rates board")
        case "Top up": return lines("top-up board")
        case "Take-profit choices": return lines("take-profit choices")
        case "Move under way": return lines("running now")
        case "Jupiter DCA": return lines("Jupiter DCA board")
        case "Spread plan", "Rebalance", "Withdrawal", "Position move", "New address": return lines("waiting for Confirm")
        case "Plan": return c.plan.map { "the trade plan \($0)" }
        case "Notes": return c.canvas.first { $0.contains(" in folder «") }
        case "Favorites": return c.favorites.isEmpty ? nil : "favorite tokens: " + c.favorites.joined(separator: "; ")
        case "Recurring buy":
            return schedules.first { $0.symbol == name }.map { "buys \(Fmt.num($0.usdc)) USDC of \($0.symbol) every \($0.days) days, next \(Fmt.date($0.next))" + ($0.paused == true ? ", paused" : "") }
        case "Ledger": return ledgerActive ? "a Ledger signs for the active wallet" : ledgerConnect != nil ? "a Ledger is being connected" : "no Ledger connected"
        default: break
        }
        if let key = currentScheduleKey, scheduleEntries.first(where: { $0.id == key })?.title == title { return scheduledFacts(key) }
        if let a = serviceAnswer, title == "\(a.service)'s answer" { return lines("\(a.service)'s answer") }
        if let b = savedBoard ?? finishedCanvas, readable(b.title) == title {
            return ([readable(b.title)] + b.bubbles.map { "\($0.label) \(readable($0.title))" + ($0.lines.isEmpty ? "" : " (\($0.lines.joined(separator: ", ")))") })
                .joined(separator: " → ") + (b.outcome.map { $0.ok ? " — done" : " — failed" } ?? "")
        }
        if let place = detail, placeTitle(place) == title {
            let held = c.balances.filter { $0.localizedCaseInsensitiveContains(title) }
            let facts = [lines("details of")].compactMap { $0 } + held
            return facts.isEmpty ? nil : facts.joined(separator: "; ")
        }
        return nil
    }

    private func routerContext(offered: [ChatRouter.Option] = [], messageShown: Bool = true) -> ChatRouter.Context {
        var c = ChatRouter.Context()
        c.recent = Array(recentChat().dropLast(messageShown ? 1 : 0))   // the last line is the message itself
        c.offered = offered.map { "\($0.title) → \($0.command.isEmpty ? "(an answer to your question)" : $0.command)\($0.recommended ? " (recommended)" : "")" }
        if intent != TradeIntent(), let json = try? JSONEncoder().encode(intent) { c.plan = String(decoding: json, as: UTF8.self) }
        if let ch = chart {
            let points = ch.candles.map { TokenMarket.Point(time: $0.time, close: $0.close) }
            // Old candles say so, with the live price beside them: the model took a 4-day-old chart for today's.
            let live = markets[ch.coin].map { ", live price now \(Fmt.price($0.markPx))" } ?? ""
            let label = "chart \(ch.coin)" + (ch.stale && !ch.candles.isEmpty ? " (candles as of \(ch.at.map(Fmt.date) ?? "an earlier load")\(live))" : "")
            c.canvas.append(TokenResearch.chartFacts(points, range: ch.range, of: label) ?? "chart \(ch.coin) (\(ch.range.title))")
        }
        // Notes: «добавь заметку …» there goes to their folder without asking which.
        if let n = noteCard, let folder = n.folder {
            c.canvas.append("\(n.canvas == canvasID ? "this canvas is" : "a card opens") \(n.texts.count == 1 ? "a note" : "\(n.texts.count) notes") in folder «\(folder)»: "
                            + n.texts.map { "«\($0.prefix(400))»" }.joined(separator: ", "))
        }
        c.canvas += canvasBlocks()
        // The rates board, best first: «ок, положи туда 500» after «где лучше держать доллары?» is its top place.
        if let y = yieldOptions ?? heldAnswer?.yields, !y.isEmpty {
            c.canvas.append("rates board, best first: " + y.prefix(8).map { "\($0.place) \($0.asset) \(Fmt.num($0.apy.rounded(2, .plain)))%" }.joined(separator: ", "))
        }
        // With the route as its card draws it (amounts, fees, where the money comes from), so «а комиссия какая?» is
        // answered from the card's numbers.
        if let p = pendingText {
            var steps: [ActivityBubble] = []
            if case .wallet(let command)? = pendingAction { steps = route(command)?.steps ?? [] }
            c.canvas.append("waiting for Confirm: «\(p)»" + (steps.isEmpty ? "" : "; its route (not part of the command): "
                + steps.map { "\($0.label) \(readable($0.title))" + ($0.lines.isEmpty ? "" : " (\($0.lines.joined(separator: ", ")))") }.joined(separator: " → ")))
        }
        // A spread or rebalance on the canvas has no pendingText: without this the model answered «there's no plan on the canvas».
        if pendingText == nil, let p = pendingAllocation {
            // Its whole math, so «это точно лучший маршрут?» and «а почему не Morpho?» are answered from it, not guessed.
            c.canvas.append("waiting for Confirm: «\(p.summary)»" + (allocationPreview.map { a in
                "; its plan (not part of the command): " + a.slices.map { "\($0.place) \(Fmt.num($0.amount)) \(a.asset) at \(Fmt.num($0.apy.rounded(2, .plain)))%" }.joined(separator: ", ")
                    + ", taking " + (a.drawn.isEmpty ? "Solana's" : a.drawn.map { "\(Fmt.num($0.amount)) from \($0.place)" }.joined(separator: " + "))
                    + (a.moves.isEmpty ? "" : " (moves first, straight: " + a.moves.map { "\(Fmt.num($0.amount)) \($0.from) → \($0.to)" }.joined(separator: ", ") + ")")
                    + "; at most \(Fmt.pct(a.maxShare)) per protocol; ≈ \(Fmt.num(a.yearly)) \(a.asset) a year"
                    + (a.leftover > 0 ? "; left free \(Fmt.num(a.leftover))" : "")
                    + (a.skipped.isEmpty ? "" : "; skipped: " + a.skipped.map { "\($0.place) \(Fmt.num($0.apy.rounded(2, .plain)))% (\($0.reason))" }.joined(separator: "; "))
            } ?? ""))
        }
        c.canvas += alerts.map { a in
            "price alert: \(a.coin) \(a.above ? "above" : "below") \(Fmt.price(a.price))" + (a.steps.map { ", then " + $0.joined(separator: " then ") } ?? "")
        }
        // As its board shows it: size, entry, stop, take and the P/L now.
        c.positions = positions.map { "\($0.size > 0 ? "long" : "short") \(IntentParser.pretty($0.coin)) \(positionSummary($0)), unrealized \(signed($0.unrealizedPnl))" }
        c.favorites = favoriteTokens.map { "\($0.symbol) (\($0.name)) = \($0.address) on \($0.chainTitle)" }
        // With its network, so «по эфир сети» to a Base entry is answered with Base, not a guess.
        c.addressBook = addressBook.map { "\($0.name) (\($0.network.title))" }
        c.replyLanguage = replyLanguage
        c.memory = agentMemory
        let skills = SkillStore.shared.on
        c.skills = skills.map { "\($0.id) — \($0.name): \($0.about)" }
        c.loadedSkills = skills.filter { activeSkills.contains($0.id) }
        c.off = Modules.current.offSummary
        c.safeSearch = safeSearch
        c.capital = equity.map { Fmt.usd($0) }
        if let s = solana { c.balances.append("Solana \(Fmt.num(s.usdc)) USDC, \(Fmt.num(s.sol, maxFraction: 4)) SOL") }
        // Every place with money, so «выведи всё из Jupiter» needs no question about which asset is there.
        if let h = hl { c.balances.append("Hyperliquid trading balance \(Fmt.usd(h.accountValue)), withdrawable \(Fmt.usd(h.withdrawable))") }
        for (dex, b) in dexBalances where b.accountValue > 0 { c.balances.append("Hyperliquid \(dex) exchange \(Fmt.usd(b.accountValue))") }
        if hlp.equity > 0 { c.balances.append("HLP vault \(Fmt.usd(hlp.equity))" + (hlp.lockedUntil.map { ", locked until \(Fmt.date($0))" } ?? "")) }
        for l in lend where l.usdc > 0 { c.balances.append("Jupiter Lend \(l.vault.replacingOccurrences(of: "jl", with: "")) \(Fmt.num(l.usdc))") }
        if kamino > 0 { c.balances.append("Kamino \(Fmt.usd(kamino))") }
        if let b = base {
            if b.aave > 0 { c.balances.append("Aave on Base \(Fmt.num(b.aave)) USDC") }
            if b.morpho > 0 { c.balances.append("Morpho on Base \(Fmt.num(b.morpho)) USDC") }
            if b.usdc > 0 { c.balances.append("Base \(Fmt.num(b.usdc)) USDC idle") }
        }
        // The other EVM networks too: without them «send from Ethereum» read as «there's nothing there».
        for net in ["arbitrum", "ethereum", "polygon", "tempo", "hyperevm", "optimism", "bnb"] {
            if let u = usdc(on: net), u > 0 { c.balances.append("\(WalletCommand.placeName(net)) \(Fmt.num(u)) USDC idle") }
        }
        for p in lendingElsewhere where p.usdc > 0 { c.balances.append("\(p.place) \(Fmt.num(p.usdc)) USDC") }
        // Made on a site. Kamino's vaults come out by the kamino form, any amount; the rest is managed on the site.
        for p in protocolPositions {
            let how = p.platform == "Kamino" ? "withdraw [N] \(p.symbol ?? "USDC") from kamino takes any amount of it" : "managed on \(p.site)"
            c.balances.append("\(p.platform) \(p.name) \(p.amount ?? Fmt.usd(p.usd)) (\(Fmt.usd(p.usd)); \(how))")
        }
        if near > 0 { c.balances.append("NEAR \(Fmt.num(near, maxFraction: 4)) NEAR") }
        if let r = robinhood {
            c.balances.append("Robinhood Chain \(Fmt.num(r.usdg)) USDG" + r.stocks.map { ", \(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol) stock" }.joined())
        }
        for t in favoriteTokens { if let held = tokenBalances[t.id], held > 0 { c.balances.append("\(t.symbol) token on \(t.chainTitle) \(Fmt.num(held, maxFraction: 4))") } }
        // Tokens the portfolio shows on every network — a vault's shares among them (fUSDC, mUSDC…), read from the chain.
        for t in shownTokens where !t.native && t.symbol.uppercased() != "USDC" && (t.usd ?? 0) >= Decimal(string: "0.5")!
            && !favoriteTokens.contains(where: { $0.id == t.id }) {
            c.balances.append("\(Fmt.num(t.amount, maxFraction: 6)) \(t.symbol) (\(t.contract)) on \(WalletCommand.placeName(t.chain)) ≈ \(Fmt.usd(t.usd ?? 0))")
        }
        return c
    }

    /// Free-form plan edits («take closer», «less risk») by the model.
    private func editPlanWithModel(_ text: String, client: any LLMClient, before: TradeIntent) async {
        busy = true
        let note = "\(model) · \(provider.info.name)"
        do {
            let plan = intent, model = model, history = recentChat(), language = replyLanguage
            let (edited, answer) = try await modelCall { try await ChatModel.edit(plan, message: text, client: client, model: model, history: history, replyLanguage: language) }
            busy = false
            let next = edited.map(Self.started)
            if let next, await editExecuted(from: before, to: next, confirm: true) { return }
            if let next { intent = next }
            let changes = next?.changes(from: before) ?? []
            let fallback = changes.isEmpty ? say("The model changed nothing.", "Модель ничего не изменила.") : planUpdated(changes)
            await refreshQuote()
            reply((answer.isEmpty ? fallback : answer) + outcomeLine, note: note)
        } catch {
            busy = false
            reply(say("The model is unavailable: ", "Модель недоступна: ") + error.localizedDescription, note: note)
        }
        await refreshQuote()
        startPlanReview()
    }

    /// Runs the model's English command through the parsers. False when it is not a command they know.
    /// `inChain`: a step of «A then B», whose «In steps» line already said what was understood.
    func runCanonical(_ canonical: String, userText text: String, before: TradeIntent, inChain: Bool = false) async -> Bool {
        if !inChain { updateActivity { $0.understood = canonical } }
        let note = "understood as “\(readable(canonical))” · \(model)"
        if ScheduledTask.cancelsAll(canonical) { cancelTasks(note: note); return true }
        if let task = ScheduledTask.parse(canonical) { schedule(task, note: note); return true }
        // A paid request before any other reading: its body is free text, where a contract, «then» or «chart» belongs to
        // the question, not to another command (Grok's X search about MOO opened MOO's token card instead).
        if case .pay? = WalletCommand.parse(canonical), let command = WalletCommand.parse(canonical) { await handleWallet(command, note: note); return true }
        if let command = MemoryCommand.parse(canonical) { memoryCommand(command, note: note); return true }
        if let entry = JournalCommand.parse(canonical) { await journalCommand(entry, note: note); return true }
        if let study = StudyNote.parse(canonical) { studyNote(study, note: note); return true }
        if let check = AgentCheck.parse(canonical) { await runCheck(check, note: note); return true }
        // Any contract's function: its addresses are checked against the user's own words and the explorer there.
        if case .contractCall? = WalletCommand.parse(canonical), let command = WalletCommand.parse(canonical) {
            if let off = moduleRefusal(command) { reply(off, note: note); return true }
            await prepareContractCall(command, note: note)
            return true
        }
        // «6 USDC then leverage 4x» is one change to the plan.
        if let merged = ChatRouter.planChange(canonical, markets: Set(markets.keys)) {
            return await runCanonical(merged, userText: text, before: before)
        }
        // «withdraw 5 USDC to Solana then deposit 5 USDC to kamino»: the steps now, one Confirm for the whole chain.
        if let steps = ChatRouter.steps(canonical) {
            guard steps.allSatisfy(knows) else { return false }
            reply(say("In steps: ", "По шагам: ", "Por pasos: ") + steps.enumerated().map { "\($0.offset + 1). \($0.element)" }.joined(separator: " → "), note: note)
            await runSteps(steps)
            return true
        }
        switch canonical.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".! ")) {
        case "new canvas":
            messages.removeLast()
            newCanvas()
            return true
        case "clear the plan":
            clearPlan(note: note)
            return true
        case "lock":
            await lockSigner()
            return true
        case "create an automation wallet":
            await createAutomationWallet(note: note)
            return true
        default: break
        }
        // «write to folder Ideas: alert me when BTC is above 120000» is a note, not an alert.
        if let command = FolderCommand.parse(canonical) { folderCommand(command, note: note); return true }
        if let command = TokenCommand.parse(canonical) {
            await tokenCommand(command, note: note)
            return true
        }
        if let command = ChartCommand.parse(canonical, markets: Set(markets.keys)) {
            await chartCommand(command, note: note)
            return true
        }
        if let command = PnLCommand.parse(canonical, markets: pnlNames) { await showPnL(command, note: note); return true }
        if TakeStyle.asksForChoice(canonical), intent != TradeIntent() {
            takeChoices = TakeStyle.allCases
            reply(say("Three ways to take profit are on the canvas. The stop stays the same; pick one.",
                      "На холсте три способа взять прибыль. Стоп остаётся прежним; выбери один."), note: note)
            return true
        }
        if let command = AlertCommand.parse(canonical, markets: Set(markets.keys)) {
            alertCommand(command, note: note)
            return true
        }
        if let command = AddressBookCommand.parse(canonical) {
            // The model only rewords: an address it wrote must be in the user's own message, character for character.
            if case .save(let address, _, _) = command, !text.contains(address) { return false }
            await addressBookCommand(command, note: note)
            return true
        }
        if case .sendTo(let name, let amount, let net)? = WalletCommand.parse(canonical) {
            prepareTransfer(amount, to: name, network: net, note: note, quiet: inChain)
            return true
        }
        if let command = WalletCommand.parse(canonical) {
            await handleWallet(command, note: note, quiet: inChain)
            return true
        }
        if let coin = targetCoin(for: canonical), let command = PositionCommand.parse(canonical) {
            pendingAction = .position(command, coin: coin)
            pendingText = canonical
            reply(say("Understood as “\(canonical)” for \(IntentParser.pretty(coin)). Press Confirm to go ahead.",
                      "Понял как «\(canonical)» для \(IntentParser.pretty(coin)). Нажми Confirm."), note: note)
            return true
        }
        // «close ETH», «move stop to breakeven» with no such position: never a new plan («close ETH» names ETH).
        let onlyPosition = canonical.lowercased().contains(/^\s*(?:close|cancel\s+orders|move\s+stop\s+to\s+breakeven)\b/)
        let plan = onlyPosition ? nil : IntentParser.apply(canonical, to: intent, markets: Set(markets.keys)).map(Self.started)
        guard let next = plan else {
            // «close» with nothing open: say so, rather than that ihatecrypto can't close.
            guard PositionCommand.parse(canonical) != nil else { return false }
            reply(positions.isEmpty && pendingEntry == nil
                  ? say("There’s no open position, so nothing to \(canonical).", "Открытых позиций нет — «\(canonical)» не к чему применить.")
                  : say("Which position? Open: \(positions.map { IntentParser.pretty($0.coin) }.joined(separator: ", ")).",
                        "Какую позицию? Открыты: \(positions.map { IntentParser.pretty($0.coin) }.joined(separator: ", "))."), note: note)
            return true
        }
        if await editExecuted(from: before, to: next, confirm: true) { return true }
        intent = next
        let changes = next.changes(from: before)
        // Right after launch the prices may not be in yet: the plan is checked against its market, so load it first.
        if market == nil { await loadMarkets() }
        await refreshQuote()
        replyPlan(changes, command: canonical, userText: text, note: note)
        startPlanReview()
        return true
    }

    /// The reply after a command built or changed the plan: the changes and the plan checked item by item at once; then
    /// the model's own word on it (ChatRouter.comment, ~10 s) takes the first line's place, with the changes worth
    /// making as cards. The chat stays open meanwhile; the word is dropped when anything was said or changed since.
    private func replyPlan(_ changes: [String], command: String, userText: String, note: String) {
        let list = outcomeLine
        reply(planUpdated(changes) + list, note: note)
        guard let client = llmClient(), !list.isEmpty, let sent = messages.last, !sent.fromUser else { return }
        let plan = intent, context = routerContext(), model = model
        Task {
            let reading = try? await ChatRouter.comment(on: list, command: command, userText, context: context, client: client, model: model)
            guard let reading, case .chat(let words) = reading.route, intent == plan, messages.last?.id == sent.id else { return }
            messages[messages.count - 1] = Message(id: sent.id, fromUser: false, text: words + list, note: note)
            updateActivity { if let j = $0.lines.lastIndex(of: sent.text) { $0.lines[j] = words + list } }
            saveCanvas()
            offer(reading, userText: userText)
        }
    }

    /// «Plan updated: leverage 3x.» in the user's language; the changes are the plan's own words.
    private func planUpdated(_ changes: [String]) -> String {
        changes.isEmpty ? say("That’s already in the plan.", "Это уже есть в плане.")
                        : say("Plan updated: ", "План обновлён: ") + changes.joined(separator: ", ") + "."
    }

    static func started(_ next: TradeIntent) -> TradeIntent { next.started }

    /// «clear the plan», typed, read by the model or the plan's Cancel.
    func clearPlan(note: String? = nil) {
        // The price alert that waited to prepare this plan goes with it.
        let alert = planAlert
        if let alert { alerts.removeAll { $0.id == alert.id } }
        intent = TradeIntent()
        executed = nil
        takeChoices = nil
        reply(alert == nil ? say("The plan is cleared. Tell me what to do next.", "План очищен. Скажи, что дальше.")
                           : say("The plan is cleared, and its price alert with it. Tell me what to do next.", "План очищен, и его алерт тоже. Скажи, что дальше."), note: note)
    }

    /// After every message about the plan: the plan checked item by item and the next step (TradeAdvice.swift).
    var outcomeLine: String {
        let list = planChecklist
        return list.isEmpty ? "" : "\n" + list
    }

    func reply(_ text: String, note: String? = nil) {
        // The executor reports its last line and the move returns it too: say it once.
        if let last = messages.last, !last.fromUser, last.text == text { return }
        draftReply = nil
        messages.append(Message(fromUser: false, text: text, note: note))
        updateActivity { $0.lines.append(text) }
        saveCanvas()
    }

    // MARK: Live data

    func refresh() async {
        // The wallet this read is for. Switched meanwhile, the new one gets its own read: this one's equity point and
        // «money arrived» (the old wallet's balances against the new one's) are dropped, not written into it.
        let wallet = (activeWalletID, solanaAddress, hlAddress)
        func same() -> Bool { wallet == (activeWalletID, solanaAddress, hlAddress) }
        let before = (solana?.usdc, solana?.sol, base?.usdc)
        defer { if updatedAt != nil, same() { announceIncoming(before: (before.0, before.1, before.2)) } }
        let read = RefreshReads()
        await Self.$landings.withValue(read) {
            async let m: Void = loadMarkets()
            async let s: Void = loadSolana()
            async let h: Void = loadHL()
            async let b: Void = loadBase()
            async let r: Void = loadRobinhood()
            async let t: Void = refreshTokens()
            async let a: Void = loadAutomationBudget()
            _ = await (m, s, h, b, r, t, a)
        }
        // Every network's numbers in one update of the window, with the time they were read.
        read.landed = true
        read.assigns.forEach { $0() }
        if same() { updatedAt = Date(); recordEquity(); rememberBalance() }
        // Not waited for: a spammed address takes many pages, and the balances above shouldn't sit behind it.
        if !holdingsLoading { Task { await loadHoldings() } }
        if let w = previewWallet {
            Task {
                let open = await executor(w).resolveTransit()
                if open != inTransit { inTransit = open }
            }
        }
        await refreshQuote()
    }

    /// While refresh() reads the networks at once, what each one read waits and lands with the rest: the reads end a few
    /// hundred milliseconds apart, and each assignment was its own redraw of the rail, the portfolio and its cards.
    /// `landed`: a Task started inside the refresh keeps the task-local after it; what it reads then assigns at once.
    final class RefreshReads { var assigns: [() -> Void] = [], landed = false }
    /// Set for refresh()'s own reads only (a task-local reaches its `async let` children, not a loader called elsewhere,
    /// which reads what it loaded right after).
    @TaskLocal static var landings: RefreshReads?
    /// Assigns now, or with the rest of the refresh it's part of. Its checks (still the same wallet?) go inside: they're
    /// made when it lands.
    func land(_ assign: @escaping () -> Void) {
        if let read = Self.landings, !read.landed { read.assigns.append(assign) } else { assign() }
    }

    /// Every token on the other networks, read at once and assigned together (each assignment after an await is a
    /// redraw). What the app counts elsewhere is skipped: USDC, Aave's aUSDC and the Morpho vault on Base, USDC on
    /// Arbitrum, USDG on Robinhood Chain, and ETH on Base and Robinhood (gas, counted with gasETH).
    func loadHoldings() async {
        holdingsLoading = true
        defer { holdingsLoading = false }
        let me = hlAddress, nearAccount = nearAddress, evm = isEVMAddress(hlAddress)
        // Aave's aUSDC and Morpho's vault shares off Base are read as lending positions, not as tokens.
        async let lending: Void = loadLending()
        async let protocols: Void = loadProtocols()
        async let base = evm ? await Holdings.evm("base", address: me, native: false, skip: [EVMRPC.baseUSDC, AaveV3.baseAUSDC, MorphoVault.gauntletUSDCPrime]) : []
        async let arbitrum = evm ? await Holdings.evm("arbitrum", address: me, native: true,
                                                      skip: [EVMRPC.arbitrumUSDC, AaveV3.arbitrum.aUSDC, MorphoVault.arbitrum.address]) : []
        async let ethereum = evm ? await Holdings.evm("ethereum", address: me, native: true, skip: [AaveV3.ethereum.aUSDC, MorphoVault.ethereum.address]) : []
        async let polygon = evm ? await Holdings.evm("polygon", address: me, native: true, skip: [AaveV3.polygon.aUSDC]) : []
        async let robinhood = evm ? await Holdings.evm("robinhood", address: me, native: false, skip: [EVMRPC.robinhoodUSDG]) : []
        async let tempo = evm ? await Holdings.evm("tempo", address: me, native: false, skip: []) : []
        async let hyperevm = evm ? await Holdings.evm("hyperevm", address: me, native: false, skip: []) : []
        async let optimism = evm ? await Holdings.evm("optimism", address: me, native: true, skip: []) : []
        // BNB Chain has no explorer to list its tokens: the ones bought here are asked for by contract.
        let boughtOnBNB = costBasis.keys.compactMap { $0.hasPrefix("bnb:") ? String($0.dropFirst(4)) : nil }
        async let bnb = evm ? await Holdings.evm("bnb", address: me, native: true, skip: [], extra: boughtOnBNB) : []
        async let spot = evm ? await Holdings.hyperliquidSpot(user: me) : []
        async let near = nearAccount != nil ? await Holdings.near(account: nearAccount ?? "") : []
        // A network that didn't answer keeps its tokens from the last read instead of dropping out of the total.
        let read: [(chain: String, tokens: [Holding]?)] = [("base", await base), ("arbitrum", await arbitrum), ("ethereum", await ethereum),
                                                           ("polygon", await polygon), ("robinhood", await robinhood), ("tempo", await tempo), ("hyperevm", await hyperevm), ("optimism", await optimism), ("bnb", await bnb), ("hyperliquid", await spot),
                                                           ("near", await near)]
        _ = await (lending, protocols)
        var found = read.flatMap { r in r.tokens ?? otherTokens.filter { $0.chain == r.chain } }
        // A vault's shares a contract call just brought, before the explorer lists them.
        if evm { found += await missingDeposits(found, owner: me) }
        // Another wallet since: its read starts now. refresh() starts none while this one runs, and the new wallet's
        // tokens waited for the next round, 30 s.
        guard me == hlAddress, nearAccount == nearAddress else { Task { if !holdingsLoading { await loadHoldings() } }; return }
        if found != otherTokens { otherTokens = found }
        if !holdingsLoaded { holdingsLoaded = true }
        rememberBalance()
        // Every network's gas coin is known now: dollars without it get their way out offered (once a network).
        offerGasWays()
    }

    /// Base balances at our EVM address and Aave's rate; quiet on errors (the rest of the portfolio still shows).
    func loadBase() async {
        guard isEVMAddress(hlAddress) else { return }
        let rpc = EVMRPC.base, me = hlAddress
        async let usdc = rpc.tokenBalance(EVMRPC.baseUSDC, owner: me)
        async let supplied = rpc.tokenBalance(AaveV3.baseAUSDC, owner: me)
        async let vault = MorphoVault.value(me, MorphoVault.base)
        async let eth = rpc.balance(me)
        async let arb = EVMRPC.arbitrum.tokenBalance(EVMRPC.arbitrumUSDC, owner: me)
        // The rates move slowly: re-read them every ten minutes, not on every refresh.
        let stale = aaveCheckedAt.map { Date().timeIntervalSince($0) > 600 } ?? true
        async let rates: (aave: Decimal?, morpho: Decimal?) = stale ? (try? await AaveV3.verify(AaveV3.base), await MorphoRates.netAPY(MorphoVault.base)) : (nil, nil)
        // Awaited first, assigned together: every assignment after an await is one more redraw of the window.
        let u = try? await usdc, a = try? await supplied, m = try? await vault, e = try? await eth, arbitrum = try? await arb, r = await rates
        land { [self] in
            guard me == hlAddress else { return }   // another wallet since: its own read follows
            if let u, let a, let e {
                let b = (usdc: Decimal(u) / 1_000_000, aave: Decimal(a) / 1_000_000, morpho: Decimal(m ?? 0) / 1_000_000,
                         eth: Decimal(e) / Decimal(sign: .plus, exponent: 18, significand: 1))
                // A tuple isn't Equatable: without this check @Observable redraws on every refresh.
                if base.map({ $0 != b }) ?? true { base = b }
            }
            if let arbitrum { arbitrumUSDC = Decimal(arbitrum) / 1_000_000 }
            if let apy = r.aave {
                aaveAPY = apy
                morphoAPY = r.morpho
                aaveCheckedAt = Date()
            }
        }
    }

    func loadMarkets() async {
        do {
            let read = try await Hyperliquid.markets()
            land { [self] in
                markets = read
                marketsError = nil
                let names = Dictionary(uniqueKeysWithValues: Market.Group.allCases.map { g in (g, markets(g, limit: g == .crypto ? 25 : 30).map(\.name)) })
                if names != marketNames { marketNames = names }
            }
        } catch {
            land { [self] in marketsError = error.localizedDescription }
        }
    }

    func loadSolana() async {
        guard isSolanaAddress(solanaAddress) else {
            solana = nil
            solanaError = solanaAddress.isEmpty ? nil : "Check the Solana address format."
            return
        }
        // All at once, assigned together: every assignment after an await is one more redraw of the window.
        let owner = solanaAddress, nearAccount = nearAddress
        async let balance = Solana.balance(owner: owner)
        async let lendPositions = try? JupiterLend.positions(user: owner)
        async let kaminoPositions = try? Kamino.positions(wallet: owner)
        async let nearBalance = { () async -> (near: Decimal, wrapped: Decimal)? in
            guard let nearAccount else { return nil }
            return try? await Near.balance(account: nearAccount)
        }()
        // Another wallet since the read began: its results aren't this one's, and its own read follows.
        func current() -> Bool { owner == solanaAddress && nearAccount == nearAddress }
        do {
            let s = try await balance
            let l: [(vault: String, usdc: Decimal)] = ((await lendPositions) ?? []).filter { $0.assets > 0 }
                .map { ($0.vault, Decimal($0.assets) / 1_000_000) }
            let k = await kaminoPositions, n = await nearBalance
            land { [self] in
                guard current() else { return }
                solana = s.pricing(from: solana?.tokens ?? [])
                solanaError = nil
                // Tuples aren't Equatable: without this check @Observable redraws on every refresh.
                if !l.elementsEqual(lend, by: ==) { lend = l }
                if let k { kamino = k.reduce(0) { $0 + $1.valueUSD } }
                if let n { near = n.near + n.wrapped }
            }
        } catch {
            // A failed read keeps the last balances: set to nil, the rail and the canvas's cards went blank until the next one.
            if current() { solanaError = error.localizedDescription }
        }
    }

    func loadHL() async {
        guard isEVMAddress(hlAddress) else {
            hl = nil
            hlError = hlAddress.isEmpty ? nil : "Check the address format: 0x and 40 hex characters."
            return
        }
        // All at once, assigned together: every assignment after an await is one more redraw of the window.
        let user = hlAddress
        async let balance = Hyperliquid.balance(user: user)
        async let openPositions = try? HyperliquidAccount.positions(user)
        async let openOrders = try? HyperliquidAccount.openOrders(user)
        async let vault = try? HyperliquidAccount.vaultEquity(user: user, vault: HLGuard.hlpVault)
        async let dexes = { () async -> [String: (accountValue: Decimal, withdrawable: Decimal)] in
            var out: [String: (accountValue: Decimal, withdrawable: Decimal)] = [:]
            for dex in Hyperliquid.hip3Dexes { out[dex] = try? await Hyperliquid.dexBalance(user: user, dex: dex) }
            return out
        }()
        do {
            let b = try await balance
            // A request that failed keeps what was read before: read as «none», the position left the canvas and
            // looked closed by its stop («closed by the exchange» notice) until the next refresh brought it back.
            let p = (await openPositions)?.filter { $0.size != 0 } ?? positions
            let o = (await openOrders) ?? orders, v = await vault, d = await dexes
            // Another wallet since the read began: its positions would read as this one's closed («closed by the
            // exchange», exit rules cleared). Dropped; the new wallet's own read follows.
            guard user == hlAddress else { return }
            // The week's closes are read again when the open positions changed: after the positions and their
            // notices, so a stop-out never waits on the fills request (one more redraw, only then).
            let readFills = recentlyClosed == nil || Set(positions.map(\.coin)) != Set(p.map(\.coin))
            let before = Set(positions.map(\.coin))
            let waitingEntries = Set(orders.filter { !$0.isTrigger }.map(\.coin))
            hl = b
            hlError = nil
            positions = p
            offerTradingKey()
            // Tuples aren't Equatable: without these checks @Observable redraws on every refresh.
            if d.count != dexBalances.count || d.contains(where: { dex, new in dexBalances[dex].map { $0 != new } ?? true }) { dexBalances = d }
            orders = o
            if let v, hlp != (v.equity, v.lockedUntil) { hlp = (v.equity, v.lockedUntil) }
            // A position that vanished while we were not closing it was closed by its stop or take.
            if !executing {
                for coin in before.subtracting(positions.map(\.coin)) where user == hlAddress { await closedByExchange(coin) }
                // A limit entry that became a position while nobody watched: it filled.
                for p in positions where !before.contains(p.coin) && waitingEntries.contains(p.coin) {
                    notify(.orders, say("Limit filled: \(IntentParser.pretty(p.coin))", "Лимитка исполнилась: \(IntentParser.pretty(p.coin))"),
                           say("\(p.size > 0 ? "Long" : "Short") \(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)); its stop and take are on the exchange.",
                               "\(p.size > 0 ? "Лонг" : "Шорт") \(Fmt.num(abs(p.size), maxFraction: 6)) по \(Fmt.price(p.entry)); стоп и тейк стоят на бирже."),
                           schedule: "perp:\(p.coin)")
                }
            }
            // The HLP deposit's 4-day lock ended: say so once per lock.
            if let until = hlp.lockedUntil, until <= Date(), hlp.equity > 0,
               UserDefaults.standard.double(forKey: "hlpUnlockNoticed") != until.timeIntervalSince1970 {
                UserDefaults.standard.set(until.timeIntervalSince1970, forKey: "hlpUnlockNoticed")
                notify(.hlp, say("HLP unlocked", "HLP разблокирован"),
                       say("\(Fmt.usd(hlp.equity)) in the HLP vault can be withdrawn now.", "\(Fmt.usd(hlp.equity)) в хранилище HLP теперь можно вывести."))
            }
            if readFills, let fills = try? await TradeHistory.fills(user: user), user == hlAddress {
                let week = Date().addingTimeInterval(-7 * 86400)
                recentlyClosed = TradeHistory.stories(fills).filter { ($0.closed ?? .distantPast) > week }
            }
        } catch {
            // As on Solana: the last balance stays through a failed read.
            if user == hlAddress { hlError = error.localizedDescription }
        }
    }

    /// Re-quotes only when the amount to bridge changed, or whether the Hyperliquid account exists yet.
    func refreshQuote() async {
        if atr[intent.asset] == nil, let v = try? await Hyperliquid.atrPct(coin: intent.asset) { atr[intent.asset] = v }
        // The resolved plan's: «5 USDC» that sizes the trade is its margin, and the bridge brings what that needs.
        let amount: Bridge.Amount? = if let a = resolved.intent.amountUSDC { .send(a) }
            else if let need = Plan.bridgeNeed(resolved.intent, evaluation, hlWithdrawable: hl.map { $0.withdrawable + (market?.dex.flatMap { dexBalances[$0]?.withdrawable } ?? 0) }, mark: market?.markPx) {
                .arrive(Executor.bridgeArrive(need: need, margin: evaluation.sizing?.margin ?? 0, solanaUSDC: solana?.usdc))
            }
            else { nil }
        // A new Hyperliquid account pays a one-time opening fee on its first deposit; the plan shows it up front.
        let newAccount = if amount != nil, let evm = previewWallet?.evm { (try? await Hyperliquid.isNewAccount(evm)) ?? false } else { false }
        // Solana's USDC is in the key: when it's short, the money comes straight from an EVM network that has it.
        let key = amount.map { "\($0) \(newAccount) \(solana?.usdc ?? 0) \(openVia1Click == true)" }
        guard key != quotedKey else { return }
        quotedKey = key
        quote = nil
        quoteError = nil
        guard let amount else { fundingNet = nil; return }
        do {
            var q = try await Bridge.dryQuote(amount, newAccount: newAccount, oneClickFirst: newAccount && openVia1Click == true)
            // The executor picks the same way, from the same quote.
            let net = q.amountIn > (solana?.usdc ?? 0) ? EVMNetwork.funding(q.amountIn) { usdc(on: $0) } : nil
            if let net { q = try await Bridge.dryQuote(.send(q.amountIn), newAccount: newAccount, from: (net.relayChain, net.usdc, net.usdcDecimals)) }
            if quotedKey == key { quote = q; fundingNet = net }
        } catch {
            if quotedKey == key { quoteError = error.localizedDescription; quotedKey = nil; fundingNet = nil }
        }
    }

    /// A new Hyperliquid account opens with this trade: the user picks the bridge on two cards, with what each costs and
    /// how long it takes. Relay lands in seconds and carries Hyperliquid's one-time opening fee (≈ $1.20, 2026-09-28);
    /// NEAR Intents charges none but takes 2–4 min, and the entry then waits for the price to hold.
    func askOpeningRoute(_ q: BridgeQuote) {
        let fee = q.amountIn - q.reserve - q.amountOut, other = q.alternativeFee ?? 0
        let (relay, near) = q.provider == "Relay" ? (fee, other) : (other, fee)
        reply(say("Your Hyperliquid account opens with this trade. How should the USDC come from Solana?",
                  "Аккаунт Hyperliquid откроется этой сделкой. Как довезти USDC с Solana?"))
        offerCards([
            ChatRouter.Option(title: say("Open via Relay", "Открыть через Relay"),
                              comment: say("seconds · ≈ \(Fmt.usd(relay)), incl. Hyperliquid's one-time opening fee",
                                           "секунды · ≈ \(Fmt.usd(relay)), включая разовую плату Hyperliquid за открытие"),
                              command: Self.openVia + "relay"),
            ChatRouter.Option(title: say("Open via NEAR Intents", "Открыть через NEAR Intents"),
                              comment: say("2–4 min · ≈ \(Fmt.usd(near)), no opening fee; the entry waits for the price to hold",
                                           "2–4 мин · ≈ \(Fmt.usd(near)), без платы за открытие; вход — если цена устоит"),
                              command: Self.openVia + "oneclick"),
        ])
    }

    static let openVia = "open hyperliquid via "

    /// The card picked: the quote follows the choice and the trade is prepared with it.
    func chooseOpening(_ command: String) async {
        openVia1Click = command.hasSuffix("oneclick")
        quotedKey = nil
        await refreshQuote()
        await prepareExecution()
    }

    /// The plan again from now: balances, prices and a new bridge quote, then a fresh preparation. For a canvas that has
    /// gone stale or a preparation that failed on old numbers.
    func rerunPlan() async {
        // The sheet stays up («Preparing the trade») through the refresh instead of closing and coming back.
        preparing = true
        defer { preparing = false }
        prepared = nil
        executionError = nil
        quotedKey = nil
        await refresh()
        await prepareExecution()
    }

    func loadSigner() async {
        guard SignerClient.isAvailable, let status = try? await SignerClient().send(.status) else { return }
        AppLock.shared.update(codeSet: status.codeSet == true)
        if tasksTampered {
            tasksTampered = false
            reply(say("The scheduled tasks were changed outside ihatecrypto, so every one is paused. Check them in the Schedule and switch on only the ones you made.",
                      "Задачи по расписанию изменили вне ihatecrypto, поэтому все они на паузе. Проверь их в Schedule и включи только свои."))
        }
        // The switch first, before anything can wait: the old wallet's card, rules and positions go at once.
        // A connected Ledger is one more wallet in the list; while it is the active one, its addresses are «ours» and
        // every move is signed on it (Ledger.swift). The signer's own choice stays as it was underneath.
        let ledger = LedgerAccount.saved
        let onLedger = ledger != nil && LedgerAccount.active
        signerHasWallet = status.hasWallet == true || ledger != nil
        wallets = (status.wallets ?? []) + (ledger.map { [$0.wallet] } ?? [])
        let active = onLedger ? LedgerAccount.id : status.activeWallet
        if active != activeWalletID { walletChanged(to: active) }
        // The addresses with the switch, before anything waits: a balance read started meanwhile would read the old
        // wallet's addresses into the new one.
        if let ledger, onLedger {
            let solana = ledger.solana ?? "", evm = ledger.evm ?? ""
            signerWallet = TradeWallet(solana: solana, evm: evm)
            if solanaAddress != solana { solanaAddress = solana }
            if hlAddress.lowercased() != evm.lowercased() { hlAddress = evm }
        } else if status.hasWallet == true, status.solanaAddress != nil || status.evmAddress != nil {
            // A key wallet leaves the other chain's address empty: its balances show nothing, its commands say why.
            let solana = status.solanaAddress ?? "", evm = status.evmAddress ?? ""
            signerWallet = TradeWallet(solana: solana, evm: evm)
            // The key holder defines the wallet: balances and plans follow it.
            if solanaAddress != solana { solanaAddress = solana }
            if hlAddress.lowercased() != evm.lowercased() { hlAddress = evm }
        } else {
            signerWallet = nil
        }
        // A phrase wallet imported before NEAR: the signer derives the address once (one unlock), then keeps it.
        // A key wallet has no NEAR account at all. The rest is assigned together after it: every assignment after an
        // await is one more redraw of the window.
        let phrase = activeWallet?.kind ?? "phrase" == "phrase"
        let near = onLedger ? nil : status.hasWallet == true && status.nearAddress == nil && phrase ? try? await SignerClient().send(.nearAddress).nearAddress : status.nearAddress
        let nearChanged = nearAddress != near
        nearAddress = near
        // On a Ledger, the trading key it approved for its own Hyperliquid account; otherwise the signer wallet's.
        var agent: SignerReply?
        if onLedger, let evm = ledger?.evm { agent = try? await SignerClient().send(.ledgerAgent(owner: evm, create: false)) }
        agentAddress = onLedger ? agent?.agentAddress : status.agentAddress
        agentActive = onLedger ? agent?.agentActive ?? false : status.agentActive ?? false
        sessionLength = status.sessionLength ?? SignerSession.standard
        confirmedAddresses = (try? await SignerClient().send(.addressBook))?.addresses ?? confirmedAddresses
        // NEAR's balance is read with Solana's.
        if nearChanged { await loadSolana() }
    }
}

func isSolanaAddress(_ s: String) -> Bool { s.wholeMatch(of: /[1-9A-HJ-NP-Za-km-z]{32,44}/) != nil }
func isEVMAddress(_ s: String) -> Bool { s.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil }

/// Credentials per provider, as JSON, in the login Keychain of this Mac only.
nonisolated enum Keychain {
    /// ihatecrypto.app keeps its own model logins: an item made by the development build would make the
    /// signed app ask for the login password.
    static let service = Bundle.main.bundleIdentifier.map { $0 + ".llm" } ?? "app.ihatecrypto.llm"

    static func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]
    }

    /// Each account as last read or saved here: a read is a round trip to securityd (≈ 4 ms), and every reply, the model
    /// menu (all the providers) and Settings asked again. Only this app writes these items, always through `save` (a
    /// refreshed token too), so what's kept stays true. A read that failed (a Keychain prompt turned down) isn't kept.
    private static let known = OSAllocatedUnfairLock<[String: Credential?]>(initialState: [:])

    static func load(_ account: String) -> Credential? {
        if let hit = known.withLock({ $0[account] }) { return hit }
        var q = query(account)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status == errSecItemNotFound { known.withLock { $0[account] = .some(nil) }; return nil }
        guard status == errSecSuccess, let data = out as? Data, let found = try? JSONDecoder().decode(Credential.self, from: data) else { return nil }
        known.withLock { $0[account] = found }
        return found
    }

    static func save(_ credential: Credential?, for account: String) {
        SecItemDelete(query(account) as CFDictionary)
        guard let credential, let data = try? JSONEncoder().encode(credential) else { known.withLock { $0[account] = .some(nil) }; return }
        var q = query(account)
        q[kSecValueData as String] = data
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let added = SecItemAdd(q as CFDictionary, nil) == errSecSuccess
        // Not written: read it again next time rather than trust what was meant to be there.
        known.withLock { $0[account] = added ? .some(credential) : nil }
    }

}
