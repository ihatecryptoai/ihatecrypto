import DeFiYields
import EVMChain
import ExecutionCore
import Foundation
import GuardJev
import HyperliquidSigning
import IhatecryptoCore
import JupiterSwap
import SignerAPI
import SwiftUI
import TradeExecution
@preconcurrency import UserNotifications
import WalletCore

/// The app's side of signing: every request goes to the signing process, which runs the guard itself.
/// Signatures waiting on the signer right now: while there are any, the running move's card asks for Touch ID.
@MainActor @Observable
final class SignerActivity {
    static let shared = SignerActivity()
    var waiting = 0
    /// What the Ledger waits for from the person now («Open the Solana app…», «Review … and approve»).
    var ledger: String?
    /// A signature waits on the person: Touch ID, the Mac's password or the Ledger's buttons.
    var atDevice: Bool { waiting > 0 || ledger != nil }
}

nonisolated struct XPCTradeSigner: TradeSigner {
    /// The automation wallet's id while a scheduled task runs on it (TaskRun): every signature is asked of it by id.
    var asWallet: String?

    private func send(_ request: SignerRequest) async throws -> SignerReply {
        try await SignerClient().send(asWallet.map { .asWallet(id: $0, request: request) } ?? request)
    }

    /// Counts the signature while the signer has it: Touch ID or the password may be up, and the canvas says so.
    private static func signing<T: Sendable>(_ body: () async throws -> T) async throws -> T {
        await MainActor.run { SignerActivity.shared.waiting += 1 }
        defer { Task { @MainActor in SignerActivity.shared.waiting -= 1 } }
        return try await body()
    }

    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data {
        let reply = try await Self.signing { try await send(.signSolana(wire: wire, accounts: accounts, recipients: recipients, tokenOwners: tokenOwners)) }
        guard reply.ok, let signed = reply.signedWire else { throw GuardStop(Self.reason(reply)) }
        return signed
    }

    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature {
        let reply = try await Self.signing { try await send(.signHyperliquidL1(action: action, nonce: nonce, mainnet: true)) }
        guard reply.ok, let signature = reply.signature else { throw GuardStop(Self.reason(reply)) }
        return signature
    }

    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature {
        let reply = try await Self.signing { try await send(.signHyperliquidUser(action: action, mainnet: true, recipients: recipients)) }
        guard reply.ok, let signature = reply.signature else { throw GuardStop(Self.reason(reply)) }
        return signature
    }

    func registerRelayOrder(_ order: Data) async throws {
        let reply = try await Self.signing { try await send(.relayOrder(order: order)) }
        guard reply.ok else { throw GuardStop(Self.reason(reply)) }
    }

    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature {
        let reply = try await Self.signing { try await send(.signRelayNonceMapping(wallet: wallet, depositor: depositor, id: id, nonce: nonce)) }
        guard reply.ok, let signature = reply.signature else { throw GuardStop(Self.reason(reply)) }
        return signature
    }

    func signSolanaSponsored(_ wire: Data, accounts: [String], tokenOwners: [String: String], sponsor: String) async throws -> Data {
        let reply = try await Self.signing {
            try await send(.signSolana(wire: wire, accounts: accounts, recipients: [], tokenOwners: tokenOwners, sponsor: sponsor))
        }
        guard reply.ok, let signed = reply.signedWire else { throw GuardStop(Self.reason(reply)) }
        return signed
    }

    func signSponsoredTransfer(_ t: SponsoredTransfer, recipients: [String]) async throws -> Data {
        let reply = try await Self.signing {
            try await send(.signSponsoredTransfer(chainId: t.chainId, nonce: t.nonce, gasLimit: t.gasLimit, token: t.token, to: t.to, amount: t.amount,
                                                  recipients: recipients))
        }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signRelayMulticallPermit(_ m: RelayMulticallPermit) async throws -> [EVMSignature] {
        let reply = try await Self.signing {
            try await send(.signRelayMulticallPermit(chainId: m.chainId, token: m.token, value: m.value, validBefore: m.validBefore, relayer: m.relayer,
                                                     refundTo: m.refundTo, nftRecipient: m.nftRecipient, metadata: m.metadata,
                                                     calls: m.calls.map { SignerCall3(target: $0.target, allowFailure: $0.allowFailure, value: $0.value, data: $0.callData) }))
        }
        guard reply.ok, let signatures = reply.signatures, signatures.count == 2 else { throw GuardStop(Self.reason(reply)) }
        return signatures
    }

    func signReceiveAuthorization(_ p: Permit3009) async throws -> EVMSignature {
        let reply = try await Self.signing {
            try await send(.signReceiveAuthorization(chainId: p.chainId, token: p.token, to: p.to, value: p.value, validBefore: p.validBefore, nonce: p.nonce))
        }
        guard reply.ok, let signature = reply.signature else { throw GuardStop(Self.reason(reply)) }
        return signature
    }

    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                 to: String, data: Data, recipients: [String]) async throws -> Data {
        let reply = try await Self.signing { try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                                      gasLimit: gasLimit, to: to, value: 0, data: data, recipients: recipients)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    var signsBatches: Bool { true }

    func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                      calls: [Batch7702.Call], recipients: [String]) async throws -> Data {
        let reply = try await Self.signing { try await send(.signEVMBatch(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                                gasLimit: gasLimit, calls: calls.map { SignerCall(to: $0.to, data: $0.data) },
                                                                recipients: recipients)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                        to: String, value: UInt64, data: Data, recipients: [String]) async throws -> Data {
        let reply = try await Self.signing { try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                                      gasLimit: gasLimit, to: to, value: value, data: data, recipients: recipients)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signEVMTransfer(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                         to: String, value: UInt64, recipients: [String]) async throws -> Data {
        let reply = try await Self.signing { try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                                      gasLimit: gasLimit, to: to, value: value, data: Data(), recipients: recipients)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await Self.signing { try await send(.signNearIntentDeposit(amount: "\(amount)", depositAddress: depositAddress, nonce: nonce, blockHash: blockHash)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signNearTransfer(amount: UInt128, receiver: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await Self.signing { try await send(.signNearTransfer(amount: "\(amount)", receiver: receiver, recipients: recipients, nonce: nonce, blockHash: blockHash)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    /// An x402 payment's EIP-3009 authorization, built and checked by the signer: the 65-byte signature.
    func signX402(chainId: UInt64, token: String, to: String, value: UInt64, validBefore: UInt64, nonce: Data, service: String) async throws -> Data {
        let reply = try await Self.signing { try await send(.signX402(chainId: chainId, token: token, to: to, value: value, validBefore: validBefore, nonce: nonce, service: service)) }
        guard reply.ok, let sig = reply.signatures?.first else { throw GuardStop(Self.reason(reply)) }
        // r ‖ s ‖ v: the 65 bytes x402 sends.
        return sig.r + sig.s + Data([UInt8(sig.v)])
    }

    /// An agent wallet's owner request (NEAR Intents' Agent API), signed by this wallet's NEAR key after the signer
    /// read and confirmed it: a NEP-413 signature, or a borsh SignedDelegateAction.
    func signAgentMessage(payload: Data) async throws -> Data {
        let reply = try await Self.signing { try await send(.signAgentMessage(payload: payload)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }
    func signAgentDelegate(payload: Data, nonce: UInt64, maxBlockHeight: UInt64, policy: Data?) async throws -> Data {
        let reply = try await Self.signing { try await send(.signAgentDelegate(payload: payload, nonce: nonce, maxBlockHeight: maxBlockHeight, policy: policy)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signNearDeleteAccount(beneficiary: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await Self.signing { try await send(.signNearDeleteAccount(beneficiary: beneficiary, recipients: recipients, nonce: nonce, blockHash: blockHash)) }
        guard reply.ok, let raw = reply.signedRaw else { throw GuardStop(Self.reason(reply)) }
        return raw
    }

    func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature {
        let reply = try await Self.signing { try await send(.signSafeMove(chainId: move.chainId, safe: move.safe, to: move.to, value: move.value,
                                                                         data: move.data, nonce: move.nonce, recipients: recipients)) }
        guard reply.ok, let signature = reply.signature else { throw GuardStop(Self.reason(reply)) }
        return signature
    }

    static func reason(_ reply: SignerReply) -> String {
        ([reply.error ?? "The signer refused."] + reply.problems).joined(separator: " ")
    }
}

extension Workspace {
    /// Balances and previews work with any addresses; signing needs the key in the signing process.
    var previewWallet: TradeWallet? {
        signerWallet ?? (isSolanaAddress(solanaAddress) && isEVMAddress(hlAddress) ? TradeWallet(solana: solanaAddress, evm: hlAddress) : nil)
    }

    var planIsReady: Bool {
        // Built once: `blocks` works the whole plan out again on every read.
        let b = blocks
        return !b.isEmpty && b.allSatisfy { [.ready, .warning, .info].contains($0.status) }
    }

    /// Everything signed, written before it's sent (Journal.swift).
    var journalURL: URL { LocalStore.url("journal.jsonl") }

    /// What went out since `start` with no outcome yet — signed in the journal and nothing after: a receipt that didn't
    /// come, a Relay order still on its way. Each as where to look.
    func unsettled(since start: Date) -> [String] { signed(since: start) { try $0.unresolved() } }

    /// «Went through: <links>. Then: <error>. Don't repeat it — check first.» when something this move signed since `start`
    /// went through before it failed (Journal.wentThrough): a balance read the node throttled right after a swap, a
    /// bridge's second leg. «Failed» with «Try again» there swapped, borrowed or bridged a second time. nil: nothing did.
    func wentThrough(since start: Date, then error: any Error) -> String? {
        let went = signed(since: start) { try $0.wentThrough() }
        guard !went.isEmpty else { return nil }
        let e = error.localizedDescription
        return say("Went through: ", "Прошло: ", "Se hizo: ") + went.joined(separator: " · ") + say(". Then: ", ". Потом: ", ". Después: ") + e
            + (e.hasSuffix(".") ? "" : ".") + say(" Don't repeat it — check first.", " Не повторяй, сначала проверь.", " No lo repitas: compruébalo antes.")
    }

    /// The journal's entries signed since `start`, each as where to look.
    private func signed(since start: Date, _ pick: (Journal) throws -> [Journal.Entry]) -> [String] {
        let from = Date(timeIntervalSince1970: start.timeIntervalSince1970.rounded(.down))   // the journal keeps whole seconds
        // A scheduled task's moves are the automation wallet's (executor).
        let solana = TaskRun.automation?.solana ?? solanaAddress
        return ((try? pick(Journal(url: journalURL))) ?? []).filter { $0.at >= from }.map { Self.whereToLook($0, solana: solana) }
    }

    static func whereToLook(_ e: Journal.Entry, solana: String) -> String {
        switch e.chain {
        // A swap with no gas of ours, still «signed», is journaled by our signature, the second: an explorer finds it only by
        // the first, the sponsor's, so the wallet's page. Once through, its entry carries the sponsor's (Journal.wentThrough).
        case "solana": e.kind == .signed && e.note.contains("; Jupiter order ") ? "https://solscan.io/account/\(solana)" : "https://solscan.io/tx/\(e.id)"
        case "near": "https://nearblocks.io/txns/\(e.id)"
        case "relay": "Relay order \(e.id)"
        case "1click": "1Click deposit \(e.id)"
        case "1click-order": "1Click limit order \(e.id) (say «show limit orders»)"
        case "x402": "x402 payment to \(e.note), signed and maybe still settling (authorization \(e.id.prefix(10))…)"
        case "layerzero": "https://layerzeroscan.com/tx/\(e.id)"
        case "cctp": TransitDetails.links(e).first?.url.absoluteString ?? "CCTP \(e.id)"
        case "hyperliquid": "Hyperliquid «\(e.note)»"
        default: EVMNetwork.named(e.chain).map { "\($0.explorer)/tx/\(e.id)" } ?? "\(e.chain) \(e.id)"
        }
    }

    func executor(_ wallet: TradeWallet) -> Executor {
        // A scheduled task on the automation wallet (Tasks.swift): its addresses, and the signer told which wallet signs.
        let automation = TaskRun.automation
        let wallet = automation.map { TradeWallet(solana: $0.solana ?? wallet.solana, evm: $0.evm ?? wallet.evm) } ?? wallet
        let signer: any TradeSigner = if let automation { XPCTradeSigner(asWallet: automation.id) }
            else if ledgerActive { ledgerSigner(wallet) } else { XPCTradeSigner() }
        let owes = automation != nil
        return Executor(wallet: wallet, signer: signer, journal: Journal(url: journalURL), askFee: !owes) { [weak self] text in
            Task { @MainActor in
                self?.reply(text, note: "execution")
                self?.runningAction?.lines.append(text)
                self?.journeyRead(text)
            }
        } swapped: { [weak self] from, to, spent, received in
            // Bought with USDC: its cost; sold for USDC: that share of the cost goes. Other swaps (SOL ⇄ JitoSOL) have no dollar side.
            Task { @MainActor in
                if from == .usdc { self?.costBasis[to.mint, default: CostBasis()].bought(received, for: spent) }
                if to == .usdc { self?.costBasis[from.mint]?.sold(spent) }
                // A task's buy on the automation wallet goes on to the wallet that's on after the run (sendBoughtToMain).
                if owes {
                    self?.owe("solana:\(to.mint == CostBasis.wsol ? "SOL" : to.mint)", received)
                    self?.owe("solana:\(from.mint == CostBasis.wsol ? "SOL" : from.mint)", -spent)
                }
            }
        } traded: { [weak self] key, tokens, dollars in
            // A buy through ihatecrypto on an EVM network: its cost, kept like the Solana swaps'.
            Task { @MainActor in
                if tokens > 0 { self?.costBasis[key, default: CostBasis()].bought(tokens, for: dollars) } else { self?.costBasis[key]?.sold(-tokens) }
                if owes { self?.owe(key, tokens) }
            }
        }
    }

    /// The plan in a few plain lines for Jev: it judged short summaries reliably and long block text poorly.
    var planLines: [String] {
        let intent = resolved.intent
        let e = evaluation
        var out = ["\(intent.side == .long ? "Long" : "Short") \(intent.asset) on Hyperliquid, leverage \(Fmt.num(intent.leverage ?? 1))×."]
        if let s = e.sizing {
            out.append("Size \(Fmt.num(s.size, maxFraction: 6)) \(intent.asset) at ≈ \(Fmt.price(s.entry)), notional \(Fmt.usd(s.notional)), margin \(Fmt.usd(s.margin)).")
            out.append("Stop \(Fmt.pct(intent.stopPct ?? 0)) → \(Fmt.price(s.stopPrice)), loss ≈ \(Fmt.usd(s.lossAtStopUSD)) (risk \(Fmt.usd(intent.riskUSD ?? 0))).")
            if let t = s.takePrice { out.append("Take-profit \(Fmt.pct(intent.takePct ?? 0)) → \(Fmt.price(t)), profit ≈ \(Fmt.usd(s.profitAtTakeUSD ?? 0)).") }
            // Without it the check read a TWAP's risk (parts may fill 3% worse) against a single fill at today's price and
            // doubted it, holding Sign (live run 2026-11-05).
            if let minutes = intent.twapMinutes {
                out.append("Entry: a TWAP over \(TradeIntent.duration(minutes)) on Hyperliquid, parts at the market every 30 s or more, each within 3% of the price. "
                           + "The risk and the size count every part filling 3% worse than now, so from today's price the stop loses less than the risk.")
            }
            if let t = intent.trailingPct {
                out.append(intent.entryPx == nil
                    ? "Trailing stop \(Fmt.pct(t)) behind the best price, held by Hyperliquid itself (reduce-only, the whole position), beside the fixed stop."
                    : "Trailing stop \(Fmt.pct(t)) behind the best price, moved by ihatecrypto once the limit fills, while the app is open.")
            }
        }
        // Tied to a price alert, the plan waits for it: without this line the check reads «long below 117.44» against
        // today's entry and cries mismatch.
        if let a = planAlert {
            out.append("It waits for a price alert: when \(intent.asset) \(a.above ? "rises" : "falls") to \(Fmt.price(a.price)), the plan comes to the user to "
                       + "confirm and enters at the market price then (≈ \(Fmt.price(a.price))); the entry above is today's price, for sizing.")
        }
        if !resolved.notes.isEmpty { out.append("The user asked to fill in the rest by the rules: " + resolved.notes.joined(separator: " ")) }
        // From where the card says: Solana, or the EVM network that holds it when Solana is short.
        let hlFee = "Hyperliquid's \(Fmt.num(RiskCalc.takerFee * 100, maxFraction: 3))%"
        out.append(intent.twapMinutes == nil
            ? "Fees on each side: \(hlFee) and ihatecrypto's \(HLBuilder.rate) (the user approves it once, on the first trade)."
            : "Fees: the TWAP's parts pay \(hlFee) only, no ihatecrypto fee on them. The exit pays \(hlFee) and ihatecrypto's \(HLBuilder.rate).")
        if let a = intent.amountUSDC { out.append("Move \(Fmt.num(a)) USDC from \(fundingNet?.name ?? "Solana") to Hyperliquid.") }
        else if let q = quote { out.append("Bridge \(q.provider): \(Fmt.num(q.amountIn)) USDC from \(fundingNet?.name ?? "Solana") to Hyperliquid.") }
        return out
    }

    /// The second check of the plan runs beside the chat: the reply shows at once, the check's card fills in a few
    /// seconds later, and Execute waits for it (prepareExecution).
    func startPlanReview() {
        let previous = planReviewTask
        planReviewTask = Task { await previous?.value; await reviewPlan() }
    }

    /// Jev reads each sized plan against the user's words; without OpenRouter the chat's model does. It can only warn.
    func reviewPlan() async {
        // Any sized plan, a stopped one too: a misread request («5 USDC» read as risk $5) is what the rules stop.
        guard let sizing = evaluation.sizing, intent != reviewedIntent || planReview == nil else { return }
        // Code first, second check on or off: the plan never holds more money or more risk than its command named.
        if let market, let why = Plan.moneyProblem(named: intent, sizing: sizing, market: market) {
            reviewedIntent = intent
            planReview = JevReview(label: "mismatch", confidence: 1, concern: true, text: why)
            jevNote = nil
            return
        }
        guard jevScreening else { planReview = nil; jevNote = nil; return }
        reviewedIntent = intent
        let requests = messages.filter(\.fromUser).map(\.text), lines = planLines, key = jevKey, client = llmClient(),
            language = replyLanguage, model = model
        // Both readers at once: the chat's model says what differs, the OpenRouter classifier adds a confident concern.
        @Sendable func byModel() async -> Result<String?, Error>? {
            guard let client else { return nil }
            do { return .success(try await ChatRouter.verifyPlan(requests: requests, plan: lines, language: language, client: client, model: model)) }
            catch { return .failure(error) }
        }
        @Sendable func byClassifier() async -> JevReview? {
            guard let key else { return nil }
            return try? await JevReviewer.plan(requests: requests, plan: lines, apiKey: key)
        }
        async let modelSays = byModel()
        async let classifierSays = byClassifier()
        let (m, c) = await (modelSays, classifierSays)
        if let c { FileHandle.standardError.write(Data("ihatecrypto plan check: \(c.label) \(c.confidence) «\(lines.first ?? "")»\n".utf8)) }
        guard intent == reviewedIntent else { return }   // the plan changed meanwhile: its own review follows
        // The model's reading decides; the classifier's reasonless alarm on right plans («лонг нефти на 5$») counts only
        // when the model couldn't answer.
        if case .success(let why)? = m {
            planReview = JevReview(label: why == nil ? "matches" : "mismatch", confidence: 1, concern: why != nil, text: why ?? "Matches your request.")
        } else if let c, c.concern {
            planReview = c
        } else if c != nil || { if case .success? = m { return true } else { return false } }() {
            planReview = JevReview(label: "matches", confidence: 1, concern: false, text: "Matches your request.")
        } else {
            planReview = nil
            jevNote = "The second check couldn't run" + (m.flatMap { if case .failure(let e) = $0 { ": \(e.localizedDescription)" } else { nil } } ?? ": connect a model in Settings › Sign-ins")
                + ". Only code checks apply."
            return
        }
        jevNote = nil
    }

    /// Fresh quote, the bridge transaction and its simulation, and Jev's reading of all of it. Nothing is signed.
    /// `rerun` (rerunPlan, Try again) first recalculates the plan from now: balances, prices and a new bridge quote.
    func prepareExecution(rerun: Bool = false) async {
        // No wallet to prepare for (its addresses went since): nothing to show, the sheet closes.
        guard let wallet = previewWallet else { cancelPreparation(); return }
        // Sign stays locked from the press to Jev's answer, the plan's own check included. Only the newest preparation
        // writes: a later press or Cancel takes over, so an older one ending never unlocks Sign on signatures still read.
        preparation += 1
        let run = preparation
        preparing = true
        executionError = nil
        txReview = nil
        defer { if run == preparation { preparing = false } }
        // The sheet stays up («Preparing the trade») through the refresh instead of closing and coming back.
        if rerun { prepared = nil; await refresh() }
        // The plan's second check may still be reading it: its warning must be on the card before anything is prepared.
        await planReviewTask?.value
        guard run == preparation else { return }
        // A stale confirmation redone under the sheet goes too: its old steps can't be signed.
        if let stop = missingTradingKey ?? tradeRefusal ?? twapRefusal { prepared = nil; executionError = stop; return }
        if openVia1Click == nil, let q = quote, q.opening, fundingNet == nil, q.alternativeFee != nil { prepared = nil; askOpeningRoute(q); return }
        do {
            let p = try await executor(wallet).prepare(intent, policy: policy, openVia1Click: openVia1Click == true, spot: spotHoldings ?? [:])
            guard run == preparation else { return }
            prepared = p
            if jevScreening, let key = jevKey {
                let review = try? await JevReviewer.transactions(plan: planLines, signatures: p.steps, changes: p.changes, apiKey: key)
                guard run == preparation else { return }
                txReview = review
            }
        } catch {
            guard run == preparation else { return }
            prepared = nil
            executionError = error.localizedDescription
            noteFailure("Prepare the \(intent.side == .long ? "long" : "short") \(IntentParser.pretty(intent.asset))", error)
            await explainFailure()
        }
    }

    /// Cancel in the confirmation sheet: the preparation under way stops writing, and the sheet closes at once.
    func cancelPreparation() {
        preparation += 1
        preparing = false
        prepared = nil
        executionError = nil
    }

    func confirmExecution() async {
        // Prepared isn't ready to sign while the second check still reads the signatures (or a stale plan is redone).
        guard !preparing, let p = prepared, let wallet = signerWallet else { return }
        guard p.isFresh else {
            // The bridge transaction's blockhash is about to expire: show fresh numbers and ask again.
            await prepareExecution()
            // Only over new numbers: a Cancel meanwhile closed the sheet, and a failed refresh keeps its own reason.
            if prepared != nil, executionError == nil { executionError = "Numbers refreshed: the confirmation was older than 45 seconds." }
            return
        }
        // The signature gets its own activity, with the plan exactly as it was signed.
        let signedPlan = canvasBubbles()
        beginActivity(.execution, title: "Signed: \(p.intent.side == .long ? "long" : "short") \(IntentParser.pretty(p.market.name))", coin: p.market.name)
        updateActivity { $0.bubbles = signedPlan?.bubbles ?? []; $0.lines = p.steps }
        prepared = nil
        executing = true
        defer { executing = false }
        var done = false
        let started = Date()
        do {
            let opened = try await executor(wallet).execute(p)
            done = true
            // Always replaced: rules left from an earlier position in the same coin must not steer this one.
            let partial = opened.partialTake != nil || (opened.pending && p.intent.partialTake == true)
            // A trailing stop the exchange holds needs nothing from the app; one that didn't go on (or a limit's) is moved here.
            let trailing = opened.trailingOnExchange ? nil : p.intent.trailingPct
            if partial || trailing != nil || opened.twapId != nil {
                var rules = ExitRules(openedSize: opened.size, entry: opened.entry, long: p.intent.side == .long,
                                      breakevenAfterPartial: partial, trailingPct: trailing)
                rules.partialOrderPending = opened.pending && p.intent.partialTake == true
                rules.twapId = opened.twapId
                rules.twapRisk = opened.twapId.map { _ in p.sizing.lossAtStopUSD }
                exitRules[opened.asset] = rules
                keepAwakeForTwaps()
                partialTries[opened.asset] = nil
                stopTries[opened.asset] = nil
            } else {
                exitRules[opened.asset] = nil
            }
            let protection = "stop \(Fmt.price(opened.stop))\(opened.take.map { ", take-profit \(Fmt.price($0))" } ?? "")"
            if let id = opened.twapId, let minutes = p.intent.twapMinutes {
                reply("TWAP \(id) runs on Hyperliquid: \(Fmt.num(opened.size, maxFraction: 6)) \(opened.asset) over \(TradeIntent.duration(minutes)), the first part at \(Fmt.price(opened.entry)), \(protection) for the whole position. «cancel the TWAP» stops the rest.")
            } else {
                reply("Done: \(opened.asset) \(Fmt.num(opened.size, maxFraction: 6)) at \(Fmt.price(opened.entry)), \(protection).")
            }
        } catch {
            if let went = wentThrough(since: started, then: error) { reply(went) } else {
                reply("Execution stopped: \(error.localizedDescription)")
                noteFailure("\(p.intent.side == .long ? "Long" : "Short") \(IntentParser.pretty(p.market.name))", error)
            }
        }
        await refresh()
        // Only now, with the new position loaded: flipped before, the plan left the canvas and the position's board
        // came only after this refresh, so the canvas stood empty in between.
        if done { executed = intent }
    }

    /// After execution «take closer» or «stop 2%» are about the open position: its orders move on the exchange
    /// instead of a new plan starting. Only stop and take may change; anything else is a new plan.
    /// `confirm`: a model read the message, so the new orders wait on a card for Confirm, like any other model reading
    /// that touches a position. Every chat message goes through the model, so a typed «stop 2%» waits too; only the
    /// no-model path (`confirm: false`) moves them at once.
    func editExecuted(from before: TradeIntent, to next: TradeIntent, confirm: Bool = false) async -> Bool {
        guard let p = positions.first(where: { $0.coin == before.asset }) else { return false }
        var same = next
        same.stopPct = before.stopPct; same.takePct = before.takePct; same.takeR = before.takeR
        guard same == before, next != before else { return false }
        let long = p.size > 0
        let dir: Decimal = long ? 1 : -1
        let resolvedNext = Plan.resolve(next, market: market, equity: equity, policy: policy).intent
        var moves: [PositionCommand] = []
        if let stopPct = resolvedNext.stopPct, stopPct != before.stopPct {
            let stop = p.entry * (1 - dir * stopPct / 100)
            // A wider stop may not break the per-trade limit. A hedge's stop loses what the coins it covers gain.
            let loss = abs(p.entry - stop) * abs(p.size) + abs(p.size) * (p.entry + stop) * RiskCalc.tradeFee
            if !before.isHedge, let equity, equity > 0, loss / equity * 100 > policy.maxRiskPct {
                reply("A \(Fmt.pct(stopPct)) stop means −\(Fmt.usd(loss)), \(Fmt.pct((loss / equity * 100).rounded(1, .plain))) of capital. The rule: at most \(Fmt.pct(policy.maxRiskPct)). Stop not moved.")
                return true
            }
            moves.append(.stopTo(stop))
        }
        if let takePct = resolvedNext.takePct, takePct != before.takePct {
            moves.append(.takeTo(p.entry * (1 + dir * takePct / 100)))
        }
        if confirm, !moves.isEmpty {
            let what = moves.map { "\(Self.describe($0))" }.joined(separator: ", ")
            pendingAction = .orders(moves, coin: p.coin, intent: next)
            pendingText = "\(IntentParser.pretty(p.coin)): \(what)"
            reply(say("\(IntentParser.pretty(p.coin)): \(what). Press Confirm to move the orders.",
                      "\(IntentParser.pretty(p.coin)): \(what). Нажми Confirm, чтобы переставить ордера."))
            return true
        }
        for move in moves { await manage(move, coin: p.coin) }
        intent = next
        executed = next
        return true
    }

    /// «withdraw to Solana», «put 1 USDC in the vault», «take it out of the vault».
    /// `confirmed`: the user said «do it» after the allocation plan.
    func move(_ command: WalletCommand, confirmed: Bool = false) async {
        guard let wallet = signerWallet else {
            reply("Transfers work in the app once the key is in secure storage.")
            return
        }
        // Again at Confirm: a card parked on a canvas may come back after its module was switched off.
        if let off = moduleRefusal(command) { reply(off); return }
        if case .yields(let asset) = command {
            // Read-only: the rates are drawn on the canvas, Aave on Base among them (Agent.swift), in one assignment.
            yieldOptions = await loadYields(asset)
            reply(yieldOptions?.isEmpty == false ? say("Rates are on the canvas — the bigger the bubble, the higher the rate.", "Ставки на холсте — чем больше пузырь, тем выше ставка.",
                                                       "Las tasas están en el lienzo: cuanto más grande la burbuja, más alta la tasa.")
                                                 : say("Yield sources didn't respond right now.", "Источники ставок сейчас не ответили."), note: "DeFi")
            return
        }
        // The route as confirmed, before any balance moves: the canvas keeps it with the outcome and the time. A spread
        // has no route of its own: its drawn plan is the route, or after Confirm the canvas kept only «Done».
        let confirmedRoute: MoveRoute? = if case .allocate = command, let a = allocationPreview {
            MoveRoute(title: readable(Self.canvasTitle(command)), footer: "", steps: allocationBubbles(a))
        } else {
            route(command)
        }
        // Only a money move owns the board, and only a call that found nothing running owns `executing`: a schedule or a
        // preview finishing while a move runs must not take its board away, end its signing session or start a chain's
        // next step (continueTask).
        let ownsBoard = movesMoney(command, confirmed: confirmed)
        let ownsExecuting: Bool = switch command {
        case .schedule, .stopSchedules: false
        default: !executing
        }
        /// `unknown`: sent, but no outcome came — it may have gone through, so no «Try again».
        func finish(_ ok: Bool, unknown: Bool = false) {
            guard ownsBoard else { return }
            // The steps as the run left them: a paid request's last card says «answered», not «the answer comes here».
            let steps = runningAction?.steps ?? confirmedRoute?.steps ?? []
            // A chain going on keeps this board until its next step's takes its place (runSteps): dropped here, the
            // canvas showed this move alone, its steps after it gone, while the balances reloaded.
            if ok, !chainSteps().isEmpty {
                runningAction?.lines.append(say("Done — the next step follows", "Готово — дальше следующий шаг", "Hecho: sigue el siguiente paso"))
            } else {
                runningAction = nil
            }
            finishedAction = (confirmedRoute?.title ?? readable(Self.canvasTitle(command)), CanvasOutcome(at: Date(), ok: ok, unknown: unknown ? true : nil), steps)
            journey = journey?.ended(ok: ok, unknown: unknown)
            retryCommand = ok || unknown ? nil : (canvasID, command)
            // The outcome board comes from the saved canvas: save now, not after the balance refresh that follows,
            // or the canvas stays blank for those seconds.
            saveCanvas()
            notify(.moves, confirmedRoute?.title ?? readable(Self.canvasTitle(command)),
                   ok ? say("Done — checked, signed and confirmed.", "Готово — проверено, подписано и подтверждено сетью.")
                      : unknown ? say("Sent, status unknown — check the links in the chat before doing it again.", "Отправлено, статус неизвестен — проверь ссылки в чате, прежде чем повторять.")
                      : say("It didn’t go through. The chat says why.", "Не прошло. Причина в чате."), record: false, failed: !ok)
        }
        if ownsBoard {
            withAnimation(StudioView.spring) {
                runningAction = (confirmedRoute?.title ?? readable(Self.canvasTitle(command)), confirmedRoute?.steps ?? [], [], Date())
            }
            // N5: a move through NEAR Intents shows its way in the chat's field while it runs (carryOut runs it after
            // Confirm without `confirmed`, which only a spread's two steps use). A chain's next step that isn't one
            // leaves the last journey as it ended, until the next message.
            if let j = IntentsJourney(self, command) { journey = j }
        }
        if ownsExecuting { executing = true }
        // The board first: `executing` going false starts a chain's next step (continueTask), which lays its own board.
        defer {
            if ownsBoard { runningAction = nil }
            // A path that left without finish(): no journey is left running in the field.
            if ownsBoard, journey?.endedAt == nil { journey = nil }
            if ownsExecuting { executing = false }
        }
        let started = Date()
        do {
            let e = executor(wallet)
            switch command {
            case .withdrawToSolana(let usdc): _ = try await e.withdrawToSolana(usdc: usdc)
            case .lendDeposit(let symbol, let amount): _ = try await e.lend(symbol, amount: amount, deposit: true)
            case .lendWithdraw(let symbol, let amount): _ = try await e.lend(symbol, amount: amount, deposit: false)
            case .kamino(let deposit, let symbol, let amount): _ = try await e.kamino(symbol, amount: amount, deposit: deposit)
            case .stake(let sol): _ = try await e.convert(from: SpotAsset.all[0], to: .jitoSOL, amount: sol)
            case .unstake(let jito): _ = try await e.convert(from: .jitoSOL, to: SpotAsset.all[0], amount: jito)
            case .yields: break   // drawn on the canvas above
            case .hlp(let deposit, let amount): _ = try await e.hlp(deposit: deposit, amount: amount)
            case .buyNear(let usdc):
                // The NEAR address comes from the signer alone; the first time it unlocks the seed to derive it.
                guard let account = try await SignerClient().send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR address.") }
                nearAddress = account
                reply(try await e.buyNear(usdc: usdc ?? 0, account: account), note: "NEAR Intents · 1Click")
                await loadNearCost()   // what it cost, for the NEAR chart's P/L
            case .sellNear(let amount, let to):
                guard let account = try await SignerClient().send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR address.") }
                let net = try to.map { place in
                    guard let n = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't pay USDC on \(place).") }
                    return n
                }
                reply(try await e.sellNear(amount: amount, account: account, to: net), note: "NEAR Intents · 1Click")
                await loadNearCost()
            case .sendNear(let name, let amount):
                guard let entry = savedAddress(name, on: .near) else { throw ExecutionError("There’s no NEAR address “\(name)” in Addresses.") }
                try requireConfirmed(entry)
                guard let account = try await SignerClient().send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR address.") }
                reply(try await e.sendNear(amount, to: entry.address, account: account, label: entry.name), note: "NEAR")
                markSent(entry)
            case .closeNear(let name):
                guard let entry = savedAddress(name, on: .near) else { throw ExecutionError("There’s no NEAR address “\(name)” in Addresses.") }
                try requireConfirmed(entry)
                guard let account = try await SignerClient().send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR address.") }
                reply(try await e.closeNear(into: entry.address, account: account, label: entry.name), note: "NEAR")
                markSent(entry)
            case .sendSol(let name, let amount):
                guard let entry = savedAddress(name, on: .solana) else { throw ExecutionError("There’s no Solana address “\(name)” in Addresses.") }
                try requireConfirmed(entry)
                reply(try await e.sendSOL(amount, to: entry.address, label: entry.name), note: "Solana")
                markSent(entry)
            case .sendToken(let name, let token, let amount?, let network) where (try? acrossRecipient(name, token: token, network: network)) != nil:
                guard let r = try acrossRecipient(name, token: token, network: network) else { break }
                reply(try await e.sendAcross(r.symbol, amount: amount, network: r.entry.network.rawValue, to: r.address, label: r.entry.name),
                      note: "NEAR Intents · \(r.entry.network.title)")
                markSent(r.entry)
            case .agentCreate(let daily): reply(try await agentCreate(dailyUSD: daily), note: "NEAR Intents · Agent API")
            case .agentFund(let usdc): reply(try await agentFund(usdc), note: "NEAR Intents · Agent API")
            case .agentSend(let usdc, let name): reply(try await agentSend(usdc, to: name), note: "NEAR Intents · Agent API")
            case .agentFreeze(let on): reply(try await agentFreeze(on), note: "NEAR Intents · Agent API")
            case .sendPrivately(let name, let token, let amount, let network):
                let r = try privateRecipient(name, token: token, network: network)
                reply(try await e.sendAcross(r.symbol, amount: amount, network: r.network, to: r.address, label: r.entry.name, confidential: true),
                      note: "NEAR Intents · private · \(WalletCommand.placeName(r.network))")
                markSent(r.entry)
            case .sendToken(let name, let token, let amount, let network):
                let (entry, net) = try tokenRecipient(name, network: network)
                reply(try await e.sendToken(token, amount: amount, on: net, to: entry.address, label: entry.name),
                      note: AddressCheck.Network(rawValue: net)?.title ?? net)
                markSent(entry)
            case .aave(let deposit, let amount, let network):
                guard let m = AaveV3.markets.first(where: { $0.chain == network }) else {
                    throw ExecutionError("ihatecrypto uses Aave on Base, Arbitrum, Polygon and Ethereum, not on \(WalletCommand.placeName(network)).")
                }
                reply(try await e.aave(deposit: deposit, usdc: amount, market: m), note: m.place)
            case .morpho(let deposit, let amount, let network):
                guard let v = MorphoVault.vaults.first(where: { $0.chain == network }) else {
                    throw ExecutionError("ihatecrypto uses Morpho on Base, Arbitrum and Ethereum, not on \(WalletCommand.placeName(network)).")
                }
                reply(try await e.morpho(deposit: deposit, usdc: amount, vault: v), note: v.place)
            case .baseToSolana(let amount): reply(try await e.baseToSolana(usdc: amount), note: "Relay · Base → Solana")
            case .arbitrumToSolana(let amount): reply(try await e.evmToSolana(.arbitrum, usdc: amount), note: "Relay · Arbitrum → Solana")
            case .evmToSolana(let network, let amount):
                guard let net = EVMNetwork.named(network) else { throw ExecutionError("ihatecrypto doesn't move money on \(network).") }
                reply(try await e.evmToSolana(net, usdc: amount), note: "Relay · \(net.name) → Solana")
            case .buyStock(let symbol, let usdc):
                reply(try await e.buyStock(symbol, usdc: usdc ?? 0), note: "Robinhood Chain · Uniswap")
                // A token bought by its contract shows among the favorites, with what the wallet holds of it.
                if !symbol.hasPrefix("0x") { rememberStock(symbol) }
                else if favoriteTokens.contains(where: { $0.address.lowercased() == symbol.lowercased() }) { await loadRobinhood() }
                else { await tokenCommand(.favorite(address: symbol, chain: "robinhood")); await loadRobinhood() }
            case .sellStock(let symbol, let amount): reply(try await e.sellStock(symbol, amount: amount), note: "Robinhood Chain · Uniswap")
            case .buyOn(let place, let token, let usdc, let from):
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't swap on \(place).") }
                // The way picked on the route's step, for this move only.
                let prefer = routeWays.removeValue(forKey: Self.wayKey(place, token))
                reply(try await e.buyOn(net, token, usdc: usdc ?? 0, prefer: prefer, from: from, shown: shownWay(command, prefer: prefer)), note: "\(net.name) · \(prefer == "relay" ? "Relay" : prefer == "bridge" ? "Relay + exchange" : "the best way")")
            case .sellOn(let place, let token, let amount, let keep):
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't swap on \(place).") }
                let prefer = routeWays.removeValue(forKey: Self.wayKey(place, token))
                reply(try await e.sellOn(net, token, amount: amount, prefer: prefer, keep: keep, shown: shownWay(command, prefer: prefer)), note: "\(net.name) · \(prefer == "relay" ? "Relay" : prefer == "bridge" ? "exchange + Relay" : "the best way")")
            case .hlSpotBuy(let symbol, let usdc): reply(try await e.hlSpot(buy: true, symbol: symbol, usdc: usdc, amount: nil), note: "Hyperliquid · spot")
            case .hlSpotSell(let symbol, let amount): reply(try await e.hlSpot(buy: false, symbol: symbol, usdc: nil, amount: amount), note: "Hyperliquid · spot")
            case .showApprovals: detail = "approvals"   // shown without a card (Canvases: before the card)
            case .revokeApproval(let place, let token, let spender, let permit2):
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't revoke on \(place).") }
                reply(try await e.revokeApproval(net, token: token, spender: spender, permit2: permit2), note: "\(net.name) · approvals")
                approvalsChanged += 1
            case .sweepDust(let under): reply(try await e.sweepDust(dustTokens(under: under)), note: "Uniswap · dust into USDC")
            case .swapOn("solana", let from, let to, let amount):
                guard let a = await solanaAsset(from, note: nil), let b = await solanaAsset(to, note: nil) else {
                    throw ExecutionError("Jupiter couldn't name both tokens of this swap. Nothing was signed.")
                }
                let shown = jupiterShown.flatMap { $0.command == command ? $0.out : nil }
                reply(try await e.convert(from: a, to: b, amount: amount, shown: shown), note: "Solana · Jupiter")
            case .swapOn("near", let from, let to, let amount):
                guard let account = try await SignerClient().send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR address.") }
                nearAddress = account
                reply(try await e.intentSwap(from, on: "near", for: to, on: "near", amount: amount, near: account), note: "NEAR Intents · 1Click")
            case .swapOn("hyperliquid", let from, let to, let amount):
                reply(try await e.hlSpotSwap(from: from, to: to, amount: amount), note: "Hyperliquid · spot")
            case .swapOn(let place, let from, let to, let amount):
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't swap on \(place).") }
                reply(try await e.swapOn(net, from: from, to: to, amount: amount), note: "\(net.name) · Uniswap")
            case .intents(let from, let to, let amount):
                // What the card showed landing holds the move (a step of a chain had no card of its own: its quote does).
                let shown = swapQuote.flatMap { $0.command == command ? $0.ways.first?.worth : nil }
                reply(try await e.intentsBridge(from: from, to: to, usdc: amount, shown: shown),
                      note: "NEAR Intents · \(WalletCommand.placeName(from)) → \(WalletCommand.placeName(to))")
            case .cctp(let from, let to, let amount):
                guard let a = EVMNetwork.named(from), let b = EVMNetwork.named(to) else { throw ExecutionError("Circle's CCTP moves USDC between Ethereum, Arbitrum, Base and Polygon.") }
                reply(try await e.cctpBridge(from: a, to: b, usdc: amount), note: "Circle's CCTP · \(a.name) → \(b.name)")
            case .usdt(let from, let to, let amount, let layerZero):
                guard let a = EVMNetwork.named(from), let b = EVMNetwork.named(to) else { throw ExecutionError("ihatecrypto doesn't move USDT between \(from) and \(to).") }
                reply(try await e.usdtMove(from: a, to: b, usdt: amount, layerZero: layerZero), note: "\(layerZero ? "USDT0 · LayerZero" : "Relay") · \(a.name) → \(b.name)")
            case .aaveBorrow(let x, let net), .aaveRepay(let x, let net), .aaveCollateral(let x, let net):
                guard let m = AaveV3.markets.first(where: { $0.chain == net }) else {
                    throw ExecutionError("ihatecrypto uses Aave on Base, Arbitrum, Polygon and Ethereum, not on \(WalletCommand.placeName(net)).")
                }
                switch command {
                case .aaveBorrow: reply(try await e.aaveBorrow(m, usdc: x ?? 0), note: "\(m.place) · borrow")
                case .aaveRepay: reply(try await e.aaveRepay(m, usdc: x), note: "\(m.place) · repay")
                default: reply(try await e.aaveSupplyETH(m, eth: x ?? 0), note: "\(m.place) · collateral")
                }
            case .kaminoBorrow(let x, let stock, let adding): reply(try await e.kaminoBorrow(usdc: x ?? 0, against: stock, adding: adding), note: "Kamino · borrow")
            case .kaminoRepay(let x, let against): reply(try await e.kaminoRepay(usdc: x, against: against), note: "Kamino · repay")
            case .receiveAcross: break   // no card: the deposit address goes on the canvas (prepareReceiveAcross)
            case .crossSwap(let a, let from, let b, let to, let amount):
                // NEAR's side: the wallet's NEAR account, which only the signer derives.
                var near = nearAddress
                if from == "near" || to == "near" {
                    near = try await SignerClient().send(.nearAddress).nearAddress
                    nearAddress = near
                }
                reply(try await e.crossSwap(a, on: from, for: b, on: to, amount: amount, near: near),
                      note: "Relay · \(WalletCommand.placeName(from)) → \(WalletCommand.placeName(to))")
            case .swapPrivately(let a, let from, let b, let to, let amount):
                var near = nearAddress
                if from == "near" || to == "near" {
                    near = try await SignerClient().send(.nearAddress).nearAddress
                    nearAddress = near
                }
                reply(try await e.intentSwap(a, on: from, for: b, on: to, amount: amount, near: near, confidential: true),
                      note: "NEAR Intents · private · \(WalletCommand.placeName(from)) → \(WalletCommand.placeName(to))")
            case .limitOrder(let sell, let coin, let on, let price, let amount, let usdcOn, let days):
                var near = nearAddress
                if on == "near" || usdcOn == "near" {
                    near = try await SignerClient().send(.nearAddress).nearAddress
                    nearAddress = near
                }
                reply(try await e.limitOrder(sell: sell, coin: coin, on: on, price: price, amount: amount, usdcOn: usdcOn, days: days, near: near),
                      note: "NEAR Intents · limit order · private")
                limitOrderList = await ownLimitOrders(e).flatMap { $0.isEmpty ? nil : $0 }
                limitFollowed = nil   // a failed read here: the loop reads them again
            case .limitOrders:
                let list = await ownLimitOrders(e) ?? []
                limitOrderList = list.isEmpty ? nil : list
                reply(Self.limitListWords(list).map { say($0.en, $0.ru) } ?? say("No open limit orders.", "Открытых лимитных ордеров нет."))
                return
            case .limitCancel(let coin):
                if nearAddress == nil { nearAddress = try? await SignerClient().send(.nearAddress).nearAddress }
                reply(try await e.cancelLimitOrders(coin, near: nearAddress), note: "NEAR Intents · limit orders")
                limitOrderList = await ownLimitOrders(e).flatMap { $0.isEmpty ? nil : $0 }
                limitFollowed = nil
            case .robinhoodToSolana(let amount): reply(try await e.evmToSolana(.robinhood, usdc: amount), note: "Relay · Robinhood → Solana")
            case .solanaTo("hyperliquid", let amount):
                reply(try await e.solanaToHyperliquid(usdc: amount ?? 0), note: "Relay · Solana → Hyperliquid")
            case .solanaTo(let place, let amount):
                // Never a guess: a network the app doesn't know gets nothing.
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't move money to \(place).") }
                reply(try await e.solanaToEVM(net, usdc: amount ?? 0), note: "Relay · Solana → \(net.name)")
            case .sendTo(let name, let amount, let net): prepareTransfer(amount, to: name, network: net)
            case .pay:
                let via = payment?.x402 != nil ? "x402 · Base" : payment?.challenge?.near != nil ? "MPP · NEAR Intents" : "MPP · Tempo"
                reply(try await pay(command, e), note: via)
            case .contractCall(let c): reply(try await runContractCall(command, e), note: "\(WalletCommand.placeName(c.network)) · contract call")
            case .multisigCreate: reply(try await runMultisigCreate(e), note: "Safe · multisig")
            case .multisigSend: reply(try await runMultisigSend(command, e), note: "Safe · multisig")
            case .multisigShow: showMultisig(note: nil)
            case .gas(let place, let usd):
                guard let net = EVMNetwork.named(place) else { throw ExecutionError("ihatecrypto doesn't top up gas on \(place).") }
                reply(try await e.topUpGas(net, usd: usd ?? 1), note: "Gas · \(net.name)")
            case .bridge(let from, let to, let amount):
                let hyperliquid = to == "hyperliquid", b = EVMNetwork.named(to)
                guard let a = EVMNetwork.named(from), hyperliquid || b != nil else {
                    throw ExecutionError("ihatecrypto doesn't move money between \(from) and \(to).")
                }
                reply(try await e.bridge(a, to: b, hyperliquid: hyperliquid, usdc: amount), note: "Relay · \(a.name) → \(b?.name ?? "Hyperliquid")")
            case .claimRewards:
                reply(try await e.claimRewards(), note: "Merkl")
                rewards = nil
            case .sweepRewards(let min, let into):
                reply(try await e.sweepRewards(minUSD: min, into: into), note: "Merkl · Uniswap")
                rewards = nil
            case .receive: showReceive = true
            case .allocate(let symbol, let share, let limit, let without, let from):
                // Two steps: the plan first, the deposits only after «do it».
                let (_, ready, preview) = try await e.allocate(symbol, maxShare: share ?? 50, limit: limit, without: without, from: from, execute: confirmed)
                pendingAllocation = ready ? command : nil
                // Nothing free: said in words, without a «0 USDC → 0 places» plan that read as if it would do something.
                allocationPreview = confirmed || preview.free == 0 ? nil : preview
                // A scheduled run that spreads nothing stays quiet (runDueTask), with a grey mark: every day's «see the
                // canvas» led to a canvas already gone (the owner, 2026-10-16: «было 3 раза и ничего не произошло»).
                if !confirmed, !ready { checkQuiet = true }
                if !confirmed, preview.free == 0 {
                    let wallet = (TaskRun.automation ?? activeWallet).map { "«\($0.label)»" } ?? say("your wallet", "твоём кошельке", "tu billetera")
                    reply(say("Nothing to spread: no free \(preview.asset) on \(wallet). What’s deposited keeps earning.",
                              "Нечего раскладывать: на \(wallet) нет свободных \(preview.asset). Что уже в депозитах, продолжает приносить.",
                              "Nada que repartir: no hay \(preview.asset) libres en \(wallet). Lo depositado sigue rindiendo."), note: "DeFi")
                } else if !confirmed {
                    let other = share.map { $0 == 70 ? 50 : 70 } ?? 70
                    reply(ready ? say("The plan is on the canvas. Sign it, ask about it, or ask for another limit, e.g. “max \(other)% per protocol”.",
                                      "План на холсте. Подпиши, спроси о нём или попроси другой лимит, например «не больше \(other)% в один протокол».",
                                      "El plan está en el lienzo. Fírmalo, pregunta por él o pide otro límite, por ejemplo «máximo \(other)% por protocolo».")
                                : say("Nowhere to put it now — see the canvas.", "Сейчас некуда положить — смотри холст.", "Ahora no hay dónde ponerlo: mira el lienzo."), note: "DeFi")
                }
            case .rebalance(let share):
                // Like «spread»: the plan first, the moves only after «do it».
                let (_, ready, plan) = try await e.rebalance(maxShare: share ?? 50, execute: confirmed)
                pendingAllocation = ready ? command : nil
                if canvasID == nil { saveCanvas() }   // a new canvas gets its id on its first save: the card belongs to it
                rebalancePlan = confirmed ? nil : (canvasID, plan, share ?? 50)
                if !confirmed {
                    let other = share.map { $0 == 70 ? 50 : 70 } ?? 70
                    reply(ready ? say("The moves are on the canvas with their math. Sign them, ask about them, or ask for another limit, e.g. “rebalance, max \(other)% per protocol”.",
                                      "Переводы на холсте с расчётом. Подпиши, спроси о них или попроси другой лимит, например «перебалансируй, не больше \(other)% в протокол».")
                                : plan.moves.isEmpty && plan.skipped.isEmpty && plan.notes.first?.hasPrefix("Nothing is deposited") == true
                                    ? say("Nothing is deposited yet: «spread USDC across the best rates» puts free USDC to work.",
                                          "Депозитов пока нет: «разложи USDC по лучшим ставкам» пустит свободные USDC в работу.",
                                          "Aún no hay depósitos: «reparte USDC en las mejores tasas» pone a trabajar los USDC libres.")
                                    : say("Nothing worth moving now — the reasons are on the canvas.", "Сейчас нечего перекладывать — причины на холсте.",
                                          "Ahora no vale la pena mover nada: los motivos están en el lienzo."), note: "DeFi")
                }
            case .schedule(let asset, let usdc, let days):
                // A schedule buys later with nobody at the window: never set up from the debug API (its runs would sign
                // outside its marker), and never «every 0 days», which would spin forever finding the next date.
                if SignerClient.untrusted, activeWallet?.isTest != true {
                    reply(say("Recurring buys can’t be set up from the debug API: type it in the window yourself.",
                              "Регулярные покупки нельзя завести из отладочного API: напиши это в окне сам."), note: "debug API")
                    return
                }
                guard days > 0 else { reply(say("A recurring buy needs at least a day between buys.", "Между регулярными покупками нужен хотя бы день.")); return }
                // The per-purchase rule applies to each run; a schedule adds up, so it is shown per month too.
                // The first buy is a card like any other and waits for Confirm: a schedule set up from the chat never signs
                // at once by itself. Only that Confirm turns the schedule on (firstBuyOf); its own runs start one period later.
                var schedule = Schedule(symbol: asset.symbol, usdc: usdc, days: days, next: Date())
                schedule.advance()
                let monthly = usdc * Decimal(30) / Decimal(days)
                let every = days == 1 ? say("daily", "каждый день") : days == 7 ? say("weekly", "каждую неделю") : say("every \(ScheduleCanvas.period(days))", "раз в \(ScheduleCanvas.periodRU(days))")
                reply(say("Scheduled purchases: \(asset.symbol) for \(Fmt.usd(usdc)) \(every) — ≈ \(Fmt.usd(monthly)) a month. The first one is on the canvas: its Confirm buys now and turns the schedule on, Cancel leaves none. The next ones start \(Fmt.date(schedule.next)) and run while ihatecrypto is open; each goes through the same checks and signing.",
                          "Регулярные покупки: \(asset.symbol) на \(Fmt.usd(usdc)) \(every) — ≈ \(Fmt.usd(monthly)) в месяц. Первая — карточкой на холсте: Confirm купит сейчас и включит расписание, Cancel не оставит его. Следующие пойдут с \(Fmt.date(schedule.next)), пока ihatecrypto открыт; каждая проходит те же проверки и подпись."))
                await handleWallet(.buy(asset, usdc: usdc), quiet: true)
                if case .wallet(.buy(let a, _))? = pendingAction, a.symbol == asset.symbol { firstBuyOf = schedule }
                return
            case .stopSchedules:
                let n = schedules.count
                schedules = []
                reply(n == 0 ? "There were no scheduled purchases." : "Scheduled purchases stopped (\(n)).")
                return
            case .buy(let asset, let usdc): _ = try await e.swap(asset, buy: true, amount: usdc, equity: equity, shown: jupiterShown.flatMap { $0.command == command ? $0.out : nil })
            case .jupiterDCA(let asset, let usdc, let orders, let every):
                _ = try await e.jupiterDCA(asset, usdc: usdc ?? 0, orders: orders ?? WalletCommand.jupiterDCAOrders(for: usdc ?? 0), every: every)
                jupiterDCAs = try? await e.jupiterDCAOrders()
            case .jupiterDCACancel(let asset):
                _ = try await e.cancelJupiterDCA(asset)
                jupiterDCAs = (try? await e.jupiterDCAOrders()).flatMap { $0.isEmpty ? nil : $0 }
            case .jupiterDCAOrders:
                let orders = try await e.jupiterDCAOrders()
                jupiterDCAs = orders.isEmpty ? nil : orders
                reply(orders.isEmpty ? say("No Jupiter DCA is running.", "Jupiter DCA сейчас не запущен.")
                      : say("Running Jupiter DCA: \(orders.count). They're on the canvas.", "Запущено Jupiter DCA: \(orders.count). Они на холсте."))
                return
            case .sell(let asset, let amount): _ = try await e.swap(asset, buy: false, amount: amount, shown: jupiterShown.flatMap { $0.command == command ? $0.out : nil })
            }
            finish(true)
        } catch {
            // Sent, but no outcome came back: it may well have gone through, and a second try would pay twice.
            let sent = ownsBoard ? unsettled(since: started) : []
            // Gone through, then a step after it failed: it stays done, so it's said with its links and never offered again.
            let went = ownsBoard && sent.isEmpty ? wentThrough(since: started, then: error) : nil
            if let went {
                reply(went)
            } else if sent.isEmpty, let costly = error as? CostlyMove {
                // More lost than a move may lose on its own: asked, not refused (the user: «показывать диалог и спрашивать
                // Send anyway?»). The cards run it again with that loss accepted, or leave it.
                costlyMove = (command, costly)
                reply(costly.localizedDescription + say(" Send it anyway?", " Отправить всё равно?"))
                // Kamino's first-deposit rent: Jupiter Lend takes the same deposit without it, one card away.
                let lend: WalletCommand? = if case .kamino(deposit: true, symbol: let s, amount: let a?) = command, ["USDC", "SOL"].contains(s) { .lendDeposit(s, a) } else { nil }
                // A swap past the price-impact cap: the part that moves the price about 1% now, the rest later — the same command
                // run for less (Self.sendPart), never its words parsed again (a token by its mint would come back as its namesake).
                let part: [ChatRouter.Option] = costly.part.flatMap { p in
                    command.with(amount: p.amount) == command ? nil
                        : [ChatRouter.Option(title: say("\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol) now · price moves ≈ 1%",
                                                        "\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol) сейчас · цена сдвинется ≈ на 1%"),
                                             comment: say("The rest later, in parts: each one loses less", "Остальное позже, частями: каждая теряет меньше"),
                                             command: Self.sendPart, recommended: true)]
                } ?? []
                // Unpriced, the loss is said by the impact itself.
                let loss = costly.lost > 0 ? (say("≈ \(Fmt.usd(costly.lost)) lost", "потеря ≈ \(Fmt.usd(costly.lost))"))
                    : say("price moves \(Fmt.num((costly.impact ?? 0) * 100, maxFraction: 1))%", "цена сдвинется на \(Fmt.num((costly.impact ?? 0) * 100, maxFraction: 1))%")
                offerCards(part + [
                        ChatRouter.Option(title: say("Send anyway · \(loss)", "Отправить всё равно · \(loss)"),
                                          comment: costly.localizedDescription, command: Self.sendAnyway),
                    ] + (lend.map { [ChatRouter.Option(title: say("Jupiter Lend instead · no rent", "Лучше в Jupiter Lend · без аренды"),
                                                       comment: say("The same deposit, no accounts to pay for", "Тот же депозит, без платы за аккаунты"), command: $0.summary)] } ?? []) + [
                        ChatRouter.Option(title: say("Cancel", "Отмена"), comment: say("Nothing was signed", "Ничего не подписано"), command: Self.leaveCostly),
                ])
            } else if sent.isEmpty {
                reply(say("Failed: ", "Не вышло: ") + error.localizedDescription)
                if ownsBoard { noteFailure(confirmedRoute?.title ?? readable(Self.canvasTitle(command)), error) }
            } else {
                reply(say("Sent, status unknown: ", "Отправлено, статус неизвестен: ") + error.localizedDescription + " "
                      + say("Check before doing it again: ", "Проверь, прежде чем повторять: ") + sent.joined(separator: " · "))
            }
            finish(false, unknown: !sent.isEmpty || went != nil)
        }
        await refresh()
    }

    /// This wallet's open limit orders for the board: empty for none, nil when 1Click didn't answer. The NEAR account,
    /// which only the signer names, counts as one of its addresses.
    func ownLimitOrders(_ e: Executor) async -> [Executor.LimitOrderView]? {
        if nearAddress == nil { nearAddress = try? await SignerClient().send(.nearAddress).nearAddress }
        return (try? await e.limitOrders(near: nearAddress))?.map(\.view)
    }

    /// The loop's look at limit orders. The wallet's orders are read once when its journal holds one not settled (at launch,
    /// after a wallet switch), so they're followed with no «show limit orders». Then, while the board shows them: an order
    /// that filled or expired leaves the board and is said in the chat and as a notification, with what came and its price
    /// after the fees. A canceled one isn't: the cancel said it. The journal is brought up to date for what left.
    func followLimitOrders() async {
        guard let w = previewWallet else { return }
        let e = executor(w)
        if limitOrderList == nil, limitFollowed != w.solana {
            guard await e.followsLimitOrders else { limitFollowed = w.solana; return }
            // Unread (1Click silent): asked again next tick. This read also brings the journal up to date.
            guard let list = await ownLimitOrders(e), previewWallet?.solana == w.solana else { return }
            limitFollowed = w.solana
            if limitOrderList == nil, !list.isEmpty { limitOrderList = list }
            return
        }
        // What left the board is written down once its payout is final (it comes seconds after the fill): only those,
        // for at most 20 ticks each, so a payout 1Click never finishes isn't asked about forever.
        if !limitPayouts.isEmpty {
            let settled = await e.syncLimitJournal(only: Set(limitPayouts.keys))
            for (id, ticks) in limitPayouts { limitPayouts[id] = settled.contains(id) || ticks <= 1 ? nil : ticks - 1 }
        }
        guard let shown = limitOrderList, !shown.isEmpty,
              let now = try? await e.limitOrders(near: nearAddress, sync: false).map(\.view) else { return }
        var left: [(Executor.LimitOrderView, Executor.LimitOrderEnd)] = []
        for o in shown where !now.contains(where: { $0.id == o.id }) {
            // Unread: the board stays as it was and the next tick asks again, so a fill is never missed.
            guard let end = await e.limitOrderEnd(o.id) else { return }
            left.append((o, end))
        }
        // Changed meanwhile (a cancel, a new order, another wallet): this read is the old one's.
        guard limitOrderList == shown, previewWallet?.solana == w.solana else { return }
        limitOrderList = now.isEmpty ? nil : now
        for (o, _) in left { limitPayouts[o.id] = 20 }
        for (o, end) in left where ["FILLED", "EXPIRED"].contains(end.status) {
            let lines = Self.limitEndWords(o, end)
            let body = say(lines.en, lines.ru)
            reply(body, note: "NEAR Intents · limit order")
            notify(.orders, end.status == "FILLED" ? say("Limit order filled: \(o.coin)", "Лимитка исполнилась: \(o.coin)")
                                                   : say("Limit order expired: \(o.coin)", "Лимитка истекла: \(o.coin)"),
                   body, record: false, schedule: "limit:\(o.coin)")
        }
    }

    /// The open orders said in the chat: each one, not only their count. nil for none.
    nonisolated static func limitListWords(_ list: [Executor.LimitOrderView]) -> (en: String, ru: String)? {
        guard !list.isEmpty else { return nil }
        let en = list.map { "\($0.sell ? "sell" : "buy") \(Fmt.num($0.coins, maxFraction: 6)) \($0.coin) at \(Fmt.usd($0.price))" }.joined(separator: " · ")
        let ru = list.map { "\($0.sell ? "продажа" : "покупка") \(Fmt.num($0.coins, maxFraction: 6)) \($0.coin) по \(Fmt.usd($0.price))" }.joined(separator: " · ")
        return list.count == 1 ? ("One open limit order: \(en). It's on the canvas.", "Один открытый лимитный ордер: \(ru). Он на холсте.")
            : ("\(list.count) open limit orders: \(en). They're on the canvas.", "Открытые лимитные ордера, \(list.count): \(ru). Они на холсте.")
    }

    /// A filled or expired limit order in words: what went in and what comes out, at what price with every fee in.
    nonisolated static func limitEndWords(_ o: Executor.LimitOrderView, _ end: Executor.LimitOrderEnd) -> (en: String, ru: String) {
        let coin = o.coin, cash = o.sell ? "USDC" : coin, limit = Fmt.usd(o.price)
        let order = "\(o.sell ? "sell" : "buy") \(Fmt.num(o.coins, maxFraction: 8)) \(coin) at \(limit)"
        let orderRU = "\(o.sell ? "продажа" : "покупка") \(Fmt.num(o.coins, maxFraction: 8)) \(coin) по \(limit)"
        var en = "", ru = ""
        if let price = end.price {
            let gave = Fmt.num(end.spent, maxFraction: 8), got = Fmt.num(end.got, maxFraction: o.sell ? 2 : 6)
            en = o.sell ? "sold \(gave) \(coin) for ≈ \(got) USDC" : "bought ≈ \(got) \(coin) for \(gave) USDC"
            ru = o.sell ? "продано \(gave) \(coin) за ≈ \(got) USDC" : "куплено ≈ \(got) \(coin) за \(gave) USDC"
            en += ", \(Fmt.usd(price)) a \(coin) with 1Click's fees and the network's (the order's price \(limit))"
            ru += ", \(Fmt.usd(price)) за \(coin) с комиссиями 1Click и сети (цена ордера \(limit))"
        }
        if end.status == "FILLED" {
            return en.isEmpty ? ("Limit order filled: \(order). The \(cash) comes to your own address.", "Лимитка исполнилась: \(orderRU). \(cash) придут на твой адрес.")
                : ("Limit order filled: \(en). The \(cash) comes to your own address.", "Лимитка исполнилась: \(ru). \(cash) придут на твой адрес.")
        }
        return ("Limit order expired: \(order)." + (en.isEmpty ? "" : " Before that it \(en).") + " What didn't fill comes back to the address it came from.",
                "Лимитка истекла: \(orderRU)." + (ru.isEmpty ? "" : " До этого \(ru).") + " Что не исполнилось, вернётся на адрес, откуда пришло.")
    }

    /// The two cards after a move the loss cap stopped: what they carry instead of a chat command.
    static let sendAnyway = "send anyway", leaveCostly = "leave it as is", sendPart = "send the part that moves the price about 1%"

    private func movesMoney(_ command: WalletCommand, confirmed: Bool) -> Bool {
        switch command {
        case .yields, .sendTo, .receive, .schedule, .stopSchedules, .jupiterDCAOrders, .limitOrders: false
        case .allocate, .rebalance: confirmed
        default: true
        }
    }

    /// The USDC a move takes from a network read through Blockscout (Polygon, Ethereum, Tempo, HyperEVM, Optimism), read
    /// from its node now: Blockscout indexes a few seconds behind, and «move all USDC from Optimism to Solana» two seconds
    /// after 2 USDC landed there was refused with «Optimism has 0 USDC». Base, Arbitrum and Robinhood are read live anyway.
    func freshenSource(_ command: WalletCommand) async {
        // The USDT a move takes, the same way on each of USDT0's networks: 0.7 USDT that LayerZero had just landed on
        // Polygon read as «Polygon has 0 USDT» (2026-10-13).
        if case .usdt(let from, _, _, _) = command {
            guard let net = EVMNetwork.named(from), let token = USDT0.on(net.rpc.chainId)?.token, isEVMAddress(hlAddress),
                  let units = try? await net.rpc.tokenBalance(token, owner: hlAddress) else { return }
            let known = otherTokens.first { $0.chain == from && $0.contract == token }
            let fresh = Holdings.dollar(from, contract: token, amount: Decimal(units) / 1_000_000, symbol: known?.symbol ?? "USDT", icon: known?.icon)
            otherTokens = otherTokens.filter { !($0.chain == from && $0.contract == token) } + (units > 0 ? [fresh] : [])
            usdtRead = (from, fresh.amount, Date())
            return
        }
        let place: String? = switch command {
        case .evmToSolana(let n, _), .bridge(from: let n, to: _, _), .cctp(from: let n, to: _, _), .intents(from: let n, to: _, _): n
        case .buyOn(_, _, _, let from?): from
        default: nil
        }
        guard let place, !["base", "arbitrum", "robinhood", "solana"].contains(place), let net = EVMNetwork.named(place), isEVMAddress(hlAddress),
              let units = try? await net.usdcBalance(hlAddress) else { return }
        let fresh = Holdings.dollar(place, contract: net.usdc, amount: Decimal(units) / 1_000_000,
                                    icon: otherTokens.first { $0.chain == place && $0.contract == net.usdc }?.icon)
        otherTokens = otherTokens.filter { !($0.chain == place && $0.contract == net.usdc) } + (units > 0 ? [fresh] : [])
    }

    /// The wallet's tokens of a Backpack stock on Solana, by its mint, or its SOL; nil before the balance loads.
    func heldOnSolana(_ stock: String) -> Decimal? {
        if stock == "SOL" { return solana?.sol }
        guard let s = solana, let mint = WalletCommand.kaminoStockMints[stock] else { return nil }
        return s.tokens.first { $0.contract == mint }?.amount ?? 0
    }

    /// A loan against SOL with none up on Kamino yet: the words that put up SOL worth 2.5 times the loan and borrow in one
    /// go (a health factor of 2 at Kamino's 80% liquidation line for SOL, 2026-11-05), or the loan the wallet's SOL
    /// carries. 0.05 SOL stays out of it for the fees and the first deposit's rent (≈ 0.04); the move checks them exactly.
    func solLoanWay(_ usdc: Decimal, sol: Decimal) -> String? {
        guard let price = markets["SOL"]?.markPx, price > 0 else { return nil }
        let free = sol - Decimal(string: "0.05")!, need = (usdc * 5 / 2 / price).rounded(2, .up), x = Fmt.num(usdc)
        if need <= free {
            let n = Fmt.num(need, maxFraction: 2)
            return say("Nothing is up on Kamino yet. «borrow \(x) USDC on kamino against \(n) SOL» puts \(n) SOL up (≈ \(Fmt.usd(need * price)), 2.5 times the loan) and borrows in one go, nothing sold.",
                       "В Kamino пока ничего не заложено. «borrow \(x) USDC on kamino against \(n) SOL» заложит \(n) SOL (≈ \(Fmt.usd(need * price)), в 2,5 раза больше займа) и займёт за один раз, ничего не продавая.")
        }
        let have = Fmt.num(sol, maxFraction: 4)
        guard free >= Decimal(string: "0.01")! else {
            return say("Kamino lends USDC against SOL put up there, and the wallet has \(have) SOL: too little to put up and still pay the fees. Top up SOL first.",
                       "Kamino даёт USDC под заложенный SOL, а в кошельке \(have) SOL — слишком мало, чтобы заложить и заплатить комиссии. Сначала пополни SOL.")
        }
        let n = Fmt.num(free.rounded(2, .down), maxFraction: 2), can = Fmt.num((free * price * 2 / 5).rounded(2, .down))
        return say("The wallet's \(have) SOL carries about \(can) USDC at a health factor of 2: «borrow \(can) USDC on kamino against \(n) SOL» puts it up and borrows, a little SOL stays for the fees.",
                   "Твои \(have) SOL выдержат заём около \(can) USDC при факторе здоровья 2: «borrow \(can) USDC on kamino against \(n) SOL» заложит их и займёт, немного SOL останется на комиссии.")
    }

    /// A move that can't go through with what the wallet holds is said so at once, instead of a card whose Confirm
    /// could only fail («Sell 0 NEAR»).
    func refusalBeforeCard(_ command: WalletCommand, inChain: Bool = false) -> String? {
        if !inChain, let why = fundingShortfall(command) { return why }
        /// A move from a place that hasn't the money: said before the card, not after a failed signature. A balance not
        /// loaded yet leaves it to the move's own checks.
        func lacks(_ have: Decimal?, _ asked: Decimal?, _ place: String, _ unit: String = "USDC") -> String? {
            guard let have else { return nil }
            if have <= 0 {
                return say("\(place) has \(Fmt.num(have)) \(unit), so there’s nothing to move from there — nothing was prepared.",
                           "На \(place) \(Fmt.num(have)) \(unit) — оттуда нечего переводить, ничего не готовлю.")
            }
            if let asked, asked > have {
                return say("Only \(Fmt.num(have)) \(unit) is on \(place), less than \(Fmt.num(asked)). Say an amount up to that.",
                           "На \(place) только \(Fmt.num(have)) \(unit) — меньше, чем \(Fmt.num(asked)). Назови сумму не больше.")
            }
            return nil
        }
        switch command {
        case .withdrawToSolana(let asked): return lacks(hl?.withdrawable, asked, "Hyperliquid")
        case .kamino(deposit: false, symbol: "USDC", amount: let asked): return lacks(kaminoVault("USDC")?.tokens ?? kamino, asked, "Kamino")
        case .lendWithdraw("USDC", let asked): return lacks(lent("USDC"), asked, "Jupiter Lend")
        case .baseToSolana(let asked): return lacks(base?.usdc, asked, "Base")
        // «с базы на арбитрум» 50 with 1 there drew «Move 1 USDC» under «Понял: move 50»: the amount is said before the card.
        // Not inside a chain: a step before it may have just brought the money, and those balances load after the refresh.
        case .evmToSolana(let net, let asked) where !inChain, .bridge(from: let net, to: _, let asked) where !inChain:
            return lacks(usdc(on: net), asked, WalletCommand.placeName(net), net == "robinhood" ? "USDG" : "USDC")
        case .intents(let from, _, let asked) where !inChain:
            return lacks(from == "solana" ? solana?.usdc : usdc(on: from), asked, WalletCommand.placeName(from))
        // A private swap pays its USDC from the network named, its NEAR from the NEAR account: said before the card.
        case .swapPrivately(let a, let from, _, _, let asked) where !inChain && a.uppercased() == "USDC":
            return lacks(from == "solana" ? solana?.usdc : usdc(on: from), asked, WalletCommand.placeName(from))
        case .swapPrivately("NEAR", "near", _, _, let asked) where !inChain:
            return lacks(near, asked, "NEAR", "NEAR")
        // A limit order's deposit: a buy's USDC from its network, a sale's NEAR from the NEAR account, SOL from Solana.
        case .limitOrder(sell: false, coin: _, on: _, price: _, amount: let asked, usdcOn: let from, days: _) where !inChain:
            return lacks(from == "solana" ? solana?.usdc : usdc(on: from), asked, WalletCommand.placeName(from))
        case .limitOrder(sell: true, coin: "NEAR", on: "near", price: _, amount: let asked, usdcOn: _, days: _) where !inChain:
            return lacks(near, asked, "NEAR", "NEAR")
        case .limitOrder(sell: true, coin: "SOL", on: "solana", price: _, amount: let asked, usdcOn: _, days: _) where !inChain:
            return lacks(solana?.sol, asked, "Solana", "SOL")
        case .usdt(let net, _, let asked, _) where !inChain:
            return lacks(heldUSDT(net), asked, WalletCommand.placeName(net), "USDT")
        // A buy that names where its money comes from («с арбитрума») pays from there or not at all: said before the card.
        case .buyOn(_, _, let asked?, let from?) where !inChain && from != "solana":
            return lacks(usdc(on: from), asked, WalletCommand.placeName(from), from == "robinhood" ? "USDG" : "USDC")
        case .hlp(deposit: false, amount: let asked):
            if let until = hlp.lockedUntil, until > Date(), hlp.equity > 0 {
                return say("HLP is locked until \(Fmt.date(until)): Hyperliquid lets deposits out 4 days after they go in.",
                           "HLP заблокирован до \(Fmt.date(until)): Hyperliquid выпускает депозит через 4 дня после внесения.")
            }
            return lacks(hlp.equity, asked, "HLP")
        case .lendDeposit("USDC", let asked?), .kamino(deposit: true, symbol: "USDC", amount: let asked?): return lacks(solana?.usdc, asked, "Solana")
        // Backpack's stocks put up on Kamino come from the wallet; a repay pays from its USDC on Solana.
        case .kamino(deposit: true, symbol: let s, amount: let asked?) where WalletCommand.kaminoStockMints[s] != nil:
            return lacks(heldOnSolana(s), asked, "Solana", s)
        case .kaminoBorrow(_, against: let s?, adding: let asked?) where !inChain: return lacks(heldOnSolana(s), asked, "Solana", s)
        // Against SOL with none up on Kamino's main market: how much to put up, from the wallet's SOL.
        case .kaminoBorrow(let usdc, against: .some("SOL"), adding: nil) where !inChain:
            guard let sol = solana?.sol, kaminoLoanRead, !kaminoDeposits.contains(where: { $0.coin == "SOL" && $0.usd > 0 }) else { return nil }
            return solLoanWay(usdc ?? 1, sol: sol)
        // A loan with nothing put up yet, on a stock named or none: the way to put one up, or to get it, before any card.
        case .kaminoBorrow(let usdc, against: let named, adding: nil) where !inChain && named.map { WalletCommand.kaminoStockMints[$0] != nil } ?? true:
            // None named borrows on the main market too, against SOL put up there.
            guard solana != nil, kaminoLoanRead, (kaminoLoan?.depositUSD ?? 0) <= 0,
                  named != nil || !kaminoDeposits.contains(where: { $0.coin == "SOL" && $0.usd > 0 }) else { return nil }
            // Unnamed: the Backpack stock the wallet holds, else SOL when there's some, else SpaceX's for the example.
            let stockHeld = WalletCommand.kaminoStocks.first { (heldOnSolana($0) ?? 0) > 0 }
            if named == nil, stockHeld == nil, let sol = solana?.sol, sol > Decimal(string: "0.06")!, let way = solLoanWay(usdc ?? 1, sol: sol) { return way }
            let s = named ?? stockHeld ?? "SPCX"
            let held = heldOnSolana(s) ?? 0, n = Fmt.num(held, maxFraction: 6), x = usdc.map { Fmt.num($0) } ?? "1"
            return held > 0
                ? say("Nothing is up on Kamino's Backpack market yet. The wallet has \(n) \(s): «borrow \(x) USDC on kamino against \(n) \(s)» puts it up and borrows in one go.",
                      "На рынке Backpack в Kamino пока ничего не заложено. В кошельке \(n) \(s): «borrow \(x) USDC on kamino against \(n) \(s)» заложит их и займёт за один раз.")
                : say("There's no \(s) on Kamino or in the wallet. Buy it first — «buy \(s) on solana for 5 USDC» — then borrow against it.",
                      "\(s) нет ни в Kamino, ни в кошельке. Сначала купи — «купи \(s) на солане на 5 USDC», — потом займи под него.")
        case .kaminoRepay(let asked, _) where !inChain:
            if solana != nil, kaminoLoanRead, (kaminoLoan?.debtUSD ?? 0) <= 0, kamino <= 0 {
                return say("No USDC debt on Kamino — nothing to pay back.", "Долга в USDC на Kamino нет — возвращать нечего.")
            }
            return lacks(solana?.usdc, asked, "Solana")
        // All of it goes in at once, and Jupiter starts at 100: «say less» would only meet its minimum.
        case .jupiterDCA(_, let asked?, _, _):
            if let have = solana?.usdc, asked > have {
                return have < WalletCommand.jupiterDCAMinimum
                    ? say("Only \(Fmt.num(have)) USDC is on Solana, and Jupiter's DCA starts at 100 — all of it goes in at once. Top up Solana first.",
                          "На Solana только \(Fmt.num(have)) USDC, а DCA у Jupiter — от 100, и вся сумма вносится сразу. Сначала пополни Solana.")
                    : say("Only \(Fmt.num(have)) USDC is on Solana, and the DCA puts all \(Fmt.num(asked)) in at once. Say up to \(Fmt.num(have)).",
                          "На Solana только \(Fmt.num(have)) USDC, а DCA вносит все \(Fmt.num(asked)) сразу. Назови сумму до \(Fmt.num(have)).")
            }
            if let sol = solana?.sol, sol < Decimal(string: "0.007")! {
                return say("The DCA's order account takes ≈ 0.006 SOL of rent, back when it ends; Solana has \(Fmt.num(sol, maxFraction: 4)) SOL.",
                           "Аккаунт ордера DCA берёт ≈ 0.006 SOL залога (вернётся в конце), а на Solana \(Fmt.num(sol, maxFraction: 4)) SOL.")
            }
            return nil
        // «move 100 USDC to arbitrum» read as from Solana, where 1.69 lay: said before the card, with where the money is.
        case .solanaTo(_, let asked?) where !inChain:
            guard let why = lacks(solana?.usdc, asked, "Solana") else { return nil }
            guard let net = EVMNetwork.funding(asked, held: { self.usdc(on: $0) }) else { return why }
            return why + say(" \(net.name) has \(Fmt.num(usdc(on: net.key) ?? 0)) USDC: say «from \(net.name)» to move it from there.",
                             " На \(net.name) — \(Fmt.num(usdc(on: net.key) ?? 0)) USDC: скажи «с \(net.name)», чтобы перевести оттуда.")
        case .lendDeposit("SOL", let asked?), .kamino(deposit: true, symbol: "SOL", amount: let asked?), .stake(let asked?):
            return lacks(solana?.sol, asked, "Solana", "SOL")
        case .sell(let a, let asked) where a.resolved:
            // «продай весь мой JUP» with none made a card that could only fail at the swap.
            guard let s = solana else { return nil }
            let have = a.symbol == "SOL" ? s.sol : s.tokens.first { $0.contract == a.mint }?.amount ?? 0
            if have <= 0 {
                return say("There’s no \(a.symbol) on Solana to sell — nothing was prepared.", "На Solana нет \(a.symbol) — продавать нечего, ничего не готовлю.")
            }
            return lacks(have, asked, "Solana", a.symbol)
        case .sweepDust(let under):
            guard dustTokens(under: under).isEmpty else { return nil }
            return say("No verified token worth $0.20 to \(Fmt.usd(under ?? 5)) on Base, Arbitrum or Polygon — no dust to sweep, nothing was prepared.",
                       "На Base, Arbitrum и Polygon нет проверенных токенов от $0.20 до \(Fmt.usd(under ?? 5)) — пыли нет, ничего не готовлю.")
        case .sellNear(let asked, _):
            // What a sale keeps on the account while it is sent: storage and prepaid gas (sellNearRoute, intentDepositOverhead).
            let sellable = max(0, near - Decimal(string: "0.09")!)
            let amount = asked ?? sellable
            guard amount < Decimal(string: "0.05")! || amount > sellable else { return nil }
            return "You have \(Fmt.num(near, maxFraction: 4)) NEAR. A sale needs at least 0.05 NEAR, and ≈ 0.09 NEAR stays on the account for storage and gas while it is sent, so \(sellable < Decimal(string: "0.05")! ? "there is nothing to sell now" : "at most \(Fmt.num(sellable, maxFraction: 4)) NEAR can be sold")."
        default:
            return nil
        }
    }

    /// What «sweep dust» sells: the verified tokens (a market list prices them) on Base, Arbitrum and Polygon worth from
    /// $0.20 up to `under` (nil = $5), the most first; the network's own coin stays, it's gas.
    func dustTokens(under: Decimal?) -> [Executor.Dust] {
        holdings.filter { Executor.dustNetworks.contains($0.chain) && $0.verified && !$0.native && $0.contract != "native" }
            .compactMap { h in
                h.usd.flatMap { u in u >= Decimal(string: "0.2")! && u < (under ?? 5) ? Executor.Dust(chain: h.chain, contract: h.contract, symbol: h.symbol, usd: u) : nil }
            }
            .sorted { $0.usd > $1.usd }
    }

    /// «Buy the token 0x… for 2 USDC on Robinhood Chain»: the summary as a canvas title.
    static func canvasTitle(_ command: WalletCommand) -> String { command.summary.prefix(1).uppercased() + command.summary.dropFirst() }

    /// Whether the command went through. `quiet`: a guardian's try again, whose failure was already said once.
    @discardableResult
    func manage(_ command: PositionCommand, coin: String, quiet: Bool = false) async -> Bool {
        if let missing = missingTradingKey { if !quiet { reply(missing) }; return false }
        updateActivity { a in
            a.coin = coin
            if a.title == nil { a.title = "\(IntentParser.pretty(coin)): \(Self.describe(command))" }
            if a.bubbles.isEmpty { a.bubbles = [ActivityBubble(kind: "position", label: "Position · Hyperliquid", title: Self.describe(command), asset: coin)] }
        }
        guard let wallet = signerWallet else {
            reply("Managing a position works in the app once the key is in secure storage.")
            return false
        }
        if case .trail = command, !positions.contains(where: { $0.coin == coin }) {
            reply("There is no open \(IntentParser.pretty(coin)) position to trail.")
            return false
        }
        // As in move(): only what this call set is reset, so a stop moved during a running move leaves that move's board.
        let ownsExecuting = !executing
        if ownsExecuting { executing = true }
        // A close keeps its card on the canvas while it goes through, then leaves it as Done or Failed: nothing vanishes.
        let shown = if case .close(let share) = command { closeRoute(share, coin: coin) } else { nil as (title: String, steps: [ActivityBubble])? }
        defer {   // the board first: `executing` going false starts a chain's next step
            if shown != nil { runningAction = nil }
            if ownsExecuting { executing = false }
        }
        if let shown { runningAction = (shown.title, shown.steps, [], Date()) }
        var ok = true
        do { _ = try await executor(wallet).manage(command, asset: coin, policy: policy, equity: equity) } catch {
            ok = false
            if !quiet {
                reply(say("Failed: ", "Не вышло: ") + error.localizedDescription)
                noteFailure(shown?.title ?? "\(command) on \(IntentParser.pretty(coin))", error)
            }
        }
        // The exchange trails it now: the app's own trailing for this coin would only move the fixed stop after it.
        if ok, case .trail = command, exitRules[coin]?.trailingPct != nil { exitRules[coin]?.trailingPct = nil }
        if let shown {
            runningAction = nil
            finishedAction = (shown.title, CanvasOutcome(at: Date(), ok: ok), shown.steps)
            saveCanvas()
        }
        await refresh()
        return ok
    }

    /// A part of a position worth under Hyperliquid's $10 minimum can't be closed: said before any card, with closing it
    /// all as a card. True when it was said.
    func closeTooSmall(_ share: Decimal, coin: String, note: String? = nil) -> Bool {
        guard let p = positions.first(where: { $0.coin == coin }), let m = markets[coin],
              Executor.partialCloseRefusal(position: p.size, fraction: share, mark: m.markPx, szDecimals: m.szDecimals, asset: coin) != nil else { return false }
        let name = IntentParser.pretty(coin), part = Fmt.usd(abs(p.size) * share * m.markPx), whole = Fmt.usd(abs(p.size) * m.markPx)
        reply(say("Closing \(Fmt.pct(share * 100)) of \(name) is \(part): Hyperliquid won't take a close under $10. Close it all, \(whole), or a share worth $10 or more.",
                  "\(Fmt.pct(share * 100)) позиции \(name) — это \(part): Hyperliquid не берёт закрытие меньше $10. Закрой её целиком, \(whole), или долю от $10."), note: note)
        offerCards([ChatRouter.Option(title: say("Close it all · \(whole)", "Закрыть всё · \(whole)"),
                                      comment: say("At the market, the stop and take-profit go with it", "По рынку, стоп и тейк уходят вместе с ней"),
                                      command: "close \(coin.hasPrefix("xyz:") ? String(coin.dropFirst(4)) : coin)", recommended: true)])
        return true
    }

    /// The card shows the position instead of a plan: just executed, or a position in this asset is already open
    /// (after a restart too). One plan per asset: the rest is done with position commands.
    var managesPosition: Bool { executed == intent || positions.contains { $0.coin == intent.asset } }

    /// The market picker: sets the plan's instrument as if the user had typed it.
    func choose(_ market: Market) async {
        guard market.name != intent.asset else { return }
        let before = intent
        intent.asset = market.name
        reply("Market: \(IntentParser.pretty(market.name)) · Hyperliquid\(market.dex.map { " \($0)" } ?? "") — \(market.group.rawValue.lowercased()). " + (intent.changes(from: before).isEmpty ? "" : "Plan recalculated.") + outcomeLine)
        await refreshQuote()
        startPlanReview()
    }

    /// Markets of one group, most traded first.
    func markets(_ group: Market.Group, limit: Int) -> [Market] {
        Array(markets.values.filter { $0.group == group }.sorted { $0.volume24h > $1.volume24h }.prefix(limit))
    }

    /// The chat's «lock»: the next signature asks for Touch ID, and orders stop going through the trading key.
    func lockSigner() async {
        guard SignerClient.isAvailable else { return }
        let ok = (try? await SignerClient().send(.lock))?.ok ?? false
        await loadSigner()
        AppLock.shared.lock()
        reply(ok ? "🔒 Locked. The next signature asks for \(Presence.name), and the trading key is paused — resume it in Settings › Wallet, or say «resume the trading key»."
                 : "Couldn’t reach the signing process to lock it.")
    }

    /// Whether this Hyperliquid account approved ihatecrypto's fee, for Settings; nil when Hyperliquid didn't answer.
    func loadFeeApproval() async {
        let user = hlAddress
        let approved = user.isEmpty ? nil : await Executor.allowedFee(user).map { $0 >= HLBuilder.fee }
        if user == hlAddress { feeApproved = approved }
    }

    /// Takes back the approval of ihatecrypto's fee (the signer asks first). The next trade confirmed asks for it again.
    func revokeFee() async {
        guard let wallet = signerWallet else { return }
        executing = true
        defer { executing = false }
        do {
            try await executor(wallet).setFeeApproval(false)
            reply("ihatecrypto's fee approval is taken back. New orders go without it; stops and take-profits already on the exchange keep it until they fill or move. The next Hyperliquid trade you confirm asks for it again.")
        } catch {
            reply("The fee approval stayed: \(error.localizedDescription)")
        }
        await loadFeeApproval()
    }

    /// Card commands of the second check's notice below.
    static let jevOpenRouterOn = "let the second check use openrouter", jevOpenRouterOff = "keep the second check on the chat's model"

    /// Before 0.7.0 an OpenRouter key on this Mac made the second check send texts there by itself; since a8ea990 only
    /// when the user turns it on. Whoever had that hears it once, with the switch as a card (the 2026-10-22 review).
    func noteSecondCheckChange() {
        let d = UserDefaults.standard
        guard d.object(forKey: "jevOpenRouter") == nil, !d.bool(forKey: "jevOpenRouterNoted"), jevScreening, providerID != "openrouter",
              openRouterKey != nil, (d.string(forKey: "onboardingStep") ?? "").isEmpty, liveSteps == nil else { return }
        d.set(true, forKey: "jevOpenRouterNoted")
        reply(say("The second check no longer sends your texts to OpenRouter by itself: your chat's model reads each command and plan twice. To have OpenRouter's model check them too, as before, turn it on — Settings › Second check has the same switch.",
                  "Вторая проверка больше не отправляет тексты в OpenRouter сама: каждую команду и план дважды читает модель чата. Чтобы, как раньше, их проверяла ещё и модель OpenRouter, включи её — тот же переключатель есть в Settings › Second check."))
        offerCards([
            ChatRouter.Option(title: say("Use OpenRouter for the second check", "Проверять ещё и через OpenRouter"),
                              comment: say("It receives your messages, lookups and plans", "Туда уйдут сообщения, найденное и планы"), command: Self.jevOpenRouterOn),
            ChatRouter.Option(title: say("Keep it on the chat's model", "Оставить модель чата"), comment: "Settings › Second check", command: Self.jevOpenRouterOff),
        ])
    }

    /// Card commands of the offer below.
    static let tradingKeyOn = "approve the hyperliquid trading key", tradingKeyLater = "leave the hyperliquid trading key off"

    /// Hyperliquid's first money on this wallet (a move, a trade's bridge, a deposit from outside): its trading key can be
    /// approved now, so the chat offers it, once per address. Nothing signs until the card is picked and Touch ID says so.
    func offerTradingKey() {
        guard !agentActive, agentAddress == nil, let evm = signerWallet?.evm, evm.lowercased() == hlAddress.lowercased(),
              let hl, hl.accountValue + hl.spotUSDC > 0, (UserDefaults.standard.string(forKey: "onboardingStep") ?? "").isEmpty,
              liveSteps == nil, pendingAction == nil, !executing, !busy else { return }
        let key = "tradingKeyOffered-" + evm.lowercased()
        guard !UserDefaults.standard.bool(forKey: key) else { return }
        UserDefaults.standard.set(true, forKey: key)
        reply(say("Hyperliquid is open on this wallet. Turn on its trading key? Orders, stops and closes then go without \(ledgerActive ? "the Ledger's buttons" : Presence.name) — new positions for 7 days, then one \(Presence.short) renews them; the key can't withdraw.",
                  "Hyperliquid на этом кошельке открыт. Включить торговый ключ? Тогда ордера, стопы и закрытия идут без \(ledgerActive ? "кнопок Ledger" : Presence.ruGen) — новые позиции 7 дней, потом одно подтверждение продлевает; вывести деньги ключ не может."))
        offerCards([
            ChatRouter.Option(title: say("Turn on the trading key", "Включить торговый ключ"),
                              comment: say("One \(ledgerActive ? "approval on the Ledger" : Presence.name); it can't withdraw", "Одно подтверждение \(ledgerActive ? "на Ledger" : Presence.ruIns); вывести не может"),
                              command: Self.tradingKeyOn, recommended: true),
            ChatRouter.Option(title: say("Not now", "Не сейчас"), comment: "Settings › Wallet › Hyperliquid", command: Self.tradingKeyLater),
        ])
    }

    /// Turns the paused trading key back on; the signer asks for Touch ID first.
    func resumeTradingKey() async {
        do {
            let r = try await SignerClient().send(ledgerActive ? .setLedgerAgentActive(owner: hlAddress, active: true) : .setAgentActive(true))
            await loadSigner()
            reply(r.ok && r.agentActive == true ? "Trading key on again for 7 days: orders skip \(Presence.name), withdrawals still need it." : "The trading key stayed off.")
        } catch {
            reply("The trading key stayed off: \(error.localizedDescription)")
        }
    }

    /// The signer makes an agent key and signs its approval with the main key; the approval goes to Hyperliquid,
    /// and only when the exchange accepts it do orders switch to the agent.
    func enableAgent() async {
        // Hyperliquid refuses a key for an account that has never had a deposit: say what opens it, and offer the way,
        // before Touch ID or the Ledger (the owner, 2026-10-23: «давайте пополним… хотя бы 1 доллар и минимальная ставка 10»).
        if let evm = signerWallet?.evm, (try? await Hyperliquid.isNewAccount(evm)) == true {
            reply(say("Hyperliquid isn't open on this wallet yet. It opens with the first USDC that lands there: about $1 of it pays the opening fee, and the smallest order is $10, so bring at least $11. Then the trading key turns on with one \(ledgerActive ? "approval on the Ledger" : Presence.name).",
                      "Hyperliquid на этом кошельке ещё не открыт. Он открывается первыми USDC, которые туда придут: около $1 уходит на открытие, а самый маленький ордер — $10, так что нужно хотя бы $11. После этого торговый ключ включается одним подтверждением \(ledgerActive ? "на Ledger" : Presence.ruIns)."))
            let canMove = (solana?.usdc ?? 0) >= 11
            offerCards((canMove ? [ChatRouter.Option(title: say("Move 11 USDC to Hyperliquid", "Перевести 11 USDC на Hyperliquid"),
                                                     comment: say("From Solana; the card waits for your Confirm", "С Solana; карточка ждёт Confirm"),
                                                     command: "move 11 USDC from Solana to Hyperliquid", recommended: true)] : [])
                       + [ChatRouter.Option(title: say("Top up ihatecrypto", "Пополнить ihatecrypto"),
                                            comment: say("From a browser wallet, your phone or an exchange", "Из кошелька в браузере, с телефона или с биржи"),
                                            command: WalletCommand.receive.summary, recommended: !canMove)])
            return
        }
        if ledgerActive { await enableLedgerAgent(); return }
        executing = true
        defer { executing = false }
        do {
            let answer = try await SignerClient().send(.enableAgent)
            guard answer.ok, let action = answer.action, let signature = answer.signature, let nonce = answer.nonce, let agent = answer.agentAddress else {
                throw GuardStop(XPCTradeSigner.reason(answer))
            }
            let result = HLReply.parse(try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true))
            guard !result.isError else { throw ExecutionError("Hyperliquid rejected the agent: \(result).") }
            _ = try await SignerClient().send(.setAgentActive(true))
            await loadSigner()
            reply("Trading key on: \(agent.prefix(10))…. Orders, stops and closes now skip \(Presence.name); this key can't withdraw money — withdrawals still go through the main key.")
        } catch {
            reply("Trading key not turned on: \(error.localizedDescription)")
        }
    }

    /// The app-long loop: refresh, exit rules, regular purchases. Faster while exit rules exist, so a trailing
    /// stop does not lag a minute behind the price.
    func watch() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(exitRules.isEmpty && alerts.isEmpty && loanGuards.isEmpty ? 30 : 10))
            await refresh()
            checkAlerts()
            await checkLoanGuards()
            await guardPositions()
            if limitFollow == nil {
                limitFollow = Task { [weak self] in
                    await self?.followLimitOrders()
                    self?.limitFollow = nil
                }
            }
            // A scheduled buy can wait on Touch ID with no one at the Mac: it runs beside the loop, so refreshes and alerts
            // go on meanwhile (the 2026-10-22 review). While it signs, `executing` keeps other moves from starting.
            if scheduledRun == nil {
                scheduledRun = Task { [weak self] in
                    await self?.runSchedules()
                    await self?.runDueTask()
                    self?.scheduledRun = nil
                }
            }
            remindTopUp()
        }
    }

    /// After a buy's run: its next date, or the one it had when «Run now» made this run an extra one.
    func advanceSchedule(_ i: Int) {
        schedules[i].advance()
        if let kept = TaskRun.kept.removeValue(forKey: "buy:\(schedules[i].symbol)") { schedules[i].next = kept }
    }

    /// Due regular purchases, one run each; the next date moves on even after a failure, so it does not retry in a loop.
    func runSchedules() async {
        guard let wallet = signerWallet, !executing else { return }
        // ponytail: a buy due while paused runs at once when switched back on; advance it on resume if that bothers.
        // By symbol (one schedule each), found again after every await: «stop recurring buys» or a wallet switch during a
        // buy's Touch ID changes the list, and an index kept across it crashed.
        func find(_ symbol: String) -> Int? {
            guard signerWallet?.solana == wallet.solana else { return nil }
            return schedules.firstIndex { $0.symbol == symbol }
        }
        for symbol in schedules.filter({ $0.next <= Date() && $0.paused != true }).map(\.symbol) {
            guard !schedulePaused, let index = find(symbol), schedules[index].next <= Date(), schedules[index].paused != true,
                  let asset = schedules[index].asset else { continue }
            // Jupiter or Solana switched off: this run is skipped, the schedule stays.
            if let off = Modules.current.refusal(.jupiter, ru: modulesRussian) {
                advanceSchedule(index)
                notify(.moves, say("Scheduled \(asset.symbol) buy skipped", "Регулярная покупка \(asset.symbol) пропущена"), off, schedule: "buy:\(asset.symbol)", failed: true)
                continue
            }
            // On a Ledger a purchase needs the device and the person: while it's away the run stays due and says so once;
            // when it's back, the buy runs and asks for its approval there.
            if ledgerActive, !(await ledgerReachable()) {
                if ledgerWaitNotified.insert(asset.symbol).inserted {
                    notify(.signature, say("Scheduled \(asset.symbol) buy waits for your Ledger", "Регулярная покупка \(asset.symbol) ждёт Ledger"),
                           say("Connect and unlock the Ledger: the buy runs then and asks you to approve it on the device.",
                               "Подключи и разблокируй Ledger: покупка запустится и попросит подтверждение на устройстве."), schedule: "buy:\(asset.symbol)")
                }
                continue
            }
            ledgerWaitNotified.remove(asset.symbol)
            // A move the user started meanwhile has the flag: this buy waits for the next round rather than share it.
            guard !executing, let index = find(symbol) else { continue }
            let usdc = schedules[index].usdc
            advanceSchedule(index)
            executing = true
            let waiting = Task { @MainActor [weak self] in
                try await Task.sleep(for: .seconds(4))
                guard let self, SignerActivity.shared.atDevice else { return }
                notify(.signature, say("Scheduled \(asset.symbol) buy", "Регулярная покупка \(asset.symbol)"),
                       ledgerActive ? say("Waiting for your Ledger — approve it on the device.", "Ждёт Ledger — подтверди на устройстве.")
                                    : AppLock.shared.codeSigns ? say("Waiting for your code — type it in ihatecrypto's chat field.", "Ждёт твой код — введи его в поле чата ihatecrypto.")
                                    : Presence.touchID ? say("Waiting for Touch ID — confirm it on the sensor.", "Ждёт Touch ID — подтверди на датчике.")
                                    : say("Waiting for your Mac password — type it when macOS asks.", "Ждёт пароль от Mac — введи его, когда macOS спросит."), record: false)
            }
            defer { waiting.cancel() }
            // Nobody may be at the Mac: a session the user opened for 5–30 minutes doesn't cover this buy, it asks again.
            if SignerClient.isAvailable { _ = try? await SignerClient().send(.endSession) }
            let started = Date()
            do {
                let line = try await executor(wallet).swap(asset, buy: true, amount: usdc, equity: equity)
                notify(.moves, say("\(asset.symbol) purchase", "Покупка \(asset.symbol)"), line, schedule: "buy:\(asset.symbol)")
            } catch {
                let next = find(symbol).map { Fmt.date(schedules[$0].next) } ?? "none, the schedule was stopped"
                // Bought, then the read after it failed: a «purchase failed» notice sent the user to buy it again by hand.
                let went = wentThrough(since: started, then: error)
                let text = went.map { "\($0) Next one: \(next)." } ?? "Scheduled \(asset.symbol) purchase failed: \(error.localizedDescription). Next one: \(next)."
                reply(text)
                notify(.moves, went == nil ? say("\(asset.symbol) purchase failed", "Покупка \(asset.symbol) не прошла") : say("\(asset.symbol) purchase", "Покупка \(asset.symbol)"),
                       text, schedule: "buy:\(asset.symbol)", failed: went == nil)
            }
            executing = false
        }
    }

    /// While a TWAP of ours runs the Mac doesn't idle-sleep (the display may): asleep, nothing would stop a TWAP whose stop
    /// fired. Ended once no TWAP is left to watch. A closed lid or a quit app still end the watch: the plan card says so.
    func keepAwakeForTwaps() {
        let running = exitRules.values.contains { $0.twapId != nil }
        if running, twapAwake == nil {
            twapAwake = ProcessInfo.processInfo.beginActivity(options: [.idleSystemSleepDisabled],
                                                              reason: "A TWAP on Hyperliquid is running: ihatecrypto watches its stop.")
        } else if !running, let token = twapAwake {
            ProcessInfo.processInfo.endActivity(token)
            twapAwake = nil
        }
    }

    /// A TWAP keeps buying for hours: only a trading key that's on can stop it with no one at the Mac (its stop fired,
    /// the loss grew). Without one it isn't started.
    var twapRefusal: String? {
        guard intent.twapMinutes != nil, !agentActive else { return nil }
        return ledgerActive
            ? say("A TWAP needs the Ledger's trading key on: without it, stopping the TWAP waits for the device's buttons. Say «approve trading key», or buy in one order.",
                  "Для TWAP нужен торговый ключ Ledger: без него остановка TWAP ждёт кнопок устройства. Скажи «approve trading key» или купи одним ордером.")
            : say("A TWAP needs the Hyperliquid trading key on (Settings › Wallet › Hyperliquid): locked or off, nothing can stop the TWAP while you're away. Turn it on, or buy in one order.",
                  "Для TWAP нужен включённый торговый ключ Hyperliquid (Settings › Wallet › Hyperliquid): когда он выключен или заблокирован, TWAP некому остановить без тебя. Включи его или купи одним ордером.")
    }

    /// Runs after each refresh while the app is open: breakeven after the partial take, trailing stops.
    func guardPositions() async {
        guard signerWallet != nil, !executing else { return }
        keepAwakeForTwaps()
        defer { keepAwakeForTwaps() }
        // On a Ledger, stops move by themselves only through the trading key it approved: without it every move would
        // wait for the device's buttons. The stop and take on the exchange protect the position meanwhile.
        if ledgerActive, !agentActive {
            if !exitRules.isEmpty, ledgerWaitNotified.insert("guardian").inserted {
                notify(.positions, say("Trailing waits for the trading key", "Трейлинг ждёт торговый ключ"),
                       say("On a Ledger, stops move by themselves once its trading key is approved — say “approve trading key”. The stop on the exchange protects the position meanwhile.",
                           "На Ledger стопы двигаются сами, когда одобрен торговый ключ — скажи «approve trading key». Пока позицию защищает стоп на бирже."))
            }
            return
        }
        // A TWAP entry, each pass, read afresh by the executor: a stop or take that fired leaves it buying, and a loss at the
        // stop past 1.5 × the plan's stops it too. It stays watched until Hyperliquid shows it ended or it's really
        // stopped: a pass that fails is tried again on the next.
        for (coin, rules) in exitRules where rules.twapId != nil {
            guard let wallet = signerWallet,
                  let watch = try? await executor(wallet).guardTwap(asset: coin, risk: rules.twapRisk) else { continue }
            switch watch {
            case .running: continue
            case .ended: exitRules[coin]?.twapId = nil
            case .stopped(let line):
                exitRules[coin]?.twapId = nil
                reply(line)
                notify(.positions, say("TWAP on \(coin) stopped", "TWAP по \(coin) остановлен"), line)
            }
        }
        // Rules live exactly as long as their position (or its waiting limit), however it was closed.
        // A TWAP not yet seen stopped keeps its rules.
        for coin in exitRules.keys where !positions.contains(where: { $0.coin == coin }) && !orders.contains(where: { $0.coin == coin && !$0.isTrigger })
            && exitRules[coin]?.twapId == nil {
            exitRules[coin] = nil
        }
        stopTries = stopTries.filter { exitRules[$0.key] != nil }   // a new position in the coin starts with no pause
        for coin in Array(exitRules.keys) {
            // Read afresh: an await for another coin earlier in this pass may have changed these rules («trail» meanwhile).
            guard var rules = exitRules[coin], let p = positions.first(where: { $0.coin == coin }), let mark = markets[coin]?.markPx else { continue }
            if rules.partialOrderPending, let wallet = signerWallet {
                let tries = partialTries[coin] ?? .init()
                // Partly filled, the rest of the limit waits with the stop (armed on the full fill): nothing to place yet.
                guard Date() >= tries.next, !HyperliquidAccount.Order.fillsInPart(orders.filter { $0.coin == coin }) else { continue }
                if tries.failures > 0, abs(p.size) < abs(rules.openedSize) {
                    // A try whose reply was lost had put the half on, and it has filled since: the breakeven step below sees it.
                    rules.partialOrderPending = false
                    partialTries[coin] = nil
                } else {
                    await placeHalf(coin, position: p, rules: rules, tries: tries, wallet: wallet)
                    continue
                }
            }
            // The tightest stop, as Executor.moveStop reads it: a new one whose check failed sits beside the old one.
            let stops = orders.filter { $0.coin == coin && $0.isTrigger && $0.reduceOnly && ($0.orderType ?? "").contains("Stop") }.compactMap(\.triggerPx)
            let stop = rules.long ? stops.max() : stops.min()
            let wasAtBreakeven = rules.movedToBreakeven
            let action = Guardian.step(&rules, size: p.size, stop: stop, mark: mark)
            // Only a real change: each write redraws and saves the rules to UserDefaults, and this runs every tick.
            if exitRules[coin] != rules { exitRules[coin] = rules }
            let breakevenTitle = say("Partial profit taken on \(coin)", "Частичная прибыль по \(coin)")
            let trailTitle = say("Trailing stop on \(coin)", "Трейлинг-стоп по \(coin)")
            let atBreakeven = say("Stop moved to breakeven.", "Стоп перенесён в безубыток.")
            // A failed series ends when the exchange leaves nothing to do: that try's stop went on before its check
            // failed, it was moved by hand, or (trailing) the price fell back. Its record then says what's there.
            if rules.movedToBreakeven, !wasAtBreakeven, (stopTries[coin]?[false]?.failures ?? 0) > 0 {
                stopTries[coin]?[false] = nil
                updateActivity(stopRecord(coin, breakevenTitle, again: true)) { $0.lines.append(atBreakeven); $0.failed = false }
                notify(.positions, breakevenTitle, atBreakeven, record: false)
            }
            if action == nil, let stop, (stopTries[coin]?[true]?.failures ?? 0) > 0 {
                stopTries[coin]?[true] = nil
                let line = "\(Fmt.time(Date())) · " + say("stop on the exchange: \(Fmt.price(stop))", "стоп на бирже: \(Fmt.price(stop))")
                updateActivity(stopRecord(coin, trailTitle, again: true)) { $0.lines.append(line); $0.failed = false }
            }
            guard let action, let evm = signerWallet?.evm else { continue }
            let trailing = action != .stopToBreakeven
            let tries = stopTries[coin]?[trailing] ?? .init()
            guard Date() >= tries.next else { continue }   // between tries: each may ask for Touch ID
            let command: PositionCommand = if case .stopTo(let price) = action { .stopTo(price) } else { .stopToBreakeven }
            let title = trailing ? trailTitle : breakevenTitle
            // Its own record before the move: `manage` writes into the current one, not into the user's last request.
            let record = stopRecord(coin, title, again: tries.failures > 0, inRow: trailing)
            stopTries[coin, default: [:]][trailing] = .init(failures: tries.failures, next: .distantFuture)   // under way: a second pass doesn't send it too
            let ok = await manage(command, coin: coin, quiet: tries.failures > 0)
            // Another wallet meanwhile has its own rules and tries.
            guard signerWallet?.evm == evm else { return }
            // Closed meanwhile (its stop, its take, by hand): moveStop answers «No open … position» without throwing.
            guard positions.contains(where: { $0.coin == coin }) else { stopTries[coin] = nil; continue }
            let after = Guardian.afterStopTry(tries, ok: ok)
            stopTries[coin, default: [:]][trailing] = after.tries
            if case .stopTo(let price) = command, ok {
                let line = "\(Fmt.time(Date())) · " + say("stop moved to \(Fmt.price(price))", "стоп передвинут на \(Fmt.price(price))")
                updateActivity(record) { $0.lines.append(line); $0.failed = false }
            } else if ok {
                exitRules[coin]?.movedToBreakeven = true   // only if the rules are still there
                updateActivity(record) { $0.lines.append(atBreakeven); $0.failed = false }
                notify(.positions, title, atBreakeven, record: false)
            } else if after.tell {
                let line = say("Stop not moved: the old one stays on the exchange. Trying again in a minute, then less often.",
                               "Стоп не перенесён — на бирже остаётся прежний. Повторю через минуту, потом реже.")
                updateActivity(record) { $0.lines.append(line); $0.failed = true }
                notify(.positions, title, line, record: false, failed: true)
            }
        }
    }

    /// The guardian's record for a stop move, made current; its id. `again`: a try after a failure, into the record that
    /// said it failed. `inRow`: trailing moves in a row are one record, a line each: a trend would otherwise fill the history.
    @discardableResult
    private func stopRecord(_ coin: String, _ title: String, again: Bool, inRow: Bool = false) -> UUID? {
        let schedule = "perp:\(coin)"
        if again, let last = activities.last(where: { $0.schedule == schedule && $0.title == title }) { currentActivity = last.id }
        else if inRow, let last = activities.last, last.schedule == schedule, last.title == title { currentActivity = last.id }
        else { beginActivity(.automatic, title: title, coin: coin, schedule: schedule) }
        return currentActivity
    }

    /// The half at 1R on a limit that has filled (guardPositions), sized from the real fill. The pending flag goes only once
    /// the exchange has it; a failure is tried again and said at the first time and at the end (Guardian.afterPartialTry).
    private func placeHalf(_ coin: String, position p: HyperliquidAccount.Position, rules: ExitRules, tries: Guardian.Tries,
                           wallet: TradeWallet) async {
        var rules = rules
        rules.openedSize = abs(p.size)
        rules.entry = p.entry
        rules.peak = p.entry
        if exitRules[coin] != rules { exitRules[coin] = rules }
        partialTries[coin] = .init(failures: tries.failures, next: .distantFuture)   // under way: a second pass doesn't send it too
        var placed: Bool?, failure: Error?
        do { placed = try await executor(wallet).addPartialTake(asset: coin) != .waiting } catch { failure = error }
        // Another wallet meanwhile has its own rules and tries.
        guard signerWallet?.evm == wallet.evm else { return }
        let now = Date()
        let after = Guardian.afterPartialTry(tries, placed: placed, refused: failure is GuardStop, now: now)
        partialTries[coin] = after.pending ? after.tries : nil
        if !after.pending { exitRules[coin]?.partialOrderPending = false }   // only if the rules are still there
        guard after.tell, let failure else { return }
        let why = failure.localizedDescription.trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        let minutes = Int((after.tries.next.timeIntervalSince(now) / 60).rounded())
        let text = after.pending
            ? say("Partial take-profit on \(coin) didn't go through (\(why)). Trying again in \(minutes) min; the stop and take-profit are on the exchange.",
                  "Частичный тейк по \(coin) не встал (\(why)). Попробую снова через \(minutes) мин; стоп и тейк стоят на бирже.")
            : say("Partial take-profit on \(coin) not placed (\(why)), and I won't try again. The stop and take-profit protect the position: place the half at 1R on Hyperliquid by hand if you still want it.",
                  "Частичный тейк по \(coin) не поставлен (\(why)), больше не пробую. Позицию защищают стоп и тейк; если половина на 1R ещё нужна, поставь её на Hyperliquid вручную.")
        reply(text)
        notify(.positions, say("Partial take-profit on \(coin)", "Частичный тейк по \(coin)"), text, schedule: "perp:\(coin)", failed: true)
    }

    /// A position closed outside ihatecrypto (its stop or take, or by hand elsewhere): say so in the chat and as a notification.
    func closedByExchange(_ coin: String) async {
        // A TWAP may still be buying after its stop or take closed the position: its rules stay for the guardian, which
        // stops it and closes what it bought after. Dropped here, it bought 0.035 ETH on with no stop (live run 2026-11-05).
        if exitRules[coin]?.twapId == nil { exitRules[coin] = nil }
        guard let close = try? await HyperliquidAccount.lastClose(hlAddress, coin: coin), Date().timeIntervalSince(close.time) < 600 else { return }
        // HyperCore fills do not say whether a trigger or a person closed it; only that it was not ihatecrypto.
        let text = say("\(coin) position closed outside ihatecrypto (stop, take-profit or by hand) at \(Fmt.price(close.price)): \(close.pnl >= 0 ? "profit" : "loss") \(Fmt.usd(abs(close.pnl))).",
                       "Позиция \(coin) закрылась вне ihatecrypto (стоп, тейк или вручную) по \(Fmt.price(close.price)): \(close.pnl >= 0 ? "прибыль" : "убыток") \(Fmt.usd(abs(close.pnl))).")
        reply(text, note: "exchange")
        notify(.positions, "\(coin)", text, schedule: "perp:\(coin)")
    }

    /// «stop to breakeven», «close half» — for the history.
    static func describe(_ command: PositionCommand) -> String {
        switch command {
        case .close(let share): share >= 1 ? "close" : "close \(Fmt.num(share * 100, maxFraction: 0))%"
        case .stopToBreakeven: "stop to breakeven"
        case .stopTo(let px): "stop to \(Fmt.price(px))"
        case .takeTo(let px): "take-profit to \(Fmt.price(px))"
        case .trail(let pct): "trail the stop \(Fmt.pct(pct))"
        case .cancelOrders: "cancel orders"
        }
    }

    /// A macOS notification (shown while the window is in the background); `kind` can be turned off in Settings.
    /// `record`: what ihatecrypto did on its own (guardian, schedules, arrivals) is an activity too; a move the user
    /// confirmed already has one.
    /// `schedule`: the Schedule canvas item it ran for («buy:SOL», «perp:BTC»).
    func notify(_ kind: Notice, _ title: String, _ body: String, record: Bool = true, schedule: String? = nil, failed: Bool = false) {
        if record {
            beginActivity(.automatic, title: title, schedule: schedule)
            updateActivity { $0.lines.append(body); if failed { $0.failed = true } }
        }
        // Asked the first time something actually happens, never at launch. Needs an app bundle:
        // the `swift run` build has none and would crash, so there the chat line is all.
        guard kind.isOn, Bundle.main.bundleIdentifier != nil else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = NoticeDelegate.shared
        center.requestAuthorization(options: [.alert, .sound]) { granted, error in
            Notice.log.notice("notify \(kind.rawValue, privacy: .public): granted=\(granted) error=\(String(describing: error), privacy: .public)")
            guard granted else { return }
            let content = UNMutableNotificationContent()
            content.title = title
            content.body = body
            content.sound = kind.sound(failed: failed)
            // One stack per position, schedule or kind in Notification Center.
            content.threadIdentifier = schedule ?? kind.rawValue
            center.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
        }
    }

    func closePosition() async {
        guard let wallet = signerWallet, let coin = position?.coin ?? pendingEntry?.coin else { return }
        executing = true
        defer { executing = false }
        do { _ = try await executor(wallet).close(asset: coin) } catch {
            reply("Couldn't close: \(error.localizedDescription)")
            noteFailure("Close \(IntentParser.pretty(coin))", error)
        }
        await refresh()
    }
}

/// What the wallet will sign and what happens to each balance, before anything is signed.
struct ConfirmSheet: View {
    @Environment(Workspace.self) private var ws
    @State private var acknowledged = false
    /// The steps, the balances and the checks under «How ›».
    @State private var details = false

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.ui(22, .heavy))
                    if ws.prepared != nil, ws.signerWallet != nil {
                        Text("Look it over, then sign with \(Presence.name).").font(.ui(13)).foregroundStyle(.secondary)
                    }
                    if let p = ws.prepared {
                        TimelineView(.periodic(from: .now, by: 1)) { context in
                            Text("numbers refreshed \(Int(context.date.timeIntervalSince(p.preparedAt))) s ago · after 45 s they are recalculated")
                                .font(.ui(10)).foregroundStyle(.secondary)
                        }
                    }
                }
                Spacer()
                if ws.prepared != nil, ws.preparing {
                    HStack(spacing: 6) { ProgressView().controlSize(.mini); Text(refreshing ? "refreshing numbers" : "checking signatures") }
                        .font(.ui(10, .semibold)).foregroundStyle(.secondary)
                        .padding(.horizontal, 10).padding(.vertical, 5)
                        .background(Capsule().fill(.fill.tertiary))
                } else if ws.prepared != nil {
                    Text(concern ? "check the warning" : "all checks passed").font(.ui(10, .semibold))
                        .foregroundStyle(concern ? Color.orange : Brand.profit)
                        .padding(.horizontal, 10).padding(.vertical, 5)
                        .background(Capsule().fill((concern ? Color.orange : Brand.profit).opacity(0.12)))
                }
            }
            if let p = ws.prepared {
                // Variant W2 «You give, they get» (owner, 2026-11-05): the margin that goes in and the position it buys,
                // as coins; the risk in plain lines; the checks in one line, their details a click away.
                giveGet(p)
                VStack(alignment: .leading, spacing: 9) {
                    line("shield.lefthalf.filled", "If the stop hits", "−\(Fmt.usd(p.sizing.lossAtStopUSD))", Brand.loss)
                    if let take = p.sizing.profitAtTakeUSD { line("arrow.up.forward", "If the take hits", "+\(Fmt.usd(take))", Brand.profit) }
                    line("dollarsign", "Fees, in and out at the stop", "\(Fmt.usd(p.sizing.feesUSD)) · ours \(Fmt.usd(p.sizing.builderFeesUSD))", nil)
                    if let b = p.bridge { line("arrow.left.arrow.right", "First from Solana", "\(Fmt.num(b.spend)) USDC · fees \(b.feesUSD)", nil) }
                }
                Button { withAnimation(.snappy) { details.toggle() } } label: {
                    HStack(spacing: 10) {
                        Image(systemName: concern ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                            .font(.system(size: 17)).foregroundStyle(concern ? Color.orange : Brand.profit)
                        Text(concern ? "The second check has a doubt: read it before you sign" : "We checked it matches what you asked")
                        Spacer(minLength: 6)
                        Text(details ? "Hide" : "How ›").foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 10).contentShape(Rectangle())
                    .background(RoundedRectangle(cornerRadius: 14).fill((concern ? Color.orange : Brand.profit).opacity(0.08)))
                }
                .buttonStyle(.plain).pointerStyle(.link)
                if details || concern {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("WHAT WILL BE SIGNED · \(p.steps.count) \(p.steps.count == 1 ? "ACTION" : "ACTIONS")")
                            .font(.ui(11, .semibold)).tracking(0.8).foregroundStyle(.secondary)
                        ForEach(Array(p.steps.enumerated()), id: \.offset) { n, step in
                            HStack(spacing: 12) {
                                icon(for: step, asset: p.market.name)
                                Text(step).fixedSize(horizontal: false, vertical: true)
                                Spacer(minLength: 0)
                                Text("\(n + 1)").font(.ui(10, .bold)).foregroundStyle(.secondary).monospacedDigit()
                            }
                            .padding(10)
                            .background(RoundedRectangle(cornerRadius: 14).fill(.fill.quaternary))
                        }
                    }
                    HStack(alignment: .top, spacing: 12) {
                        box("WHAT CHANGES") { ForEach(p.changes, id: \.self) { Text($0).fixedSize(horizontal: false, vertical: true) } }
                        box("CHECKS") {
                            Label("Recipients are yours only, programs are on the list, the simulation matches the plan.", systemImage: "checkmark.shield")
                            ForEach([ws.planReview, ws.txReview].compactMap { $0 }, id: \.text) { r in
                                Label(r.text, systemImage: r.concern ? "exclamationmark.triangle.fill" : "checkmark.seal")
                                    .foregroundStyle(r.concern ? AnyShapeStyle(.orange) : AnyShapeStyle(.primary))
                            }
                            if let note = ws.jevNote { Label(note, systemImage: "info.circle").foregroundStyle(.secondary) }
                            if ws.preparing {
                                HStack(spacing: 6) { ProgressView().controlSize(.mini); Text(refreshing ? "Numbers older than 45 s: recalculating them…" : "The second check is reading the signatures…") }
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                if concern {
                    Toggle("I understand the second check's warning and execute anyway", isOn: $acknowledged)
                        // Ticked while Jev still reads, it would pass a signature warning nobody has seen yet.
                        .disabled(ws.preparing)
                }
            } else if ws.preparing {
                HStack { ProgressView().controlSize(.small); Text("Plan check, quote, transaction and simulation…").foregroundStyle(.secondary) }
            }
            if let error = ws.executionError {
                Label(error, systemImage: "xmark.octagon.fill").foregroundStyle(Brand.loss)
                    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Brand.blockedTint))
                if failed {
                    Text("The plan may be out of date. Try again recalculates it from now — prices, balances and a new bridge quote — and prepares it again; nothing is signed.")
                        .font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            HStack(spacing: 12) {
                Text(ws.signerWallet == nil
                     ? "Signing is available in the app once the key is in secure storage. This is a preview only."
                     : ws.preparing && ws.prepared != nil
                     ? (refreshing ? "Sign opens once the new numbers are checked." : "Sign opens once the second check has read the signatures.")
                     : "A separate process signs; the model and websites never see the key.")
                    .font(.ui(10)).foregroundStyle(.secondary)
                Spacer()
                Button(ws.signerWallet == nil ? "Close" : "Cancel") { ws.cancelPreparation() }
                    .keyboardShortcut(.cancelAction)
                    .controlSize(.large)
                if failed {
                    Button { Task { await ws.rerunPlan() } } label: {
                        Label("Try again", systemImage: "arrow.clockwise").font(.ui(14, .bold)).padding(.horizontal, 6)
                    }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .tint(Brand.accent)
                    .controlSize(.large)
                } else if ws.signerWallet != nil {
                    Button { Task { await ws.confirmExecution() } } label: {
                        Label(ws.ledgerActive ? "Sign · Ledger" : "Sign · \(Presence.signShort)", systemImage: ws.ledgerActive ? "hand.point.up.left.fill" : Presence.signIcon).font(.ui(14, .bold))
                            .padding(.horizontal, 6)
                    }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .tint(Brand.accent)
                    .controlSize(.large)
                    .disabled(ws.prepared == nil || ws.preparing || (concern && !acknowledged) || ws.executing)
                }
            }
        }
        .font(.ui(13))
        .padding(26)
        .frame(width: 600)
        // A failed preparation is a short note, not a tall empty sheet.
        .frame(minHeight: ws.prepared == nil ? 0 : 460)
        // A stale plan is prepared again under the open sheet: a tick given to the old warning mustn't pass a new one.
        .onChange(of: ws.prepared?.preparedAt) { acknowledged = false }
    }

    /// The preparation stopped with an error and there is nothing to sign: offer to run the plan again.
    private var failed: Bool { ws.prepared == nil && ws.executionError != nil && !ws.preparing }

    private var title: String {
        guard let p = ws.prepared else { return ws.executionError == nil ? "Preparing the trade" : "Couldn’t prepare the trade" }
        return "Sign: \(p.intent.side == .long ? "long" : "short") \(IntentParser.pretty(p.market.name))"
    }

    /// What each step touches, before the words: the bridge, the exchange, the asset, the protection.
    @ViewBuilder private func icon(for step: String, asset: String) -> some View {
        if step.hasPrefix("Relay") { TokenLogo(name: "relay", size: 28) }
        else if step.hasPrefix("NEAR") || step.contains("1Click") { TokenLogo(name: "usdc", size: 28) }
        else if step.hasPrefix("Leverage") || step.hasPrefix("Transfer") { TokenLogo(name: "hype", size: 28) }
        else if step.hasPrefix("Long") || step.hasPrefix("Short") { AssetBadge(coin: asset, size: 28, dark: true) }
        else if step.hasPrefix("Stop") { Image(systemName: "shield.lefthalf.filled").font(.system(size: 16)).frame(width: 28, height: 28).foregroundStyle(Brand.profit) }
        else if step.hasPrefix("Trailing") { Image(systemName: "arrow.up.forward").frame(width: 28, height: 28).foregroundStyle(Brand.jevCursor) }
        else { Image(systemName: "checkmark.circle").frame(width: 28, height: 28).foregroundStyle(.secondary) }
    }

    /// You give the margin, from where it is; you get the position. Coins with their network, the mascot between.
    private func giveGet(_ p: Executor.Prepared) -> some View {
        let from: (logo: String, name: String) = p.bridge != nil || p.openVia1Click != nil ? ("sol", "Solana")
            : p.fundFrom.map { ($0.net.key, $0.net.name) } ?? ("hype", "Hyperliquid")
        let long = p.intent.side == .long
        func coin(_ face: some View, badge: some View) -> some View {
            face.frame(width: 58, height: 58).overlay(alignment: .bottomTrailing) { badge.offset(x: 5, y: 5) }
        }
        return HStack(alignment: .center, spacing: 10) {
            VStack(spacing: 7) {
                Text("YOU GIVE").font(.ui(11, .semibold)).tracking(0.8).foregroundStyle(.secondary)
                coin(TokenLogo(name: "usdc", size: 58), badge: TokenLogo(name: from.logo, size: 24, round: true))
                Text(Fmt.usd(p.sizing.margin)).font(.ui(26, .bold)).monospacedDigit()
                Text("USDC margin from \(from.name)").font(.ui(12)).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity).padding(.vertical, 16).padding(.horizontal, 12)
            .background(RoundedRectangle(cornerRadius: 20).fill(.fill.quaternary))
            VStack(spacing: 6) {
                BrandLogo(size: 46, mood: .waiting)
                Image(systemName: "arrow.right").font(.system(size: 16, weight: .semibold)).foregroundStyle(Brand.accentInk)
                Text("×\(p.leverage)").font(.ui(11, .semibold)).foregroundStyle(Brand.accentInk)
            }
            .frame(width: 58)
            VStack(spacing: 7) {
                Text("YOU GET").font(.ui(11, .semibold)).tracking(0.8).foregroundStyle(Brand.accentInk)
                coin(AssetBadge(coin: p.market.name, size: 58, dark: true),
                     badge: Text(long ? "LONG" : "SHORT").font(.ui(9, .bold)).foregroundStyle(.white)
                        .padding(.horizontal, 5).padding(.vertical, 2).background(Capsule().fill(long ? Brand.profit : Brand.loss)))
                Text(Fmt.usd(p.sizing.notional)).font(.ui(26, .bold)).monospacedDigit()
                Text("\(long ? "a long" : "a short") on \(IntentParser.pretty(p.market.name).components(separatedBy: " (").first ?? p.market.name) · \(Fmt.num(p.sizing.size, maxFraction: 6))")
                    .font(.ui(12)).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity).padding(.vertical, 16).padding(.horizontal, 12)
            .background(RoundedRectangle(cornerRadius: 20).fill(Brand.accent.opacity(0.10)))
            .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(Brand.accent.opacity(0.35)))
        }
    }

    /// One plain line: an icon, what it is, and its number.
    private func line(_ icon: String, _ what: String, _ value: String, _ tint: Color?) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon).font(.system(size: 14, weight: .semibold)).frame(width: 22).foregroundStyle(tint ?? .secondary)
            Text(what)
            Spacer(minLength: 6)
            Text(value).font(.ui(13, .semibold)).monospacedDigit().foregroundStyle(tint ?? .primary)
        }
    }

    private func box(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(title).font(.ui(11, .semibold)).tracking(0.8).foregroundStyle(.secondary)
            content()
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.cardBorder))
    }

    private var concern: Bool { ws.planReview?.concern == true || ws.txReview?.concern == true }
    /// A stale confirmation is prepared again under the old steps: the wait is new numbers first, the second check after.
    private var refreshing: Bool { ws.preparing && ws.prepared?.isFresh == false }

}
