import Foundation

/// Screening of text that came from outside (a pasted message, a web page, token metadata, an API reply)
/// before any model sees it. This is where prompt injection lives. Local rules run always; a classifier
/// such as Jev can add problems but can never clear one, and nothing here approves a transaction:
/// the transaction guard still checks every signature on its own.
public enum InjectionHeuristics {
    static let commandPatterns = [
        #"ignore\s+(all\s+|any\s+|the\s+)?(previous|prior|above|earlier)\s+(instructions|rules|messages|prompts)"#,
        #"disregard\s+(all\s+|the\s+|any\s+)?(previous|prior|above|earlier)"#,
        // A prompt is named as a header or a tag that opens new orders («SYSTEM PROMPT: …», «[system]», «new system prompt»);
        // a question about it («what's the system prompt size?») is not a command.
        #"(system|developer)\s+(prompt|message)\s*[:>\]]|[\[<]\s*/?\s*(system|developer)\s*[\]>]|(new|override|updated)\s+(system|developer)\s+(prompt|message)|developer\s+mode"#,
        #"(new\s+instructions|do\s+not\s+tell\s+the\s+user|don't\s+tell\s+the\s+user)"#,
        // «you are now» and «ты теперь» only with a new role or mode: the user's own «ты теперь всегда отвечай по-русски»,
        // «you are now my trading buddy» are wishes, and «киты теперь продают ETH?» held «ты теперь» before the word
        // boundary. Trading words stay out: support (a price level), AI tokens, «торговых ботов». Measured 2026-10-27:
        // none of the 280 own messages of Tools/jevcheck/screens.py stopped, all 7 role switches still stopped. A switch in
        // other words is Jev's to catch (it reads every message); a lookup's result still stops on any of the two (newRole).
        #"\byou\s+are\s+now\b[^.!?\n]{0,40}?(agent|assistant|\bbot\b|\bai\b(?![\s-]*tokens?)|model|admin|developer|mode\b|\bdan\b|unrestricted|unfiltered|uncensored|jailbr|without\s+(limits|rules|checks|restrictions|filters|confirm))\w*"#,
        #"(игнорируй|забудь|отмени)\s+(все\s+|всё\s+|предыдущие\s+|прежние\s+)*(инструкции|правила|указания)"#,
        #"(системн\w+\s+(промпт\w*|сообщени\w+)\s*:|нов\w+\s+системн\w+\s+промпт|новые\s+инструкции|не\s+(говори|сообщай)\s+пользователю)"#,
        #"\bты\s+теперь\b[^.!?\n]{0,40}?(агент|ассистент|\bбот(ом)?\b|\bии\b(?![\s-]*токен)|модел|админ|разработчик|режим|свобод|\bdan\b|jailbr|джейлбр|без\s+(огранич|правил|провер|подтвержд|вопрос|фильтр|цензур)|не\s+обязан)\w*"#,
    ]
    /// What a lookup brings back speaks to no one: «you are now…», «ты теперь…» there is said to the model reading it.
    static let newRole = #"\b(you\s+are\s+now|ты\s+теперь)\b"#
    static let transferVerbs = #"(send|transfer|withdraw|bridge|deposit|approve|pay|отправ\w*|перевед\w*|перевест\w*|переведи|выведи|вывест\w*|скинь|пополн\w*|оплат\w*)"#
    static let evmAddress = #"0x[0-9a-fA-F]{40}"#
    static let solanaAddress = #"[1-9A-HJ-NP-Za-km-z]{32,44}"#
    /// «send all 0x… on base to Bybit»: an address right after the verb (and the amount) is the token sent, when a name
    /// after «to» receives it. Group 1 the token, group 2 the receiver.
    static let tokenSlot = #"(?i)(?:send|transfer|отправ\w*|перевед\w*|перевести|скинь|кинь)\s+(?:(?:all|весь|всё|все|\d+(?:[.,]\d+)?)\s+)?(?:(?:the|my|мои|мой)\s+)?(?:tokens?\s+|токен\w*\s+)?("#
        + evmAddress + "|" + solanaAddress + #")(?:\s+tokens?)?(?:\s+(?:on|in|в|на)\s+\S+)?\s+(?:to|на|для)\s+[«"“]?(\S+)"#

    static func all(_ pattern: String, in text: String) -> [String] {
        guard let regex = try? Regex(pattern) else { return [] }
        return text.matches(of: regex).map { String(text[$0.range]) }
    }

    public struct Findings: Sendable, Equatable {
        public var commands: [String] = []
        public var addresses: [String] = []
        public var transferVerb = false
        /// Addresses that name the token sent, not who gets it.
        public var tokens: [String] = []
        /// The addresses that would receive the money.
        public var recipients: [String] { addresses.filter { !tokens.contains($0) } }
    }

    public static func findings(_ text: String) -> Findings {
        var f = Findings()
        let lower = text.lowercased()
        for pattern in commandPatterns { f.commands += all(pattern, in: lower).prefix(1) }
        f.addresses = all(evmAddress, in: text)
        // A Solana address mixes digits, upper and lower case; plain words of that length do not.
        let evm = f.addresses
        f.addresses += all(solanaAddress, in: text).filter { s in
            s.contains(where: \.isNumber) && s.contains(where: \.isUppercase) && s.contains(where: \.isLowercase)
                && !evm.contains(where: { $0.contains(s) })
        }
        f.transferVerb = !all(transferVerbs, in: lower).isEmpty
        if let regex = try? Regex(tokenSlot) {
            for m in text.matches(of: regex) {
                guard let token = m.output[1].substring.map(String.init), let to = m.output[2].substring.map(String.init),
                      !f.addresses.contains(where: { to.contains($0) }) else { continue }
                f.tokens.append(token)
            }
        }
        return f
    }
}

public protocol TextClassifier: Sendable {
    var name: String { get }
    /// label: "data", "instructions" or "payment_redirect"; confidence 0…1: how likely the text is plain data when
    /// the label is «data», how likely it isn't when the label is one of the other two.
    func classify(_ text: String, source: String) async throws -> (label: String, confidence: Double)
}

public enum InjectionGuard {
    public static let threshold = 0.7

    /// Why a text was stopped, as data: the problems' English stays for logs and the terminal, the chat says these in
    /// its own language.
    public enum Stop: Sendable, Equatable {
        /// Commands to the assistant the local rules found, as written.
        case commands([String])
        /// The address from the text the money would go to.
        case sendsTo(String)
        /// The classifier, in percent: someone else's orders to the assistant.
        case steers(Int)
        /// The classifier, in percent: funds pushed to the text's own address.
        case redirects(Int)
    }

    public struct Screen: Sendable {
        public var verdict = Verdict()
        /// One for each problem.
        public var stops: [Stop] = []
        public var ok: Bool { verdict.ok }
        public var checks: [String] { verdict.checks }
        public var problems: [String] { verdict.problems }
    }

    public static func screen(_ text: String, source: String, classifier: (any TextClassifier)?) async -> Screen {
        var s = Screen()
        let f = InjectionHeuristics.findings(text)
        if !f.commands.isEmpty {
            s.verdict.problems.append("The text has commands for the assistant: “\(f.commands.joined(separator: "”, “"))”.")
            s.stops.append(.commands(f.commands))
        }
        if !f.addresses.isEmpty {
            if f.transferVerb, let to = f.recipients.first {
                s.verdict.problems.append("The text asks to send funds to \(to). Addresses from text never become recipients.")
                s.stops.append(.sendsTo(to))
            } else if f.transferVerb {
                s.verdict.checks.append("The address names the token to send; only a saved name receives it.")
            } else {
                s.verdict.checks.append("The text has an address; it will not become a payment recipient.")
            }
        }
        guard let classifier else {
            s.verdict.checks.append("Checked by local rules.")
            return s
        }
        do {
            let (label, confidence) = try await classifier.classify(text, source: source)
            let percent = Int(confidence * 100)
            switch label {
            case "data":
                s.verdict.checks.append("\(classifier.name): looks like ordinary data (\(percent)%).")
            case "instructions" where confidence >= threshold:
                s.verdict.problems.append("\(classifier.name): the text tries to steer the assistant (\(percent)%).")
                s.stops.append(.steers(percent))
            case "payment_redirect" where confidence >= threshold:
                s.verdict.problems.append("\(classifier.name): the text tries to redirect funds to its own address (\(percent)%).")
                s.stops.append(.redirects(percent))
            default:
                s.verdict.checks.append("\(classifier.name) is unsure: \(label) (\(percent)%). Local rules decide.")
            }
        } catch {
            s.verdict.checks.append("\(classifier.name) is unavailable (\(error.localizedDescription)); local rules apply.")
        }
        return s
    }

    /// What a lookup brought back for the chat's model — a token's listing, a page, a service's docs, a contract's
    /// string — before the model reads it: nil when it is plain data, else why it is withheld. The local rules look for
    /// commands to the assistant only (an address beside «deposit» is what such texts are made of, so that rule stays
    /// with the chat); the classifier adds what is said in other words. Whatever is decided here, an address a lookup
    /// finds never becomes a recipient.
    public static func lookupProblem(_ text: String, source: String, classifier: (any TextClassifier)?) async -> String? {
        let commands = InjectionHeuristics.findings(text).commands + InjectionHeuristics.all(InjectionHeuristics.newRole, in: text.lowercased()).prefix(1)
        if !commands.isEmpty { return "it has commands for the assistant: “\(commands.joined(separator: "”, “"))”" }
        guard let classifier, let (label, confidence) = try? await classifier.classify(text, source: source), label != "data", confidence >= threshold else { return nil }
        let percent = Int(confidence * 100)
        return label == "instructions" ? "it tries to steer the assistant (\(classifier.name.lowercased()), \(percent)%)"
            : "it pushes funds to an address of its own (\(classifier.name.lowercased()), \(percent)%)"
    }
}
