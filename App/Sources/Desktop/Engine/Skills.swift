import Foundation
import IhatecryptoCore
import WalletCore

/// Skills (the built-in Trading study, and .md or .txt files the user adds), the study's journal and notes, and what
/// the user asked the chat to remember — the Mac's, over the same IhatecryptoCore. A skill changes how the assistant
/// works with the user, never the risk rules, the checks or the Confirm.
extension Engine {
    var skillsOn: [Skill] { (Skill.builtIn + addedSkills).filter { !skillsOff.contains($0.id) } }

    static var studyFolder: URL {
        let folder = Store.url("study")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// A skill from a file's text the window read (its own file dialog).
    func addSkill(_ text: String, fileName: String) throws {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw WalletErrorText(say("The file is empty.", "Файл пустой.")) }
        var s = Skill.parse(String(text.prefix(60_000)), fileName: fileName)
        if let i = addedSkills.firstIndex(where: { $0.id == s.id }) { addedSkills[i] = s; return }
        let base = s.id
        var n = 2
        while Skill.builtIn.contains(where: { $0.id == s.id }) { s.id = "\(base)-\(n)"; n += 1 }
        addedSkills.append(s)
    }

    /// The person's last own message: a scheduled run's request on its canvas (note «schedule») is the task's steps, which
    /// a model may have written, so a «remember» or a journal entry in it is never kept as theirs.
    var ownWords: String { messages.last { $0.fromUser && $0.note != "schedule" }?.text ?? "" }

    func memoryCommand(_ c: MemoryCommand, note: String?) {
        switch c {
        case .remember(let words):
            // Only words of their own last message: a page, a note or a token's name can't plant a preference.
            guard MemoryCommand.inTheirWords(words, ownWords) else {
                reply(say("I keep only your own words. Write «remember: …» and I'll keep exactly that.",
                          "Запоминаю только твои собственные слова. Напиши «запомни: …» — сохраню ровно это."), note: note)
                return
            }
            let kept = String(words.prefix(300))
            memory = memory.filter { $0.lowercased() != kept.lowercased() } + [kept]
            reply(say("I'll keep in mind: «\(kept)». «What do you remember» lists it; «forget …» drops it.",
                      "Запомнил: «\(kept)». «Что ты помнишь» — весь список; «забудь …» — убрать."), note: note)
        case .forget(let words):
            let before = memory.count, w = words.lowercased()
            memory = w == "all" ? [] : memory.filter { !$0.lowercased().contains(w) }
            reply(before == memory.count ? say("Nothing I keep has those words.", "Среди запомненного таких слов нет.") : say("Forgotten.", "Забыл."), note: note)
        case .list:
            reply(memory.isEmpty ? say("I don't keep anything yet: say «remember: …».", "Пока ничего не запомнено: скажи «запомни: …».")
                  : say("I keep in mind:\n", "Помню:\n") + memory.enumerated().map { "\($0.offset + 1). \($0.element)" }.joined(separator: "\n"), note: note)
        }
    }

    func journalCommand(_ j: JournalCommand, note: String?) async {
        guard MemoryCommand.inTheirWords(j.words, ownWords) else {
            reply(say("The journal keeps only your own words: write the entry yourself and I'll save exactly that.",
                      "В журнал записываю только твои слова: напиши запись сам — сохраню ровно её."), note: note)
            return
        }
        let coin = IntentParser.namedAsset(j.asset, markets: Set(markets.keys)) ?? j.asset
        var e = JournalEntry(at: Date(), asset: coin, words: String(j.words.prefix(2000)))
        if let m = markets[coin] {
            e.price = m.markPx; e.funding = m.funding; e.change24h = m.change24h
            e.volume24h = m.volume24h > 0 ? m.volume24h : nil
        }
        e.openInterestUSD = await JournalEntry.openInterestUSD(coin: coin)
        e.market = ["BTC", "ETH"].filter { $0 != coin }.compactMap { c in
            markets[c].map { "\(c) \(Fmt.price($0.markPx))" + ($0.change24h.map { " (24h \(signed($0)))" } ?? "") }
        }
        if let p = positions.first(where: { $0.coin == coin }) {
            e.position = "\(p.size > 0 ? "long" : "short") \(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)), unrealized \(signedUSD(p.unrealizedPnl))"
        }
        let entries = Store.load([JournalEntry].self, "journal.json") ?? []
        Store.save(entries + [e], "journal.json")
        reply(say("Saved to your journal — \(IntentParser.pretty(coin)): \(e.snapshot).", "Записал в журнал — \(IntentParser.pretty(coin)): \(e.snapshot)."), note: note)
    }

    func studyNote(_ n: StudyNote, note: String?) {
        let url = Self.studyFolder.appendingPathComponent("\(n.file).md")
        let entry = "\n## \(Fmt.date(Date()))\n\(n.text)\n"
        if let h = try? FileHandle(forWritingTo: url) {
            h.seekToEndOfFile()
            h.write(Data(entry.utf8))
            try? h.close()
        } else {
            try? Data(("# \(n.file)\n" + entry).utf8).write(to: url)
        }
        reply(say("Noted in the study's \(n.file).", "Записал в \(n.file) исследования."), note: note)
    }

    /// The skills' lookups: a skill's whole text, an account's trades rebuilt, the journal, the study's notes.
    func skillLookup(_ l: ChatRouter.Lookup) async -> String {
        switch l.tool {
        case "skill":
            guard let s = skillsOn.first(where: { $0.id == Skill.slug(l.arg) || $0.name.lowercased() == l.arg.lowercased() }) else {
                return "No switched-on skill «\(l.arg)». Switched on: " + (skillsOn.isEmpty ? "none" : skillsOn.map(\.id).joined(separator: ", ")) + "."
            }
            if !loadedSkills.contains(s.id) { loadedSkills.append(s.id) }
            return "Skill «\(s.name)», loaded on this chat from now on — follow it:\n" + s.text
        case "trades":
            let asked = l.arg.trimmingCharacters(in: .whitespaces)
            let me = asked.isEmpty || asked.lowercased() == "me"
            guard let address = me ? evmAddress : asked, AddressCheck.problem(address, network: .base) == nil else {
                return "trades takes a Hyperliquid account's 0x address, or me: «\(asked)» isn't one."
            }
            async let fills = TradeHistory.allFills(user: address)
            async let funding = TradeHistory.allFunding(user: address)
            guard let all = try? await fills else { return "Hyperliquid didn't answer for \(me ? "this wallet" : address); try again in a minute." }
            let stories = TradeHistory.stories(all)
            let trades = TradeStudy.trades(stories, funding: (try? await funding) ?? [])
            return (me ? "The wallet in ihatecrypto" : "Account \(address)") + ": \(all.count) fills"
                + (all.count >= 10_000 ? " (Hyperliquid serves only the last 10,000: older trades aren't in it)" : "") + ".\n"
                + TradeStudy.report(trades, open: stories.filter(\.isOpen).count, cut: TradeStudy.cut(stories))
        case "journal":
            let entries = Store.load([JournalEntry].self, "journal.json") ?? []
            guard !entries.isEmpty else { return "The journal is empty: nothing was written with the journal command yet." }
            return "The journal, oldest first\(entries.count > 40 ? " (the last 40 of \(entries.count))" : ""):\n"
                + entries.suffix(40).map { $0.review(now: markets[$0.asset]?.markPx) }.joined(separator: "\n")
        default:   // study
            let one = StudyNote.parse("study \(l.arg): x")?.file
            let notes = StudyNote.files.map(\.value).filter { one == nil || $0 == one }.compactMap { file -> String? in
                guard let text = try? String(contentsOf: Self.studyFolder.appendingPathComponent("\(file).md"), encoding: .utf8) else { return nil }
                let keep = one == nil ? 1200 : 6000
                return "\(file):\n" + (text.count > keep ? "…" + text.suffix(keep) : text)
            }
            return notes.isEmpty ? "No study notes\(one.map { " in \($0)" } ?? "") yet." : notes.joined(separator: "\n\n")
        }
    }
}

/// A plain error with its text, for the window's toast.
nonisolated struct WalletErrorText: LocalizedError {
    let text: String
    init(_ text: String) { self.text = text }
    var errorDescription: String? { text }
}

extension Engine {
    /// The study's journal is kept: Settings › Skills' «Journal» opens it (greyed out before the first entry, as on the Mac).
    static var journalKept: Bool { FileManager.default.fileExists(atPath: Store.url("journal.json").path) }

    /// Settings › Skills: the study's notes folder, or its journal, in Explorer (the Mac opens them in Finder). Only these
    /// two fixed paths, never one the page names.
    func openStudyFiles(journal: Bool) throws {
        guard !journal || Self.journalKept else { throw WalletErrorText(say("No journal entries yet.", "В журнале пока нет записей.")) }
        let target = journal ? Store.url("journal.json") : Self.studyFolder
        let path = target.withUnsafeFileSystemRepresentation { $0.map { String(cString: $0) } } ?? target.path
        let p = Process()
        #if os(Windows)
        p.executableURL = URL(fileURLWithPath: (ProcessInfo.processInfo.environment["SystemRoot"] ?? "C:\\Windows") + "\\explorer.exe")
        #else
        p.executableURL = URL(fileURLWithPath: "/usr/bin/open")
        #endif
        p.arguments = [path]
        try p.run()
    }
}
