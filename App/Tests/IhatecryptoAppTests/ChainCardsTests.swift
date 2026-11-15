@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import Testing

// «all» alone never stands on a card, and a chain's later cards say which step they are and what they sell (the
// owner, 2026-11-14: «Swap all SPCX → USDG on Robinhood» over four cards that each read «USDG»).

private func step(_ label: String, _ title: String, _ lines: [String] = []) -> ActivityBubble {
    var s = ActivityBubble(kind: "step", label: label, title: title)
    s.lines = lines
    return s
}

@Test func allOfATokenIsItsAmountOnTheCard() {
    // Robinhood's stock tokens are kept apart from the other holdings, and «all SPCX» there is that amount: looked for
    // only among the holdings, the card said a bare «all».
    let stock = StockHolding(symbol: "SPCX", name: "SpaceX", address: "0xAAaa000000000000000000000000000000000001", amount: Decimal(string: "0.036312")!, price: nil, logo: nil)
    let wallet = [Holding(chain: "solana", contract: "SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb", amount: Decimal(string: "0.01")!, symbol: "SPCX"),
                  Holding(chain: "solana", contract: "Dust111111111111111111111111111111111111111", amount: 0, symbol: "DUST")]
    let held = Workspace.heldTokens(wallet, stocks: [stock])
    #expect(Workspace.held("spcx", place: "robinhood", among: held).map { $0.0 } == Decimal(string: "0.036312"))
    #expect(Workspace.held("SPCX", place: "robinhood", among: Workspace.heldTokens(wallet, stocks: [])) == nil)
    #expect(Workspace.held("SPCX", place: "solana", among: held).map { $0.0 } == Decimal(string: "0.01"))
    // Two different tokens of one ticker and no place named, or nothing of it held: «all» can't be told.
    #expect(Workspace.held("SPCX", place: nil, among: held) == nil)
    #expect(Workspace.held("DUST", place: "solana", among: held) == nil && Workspace.held("TSLA", place: "robinhood", among: held) == nil)
    // One token listed twice, its contract written in another case, is one token.
    let twice = held + [(chain: "robinhood", contract: "0xaaaa000000000000000000000000000000000001", amount: Decimal(string: "0.036312")!, symbol: "SPCX")]
    #expect(Workspace.held("SPCX", place: "robinhood", among: twice).map { $0.0 } == Decimal(string: "0.036312"))

    // The title and the approve say the amount the first card found, not «all», in whatever case the ticker was written.
    let route = MoveRoute(title: "Swap all SPCX → USDG on Robinhood", footer: "",
                          steps: [step("FROM · ROBINHOOD", "0.036312 SPCX"), step("APPROVE · EXACT", "all SPCX"), step("SWAP · UNISWAP", "USDG")])
    let shown = Workspace.withHeld(route)
    #expect(shown.title == "Swap 0.036312 SPCX → USDG on Robinhood")
    #expect(shown.steps.map(\.title) == ["0.036312 SPCX", "0.036312 SPCX", "USDG"])
    let cased = MoveRoute(title: "Swap all CBBTC on Base → ETH on Arbitrum", footer: "", steps: [step("FROM · BASE", "0.001 cbBTC")])
    #expect(Workspace.withHeld(cased).title == "Swap 0.001 cbBTC on Base → ETH on Arbitrum")
    // Nothing found held, or a ticker that only has a digit in it: the route stays as it was drawn.
    for bare in ["all SPCX", "all USDT0"] {
        let unread = MoveRoute(title: "Swap \(bare) → USDG on Robinhood", footer: "", steps: [step("FROM · ROBINHOOD", bare)])
        #expect(Workspace.withHeld(unread).title == unread.title)
    }
    #expect(Workspace.saysAmount("0.5 TSLA") && Workspace.saysAmount("≈ 12 USDC") && Workspace.saysAmount("1,234.5 USDC"))
    #expect(!Workspace.saysAmount("USDT0") && !Workspace.saysAmount("1INCH → ETH") && !Workspace.saysAmount("all SPCX") && !Workspace.saysAmount("comparing…"))
}

/// A step that moves «all» of a network's dollars after sales into them is drawn by what will be there at its turn:
/// each sale adds what it sells at today's price, «≈». A token sold twice is counted once, and one with no price turns
/// the figure into «≥» today's balance.
@Test func aSaleBeforeAddsToTheDollarsMovedAfter() {
    // «sell 0.3 TSLA, then sell all TSLA» with 0.6 held sells 0.3 and then the 0.3 that's left, never 0.9.
    #expect(Workspace.selling(Decimal(string: "0.3")!, held: Decimal(string: "0.6")!, sold: 0) == Decimal(string: "0.3"))
    #expect(Workspace.selling(nil, held: Decimal(string: "0.6")!, sold: Decimal(string: "0.3")!) == Decimal(string: "0.3"))
    #expect(Workspace.selling(5, held: 2, sold: 0) == 2 && Workspace.selling(nil, held: 2, sold: 3) == 0)

    let bare = Workspace.dollarsAtTurn(held: 5, ahead: DollarsAhead())
    #expect(bare.amount == 5 && bare.mark.isEmpty)
    let more = Workspace.dollarsAtTurn(held: 5, ahead: DollarsAhead(by: Decimal(string: "17.9")!))
    #expect(more.amount == Decimal(string: "22.9") && more.mark == "≈ ")
    // A swap out of the dollars before it takes them off, never under zero.
    #expect(Workspace.dollarsAtTurn(held: 5, ahead: DollarsAhead(by: -8)).amount == 0)
    // A sale of a token no price tells: at least what's there now.
    let unknown = Workspace.dollarsAtTurn(held: 5, ahead: DollarsAhead(by: 3, unpriced: true))
    #expect(unknown.amount == 8 && unknown.mark == "≥ ")
    // A balance that isn't read stays unread.
    #expect(Workspace.dollarsAtTurn(held: nil, ahead: DollarsAhead(by: 3)).amount == nil)

    // The network's dollar: USDC everywhere, USDG by either name on Robinhood Chain only.
    #expect(Workspace.isNetworkDollar("usdc", on: "base") && Workspace.isNetworkDollar("USDG", on: "robinhood") && Workspace.isNetworkDollar("USDC", on: "robinhood"))
    #expect(!Workspace.isNetworkDollar("USDG", on: "solana") && !Workspace.isNetworkDollar("USDT", on: "base") && !Workspace.isNetworkDollar("SPCX", on: "robinhood"))
}

/// The steps before a card, counted one after another: what each adds to a network's dollars or takes off them.
@Test @MainActor func aChainsStepsAreCountedIntoTheDollarsAhead() {
    let d = { (s: String) in Decimal(string: s)! }
    // Arbitrum holds 60 USDC and 0.002 ETH at 3000, Robinhood 5 USDG, 0.6 TSLA at 400 and a token no price tells.
    func chain(_ usdc: [String: Decimal] = ["arbitrum": 60, "base": 5, "robinhood": 5, "solana": 100]) -> ChainDollars {
        ChainDollars(usdc: { usdc[$0] }, held: { token, place in
            switch (token.uppercased(), place) {
            case ("ETH", "arbitrum"): ("ETH", d("0.002"), 3000)
            case ("TSLA", "robinhood"), ("0XT5LA", "robinhood"): ("0xT5LA", d("0.6"), 400)
            case ("XYZ", "robinhood"): ("0xXYZ", 10, nil)
            case ("TSLAON", "solana"): ("TsLaMint", 1, 400)
            default: nil
            }
        }, sendsOn: { $0 == "Bybit" ? "arbitrum" : nil })
    }
    // A sale into the network's dollar adds it at today's price, and the quote of the card that waits wins over it.
    var c = chain()
    c.count(.swapOn("arbitrum", from: "ETH", to: "USDC", amount: nil))
    #expect(c.ahead["arbitrum"] == 6 && c.unpriced.isEmpty)
    c = chain()
    c.count(.swapOn("arbitrum", from: "ETH", to: "USDC", amount: nil), quote: d("5.38"))
    #expect(c.ahead["arbitrum"] == d("5.38"))
    // A quote counts for a token the holdings don't show yet.
    c = chain()
    c.count(.sellOn("base", token: "AERO", amount: 3, keep: true), quote: 2)
    #expect(c.ahead["base"] == 2 && c.unpriced.isEmpty)
    // A token sold twice is counted once: 0.3, then the 0.3 that's left.
    c = chain()
    c.count(.sellStock(symbol: "TSLA", amount: d("0.3")))
    c.count(.sellStock(symbol: "TSLA", amount: nil))
    #expect(c.ahead["robinhood"] == 240)
    // Named by its ticker and then by its contract, it is the same token.
    c = chain()
    c.count(.sellStock(symbol: "TSLA", amount: d("0.3")))
    c.count(.sellStock(symbol: "0xT5LA", amount: nil))
    #expect(c.ahead["robinhood"] == 240)
    // What spends the dollars is taken off: a swap out of them, a buy paid there, a deposit as far as the network pays.
    c = chain()
    c.count(.swapOn("arbitrum", from: "USDC", to: "ETH", amount: 10))
    c.count(.buyOn("arbitrum", token: "ARB", usdc: 20, from: "arbitrum"))
    c.count(.aave(deposit: true, amount: 100, network: "arbitrum"))
    #expect(c.ahead["arbitrum"] == -60 && c.there("arbitrum") == 0)
    // A token bought with no network named is paid there only when the network holds it all; a stock takes what is there.
    c = chain()
    c.count(.buyOn("base", token: "AERO", usdc: 50))
    #expect(c.ahead["base"] == nil)
    c.count(.buyOn("base", token: "AERO", usdc: 4))
    c.count(.buyStock(symbol: "NVDA", usdc: 100))
    #expect(c.ahead["base"] == -4 && c.ahead["robinhood"] == -5)
    // The owner's chain: a sale, a buy with what it brings, then «all» of what's left.
    c = chain()
    c.count(.sellStock(symbol: "TSLA", amount: d("0.3")))
    c.count(.buyStock(symbol: "NVDA", usdc: 100))
    #expect(c.there("robinhood") == 25)
    // A Solana sale by a ticker the app hasn't looked up is priced by the wallet's token of that ticker.
    c = chain()
    c.count(.sell(.bySymbol("TSLAon"), amount: nil))
    #expect(c.ahead["solana"] == 400 && c.unpriced.isEmpty)
    // An amount moved is never more than will be there, and lands as that.
    c = chain()
    c.count(.bridge(from: "base", to: "arbitrum", 20))
    #expect(c.ahead["base"] == -5 && c.ahead["arbitrum"] == 5)
    // Dollars no price tells follow «all» of them to where it lands, and none are left behind.
    c = chain()
    c.count(.sellStock(symbol: "XYZ", amount: nil))
    #expect(c.unpriced == ["robinhood"])
    c.count(.bridge(from: "robinhood", to: "base", nil))
    #expect(c.unpriced == ["base"] && c.ahead["base"] == 5 && c.there("robinhood") == 0)

    // A burn through Circle lands as a bridge does. A loan and a vault bring dollars, a transfer takes them.
    c = chain()
    c.count(.cctp(from: "base", to: "arbitrum", 5))
    c.count(.aaveBorrow(20, network: "arbitrum"))
    c.count(.sendTo(name: "Bybit", amount: 10))
    c.count(.lendWithdraw("USDC", 7))
    #expect(c.ahead["base"] == -5 && c.ahead["arbitrum"] == 15 && c.ahead["solana"] == 7 && c.unpriced.isEmpty)
    c.count(.aave(deposit: false, amount: nil, network: "base"))
    #expect(c.unpriced == ["solana"])

    // The cards kept at Confirm are shown for what's left of that chain only.
    #expect(Workspace.kept(["b", "c"], of: ["sell", "move"], for: ["move"]) == ["c"])
    #expect(Workspace.kept(["b", "c"], of: ["sell", "move"], for: ["sell", "move"]) == ["b", "c"])
    #expect(Workspace.kept(["b", "c"], of: ["sell", "move"], for: []) == nil)
    #expect(Workspace.kept(["b", "c"], of: ["sell", "move"], for: ["sell"]) == nil)
    #expect(Workspace.kept(["b", "c"], of: ["sell", "move"], for: ["buy", "sell", "move"]) == nil)

    // The figure on a later card: «all» is what will be there, an amount is cut to it and marked then.
    let ahead = DollarsAhead(by: d("0.46"))
    #expect(Workspace.movingAtTurn(nil, held: 5, ahead: ahead) == (d("5.46"), "≈ "))
    #expect(Workspace.movingAtTurn(20, held: 5, ahead: ahead) == (d("5.46"), "≈ "))
    #expect(Workspace.movingAtTurn(3, held: 5, ahead: ahead) == (3, ""))
    // After a sale no price tells, an amount stays as asked, and «all» is «at least».
    #expect(Workspace.movingAtTurn(20, held: 3, ahead: DollarsAhead(unpriced: true)) == (20, ""))
    #expect(Workspace.movingAtTurn(nil, held: 3, ahead: DollarsAhead(unpriced: true)) == (3, "≥ "))
    #expect(Workspace.landsMark("≥ ") == "≥ " && Workspace.landsMark("") == "≈ " && Workspace.landsMark("≈ ") == "≈ ")
    // A sale and a buy that come to nothing more are an estimate all the same.
    #expect(Workspace.dollarsAtTurn(held: 5, ahead: DollarsAhead(counted: true)).mark == "≈ ")
}

@Test func aChainsLaterCardSaysWhatItSells() {
    // A later swap keeps only its swap card: that card names what goes in too.
    let swap = Workspace.laterCards([step("FROM · ROBINHOOD", "0.5 TSLA", ["on Robinhood"]), step("APPROVE · EXACT", "0.5 TSLA"), step("SWAP · UNISWAP", "USDG")], brought: false)
    #expect(swap.map(\.title) == ["0.5 TSLA → USDG"] && swap.allSatisfy { $0.status == "then" } && swap[0].lines.isEmpty)
    // «All» of a token later in a chain is what's there at its turn (a step before may have bought more): said on the card.
    let all = Workspace.laterCards([step("FROM · BASE", "3 AERO", ["all available · on Base"]), step("SWAP · UNISWAP", "ETH")], brought: false)
    #expect(all.map(\.title) == ["3 AERO → ETH"] && all[0].lines.first == "all available when its turn comes · today's amount")
    // A ticker with a digit in it is no amount: its card takes what goes in too.
    #expect(Workspace.laterCards([step("FROM · HYPEREVM", "5 USDC"), step("SWAP · UNISWAP", "USDT0")], brought: false).map(\.title) == ["5 USDC → USDT0"])
    // Only the card that waits is quoted: a later one doesn't say «quoting…» for ever.
    let tempo = Workspace.laterCards([step("FROM · TEMPO", "0.1 USDC"), step("SWAP · TEMPO'S EXCHANGE", "pathUSD", ["quoting…"])], brought: false)
    #expect(tempo.map(\.title) == ["0.1 USDC → pathUSD"] && tempo[0].lines == ["quoted when its turn comes"])
    // A last card that only names the coin landing in this wallet, where the money started, folds into the first.
    let sale = Workspace.laterCards([step("SELL · JUPITER", "1 USDT"), step("TO · SOLANA", "USDC", ["your wallet"])], brought: false)
    #expect(sale.map(\.title) == ["1 USDT → USDC"])
    let pair = Workspace.laterCards([step("FROM · SOLANA", "1 USDT"), step("SWAP · JUPITER", "USDC"), step("TO · SOLANA", "USDC", ["your wallet ✓"])], brought: false)
    #expect(pair.map(\.title) == ["1 USDT → USDC"])
    // A landing on another network keeps its card, and a way still being compared isn't given an amount.
    let across = Workspace.laterCards([step("FROM · SOLANA", "0.3 USDC"), step("SWAP ACROSS · RELAY", "ETH"), step("TO · ARBITRUM", "ETH", ["your own address there"])], brought: false)
    #expect(across.map(\.title) == ["0.3 USDC → ETH", "ETH"] && across.map(\.label) == ["SWAP ACROSS · RELAY", "TO · ARBITRUM"])
    let back = Workspace.laterCards([step("FROM · BASE", "0.5 AERO"), step("RELAY · THE WAY", "comparing…"), step("TO · SOLANA", "USDC", ["your wallet"])], brought: false)
    #expect(back.map(\.title) == ["comparing…", "USDC"])
    // Money a step before brought there: its bridge isn't drawn again.
    let brought = Workspace.laterCards([step("FROM · BASE", "5 USDC"), step("BRIDGE · RELAY", "5 USDC"), step("DEPOSIT · AAVE", "5 USDC")], brought: true)
    #expect(brought.map(\.label) == ["DEPOSIT · AAVE"])
}

/// A buy or a deposit later in a chain is paid by what the steps before leave on its network: a sale before it covers
/// it and no bridge is drawn, a move out before it leaves it short (the owner's pick, 2026-11-15).
@Test @MainActor func aLaterBuyIsPaidByWhatTheStepsBeforeLeave() {
    var chain = ChainDollars(usdc: { ["robinhood": 0, "arbitrum": 5][$0] }, held: { token, _ in token == "TSLA" ? ("tsla", 1, 400) : nil })
    chain.count(.sellStock(symbol: "TSLA", amount: Decimal(string: "0.3")))
    // 0.3 TSLA at $400 bring $120: the $100 buy after it is paid there.
    var (there, later) = Workspace.dollarsThere(0, ahead: chain.ahead["robinhood"])
    #expect(there == 120 && later)
    // Nothing before it touches the network: today's balance, said as today's.
    (there, later) = Workspace.dollarsThere(5, ahead: chain.ahead["arbitrum"])
    #expect(there == 5 && !later)
    // A move out before it: what's left, never below nothing.
    chain.count(.bridge(from: "arbitrum", to: "base", nil))
    (there, later) = Workspace.dollarsThere(5, ahead: chain.ahead["arbitrum"])
    #expect(there == 0 && later)
    #expect(Workspace.dollarsThere(nil, ahead: -3).there == 0)
    // A sale of the network's own coin may leave too little gas for the step after it: a buy or a deposit there isn't
    // counted as paid by it, a «move all» still counts it.
    var gas = ChainDollars(usdc: { ["polygon": 1, "arbitrum": 1][$0] }, held: { token, _ in token == "POL" ? ("pol", 100, 1) : token == "ARB" ? ("arb", 10, 1) : nil })
    gas.count(.swapOn("polygon", from: "POL", to: "USDC", amount: nil))
    gas.count(.swapOn("arbitrum", from: "ARB", to: "USDC", amount: nil))
    #expect(gas.ahead["polygon"] == 100 && gas.paying["polygon"] == nil && gas.paying["arbitrum"] == 10)
    // An amount that leaves gas for the step after it pays for that step. One that leaves under twice the floor (0.3 POL) doesn't.
    for (amount, pays) in [(Decimal(50), true), (Decimal(string: "99.5")!, false)] {
        var part = ChainDollars(usdc: { _ in 1 }, held: { token, _ in token == "POL" ? ("pol", 100, 1) : nil })
        part.count(.swapOn("polygon", from: "POL", to: "USDC", amount: amount))
        #expect((part.paying["polygon"] != nil) == pays, "\(amount)")
    }
}
