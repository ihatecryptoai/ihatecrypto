import Foundation
import HyperliquidSigning

public struct Market: Sendable, Equatable {
    public let name: String
    public let markPx: Decimal
    public let szDecimals: Int
    public let maxLeverage: Int
    /// Position in Hyperliquid's perp universe, the asset id in orders; −1 when not loaded from the exchange.
    public let index: Int
    /// Price 24 hours ago, for the entry-point check.
    public let prevDayPx: Decimal?
    /// Hourly funding rate; positive means longs pay shorts.
    public let funding: Decimal?
    /// Average true range of 4-hour candles, % of price; loaded separately from candles.
    public var atrPct: Decimal?
    /// The HIP-3 exchange ("xyz") for markets outside the main one; its margin is a separate balance.
    public let dex: String?
    /// Traded in the last 24 hours, USD; orders the market picker.
    public var volume24h: Decimal = 0
    /// How far the price moved in 24 hours, in %; nil without the price from then.
    public var change24h: Decimal? { prevDayPx.flatMap { $0 > 0 ? (markPx - $0) / $0 * 100 : nil } }

    public enum Group: String, CaseIterable, Sendable {
        case crypto = "Crypto"
        case commodities = "Commodities"
        case stocks = "Stocks"
        case indices = "Indices & FX"
    }

    static let commodities: Set<String> = ["CL", "BRENTOIL", "GOLD", "SILVER", "NATGAS", "COPPER", "PLATINUM", "PALLADIUM", "URANIUM",
                                           "WHEAT", "CORN", "COCOA", "COFFEE", "SUGAR"]
    static let indices: Set<String> = ["SP500", "XYZ100", "NDX", "DJI", "VIX", "DXY", "JPY", "EUR", "GBP", "CNH", "KRW", "EWY", "XLE", "SOXL", "KORU"]

    /// Main-exchange markets are crypto; HIP-3 ones are sorted by what they track.
    public var group: Group {
        guard dex != nil, let base = name.split(separator: ":").last.map(String.init) else { return .crypto }
        if Self.commodities.contains(base) { return .commodities }
        if Self.indices.contains(base) { return .indices }
        return .stocks
    }

    public init(name: String, markPx: Decimal, szDecimals: Int, maxLeverage: Int, index: Int = -1,
                prevDayPx: Decimal? = nil, funding: Decimal? = nil, atrPct: Decimal? = nil, dex: String? = nil) {
        self.name = name
        self.markPx = markPx
        self.szDecimals = szDecimals
        self.maxLeverage = maxLeverage
        self.index = index
        self.prevDayPx = prevDayPx
        self.funding = funding
        self.atrPct = atrPct
        self.dex = dex
    }
}

public struct Sizing: Sendable, Equatable {
    public let size: Decimal
    public let entry: Decimal
    public let notional: Decimal
    public let margin: Decimal
    public let stopPrice: Decimal
    public let takePrice: Decimal?
    public let liquidationPrice: Decimal
    /// Fees for entry and exit at the stop: Hyperliquid's taker fee and ihatecrypto's.
    public let feesUSD: Decimal
    /// ihatecrypto's part of `feesUSD`.
    public let builderFeesUSD: Decimal
    public let lossAtStopUSD: Decimal
    public let profitAtTakeUSD: Decimal?
}

/// Position math. Code decides the numbers; the model never does.
public enum RiskCalc {
    /// Hyperliquid tier-0 taker fee (RESEARCH.md §8).
    public static let takerFee = Decimal(string: "0.00045")!
    /// ihatecrypto's fee on each side (HLBuilder), on top of Hyperliquid's.
    public static let builderFee = HLBuilder.share
    /// What each side of a trade pays in all.
    public static let tradeFee = takerFee + builderFee
    /// Hyperliquid's tier-0 spot taker fee; ihatecrypto's fee is taken on spot sells only.
    public static let spotTakerFee = Decimal(string: "0.0007")!

    /// `entry`: a limit price; the mark when nil.
    public static func size(
        riskUSD: Decimal, stopPct: Decimal, takePct: Decimal?, leverage: Decimal, side: Side, market: Market, entry limit: Decimal? = nil
    ) -> Sizing {
        let entry = limit ?? market.markPx
        let dir: Decimal = side == .long ? 1 : -1
        let s = stopPct / 100
        let stop = entry * (1 - dir * s)
        // Loss per coin if the stop fills at its price: the move plus the fees on both legs.
        let perCoinLoss = entry * s + (entry + stop) * tradeFee
        let size = (riskUSD / perCoinLoss).rounded(market.szDecimals, .down)
        let notional = size * entry
        let fees = size * (entry + stop) * tradeFee
        let take = takePct.map { entry * (1 + dir * $0 / 100) }
        let profit = take.map { size * abs($0 - entry) - size * (entry + $0) * tradeFee }
        // Isolated margin, lowest tier: maintenance = 1 / (2 · max leverage).
        // ponytail: ignores Hyperliquid margin tiers, fine below the first tier's notional; read marginTables for big positions.
        let l = 1 / (2 * Decimal(market.maxLeverage))
        let liq = entry * (1 - dir * (1 / leverage - l) / (1 - dir * l))
        return Sizing(
            size: size, entry: entry, notional: notional, margin: notional / leverage,
            stopPrice: stop, takePrice: take, liquidationPrice: liq,
            feesUSD: fees, builderFeesUSD: size * (entry + stop) * builderFee, lossAtStopUSD: size * entry * s + fees, profitAtTakeUSD: profit
        )
    }
}

public struct Evaluation: Sendable {
    public var missing: [String] = []
    public var problem: String?
    public var sizing: Sizing?
}

public struct BridgeQuote: Sendable, Equatable {
    /// "Relay" or "NEAR 1Click".
    public let provider: String
    /// What to send, including `reserve` for slippage.
    public let amountIn: Decimal
    public let reserve: Decimal
    public let amountOut: Decimal
    /// Time as measured in live runs, not the provider's estimate.
    public let time: String
    /// The other provider's price, for comparison.
    public var alternative: String?
    /// The other provider's fee, for the choice of how to open a new Hyperliquid account.
    public var alternativeFee: Decimal?
    /// The first deposit into a Hyperliquid account that doesn't exist yet: the price includes its one-time opening fee.
    public var opening = false

    public init(provider: String, amountIn: Decimal, reserve: Decimal, amountOut: Decimal, time: String, alternative: String? = nil) {
        self.provider = provider
        self.amountIn = amountIn
        self.reserve = reserve
        self.amountOut = amountOut
        self.time = time
        self.alternative = alternative
    }
}

/// What the network told us. nil means «no data», never zero.
public struct Live: Sendable {
    public var solanaAddressSet = false
    public var solanaUSDC: Decimal?
    public var hlAddressSet = false
    public var hlWithdrawable: Decimal?
    public var marketsLoaded = false
    public var market: Market?
    public var quote: BridgeQuote?
    /// The EVM network the trade's USDC leaves from when Solana is short, and what it holds there; nil = Solana.
    public var from: (name: String, usdc: Decimal)?
    /// USDC on Solana + Hyperliquid account value + vaults: what the risk rules measure against.
    public var equity: Decimal?
    public var policy = RiskPolicy()
    /// Loss at the stops of positions already open; nil when one has no stop.
    public var openRisk: Decimal? = 0
    /// What `Plan.resolve` filled in, shown in the risk block.
    public var notes: [String] = []

    public init() {}
}

public struct Block: Identifiable, Sendable, Equatable {
    public enum Status: Sendable { case ready, warning, needsInput, blocked, noData, info }

    public let id: String
    public let label: String
    public let title: String
    public let lines: [String]
    public let status: Status
}

public enum Plan {
    /// Hyperliquid rejects orders below $10 notional.
    static let minNotional: Decimal = 10

    public static func evaluate(_ i: TradeIntent, market: Market?) -> Evaluation {
        var e = Evaluation()
        if i.riskUSD == nil { e.missing.append("risk in $") }
        if i.stopPct == nil { e.missing.append("stop in %") }
        guard e.missing.isEmpty, let m = market, let risk = i.riskUSD, let stop = i.stopPct else { return e }
        let lev = i.leverage ?? 1

        if risk <= 0 { e.problem = "Risk must be above zero."; return e }
        if stop <= 0 || stop >= 100 { e.problem = "Stop must be above 0% and below 100%."; return e }
        if let t = i.takePct, t <= 0 || (i.side == .short && t >= 100) { e.problem = "Take-profit is out of range."; return e }
        // The signer refuses more than 20× whatever the market allows (HLGuard.maxLeverage): said here, before money moves.
        let maxLev = min(m.maxLeverage, 20)
        if lev < 1 || lev > Decimal(maxLev) { e.problem = "Leverage for \(m.name): 1× to \(maxLev)×."; return e }
        if lev != lev.rounded(0, .down) { e.problem = "Leverage on Hyperliquid is a whole number: \(Fmt.num(lev.rounded(0, .down)))× or \(Fmt.num(lev.rounded(0, .up)))×."; return e }

        if let px = i.entryPx, i.side == .long ? px >= m.markPx : px <= m.markPx {
            e.problem = "Limit \(Fmt.price(px)) is \(i.side == .long ? "above" : "below") the market \(Fmt.price(m.markPx)) and would fill at once. For a market entry, type “at market”."
            return e
        }
        let s = RiskCalc.size(riskUSD: risk, stopPct: stop, takePct: i.takePct, leverage: lev, side: i.side, market: m, entry: i.entryPx)
        e.sizing = s
        let liqBeforeStop = i.side == .long ? s.liquidationPrice >= s.stopPrice : s.liquidationPrice <= s.stopPrice
        if s.size <= 0 {
            e.problem = "Risk is too small for the minimum \(m.name) size step."
        } else if s.notional < minNotional {
            e.problem = "Notional \(Fmt.usd(s.notional)) is below the Hyperliquid $10 minimum order. Raise the risk or tighten the stop."
        } else if liqBeforeStop {
            e.problem = "Liquidation (\(Fmt.price(s.liquidationPrice))) comes before the stop. Lower the leverage."
        }
        return e
    }

    /// The money a plan holds against what its command named, by code rather than by a reader: «N USDC» is the most
    /// that goes in as margin, «risk $N» the most lost at the stop. nil when the plan keeps to it, else what differs.
    /// `named` is the intent as the commands left it, before the rules (resolve) turned the money into a risk.
    /// A classifier reading the finished plan missed a tenfold amount or money taken as risk in a quarter of the cases
    /// (Tools/jevcheck, plans); in the command it sees them, and from the command to the plan the numbers are code's.
    /// Only «more than named» is a problem: the size rounds down to the market's step, so a little less is the rule.
    /// What the rules themselves may add is allowed for: an order raised to Hyperliquid's $10 minimum rounds its size up
    /// by a step and its risk up by a cent, and a cent of risk at a 0.5% stop is $2 of position — «5 USDC» at 2× came
    /// out as $5.90 of margin (seen 2026-10-14 in this check's own test; the sizing is left as it is).
    public static func moneyProblem(named i: TradeIntent, sizing s: Sizing, market m: Market) -> String? {
        guard s.notional > 0, s.lossAtStopUSD > 0 else { return nil }
        let cent = Decimal(string: "0.01")!
        // In dollars of position: one step of the size, and what a cent of risk buys.
        let slack = s.entry / pow(10, m.szDecimals) + cent * s.notional / s.lossAtStopUSD
        func over(_ got: Decimal, _ named: Decimal) -> Bool { got > named * Decimal(string: "1.01")! + slack * got / s.notional + cent }
        if i.riskUSD == nil, let money = i.amountUSDC, money > 0, over(s.margin, money) {
            return "The plan puts \(Fmt.usd(s.margin)) into the trade, more than the \(Fmt.num(money)) USDC its command named."
        }
        if let risk = i.riskUSD, risk > 0, over(s.lossAtStopUSD, risk) {
            return "The plan loses \(Fmt.usd(s.lossAtStopUSD)) at the stop, more than the $\(Fmt.num(risk)) risk its command named."
        }
        return nil
    }

    /// USDC that must arrive on HyperCore, or nil when the trading balance already covers margin and fees
    /// or the position has not passed the checks yet.
    /// `mark`: Hyperliquid holds an order's margin at the current price when that's above the limit — a long on a
    /// pullback. 0.09 SOL at a 115.98 limit is $2.09 at 5×, yet $2.15 is held while SOL trades at 119.6, and an order
    /// funded to the limit's margin was refused with «Insufficient margin» (test wallet, 2026-10-03).
    public static func bridgeNeed(_ i: TradeIntent, _ e: Evaluation, hlWithdrawable: Decimal?, mark: Decimal? = nil) -> Decimal? {
        guard i.amountUSDC == nil, e.problem == nil, let s = e.sizing else { return nil }
        let held = s.entry > 0 ? s.margin * max(1, (mark ?? s.entry) / s.entry) : s.margin
        let need = held + s.feesUSD - (hlWithdrawable ?? 0)
        return need > 0 ? need.rounded(2, .up) : nil
    }

    public static func blocks(_ i: TradeIntent, _ e: Evaluation, _ live: Live) -> [Block] {
        let need = bridgeNeed(i, e, hlWithdrawable: live.hlWithdrawable, mark: live.market?.markPx)
        let bridging = i.amountUSDC != nil || need != nil
        let send = live.quote?.amountIn ?? i.amountUSDC ?? need
        return [
            source(live, send: bridging ? send : nil),
            bridge(i, e, live, need: need),
            position(i, e, live),
            protection(i, e),
            risk(i, e, live),
        ]
    }

    static func source(_ live: Live, send: Decimal?) -> Block {
        let title = "USDC · \(live.from?.name ?? "Solana")"
        guard live.from != nil || live.solanaAddressSet else {
            return Block(id: "source", label: "Source", title: title, lines: ["Add a Solana address in Settings."], status: .needsInput)
        }
        guard let have = live.from?.usdc ?? live.solanaUSDC else {
            return Block(id: "source", label: "Source", title: title, lines: ["Balance: no data."], status: .noData)
        }
        var lines = ["Wallet: \(Fmt.num(have)) USDC."]
        if let send {
            lines.append("Send \(Fmt.num(send)) USDC.")
            if send > have {
                lines.append("Short by \(Fmt.num(send - have)) USDC.")
                return Block(id: "source", label: "Source", title: title, lines: lines, status: .blocked)
            }
        }
        return Block(id: "source", label: "Source", title: title, lines: lines, status: .ready)
    }

    static func bridge(_ i: TradeIntent, _ e: Evaluation, _ live: Live, need: Decimal?) -> Block {
        let label = "Bridge · \(live.quote?.provider ?? "Relay or 1Click")"
        let title = "\(live.from?.name ?? "Solana") → HyperCore USDC"
        if i.amountUSDC == nil, e.problem != nil {
            return Block(id: "bridge", label: label, title: title, lines: ["The amount appears once the position passes the checks."], status: .needsInput)
        }
        if i.amountUSDC == nil, need == nil {
            let line = e.sizing == nil
                ? "The amount appears once the position is sized."
                : "Not needed: the Hyperliquid balance covers margin and fees."
            return Block(id: "bridge", label: label, title: title, lines: [line], status: e.sizing == nil ? .needsInput : .info)
        }
        guard let q = live.quote else {
            return Block(id: "bridge", label: label, title: title, lines: ["Quote: no data."], status: .noData)
        }
        var lines = [
            "Send \(Fmt.num(q.amountIn)) → receive \(Fmt.num(q.amountOut)) USDC.",
            q.reserve > 0
                ? "Fees and spread ≈ \(Fmt.usd(q.amountIn - q.reserve - q.amountOut)), slippage reserve \(Fmt.num(q.reserve)) USDC."
                : "Fees and spread ≈ \(Fmt.usd(q.amountIn - q.amountOut)).",
            q.time,
        ]
        if q.opening {
            lines.append(q.provider == "Relay" ? "Your Hyperliquid account is new: this first deposit includes its one-time opening fee."
                                               : "Your Hyperliquid account is new: NEAR Intents opens it without the opening fee.")
        }
        // Worth saying before the signature: a bridge that costs more than the take-profit earns makes the trade a loss.
        let costly = e.sizing?.profitAtTakeUSD.map { q.amountIn - q.reserve - q.amountOut > $0 } ?? false
        if costly, let profit = e.sizing?.profitAtTakeUSD {
            lines.append("The bridge costs more than the take-profit earns (\(Fmt.usd(profit))): even at the take this trade loses money"
                         + (q.opening ? ", unless the account is used again." : "."))
        }
        if let alternative = q.alternative { lines.append(alternative) }
        lines.append("Dry quote: no addresses are sent.")
        // A set amount («risk $1, 5 USDC») must still carry the margin once the bridge has taken its fee.
        if i.amountUSDC != nil, let s = e.sizing, q.amountOut + (live.hlWithdrawable ?? 0) < s.margin + s.feesUSD {
            lines.insert("Only \(Fmt.num(q.amountOut)) USDC arrive, and the position needs \(Fmt.usd(s.margin + s.feesUSD)) for margin and fees: "
                         + "send more or lower the risk.", at: 0)
            return Block(id: "bridge", label: label, title: title, lines: lines, status: .blocked)
        }
        return Block(id: "bridge", label: label, title: title, lines: lines, status: costly ? .warning : .ready)
    }

    static func position(_ i: TradeIntent, _ e: Evaluation, _ live: Live) -> Block {
        let label = "Position · Hyperliquid"
        let title = "\(i.side == .long ? "Long" : "Short") \(IntentParser.pretty(i.asset))"
        guard let m = live.market else {
            let line = live.marketsLoaded ? "No \(i.asset) market on Hyperliquid." : "Price: no data."
            return Block(id: "position", label: label, title: title, lines: [line], status: live.marketsLoaded ? .blocked : .noData)
        }
        guard e.missing.isEmpty else {
            return Block(id: "position", label: label, title: title, lines: [
                "Mark \(Fmt.price(m.markPx)).",
                "Set \(e.missing.joined(separator: " and ")).",
            ], status: .needsInput)
        }
        guard let s = e.sizing else {
            return Block(id: "position", label: label, title: title, lines: [e.problem ?? ""], status: .blocked)
        }
        var lines = [
            i.entryPx == nil
                ? "\(Fmt.num(s.size, maxFraction: m.szDecimals)) \(m.name) at ≈ \(Fmt.price(s.entry)) (mark)."
                : "Limit: \(Fmt.num(s.size, maxFraction: m.szDecimals)) \(m.name) at \(Fmt.price(s.entry)), now \(Fmt.price(m.markPx)). Waiting; stop and take-profit start after entry.",
            "Notional \(Fmt.usd(s.notional)), margin \(Fmt.usd(s.margin)) at \(Fmt.num(i.leverage ?? 1))× leverage\(i.leverage == nil ? " (default)" : "").",
            "Liquidation ≈ \(Fmt.price(s.liquidationPrice)), isolated margin.",
            "Fees per side: Hyperliquid \(Fmt.num(RiskCalc.takerFee * 100, maxFraction: 3))%, ihatecrypto \(Fmt.num(RiskCalc.builderFee * 100, maxFraction: 3))%.",
        ]
        if let p = e.problem { lines.append(p) }
        return Block(id: "position", label: label, title: title, lines: lines, status: e.problem == nil ? .ready : .blocked)
    }

    static func protection(_ i: TradeIntent, _ e: Evaluation) -> Block {
        let label = "Protection"
        guard let s = e.sizing, let stopPct = i.stopPct else {
            return Block(id: "protection", label: label, title: "Stop & take-profit", lines: ["Set a stop in %, e.g. “stop 3%”."], status: .needsInput)
        }
        var lines = [
            "Stop \(Fmt.pct(stopPct)) → \(Fmt.price(s.stopPrice)). Loss ≈ \(Fmt.usd(s.lossAtStopUSD)) incl. \(Fmt.usd(s.feesUSD)) fees (ihatecrypto \(Fmt.usd(s.builderFeesUSD))).",
        ]
        if let t = i.takePct, let tp = s.takePrice, let profit = s.profitAtTakeUSD {
            lines.append("Take-profit \(Fmt.pct(t)) → \(Fmt.price(tp)). Profit ≈ \(Fmt.usd(profit)).")
        } else {
            lines.append("No take-profit set.")
        }
        if i.partialTake == true {
            let oneR = s.entry + (s.entry - s.stopPrice)
            lines.append(s.notional / 2 >= minNotional
                ? "In parts: half at 1R → \(Fmt.price(oneR)), then stop to breakeven (while ihatecrypto is open)."
                : "Can't take in parts: half the position is below the $10 minimum order.")
        }
        if let trail = i.trailingPct {
            lines.append("Trailing \(Fmt.pct(trail)) from the best price; moves while ihatecrypto is open.")
        }
        lines.append("Triggers on mark price; a market exit may fill worse.")
        return Block(id: "protection", label: label, title: "Stop & take-profit", lines: lines, status: e.problem == nil ? .ready : .blocked)
    }
}
