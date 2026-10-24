import EVMChain
import Foundation
import IhatecryptoCore
import LLMCatalog
import LLMCore
import TradeExecution
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// One block a message went with, as its chip over the message says it (the Mac's SentContextChip): its title, what the
/// model was told, and whether only its name went.
nonisolated struct SentContext: Codable, Equatable {
    var title: String
    var about: String
    var bare: Bool
}

/// A card a reply offered, kept with that reply (the Mac's Message.options): live while it's the last reply's and under
/// 30 minutes old; an older set folds, its pick keeping the check.
nonisolated struct OfferView: Codable, Equatable {
    var title: String
    var comment: String
    var recommended: Bool
    /// What the card runs, shown on hover as on the Mac («Answer with this» when it's a reply); nil in older saved chats.
    var command: String?
}

/// A priced swap's card, for the field's waiting bar (the Mac's quoteState): quoting, fresh until 15 s after its quote,
/// then stale — Refresh asks again before Confirm comes back.
nonisolated struct QuoteView: Encodable, Equatable {
    var state: String
    var at: Date?
    var line: String?
}

/// The model menu and the connect bar's facts beyond the catalog: the lists a subscription or a model on this PC gives
/// now, whether Ollama answers and is installed, what each model takes, downloads under way, this PC's memory.
nonisolated struct ModelsView: Encodable {
    var live: [String: [String]]
    var servers: [String: Bool]
    var installed: Bool
    var sizes: [String: Double]
    var downloads: [String: Double]
    var memoryGB: Int
    var recommended: [String: String]
    var refusals: [String: String]
}

extension Engine {
    // MARK: Stop

    /// A model call the Stop button can cancel, as the Mac's modelCall: its own task, so cancelling reaches the model and
    /// its lookups, never a signature (a move runs outside it).
    func modelCall<T: Sendable>(_ work: @escaping @Sendable () async throws -> T) async throws -> T {
        let task = Task { try await work() }
        cancelModel = { task.cancel() }
        modelRunning = true
        changed()
        defer { cancelModel = nil; modelRunning = false; changed() }
        return try await task.value
    }

    /// The Stop button and Esc: the model stops answering and looking things up, nothing it would have led to runs, and the
    /// request it carried on with is dropped. A move already signing goes on: it can't be called back.
    func stopModel() {
        guard runningBoard == nil, let cancel = cancelModel else { return }
        stopRequested = true
        agentGoal = nil
        cancel()
    }

    static let stoppedLine = ("Stopped — nothing ran. Write what you'd like instead.", "Остановил — ничего не запускал. Напиши, что сделать вместо этого.")

    // MARK: What the model looks at

    /// The line under the thinking dots while a lookup runs (the Mac's agentStep).
    func lookupStep(_ l: ChatRouter.Lookup) -> String? {
        switch l.tool {
        case "portfolio": return say("Checking your portfolio", "Смотрю портфель")
        case "market":
            let coin = IntentParser.namedAsset(l.arg, markets: Set(markets.keys)).map(IntentParser.pretty) ?? l.arg.uppercased()
            return say("Looking at \(coin)", "Смотрю \(coin)")
        case "token": return say("Looking the token up", "Ищу токен")
        case "rates": return say("Comparing rates", "Сравниваю ставки")
        case "gas": return say("Checking gas", "Смотрю газ")
        case "services": return say("Looking for a service that takes payment", "Ищу сервис с оплатой")
        case "service": return say("Reading the service's docs", "Читаю документацию сервиса")
        case "skill", "journal", "study":
            let name = skillsOn.first { $0.id == l.arg }?.name
            return name.map { say("Reading the skill «\($0)»", "Читаю скилл «\($0)»") } ?? say("Reading the skill", "Читаю скилл")
        case "trades": return say("Rebuilding the trades on Hyperliquid", "Восстанавливаю сделки на Hyperliquid")
        case let tool where AgentReads.tools.contains(tool):
            let host = tool == "fetch" ? URL(string: l.arg.components(separatedBy: " ")[0])?.host : nil
            return host.map { say("Reading \($0)", "Читаю \($0)") } ?? say("Reading the chain", "Читаю блокчейн")
        default: return nil
        }
    }

    // MARK: Cards kept with their reply

    /// The cards just offered go with the reply they follow, so an older reply keeps its own, folded (Engine.options).
    func pinOptions() {
        guard !options.isEmpty else { optionsMessage = nil; return }
        guard let i = messages.indices.last, !messages[i].fromUser else { optionsMessage = nil; return }
        let shown = options.map { OfferView(title: $0.title, comment: $0.comment, recommended: $0.recommended, command: $0.command) }
        if messages[i].options != shown { messages[i].options = shown; messages[i].picked = nil }
        optionsMessage = messages[i].id
        saveChat()
    }

    /// A card picked: the check stays on it once the set folds.
    func markPicked(_ index: Int) {
        guard let id = optionsMessage, let i = messages.firstIndex(where: { $0.id == id }) else { return }
        messages[i].picked = index
        saveChat()
    }

    // MARK: What a message went with

    /// The chips over a message: the block it was about, and the others the person attached with it.
    func sentContexts() -> [SentContext]? {
        let all = ([attached].compactMap { $0 } + alsoAttached)
        guard !all.isEmpty else { return nil }
        return all.map { b in
            let facts = blockFacts(b)
            let shows = facts.split(separator: ":", maxSplits: 1).last.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
            return SentContext(title: b.kind == "move" || b.kind == "result" || b.kind == "chart" ? b.title : b.kind, about: facts, bare: shows.isEmpty)
        }
    }

    // MARK: NEXT · AFTER THIS

    /// The canvas's blocks as the window draws them: the card waiting for Confirm gets the step the model plans after it
    /// (the Mac's agentNextSteps), pale, until the move goes or the request is dropped.
    var boardsShown: [Board] {
        guard let next = agentGoal?.next, let id = card?.id else { return boards }
        return boards.map { b in
            guard b.id == id else { return b }
            var b = b
            var step = Board.Step(label: say("NEXT · AFTER THIS", "ДАЛЬШЕ · ПОСЛЕ ЭТОГО"), title: next,
                                  lines: [say("the chat goes on once this is through · write a change before Confirm",
                                              "чат продолжит, когда это пройдёт · изменение напиши до Confirm")])
            step.status = "then"
            b.steps = (b.steps ?? []) + [step]
            return b
        }
    }

    // MARK: Try again, and the steps a move took

    /// A move that failed before anything went out can be laid out again, with fresh quotes (the Mac's retryCommand).
    func noteRetry(_ waiting: Pending) {
        if case .wallet(let c) = waiting, let board = runningBoard { retry = (board, c) }
    }

    func retryMove() async {
        guard let r = retry else { return }
        retry = nil
        await handleWallet(r.command)
    }

    /// The move's lines as it ran stay in the chat as one «Executed · N steps» card, before what it ended with.
    func keepSteps(at index: Int) {
        guard let lines = running?.lines, !lines.isEmpty, index <= messages.count else { return }
        var m = Message(fromUser: false, text: lines.joined(separator: "\n"), note: "execution")
        m.steps = lines
        messages.insert(m, at: index)
        saveChat()
    }

    // MARK: The price on a swap's card

    static let quoteLife: TimeInterval = 15

    /// A swap whose card shows a price: Jupiter's on Solana, the two ways of a Base or Arbitrum trade, a stock's.
    func priced(_ c: WalletCommand) -> Bool {
        if unpriced == c { return false }
        return switch c {
        case .buy(_, _?), .sell(_, _?), .swapOn("solana", _, _, _?), .crossSwap: true
        // The Mac's quoteSwap prices every place's swap and a coin sent through NEAR Intents (pricedCommand) — but not
        // Hyperliquid's spot book: its quote shows on the card and Confirm isn't held for it.
        case .hlSpotBuy, .hlSpotSell, .swapOn("hyperliquid", _, _, _): false
        case .swapOn("near", _, _, let x): x != nil
        case .swapOn(let place, _, _, _): EVMNetwork.named(place) != nil
        case .sendToken(let name, let token, _?, let net): (try? acrossRecipient(name, token: token, network: net)) != nil
        default: RouteEdit.wayKey(c) != nil || c.isStockTrade
        }
    }

    /// The open position this canvas's plan is on, or the coin just traded while its position is open: the chat's position
    /// chips (breakeven, trail, close half, close) show only then and act on it (the Mac's managesPosition and
    /// positions.contains(intent.asset)); targetCoin picks the same coin when the phrase names none.
    var chipsPosition: String? {
        let plan = intent != TradeIntent() ? intent.asset : tradedPlan?.asset
        guard let plan, positions.contains(where: { $0.coin == plan }) else { return nil }
        return plan
    }

    var quoteView: QuoteView? {
        guard let c = quotedMove, priced(c) else { return nil }
        if let j = jupiterShown, j.command == c {
            return state(c, gets: String(j.title.dropFirst()).trimmingCharacters(in: .whitespaces))
        }
        if let w = wayQuotes, w.command == c {
            // Asked and not answered yet: Refresh and «over 15 s old» while it keeps asking (the Mac's empty swapQuote).
            guard let best = w.ways.first else { return QuoteView(state: "stale") }
            return state(c, gets: best.gets, worth: best.usd)
        }
        return QuoteView(state: "quoting")
    }

    private func state(_ c: WalletCommand, gets: String, worth: Decimal? = nil) -> QuoteView {
        guard let at = quotedAt else { return QuoteView(state: "quoting") }
        // A sale by contract names its ticker once known (the Mac's tokenSymbol).
        var sold: String?
        if case .sellOn(let place, let t, _, _) = c, t.hasPrefix("0x") { sold = tokenSymbol(t, on: place) }
        return QuoteView(state: Date().timeIntervalSince(at) < Self.quoteLife ? "fresh" : "stale", at: at,
                         line: Self.swapLine(c, gets: gets, worth: worth, sold: sold))
    }

    /// «You get ≈ 23.01 SPX · 1 SPX ≈ $0.4346», from what the quote gets (the Mac's swapLine), and «⚠️ fees take ≈ $0.18
    /// (18%): a bigger amount loses less» when dollars go in and fees keep more than a tenth. `worth`: what arrives in
    /// dollars after gas (an EVM way's, a stock's); without it, a cross quote's own «≈ $X».
    nonisolated static func swapLine(_ c: WalletCommand, gets: String, worth: Decimal? = nil, sold: String? = nil) -> String {
        // A coin bought on the way to a NEAR Intents address: the quote's own words — what it costs in USDC, the fees, the
        // time — never «You get» (that's what the send costs, not what arrives).
        if case .sendToken = c { return gets }
        guard let m = gets.firstMatch(of: /([\d,]+(?:\.\d+)?)\s+([^\s·≈(]+)/),
              let out = Decimal(string: String(m.1).replacingOccurrences(of: ",", with: "")), out > 0 else { return "You get ≈ \(gets)" }
        let got = String(m.2)
        let pays: (amount: Decimal, symbol: String)? = switch c {
        case .buy(_, let u?), .buyOn(_, _, let u?, _), .buyStock(_, let u?): (u, "USDC")
        case .sell(let a, let x?): (x, a.symbol)
        case .sellOn(_, let t, let x?, _): (x, sold ?? (t.hasPrefix("0x") ? "\(t.prefix(6))…" : t.uppercased()))
        case .sellStock(let s, let x?): (x, s)
        case .swapOn(_, let a, _, let x?), .crossSwap(let a, _, _, _, let x?): (x, a.uppercased())
        default: nil
        }
        var line = "You get ≈ \(Fmt.num(out, maxFraction: 6)) \(got)"
        if let pays, pays.amount > 0 {
            let dollars = Stablecoins.dollars
            if dollars.contains(pays.symbol.uppercased()) { line += " · 1 \(got) ≈ $\(Fmt.price(pays.amount / out))" }
            else if dollars.contains(got.uppercased()) { line += " · 1 \(pays.symbol) ≈ $\(Fmt.price(out / pays.amount))" }
            else { line += " · 1 \(pays.symbol) ≈ \(Fmt.price(out / pays.amount)) \(got)" }
            let value = worth.flatMap { $0 > 0 ? $0 : nil }
                ?? gets.firstMatch(of: /≈\s*\$([\d,]+(?:\.\d+)?)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: "")) }
            if dollars.contains(pays.symbol.uppercased()), let value, let warning = Fmt.feeWarning(paid: pays.amount, worth: value) { line += " · " + warning }
        }
        return line
    }

    /// «Sell all USDT» with the amount held for each «all X» the wallet holds in one place only: «Sell 0.999604 USDT»
    /// (the Mac's withAmounts, for the chat bar's head). Words stay where it's held in several places or not read yet,
    /// and for USDC, which is on every network.
    func withAmounts(_ text: String) -> String {
        text.replacing(/\ball ([A-Za-z][A-Za-z0-9₮.]*)/) { m in
            let want = String(m.1).uppercased()
            let held = portfolio.networks.flatMap { n in n.rows.filter { $0.section == "tokens" && $0.symbol.uppercased() == want }.map { (n.key, $0) } }
            guard want != "USDC", let first = held.first, first.1.value > 0,
                  held.allSatisfy({ $0.0 == first.0 && $0.1.contract == first.1.contract }) else { return String(m.0) }
            return "\(Fmt.num(first.1.value, maxFraction: 8)) \(m.1)"
        }
    }

    /// Refresh on a stale price: quoted again, and Confirm back for 15 s once it is.
    func requote() {
        guard let c = quotedMove, let id = card?.id else { return }
        quotedAt = nil
        jupiterShown = nil
        wayQuotes = nil
        changed()
        Task { await quoteCard(c, board: id) }
        if RouteEdit.wayKey(c) != nil || c.isStockTrade { Task { await quoteWays(c) } }
    }

    // MARK: Lookups the field asks for

    /// A contract or mint typed after «@» or «$»: what DexScreener knows of it, one row per network, for the list over the
    /// field (the Mac's lookUpContract). The person typed it there to look it up, so it goes out.
    func lookUpContract(_ address: String) async -> [String: String] {
        guard address.wholeMatch(of: /0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}/) != nil else { return ["list": "[]"] }
        let found = (try? await TokenLookup.lookup(address)) ?? []
        var chains = Set<String>()
        let rows: [[String: String]] = found.filter { chains.insert($0.chain).inserted }.map { t in
            var row = ["symbol": t.symbol, "name": t.name, "chain": t.chain, "address": t.address]
            row["image"] = t.imageURL?.absoluteString
            row["price"] = t.priceUSD.map { Fmt.usd($0) }
            row["liquidity"] = t.liquidityUSD.map { Fmt.usd($0) }
            return row
        }
        let json = (try? JSONSerialization.data(withJSONObject: rows)).flatMap { String(data: $0, encoding: .utf8) } ?? "[]"
        return ["list": json]
    }

    /// The blocks «/» lists: the ones a pick puts an example of in the field (BlockCatalog, the Mac's list).
    func blockCatalog() -> [String: String] {
        let rows = BlockCatalog.all.filter { !$0.automatic && !$0.example.isEmpty }.map { b in
            ["title": b.title, "group": b.group, "example": b.example, "logo": b.logo ?? ""]
        }
        let json = (try? JSONSerialization.data(withJSONObject: rows)).flatMap { String(data: $0, encoding: .utf8) } ?? "[]"
        return ["list": json]
    }

    // MARK: The context meter

    /// The lines of the chat that go to the model with a message (ChatRecall): all of them to a wide window, the last 12
    /// at most to a model on this PC, as on the Mac.
    func sentChat(_ lines: [String]) -> [String] {
        ChatRecall.sent(lines, window: LLMCatalog.window(provider: providerID) ?? meters[canvasID]?.window,
                        small: provider(providerID)?.info.kind == .local, instructions: ChatRouter.instructionTokens)
    }

    /// What one answer read and cost, for the chip over the field (the Mac's ContextMeter.count): the window's parts when it
    /// was the chat reading a message; dollars for every answer an API key pays for.
    func counted(_ usage: LLMUsage, request: LLMRequest, provider id: String) async {
        let paid = provider(id)?.info.kind == .apiKey
        if paid, modelFacts.isEmpty { await loadModelFacts() }
        let facts = paid ? modelFacts[ModelFacts.key(request.model)] : nil
        var m = meters[canvasID] ?? .init(used: 0, window: nil)
        if ChatRouter.reads(request) {
            canvasTalked(provider: id)
            let bytes = max(1, request.messages.reduce(0) { $0 + $1.text.utf8.count }), brief = request.system.utf8.count
            let all = chatLines(), sent = sentChat(all)
            m.used = usage.input
            m.window = usage.window ?? LLMCatalog.window(provider: id) ?? facts?.window ?? m.window
            m.instructions = usage.input * min(brief, bytes) / bytes
            m.chat = usage.input * min(max(0, bytes - brief), sent.reduce(0) { $0 + $1.utf8.count }) / bytes
            m.sent = sent.count
            m.lines = all.count
        }
        if paid, let cost = usage.cost ?? facts?.cost(usage) {
            m.spent += cost
            m.last = cost
            m.billed = usage.cost != nil
        }
        meters[canvasID] = m
        changed()
    }

    /// Windows and prices of the models a key pays for, read once a launch (asked again after a minute when it failed).
    func loadModelFacts() async {
        guard Date().timeIntervalSince(modelFactsAsked) > 60, let url = URL(string: ModelFacts.source) else { return }
        modelFactsAsked = Date()
        if let (data, _) = try? await URLSession.shared.data(from: url) { modelFacts = ModelFacts.table(data) }
    }

    // MARK: The model menu's lists, and models on this PC

    nonisolated static let ollama = "http://localhost:11434"

    /// Ollama's app or its server on this PC.
    nonisolated static var ollamaInstalled: Bool {
        let local = ProcessInfo.processInfo.environment["LOCALAPPDATA"] ?? ""
        let program = ProcessInfo.processInfo.environment["ProgramFiles"] ?? "C:\\Program Files"
        return [local + "\\Programs\\Ollama\\ollama app.exe", local + "\\Programs\\Ollama\\ollama.exe", program + "\\Ollama\\ollama.exe"]
            .contains { FileManager.default.fileExists(atPath: $0) }
    }

    nonisolated static var memoryGB: Double { Double(ProcessInfo.processInfo.physicalMemory) / 1_073_741_824 }

    /// A model fits when it and its working memory take at most ~70% of the PC's (the Mac's OllamaModels.fits).
    nonisolated static func fits(sizeGB: Double) -> Bool { sizeGB * 1.2 <= memoryGB * 1.073_741_824 * 0.7 }

    /// The biggest suggested model that fits this PC's memory; nil until the sizes are read.
    var localPick: String? {
        (provider("ollama")?.info.models ?? []).first { m in modelSizes[m].map(Self.fits(sizeGB:)) ?? false }
    }

    var modelsView: ModelsView {
        var recommended: [String: String] = [:]
        for p in LLMCatalog.all { recommended[p.info.id] = p.info.id == "ollama" ? localPick ?? p.info.defaultModel : p.info.defaultModel }
        let listed = Set(LLMCatalog.all.flatMap(\.info.models) + liveModels.values.flatMap { $0 })
        var refusals: [String: String] = [:]
        for m in listed { if let why = LLMCatalog.chatRefusal(model: m) { refusals[m] = why } }
        return ModelsView(live: liveModels, servers: localServers, installed: Self.ollamaInstalled, sizes: modelSizes, downloads: modelDownloads,
                          memoryGB: Int(Self.memoryGB.rounded()), recommended: recommended, refusals: refusals)
    }

    /// Read when the chat's model chip appears and after its menu closes, never while it's open (the rows would move under
    /// the pointer): a subscription's live list, and what the servers on this PC have downloaded.
    func refreshModels() async {
        let id = providerID
        if let p = provider(id), p.info.kind == .subscription, let list = try? await p.models(Store.credentials()[id]), !list.isEmpty {
            liveModels[id] = Array(list.prefix(4))
        }
        for p in LLMCatalog.all where p.info.kind == .local && p.info.unavailable == nil && p.info.id != "apple" {
            if p.info.id == "ollama", let tags = await Self.ollamaDownloaded() {
                for (name, gb) in tags { modelSizes[name] = gb }
                // Its version, as the Mac reads it here: an Ollama updated outside the app takes the model again.
                await readOllamaVersions()
            }
            guard let list = try? await p.models(nil) else { localServers[p.info.id] = false; continue }
            localServers[p.info.id] = true
            liveModels[p.info.id] = list.filter { !$0.lowercased().contains("embed") }
            if p.info.id == "ollama" {
                for m in p.info.models where !(liveModels["ollama"] ?? []).contains(m) && modelSizes[m] == nil { modelSizes[m] = await Self.ollamaSize(m) }
            }
        }
        changed()
    }

    /// The downloaded models and their sizes, GB, from Ollama's /api/tags; nil when Ollama doesn't answer.
    nonisolated static func ollamaDownloaded() async -> [String: Double]? {
        guard let url = URL(string: "\(ollama)/api/tags"),
              let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: url, timeoutInterval: 3)),
              let models = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["models"] as? [[String: Any]] else { return nil }
        return Dictionary(models.compactMap { m in (m["name"] as? String).map { ($0, ((m["size"] as? NSNumber)?.doubleValue ?? 0) / 1_000_000_000) } },
                          uniquingKeysWith: { a, _ in a })
    }

    /// A model's download size, GB, from the registry's manifest; nil when the registry doesn't know it.
    nonisolated static func ollamaSize(_ model: String) async -> Double? {
        let parts = model.split(separator: ":", maxSplits: 1).map(String.init)
        let name = parts[0].contains("/") ? parts[0] : "library/\(parts[0])"
        guard let url = URL(string: "https://registry.ollama.ai/v2/\(name)/manifests/\(parts.count > 1 ? parts[1] : "latest")") else { return nil }
        var request = URLRequest(url: url, timeoutInterval: 10)
        request.setValue("application/vnd.docker.distribution.manifest.v2+json", forHTTPHeaderField: "Accept")
        guard let (data, response) = try? await URLSession.shared.data(for: request), (response as? HTTPURLResponse)?.statusCode == 200,
              let layers = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["layers"] as? [[String: Any]] else { return nil }
        return layers.reduce(0) { $0 + ((($1["size"] as? NSNumber)?.doubleValue) ?? 0) } / 1_000_000_000
    }

    /// Downloads a model into Ollama from the model menu or the connect bar, with its progress; the chat says when it's ready.
    func downloadModel(_ model: String) async {
        guard modelDownloads[model] == nil else { return }
        // An Ollama older than its newest release is updated first: a new model wants a new Ollama (the Mac's pull).
        if localSetup.ollamaVersion == nil { await readOllamaVersions() }
        if localServers["ollama"] == true, needsNewOllama { await installOllama() }
        modelDownloads[model] = 0
        changed()
        defer { modelDownloads[model] = nil; changed() }
        do {
            try await OllamaPull.run(model) { [weak self] p in Task { @MainActor in self?.modelDownloads[model] = p; self?.changed() } }
            await refreshModels()
            reply(say("\(LLMCatalog.displayName(model: model)) is downloaded to this PC — pick it in the model menu.",
                      "\(LLMCatalog.displayName(model: model)) скачана на этот ПК — выбери её в меню моделей."))
        } catch {
            // «pull model manifest: 412: The model you are attempting to pull requires a newer version of Ollama.»: the bar
            // and the menu offer the update from then on (the Mac's ollamaOutdated).
            if error.localizedDescription.contains("newer version of Ollama") {
                localSetup.ollamaOutdated = true
                let version = localSetup.ollamaVersion.map { " \($0)" } ?? ""
                reply(say("Ollama\(version) is too old for \(LLMCatalog.displayName(model: model)). Update it (free, ollama.com/download), then press Download again.",
                          "Ollama\(version) слишком старая для \(LLMCatalog.displayName(model: model)). Обнови её (бесплатно, ollama.com/download) и нажми «Скачать» ещё раз."))
            } else {
                reply(say("Couldn't download \(model): ", "Не удалось скачать \(model): ") + error.localizedDescription)
            }
        }
    }

    /// «Open Ollama»: its app, when it's installed but not running.
    func startOllama() {
        #if os(Windows)
        // Every place ollamaInstalled counts: the tray app first, else the server alone.
        let env = ProcessInfo.processInfo.environment
        let local = env["LOCALAPPDATA"] ?? ""
        let program = env["ProgramFiles"] ?? "C:\\Program Files"
        let p = Process()
        if let app = [local + "\\Programs\\Ollama\\ollama app.exe", program + "\\Ollama\\ollama app.exe"]
            .first(where: { FileManager.default.fileExists(atPath: $0) }) {
            p.executableURL = URL(fileURLWithPath: app)
        } else if let server = [local + "\\Programs\\Ollama\\ollama.exe", program + "\\Ollama\\ollama.exe"]
            .first(where: { FileManager.default.fileExists(atPath: $0) }) {
            p.executableURL = URL(fileURLWithPath: server)
            p.arguments = ["serve"]
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
        } else { return }
        try? p.run()
        #endif
    }

    /// «Get Ollama»: its download page, in the person's browser (the Mac installs its signed app itself).
    func getOllama() {
        out.event("openURL", ["url": "https://ollama.com/download/windows"])
    }
}

/// Ollama's /api/pull streamed, read line by line as its JSON comes, reporting 0…1 over all the layers (the Mac's
/// OllamaModels.pull). A delegate, since URLSession's byte stream isn't on every platform's Foundation.
nonisolated final class OllamaPull: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private var buffer = Data()
    private var layers: [String: (done: Double, total: Double)] = [:]
    private var shown = -1.0
    private let progress: @Sendable (Double) -> Void
    private var finish: CheckedContinuation<Void, Error>?
    private var failed: Error?
    private var succeeded = false

    private init(progress: @escaping @Sendable (Double) -> Void) { self.progress = progress }

    static func run(_ model: String, progress: @escaping @Sendable (Double) -> Void) async throws {
        var request = URLRequest(url: URL(string: "\(Engine.ollama)/api/pull")!, timeoutInterval: 3600)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["model": model, "stream": true])
        let pull = OllamaPull(progress: progress)
        let session = URLSession(configuration: .default, delegate: pull, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
            pull.finish = c
            session.dataTask(with: request).resume()
        }
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        buffer.append(data)
        while let end = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<end]
            buffer.removeSubrange(buffer.startIndex...end)
            read(Data(line))
        }
    }

    private func read(_ line: Data) {
        guard let j = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { return }
        if let error = j["error"] as? String { failed = LLMError(error); return }
        if let digest = j["digest"] as? String, let total = (j["total"] as? NSNumber)?.doubleValue, total > 0 {
            layers[digest] = ((j["completed"] as? NSNumber)?.doubleValue ?? 0, total)
            let all = layers.values.reduce((0.0, 0.0)) { ($0.0 + $1.done, $0.1 + $1.total) }
            let p = min(all.0 / all.1, 1)
            if abs(p - shown) >= 0.01 { shown = p; progress(p) }
        }
        if j["status"] as? String == "success" { succeeded = true }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if !buffer.isEmpty { read(buffer); buffer.removeAll() }
        let status = (task.response as? HTTPURLResponse)?.statusCode ?? 0
        if let error { finish?.resume(throwing: error) }
        else if let failed { finish?.resume(throwing: failed) }
        else if status != 200 { finish?.resume(throwing: LLMError("Ollama didn't start the download (HTTP \(status)).")) }
        else if succeeded { finish?.resume() }
        else { finish?.resume(throwing: LLMError("Ollama stopped before the download finished.")) }
        finish = nil
    }
}
