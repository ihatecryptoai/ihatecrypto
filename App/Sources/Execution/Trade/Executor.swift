import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
import RelayBridge
import SolanaChain
import Synchronization
import TxGuard
import WalletCore

/// Whoever holds the keys. The app's is the signing process over XPC; the terminal check signs with the
/// test wallet's key. Both run the guard before signing, so the executor cannot talk either into more.
public protocol TradeSigner: Sendable {
    /// `accounts`: the message's full account list; `recipients`: addresses the user approved besides their own.
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data
    /// Orders, cancels, leverage.
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature
    /// Transfers signed by the main key (sendAsset to Relay's depository); `recipients` as approved.
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature
    /// Relay's binding of a Hyperliquid deposit to an order.
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature
    /// An EIP-1559 transaction; the signer decodes the calldata itself (no unlimited approvals, no transfers to others).
    /// `recipients`: addresses the user approved for a token transfer (from the address book), besides their own.
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                 to: String, data: Data, recipients: [String]) async throws -> Data
    /// Whether `signEVMBatch` signs: the signing process and the terminal's test key do; a Ledger doesn't.
    var signsBatches: Bool { get }
    /// `calls` as one EIP-7702 transaction from and to the wallet (Batch7702); each call is held to signEVM's rules.
    func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                      calls: [Batch7702.Call], recipients: [String]) async throws -> Data
    /// A contract call carrying `value` wei of the network's own coin: the signer lets it go only where the coins come back
    /// to this wallet (EVMCall.returnsCoins: a wrap into the network's WETH, a v4 swap of native ETH), or to `recipients`.
    func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                        to: String, value: UInt64, data: Data, recipients: [String]) async throws -> Data
    /// The network's own coin (ETH, POL): `value` wei to `to`, which must be among `recipients`; no calldata.
    func signEVMTransfer(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                         to: String, value: UInt64, recipients: [String]) async throws -> Data
    /// Native NEAR into a 1Click deposit: the signer builds the one wrap.near transaction itself
    /// (`NearTransaction.intentDeposit`) and returns it signed, in borsh.
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data
    /// A native NEAR transfer of exactly `amount` yocto to `receiver`, which must be among `recipients` (saved addresses).
    func signNearTransfer(amount: UInt128, receiver: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data
    /// Deletes the wallet's NEAR account; NEAR sends all that is on it to `beneficiary` (one of `recipients`).
    func signNearDeleteAccount(beneficiary: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data
    /// This key's owner signature on a move out of a Safe multisig (Safe.refusal holds: a plain payment, money only to
    /// `recipients`, the signer's own address or the multisig).
    func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature
    /// A transaction someone else pays the fee for and signs first (`sponsor`: Jupiter's gasless swap); our wallet signs second.
    func signSolanaSponsored(_ wire: Data, accounts: [String], tokenOwners: [String: String], sponsor: String) async throws -> Data
    /// Dollars Relay takes by signature (Permit3009): its solver pays the network fee.
    func signReceiveAuthorization(_ permit: Permit3009) async throws -> EVMSignature
    /// The same on Relay's v3.2 proxy: the MulticallAuthorization's signature, then the EIP-3009 one.
    func signRelayMulticallPermit(_ m: RelayMulticallPermit) async throws -> [EVMSignature]
    /// A zero-fee dollar transfer BNB Chain's paymaster pays the gas for; `to` among `recipients`. The raw transaction.
    func signSponsoredTransfer(_ t: SponsoredTransfer, recipients: [String]) async throws -> Data
    /// A Relay order handed to the signer before the deposit that names it: the signer checks whom it pays.
    func registerRelayOrder(_ order: Data) async throws
}

extension TradeSigner {
    public var signsBatches: Bool { false }
    /// A signer that signs on its own screen (a Ledger) or in a test has nothing to check ahead.
    public func registerRelayOrder(_ order: Data) async throws {}
    public func signSolanaSponsored(_ wire: Data, accounts: [String], tokenOwners: [String: String], sponsor: String) async throws -> Data {
        throw ExecutionError("This signer doesn't sign a transaction someone else pays for: add a little SOL instead.")
    }
    public func signReceiveAuthorization(_ permit: Permit3009) async throws -> EVMSignature {
        throw ExecutionError("This signer doesn't pay by signature: add a little of the network's coin for gas instead.")
    }
    public func signRelayMulticallPermit(_ m: RelayMulticallPermit) async throws -> [EVMSignature] {
        throw ExecutionError("This signer doesn't pay by signature: add a little of the network's coin for gas instead.")
    }
    public func signSponsoredTransfer(_ t: SponsoredTransfer, recipients: [String]) async throws -> Data {
        throw ExecutionError("This signer doesn't send a zero-fee transfer: add a little BNB for gas instead.")
    }
    public func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature {
        throw ExecutionError("This signer doesn't sign multisig moves.")
    }
    public func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                             calls: [Batch7702.Call], recipients: [String]) async throws -> Data {
        throw ExecutionError("This signer doesn't sign batched calls.")
    }
    public func signEVMTransfer(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                                to: String, value: UInt64, recipients: [String]) async throws -> Data {
        throw ExecutionError("Sending a network's own coin is signed in the ihatecrypto app.")
    }
    public func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                               to: String, value: UInt64, data: Data, recipients: [String]) async throws -> Data {
        throw ExecutionError("This signer doesn't send the network's own coin into a contract: swap from its wrapped token (WETH) instead.")
    }
    public func signNearTransfer(amount: UInt128, receiver: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        throw ExecutionError("NEAR transfers are signed in the ihatecrypto app.")
    }
    public func signNearDeleteAccount(beneficiary: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        throw ExecutionError("Closing the NEAR account is signed in the ihatecrypto app.")
    }
}

public struct TradeWallet: Sendable {
    public let solana: String
    public let evm: String
    public init(solana: String, evm: String) {
        self.solana = solana
        self.evm = evm
    }
}

public struct ExecutionError: LocalizedError {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

/// The guard, the signer or a scam check stopped a move: something in it is not what it should be. Nothing is offered
/// around it (Workspace.explainFailure), unlike a move that only lacked money or a route.
public struct GuardStop: LocalizedError {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

/// A move that would lose more than ihatecrypto lets one lose by itself — 3% of it, 15¢ on a small one, $1 at most (5%
/// for gas): stopped before anything is signed, with the numbers, so the user can take it anyway («Send anyway»).
public struct CostlyMove: LocalizedError, Sendable {
    public let lost: Decimal
    public let paid: Decimal
    /// Who keeps it: «Relay», «Relay's swap».
    public let what: String
    /// Its own words, where the loss isn't a share the move keeps: Kamino's first-deposit rent comes on top, in SOL.
    var words: String?
    /// A swap's price impact (0…1) when that's what stopped it: «Send anyway» takes that impact, not a dollar loss.
    public var impact: Decimal?
    /// The part of what goes in that moves the price only about 1% («160,000 WIF»), for a card.
    public var part: (amount: Decimal, symbol: String)?
    public var errorDescription: String? {
        words ?? "\(what) keeps ≈ \(Fmt.usd(lost)) of \(Fmt.usd(paid)) (\(Fmt.num(lost / max(paid, Decimal(string: "0.01")!) * 100, maxFraction: 0))%): "
            + "more than a move may lose on its own. Nothing was signed."
    }
}

/// Runs an approved plan on mainnet: bridge if the trading balance is short, leverage, entry, stop and take,
/// then checks on the exchange that the position is protected. A position is never left without its stop:
/// if the stop cannot be placed, the position is closed at once.
public actor Executor {
    let wallet: TradeWallet
    let signer: any TradeSigner
    let journal: Journal
    let rpc: SolanaRPC
    let report: @Sendable (String) -> Void
    /// A Jupiter swap that went through: from, to, how much went in and how much arrived (the app keeps spot costs).
    let swapped: (@Sendable (SpotAsset, SpotAsset, Decimal, Decimal) -> Void)?
    /// A token bought (+tokens, for dollars) or sold (−tokens) on an EVM network, under «chain:contract» (Holding.id), so
    /// the P/L knows what a Relay buy cost: its money and its tokens are two transactions the chain can't pair.
    let traded: (@Sendable (String, Decimal, Decimal) -> Void)?
    /// The user confirmed this move in the window: it may ask them to approve ihatecrypto's fee (prepareFee). A scheduled
    /// task's run never asks; its orders carry the fee only once the account approved it.
    let askFee: Bool
    private var lastNonce: UInt64 = 0
    /// The fee this account lets ihatecrypto take (maxBuilderFee), read once per move; nil when it couldn't be read.
    private var allowed: UInt64?
    private var allowedRead = false
    /// Accounts whose referral was looked at in this run of the app: once each.
    static let referralLooked = Mutex<Set<String>>([])
    /// «chainId:address» → the nonce after that wallet's last confirmed transaction there, across moves (nextNonce).
    static let evmNonces = Mutex<[String: UInt64]>([:])

    /// Worst price accepted for a market entry or exit, from the mark.
    /// Smallest bridge worth its fee (Relay ≈ $0.02 flat).
    static let minBridgeUSDC: Decimal = 2

    /// What a plan brings to Hyperliquid. A little over its need: the entry may fill up to 1% worse than the mark the
    /// plan used. A tiny top-up would pay about as much in bridge fees as it moves (and the guard refuses that), so at
    /// least `minBridgeUSDC` when Solana has it; the rest stays free on Hyperliquid. The plan's card quotes the same
    /// number: it once showed 0.05 USDC while 2 were signed, and the second check flagged the difference.
    public static func bridgeArrive(need: Decimal, margin: Decimal, solanaUSDC: Decimal?) -> Decimal {
        var arrive = (need + margin * slippage).rounded(2, .up)
        if arrive < minBridgeUSDC, let onSolana = solanaUSDC {
            arrive = max(arrive, min(minBridgeUSDC, (onSolana - Decimal(string: "0.05")!).rounded(2, .down)))
        }
        return arrive
    }
    static let slippage = Decimal(string: "0.01")!

    /// Dollars the user agreed to lose on this run: «Send anyway» on a move the cap stopped (CostlyMove).
    @TaskLocal public static var lossAccepted: Decimal = 0

    /// What a move may lose: up to `cap` by itself, up to what the user accepted with «Send anyway»; past both it stops.
    static func checkLoss(_ lost: Decimal, paid: Decimal, cap: Decimal, what: String) throws {
        guard lost > cap, lost > lossAccepted else { return }
        throw CostlyMove(lost: lost, paid: paid, what: what)
    }

    /// How far a swap may move the price on its own. Past it (and past $1), it stops before anything is signed and asks:
    /// a part now, the whole anyway, or nothing (2026-10-24: someone lost $340k selling a memecoin in one go).
    static let impactCap = Decimal(string: "0.02")!
    /// The price impact the user took with «Send anyway» on a swap that impactCap stopped; the dollar caps stay as they are.
    @TaskLocal public static var impactAccepted: Decimal = 0

    /// Stops a swap whose price impact (0…1) is past `impactCap` and what was accepted: `paid` is what goes in, in USD at
    /// the market price (0 = nobody prices it: the impact alone decides), `amount` the same in the input's own units
    /// («all» as the number it is), for the part that moves the price ≈ 1%. `signed`: what already went (a bridge, gas)
    /// and where the money waits — then it's a plain refusal, since «Nothing was signed» would be untrue.
    static func checkImpact(_ impact: Decimal, paid: Decimal, amount: Decimal, symbol: String, venue: String, signed: String? = nil) throws {
        let lost = paid * impact
        guard impact > impactCap, impact > impactAccepted, paid == 0 || lost > 1 else { return }
        // ponytail: impact grows about in step with size; the part is quoted and checked again when it runs.
        let share = amount * Decimal(string: "0.01")! / impact, part = share.rounded(share < 10 ? 6 : 0, .down)
        let words = "At once, this moves \(venue)'s price by \(Fmt.num((impact * 100).rounded(1, .plain)))%"
            + (paid > 0 ? ": ≈ \(Fmt.usd(lost)) of \(Fmt.usd(paid)) lost to the thin market. " : ", and nobody prices \(symbol) to say it in dollars. ")
            + (part > 0 ? "\(Fmt.num(part, maxFraction: 6)) \(symbol) at a time moves it about 1%. " : "")
        if let signed { throw ExecutionError(words + signed) }
        throw CostlyMove(lost: lost, paid: paid, what: venue, words: words + "Nothing was signed.", impact: impact, part: part > 0 ? (part, symbol) : nil)
    }
    /// Market stop and take fill up to this far past the trigger, as Hyperliquid's own interface does.
    static let triggerSlippage = Decimal(string: "0.1")!

    public init(wallet: TradeWallet, signer: any TradeSigner, journal: Journal, rpc: SolanaRPC = SolanaRPC(), askFee: Bool = false,
                report: @escaping @Sendable (String) -> Void, swapped: (@Sendable (SpotAsset, SpotAsset, Decimal, Decimal) -> Void)? = nil,
                traded: (@Sendable (String, Decimal, Decimal) -> Void)? = nil) {
        self.wallet = wallet
        self.askFee = askFee
        self.signer = signer
        self.journal = journal
        self.rpc = rpc
        self.report = report
        self.swapped = swapped
        self.traded = traded
    }

    public struct Opened: Sendable {
        public let asset: String
        public let size: Decimal
        public let entry: Decimal
        public let stop: Decimal
        public let take: Decimal?
        /// A limit entry still waiting on the book; its stop and take switch on when it fills.
        public var pending = false
        /// First partial take (half at 1R), when the plan asked for one.
        public var partialTake: Decimal?
    }

    /// The bridge transaction exactly as it will be signed, with its simulated effect.
    public struct BridgeStep: Sendable {
        public let requestId: String
        public let unsigned: Data
        public let accounts: [String]
        public let tokenOwners: [String: String]
        public let lastValid: UInt64
        public let spend: Decimal
        public let arriving: Decimal
        public let feesUSD: String
        /// Where the USDC goes: Hyperliquid for trades, Base for Aave.
        public var chain: UInt64 = Relay.Chain.hyperliquid
        /// Part of the fee that is Hyperliquid's one-time charge for opening the account; 0 for an existing one.
        public var opening: Decimal = 0
        /// The journal's words for the order, when it isn't Solana's USDC (a cross-network swap).
        public var note: String?
    }

    /// Everything the user confirms: what will be signed, in order, and what it does to each balance.
    public struct Prepared: Sendable {
        public let intent: TradeIntent
        public let market: Market
        public let sizing: Sizing
        public let leverage: Int
        public let bridge: BridgeStep?
        /// USDC to move from the main Hyperliquid balance to a HIP-3 exchange's margin («xyz»).
        public var dexTransfer: (dex: String, usdc: Decimal)?
        /// Solana was short: this much USDC goes straight from an EVM network to Hyperliquid instead of `bridge`.
        public var fundFrom: (net: EVMNetwork, usdc: Decimal)?
        /// A new Hyperliquid account opened through NEAR Intents' 1Click instead of `bridge`, as the user chose: USDC units
        /// sent from Solana and what 1Click's quote lands.
        public var openVia1Click: (units: UInt64, arriving: Decimal)?
        /// «Solana USDC: 5.80 → 3.75 (−2.05)», one line per balance.
        public let changes: [String]
        /// One line per signature, in order.
        public let steps: [String]
        public let preparedAt: Date
        /// The first trade asks to approve ihatecrypto's fee: one signature more than the plan's steps.
        public var asksFee = false

        /// The bridge transaction's blockhash lives about a minute; after that the preview is redone.
        public var isFresh: Bool { Date().timeIntervalSince(preparedAt) < 45 }
    }

    // MARK: Prepare

    /// Fresh prices and balances, the bridge quote and transaction, and a simulation of it. Nothing is signed.
    /// The risk rules are applied here too, on fresh balances: the UI showing a green plan is not enough.
    /// `openVia1Click`: a new Hyperliquid account opens through NEAR Intents (2–4 min, no opening fee) instead of Relay.
    public func prepare(_ requested: TradeIntent, policy: RiskPolicy = RiskPolicy(), openVia1Click: Bool = false) async throws -> Prepared {
        let markets = try await Hyperliquid.markets()
        guard var market = markets[requested.asset], market.index >= 0 else { throw ExecutionError("No \(requested.asset) market on Hyperliquid.") }
        market.atrPct = try? await Hyperliquid.atrPct(coin: market.name)
        let hl = try await Hyperliquid.balance(user: wallet.evm)
        let solanaUSDC = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        var equity = hl.accountValue + solanaUSDC
        for dex in Hyperliquid.hip3Dexes { equity += (try? await Hyperliquid.dexBalance(user: wallet.evm, dex: dex).accountValue) ?? 0 }
        // Idle USDC on the EVM networks is capital too, and the trade can be paid from it when Solana is short.
        var held: [String: Decimal] = [:]
        for net in EVMNetwork.searched {
            held[net.key] = (try? await net.usdcBalance(wallet.evm)).map { Decimal($0) / 1_000_000 }
        }
        equity += held.values.reduce(0, +)
        let intent = Plan.resolve(requested, market: market, equity: equity, policy: policy).intent
        let evaluation = Plan.evaluate(intent, market: market)
        guard evaluation.missing.isEmpty, evaluation.problem == nil, let sizing = evaluation.sizing, intent.stopPct != nil else {
            throw ExecutionError(evaluation.problem ?? "The plan isn't ready: missing \(evaluation.missing.joined(separator: ", ")).")
        }
        // One plan per asset: a second entry would merge into the open position and its stop.
        let positions = try await HyperliquidAccount.positions(wallet.evm).filter { $0.size != 0 }
        let orders = try await HyperliquidAccount.openOrders(wallet.evm)
        if positions.contains(where: { $0.coin == market.name }) || orders.contains(where: { $0.coin == market.name }) {
            throw ExecutionError("There's already a position or order on \(market.name). Manage it in chat («stop to breakeven», «close half», «cancel the limit») or pick another asset.")
        }
        let openRisk = HyperliquidAccount.riskAtStops(positions: positions, orders: orders)
        let blocked = RiskRules.check(intent, sizing, market: market, equity: equity, policy: policy, openRisk: openRisk).filter { $0.level == .block }
        guard blocked.isEmpty else { throw ExecutionError(blocked.map(\.text).joined(separator: " ")) }
        let leverage = NSDecimalNumber(decimal: intent.leverage ?? 1).intValue

        // HIP-3 markets («xyz:CL») keep margin on their own exchange: what is free there counts, and the rest
        // moves over from the main balance before the order.
        var dexFree: Decimal = 0
        if let dex = market.dex { dexFree = try await Hyperliquid.dexBalance(user: wallet.evm, dex: dex).withdrawable }
        var dexTransfer: (dex: String, usdc: Decimal)?
        if let dex = market.dex {
            let needed = sizing.margin * (1 + Self.slippage) + sizing.feesUSD
            if needed > dexFree { dexTransfer = (dex, (needed - dexFree).rounded(2, .up)) }
        }
        var amount: Bridge.Amount?
        if let usdc = intent.amountUSDC {
            amount = .send(usdc)
        } else if let need = Plan.bridgeNeed(intent, evaluation, hlWithdrawable: hl.withdrawable + dexFree, mark: market.markPx) {
            var arrive = Self.bridgeArrive(need: need, margin: sizing.margin, solanaUSDC: nil)
            if arrive < Self.minBridgeUSDC {
                let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
                arrive = Self.bridgeArrive(need: need, margin: sizing.margin, solanaUSDC: onSolana)
            }
            amount = .arrive(arrive)
        }
        var prepared: (step: BridgeStep, changes: [String])?
        var fund: (net: EVMNetwork, usdc: Decimal, quote: BridgeQuote, gas: Bool, have: Decimal)?
        var opening: (units: UInt64, arriving: Decimal)?
        if let amount {
            // Solana first. Short there, the money goes straight from an EVM network that holds it — the same choice,
            // from the same dry quote, as the plan's card.
            let newAccount = (try? await Hyperliquid.isNewAccount(wallet.evm)) ?? false
            let viaSolana = switch amount {
            case .send(let v): v
            case .arrive: try await Bridge.dryQuote(amount, newAccount: newAccount).amountIn
            }
            if viaSolana > solanaUSDC, let net = EVMNetwork.funding(viaSolana, held: { held[$0] }) {
                let q = try await Bridge.dryQuote(.send(viaSolana), newAccount: newAccount, from: (net.relayChain, net.usdc, net.usdcDecimals))
                // No gas coin there: a little comes from Solana first (ensureEVM), which needs half a dollar there.
                let gas = net.feeToken == nil ? try await net.rpc.balance(wallet.evm) >= net.gasFloor : true
                guard gas || solanaUSDC >= Decimal(string: "0.7")! else {
                    throw ExecutionError("\(net.name) has the USDC but no \(net.gasCoin) for gas, and Solana has too little USDC to bring it. Send a little \(net.gasCoin) to \(net.name) or USDC to Solana.")
                }
                fund = (net, viaSolana, q, gas, held[net.key] ?? 0)
            } else if newAccount, openVia1Click {
                opening = try await oneClickOpening(amount, solanaUSDC: solanaUSDC)
            } else {
                prepared = try await prepareBridge(amount)
            }
        }
        let bridge = prepared?.step
        var changes = prepared?.changes ?? []
        var steps: [String] = []
        if let b = bridge {
            steps.append("Relay bridge: \(Fmt.num(b.spend)) USDC from Solana → \(Fmt.num(b.arriving)) USDC on Hyperliquid, fee \(Decimal(string: b.feesUSD).map { Fmt.usd($0) } ?? "$" + b.feesUSD)"
                         + (b.opening > 0 ? ", of it \(Fmt.usd(b.opening)) once for opening your Hyperliquid account." : "."))
        }
        if let f = fund {
            if !f.gas { steps.append("\(f.net.gasCoin) for gas on \(f.net.name) first: 0.5 USDC from Solana over Relay, the \(f.net.gasCoin) arrives with it.") }
            steps.append("Relay bridge straight from \(f.net.name): approve exactly \(Fmt.num(f.usdc)) USDC, then Relay's deposit → ≈ \(Fmt.num(f.quote.amountOut)) USDC on Hyperliquid, "
                         + "fee ≈ \(Fmt.usd(f.quote.amountIn - f.quote.amountOut)). Solana has \(Fmt.num(solanaUSDC)) USDC, less than the trade needs.")
            changes.append("\(f.net.name) USDC: \(Fmt.num(f.have)) → \(Fmt.num(f.have - f.usdc)) (−\(Fmt.num(f.usdc))).")
        }
        if let o = opening {
            let send = Decimal(o.units) / 1_000_000
            steps.append("NEAR Intents opens your Hyperliquid account: \(Fmt.num(send)) USDC from Solana → ≈ \(Fmt.num(o.arriving)) USDC on Hyperliquid, "
                         + "fee \(Fmt.usd(send - o.arriving)), no opening fee. Usually 2–4 min; the trade waits for it and re-checks the price.")
            changes.append("Solana USDC: \(Fmt.num(solanaUSDC)) → \(Fmt.num(solanaUSDC - send)) (−\(Fmt.num(send))).")
        }
        let arriving = bridge?.arriving ?? fund?.quote.amountOut ?? opening?.arriving ?? 0
        let side = intent.side == .long ? "Long" : "Short"
        if let t = dexTransfer {
            steps.append("Transfer \(Fmt.num(t.usdc)) USDC from the main Hyperliquid balance to the \(t.dex) exchange — its markets have their own margin.")
            changes.append("Hyperliquid \(t.dex), free: \(Fmt.usd(dexFree)) → \(Fmt.usd(dexFree + t.usdc)) before entry.")
        }
        // ihatecrypto's fee is asked for once, here, by the signer; it stays off a card where Hyperliquid has it approved.
        let fee = askFee ? await allowedFee() : nil
        let asksFee = fee.map { $0 < HLBuilder.fee } == true
        if asksFee {
            steps.append("Approve ihatecrypto's fee once: \(HLBuilder.rate) of each Hyperliquid trade, on top of the exchange's own. Settings › Wallet › Hyperliquid can take it back.")
        }
        steps.append("Leverage \(leverage)×, isolated margin.")
        if let limit = intent.entryPx {
            steps.append("\(side) \(Fmt.num(sizing.size, maxFraction: market.szDecimals)) \(market.name) as a limit at \(Fmt.price(HLAction.price(limit, szDecimals: market.szDecimals))) (now \(Fmt.price(market.markPx))), waiting on the exchange, with stop −\(Fmt.pct(intent.stopPct ?? 0))\(intent.takePct.map { " and take-profit +\(Fmt.pct($0))" } ?? ""), active once it fills.")
        } else {
            steps.append("\(side) \(Fmt.num(sizing.size, maxFraction: market.szDecimals)) \(market.name) at market, no worse than \(Fmt.price(HLAction.price(market.markPx * (intent.side == .long ? 1 + Self.slippage : 1 - Self.slippage), szDecimals: market.szDecimals))).")
            steps.append("Stop \(Fmt.pct(intent.stopPct ?? 0))\(intent.takePct.map { " and take-profit \(Fmt.pct($0))" } ?? "") from the entry price (\(intent.side == .long ? "stop below, take above" : "stop above, take below")), on the exchange.")
        }
        if intent.partialTake == true, intent.entryPx == nil { steps.append("Partial take-profit: half the position at 1R, as a separate order on the exchange; then the stop moves to breakeven.") }
        if let trail = intent.trailingPct { steps.append("Trailing \(Fmt.pct(trail)): ihatecrypto moves the stop while the app is open (each move is a signature).") }
        changes.append("Hyperliquid, free: \(Fmt.usd(hl.withdrawable)) → ≈ \(Fmt.usd(hl.withdrawable + arriving - sizing.margin - sizing.feesUSD / 2)) (margin \(Fmt.usd(sizing.margin)) goes into the position).")
        changes.append("Position: \(side.lowercased()) \(Fmt.num(sizing.size, maxFraction: market.szDecimals)) \(market.name), at the stop ≈ −\(Fmt.usd(sizing.lossAtStopUSD))\(sizing.profitAtTakeUSD.map { ", at the take ≈ +\(Fmt.usd($0))" } ?? "").")
        var p = Prepared(intent: intent, market: market, sizing: sizing, leverage: leverage, bridge: bridge,
                         changes: changes, steps: steps, preparedAt: Date())
        p.dexTransfer = dexTransfer
        p.fundFrom = fund.map { ($0.net, $0.usdc) }
        p.openVia1Click = opening
        p.asksFee = asksFee
        return p
    }

    /// The USDC from Solana that opens a new Hyperliquid account through 1Click, and what its quote lands.
    func oneClickOpening(_ amount: Bridge.Amount, solanaUSDC: Decimal) async throws -> (units: UInt64, arriving: Decimal) {
        let send: Decimal = switch amount {
        case .send(let v): v
        case .arrive: try await Bridge.oneClickQuote(amount).amountIn
        }
        guard send <= solanaUSDC else {
            throw ExecutionError("Solana has \(Fmt.num(solanaUSDC)) USDC; opening through NEAR Intents needs \(Fmt.num(send)). Nothing was signed.")
        }
        let units = NSDecimalNumber(decimal: (send * 1_000_000).rounded(0, .up)).uint64Value
        guard let q = await oneClickQuote(from: nil, to: "hyperliquid", units: units), let out = Decimal(string: q.amountOutFormatted), out > 0 else {
            throw ExecutionError("NEAR Intents didn't quote Solana → Hyperliquid just now. Nothing was signed; Relay can open it instead.")
        }
        return (units, out)
    }

    // MARK: Open

    /// Signs and sends what `prepare` showed. Prices may move inside the approved limits; outside them it stops.
    public func execute(_ p: Prepared) async throws -> Opened {
        guard p.isFresh else { throw ExecutionError("This confirmation is stale: the numbers need a refresh.") }
        var market = try await Hyperliquid.markets()[p.market.name] ?? p.market
        guard abs(market.markPx - p.market.markPx) / p.market.markPx <= Self.slippage else {
            throw ExecutionError("\(market.name) moved more than 1% since you confirmed. Nothing was signed.")
        }
        let long = p.intent.side == .long
        if let b = p.bridge { try await sendBridge(b) }
        if let o = p.openVia1Click {
            let got = try await oneClickMove(from: nil, to: "hyperliquid", units: o.units, compared: o.arriving, label: "USDC that opens Hyperliquid")
            report("\(Fmt.num(got)) USDC are on Hyperliquid: the account is open.")
            // Minutes have passed: the entry goes in only at the price confirmed.
            market = try await Hyperliquid.markets()[p.market.name] ?? market
            guard abs(market.markPx - p.market.markPx) / p.market.markPx <= Self.slippage else {
                throw ExecutionError("\(market.name) moved more than 1% while the USDC travelled. No trade; the money is on your Hyperliquid balance.")
            }
        }
        if let f = p.fundFrom {
            try await ensureEVM(f.net, usdc: 0)
            report(try await bridge(f.net, to: nil, hyperliquid: true, usdc: f.usdc))
        }
        if let t = p.dexTransfer { try await moveMargin(from: "", to: t.dex, usdc: t.usdc) }
        try await prepareFee()

        let lev = try await post(Self.leverageAction(asset: p.market.index, leverage: p.leverage), label: "leverage")
        guard case .ok = lev else { throw ExecutionError("Hyperliquid rejected leverage \(p.leverage)×: \(lev).") }
        report("Leverage \(p.leverage)×, isolated margin.")

        if let limit = p.intent.entryPx { return try await placeLimit(p, market: market, limit: limit) }

        let entryPx = HLAction.price(market.markPx * (long ? 1 + Self.slippage : 1 - Self.slippage), szDecimals: market.szDecimals)
        let entry: HLReply
        do {
            entry = try await post(try HLAction.order([HLOrder(asset: market.index, isBuy: long, price: entryPx, size: p.sizing.size,
                                                               reduceOnly: false, kind: .limit(tif: "Ioc"))]), label: "entry")
        } catch {
            // The reply was lost, not necessarily the order: an entry that filled is a position, and it gets its stop as usual.
            guard let positions = try? await HyperliquidAccount.positions(wallet.evm) else {
                throw ExecutionError("The entry's reply didn't come (\(error.localizedDescription)) and the positions can't be read now. Check \(market.name) on Hyperliquid: if it opened, it has no stop yet.")
            }
            guard let open = positions.first(where: { $0.coin == market.name && $0.size != 0 }) else {
                throw ExecutionError("The entry's reply didn't come (\(error.localizedDescription)); no \(market.name) position is open, the money is on your Hyperliquid balance.")
            }
            report("The entry's reply didn't come, but the \(market.name) position is open: placing its stop.")
            entry = .filled(size: abs(open.size), avgPx: open.entry)
        }
        guard case .filled(let size, let avgPx) = entry else {
            throw ExecutionError("The entry didn't fill (\(entry)). No position; the money is on your Hyperliquid balance.")
        }
        report("\(long ? "Long" : "Short") \(Fmt.num(size, maxFraction: market.szDecimals)) \(market.name) at \(Fmt.price(avgPx)).")

        // Stop and take from the actual fill, so the loss at the stop stays the approved risk.
        let (stop, take) = Self.protectionPrices(entry: avgPx, stopPct: p.intent.stopPct ?? 0, takePct: p.intent.takePct, long: long,
                                                 szDecimals: market.szDecimals)
        do {
            let placed = try await post(try Self.protectionAction(asset: market.index, long: long, size: size, stop: stop, take: take,
                                                                  szDecimals: market.szDecimals), label: "stop and take-profit")
            if case .error(let e) = placed { throw ExecutionError(e) }
            try await verifyProtection(coin: market.name, stop: stop, take: take)
        } catch {
            report("The stop didn't go through: \(error.localizedDescription). Closing the position so it isn't left unprotected.")
            let closed: String
            do { closed = try await close(asset: market.name) } catch let failed {
                let left = await restoreStop(market, stop: stop)
                throw ExecutionError("The stop didn't go through and the emergency close failed too (\(failed.localizedDescription)). \(left)")
            }
            throw ExecutionError("Position closed: the exchange didn't confirm the stop. \(closed)")
        }
        report("On the exchange: stop \(Fmt.price(stop))\(take.map { " and take-profit \(Fmt.price($0))" } ?? ""). The position is protected.")
        var opened = Opened(asset: market.name, size: size, entry: avgPx, stop: stop, take: take)
        if p.intent.partialTake == true { opened.partialTake = try? await placePartialTake(market: market, long: long, size: size, entry: avgPx, stop: stop) }
        return opened
    }

    /// After an emergency close that failed: close() cancelled every order first, so what is still open gets its stop back.
    private func restoreStop(_ market: Market, stop: Decimal) async -> String {
        guard let positions = try? await HyperliquidAccount.positions(wallet.evm) else {
            return "The positions can't be read now: check \(market.name) on Hyperliquid, it may be open without a stop."
        }
        guard let p = positions.first(where: { $0.coin == market.name && $0.size != 0 }) else { return "No \(market.name) position is open." }
        do {
            let placed = try await post(try Self.protectionAction(asset: market.index, long: p.size > 0, size: abs(p.size), stop: stop, take: nil,
                                                                  szDecimals: market.szDecimals), label: "stop back on")
            if case .error(let e) = placed { throw ExecutionError(e) }
            try await verifyProtection(coin: market.name, stop: stop, take: nil)
            return "The \(market.name) position is still open, with its stop at \(Fmt.price(stop)) back on the exchange."
        } catch {
            return "The \(market.name) position is still open WITHOUT a stop (\(error.localizedDescription)): set one or close it on Hyperliquid now."
        }
    }

    /// For a limit entry that has filled: half at 1R from the actual entry and stop on the exchange.
    public func addPartialTake(asset: String) async throws -> Decimal? {
        let market = try await market(asset)
        guard let p = try await HyperliquidAccount.positions(wallet.evm).first(where: { $0.coin == asset && $0.size != 0 }),
              let stop = try await HyperliquidAccount.openOrders(wallet.evm).first(where: { $0.coin == asset && $0.isTrigger && ($0.orderType ?? "").contains("Stop") })?.triggerPx
        else { return nil }
        return try await placePartialTake(market: market, long: p.size > 0, size: abs(p.size), entry: p.entry, stop: stop)
    }

    /// Half the position at 1R as a separate reduce-only take. The position stays protected if this fails.
    private func placePartialTake(market: Market, long: Bool, size: Decimal, entry: Decimal, stop: Decimal) async throws -> Decimal? {
        let half = HLAction.size(size / 2, szDecimals: market.szDecimals)
        guard half * entry >= 10 else {
            report("Partial take-profit skipped: half the position is below the $10 minimum order.")
            return nil
        }
        let oneR = HLAction.price(entry + (entry - stop), szDecimals: market.szDecimals)
        let worst = HLAction.price(oneR * (long ? 1 - Self.triggerSlippage : 1 + Self.triggerSlippage), szDecimals: market.szDecimals)
        let reply = try await post(try HLAction.order([HLOrder(asset: market.index, isBuy: !long, price: worst, size: half, reduceOnly: true,
                                                               kind: .trigger(price: oneR, isMarket: true, tpsl: "tp"))]), label: "partial take-profit")
        guard !reply.isError else {
            report("Partial take-profit didn't go through (\(reply)); the stop and full take-profit are still in place.")
            return nil
        }
        report("Partial take-profit: \(Fmt.num(half, maxFraction: market.szDecimals)) \(market.name) at 1R → \(Fmt.price(oneR)). After it fills, the stop moves to breakeven.")
        return oneR
    }

    /// A resting limit entry with its stop and take in one normalTpsl group: the exchange arms them when the
    /// entry fills, so there is no moment with a position and no stop.
    private func placeLimit(_ p: Prepared, market: Market, limit: Decimal) async throws -> Opened {
        let long = p.intent.side == .long
        let px = HLAction.price(limit, szDecimals: market.szDecimals)
        let (stop, take) = Self.protectionPrices(entry: px, stopPct: p.intent.stopPct ?? 0, takePct: p.intent.takePct, long: long,
                                                 szDecimals: market.szDecimals)
        let entry = HLOrder(asset: market.index, isBuy: long, price: px, size: p.sizing.size, reduceOnly: false, kind: .limit(tif: "Gtc"))
        let action = try HLAction.order([entry] + Self.protectionOrders(asset: market.index, long: long, size: p.sizing.size, stop: stop, take: take,
                                                                         szDecimals: market.szDecimals), grouping: "normalTpsl")
        let reply = try await post(action, label: "limit with stop and take-profit")
        switch reply {
        case .resting, .ok:
            let orders = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == market.name }
            guard orders.contains(where: { !$0.isTrigger && $0.limitPx == px }) else { throw ExecutionError("The limit isn't among open orders: \(reply).") }
            report("Limit placed: \(long ? "long" : "short") \(Fmt.num(p.sizing.size, maxFraction: market.szDecimals)) \(market.name) at \(Fmt.price(px)). Stop \(Fmt.price(stop))\(take.map { " and take-profit \(Fmt.price($0))" } ?? ""): active once it fills.")
            return Opened(asset: market.name, size: p.sizing.size, entry: px, stop: stop, take: take, pending: true)
        case .filled(let size, let avgPx):
            // The price was already there: the group armed its stop and take at once.
            try await verifyProtection(coin: market.name, stop: stop, take: take)
            report("The limit filled at once: \(Fmt.num(size, maxFraction: market.szDecimals)) \(market.name) at \(Fmt.price(avgPx)); stop and take-profit are on the exchange.")
            return Opened(asset: market.name, size: size, entry: avgPx, stop: stop, take: take)
        case .error(let e):
            throw ExecutionError("Limit rejected: \(e). No position.")
        }
    }

    // MARK: Close

    /// Cancels the asset's stop and take and closes the position at market. Returns a line for the user.
    public func close(asset: String) async throws -> String {
        let markets = try await Hyperliquid.markets()
        guard let market = markets[asset], market.index >= 0 else { throw ExecutionError("No \(asset) market.") }
        let triggers = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == asset }
        if !triggers.isEmpty {
            _ = try await post(HLAction.cancel(triggers.map { (market.index, $0.oid) }), label: "cancel orders")
        }
        guard let position = try await HyperliquidAccount.positions(wallet.evm).first(where: { $0.coin == asset }), position.size != 0 else {
            let line = triggers.isEmpty ? "No open \(asset) position." : "\(asset) orders cancelled (\(triggers.count)); there was no position."
            report(line)
            return line
        }
        let buy = position.size < 0
        let px = HLAction.price(market.markPx * (buy ? 1 + 2 * Self.slippage : 1 - 2 * Self.slippage), szDecimals: market.szDecimals)
        let result = try await post(try HLAction.order([HLOrder(asset: market.index, isBuy: buy, price: px, size: abs(position.size),
                                                                reduceOnly: true, kind: .limit(tif: "Ioc"))]), label: "close")
        guard case .filled(let size, let avgPx) = result else { throw ExecutionError("The close didn't fill: \(result).") }
        let line = "\(asset) position closed: \(Fmt.num(size, maxFraction: market.szDecimals)) at \(Fmt.price(avgPx)); PnL before the close \(Fmt.usd(position.unrealizedPnl))."
        report(line)
        return line
    }

    /// sendAsset to our own address between perp balances — "" is the main one, "xyz" a HIP-3 exchange — then wait
    /// for the credit.
    public func moveMargin(from source: String, to dex: String, usdc: Decimal) async throws {
        func free() async throws -> Decimal {
            dex.isEmpty ? try await Hyperliquid.balance(user: wallet.evm).withdrawable : try await Hyperliquid.dexBalance(user: wallet.evm, dex: dex).withdrawable
        }
        let name = { (d: String) in d.isEmpty ? "the main balance" : d }
        let before = try await free()
        let action = HLSigner.sendAssetAction(destination: wallet.evm, sourceDex: source, destinationDex: dex,
                                              token: "USDC:0x6d1e7cde53ba9467b783cb7c530ce054", amount: try HLAction.number(usdc),
                                              nonce: nonce(), mainnet: true)
        guard case .map(let pairs) = action, case .uint(let n)? = pairs.first(where: { $0.0 == "nonce" })?.1 else { throw ExecutionError("No nonce.") }
        let signature = try await signer.signHyperliquidUser(action, recipients: [])
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: .signed, note: "sendAsset \(name(source)) → \(name(dex)) \(usdc)"))
        let reply = HLReply.parse(try await HLExchange.send(action: action, nonce: n, signature: signature, mainnet: true))
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: reply.isError ? .failed : .confirmed, note: "\(reply)"))
        guard !reply.isError else { throw ExecutionError("Transfer to \(name(dex)) rejected: \(reply).") }
        for _ in 0..<20 {
            if try await free() >= before + usdc * Decimal(string: "0.99")! {
                report("Margin on \(name(dex)): +\(Fmt.num(usdc)) USDC from \(name(source)).")
                return
            }
            try await Task.sleep(for: .milliseconds(500))
        }
        throw ExecutionError("Transfer to \(name(dex)) wasn't credited within 10 s.")
    }

    // MARK: Manage

    /// Carries out a command about an open position. Returns a line for the user.
    /// `policy` and `equity`: a stop moved wider than the per-trade rule is refused (nil equity: not checked).
    public func manage(_ command: PositionCommand, asset: String, policy: RiskPolicy = RiskPolicy(), equity: Decimal? = nil) async throws -> String {
        switch command {
        case .cancelOrders: return try await cancelEntries(asset: asset)
        case .close(1): return try await close(asset: asset)
        case .close(let fraction): return try await closePart(asset: asset, fraction: fraction)
        case .stopToBreakeven: return try await moveStop(asset: asset, to: nil, policy: policy, equity: equity)
        case .stopTo(let price): return try await moveStop(asset: asset, to: price, policy: policy, equity: equity)
        case .takeTo(let price): return try await moveTake(asset: asset, to: price)
        case .trail: throw ExecutionError("A trailing stop is kept by the ihatecrypto app, not placed on the exchange.")
        }
    }

    /// «cancel the limit»: only the entries waiting on the book go. A position, its stop and its take stay as they are.
    private func cancelEntries(asset: String) async throws -> String {
        let market = try await market(asset)
        let entries = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == asset && !$0.isTrigger && !$0.reduceOnly }
        guard !entries.isEmpty else {
            let line = "No \(asset) limit is waiting on the book; nothing was cancelled."
            report(line)
            return line
        }
        let reply = try await post(HLAction.cancel(entries.map { (market.index, $0.oid) }), label: "cancel limit")
        guard !reply.isError else { throw ExecutionError("Hyperliquid didn't cancel the \(asset) limit: \(reply).") }
        let open = try? await HyperliquidAccount.positions(wallet.evm).contains { $0.coin == asset && $0.size != 0 }
        let line = "\(asset) limit cancelled (\(entries.count))." + (open == true ? " The open position keeps its stop and take-profit." : "")
        report(line)
        return line
    }

    /// New take first, then the old one is cancelled.
    private func moveTake(asset: String, to target: Decimal) async throws -> String {
        let market = try await market(asset)
        guard let position = try await HyperliquidAccount.positions(wallet.evm).first(where: { $0.coin == asset && $0.size != 0 }) else {
            return "No open \(asset) position."
        }
        let long = position.size > 0
        let take = HLAction.price(target, szDecimals: market.szDecimals)
        guard long ? take > market.markPx : take < market.markPx else {
            throw ExecutionError("Take-profit \(Fmt.price(take)) is \(long ? "below" : "above") the current price \(Fmt.price(market.markPx)): it would trigger at once. To exit, type «close».")
        }
        let old = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == asset && $0.isTrigger && ($0.orderType ?? "").contains("Take") }
        func worst(_ p: Decimal) -> Decimal { HLAction.price(p * (long ? 1 - Self.triggerSlippage : 1 + Self.triggerSlippage), szDecimals: market.szDecimals) }
        let order = HLOrder(asset: market.index, isBuy: !long, price: worst(take), size: abs(position.size), reduceOnly: true,
                            kind: .trigger(price: take, isMarket: true, tpsl: "tp"))
        let placed = try await post(try HLAction.order([order], grouping: "positionTpsl"), label: "new take-profit")
        if case .error(let e) = placed { throw ExecutionError("New take-profit rejected: \(e). The old one stays.") }
        let stale = old.filter { $0.triggerPx != take }
        if !stale.isEmpty { _ = try await post(HLAction.cancel(stale.map { (market.index, $0.oid) }), label: "cancel old take-profit") }
        let line = "Take-profit moved to \(Fmt.price(take))."
        report(line)
        return line
    }

    private func closePart(asset: String, fraction: Decimal) async throws -> String {
        let market = try await market(asset)
        guard let position = try await HyperliquidAccount.positions(wallet.evm).first(where: { $0.coin == asset && $0.size != 0 }) else {
            return "No open \(asset) position."
        }
        let size = HLAction.size(abs(position.size) * fraction, szDecimals: market.szDecimals)
        guard size > 0, size < abs(position.size) else { return try await close(asset: asset) }
        let buy = position.size < 0
        let px = HLAction.price(market.markPx * (buy ? 1 + 2 * Self.slippage : 1 - 2 * Self.slippage), szDecimals: market.szDecimals)
        let result = try await post(try HLAction.order([HLOrder(asset: market.index, isBuy: buy, price: px, size: size, reduceOnly: true,
                                                                kind: .limit(tif: "Ioc"))]), label: "partial close")
        guard case .filled(let filled, let avgPx) = result else { throw ExecutionError("The partial close didn't fill: \(result).") }
        let left = abs(position.size) - filled
        let line = "Closed \(Fmt.num(filled, maxFraction: market.szDecimals)) \(asset) at \(Fmt.price(avgPx)); \(Fmt.num(left, maxFraction: market.szDecimals)) left, stop and take-profit in place."
        report(line)
        return line
    }

    /// New stop first, then the old one is cancelled: the position is never without a stop.
    /// `to: nil` = breakeven: the entry plus both taker fees, so a stop-out costs nothing.
    private func moveStop(asset: String, to target: Decimal?, policy: RiskPolicy, equity: Decimal?) async throws -> String {
        let market = try await market(asset)
        guard let position = try await HyperliquidAccount.positions(wallet.evm).first(where: { $0.coin == asset && $0.size != 0 }) else {
            return "No open \(asset) position."
        }
        let long = position.size > 0
        let fees = position.entry * 2 * RiskCalc.tradeFee
        let stop = HLAction.price(target ?? (long ? position.entry + fees : position.entry - fees), szDecimals: market.szDecimals)
        guard long ? stop < market.markPx : stop > market.markPx else {
            throw ExecutionError("Stop \(Fmt.price(stop)) is \(long ? "above" : "below") the current price \(Fmt.price(market.markPx)): it would trigger at once. To exit, type «close».")
        }
        let old = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == asset && $0.isTrigger && ($0.orderType ?? "").contains("Stop") }
        let current = long ? old.compactMap(\.triggerPx).max() : old.compactMap(\.triggerPx).min()
        if let loss = Self.stopBreaksRule(long: long, entry: position.entry, size: abs(position.size), stop: stop, current: current, equity: equity, policy: policy),
           let equity {
            throw ExecutionError("A stop at \(Fmt.price(stop)) means −\(Fmt.usd(loss)), \(Fmt.pct((loss / equity * 100).rounded(1, .plain))) of capital. The rule: at most \(Fmt.pct(policy.maxRiskPct)). Stop not moved.")
        }
        let orders = Self.protectionOrders(asset: market.index, long: long, size: abs(position.size), stop: stop, take: nil, szDecimals: market.szDecimals)
        let placed = try await post(try HLAction.order(orders, grouping: "positionTpsl"), label: "new stop")
        if case .error(let e) = placed { throw ExecutionError("New stop rejected: \(e). The old one stays.") }
        try await verifyProtection(coin: asset, stop: stop, take: nil)
        let stale = old.filter { $0.triggerPx != stop }
        if !stale.isEmpty { _ = try await post(HLAction.cancel(stale.map { (market.index, $0.oid) }), label: "cancel old stop") }
        let line = target == nil
            ? "Stop at breakeven: \(Fmt.price(stop)) (entry \(Fmt.price(position.entry)) plus fees). The trade can no longer end in a loss, barring slippage."
            : "Stop moved to \(Fmt.price(stop))."
        report(line)
        return line
    }

    /// The loss at a new stop when it is wider than the per-trade rule allows; nil when it's within the rule, when it
    /// tightens the current stop (or there is none), or when the capital is unknown — the same rule as «stop N%».
    static func stopBreaksRule(long: Bool, entry: Decimal, size: Decimal, stop: Decimal, current: Decimal?, equity: Decimal?,
                               policy: RiskPolicy) -> Decimal? {
        if let current, long ? stop >= current : stop <= current { return nil }
        guard current != nil, let equity, equity > 0 else { return nil }
        let loss = max(0, long ? entry - stop : stop - entry) * size + size * (entry + stop) * RiskCalc.tradeFee
        return loss / equity * 100 > policy.maxRiskPct ? loss : nil
    }

    private func market(_ asset: String) async throws -> Market {
        guard let m = try await Hyperliquid.markets()[asset], m.index >= 0 else { throw ExecutionError("No \(asset) market.") }
        return m
    }

    // MARK: Hyperliquid

    func nonce() -> UInt64 {
        lastNonce = max(UInt64(Date().timeIntervalSince1970 * 1000), lastNonce + 1)
        return lastNonce
    }

    /// Signs, journals before sending, sends, journals the reply. An order carries ihatecrypto's fee once the account has
    /// approved it; a refusal over the fee (the approval taken back meanwhile) never stops an order: it goes again without.
    func post(_ action: MsgPack, label: String) async throws -> HLReply {
        if HLAction.type(action) == "order", (await allowedFee() ?? 0) >= HLBuilder.fee {
            let reply = try await send(HLAction.withBuilder(action), label: label)
            guard case .error(let e) = reply, e.lowercased().contains("builder") else { return reply }
            allowed = 0
        }
        return try await send(action, label: label)
    }

    private func send(_ action: MsgPack, label: String) async throws -> HLReply {
        let nonce = nonce()
        let signature = try await signer.signHyperliquid(action, nonce: nonce)
        try journal.append(.init(chain: "hyperliquid", id: "\(nonce)", kind: .signed, note: label))
        let data = try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true)
        let reply = HLReply.parse(data)
        try journal.append(.init(chain: "hyperliquid", id: "\(nonce)", kind: reply.isError ? .failed : .confirmed, note: "\(reply)"))
        return reply
    }

    // MARK: ihatecrypto's fee and referral code (HLBuilder)

    /// What this account lets ihatecrypto take, in tenths of a basis point; nil when Hyperliquid didn't answer.
    public static func allowedFee(_ user: String) async -> UInt64? {
        (try? await hlInfo(["type": "maxBuilderFee", "user": user.lowercased(), "builder": HLBuilder.address]) as? NSNumber)?.uint64Value
    }

    func allowedFee() async -> UInt64? {
        if !allowedRead { (allowed, allowedRead) = (await Self.allowedFee(wallet.evm), true) }
        return allowed
    }

    /// Before the first order of a move the user confirmed: ihatecrypto's fee approved once (the signer shows it; turned
    /// down, the move stops before any order), and the referral code bound when the account has none. A fee Hyperliquid
    /// won't take, or a signer that can't sign it, leaves the trade without the fee.
    func prepareFee() async throws {
        if askFee, let fee = await allowedFee(), fee < HLBuilder.fee {
            do { try await setFeeApproval(true) } catch let stop as GuardStop { throw stop } catch {}
        }
        await bindReferral()
    }

    /// Approves ihatecrypto's fee (`HLBuilder.rate`) for this account, or takes the approval back (`on` false).
    public func setFeeApproval(_ on: Bool) async throws {
        let n = nonce()
        let action = HLBuilder.approveAction(maxFeeRate: on ? HLBuilder.rate : HLBuilder.revoke, nonce: n, mainnet: true)
        let note = on ? "approve ihatecrypto's fee \(HLBuilder.rate)" : "take back ihatecrypto's fee"
        let signature = try await signer.signHyperliquidUser(action, recipients: [])
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: .signed, note: note))
        let reply = HLReply.parse(try await HLExchange.send(action: action, nonce: n, signature: signature, mainnet: true))
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: reply.isError ? .failed : .confirmed, note: "\(note): \(reply)"))
        guard !reply.isError else { throw ExecutionError("Hyperliquid didn't take it: \(reply).") }
        (allowed, allowedRead) = (on ? HLBuilder.fee : 0, true)
        if on { report("ihatecrypto's fee approved: \(HLBuilder.rate) of each Hyperliquid trade, on top of the exchange's own.") }
    }

    /// Binds ihatecrypto's referral code to an account that has none (4% off its Hyperliquid fees), signed like an order:
    /// by the trading key, without a prompt. Once per account per run of the app; nothing in it stops a trade.
    func bindReferral() async {
        let user = wallet.evm.lowercased()
        guard Self.referralLooked.withLock({ $0.insert(user).inserted }),
              let state = try? await Self.hlInfo(["type": "referral", "user": user]) as? [String: Any], state["referredBy"] is NSNull,
              let reply = try? await post(HLBuilder.referrerAction, label: "referral code \(HLBuilder.referralCode)"), !reply.isError else { return }
        report("Referral code \(HLBuilder.referralCode) on: 4% off your Hyperliquid fees.")
    }

    private func verifyProtection(coin: String, stop: Decimal, take: Decimal?) async throws {
        // The exchange lists new trigger orders within a moment; ask a few times.
        for attempt in 0..<5 {
            let triggers = try await HyperliquidAccount.openOrders(wallet.evm).filter { $0.coin == coin && $0.isTrigger }
            let prices = Set(triggers.compactMap(\.triggerPx))
            if prices.contains(stop), take.map(prices.contains) ?? true { return }
            if attempt < 4 { try await Task.sleep(for: .milliseconds(600)) }
        }
        throw ExecutionError("the stop isn't among open orders")
    }

    /// Sends a signed Solana transaction already journaled as signed, and waits for its outcome. A node that refuses it at
    /// preflight (its simulation failed: the route moved since the check) hasn't sent it — unless an earlier node in the
    /// retry chain had — so it's waited out to its last valid block like any other: one that never lands is «expired», a
    /// move that failed with nothing sent, never one of unknown outcome that the chat won't let the user retry.
    func broadcastSolana(_ signed: Data, signature: String, lastValid: UInt64) async throws -> SolanaRPC.Outcome {
        do { _ = try await rpc.send(signed) } catch let e as WalletError where e.message.contains("simulation failed") {
            report("Solana refused the transaction at its check before sending (the route moved): making sure it didn't land.")
        }
        return try await rpc.waitForConfirmation(signature, lastValidBlockHeight: lastValid)
    }

    static func leverageAction(asset: Int, leverage: Int) -> MsgPack {
        .map([("type", .string("updateLeverage")), ("asset", .uint(UInt64(asset))), ("isCross", .bool(false)), ("leverage", .uint(UInt64(leverage)))])
    }

    static func protectionPrices(entry: Decimal, stopPct: Decimal, takePct: Decimal?, long: Bool, szDecimals: Int) -> (stop: Decimal, take: Decimal?) {
        let dir: Decimal = long ? 1 : -1
        let stop = HLAction.price(entry * (1 - dir * stopPct / 100), szDecimals: szDecimals)
        let take = takePct.map { HLAction.price(entry * (1 + dir * $0 / 100), szDecimals: szDecimals) }
        return (stop, take)
    }

    /// Stop and take as one positionTpsl group: reduce-only market triggers on the opposite side.
    static func protectionAction(asset: Int, long: Bool, size: Decimal, stop: Decimal, take: Decimal?, szDecimals: Int) throws -> MsgPack {
        try HLAction.order(protectionOrders(asset: asset, long: long, size: size, stop: stop, take: take, szDecimals: szDecimals), grouping: "positionTpsl")
    }

    static func protectionOrders(asset: Int, long: Bool, size: Decimal, stop: Decimal, take: Decimal?, szDecimals: Int) -> [HLOrder] {
        func worst(_ trigger: Decimal) -> Decimal {
            HLAction.price(trigger * (long ? 1 - triggerSlippage : 1 + triggerSlippage), szDecimals: szDecimals)
        }
        var orders = [HLOrder(asset: asset, isBuy: !long, price: worst(stop), size: size, reduceOnly: true,
                              kind: .trigger(price: stop, isMarket: true, tpsl: "sl"))]
        if let take {
            orders.append(HLOrder(asset: asset, isBuy: !long, price: worst(take), size: size, reduceOnly: true,
                                  kind: .trigger(price: take, isMarket: true, tpsl: "tp")))
        }
        return orders
    }

    // MARK: Bridge (Relay, Solana USDC → Hyperliquid perp or USDC on an EVM chain)

    /// The bridge transaction and the balance lines its simulation gives. `decimals` are the destination's;
    /// `topupGasUSD` adds that much native coin for gas on an EVM destination.
    /// `swap`: the destination is another token (Relay swaps on the way), so the cost is judged by USD in against USD out.
    /// Relay's quote for a move that's about to run, its order handed to the signer: a deposit naming that order signs only
    /// once the signer has checked whom it pays (Relay.Quote.order, SignerRequest.relayOrder).
    func relayQuote(user: String, recipient: String, originChain: UInt64, destinationChain: UInt64, originCurrency: String,
                    destinationCurrency: String, amount: UInt64, exactOutput: Bool = false, topupGasUSD: Decimal? = nil, units: String? = nil,
                    usePermit: Bool = false, useDepositAddress: Bool = false, refundTo: String? = nil) async throws -> Relay.Quote {
        let quote = try await Relay.quote(user: user, recipient: recipient, originChain: originChain, destinationChain: destinationChain,
                                          originCurrency: originCurrency, destinationCurrency: destinationCurrency, amount: amount,
                                          exactOutput: exactOutput, topupGasUSD: topupGasUSD, units: units, usePermit: usePermit,
                                          useDepositAddress: useDepositAddress, refundTo: refundTo)
        if let order = quote.order { try await signer.registerRelayOrder(order) }
        return quote
    }

    func prepareBridge(_ amount: Bridge.Amount, to chain: UInt64 = Relay.Chain.hyperliquid, currency: String = Relay.Currency.hyperliquidUSDC,
                       decimals: Int = 8, topupGasUSD: Decimal? = nil, swap: Bool = false) async throws -> (step: BridgeStep, changes: [String]) {
        let usdcMint = Relay.Currency.solanaUSDC
        let (units, exactOutput): (UInt64, Bool) = switch amount {
        case .arrive(let v): (NSDecimalNumber(decimal: (v * Decimal(sign: .plus, exponent: decimals, significand: 1)).rounded(0, .up)).uint64Value, true)
        case .send(let v): (NSDecimalNumber(decimal: (v * 1_000_000).rounded(0, .down)).uint64Value, false)
        }
        let quote = try await relayQuote(user: wallet.solana, recipient: wallet.evm, originChain: Relay.Chain.solana,
                                          destinationChain: chain, originCurrency: usdcMint,
                                          destinationCurrency: currency, amount: units, exactOutput: exactOutput, topupGasUSD: topupGasUSD)
        let binding = BindingGuard.check(["sender": quote.sender, "recipient": quote.recipient],
                                         expected: ["sender": wallet.solana, "recipient": wallet.evm], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        guard let spendUnits = UInt64(quote.amountIn), spendUnits > 0 else { throw ExecutionError("Relay didn't give the amount to send.") }
        let spend = Decimal(spendUnits) / 1_000_000
        let arriving = Decimal(string: quote.amountOutFormatted) ?? 0
        // Hyperliquid charges once for opening an account and Relay passes it on as its gas. It's allowed on top of the
        // cap only while Hyperliquid says the account doesn't exist, and only up to $1.50 (measured 2026-09-28: $1.20).
        var opening: Decimal = 0
        if chain == Relay.Chain.hyperliquid, (try? await Hyperliquid.isNewAccount(wallet.evm)) == true {
            opening = min(Decimal(string: quote.relayerGasUSD) ?? 0, Decimal(string: "1.5")!)
        }
        // Bridge costs are capped: over what arrives (and the gas asked for) at most 3%, never more than $1; small bridges
        // may cost up to $0.15, Relay's floor for delivering gas (measured 2026-09-26: ≈ $0.10 on a $2 bridge to Base).
        let worth = swap ? Decimal(string: quote.amountOutUSD) ?? 0 : arriving
        try Self.checkLoss(spend - worth - (topupGasUSD ?? 0) - opening, paid: spend,
                           cap: min(max(worth * Decimal(string: "0.03")!, Decimal(string: "0.15")!), 1), what: swap ? "Relay's swap" : "Relay")
        let have = try await rpc.tokenBalance(owner: wallet.solana, mint: usdcMint)
        guard have >= spendUnits else { throw ExecutionError("\(Fmt.num(Decimal(have) / 1_000_000)) USDC on Solana; the bridge needs \(Fmt.num(spend)).") }

        let (blockhash, lastValid) = try await rpc.latestBlockhash()
        let unsigned = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage(quote.solanaInstructions, payer: wallet.solana, blockhash: blockhash))
        let message = try SolanaMessage.parse(transaction: unsigned)
        let myUSDC = try SPL.associatedAccount(owner: wallet.solana, mint: usdcMint)
        let owners = [myUSDC: wallet.solana]
        // Checked here for a readable reason and for the preview; the signer checks again on its own.
        var verdict = SolanaGuard.check(message, accounts: message.staticKeys, policy: SolanaPolicy(owner: wallet.solana), tokenOwners: owners)
        let solBefore = try await rpc.balance(wallet.solana)
        let simulation = try await rpc.simulate(unsigned, watch: [myUSDC, wallet.solana], sigVerify: false)
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let error = simulation.error { throw ExecutionError("The simulation failed, so nothing was signed: \(error)") }
        let usdcAfter = simulation.after[myUSDC]?.token ?? 0
        let solAfter = simulation.after[wallet.solana]?.lamports ?? 0
        verdict.merge(SolanaGuard.balances([
            .init(label: "USDC", before: Int64(have), after: Int64(usdcAfter), atLeast: -Int64(spendUnits)),
            .init(label: "lamports", before: Int64(solBefore), after: Int64(solAfter), atLeast: -5_000_000),
        ]))
        guard verdict.ok else { throw GuardStop("The guard stopped the bridge: " + verdict.problems.joined(separator: " ")) }
        func usdc(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        func sol(_ l: UInt64) -> String { Fmt.num(Decimal(l) / 1_000_000_000, maxFraction: 6) }
        var changes = [
            "Solana USDC: \(usdc(have)) → \(usdc(usdcAfter)) (−\(usdc(have - min(have, usdcAfter)))), simulated.",
            "Solana SOL: \(sol(solBefore)) → \(sol(solAfter)) (network fee), simulated.",
        ]
        if let gas = Decimal(string: quote.gasTopup), gas > 0 {
            let coin = EVMNetwork.all.first { $0.relayChain == chain }?.gasCoin ?? "ETH"
            changes.append("Gas for the destination: +\(Fmt.num(gas, maxFraction: 6)) \(coin) arrives with the USDC.")
        }
        return (BridgeStep(requestId: quote.requestId, unsigned: unsigned, accounts: message.staticKeys, tokenOwners: owners,
                           lastValid: lastValid, spend: spend, arriving: arriving, feesUSD: quote.feesUSD, chain: chain, opening: opening), changes)
    }

    /// Returns the Solana transaction's signature.
    @discardableResult
    func sendBridge(_ b: BridgeStep) async throws -> String {
        let signed = try await signer.signSolana(b.unsigned, accounts: b.accounts, recipients: [], tokenOwners: b.tokenOwners)
        guard signed.count == b.unsigned.count, signed.subdata(in: 65..<signed.count) == b.unsigned.subdata(in: 65..<b.unsigned.count) else {
            throw GuardStop("What was signed isn't what was checked.")
        }
        let signature = Base58.encode(signed.subdata(in: 1..<65))
        let toHyperliquid = b.chain == Relay.Chain.hyperliquid
        let hlBefore = toHyperliquid ? try await Hyperliquid.balance(user: wallet.evm).withdrawable : 0
        try journal.append(.init(chain: "relay", id: b.requestId, kind: .signed, note: "\(b.note ?? "Solana USDC→\(toHyperliquid ? "Hyperliquid" : "chain \(b.chain)")"), tx \(signature)"))
        try journal.append(.init(chain: "solana", id: signature, kind: .signed, note: "Relay deposit \(b.requestId); valid until block \(b.lastValid)"))
        let started = Date()
        switch try await broadcastSolana(signed, signature: signature, lastValid: b.lastValid) {
        case .confirmed: try journal.append(.init(chain: "solana", id: signature, kind: .confirmed))
        // Relay's order ends with its deposit: left «signed», the app would take the move for one still on its way.
        case .failed(let e):
            try journal.append(.init(chain: "solana", id: signature, kind: .failed, note: e))
            try journal.append(.init(chain: "relay", id: b.requestId, kind: .failed, note: "its Solana deposit failed"))
            throw ExecutionError("The bridge transaction failed: \(e). The money stayed on Solana.")
        case .expired:
            try journal.append(.init(chain: "solana", id: signature, kind: .expired))
            try journal.append(.init(chain: "relay", id: b.requestId, kind: .expired))
            throw ExecutionError("The bridge transaction didn't make it into a block. The money stayed on Solana; you can retry.")
        }
        while Date().timeIntervalSince(started) < 180 {
            let status = try await Relay.status(requestId: b.requestId).status
            if status == "success" {
                try journal.append(.init(chain: "relay", id: b.requestId, kind: .settled, note: status))
                break
            }
            if ["refund", "failure"].contains(status) {
                try journal.append(.init(chain: "relay", id: b.requestId, kind: .failed, note: status))
                throw ExecutionError("Relay returned \(status). Check your Solana balance.")
            }
            try await Task.sleep(for: .seconds(1))
        }
        // An EVM destination: the caller waits for the tokens on its own chain.
        guard toHyperliquid else { return signature }
        // Wait for the credit itself: the trade needs it on the balance, not just in Relay's status.
        let target = hlBefore + b.arriving * Decimal(string: "0.99")!
        while Date().timeIntervalSince(started) < 240 {
            if try await Hyperliquid.balance(user: wallet.evm).withdrawable >= target {
                report("Bridge done in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(b.arriving)) USDC on Hyperliquid. https://solscan.io/tx/\(signature)")
                return signature
            }
            try await Task.sleep(for: .seconds(1))
        }
        throw ExecutionError("Relay didn't credit the USDC within 4 minutes. Order \(b.requestId) is in the journal; the trade wasn't opened.")
    }
}

// MARK: - Replies and account state

/// What /exchange answered for one action (the first order status for order actions).
public enum HLReply: Equatable, Sendable, CustomStringConvertible {
    case ok
    case filled(size: Decimal, avgPx: Decimal)
    case resting
    case error(String)

    public var isError: Bool { if case .error = self { true } else { false } }

    public var description: String {
        switch self {
        case .ok: "ok"
        case .filled(let s, let p): "filled \(s) at \(p)"
        case .resting: "resting on the book"
        case .error(let e): e
        }
    }

    public static func parse(_ data: Data) -> HLReply {
        guard let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return .error(String(decoding: data.prefix(200), as: UTF8.self))
        }
        guard j["status"] as? String == "ok" else { return .error("\(j["response"] ?? j)") }
        let statuses = ((j["response"] as? [String: Any])?["data"] as? [String: Any])?["statuses"] as? [Any] ?? []
        // Several statuses (stop and take): any error wins, otherwise the first one describes the action.
        for case let s as [String: Any] in statuses {
            if let e = s["error"] as? String { return .error(e) }
        }
        guard let first = statuses.first else { return .ok }
        if let f = (first as? [String: Any])?["filled"] as? [String: Any],
           let size = (f["totalSz"] as? String).flatMap({ Decimal(string: $0) }), let px = (f["avgPx"] as? String).flatMap({ Decimal(string: $0) }) {
            return .filled(size: size, avgPx: px)
        }
        if (first as? [String: Any])?["resting"] != nil { return .resting }
        return .ok
    }
}

public enum HyperliquidAccount {
    public struct Order: Decodable, Sendable, Equatable {
        public let coin: String
        public let oid: UInt64
        public let isTrigger: Bool
        public let triggerPx: Decimal?
        public let limitPx: Decimal?
        public let size: Decimal?
        /// "B" buy, "A" sell.
        public let side: String?
        /// "Limit", "Stop Market", "Take Profit Market"…
        public let orderType: String?
        /// Only shrinks a position (a stop, a take, a close), never opens one.
        public let reduceOnly: Bool
        /// A resting entry's own stop and take (normalTpsl): armed when it fills.
        public let children: [Order]

        enum CodingKeys: String, CodingKey { case coin, oid, isTrigger, triggerPx, limitPx, sz, side, orderType, reduceOnly, children }
        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            coin = try c.decode(String.self, forKey: .coin)
            oid = try c.decode(UInt64.self, forKey: .oid)
            isTrigger = try c.decodeIfPresent(Bool.self, forKey: .isTrigger) ?? false
            triggerPx = try c.decodeIfPresent(String.self, forKey: .triggerPx).flatMap { Decimal(string: $0) }
            limitPx = try c.decodeIfPresent(String.self, forKey: .limitPx).flatMap { Decimal(string: $0) }
            size = try c.decodeIfPresent(String.self, forKey: .sz).flatMap { Decimal(string: $0) }
            side = try c.decodeIfPresent(String.self, forKey: .side)
            orderType = try c.decodeIfPresent(String.self, forKey: .orderType)
            reduceOnly = try c.decodeIfPresent(Bool.self, forKey: .reduceOnly) ?? false
            // Read for the risk sum only: a child in an unexpected shape must not hide the open orders.
            children = (try? c.decodeIfPresent([Order].self, forKey: .children)) ?? []
        }
    }

    public struct Position: Sendable, Equatable {
        public let coin: String
        /// Signed: negative for a short.
        public let size: Decimal
        public let entry: Decimal
        public let unrealizedPnl: Decimal
        public let liquidation: Decimal?
    }

    /// What the open positions and the resting limit entries lose if every stop fills; nil when one of them has no stop.
    public static func riskAtStops(positions: [Position], orders: [Order]) -> Decimal? {
        func stops(_ coin: String, _ own: [Order] = []) -> [Decimal] {
            (own + orders.filter { $0.coin == coin }).filter { $0.isTrigger && ($0.orderType ?? "").contains("Stop") }.compactMap(\.triggerPx)
        }
        var total: Decimal = 0
        for p in positions where p.size != 0 {
            // The stop that fills first is the one nearest the entry on the losing side.
            guard let stop = p.size > 0 ? stops(p.coin).max() : stops(p.coin).min() else { return nil }
            total += max(0, (p.size > 0 ? p.entry - stop : stop - p.entry) * abs(p.size))
        }
        // A limit entry still waiting loses as much once it fills: at its own stop (armed with it) or the coin's.
        for o in orders where !o.isTrigger && !o.reduceOnly {
            guard let px = o.limitPx, let size = o.size, size > 0 else { continue }
            let buy = o.side == "B"
            guard let stop = buy ? stops(o.coin, o.children).max() : stops(o.coin, o.children).min() else { return nil }
            total += max(0, (buy ? px - stop : stop - px) * size)
        }
        return total
    }

    /// What the user has in a vault: its equity and what can be withdrawn now (lockups hold the rest).
    public static func vaultEquity(user: String, vault: String) async throws -> (equity: Decimal, withdrawable: Decimal, lockedUntil: Date?) {
        struct Row: Decodable { let vaultAddress: String; let equity: String; let lockedUntilTimestamp: Double? }
        let rows = try JSONDecoder().decode([Row].self, from: try await info(["type": "userVaultEquities", "user": user]))
        guard let row = rows.first(where: { $0.vaultAddress.lowercased() == vault.lowercased() }) else { return (0, 0, nil) }
        let equity = Decimal(string: row.equity) ?? 0
        let until = row.lockedUntilTimestamp.map { Date(timeIntervalSince1970: $0 / 1000) }
        let unlocked = (until ?? .distantPast) <= Date()
        return (equity, unlocked ? equity : 0, unlocked ? nil : until)
    }

    /// The latest fill that closed (part of) a position in `coin`: price and realised PnL.
    public static func lastClose(_ user: String, coin: String) async throws -> (price: Decimal, pnl: Decimal, time: Date)? {
        struct Fill: Decodable { let coin: String; let px: String; let closedPnl: String; let dir: String; let time: Double }
        let fills = try JSONDecoder().decode([Fill].self, from: try await info(["type": "userFills", "user": user]))
        guard let f = fills.filter({ $0.coin == coin && $0.dir.hasPrefix("Close") }).max(by: { $0.time < $1.time }) else { return nil }
        return (Decimal(string: f.px) ?? 0, Decimal(string: f.closedPnl) ?? 0, Date(timeIntervalSince1970: f.time / 1000))
    }

    /// Open orders on the main exchange and the HIP-3 ones ihatecrypto shows; coins are unique («xyz:CL»).
    public static func openOrders(_ user: String) async throws -> [Order] {
        var out: [Order] = []
        for dex in [nil] + Hyperliquid.hip3Dexes.map(Optional.some) {
            var body = ["type": "frontendOpenOrders", "user": user]
            body["dex"] = dex
            out += try JSONDecoder().decode([Order].self, from: try await info(body))
        }
        return out
    }

    /// Positions on the main exchange and the HIP-3 ones.
    public static func positions(_ user: String) async throws -> [Position] {
        var out: [Position] = []
        for dex in [nil] + Hyperliquid.hip3Dexes.map(Optional.some) { out += try await positions(user, dex: dex) }
        return out
    }

    static func positions(_ user: String, dex: String?) async throws -> [Position] {
        struct P: Decodable { let coin: String; let szi: String; let entryPx: String?; let unrealizedPnl: String; let liquidationPx: String? }
        struct Row: Decodable { let position: P }
        struct State: Decodable { let assetPositions: [Row] }
        var body = ["type": "clearinghouseState", "user": user]
        body["dex"] = dex
        let state = try JSONDecoder().decode(State.self, from: try await info(body))
        return state.assetPositions.map { r in
            Position(coin: r.position.coin, size: Decimal(string: r.position.szi) ?? 0, entry: Decimal(string: r.position.entryPx ?? "") ?? 0,
                     unrealizedPnl: Decimal(string: r.position.unrealizedPnl) ?? 0, liquidation: r.position.liquidationPx.flatMap { Decimal(string: $0) })
        }
    }

    static func info(_ body: [String: String]) async throws -> Data {
        var request = URLRequest(url: URL(string: "https://api.hyperliquid.xyz/info")!, timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await URLSession.shared.data(for: request).0
    }
}
