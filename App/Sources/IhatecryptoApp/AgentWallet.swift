import AgentAPIKit
import CryptoKit
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import SignerAPI
import SwiftUI
import TradeExecution
import WalletCore

/// Settings › General: the Agent API partner key that NEAR Intents' agent wallets need. Saved by the app itself, so
/// macOS never asks whether the app may read it.
struct AgentKeySection: View {
    /// Read from the Keychain when shown, not on every redraw of Settings.
    @State private var set = false
    @State private var adding = false

    var body: some View {
        SettingsSection(icon: "person.badge.key", title: "Agent wallet key", line: "An agent wallet on NEAR Intents: tasks spend within rules you sign.") {
            SettingsGroup {
                SettingsRow(set ? "Agent API: your partner key" : "Agent API: no key", icon: "person.badge.key", logo: "near",
                            info: "NEAR Intents keeps an agent wallet under a policy your NEAR key signs — what it may do, how much a day, to which addresses — and refuses anything else. The partner key only lets this app talk to that service; it can't move money.") {
                    HStack(spacing: 8) {
                        if set { Button("Remove") { NodeStore.agentKey = nil; set = false } }
                        Button(set ? "Change…" : "Add your key…") { adding = true }
                    }
                }
            }
        }
        .sheet(isPresented: $adding) { AgentKeySheet { set = true } }
        .onAppear { set = NodeStore.agentKey != nil }
    }
}

struct AgentKeySheet: View {
    var saved: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var key = ""
    @State private var checking = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                TokenLogo(name: "near", size: 28).clipShape(RoundedRectangle(cornerRadius: 7))
                Text("Agent API partner key").font(.ui(20, .bold))
            }
            Text("The key lets this app create and run agent wallets on NEAR Intents. It can't move money or sign anything: every rule is signed by your own NEAR key, after Confirm. It's kept in this Mac's Keychain.")
                .font(.ui(12.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Link("Open the partner dashboard", destination: URL(string: "https://partners.demo.agentsonintents.com/dashboard")!).font(.ui(12.5))
            SecureField("naa_…", text: $key)
                .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                .onSubmit { Task { await check() } }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").font(.ui(12)).foregroundStyle(Brand.loss)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                if checking { ProgressView().controlSize(.small) }
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Check and save") { Task { await check() } }
                    .keyboardShortcut(.defaultAction).disabled(checking || key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24).frame(width: 520)
    }

    private func check() async {
        let token = key.trimmingCharacters(in: .whitespacesAndNewlines)
        checking = true
        defer { checking = false }
        problem = await AgentAPI.problem(withKey: token)
        guard problem == nil else { return }
        NodeStore.agentKey = token
        saved()
        dismiss()
    }
}

/// The agent wallet this wallet's NEAR key owns on NEAR Intents (one at most): its id, the agent's own NEAR Intents
/// account, the daily cap the owner signed and the grant this Mac acts with. The grant token itself is in the Keychain.
nonisolated struct AgentWalletRecord: Codable, Equatable, Sendable {
    var agentID: String
    var account: String?
    var owner: String
    var dailyUSD: Decimal
    var grantID: String?
    var frozen = false
    var created = Date()
}

extension Workspace {
    static let agentUSDC = (solana: OneClick.Asset.solanaUSDC, base: OneClick.Asset.usdc(onEVM: "base")!)
    /// USDC in its 6-decimal units, rounded down.
    static func agentUnits(_ usdc: Decimal) -> UInt64 { NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value }

    /// The owner: this wallet's NEAR implicit account and its key, as the Agent API names them.
    func agentOwner() async throws -> (account: String, publicKey: String) {
        guard let account = try await SignerClient().send(.nearAddress).nearAddress, let raw = Data(hex: account), raw.count == 32 else {
            throw ExecutionError("The signer has no NEAR account for this wallet.")
        }
        return (account, "ed25519:" + Base58.encode(raw))
    }

    /// The agent wallet, when this wallet's NEAR key owns it: another wallet's agent is never funded or run from here.
    func ownAgent() async throws -> (record: AgentWalletRecord, owner: (account: String, publicKey: String)) {
        guard let a = agentWallet else { throw ExecutionError("There's no agent wallet yet: “create an agent wallet with $20 a day”.") }
        let owner = try await agentOwner()
        guard a.owner == owner.account else { throw ExecutionError("The agent wallet belongs to another wallet's NEAR key: switch back to that wallet to use it.") }
        return (a, owner)
    }

    /// Where the agent may send USDC: this wallet's own Solana and Base addresses, the Solana and EVM addresses saved
    /// and confirmed in Addresses, and the owner's NEAR account — nothing else (destinations mode «only»).
    func agentDestinations(owner: String) -> [[String: Any]] {
        var list: [[String: Any]] = [["action": "transfer", "address": owner, "confidential": false]]
        var seen = Set<String>()
        func add(_ chain: String, _ address: String) {
            let a = chain == "base" ? address.lowercased() : address
            guard seen.insert(chain + a).inserted else { return }
            list.append(["action": "withdraw", "chain": chain, "address": a, "memo": NSNull()])
        }
        if !solanaAddress.isEmpty { add("sol", solanaAddress) }
        if let evm = previewWallet?.evm { add("base", evm) }
        for e in addressBook where confirmedAddresses.contains(where: { $0.address == e.address }) {
            if e.network == .solana { add("sol", e.address) } else if isEVMAddress(e.address) { add("base", e.address) }
        }
        return list
    }

    func agentPolicy(dailyUSD: Decimal, owner: String, frozen: Bool = false) -> [String: Any] {
        let cap = "\(Self.agentUnits(dailyUSD))"
        let usdc = [Self.agentUSDC.solana, Self.agentUSDC.base]
        return ["frozen": frozen, "actions": ["swap", "transfer", "withdraw"], "confidential": false, "owner_approval": false,
                "assets": usdc,
                "limits": ["per_transaction": Dictionary(uniqueKeysWithValues: usdc.map { ($0, cap) }), "hourly": [String: String](),
                           "daily": Dictionary(uniqueKeysWithValues: usdc.map { ($0, cap) }), "monthly": [String: String]()],
                "max_actions_per_hour": 10,
                "destinations": ["mode": "only", "list": agentDestinations(owner: owner)],
                "budget": ["daily_usd": NSDecimalNumber(decimal: dailyUSD).stringValue, "weekly_usd": NSNull(), "monthly_usd": NSNull()],
                "timelock_ms": 0]
    }

    /// The owner's signature on what the API generated, as `signed_data`: the signer reads the payload itself.
    func agentOwnerSigned(_ g: AgentAPI.Generated, owner: (account: String, publicKey: String), policy: Data? = nil) async throws -> Data {
        switch g.standard {
        case "nep413":
            let signature = try await XPCTradeSigner().signAgentMessage(payload: g.payload)
            return Data(#"{"standard":"nep413","payload":"#.utf8) + g.payload
                + AgentAPI.json(["public_key": owner.publicKey, "signature": signature.base64EncodedString()]).dropFirst().prefixingComma()
        case "nep366":
            let context = try await Near.delegateContext(account: owner.account)
            let signed = try await XPCTradeSigner().signAgentDelegate(payload: g.payload, nonce: context.nonce, maxBlockHeight: context.maxBlockHeight, policy: policy)
            return Data(#"{"standard":"nep366","payload":"#.utf8) + g.payload
                + AgentAPI.json(["signed_delegate": signed.base64EncodedString()]).dropFirst().prefixingComma()
        default: throw ExecutionError("The Agent API asked for a «\(g.standard)» signature; this wallet signs NEAR's.")
        }
    }

    /// Generated, signed by the owner, sent, and waited on until final.
    func agentOwnerIntent(_ body: [String: Any], owner: (account: String, publicKey: String)) async throws -> AgentAPI.Status {
        let json = AgentAPI.json(body), type = body["type"] as? String ?? "intent"
        let g = try await AgentAPI.generate(json, idempotency: "\(type)-\(UUID().uuidString.lowercased())")
        // What comes back to sign must be what was asked: the type, this owner, the same agent.
        guard g.type == type, g.signerAccount == nil || g.signerAccount == owner.account,
              body["agent_id"] == nil || g.agentID == nil || g.agentID == body["agent_id"] as? String else {
            throw ExecutionError("The Agent API answered with a different request than the one asked. Nothing was signed.")
        }
        if type == "grant_issue", let credential = body["credential"] as? String {
            let message = (try? JSONSerialization.jsonObject(with: g.payload) as? [String: Any])?["message"] as? String ?? ""
            let m = (try? JSONSerialization.jsonObject(with: Data(message.utf8))) as? [String: Any] ?? [:]
            guard (m["credential"] as? String ?? m["credential_hash"] as? String) == credential else {
                throw ExecutionError("The grant to sign isn't for this Mac's token. Nothing was signed.")
            }
        }
        // The policy goes on chain encrypted for the custody, so the signer can't read it there: what it checks is the
        // policy the API says it installs (its preview), which must be the one asked, under the hash the API names.
        var policy: Data?
        if let asked = (body["policy"] as? [String: Any]).map(AgentAPI.json) {
            guard let shown = g.previewPolicy, let hash = g.previewHash, AgentAPI.policyHash(shown) == hash,
                  AgentAPI.policyHash(asked) == hash else {
                throw ExecutionError("The Agent API would install other rules than the ones asked. Nothing was signed.")
            }
            policy = shown
        }
        // A freeze or unfreeze stores the same rules again with only «frozen» flipped (a live agent_freeze, 2026-11-05):
        // the rules it shows must be frozen as asked and within today's cap, and the signer checks them like new ones.
        if policy == nil, type == "agent_freeze" || type == "agent_unfreeze" {
            guard let shown = g.previewPolicy, let hash = g.previewHash, AgentAPI.policyHash(shown) == hash,
                  let p = (try? JSONSerialization.jsonObject(with: shown)) as? [String: Any], p["frozen"] as? Bool == (type == "agent_freeze"),
                  let cap = ((p["budget"] as? [String: Any])?["daily_usd"] as? String).flatMap({ Decimal(string: $0) }),
                  let today = agentWallet?.dailyUSD, cap <= today else {
                throw ExecutionError("The Agent API would install other rules with this \(type == "agent_freeze" ? "freeze" : "unfreeze"). Nothing was signed.")
            }
            policy = shown
        }
        try await AgentAPI.submit(g, signedData: try await agentOwnerSigned(g, owner: owner, policy: policy))
        let s = try await AgentAPI.final(g.correlationID)
        guard s.ok else { throw ExecutionError("NEAR Intents' Agent API: \(g.type.replacingOccurrences(of: "_", with: " ")) ended \(s.status).") }
        // And the policy that went live is that one.
        if let hash = g.previewHash, policy != nil,
           let applied = (s.details.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any])?["policy_hash"] as? String, applied != hash {
            throw ExecutionError("NEAR Intents applied rules with another hash (\(applied.prefix(8))…) than the ones signed (\(hash.prefix(8))…).")
        }
        return s
    }

    func agentCreate(dailyUSD: Decimal) async throws -> String {
        try NodeStore.agentAPIReady()
        let owner = try await agentOwner()
        if let a = agentWallet {
            // Made, but its grant never came (a failed signature): the same request finishes it, nothing is made twice.
            guard a.owner == owner.account, NodeStore.agentGrant == nil else {
                throw ExecutionError("There's already an agent wallet (\(a.agentID.prefix(8))…) with $\(Fmt.num(a.dailyUSD)) a day.")
            }
            try await agentGrant()
            return "Agent wallet \(a.agentID.prefix(8))… was already made; this Mac now has its grant: at most $\(Fmt.num(a.dailyUSD)) a day, within your rules."
        }
        let s = try await agentOwnerIntent(["type": "agent_create", "name": "ihatecrypto", "owner": ["type": "near", "account_id": owner.account, "public_key": owner.publicKey],
                                            "policy": agentPolicy(dailyUSD: dailyUSD, owner: owner.account)], owner: owner)
        let details = s.details.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
        guard let id = details["agent_id"] as? String ?? (details["agent"] as? [String: Any])?["agent_id"] as? String else {
            throw ExecutionError("The Agent API created no agent id.")
        }
        agentWallet = AgentWalletRecord(agentID: id, owner: owner.account, dailyUSD: dailyUSD)
        // The policy goes live on chain a moment later; the agent's own account comes with it.
        for _ in 0..<20 {
            let agent = (try? JSONSerialization.jsonObject(with: try await AgentAPI.call("GET", "/v1/agents/\(id)"))) as? [String: Any]
            if let account = (agent?["wallet"] as? [String: Any])?["near_account_id"] as? String { agentWallet?.account = account; break }
            try await Task.sleep(for: .seconds(3))
        }
        try await agentGrant()
        return "Agent wallet \(id.prefix(8))… is ready: at most $\(Fmt.num(dailyUSD)) a day, USDC only, to your own and saved addresses — NEAR Intents refuses anything else. Fund it with “fund the agent wallet with 5 USDC”."
    }

    /// A grant for this Mac: a random token whose sha256 the owner signs; the token stays in the Keychain.
    func agentGrant() async throws {
        try NodeStore.agentAPIReady()
        let (a, owner) = try await ownAgent()
        var random = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, 32, &random) == errSecSuccess else { throw ExecutionError("No randomness for the grant.") }
        let token = "ngt_" + Data(random).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        let commitment = Data(SHA256.hash(data: Data(token.utf8))).hex
        let expires = ISO8601DateFormatter().string(from: Date().addingTimeInterval(30 * 86400))
        let s = try await agentOwnerIntent(["type": "grant_issue", "agent_id": a.agentID, "label": "ihatecrypto on this Mac", "credential": commitment, "expires_at": expires], owner: owner)
        let details = s.details.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
        NodeStore.agentGrant = token
        agentWallet?.grantID = (details["grant"] as? [String: Any])?["grant_id"] as? String
    }

    func agentFund(_ usdc: Decimal) async throws -> String {
        try NodeStore.agentAPIReady()
        let (a, _) = try await ownAgent()
        guard let account = a.account else { throw ExecutionError("The agent wallet's own account isn't known yet; try again in a minute.") }
        guard let wallet = previewWallet else { throw ExecutionError("No wallet.") }
        let units = Self.agentUnits(usdc)
        let data = try await AgentAPI.call("POST", "/v1/agents/\(a.agentID)/deposit", body: AgentAPI.json(["origin_asset": Self.agentUSDC.solana, "amount": "\(units)"]),
                                           idempotency: "deposit-\(UUID().uuidString.lowercased())")
        let o = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
        guard let details = o["details"] as? [String: Any], let deposit = details["deposit_address"] as? String, let op = o["correlation_id"] as? String else {
            throw ExecutionError("The Agent API gave no deposit address. Nothing was sent.")
        }
        if let min = (details["min_amount"] as? String).flatMap(UInt64.init), units < min {
            throw ExecutionError("The agent wallet takes at least \(Fmt.num(Decimal(min) / 1_000_000)) USDC. Nothing was sent.")
        }
        _ = try await executor(wallet).fundAgent(units: units, depositAddress: deposit, agentAccount: account)
        // The USDC has left Solana: from here on nothing reads as «failed», which would invite a second deposit.
        let s = try? await AgentAPI.final(op, within: 900)
        guard let s, s.final else { throw ExecutionError(Self.mayHaveGone("The \(Fmt.num(usdc)) USDC left Solana for the agent wallet", op)) }
        guard s.ok else { throw ExecutionError("The deposit into the agent wallet ended \(s.status).") }
        return "\(Fmt.num(usdc)) USDC are in the agent wallet. It spends them only within the rules you signed."
    }

    /// USDC out of the agent wallet by the grant alone: NEAR Intents checks it against the owner's policy.
    func agentSend(_ usdc: Decimal, to name: String) async throws -> String {
        try NodeStore.agentAPIReady()
        let (a, _) = try await ownAgent()
        guard let grant = NodeStore.agentGrant else { throw ExecutionError("This Mac has no grant for the agent wallet.") }
        let (payee, _, chain, address) = try agentPayee(name)
        let body = AgentAPI.json(["asset": Self.agentUSDC.solana, "amount": "\(Self.agentUnits(usdc))", "chain": chain, "recipient": address])
        // A timeout doesn't say the send wasn't taken: the same request under the same Idempotency-Key gives back the
        // original result (the API's rule), so it's asked again rather than read as «may have gone» (a 0.3 USDC send read
        // «timed out» on 2026-11-05; a 0.5 USDC one landed 10 s after «status unknown» on 2026-11-10).
        let key = "withdraw-\(UUID().uuidString.lowercased())"
        var data: Data?
        for attempt in 1...3 where data == nil {
            do {
                data = try await AgentAPI.call("POST", "/v1/agents/\(a.agentID)/withdraw", body: body, grant: grant, idempotency: key)
            } catch let e as AgentAPI.Failure where e.status < 500 && !["wallet_busy", "idempotency_conflict"].contains(e.code) {
                // The policy at work: say what NEAR Intents refused and why (a replay gives back the same refusal).
                throw ExecutionError("NEAR Intents refused the agent wallet's send: \(e.text) (\(e.code)). " + agentRules(a, payee: e.code == "policy_destination_denied" ? name : nil))
            } catch is AgentAPI.Failure, is URLError {
                // No answer, a server error, or the first request still being carried out: asked again, then said as unknown.
                if attempt < 3 { try await Task.sleep(for: .seconds(5)) }
            }
        }
        guard let data else { throw ExecutionError(Self.mayHaveGone("The agent wallet's send of \(Fmt.num(usdc)) USDC was asked of NEAR Intents", nil)) }
        guard let op = (try JSONSerialization.jsonObject(with: data) as? [String: Any])?["correlation_id"] as? String else { throw ExecutionError("The Agent API gave no operation id.") }
        let s = try? await AgentAPI.final(op, within: 900)
        guard let s, s.final else { throw ExecutionError(Self.mayHaveGone("The agent wallet's send of \(Fmt.num(usdc)) USDC is with NEAR Intents", op)) }
        guard s.ok else { throw ExecutionError("The agent wallet's send ended \(s.status).") }
        return "The agent wallet sent \(Fmt.num(usdc)) USDC to «\(payee)» on \(chain == "sol" ? "Solana" : "Base") — within your rules, without your key."
    }

    /// A move NEAR Intents may have made with no word back: it says so and that it mustn't be asked again yet.
    static func mayHaveGone(_ what: String, _ op: String?) -> String {
        "\(what), but no final answer came back, so it may have gone. Status unknown: don't send it again until the balance shows it\(op.map { " (NEAR Intents operation \($0))" } ?? "")."
    }

    /// What the rules allow, as they were signed: the destinations are this wallet's own addresses and the saved ones the
    /// signer had confirmed at that moment (agentDestinations). «Only to your own and saved addresses» let the model explain
    /// a refused «Automation» as saved after the agent wallet, when it had been saved weeks before and never confirmed.
    func agentRules(_ a: AgentWalletRecord, payee: String? = nil) -> String {
        "Your rules, signed on \(a.created.formatted(date: .abbreviated, time: .omitted)): at most $\(Fmt.num(a.dailyUSD)) a day, only to this "
            + "wallet's own addresses and to the saved addresses that were confirmed with \(Presence.name) when the rules were signed."
            + (payee.map { " «\($0)» wasn't one of them, and the rules can't be changed from the chat." } ?? "")
    }

    /// The saved address an agent's send pays, and where: a Solana one on Solana, a Base one on Base — the same the
    /// card names; anything else (a multisig, another network) is refused.
    func agentPayee(_ name: String) throws -> (name: String, label: String?, chain: String, address: String) {
        guard let entry = savedEntry(name, network: nil) else {
            // The wallet itself («Main», «me»): its own Solana address, on the rules since the agent wallet was made.
            if WalletCommand.isOwnWallet(name, label: activeWallet?.label), !solanaAddress.isEmpty {
                return (activeWallet?.label ?? "This wallet", "Your wallet", "sol", solanaAddress)
            }
            throw ExecutionError("There's no “\(name)” in Addresses.")
        }
        if entry.network == .solana { return (entry.name, entry.label, "sol", entry.address) }
        if entry.network.rawValue == "base", isEVMAddress(entry.address) { return (entry.name, entry.label, "base", entry.address.lowercased()) }
        throw ExecutionError("The agent wallet pays a saved Solana or Base address; “\(entry.name)” is on \(entry.network.title).")
    }

    func agentFreeze(_ on: Bool) async throws -> String {
        try NodeStore.agentAPIReady()
        let (a, owner) = try await ownAgent()
        _ = try await agentOwnerIntent(["type": on ? "agent_freeze" : "agent_unfreeze", "agent_id": a.agentID], owner: owner)
        agentWallet?.frozen = on
        return on ? "The agent wallet is frozen: nothing moves until you unfreeze it." : "The agent wallet acts again, within your rules."
    }
}

private extension Data {
    /// `"a":1}` → `,"a":1}`: one more field spliced after a raw value.
    func prefixingComma() -> Data { Data(",".utf8) + self }
}
