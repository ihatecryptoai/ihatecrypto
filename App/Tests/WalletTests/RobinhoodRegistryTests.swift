import Foundation
import Testing
import IhatecryptoCore
@testable import TradeExecution

/// The registry's mark goes by the contract on Robinhood Chain, never by the ticker: a copy under «NVDA» isn't the stock.
@Test func robinhoodRegistryMarksByContractOnRobinhoodChain() {
    let json = """
    {"assets": [
      {"tokenSymbol": "NVDA", "tokenName": "NVIDIA • Robinhood Token", "status": "ASSET_STATUS_ACTIVE", "tokenDecimals": 18,
       "deployments": [{"contractAddress": "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC", "chainId": 4663}]},
      {"tokenSymbol": "SPCX", "tokenName": "Space Exploration Technologies • Robinhood Token", "status": "ASSET_STATUS_ACTIVE",
       "deployments": [{"contractAddress": "0x4a0E65A3EcceC6dBe60AE065F2e7bb85Fae35eEa", "chainId": 4663}]},
      {"tokenSymbol": "OLD", "status": "ASSET_STATUS_INACTIVE",
       "deployments": [{"contractAddress": "0x1111111111111111111111111111111111111111", "chainId": 4663}]},
      {"tokenSymbol": "ELSE", "status": "ASSET_STATUS_ACTIVE",
       "deployments": [{"contractAddress": "0x2222222222222222222222222222222222222222", "chainId": 1}]}
    ]}
    """
    let registry = RobinhoodStocks.parse(Data(json.utf8))
    #expect(registry.map(\.symbol) == ["NVDA", "SPCX"])

    let listed = { (address: String, symbol: String, chain: String) in RobinhoodStocks.listed(address, symbol: symbol, chain: chain, in: registry) }
    #expect(listed("0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC", "NVDA", "robinhood") == true)
    #expect(listed("0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec", "nvda", "robinhood") == true)
    // A copy on Robinhood Chain under the registry's ticker.
    #expect(listed("0xA3284897ad7c8FED700b04f573Cf64E4f6c10f2c", "NVDA", "robinhood") == false)
    // Another network: no mark at all, even for the registry's own address.
    #expect(listed("0x8edc6b4df4e29AC2D5DAbd170B8AB0F700C59C95", "SPCX", "base") == nil)
    #expect(listed("0x4a0E65A3EcceC6dBe60AE065F2e7bb85Fae35eEa", "SPCX", "base") == nil)
    // A token whose ticker isn't the registry's.
    #expect(listed("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", "AI", "robinhood") == nil)
    // Skipped by parse: inactive, or deployed elsewhere.
    #expect(listed("0x1111111111111111111111111111111111111111", "OLD", "robinhood") == nil)
    #expect(listed("0x2222222222222222222222222222222222222222", "ELSE", "robinhood") == nil)
}

/// A Stock Token's Uniswap price is checked against Robinhood's bid–ask, a token by contract against DexScreener's
/// price: with no price to check against, the trade stops before anything is bridged or signed.
@Test func stockTradesStopWithoutAPriceToCheckAgainst() throws {
    // Robinhood's answers: a quote, an unknown symbol (404), no quotes, a zero side.
    let tsla = RobinhoodStocks.parseQuote(Data(#"{"quotes":[{"tokenSymbol":"TSLA","bid":"401.27","ask":"401.8","isTradingHalt":false}]}"#.utf8))
    #expect(tsla?.bid == Decimal(string: "401.27") && tsla?.ask == Decimal(string: "401.8"))
    #expect(RobinhoodStocks.parseQuote(Data(#"{"code":5,"message":"no whitelisted asset with symbol \"ZZZZ\"","details":[]}"#.utf8)) == nil)
    #expect(RobinhoodStocks.parseQuote(Data(#"{"quotes":[]}"#.utf8)) == nil)
    #expect(RobinhoodStocks.parseQuote(Data(#"{"quotes":[{"bid":"0","ask":"401.8"}]}"#.utf8)) == nil)
    #expect(RobinhoodStocks.parseQuote(Data(#"{"quotes":[{"bid":"401.27","ask":"0"}]}"#.utf8)) == nil)
    #expect(RobinhoodStocks.parseQuote(Data("<html>502</html>".utf8)) == nil)
    // A Stock Token with no Robinhood quote, a contract with no DexScreener price: stopped, nothing signed.
    #expect(throws: ExecutionError.self) { try Executor.priceBand("TSLA", listing: nil, robinhood: nil) }
    #expect(try Executor.priceBand("TSLA", listing: nil, robinhood: (401, 402)).ask == 402)
    // Within 5% of the band, both before the bridge and asked again after it; a price of nothing is outside.
    #expect(!Executor.offBand(400, (401, 402)) && !Executor.offBand(422, (401, 402)) && !Executor.offBand(381, (401, 402)))
    #expect(Executor.offBand(423, (401, 402)) && Executor.offBand(380, (401, 402)) && Executor.offBand(0, (401, 402)))
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"
    func pair(_ price: String) -> Data {
        Data(#"{"pairs":[{"chainId":"robinhood","baseToken":{"address":"\#(ai)","name":"AI","symbol":"AI"},\#(price)"liquidity":{"usd":90000}}]}"#.utf8)
    }
    #expect(try Executor.priceBand("AI", listing: TokenLookup.parse(pair(#""priceUsd":"0.157","#), address: ai).first, robinhood: nil).bid == Decimal(string: "0.157"))
    // Listed, with no price or a zero one: DexScreener's branch stops it, not Robinhood's.
    for price in ["", #""priceUsd":"0","#] {
        let listing = try #require(TokenLookup.parse(pair(price), address: ai).first)
        #expect(listing.priceUSD ?? 0 == 0)
        #expect(throws: ExecutionError.self) { try Executor.priceBand("AI", listing: listing, robinhood: (401, 402)) }
    }
}
