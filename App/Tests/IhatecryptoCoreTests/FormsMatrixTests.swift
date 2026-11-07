import Foundation
@testable import IhatecryptoCore
import Testing

/// Every English form the router tells the model to write (ChatRouter.forms), with amounts, networks, tickers, names and
/// casing varied — thousands of commands — each read the way Workspace.runCanonical reads it, in its order. A form
/// that lands on another action, another amount or another place is a money move gone wrong.
/// ponytail: `read` mirrors runCanonical's order by hand; move both together when a parser moves.

/// Hyperliquid's markets on 2026-10-04, main exchange and xyz, as the app loads them.
private let markets = Set(("BTC,ETH,ATOM,DYDX,SOL,AVAX,BNB,APE,OP,LTC,ARB,DOGE,INJ,SUI,kPEPE,CRV,LDO,LINK,STX,CFX,GMX,SNX,XRP,BCH,APT,AAVE,COMP,WLD,YGG,TRX,kSHIB,UNI,SEI,RUNE,ZRO,DOT,BANANA,TRB,BIGTIME,KAS,BLUR,TIA,BSV,ADA,MINA,POLYX,GAS,PENDLE,FET,NEAR,MEME,ORDI,NEO,ZEN,FIL,PYTH,SUSHI,IMX,kBONK,GMT,SUPER,JUP,kLUNC,RSR,GALA,JTO,ACE,WIF,CAKE,PEOPLE,ENS,ETC,XAI,MANTA,UMA,ONDO,ALT,ZETA,DYM,W,STRK,TAO,AR,kFLOKI,BOME,ETHFI,ENA,MNT,TNSR,SAGA,MERL,HBAR,POPCAT,EIGEN,REZ,NOT,TURBO,BRETT,IO,ZK,RENDER,POL,CELO,HMSTR,kNEIRO,GOAT,MOODENG,GRASS,PURR,PNUT,XLM,SAND,IOTA,ALGO,HYPE,ME,MOVE,VIRTUAL,PENGU,USUAL,FARTCOIN,AIXBT,BIO,GRIFFAIN,SPX,S,MORPHO,TRUMP,MELANIA,ANIME,VVV,BERA,LAYER,KAITO,NIL,PAXG,BABY,WCT,HYPER,ZORA,INIT,NXPC,SOPH,RESOLV,SYRUP,PUMP,PROVE,XPL,WLFI,LINEA,SKY,ASTER,AVNT,STBL,0G,HEMI,APEX,2Z,ZEC,MON,MET,MEGA,CC,ICP,AERO,STABLE,FOGO,LIT,XMR,AXS,DASH,SKR,AZTEC,CHIP,GRAM,CASHCAT,PONS,USELESS,"
    + "xyz:XYZ100,xyz:TSLA,xyz:NVDA,xyz:GOLD,xyz:HOOD,xyz:INTC,xyz:PLTR,xyz:COIN,xyz:META,xyz:AAPL,xyz:MSFT,xyz:ORCL,xyz:GOOGL,xyz:AMZN,xyz:AMD,xyz:MU,xyz:SNDK,xyz:MSTR,xyz:CRCL,xyz:NFLX,xyz:COST,xyz:LLY,xyz:SKHX,xyz:TSM,xyz:JPY,xyz:EUR,xyz:SILVER,xyz:RIVN,xyz:BABA,xyz:CL,xyz:COPPER,xyz:NATGAS,xyz:SMSN,xyz:PLATINUM,xyz:USAR,xyz:CRWV,xyz:URNM,xyz:PALLADIUM,xyz:GME,xyz:KR200,xyz:SOFTBANK,xyz:JP225,xyz:HYUNDAI,xyz:KIOXIA,xyz:EWY,xyz:EWJ,xyz:BRENTOIL,xyz:HIMS,xyz:SP500,xyz:DKNG,xyz:LITE,xyz:XLE,xyz:BX,xyz:PURRDAT,xyz:MRVL,xyz:RKLB,xyz:BIRD,xyz:DRAM,xyz:CBRS,xyz:EWZ,xyz:ZM,xyz:EBAY,xyz:ARM,xyz:EWT,xyz:GBP,xyz:SPCX,xyz:ASML,xyz:MINIMAX,xyz:BB,xyz:QNT,xyz:DELL,xyz:IBM,xyz:AVGO,xyz:NOW,xyz:NBIS,xyz:WDC,xyz:NOK,xyz:SMH,xyz:BE,xyz:ZHIPU,xyz:QCOM,xyz:STRC,xyz:BOT,xyz:AMAT,xyz:GIGADEV,xyz:SHAZ,xyz:SKHY,xyz:KSTR,xyz:CXMT,xyz:GEV,xyz:KORU,xyz:UNITREE,xyz:LYTE,xyz:NCLD,xyz:SOXL,xyz:MAGS,xyz:IREN,xyz:NET,xyz:CRWD,xyz:RDDT,xyz:AAOI,xyz:MRNA,xyz:XBI,xyz:SHEIN,xyz:BMNR,xyz:SNXX,xyz:CVX,xyz:TLT,xyz:HO,xyz:OURA")
    .split(separator: ",").map(String.init))

/// What the app does with a command.
indirect enum Read: Equatable {
    case cancelTasks, task(ScheduledTask.Every?, [Read]), steps([Read]), app(String)
    case token(TokenCommand), chart(ChartCommand), pnl(PnLCommand), takeChoice, loanGuard(LoanGuardCommand), alert(AlertCommand), folder(FolderCommand)
    case book(AddressBookCommand), wallet(WalletCommand), position(PositionCommand), plan(TradeIntent), none

    var shown: String {
        switch self {
        case .plan(let i): "plan(\(i.command))"
        case .steps(let s): "steps(" + s.map(\.shown).joined(separator: " → ") + ")"
        case .task(let e, let s): "task(\(e.map { "\($0)" } ?? "once"): " + s.map(\.shown).joined(separator: " → ") + ")"
        default: String(describing: self).replacingOccurrences(of: "IhatecryptoCore.", with: "")
        }
    }
}

/// Workspace.runCanonical's order. `positionOpen`: a position is open on the market the command names (or the only one).
func read(_ c: String, plan: TradeIntent = TradeIntent(), positionOpen: Bool = false) -> Read {
    if ScheduledTask.cancelsAll(c) { return .cancelTasks }
    if let t = ScheduledTask.parse(c) { return .task(t.every, t.steps.map { read($0) }) }
    if case .pay? = WalletCommand.parse(c), let w = WalletCommand.parse(c) { return .wallet(w) }
    if let merged = ChatRouter.planChange(c, markets: markets) { return read(merged, plan: plan, positionOpen: positionOpen) }
    if let steps = ChatRouter.steps(c) { return .steps(steps.map { read($0, plan: plan, positionOpen: positionOpen) }) }
    let app = c.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".! "))
    if ["new canvas", "clear the plan", "lock", "create an automation wallet"].contains(app) { return .app(app) }
    if let x = FolderCommand.parse(c) { return .folder(x) }
    if let x = TokenCommand.parse(c) { return .token(x) }
    if let x = ChartCommand.parse(c, markets: markets) { return .chart(x) }
    if let x = PnLCommand.parse(c, markets: markets) { return .pnl(x) }
    if TakeStyle.asksForChoice(c), plan != TradeIntent() { return .takeChoice }
    if let x = LoanGuardCommand.parse(c) { return .loanGuard(x) }
    if let x = AlertCommand.parse(c, markets: markets) { return .alert(x) }
    if let x = AddressBookCommand.parse(c) { return .book(x) }
    if let x = WalletCommand.parse(c) { return .wallet(x) }
    if positionOpen, let x = PositionCommand.parse(c) { return .position(x) }
    if c.lowercased().contains(/^\s*(?:close|cancel\s+orders|move\s+stop\s+to\s+breakeven)\b/) { return .none }
    return IntentParser.apply(c, to: plan, markets: markets).map { .plan($0.started) } ?? .none
}

private func d(_ s: String) -> Decimal { Decimal(string: s)! }

/// SplitMix64: the same «random» plans on every run.
private struct Seeded {
    var state: UInt64
    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
    mutating func chance(_ p: Double) -> Bool { Double(next() % 10_000) / 10_000 < p }
    mutating func pick<T>(_ a: [T]) -> T { a[Int(next() % UInt64(a.count))] }
}

struct FormCase { let family: String; let command: String; let expected: Read; var plan = TradeIntent(); var positionOpen = false }

private let amounts: [(String, Decimal)] = [("1", 1), ("2.5", d("2.5")), ("0.01", d("0.01")), ("1000", 1000), ("1,000", 1000), ("12.345", d("12.345")),
                                            ("2,500.5", d("2500.5")), ("5.00", 5)]
private let someAmounts: [(String?, Decimal?)] = [(nil, nil)] + amounts.map { ($0.0, $0.1) }
private let names = ["Mom", "Bybit", "New wallet", "Cold wallet 2"]
private let contract = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"
private let evmAddress = "0x23618e81E3f5cdF7f54C3d65f7FBc0aBf5B21E8f"
private let mint = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
private let solAddress = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm"

func formCases() -> [FormCase] {
    var out: [FormCase] = []
    func add(_ family: String, _ c: String, _ r: Read, plan: TradeIntent = TradeIntent(), position: Bool = false) {
        out.append(FormCase(family: family, command: c, expected: r, plan: plan, positionOpen: position))
    }
    func w(_ c: WalletCommand) -> Read { .wallet(c) }
    func n(_ a: String?) -> String { a.map { "\($0) " } ?? "" }

    // EARNING
    for (a, v) in someAmounts {
        for s in ["USDC", "SOL", "USDT"] { add("lend", "lend \(n(a))\(s)", w(.lendDeposit(s, v))) }
        for s in ["USDC", "SOL"] {
            add("vault out", "withdraw \(n(a))\(s) from the vault", w(.lendWithdraw(s, v)))
            add("kamino", "deposit \(n(a))\(s) to kamino", w(.kamino(deposit: true, symbol: s, amount: v)))
            add("kamino", "withdraw \(n(a))\(s) from kamino", w(.kamino(deposit: false, symbol: s, amount: v)))
        }
        add("hlp", "deposit \(n(a))USDC to HLP", w(.hlp(deposit: true, amount: v)))
        add("hlp", "withdraw \(n(a))USDC from HLP", w(.hlp(deposit: false, amount: v)))
        add("aave", "lend \(n(a))USDC on Aave", w(.aave(deposit: true, amount: v)))
        add("aave", "withdraw \(n(a))USDC from Aave", w(.aave(deposit: false, amount: v)))
        add("morpho", "deposit \(n(a))USDC into Morpho", w(.morpho(deposit: true, amount: v)))
        add("morpho", "withdraw \(n(a))USDC from Morpho", w(.morpho(deposit: false, amount: v)))
        for net in ["Base", "Arbitrum", "Polygon", "Ethereum"] {
            add("aave", "lend \(n(a))USDC on Aave on \(net)", w(.aave(deposit: true, amount: v, network: net.lowercased())))
            add("aave", "withdraw \(n(a))USDC from Aave on \(net)", w(.aave(deposit: false, amount: v, network: net.lowercased())))
            guard net != "Polygon" else { continue }
            add("morpho", "deposit \(n(a))USDC into Morpho on \(net)", w(.morpho(deposit: true, amount: v, network: net.lowercased())))
            add("morpho", "withdraw \(n(a))USDC from Morpho on \(net)", w(.morpho(deposit: false, amount: v, network: net.lowercased())))
        }
        guard let a else { continue }
        add("stake", "stake \(a) SOL", w(.stake(v)))
        add("stake", "unstake \(a) JitoSOL", w(.unstake(v)))
    }
    add("yields", "where to keep USDC", w(.yields("USDC")))
    add("yields", "where to keep SOL", w(.yields("SOL")))

    // SPREAD and REBALANCE
    let withouts: [(String, [String])] = [("", []), (" without kamino", ["kamino"]), (" without kamino, aave", ["kamino", "aave"]),
                                          (" without jupiter, morpho", ["jupiter", "morpho"]), (" without aave", ["aave"])]
    let shares: [(String, Decimal?)] = [("", nil), (" max 30% per protocol", 30), (" max 50% per protocol", 50)]
    let sources = ["Solana", "Base", "Arbitrum", "Polygon", "Ethereum", "Robinhood", "Tempo"]
    for (a, v) in someAmounts {
        for (src, from) in [("", nil)] + sources.map({ (" from \($0)", $0.lowercased()) }) as [(String, String?)] {
            for (sh, share) in shares {
                for (wo, keys) in withouts {
                    add("spread", "spread \(n(a))USDC\(src) across the best rates\(sh)\(wo)", w(.allocate("USDC", maxShare: share, amount: v, without: keys, from: from)))
                }
            }
        }
        for (sh, share) in shares {
            for (wo, keys) in withouts { add("spread", "spread \(n(a))SOL across the best rates\(sh)\(wo)", w(.allocate("SOL", maxShare: share, amount: v, without: keys))) }
        }
    }
    add("rebalance", "rebalance USDC", w(.rebalance(maxShare: nil)))
    add("rebalance", "rebalance USDC max 40% per protocol", w(.rebalance(maxShare: 40)))
    add("rewards", "claim rewards", w(.claimRewards))
    add("rewards", "sweep rewards", w(.sweepRewards(minUSD: nil, into: nil)))
    for into in ["USDC", "SOL", "JUP", "WETH", "cbBTC"] { add("rewards", "sweep rewards into \(into)", w(.sweepRewards(minUSD: nil, into: into == "USDC" ? nil : into.uppercased()))) }
    for m in ["5", "10", "12.5", "1,000"] {
        let min = d(m.replacingOccurrences(of: ",", with: ""))
        add("rewards", "sweep rewards over $\(m)", w(.sweepRewards(minUSD: min, into: nil)))
        for into in ["USDC", "SOL", "JUP", "WETH"] { add("rewards", "sweep rewards over $\(m) into \(into)", w(.sweepRewards(minUSD: min, into: into == "USDC" ? nil : into))) }
    }

    // MOVING MONEY
    let evm = ["Base", "Arbitrum", "Polygon", "Ethereum", "Tempo"]
    for (a, v) in someAmounts {
        add("to solana", "withdraw \(n(a))USDC to Solana", w(.withdrawToSolana(v)))
        for net in evm {
            let back: WalletCommand = net == "Base" ? .baseToSolana(v) : net == "Arbitrum" ? .arbitrumToSolana(v) : .evmToSolana(net.lowercased(), v)
            add("to solana", "move \(n(a))USDC from \(net) to Solana", w(back))
        }
        add("to solana", "move \(n(a))USDG from Robinhood to Solana", w(.robinhoodToSolana(v)))
        add("to solana", "move \(n(a))USDC from Robinhood to Solana", w(.robinhoodToSolana(v)))
        for net in evm + ["Robinhood"] { add("from solana", "move \(n(a))USDC from Solana to \(net)", w(.solanaTo(net.lowercased(), v))) }
        for from in evm + ["Robinhood"] {
            for to in evm + ["Robinhood", "Hyperliquid"] where to != from {
                add("bridge", "move \(n(a))USDC from \(from) to \(to)", w(.bridge(from: from.lowercased(), to: to.lowercased(), v)))
            }
        }
    }
    for net in ["Base", "Arbitrum", "Polygon", "Ethereum", "Robinhood"] {
        add("gas", "add gas on \(net)", w(.gas(net.lowercased(), usd: nil)))
        for (a, v) in amounts { add("gas", "add gas on \(net) for \(a) USDC", w(.gas(net.lowercased(), usd: v))) }
    }

    // SENDING
    let sendNets = ["Ethereum", "Base", "Arbitrum", "Polygon", "Solana", "Robinhood", "Tempo"]
    for name in names {
        for (a, v) in amounts {
            add("send usdc", "send \(a) USDC to \(name)", w(.sendTo(name: name, amount: v)))
            for net in sendNets {
                add("send usdc", "send \(a) USDC to \(name) on \(net)", w(.sendTo(name: name, amount: v, network: net.lowercased())))
                add("send usdc", "send \(a) USDC on \(net.lowercased()) to \(name)", w(.sendTo(name: name, amount: v, network: net.lowercased())))
            }
            add("send near", "send \(a) NEAR to \(name)", w(.sendNear(name: name, amount: v)))
            add("send sol", "send \(a) SOL to \(name)", w(.sendSol(name: name, amount: v)))
            add("send sol", "send \(a) SOL on solana to \(name)", w(.sendSol(name: name, amount: v)))
        }
        add("send near", "send all NEAR to \(name)", w(.sendNear(name: name, amount: nil)))
        add("send near", "close the NEAR account into \(name)", w(.closeNear(name: name)))
        let tokens: [(String, [String])] = [("RAY", ["solana"]), ("AI", ["robinhood"]), ("gtUSDCp", ["base", "arbitrum"]), ("aBasUSDC", ["base"]),
                                            ("jlUSDC", ["solana"]), (contract, ["robinhood", "base"]), (mint, ["solana"]), ("USDG", ["robinhood"]),
                                            ("ETH", ["base", "ethereum"]), ("POL", ["polygon"]), ("SOL", ["solana"]), ("PEPE", ["ethereum"])]
        for (token, nets) in tokens {
            for net in nets {
                add("send token", "send all \(token) on \(net) to \(name)", w(.sendToken(name: name, token: token, amount: nil, network: net)))
                add("send token", "send 3.5 \(token) on \(net) to \(name)", token == "SOL" ? w(.sendSol(name: name, amount: d("3.5")))
                    : token == "USDG" ? w(.sendTo(name: name, amount: d("3.5"), network: net)) : w(.sendToken(name: name, token: token, amount: d("3.5"), network: net)))
                add("send token", "send 1973.719299 \(token) on \(net) to \(name)", token == "SOL" ? w(.sendSol(name: name, amount: d("1973.719299")))
                    : token == "USDG" ? w(.sendTo(name: name, amount: d("1973.719299"), network: net)) : w(.sendToken(name: name, token: token, amount: d("1973.719299"), network: net)))
            }
        }
    }

    // SPOT ON SOLANA, NEAR, STOCKS, TOKENS
    let spot = [("SOL", SpotAsset.all[0]), ("BTC", SpotAsset.all[1]), ("ETH", SpotAsset.all[2]), ("JUP", SpotAsset.all[3]), ("JitoSOL", SpotAsset.all[4])]
    for (t, asset) in spot {
        add("spot", "buy \(t)", w(.buy(asset, usdc: nil)))
        for (a, v) in amounts { add("spot", "buy \(t) for \(a) USDC", w(.buy(asset, usdc: v))) }
        for (every, seconds) in [("day", 86400), ("week", 604_800), ("3 days", 259_200), ("hour", 3600)] {
            for orders in [2, 4] {
                add("jupiter dca", "Jupiter DCA 200 USDC into \(t) in \(orders) orders every \(every)", w(.jupiterDCA(asset, usdc: 200, orders: orders, every: seconds)))
            }
        }
        add("jupiter dca", "cancel Jupiter DCA \(t)", w(.jupiterDCACancel(asset)))
        guard t != "JitoSOL" else { continue }
        add("spot", "sell \(t)", w(.sell(asset, amount: nil)))
        for (a, v) in amounts { add("spot", "sell \(a) \(t)", w(.sell(asset, amount: v))) }
        for (a, v) in amounts {
            for (every, days) in [("day", 1), ("week", 7), ("3 days", 3), ("10 days", 10), ("2 weeks", 14), ("month", 30), ("2 months", 60)] {
                add("dca", "buy \(t) for \(a) USDC every \(every)", w(.schedule(asset, usdc: v, days: days)))
            }
        }
    }
    add("dca", "stop recurring buys", w(.stopSchedules))
    add("jupiter dca", "cancel Jupiter DCA", w(.jupiterDCACancel(nil)))
    add("jupiter dca", "show Jupiter DCA", w(.jupiterDCAOrders))
    add("jupiter dca", "Jupiter DCA 100 USDC into BONK in 2 orders every day", w(.jupiterDCA(.bySymbol("BONK"), usdc: 100, orders: 2, every: 86400)))
    for t in ["BONK", "WIF", "USDT", "JTO", "PYTH", "POPCAT", "RENDER", "RAY", "TRUMP"] {
        for (a, v) in amounts { add("jupiter token", "buy \(t) on solana for \(a) USDC", w(.buy(.bySymbol(t), usdc: v))) }
        add("jupiter token", "sell \(t) on solana", w(.sell(.bySymbol(t), amount: nil)))
        for (a, v) in amounts { add("jupiter token", "sell \(a) \(t) on solana", w(.sell(.bySymbol(t), amount: v))) }
    }
    for (a, v) in amounts {
        add("near", "buy NEAR for \(a) USDC", w(.buyNear(usdc: v)))
        add("near", "sell \(a) NEAR", w(.sellNear(amount: v)))
    }
    add("near", "sell NEAR", w(.sellNear(amount: nil)))
    let stocks = [("TSLA", "TSLA"), ("Tesla", "TSLA"), ("NVDA", "NVDA"), ("Nvidia", "NVDA"), ("SPCX", "SPCX"), ("SpaceX", "SPCX"), ("AAPL", "AAPL"),
                  ("Apple", "AAPL"), ("GOOGL", "GOOGL"), ("Google", "GOOGL"), ("HOOD", "HOOD"), ("META", "META"), ("Coinbase", "COIN"), ("AMZN", "AMZN"),
                  ("PLTR", "PLTR"), ("Microsoft", "MSFT")]
    for (word, symbol) in stocks {
        for (a, v) in amounts { add("stock", "buy \(word) stock for \(a) USDC", w(.buyStock(symbol: symbol, usdc: v))) }
        add("stock", "sell \(symbol) stock", w(.sellStock(symbol: symbol, amount: nil)))
        add("stock", "sell 2 \(symbol) stock", w(.sellStock(symbol: symbol, amount: 2)))
        add("stock", "sell 0.132082 \(symbol) stock", w(.sellStock(symbol: symbol, amount: d("0.132082"))))
    }
    for (a, v) in amounts {
        add("contract", "buy token \(contract) for \(a) USDC", w(.buyStock(symbol: contract, usdc: v)))
        add("mint", "buy token \(mint) for \(a) USDC", w(.buy(.byMint(mint), usdc: v)))
        for net in ["base", "arbitrum", "polygon", "ethereum"] {
            add("contract", "buy token \(contract) on \(net) for \(a) USDC", w(.buyOn(net, token: contract, usdc: v)))
        }
    }
    add("contract", "sell tokens \(contract)", w(.sellStock(symbol: contract, amount: nil)))
    add("contract", "sell 1973 tokens \(contract)", w(.sellStock(symbol: contract, amount: 1973)))
    add("mint", "sell tokens \(mint)", w(.sell(.byMint(mint), amount: nil)))
    add("mint", "sell 1000 tokens \(mint)", w(.sell(.byMint(mint), amount: 1000)))
    for net in ["base", "arbitrum", "polygon", "ethereum"] {
        add("contract", "sell tokens \(contract) on \(net)", w(.sellOn(net, token: contract, amount: nil)))
        add("contract", "sell 50 tokens \(contract) on \(net)", w(.sellOn(net, token: contract, amount: 50)))
    }
    let evmTokens = [("base", ["AERO", "DEGEN", "VIRTUAL", "cbBTC", "WETH"]), ("arbitrum", ["ARB", "GMX", "PENDLE", "LINK", "UNI"]),
                     ("polygon", ["AAVE", "LINK", "WPOL"]), ("ethereum", ["UNI", "PEPE", "LDO"])]
    for (net, tokens) in evmTokens {
        for t in tokens {
            for place in [net, net.capitalized] {
                for (a, v) in amounts { add("evm token", "buy \(t) on \(place) for \(a) USDC", w(.buyOn(net, token: t, usdc: v))) }
                for src in ["solana", "base", "arbitrum", "polygon", "ethereum", "robinhood"] {
                    add("evm token", "buy \(t) on \(place) for 5 USDC from \(src)", w(.buyOn(net, token: t, usdc: 5, from: src)))
                }
                add("evm token", "sell \(t) on \(place)", w(.sellOn(net, token: t, amount: nil)))
                add("evm token", "sell \(t) on \(place) and keep the USDC there", w(.sellOn(net, token: t, amount: nil, keep: true)))
                for (a, v) in amounts {
                    add("evm token", "sell \(a) \(t) on \(place)", w(.sellOn(net, token: t, amount: v)))
                    add("evm token", "sell \(a) \(t) on \(place) and keep the USDC there", w(.sellOn(net, token: t, amount: v, keep: true)))
                }
            }
        }
    }

    // PAYING FOR A SERVICE
    add("pay", "pay GET https://api.example.com/v1/search?q=eth", w(.pay(method: "GET", url: "https://api.example.com/v1/search?q=eth", body: nil)))
    add("pay", "pay https://api.example.com/v1/price", w(.pay(method: "GET", url: "https://api.example.com/v1/price", body: nil)))
    for body in [#"{"query": "chart ETH then buy SOL", "n": 3}"#, #"{"to": "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", "text": "every day at 09:00: hi"}"#,
                 #"[{"q": "alert me when BTC is above 120000"}]"#] {
        add("pay", "pay POST https://grok.example/x-search \(body)", w(.pay(method: "POST", url: "https://grok.example/x-search", body: body)))
    }

    // ADDRESSES AND WATCHLIST
    for address in [evmAddress, solAddress] {
        for name in ["Bybit", "Mom", "Cold wallet"] {
            add("save address", "save \(address) as \(name)", .book(.save(address: address, name: name, network: nil)))
            for net in ["solana", "base", "arbitrum", "ethereum", "tempo", "hyperliquid"] {
                add("save address", "save \(address) as \(name) on \(net)", .book(.save(address: address, name: name, network: net)))
            }
        }
    }
    for name in names {
        add("favorites", "add \(name) to favorites", .book(.favorite(name: name, on: true)))
        add("favorites", "remove \(name) from favorites", .book(.favorite(name: name, on: false)))
    }
    add("watch", "watch token \(contract)", .token(.favorite(address: contract, chain: nil)))
    add("watch", "watch token \(mint)", .token(.favorite(address: mint, chain: "solana")))
    for chain in ["robinhood", "base", "arbitrum", "ethereum"] {
        add("watch", "watch token \(contract) on \(chain)", .token(.favorite(address: contract, chain: chain)))
        add("token card", "chart token \(contract) on \(chain)", .token(.show(address: contract, chain: chain)))
    }
    add("watch", "watch token \(mint) on solana", .token(.favorite(address: mint, chain: "solana")))
    add("token card", "chart token \(mint) on solana", .token(.show(address: mint, chain: "solana")))
    for s in ["AI", "MOO", "BONK"] { add("watch", "remove token \(s) from favorites", .token(.unfavorite(symbol: s))) }

    // MARKET, P/L
    let assets = [("BTC", "BTC"), ("ETH", "ETH"), ("SOL", "SOL"), ("HYPE", "HYPE"), ("oil", "xyz:CL"), ("brent", "xyz:BRENTOIL"), ("gold", "xyz:GOLD"),
                  ("silver", "xyz:SILVER"), ("TSLA", "xyz:TSLA"), ("NVDA", "xyz:NVDA"), ("PEPE", "kPEPE"), ("BONK", "kBONK"), ("AAVE", "AAVE"),
                  ("FARTCOIN", "FARTCOIN"), ("SPCX", "xyz:SPCX"), ("SP500", "xyz:SP500"), ("brent oil", "xyz:BRENTOIL"), ("WTI", "xyz:CL"),
                  ("LINK", "LINK"), ("NEAR", "NEAR")]
    for (word, coin) in assets {
        add("chart", "chart \(word)", .chart(.chart(coin: coin, range: .day)))
        add("chart", "chart \(word) week", .chart(.chart(coin: coin, range: .week)))
        add("chart", "chart \(word) month", .chart(.chart(coin: coin, range: .month)))
        for (side, above) in [("above", true), ("below", false), ("at", nil)] as [(String, Bool?)] {
            for (p, price) in [("2680", d("2680")), ("2,680", d("2680")), ("93.5", d("93.5")), ("0.000012", d("0.000012")), ("117,000", d("117000"))] {
                add("alert", "alert me when \(word) is \(side) \(p)", .alert(.set(coin: coin, price: price, above: above)))
            }
        }
    }
    add("chart", "what's moving", .chart(.movers))
    add("alert", "cancel alerts", .alert(.clear))
    for (p, period) in [("", PnL.Period.all), (" today", .day), (" week", .week), (" month", .month), (" all", .all)] {
        add("pnl", "profit and loss\(p)", .pnl(PnLCommand(period: period, coin: nil)))
        for (word, coin) in [("ETH", "ETH"), ("oil", "xyz:CL"), ("SOL", "SOL"), ("MOO", "MOO"), ("gold", "xyz:GOLD"), ("AERO", "AERO"), ("SPCX", "xyz:SPCX")] {
            add("pnl", "profit and loss\(p) for \(word)", .pnl(PnLCommand(period: period, coin: coin)))
        }
    }

    // APP, FOLDERS
    for c in ["new canvas", "clear the plan", "lock", "create an automation wallet"] { add("app", c, .app(c)) }
    for folder in ["Research", "Ideas", "Мои идеи", "Q4 plans"] {
        add("folder", "save this canvas to folder \(folder)", .folder(.save(folder: folder)))
        for text in ["buy ETH below 2500", "chart ETH looks bullish, watch 2,700", "alert me when BTC is above 120000", "my pnl this week was bad",
                     "token \(contract) looks interesting", "long SOL then short BTC later", "every day at 09:00 check rates", "remember: rebalance on Monday",
                     "show me gold month", "cancel alerts before the Fed", "add Mom to favorites", "save \(evmAddress) as Bybit"] {
            add("folder note", "write to folder \(folder): \(text)", .folder(.note(folder: folder, text: text)))
        }
    }

    // NOW, IN STEPS
    let chainable: [(String, Read)] = [
        ("withdraw 5 USDC to Solana", w(.withdrawToSolana(5))), ("deposit 5 USDC to kamino", w(.kamino(deposit: true, symbol: "USDC", amount: 5))),
        ("move 50 USDC from Ethereum to Solana", w(.evmToSolana("ethereum", 50))), ("buy SOL for 30 USDC", w(.buy(SpotAsset.all[0], usdc: 30))),
        ("buy JUP for 20 USDC", w(.buy(SpotAsset.all[3], usdc: 20))), ("lend 300 USDC on Aave on Arbitrum", w(.aave(deposit: true, amount: 300, network: "arbitrum"))),
        ("spread 5 USDC from Solana across the best rates", w(.allocate("USDC", maxShare: nil, amount: 5, from: "solana"))),
        ("move 200 USDC from Ethereum to Base", w(.bridge(from: "ethereum", to: "base", 200))), ("deposit 200 USDC into Morpho on Base", w(.morpho(deposit: true, amount: 200))),
        ("sell SOL", w(.sell(SpotAsset.all[0], amount: nil))), ("lend USDC", w(.lendDeposit("USDC", nil))),
        ("send all RAY on solana to Bybit", w(.sendToken(name: "Bybit", token: "RAY", amount: nil, network: "solana"))),
        ("withdraw USDC from kamino", w(.kamino(deposit: false, symbol: "USDC", amount: nil))), ("chart ETH", .chart(.chart(coin: "ETH", range: .day))),
        ("move 5 USDC from Base to Robinhood", w(.bridge(from: "base", to: "robinhood", 5))), ("buy token \(contract) for 5 USDC", w(.buyStock(symbol: contract, usdc: 5))),
        ("buy AERO on base for 3 USDC", w(.buyOn("base", token: "AERO", usdc: 3))), ("withdraw 1 USDC from HLP", w(.hlp(deposit: false, amount: 1))),
    ]
    for (a, ra) in chainable {
        for (b, rb) in chainable where b != a { add("steps", "\(a) then \(b)", .steps([ra, rb])) }
    }
    for i in chainable.indices {
        let (a, ra) = chainable[i], (b, rb) = chainable[(i + 3) % chainable.count], (c, rc) = chainable[(i + 7) % chainable.count]
        add("steps", "\(a) then \(b) then \(c)", .steps([ra, rb, rc]))
    }

    // LATER OR ON REPEAT
    let heads: [(String, ScheduledTask.Every?)] = [
        ("in 3 days", nil), ("in 1 week", nil), ("in 30 minutes", nil), ("in 2 hours", nil), ("in 1 month", nil), ("at 18:30", nil), ("tomorrow", nil),
        ("tomorrow at 10:00", nil), ("on 2027-01-15", nil), ("on 2027-01-15 at 08:15", nil), ("every hour", .hours(1)), ("every 4 hours", .hours(4)),
        ("every day", .days(1)), ("every day at 09:00", .days(1)), ("every week", .days(7)), ("every 2 weeks", .days(14)), ("every month", .months(1)),
        ("every 3 months", .months(3)), ("every monday", .days(7)), ("every friday at 17:45", .days(7)), ("every month on the 1st", .months(1)),
        ("every month on the 15th at 12:00", .months(1)), ("every 3 days", .days(3)),
    ]
    let bodies: [(String, [Read])] = [
        ("rebalance USDC", [w(.rebalance(maxShare: nil))]), ("sell SOL then lend USDC", [w(.sell(SpotAsset.all[0], amount: nil)), w(.lendDeposit("USDC", nil))]),
        ("send 20 USDC to Mom", [w(.sendTo(name: "Mom", amount: 20))]), ("sweep rewards over $10 into USDC", [w(.sweepRewards(minUSD: 10, into: nil))]),
        ("buy SOL for 5 USDC", [w(.buy(SpotAsset.all[0], usdc: 5))]), ("spread USDC across the best rates", [w(.allocate("USDC", maxShare: nil))]),
        ("long ETH leverage 3x", [.plan({ var i = TradeIntent(); i.leverage = 3; return i.started }())]),
        ("move 5 USDC from Base to Solana then deposit 5 USDC to kamino", [w(.baseToSolana(5)), w(.kamino(deposit: true, symbol: "USDC", amount: 5))]),
        ("claim rewards", [w(.claimRewards)]), ("profit and loss week", [.pnl(PnLCommand(period: .week, coin: nil))]),
    ]
    for (head, every) in heads {
        for (body, steps) in bodies { add("task", "\(head): \(body)", .task(every, steps)) }
    }
    add("task", "cancel scheduled tasks", .cancelTasks)
    for (word, coin) in [("ETH", "ETH"), ("BTC", "BTC"), ("oil", "xyz:CL"), ("SOL", "SOL"), ("gold", "xyz:GOLD")] {
        for (side, above) in [("above", true), ("below", false), ("at", nil)] as [(String, Bool?)] {
            for (p, price) in [("2680", d("2680")), ("2,680", d("2680")), ("93.5", d("93.5"))] {
                for body in ["long ETH leverage 3x", "buy SOL for 5 USDC", "sell SOL then lend USDC", "close ETH", "send 20 USDC to Mom"] {
                    add("alert steps", "when \(word) is \(side) \(p): \(body)",
                        .alert(.set(coin: coin, price: price, above: above, steps: body.components(separatedBy: " then "))))
                }
            }
        }
    }

    // TRADE PLAN: random combinations of every optional part, in the form's order.
    var rng = Seeded(state: 20_261_004)
    let planAssets = [("BTC", "BTC"), ("ETH", "ETH"), ("SOL", "SOL"), ("HYPE", "HYPE"), ("oil", "xyz:CL"), ("brent", "xyz:BRENTOIL"), ("gold", "xyz:GOLD"),
                      ("silver", "xyz:SILVER"), ("TSLA", "xyz:TSLA"), ("NVDA", "xyz:NVDA"), ("PEPE", "kPEPE"), ("BONK", "kBONK"), ("kPEPE", "kPEPE"),
                      ("AAVE", "AAVE"), ("LINK", "LINK"), ("NEAR", "NEAR"), ("MOVE", "MOVE"), ("FARTCOIN", "FARTCOIN"), ("SPCX", "xyz:SPCX"),
                      ("SP500", "xyz:SP500"), ("ethereum", "ETH"), ("bitcoin", "BTC"), ("2Z", "2Z"), ("0G", "0G"), ("XYZ100", "xyz:XYZ100"),
                      ("brent oil", "xyz:BRENTOIL"), ("WTI", "xyz:CL"), ("NATGAS", "xyz:NATGAS"), ("COPPER", "xyz:COPPER"), ("JPY", "xyz:JPY"),
                      ("ME", "ME"), ("NOW", "xyz:NOW"), ("HOOD", "xyz:HOOD"), ("MSTR", "xyz:MSTR"), ("VIRTUAL", "VIRTUAL"), ("PUMP", "PUMP")]
    let money = [("5", d("5")), ("2.5", d("2.5")), ("10", d("10")), ("1,000", d("1000")), ("50", d("50")), ("0.5", d("0.5"))]
    let risks = [("0.5", d("0.5")), ("1", d("1")), ("2.5", d("2.5")), ("10", d("10")), ("0.25", d("0.25"))]
    let pcts = [("1", d("1")), ("2.5", d("2.5")), ("0.8", d("0.8")), ("3", d("3")), ("6.5", d("6.5")), ("12", d("12"))]
    let rs = [("2", d("2")), ("1.5", d("1.5")), ("3", d("3"))]
    let levs = [("2", d("2")), ("3", d("3")), ("10", d("10")), ("2.5", d("2.5")), ("20", d("20"))]
    let prices = [("2650", d("2650")), ("0.1234", d("0.1234")), ("95000", d("95000")), ("2,650", d("2650")), ("67.5", d("67.5")), ("117,250.5", d("117250.5"))]
    for _ in 0..<3_000 {
        let long = rng.chance(0.5), (word, coin) = rng.pick(planAssets)
        var e = TradeIntent()
        e.asset = coin
        e.side = long ? .long : .short
        var parts = ["\(long ? "long" : "short") \(word)"]
        if rng.chance(0.5) { let (s, v) = rng.pick(money); parts.append("\(s) USDC"); e.amountUSDC = v }
        if rng.chance(0.5) { let (s, v) = rng.pick(risks); parts.append("risk $\(s)"); e.riskUSD = v }
        if rng.chance(0.5) { let (s, v) = rng.pick(pcts); parts.append("stop \(s)%"); e.stopPct = v }
        switch rng.next() % 3 {
        case 1: let (s, v) = rng.pick(pcts); parts.append("take \(s)%"); e.takePct = v
        case 2: let (s, v) = rng.pick(rs); parts.append("take \(s)R"); e.takeR = v
        default: break
        }
        if rng.chance(0.5) { let (s, v) = rng.pick(levs); parts.append("leverage \(s)x"); e.leverage = v }
        if rng.chance(0.3) { parts.append("by the book"); e.autoRules = true }
        switch rng.next() % 3 {
        case 1: let (s, v) = rng.pick(pcts); parts.append("on a \(s)% pullback"); e.pullbackPct = v
        case 2: let (s, v) = rng.pick(prices); parts.append("limit \(s)"); e.entryPx = v
        default: break
        }
        if rng.chance(0.3) { parts.append("take in parts"); e.partialTake = true }
        if rng.chance(0.3) { let (s, v) = rng.pick(pcts); parts.append("trailing \(s)%"); e.trailingPct = v }
        add("plan", parts.joined(separator: " "), .plan(e.started))
    }

    // A change alone, to a plan already on the canvas.
    var base = TradeIntent()
    base.asset = "SOL"; base.riskUSD = 1; base.stopPct = 2
    func changed(_ f: (inout TradeIntent) -> Void) -> Read { var i = base; f(&i); return .plan(i.started) }
    var changes: [(String, Read)] = []
    for (s, v) in money { changes.append(("\(s) USDC", changed { $0.amountUSDC = v })) }
    for (s, v) in risks { changes.append(("risk $\(s)", changed { $0.riskUSD = v })) }
    for (s, v) in pcts {
        changes.append(("stop \(s)%", changed { $0.stopPct = v }))
        changes.append(("take \(s)%", changed { $0.takePct = v }))
        changes.append(("trailing \(s)%", changed { $0.trailingPct = v }))
        changes.append(("on a \(s)% pullback", changed { $0.pullbackPct = v }))
    }
    for (s, v) in rs { changes.append(("take \(s)R", changed { $0.takeR = v })) }
    for (s, v) in levs { changes.append(("leverage \(s)x", changed { $0.leverage = v })) }
    for (s, v) in prices { changes.append(("limit \(s)", changed { $0.entryPx = v })) }
    changes += [("at market", changed { _ in }), ("by the book", changed { $0.autoRules = true }), ("take in parts", changed { $0.partialTake = true })]
    for (c, r) in changes { add("plan change", c, r, plan: base) }
    for (s, v) in money {
        for (l, x) in levs {
            let r = changed { $0.amountUSDC = v; $0.leverage = x }
            add("plan change", "\(s) USDC leverage \(l)x", r, plan: base)
            add("plan change", "\(s) USDC then leverage \(l)x", r, plan: base)
        }
    }
    add("take choice", "take profit options", .takeChoice, plan: base)

    // OPEN POSITION
    for c in ["close", "close ETH", "close oil", "close BTC"] { add("position", c, .position(.close(1)), position: true) }
    for c in ["close half", "close half ETH", "close half of BTC"] { add("position", c, .position(.close(d("0.5"))), position: true) }
    for (c, f) in [("close 25%", d("0.25")), ("close 25% ETH", d("0.25")), ("close 33.3% of SOL", d("0.333")), ("close 100%", 1), ("close 10% oil", d("0.1"))] {
        add("position", c, .position(.close(f)), position: true)
    }
    for c in ["move stop to breakeven", "move stop to breakeven on ETH"] { add("position", c, .position(.stopToBreakeven), position: true) }
    for c in ["cancel orders", "cancel orders on ETH", "cancel orders ETH"] { add("position", c, .position(.cancelOrders), position: true) }
    for (p, v) in prices {
        add("position", "stop at \(p)", .position(.stopTo(v)), position: true)
        add("position", "take at \(p)", .position(.takeTo(v)), position: true)
        add("position", "stop at \(p) on ETH", .position(.stopTo(v)), position: true)
    }
    for (p, v) in pcts {
        add("position", "trail \(p)%", .position(.trail(v)), position: true)
        add("position", "trail \(p)% on ETH", .position(.trail(v)), position: true)
    }
    return out
}

/// The same command as a model may write it: capitalized, or with a full stop.
func variants(_ c: FormCase) -> [FormCase] {
    let capital = c.command.prefix(1).uppercased() + c.command.dropFirst()
    var out = [c, FormCase(family: c.family, command: capital, expected: c.expected, plan: c.plan, positionOpen: c.positionOpen)]
    if !["pay", "folder note"].contains(c.family) {
        // An alert keeps its steps as written, the full stop with the last one.
        var expected = c.expected
        if case .alert(.set(let coin, let price, let above, let steps)) = expected, let last = steps.last {
            expected = .alert(.set(coin: coin, price: price, above: above, steps: steps.dropLast() + [last + "."]))
        }
        out.append(FormCase(family: c.family + " .", command: c.command + ".", expected: expected, plan: c.plan, positionOpen: c.positionOpen))
    }
    return out
}

/// Every 25th command by default (Swift's regexes make the whole matrix take minutes); `FORMS_MATRIX=full swift test
/// --filter FormsMatrix` reads them all.
private let sampled: [FormCase] = {
    let all = formCases().flatMap(variants)
    return ProcessInfo.processInfo.environment["FORMS_MATRIX"] == "full" ? all : all.enumerated().filter { $0.offset % 25 == 0 }.map(\.element)
}()

@Test func everyFormReadsAsItSays() {
    let cases = sampled
    var failed: [String: [String]] = [:]
    for c in cases {
        let got = read(c.command, plan: c.plan, positionOpen: c.positionOpen)
        if got != c.expected { failed[c.family, default: []].append("«\(c.command)»\n      got      \(got.shown)\n      expected \(c.expected.shown)") }
    }
    let total = failed.values.reduce(0) { $0 + $1.count }
    print("forms matrix: \(cases.count) commands, \(total) misread")
    for (family, list) in failed.sorted(by: { $0.key < $1.key }) {
        print("  \(family): \(list.count)")
        for line in list.prefix(4) { print("    " + line) }
    }
    #expect(total == 0)
}

/// Every form ChatRouter.parses accepts, so the router never sends one back to the model as «can't run».
@Test func everyFormParsesForTheRouter() {
    let rejected = sampled.filter { $0.family != "take choice" && !ChatRouter.parses($0.command, markets: markets) }.map(\.command)
    if !rejected.isEmpty { print("parses() rejects \(rejected.count): \(rejected.prefix(20))") }
    #expect(rejected.isEmpty)
}

/// A plan written back in its own form (`TradeIntent.command`, what a price alert tied to a plan runs) reads back to the
/// same plan; a token transfer's summary reads back to the same transfer.
@Test func formsReadBackToWhatWroteThem() {
    var wrong: [String] = []
    for c in sampled {
        switch c.expected {
        case .plan(let p) where c.plan == TradeIntent():
            let back = IntentParser.apply(p.command, to: TradeIntent(), markets: markets)?.started
            if back != p { wrong.append("«\(p.command)» → \(back?.command ?? "nothing")") }
        case .wallet(let w):
            if case .sendToken = w, WalletCommand.parse(w.summary) != w { wrong.append("«\(w.summary)» → \(String(describing: WalletCommand.parse(w.summary)))") }
        default: break
        }
    }
    if !wrong.isEmpty { print("read back wrong \(wrong.count): \(wrong.prefix(10))") }
    #expect(wrong.isEmpty)
}

extension Seeded: RandomNumberGenerator {}

/// The instant yes and no, without the model: any mix of yes-words and fillers signs; the same with a change, a number,
/// a question or a «not» in it never does; any mix of no-words drops the card.
@Test func yesAndNoAreWholeMessages() {
    var rng = Seeded(state: 1004)
    let yes = ChatWords.yes.sorted(), filler = ChatWords.filler.sorted()
    let no = ChatWords.no.subtracting(["never"]).sorted(), noFiller = ChatWords.noFiller.subtracting(["не", "no"]).sorted()
    let changes = ["на 30", "30", "но меньше", "only half", "instead", "on arbitrum", "без kamino", "потом", "later", "wait", "подожди", "?", "не",
                   "don't", "but", "только", "половину", "ещё 300", "another 300", "to kamino", "a bit less", "хм", "maybe", "может", "двадцать",
                   "twice", "в aave", "сначала", "first", "tomorrow", "завтра", "x3", "50%", "or", "или", "not yet", "ещё нет", "right"]
    var wrong: [String] = []
    for _ in 0..<5_000 {
        var words = (0..<Int(rng.next() % 3 + 1)).map { _ in rng.pick(yes) } + (0..<Int(rng.next() % 3)).map { _ in rng.pick(filler) }
        words.shuffle(using: &rng)
        let text = words.joined(separator: rng.pick([" ", ", ", " "])) + rng.pick(["", "!", ".", " 👍"])
        let line = " " + words.joined(separator: " ") + " "
        if !["да ну", "ну да", "let it go"].contains(where: { line.contains(" \($0) ") }), !ChatWords.confirms(text) || ChatWords.declines(text) {
            wrong.append("a yes not taken: «\(text)»")
        }
        words.insert(rng.pick(changes), at: Int(rng.next() % UInt64(words.count + 1)))
        let changed = words.joined(separator: " ")
        if ChatWords.confirms(changed) { wrong.append("signs a change: «\(changed)»") }
        var nos = (0..<Int(rng.next() % 2 + 1)).map { _ in rng.pick(no) } + (0..<Int(rng.next() % 3)).map { _ in rng.pick(noFiller) }
        nos.shuffle(using: &rng)
        let refusal = nos.joined(separator: rng.pick([" ", ", "])) + rng.pick(["", "!", "."])
        if !ChatWords.declines(refusal) || ChatWords.confirms(refusal) { wrong.append("a no not taken: «\(refusal)»") }
    }
    for sarcasm in ["yeah right", "ну да, конечно", "да ну его"] where ChatWords.confirms(sarcasm) { wrong.append("signs «\(sarcasm)»") }
    if !wrong.isEmpty { print("yes/no wrong \(wrong.count): \(Array(Set(wrong)).prefix(15))") }
    #expect(wrong.isEmpty)
}
