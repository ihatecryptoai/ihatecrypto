@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Wave 12's rail part: a canvas's name over its newest move's title over its own, an empty rename giving the title
/// back, the open canvas removed giving way to a blank one, one new wallet at a time, Jupiter's platforms in dollars,
/// a stock's day off Hyperliquid's market, and the gas offer reading Hyperliquid's perps money. In-process, state set
/// by hand; no addresses.
@MainActor @Suite(.serialized) struct Rail13Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("rail13-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    @Test func aCanvasShowsItsNameThenItsMoveThenItsTitle() {
        var c = CanvasDoc(title: "buy some SOL")
        #expect(Engine.shownTitle(c, boards: []) == "buy some SOL")
        let move = Board(kind: "move", title: "Move 5 USDC to Base")
        #expect(Engine.shownTitle(c, boards: [Board(kind: "result", title: "x"), move]) == "Move 5 USDC to Base")
        c.name = "Rent"
        #expect(Engine.shownTitle(c, boards: [move]) == "Rent")
        c.name = ""
        #expect(Engine.shownTitle(c, boards: []) == "buy some SOL")
    }

    @Test func anEmptyRenameGivesTheTitleBack() throws {
        try engine.newCanvas()
        let id = engine.canvasID
        engine.renameCanvas(id, "  Rent  ")
        #expect(engine.canvases.first { $0.id == id }?.name == "Rent")
        #expect(engine.canvasRows.first { $0.id == id }?.title == "Rent")
        engine.renameCanvas(id, "   ")
        #expect(engine.canvases.first { $0.id == id }?.name == nil)
        #expect(engine.canvasRows.first { $0.id == id }?.title == engine.canvases.first { $0.id == id }?.title)
    }

    @Test func removingTheOpenCanvasOpensABlankOne() throws {
        try engine.newCanvas()
        let other = engine.canvasID
        try engine.newCanvas()
        let open = engine.canvasID
        try engine.removeCanvas(open)
        #expect(!engine.canvases.contains { $0.id == open })
        // Not the other one: a new blank canvas, as the Mac's deleteCanvas.
        #expect(engine.canvasID != other && engine.canvases.first?.id == engine.canvasID && engine.boards.isEmpty)
        // Another canvas removed leaves the open one open.
        let now = engine.canvasID
        try engine.removeCanvas(other)
        #expect(engine.canvasID == now && !engine.canvases.contains { $0.id == other })
    }

    @Test func noNewWalletOverWordsNotWrittenDown() async {
        engine.phrase = Array(repeating: "abandon", count: 11) + ["about"]
        defer { engine.phrase = nil }
        await #expect(throws: (any Error).self) { try await engine.createWallet(test: true) }
        #expect(engine.phrase?.last == "about")
    }

    @Test func jupitersPlatformsAreInDollarsOnTheirSite() {
        let rows = Engine.jupiterRows([(fetcher: "jupiter-staked", platform: "jup", name: "Staked JUP", label: nil, value: 123.456, apy: nil),
                                       (fetcher: "jupiter-exchange-earn", platform: nil, name: nil, label: nil, value: 5, apy: nil)])
        #expect(rows.count == 1)
        #expect(rows.first?.amount == "$123.46" && rows.first?.usd == Decimal(string: "123.46") && rows.first?.site == "jup.ag", "\(rows)")
    }

    @Test func aStocksDayComesFromItsHyperliquidMarket() {
        let markets = ["xyz:TSLA": Market(name: "xyz:TSLA", markPx: 110, szDecimals: 2, maxLeverage: 10, prevDayPx: 100, dex: "xyz")]
        #expect(Engine.stockDay("TSLA", markets) == 10)
        #expect(Engine.stockDay("tsla", markets) == 10)
        #expect(Engine.stockDay("AAPL", markets) == nil)
    }

    @Test func hyperliquidsPerpsMoneyBringsTheGas() {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: [
            .init(key: "base", name: "Base", usd: 5, rows: [Engine.row("USDC", 5, usd: 5)]),
            .init(key: "hyperliquid", name: "Hyperliquid", usd: 10, rows: [Engine.row("USDC", 10, usd: 10)]),
        ], updatedAt: Date())
        // Spot USDC alone brings no gas (the Mac reads perps withdrawable): Base's dollars are stuck.
        engine.placeFacts.hyperliquid = nil
        #expect(engine.stuckDollars.map(\.key) == ["base"])
        engine.placeFacts.hyperliquid = .init(accountValue: 10, withdrawable: 10, spotUSDC: 0, dexes: [], hlpEquity: 0)
        #expect(engine.stuckDollars.isEmpty)
        engine.placeFacts.hyperliquid = nil
        engine.portfolio = PortfolioView()
    }
}
