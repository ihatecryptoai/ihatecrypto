import Foundation

/// What the signer agrees to sign as the owner of a NEAR Intents agent wallet (Agent API), read from the API's own
/// payload — never from what the app says about it. Each request is said in plain words for the confirmation, and
/// anything it can't say is refused.
public enum AgentOwner {
    /// The Agent API hosts a NEP-413 message may be addressed to.
    public static let hosts: Set<String> = ["api.demo.agentsonintents.com", "api.agentsonintents.com"]
    /// The custody contract the owner's delegate actions call (GET /v1/network: contract_id, 2026-11-02).
    public static let custody: Set<String> = ["outlayer.near"]
    /// NEP-413 owner requests the signer signs. An approval vote (to the custody contract) isn't one yet.
    static let messages: Set<String> = ["grant_issue", "grant_revoke", "execution_cancel"]
    /// The custody contract's methods an owner's delegate action may call, pinned from what the API was seen to ask
    /// (a live generate-intent, 2026-11-04): agent_create stores the wallet's first policy with `store_wallet_policy`
    /// (args: wallet_pubkey, encrypted_data, wallet_signature, agent_policy_hash, storage_refund_to).
    // and agent_freeze stores it again frozen, then calls `freeze_wallet` (2026-11-05), no NEAR attached.
    // ponytail: unfreeze/policy_update methods get pinned the same way when a live run first asks for them.
    public static let methods: Set<String> = ["store_wallet_policy", "freeze_wallet"]
    /// The NEAR a policy's storage may take from the owner's account (store_wallet_policy asked 0.01882 NEAR).
    static let maxStorage: UInt128 = 50_000_000_000_000_000_000_000   // 0.05 NEAR
    /// A grant lasts at most this long: a bearer token on the Mac shouldn't outlive a month.
    static let grantDays: Double = 31

    public struct Message: Equatable, Sendable {
        public let message: String
        public let nonce: Data
        public let recipient: String
        public let doing: String
    }

    /// A NEP-413 payload {message, recipient, nonce}: the API host, 32 random bytes, an owner request it can say.
    public static func message(_ payload: Data) throws -> Message {
        guard let p = try JSONSerialization.jsonObject(with: payload) as? [String: Any], let message = p["message"] as? String,
              let recipient = p["recipient"] as? String, let nonceText = p["nonce"] as? String, let nonce = Data(base64Encoded: nonceText) else {
            throw NearOwnerSigning.Problem("Not a NEP-413 request.")
        }
        guard hosts.contains(recipient) else { throw NearOwnerSigning.Problem("A message for «\(recipient)», not NEAR Intents' Agent API: not signed.") }
        guard nonce.count == 32 else { throw NearOwnerSigning.Problem("A NEP-413 nonce is 32 bytes.") }
        _ = try NearOwnerSigning.stringify(Data(message.utf8))   // refuses a key written twice
        // A grant names itself by its domain (live, 2026-11-04: «near-intents-agent-api.agent-grant.v6», no «type»).
        guard let m = try? JSONSerialization.jsonObject(with: Data(message.utf8)) as? [String: Any],
              let type = (m["domain"] as? String).flatMap(Self.type(domain:)) ?? m["type"] as? String else {
            throw NearOwnerSigning.Problem("The message isn't an Agent API request: not signed.")
        }
        guard messages.contains(type) else { throw NearOwnerSigning.Problem("The signer doesn't sign «\(type)» for an agent wallet.") }
        if let inner = m["recipient"] as? String, inner != recipient {
            throw NearOwnerSigning.Problem("The message names another host than it's addressed to: not signed.")
        }
        if type == "grant_issue" {
            let expires = (m["expires_at_ms"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue / 1000) }
                ?? (m["expires_at"] as? String).flatMap { ISO8601DateFormatter().date(from: $0) ?? Self.fractional.date(from: $0) }
            guard let expires, expires.timeIntervalSinceNow <= grantDays * 86400 else {
                throw NearOwnerSigning.Problem("A grant for longer than \(Int(grantDays)) days, or with no end: not signed.")
            }
        }
        return Message(message: message, nonce: nonce, recipient: recipient, doing: say(type, m))
    }

    public struct Delegate: Equatable, Sendable {
        public let receiver: String
        public let actions: [NearTransaction.Action]
        public let doing: String
    }

    /// A NEP-366 payload {receiverId, actions[{type: FunctionCall, params{methodName, args, gas, deposit}}]}: calls to
    /// the custody contract only, attaching no NEAR, the args signed byte for byte as JSON.stringify writes them.
    public static func delegate(_ payload: Data, allowed: Set<String> = methods) throws -> Delegate {
        guard let p = try JSONSerialization.jsonObject(with: payload) as? [String: Any], let receiver = p["receiverId"] as? String,
              let list = p["actions"] as? [[String: Any]], (1...2).contains(list.count) else {
            throw NearOwnerSigning.Problem("Not a NEP-366 request.")
        }
        guard custody.contains(receiver) else { throw NearOwnerSigning.Problem("A delegate action for «\(receiver)», not the agent wallets' custody: not signed.") }
        _ = try NearOwnerSigning.stringify(payload)   // refuses a key written twice anywhere in it
        // The raw text of each action's args, as the API wrote them: re-encoding would change the bytes it checks.
        let rawActions = try NearOwnerSigning.rawElements(try NearOwnerSigning.rawValue("actions", in: payload))
        guard rawActions.count == list.count else { throw NearOwnerSigning.Problem("The actions don't read the same twice: not signed.") }
        var actions = [NearTransaction.Action](), words = [String]()
        for (a, raw) in zip(list, rawActions) {
            guard a["type"] as? String == "FunctionCall", let params = a["params"] as? [String: Any], let method = params["methodName"] as? String,
                  let gas = UInt64(params["gas"] as? String ?? "\(params["gas"] ?? "")"),
                  let deposit = UInt128(params["deposit"] as? String ?? "\(params["deposit"] ?? "")") else {
                throw NearOwnerSigning.Problem("Only function calls are signed for an agent wallet.")
            }
            guard allowed.contains(method) else { throw NearOwnerSigning.Problem("The signer doesn't sign «\(method)» on the agent wallets' custody.") }
            // Only a policy's storage may cost NEAR, and only a little; it's said in the words the owner confirms.
            guard deposit <= 1 || (method == "store_wallet_policy" && deposit <= maxStorage) else {
                throw NearOwnerSigning.Problem("The call attaches \(near(deposit)) NEAR: not signed.")
            }
            guard gas <= 300_000_000_000_000 else { throw NearOwnerSigning.Problem("The call asks for more than 300 Tgas: not signed.") }
            let args = try NearOwnerSigning.stringify(try NearOwnerSigning.rawValue("args", in: try NearOwnerSigning.rawValue("params", in: raw)))
            actions.append(.functionCall(method: method, args: args, gas: gas, deposit: deposit))
            guard let parsed = (try? JSONSerialization.jsonObject(with: args)) as? [String: Any] else {
                throw NearOwnerSigning.Problem("The call's arguments can't be read: not signed.")
            }
            var said = sayCall(method, parsed)
            if deposit > 1 {
                // The policy is stored encrypted for the custody: the signer can't read it, so its storage is all it says.
                let refund = (parsed["storage_refund_to"] as? String).map { " (refunded to \($0.prefix(8))…, not to you)" } ?? ""
                said += ", paying \(near(deposit)) NEAR from your NEAR account for its storage" + refund
            }
            words.append(said)
        }
        return Delegate(receiver: receiver, actions: actions, doing: words.joined(separator: ", then "))
    }

    /// A call that stores rules is signed only beside the rules the signer checked (`policy`): the hash it carries
    /// (`agent_policy_hash`) is the custody's own — a live agent_create (2026-11-03) carried 2021a689… for rules the API
    /// hashed cd7ca4a9… — so it can't be recomputed here. The app binds the API's preview hash before and the applied
    /// hash after.
    public static func rulesShown(_ actions: [NearTransaction.Action], policy: Data?) throws {
        let stores = actions.contains { if case .functionCall("store_wallet_policy", _, _, _) = $0 { true } else { false } }
        if stores && policy == nil { throw NearOwnerSigning.Problem("The rules this call stores weren't shown to the signer: not signed.") }
    }

    /// yoctoNEAR as NEAR, to 5 decimals.
    static func near(_ yocto: UInt128) -> String {
        let micro = yocto / 10_000_000_000_000_000_000   // 1e19: hundred-thousandths of a NEAR
        let whole = micro / 100_000, frac = String(micro % 100_000)
        return "\(whole)." + String(repeating: "0", count: 5 - frac.count) + frac
    }

    nonisolated(unsafe) static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    /// An agent wallet's complete policy, checked by the signer before the owner signs anything that carries it (or its
    /// hash): money leaves only for addresses the signer itself knows — this wallet's own (`own`), ones confirmed in
    /// person (`known`) — or the owner's NEAR account; no native signing routes; nothing frozen-looking hides a change.
    /// Returns the policy in words for the prompt.
    public static func policy(_ json: Data, owner: String, own: Set<String>, known: Set<String>) throws -> String {
        _ = try NearOwnerSigning.stringify(json)   // a key written twice
        guard let p = try JSONSerialization.jsonObject(with: json) as? [String: Any] else { throw NearOwnerSigning.Problem("Not a policy.") }
        if let native = p["native"], !(native is NSNull) { throw NearOwnerSigning.Problem("The policy lets the agent sign on chains directly (native routes): not signed.") }
        guard let d = p["destinations"] as? [String: Any], d["mode"] as? String == "only", let list = d["list"] as? [[String: Any]] else {
            throw NearOwnerSigning.Problem("The policy lets the agent send to addresses you didn't list: not signed.")
        }
        func key(_ a: String) -> String { a.hasPrefix("0x") ? a.lowercased() : a }
        let allowed = Set((own.union(known)).map(key))
        for e in list {
            guard let address = e["address"] as? String else { throw NearOwnerSigning.Problem("A destination without an address: not signed.") }
            let ok = (e["action"] as? String == "transfer" && address == owner) || (e["action"] as? String == "withdraw" && allowed.contains(key(address)))
            guard ok else { throw NearOwnerSigning.Problem("The policy lets the agent pay \(address), an address this wallet hasn't confirmed: not signed.") }
        }
        let budget = (p["budget"] as? [String: Any])?["daily_usd"] as? String
        guard let budget, let daily = Decimal(string: budget), daily > 0 else { throw NearOwnerSigning.Problem("The policy has no daily cap: not signed.") }
        return "at most $\(budget) a day, only to \(list.count) address\(list.count == 1 ? "" : "es") of yours" + (p["frozen"] as? Bool == true ? ", frozen" : "")
    }

    /// The request a domain names: «near-intents-agent-api.agent-grant.v6» is a grant, whatever its version.
    static func type(domain: String) -> String? {
        domain.hasPrefix("near-intents-agent-api.agent-grant.") ? "grant_issue" : nil
    }

    /// The owner request in words, from its own fields.
    static func say(_ type: String, _ m: [String: Any]) -> String {
        let agent = (m["agent_id"] as? String).map { " \($0.prefix(8))…" } ?? ""
        switch type {
        case "grant_issue":
            let until = (m["expires_at_ms"] as? NSNumber).map { ISO8601DateFormatter().string(from: Date(timeIntervalSince1970: $0.doubleValue / 1000)) }
                ?? m["expires_at"] as? String
            return "let this Mac run agent wallet\(agent) within its rules" + (until.map { " until \($0.prefix(10))" } ?? "")
        case "grant_revoke": return "take back this Mac's access to agent wallet\(agent)"
        case "execution_cancel": return "cancel a queued move of agent wallet\(agent)"
        default: return type
        }
    }

    static func sayCall(_ method: String, _ args: [String: Any]) -> String {
        // The policy a call carries, when it carries one: the daily budget and where money may go.
        let policy = (args["policy"] as? [String: Any]) ?? args
        if method == "freeze_wallet" { return "freeze the agent wallet: nothing leaves it until you unfreeze it" }
        var parts = [method.replacingOccurrences(of: "_", with: " ")]
        if let budget = (policy["budget"] as? [String: Any])?["daily_usd"] as? String { parts.append("at most $\(budget) a day") }
        if let d = policy["destinations"] as? [String: Any], let mode = d["mode"] as? String {
            let n = (d["list"] as? [Any])?.count ?? 0
            parts.append(mode == "only" ? "only to \(n) address\(n == 1 ? "" : "es") you chose" : mode == "any" ? "to ANY address" : "to any address but \(n)")
        }
        if policy["frozen"] as? Bool == true { parts.append("frozen") }
        return parts.joined(separator: ", ")
    }
}
