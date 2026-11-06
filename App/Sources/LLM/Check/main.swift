import Foundation
import LLMCatalog
import IhatecryptoCore
import LLMCore
import TradeExecution

// Checks one provider from the terminal, without the app:
//   swift run LLMCheck                      list providers and existing logins
//   swift run LLMCheck chatgpt "hello"      reuse an existing login (Codex CLI, Claude Code)
//   IHATECRYPTO_LLM_KEY=sk-… swift run LLMCheck deepseek "hello"
//   IHATECRYPTO_LLM_MODEL=… overrides the default model.
//   swift run LLMCheck chatgpt --models                    the models the app offers, live
//   swift run LLMCheck chatgpt --route "купи AI на $2" …   how the chat router reads each message

let args = Array(CommandLine.arguments.dropFirst())
let env = ProcessInfo.processInfo.environment

guard let id = args.first else {
    for p in LLMCatalog.all {
        let status = p.info.unavailable != nil ? "unavailable" : p.existingLogin() != nil ? "signed in on this Mac" : ""
        print("\(p.info.id.padding(toLength: 20, withPad: " ", startingAt: 0)) \(p.info.name)  \(status)")
    }
    exit(0)
}
guard let provider = LLMCatalog.provider(id: id) else {
    fputs("No provider \(id)\n", stderr)
    exit(2)
}

let credential = env["IHATECRYPTO_LLM_KEY"].map(Credential.apiKey) ?? provider.existingLogin()
let model = env["IHATECRYPTO_LLM_MODEL"] ?? provider.info.defaultModel
let prompt = args.dropFirst().first ?? "Answer in one word: working"

// `--models`: the list the app's model menu shows, live from the provider.
if prompt == "--models" {
    print(try await provider.models(credential).joined(separator: "\n"))
    exit(0)
}

// `--verify "message" "command"`: the second reading alone, as the chat runs it on every command.
if prompt == "--verify", args.count >= 4 {
    var context = ChatRouter.Context()
    context.plan = env["IHATECRYPTO_ROUTE_PLAN"]
    context.recent = env["IHATECRYPTO_ROUTE_RECENT"].map { $0.components(separatedBy: "|") } ?? []
    context.replyLanguage = args[2].contains(/[а-яА-ЯёЁ]/) ? "Russian" : "English"
    let client = try provider.client(credential, save: { _ in })
    let why = try await ChatRouter.verify(args[2], command: args[3], context: context, client: client, model: model)
    print("«\(args[2])» as «\(args[3])» → " + (why.map { "✗ \($0)" } ?? "✓ matches"))
    exit(0)
}

// `--exam`: the app's ModelExam (Settings › AI model › Check) from the terminal: `swift run LLMCheck ollama --exam`.
if prompt == "--exam" {
    let client = try provider.client(credential, save: { _ in })
    let r = await ModelExam.run(client: client, model: model) { done in fputs("\r\(done)/\(ModelExam.questions.count)", stderr) }
    print("\n\(model): \(r.short)" + (r.misses.isEmpty ? "" : " · missed: " + r.misses.joined(separator: " | ")))
    exit(0)
}

if prompt == "--route" {
    var context = ChatRouter.Context()
    context.favorites = ["AI (Artificial Inu) = 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18 on Robinhood Chain"]
    context.addressBook = ["Bybit", "Mom"]
    context.balances = ["Solana 22.38 USDC, 0.0297 SOL", "Hyperliquid $7.47"]
    context.plan = env["IHATECRYPTO_ROUTE_PLAN"]
    context.capital = "$29.85"
    // `IHATECRYPTO_ROUTE_RECENT="user: …|ihatecrypto: …"`: the chat before the message, to check follow-ups like «да».
    context.recent = env["IHATECRYPTO_ROUTE_RECENT"].map { $0.components(separatedBy: "|") } ?? []
    // `IHATECRYPTO_SAFE_SEARCH=0`: as with Safe search off in Settings.
    context.safeSearch = env["IHATECRYPTO_SAFE_SEARCH"] != "0"
    // `IHATECRYPTO_ROUTE_CANVAS="waiting for Confirm: …|chart ETH (1 day)"` and `IHATECRYPTO_ROUTE_BALANCES="Ethereum 18,587 USDC idle|…"`:
    // the canvas and the money as the app gives them, for follow-ups about a waiting card.
    if let canvas = env["IHATECRYPTO_ROUTE_CANVAS"] { context.canvas = canvas.components(separatedBy: "|") }
    if let b = env["IHATECRYPTO_ROUTE_BALANCES"] { context.balances = b.components(separatedBy: "|") }
    // `IHATECRYPTO_ROUTE_BOOK="Bybit USDT (Solana)|Mom (Base)"`: the address book as the app gives it.
    if let b = env["IHATECRYPTO_ROUTE_BOOK"] { context.addressBook = b.components(separatedBy: "|") }
    // `IHATECRYPTO_ROUTE_POSITIONS="long ETH|short WTI oil (xyz:CL)"`: open positions, for «стоп в безубыток», «закрой половину».
    if let p = env["IHATECRYPTO_ROUTE_POSITIONS"] { context.positions = p.components(separatedBy: "|") }
    // `IHATECRYPTO_ROUTE_MEMORY="не трогай Ethereum mainnet|риск 1%"`: what the user asked the chat to remember.
    if let m = env["IHATECRYPTO_ROUTE_MEMORY"] { context.memory = m.components(separatedBy: "|") }
    // Counted: the model calls of each message and what went in and out of them, for the agent checks.
    let counter = CallCounter()
    // `IHATECRYPTO_THINK=1`: as with «Think before answering» on in Settings › Models.
    let made = try provider.client(credential, save: { _ in })
    let client = CountingClient(base: MeteredClient(env["IHATECRYPTO_THINK"] == "1" ? ThinkingClient(made) : made) { usage, _ in counter.count(usage) }, counter: counter)
    // `IHATECRYPTO_MARKETS="BTC,ETH,…,xyz:CL"`: Hyperliquid's real list, for the plans test.
    let markets: Set<String> = env["IHATECRYPTO_MARKETS"].map { Set($0.components(separatedBy: ",")) } ?? ["BTC", "ETH", "SOL", "xyz:CL", "xyz:TSLA"]
    // Real token lookups; the app's own data (portfolio, markets, rates) is what the context above says.
    let balances = context.balances.joined(separator: "; ")
    let lookup: @Sendable (ChatRouter.Lookup) async -> String = { l in
        l.tool == "token" ? await TokenResearch.report(l.arg) : l.tool == "portfolio" ? "Balances: \(balances). No open positions."
            : l.tool == "services" ? await MPP.search(l.arg) : l.tool == "service" ? await MPP.service(l.arg)
            // `IHATECRYPTO_ROUTE_EVM=0x…` / `IHATECRYPTO_ROUTE_SOLANA=…`: the addresses «me» stands for in the raw reads.
            : AgentReads.tools.contains(l.tool) ? await AgentReads.answer(l.tool, l.arg, me: .init(evm: env["IHATECRYPTO_ROUTE_EVM"], solana: env["IHATECRYPTO_ROUTE_SOLANA"]))
            : "Not available in this check."
    }
    var message = ""
    func verdict(_ c: String, found: [String] = []) -> String {
        var line = ChatRouter.parses(c, markets: markets) ? "✓ parses" : "✗ NO PARSER"
        if let task = ScheduledTask.parse(c) { line += "  [\(task.when)]" }
        if !ChatRouter.addressesAllowed(c, userText: message, known: ["0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"] + found) { line += "  ✗ FOREIGN ADDRESS" }
        return line
    }
    for m in args.dropFirst(2) {
        counter.reset()
        message = m
        context.replyLanguage = ChatLanguage.auto.name(recentUserMessages: [m])
        let started = Date()
        do {
            let checkContext = context
            @Sendable func secondReading(_ command: String) async -> String? {
                do { return try await ChatRouter.verify(m, command: command, context: checkContext, client: client, model: model, markets: markets) } catch { return nil }
            }
            let reading = try await ChatRouter.route(message, context: context, client: client, model: model, markets: markets,
                                                     lookup: lookup, check: secondReading)
            // `IHATECRYPTO_ROUTE_JSON=1`: one JSON line per message with the plan it leads to, for the plans test.
            if env["IHATECRYPTO_ROUTE_JSON"] == "1" {
                let base = context.plan.flatMap { try? JSONDecoder().decode(TradeIntent.self, from: Data($0.utf8)) } ?? TradeIntent()
                var out: [String: Any] = ["message": m, "seconds": Date().timeIntervalSince(started)]
                var intent: TradeIntent?
                switch reading.route {
                case .command(var written):
                    // The user's folders (`IHATECRYPTO_ROUTE_FOLDERS="Research,Ideas"`): a typo of one is that folder, as
                    // the app reads it (Workspace.folderCommand).
                    if let names = env["IHATECRYPTO_ROUTE_FOLDERS"]?.components(separatedBy: ","), let f = FolderCommand.parse(written) {
                        let name = switch f { case .save(let n): n; case .note(let n, _): n }
                        if let real = FolderCommand.existing(name, among: names), real != name { written = written.replacingOccurrences(of: name, with: real) }
                    }
                    let c = ChatRouter.planChange(written, markets: markets) ?? written
                    out["route"] = "command"; out["command"] = written; out["parses"] = ChatRouter.parses(written, markets: markets)
                    if ChatRouter.steps(c) == nil, ScheduledTask.parse(c) == nil, AlertCommand.parse(c, markets: markets) == nil,
                       WalletCommand.parse(c) == nil, TokenCommand.parse(c) == nil, ChartCommand.parse(c, markets: markets) == nil,
                       PnLCommand.parse(c, markets: markets) == nil, !c.lowercased().hasPrefix("close") {
                        intent = IntentParser.apply(c, to: base, markets: markets)?.started
                    }
                case .plan:
                    out["route"] = "plan"
                    intent = try? await ChatModel.edit(base, message: m, client: client, model: model, history: context.recent, replyLanguage: context.replyLanguage).0?.started
                case .chat(let r): out["route"] = "chat"; out["reply"] = r
                case .clarify(let r): out["route"] = "clarify"; out["reply"] = r
                case .confirm: out["route"] = "confirm"
                case .cancel: out["route"] = "cancel"
                case .question: out["route"] = "question"
                }
                if let intent, let data = try? JSONEncoder().encode(intent) { out["intent"] = try? JSONSerialization.jsonObject(with: data) }
                if let d = reading.doubt { out["doubt"] = d }
                out["lookups"] = reading.lookups.map { "\($0.tool) \($0.arg)" }
                if reading.continues { out["continue"] = true }
                if let next = reading.next { out["next"] = next }
                if reading.quiet { out["quiet"] = true }
                (out["calls"], out["in_chars"], out["out_chars"]) = counter.totals
                if let show = reading.show { out["show"] = show }
                if let c = reading.corrected { out["corrected"] = c }
                out["options"] = reading.options.map { $0.command.isEmpty ? "answer: " + $0.title : $0.command }
                print(String(decoding: try JSONSerialization.data(withJSONObject: out, options: [.sortedKeys]), as: UTF8.self))
                continue
            }
            var line = "\(reading.route)" + (reading.continues ? "  [continue]" : "")
            if case .command(let c) = reading.route { line += "  " + verdict(c) }
            for l in reading.lookups { line += "\n   lookup: \(l.tool) \(l.arg)" }
            if let show = reading.show { line += "\n   show: \(show)  " + verdict(show, found: reading.found) }
            for o in reading.options { line += "\n   \(o.recommended ? "★" : "·") \(o.title) — \(o.comment)\n       " + (o.command.isEmpty ? "(an answer: back to the model)" : "\(o.command)  " + verdict(o.command, found: reading.found)) }
            print("«\(message)» \(String(format: "%.1f", Date().timeIntervalSince(started))) s\(counter.context)\n   → \(line)")
        } catch {
            print("«\(message)» → error: \(error.localizedDescription)")
        }
    }
    exit(0)
}

do {
    let started = Date()
    let counter = CallCounter()
    let client = MeteredClient(try provider.client(credential, save: { _ in })) { usage, _ in counter.count(usage) }
    let reply = try await client.complete(LLMRequest(model: model, messages: [LLMMessage(.user, prompt)]))
    print("\(provider.info.name) · \(model) · \(String(format: "%.1f", Date().timeIntervalSince(started))) s\(counter.context)")
    print(reply)
} catch {
    fputs("\(provider.info.name) · \(model): \(error.localizedDescription)\n", stderr)
    exit(1)
}


/// Counts a message's model calls and the characters that went in (system and dialogue) and came out: ≈ 4 a token.
final class CallCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var calls = 0, inChars = 0, outChars = 0
    /// What the service itself counted: the biggest prompt of the message's calls, its window and price when told.
    private var read = 0, window: Int?, cost = 0.0
    func reset() { lock.withLock { (calls, inChars, outChars, read, cost) = (0, 0, 0, 0, 0) } }
    func count(_ u: LLMUsage) { lock.withLock { read = max(read, u.input); window = u.window ?? window; cost += u.cost ?? 0 } }
    /// « · read 11204 tokens of 1000000 · $0.0312», empty when the service counted nothing.
    var context: String {
        lock.withLock { read == 0 ? "" : " · read \(read) tokens" + (window.map { " of \($0)" } ?? "") + (cost > 0 ? String(format: " · $%.4f", cost) : "") }
    }
    func add(_ r: LLMRequest, _ out: String) {
        lock.withLock { calls += 1; inChars += r.messages.reduce(0) { $0 + $1.text.count }; outChars += out.count }
    }
    var totals: (Int, Int, Int) { lock.withLock { (calls, inChars, outChars) } }
}

struct CountingClient: LLMClient {
    let base: any LLMClient
    let counter: CallCounter
    func complete(_ r: LLMRequest) async throws -> String {
        let out = try await base.complete(r)
        counter.add(r, out)
        return out
    }
    func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let out = try await base.stream(r, partial: partial)
        counter.add(r, out)
        return out
    }
}
