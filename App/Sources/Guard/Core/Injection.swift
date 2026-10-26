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
        #"(you\s+are\s+now|new\s+instructions|do\s+not\s+tell\s+the\s+user|don't\s+tell\s+the\s+user)"#,
        #"(игнорируй|забудь|отмени)\s+(все\s+|всё\s+|предыдущие\s+|прежние\s+)*(инструкции|правила|указания)"#,
        #"(системн\w+\s+(промпт\w*|сообщени\w+)\s*:|нов\w+\s+системн\w+\s+промпт|новые\s+инструкции|ты\s+теперь|не\s+(говори|сообщай)\s+пользователю)"#,
    ]
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

    public static func screen(_ text: String, source: String, classifier: (any TextClassifier)?) async -> Verdict {
        var v = Verdict()
        let f = InjectionHeuristics.findings(text)
        if !f.commands.isEmpty {
            v.problems.append("The text has commands for the assistant: “\(f.commands.joined(separator: "”, “"))”.")
        }
        if !f.addresses.isEmpty {
            if f.transferVerb, let to = f.recipients.first {
                v.problems.append("The text asks to send funds to \(to). Addresses from text never become recipients.")
            } else if f.transferVerb {
                v.checks.append("The address names the token to send; only a saved name receives it.")
            } else {
                v.checks.append("The text has an address; it will not become a payment recipient.")
            }
        }
        guard let classifier else {
            v.checks.append("Checked by local rules.")
            return v
        }
        do {
            let (label, confidence) = try await classifier.classify(text, source: source)
            let percent = Int(confidence * 100)
            switch label {
            case "data":
                v.checks.append("\(classifier.name): looks like ordinary data (\(percent)%).")
            case "instructions" where confidence >= threshold:
                v.problems.append("\(classifier.name): the text tries to steer the assistant (\(percent)%).")
            case "payment_redirect" where confidence >= threshold:
                v.problems.append("\(classifier.name): the text tries to redirect funds to its own address (\(percent)%).")
            default:
                v.checks.append("\(classifier.name) is unsure: \(label) (\(percent)%). Local rules decide.")
            }
        } catch {
            v.checks.append("\(classifier.name) is unavailable (\(error.localizedDescription)); local rules apply.")
        }
        return v
    }

    /// What a lookup brought back for the chat's model — a token's listing, a page, a service's docs, a contract's
    /// string — before the model reads it: nil when it is plain data, else why it is withheld. The local rules look for
    /// commands to the assistant only (an address beside «deposit» is what such texts are made of, so that rule stays
    /// with the chat); the classifier adds what is said in other words. Whatever is decided here, an address a lookup
    /// finds never becomes a recipient.
    public static func lookupProblem(_ text: String, source: String, classifier: (any TextClassifier)?) async -> String? {
        let commands = InjectionHeuristics.findings(text).commands
        if !commands.isEmpty { return "it has commands for the assistant: “\(commands.joined(separator: "”, “"))”" }
        guard let classifier, let (label, confidence) = try? await classifier.classify(text, source: source), label != "data", confidence >= threshold else { return nil }
        let percent = Int(confidence * 100)
        return label == "instructions" ? "it tries to steer the assistant (\(classifier.name.lowercased()), \(percent)%)"
            : "it pushes funds to an address of its own (\(classifier.name.lowercased()), \(percent)%)"
    }
}
