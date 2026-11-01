import Foundation
import Testing
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
