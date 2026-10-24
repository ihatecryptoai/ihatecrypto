@testable import DesktopEngine
import Foundation
import Testing
@testable import TradeExecution

/// Wave 6's portfolio rows (audit5 G5, G13): what's lent keeps its last reading with its rate, and a Stock Token is read
/// at Robinhood's bid. Pure: the reads are handed in, nothing goes to the network.
@Suite(.serialized) struct Ui7Tests {
    // Hardhat's public account #0.
    static let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

    @Test func whatsLentKeepsItsLastReadingAndItsRate() async {
        let place = "Aave · Arbitrum test-\(UUID().uuidString)"
        var rateReads = 0
        let start = Date()
        // Read: 25 USDC at 4.123% a year; off Base it opens Aave's site.
        let first = await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { 25_000_000 }, apy: { rateReads += 1; return Decimal(string: "4.123")! }, now: start)
        #expect(first?.usd == 25 && first?.note == "earning 4.12%" && first?.site == "app.aave.com" && first?.section == "defi", "\(String(describing: first))")
        // The node doesn't answer: the last reading stays, the rate isn't asked again.
        let silent = await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { nil }, apy: { rateReads += 1; return 9 }, now: start.addingTimeInterval(60))
        #expect(silent == first && rateReads == 1)
        // A new reading within 10 minutes keeps the rate; after them the rate is read again.
        let later = await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { 30_000_000 }, apy: { rateReads += 1; return 5 }, now: start.addingTimeInterval(120))
        #expect(later?.usd == 30 && later?.note == "earning 4.12%" && rateReads == 1)
        let stale = await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { 30_000_000 }, apy: { rateReads += 1; return 5 }, now: start.addingTimeInterval(800))
        #expect(stale?.note == "earning 5%" && rateReads == 2, "\(String(describing: stale?.note))")
        // Withdrawn: gone, and a silent node afterwards doesn't bring it back.
        #expect(await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { 0 }, apy: { 5 }) == nil)
        #expect(await Engine.lent("Aave", place: place, owner: Self.me, onBase: false, read: { nil }, apy: { 5 }) == nil)
        // On Base the row opens its details board, not a site.
        let base = await Engine.lent("Morpho", place: "Morpho · Base test-\(UUID().uuidString)", owner: Self.me, onBase: true, read: { 10_000_000 }, apy: { nil })
        #expect(base?.site == nil && base?.note == "earning")
    }

    @Test func aStockTokenIsReadAtRobinhoodsBid() {
        let tsla = RobinhoodStocks.Token(symbol: "TSLA", name: "Tesla", address: "0x00000000000000000000000000000000000000a1", decimals: 18, logo: nil)
        var explorer = Engine.row("TSLA", 2, usd: 3, chain: "robinhood", contract: tsla.address)
        explorer.id = "robinhood:" + tsla.address
        explorer.real = false
        let r = Engine.stockRow(tsla, amount: 2, bid: 250, explorer: explorer)
        // The bid's value, kept on show (the registry vouches for it), hidden or shown by the explorer row's own id.
        #expect(r.usd == 500 && r.real == true && r.stock == true && r.verified == true && r.id == explorer.id)
        // No quote from Robinhood: the explorer's price stays.
        #expect(Engine.stockRow(tsla, amount: 2, bid: nil, explorer: explorer).usd == 3)
        // Bought here, not listed yet: no id, so it can't be tucked away.
        #expect(Engine.stockRow(tsla, amount: 1, bid: 250, explorer: nil).id == nil)
    }

    @Test func theEngineSaysWhenItWasBuilt() {
        #expect(Engine.build?.wholeMatch(of: /\d{8}\.\d{4}/) != nil, "\(Engine.build ?? "nil")")
    }
}
