import EVMChain
import Foundation
import HyperliquidSigning
import SignerAPI
import SolanaChain
import TxGuard
import WalletCore

/// The automation wallet signs with no one at the Mac: the signer itself counts the dollars that leave it, held to the
/// person's day and week (Settings), and asks in person (a prompt that closes after a minute) for anything past them or
/// anything that isn't dollars going somewhere other than the person's own wallets — never the app's word (the
/// 2026-11-08 review). Read from the same request the signer then checks and signs.
enum Automation {
    /// Dollars the signer counts by contract or mint, with their decimals.
    static let solanaDollars: [String: Int] = ["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": 6, "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": 6]
    /// Programs a task's Solana move may call without the person: fees, accounts, transfers, Jupiter's swaps and fills.
    static let solanaPrograms: Set<String> = ["ComputeBudget111111111111111111111111111111", "11111111111111111111111111111111",
                                              SPL.tokenProgram, SPL.token2022Program, "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
                                              SolanaGuard.jupiterSwap, SolanaGuard.jupiterZ]

    /// What `request` spends in dollars, already taken from the budget — after the person was asked about whatever the
    /// signer can't count or what goes past the limits. The caller gives it back if the move isn't signed.
    static func spend(_ request: SignerRequest) throws -> Decimal {
        if case .within(_, let inner) = request { return try spend(inner) }
        var dollars = Decimal(), others: [String] = []
        func add(_ asset: String, _ units: Decimal, decimals: Int?, to: String?, name: String) {
            if let to, own(to) { return }
            if let decimals { dollars += units / pow(10, decimals) } else { others.append(name) }
        }
        switch request {
        case .signSolana(let wire, let accounts, let recipients, let tokenOwners, let extraPrograms, let sponsor):
            let owner = try Signer.address("solana")
            let message = try SolanaMessage.parse(transaction: wire)
            let verdict = SolanaGuard.check(message, accounts: accounts,
                                            policy: SolanaPolicy(owner: owner, recipients: Set(recipients), trustLookupTables: false, sponsor: sponsor),
                                            tokenOwners: Signer.verifiedOwners(tokenOwners, mints: accounts))
            for o in verdict.outflows {
                add(o.asset, Decimal(o.units), decimals: solanaDollars[o.asset], to: o.to,
                    name: o.asset == "SOL" ? "\(Decimal(o.units) / 1_000_000_000) SOL" : "\(o.units) units of \(o.asset.prefix(6))…")
            }
            let called = Set(message.instructions.compactMap { $0.programIndex < accounts.count ? accounts[$0.programIndex] : nil })
            others += called.subtracting(solanaPrograms).sorted().map { "a call to \($0.prefix(6))…" }
            if !extraPrograms.isEmpty { others.append("a program not on ihatecrypto's list") }
        case .signEVMTransaction(let chainId, _, _, _, _, let to, let value, let data, _):
            evm(chainId, to: to, value: value, data: data, add: add)
        case .signEVMBatch(let chainId, _, _, _, _, let calls, _):
            for c in calls { evm(chainId, to: c.to, value: 0, data: c.data, add: add) }
        case .signSponsoredTransfer(_, _, _, let token, let to, let amount, _):
            // Read as the signing path reads it (SponsoredTransfer.refusal): a whole number of units, else the person.
            if let units = Decimal(string: amount), units > 0, units == units.whole {
                add(token, units, decimals: dollarDecimals(token), to: to, name: "\(amount) units of \(token.prefix(8))…")
            } else { others.append("an amount the signer can't read (\(amount.prefix(20)))") }
        case .signRelayMulticallPermit(let chainId, let token, let value, let validBefore, let relayer, let refundTo, let nftRecipient, let metadata, let calls):
            // Into Relay's depository for an order whose payee the signer checked. To one of the person's own wallets,
            // in dollars, for exactly this deposit: what is spent is what doesn't come back, the deposit less the order's
            // minimum. The order names its own amounts, so «to my own wallet» alone counted nothing while it could pay
            // back a cent (the 2026-11-10 review). Another payee, or a coin the signer can't count: the whole deposit.
            let m = RelayMulticallPermit(chainId: chainId, token: token, from: try Signer.address("evm"), value: value, validBefore: validBefore,
                                         relayer: relayer, refundTo: refundTo, nftRecipient: nftRecipient, metadata: metadata,
                                         calls: calls.map { .init(target: $0.target, allowFailure: $0.allowFailure, value: $0.value, callData: $0.data) })
            add(token, Decimal(value), decimals: dollarDecimals(token), to: nil, name: "\(value) units of \(token.prefix(8))…")
            if let id = m.depositID, let order = Signer.RelayOrders.order(id), own(order.recipient), dollarDecimals(token) != nil,
               order.inputCurrency == token.lowercased(), order.inputAmount == String(value), let back = order.minimumDollars {
                dollars = max(0, dollars - back)
            }
        case .signHyperliquidL1(let action, _, _):
            switch HLAction.opening(action) {
            case .dollars(let d): dollars += d
            case .person(let why): others.append(why)
            }
        case .signHyperliquidUser(let action, _, _):
            let to = HLAction.text(action, "destination") ?? ""
            let token = HLAction.text(action, "token").map { String($0.split(separator: ":").first ?? "") } ?? "USDC"
            let amount = Decimal(string: HLAction.text(action, "amount") ?? "", locale: Locale(identifier: "en_US_POSIX"))
            switch HLAction.type(action) {
            case "usdClassTransfer": break
            case "vaultTransfer": others.append("a Hyperliquid vault transfer")
            default:
                if let amount, amount >= 0 { add(token, amount, decimals: token == "USDC" ? 0 : nil, to: to, name: "\(amount) \(token)") }
                else { others.append("a Hyperliquid amount the signer can't read") }
            }
        case .signNearIntentDeposit(let amount, _, _, _):
            others.append("\(amount) yoctoNEAR")
        case .signNearTransfer(let amount, let receiver, _, _, _):
            add("NEAR", 0, decimals: nil, to: receiver, name: "\(amount) yoctoNEAR")
        case .signRelayNonceMapping, .relayOrder, .signReceiveAuthorization: break
        default: others.append("a request a task doesn't make")
        }
        if let uncounted = try Allowances.read(request).uncounted { others.append(uncounted) }
        // Taken from the day and week at once (two tasks at the same moment can't both fit in what's left); past them, or
        // for anything that isn't dollars, the person allows it and it's counted all the same.
        if let why = try others.isEmpty ? Limits.takeAutomation(dollars) : "the automation wallet spends without you only dollars, not " + others.joined(separator: ", ") {
            try Signer.confirmAfresh("a scheduled task's move: \(why)")
            try Limits.take(.automation, dollars)
        }
        return dollars
    }

    /// An EVM call's outflows: coins sent with it, tokens transferred or approved for someone else to pull, dollars burned
    /// or sent across.
    static func evm(_ chainId: UInt64, to: String, value: UInt64, data: Data,
                    add: (String, Decimal, Int?, String?, String) -> Void) {
        let call = EVMCall.decode(data)
        if value > 0, !call.returnsCoins(to: to, value: value, chainId: chainId) {
            add("native", Decimal(value), nil, to, "\(Decimal(value) / 1_000_000_000_000_000_000) \(EVMRPC.coin(chainId))")
        }
        let token = to.lowercased(), decimals = dollarDecimals(token)
        func units(_ word: Data) -> Decimal { word.reduce(Decimal()) { $0 * 256 + Decimal($1) } }
        switch call {
        case .transfer(let r, let a): add(token, units(a), decimals, r, "\(units(a)) units of \(token.prefix(8))…")
        case .approve(let s, let a), .increaseAllowance(let s, let a): add(token, units(a), decimals, s, "a right to \(units(a)) units of \(token.prefix(8))…")
        case .permit2Approve(let t, let s, let a, _): add(t.lowercased(), units(a), dollarDecimals(t), s, "a right to \(units(a)) units of \(t.prefix(8))…")
        case .cctpBurn(let r, _, let a), .oftSend(let r, _, let a, _, _): add(token, units(a), decimals, r, "\(units(a)) units sent across")
        // An NFT needs no right given first and has no dollars to count: to anyone but the person's own wallets it's theirs
        // to allow (it went uncounted and unasked: the 2026-11-13 review).
        case .nftTransfer(_, let r): add(token, 0, nil, r, "NFTs of \(token)")
        default: break
        }
    }

    /// A swap or a deposit spends what an approval gave its contract, and the budget counts the approval. So a call that
    /// spends must fit an approval this signer counted: the same spender, the same token, no more than it gave. One the
    /// signer never counted (from before the limits, or of another token) is the person's to allow (the 2026-11-13
    /// review). The last approval of each token to each spender is kept with the signer's settings, not used up: the
    /// token itself lets a spender take an approval only once, and a swap that failed must be free to go again.
    enum Allowances {
        private static let lock = NSLock()
        static let key = "automationAllowances"

        /// The request's calls read in order, starting from `kept`. `uncounted`: why a call's spending fits no counted
        /// approval, or nil. `after`: what to keep once the request is signed.
        static func read(_ request: SignerRequest, from kept: [String: String]) throws -> (uncounted: String?, after: [String: String]) {
            if case .within(_, let inner) = request { return try read(inner, from: kept) }
            var kept = kept, uncounted: String?
            func units(_ word: Data) -> Decimal { word.reduce(Decimal()) { $0 * 256 + Decimal($1) } }
            func given(_ k: String) -> Decimal { kept[k].flatMap { Decimal(string: $0) } ?? 0 }
            let owner = try Signer.address("evm").lowercased()
            func call(_ chainId: UInt64, _ to: String, _ data: Data) {
                let decoded = EVMCall.decode(data), prefix = "\(owner):\(chainId):"
                func entry(_ token: String, _ spender: String) -> String { prefix + token.lowercased() + ":" + spender.lowercased() }
                switch decoded {
                case .approve(let s, let a): kept[entry(to, s)] = "\(units(a))"
                case .increaseAllowance(let s, let a): kept[entry(to, s)] = "\(given(entry(to, s)) + units(a))"
                case .permit2Approve(let t, let s, let a, _): kept[entry(t, s)] = "\(units(a))"
                // A swap whose amounts the signer can't read (several in one multicall) could sell a token no counted
                // approval names: never covered. ihatecrypto's own swaps are one swap each.
                case .swap(_, nil): uncounted = "a swap whose amounts the signer can't read"
                default:
                    guard let pull = decoded.pulls(to: to, data: data, chainId: chainId) else { return }
                    let spender = ":" + to.lowercased()
                    // The token unread (a vault's asset, an unread call): any one approval to that spender must cover it.
                    let most = pull.token.map { given(entry($0, to)) }
                        ?? kept.filter { $0.key.hasPrefix(prefix) && $0.key.hasSuffix(spender) }.values.compactMap { Decimal(string: $0) }.max() ?? 0
                    if most <= 0 || pull.units.map({ $0 > most }) == true { uncounted = "a swap or a deposit on an approval the signer didn't count" }
                }
            }
            switch request {
            case .signEVMTransaction(let chainId, _, _, _, _, let to, _, let data, _): call(chainId, to, data)
            case .signEVMBatch(let chainId, _, _, _, _, let calls, _): for c in calls { call(chainId, c.to, c.data) }
            default: break
            }
            return (uncounted, kept)
        }

        static func read(_ request: SignerRequest) throws -> (uncounted: String?, after: [String: String]) {
            try read(request, from: lock.withLock { Signer.defaults.object(forKey: key) as? [String: String] ?? [:] })
        }

        /// After the request was signed: the approvals it gave are kept. Read and written in one step, so two tasks'
        /// approvals signed at the same moment both stay.
        static func signed(_ request: SignerRequest) {
            lock.withLock {
                let kept = Signer.defaults.object(forKey: key) as? [String: String] ?? [:]
                guard let after = try? read(request, from: kept).after, after != kept else { return }
                _ = Signer.defaults.set(after, forKey: key)
            }
        }
    }

    static func dollarDecimals(_ token: String) -> Int? {
        EVMRPC.dollars[token.lowercased()] == nil ? nil : Int(EVMRPC.dollarDecimals(token))
    }

    /// One of the person's own wallets on this Mac: money moving there isn't spent. Not a test wallet, which signs without
    /// a prompt itself: money could leave the limits through it.
    static func own(_ address: String) -> Bool {
        let key = RecipientStore.key(address)
        func keys(_ w: SignerWallet) -> [String] { [w.solana, w.evm, w.near].compactMap { $0 }.map(RecipientStore.key) }
        if Wallets.all.filter(\.isTest).flatMap(keys).contains(key) { return false }
        return OwnStore.all().contains(key) || Wallets.all.flatMap(keys).contains(key)
    }
}
