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
