@testable import IhatecryptoCore
import Testing

@Test func everythingIsOnUntilSwitchedOff() {
    let m = Modules()
    #expect(ModuleProtocol.allCases.allSatisfy(m.allows))
    #expect(m.offSummary.isEmpty)
    #expect(m.refusal(.kamino(deposit: true, symbol: "USDC", amount: 1)) == nil)
}

@Test func aSwitchOffStopsItsMovesAndSurvivesStorage() {
    var m = Modules()
    m.set(.kamino, false)
    m.set(.base, false)
    m = Modules(stored: m.stored)
    #expect(m.refusal(.kamino(deposit: true, symbol: "USDC", amount: 1)) == "Kamino is off in Settings › Modules.")
    // Aave needs Base; Jupiter Lend doesn't.
    #expect(m.refusal(.aave(deposit: true, amount: 1)) == "Base is off in Settings › Modules.")
    #expect(m.refusal(.lendDeposit("USDC", 1)) == nil)
    #expect(m.refusal(.solanaTo("base", 1)) != nil)
    #expect(m.refusal(.solanaTo("arbitrum", 1)) == nil)
    #expect(m.refusal(vault: "Kamino Lend") != nil)
    #expect(m.refusal(vault: "Jupiter Lend") == nil)
}

@Test func aKindOffTakesItsProtocolsWithIt() {
    var m = Modules()
    m.set(.perps, false)
    #expect(m.refusal(trade: false, bridges: false) == "Perps is off in Settings › Modules, so Hyperliquid is too.")
    #expect(m.refusal(trade: true, bridges: false) == nil)   // oil and stocks are their own kind
    m.set(.earn, false)
    #expect(m.refusal(.allocate("USDC", maxShare: nil)) != nil)
    #expect(m.refusal(.stake(1)) != nil)
    #expect(m.refusal(.buy(.usdc, usdc: 1)) == nil)
    // A Russian reply for a Russian chat.
    #expect(m.refusal(.stake(1), ru: true)?.contains("выключены") == true)
}

@Test func vaultsOnEveryNetworkFollowTheirNetworksSwitch() {
    var m = Modules()
    #expect(m.refusal(vault: "Aave · Polygon") == nil)
    #expect(m.refusal(vault: "Morpho · Ethereum") == nil)
    m.set(.polygon, false)
    #expect(m.refusal(vault: "Aave · Polygon") != nil)
    #expect(m.refusal(vault: "Aave · Arbitrum") == nil)
    m.set(.aave, false)
    #expect(m.refusal(vault: "Aave · Arbitrum") != nil)
    // A place no switch covers is refused, not waved through.
    #expect(Modules().refusal(vault: "Aave · BNB") != nil)
    #expect(Modules().refusal(vault: "Somewhere") != nil)
    #expect(ModuleNetwork(place: "polygon") == .polygon && ModuleNetwork(place: "ethereum") == .ethereum)
}

@Test func nearMovesFollowTheNetworkTheMoneyIsOn() {
    var m = Modules()
    m.set(.solana, false)
    // A buy pays with Solana USDC, what's received lands there, a sale with no network pays out there.
    #expect(m.refusal(.buyNear(usdc: 10)) == "Solana is off in Settings › Modules.")
    #expect(m.refusal(.receiveAcross(network: "sui", token: "USDC", amount: 50)) != nil)
    #expect(m.refusal(.sellNear(amount: 1, to: nil)) != nil)
    #expect(m.refusal(.sellNear(amount: 1, to: "base")) == nil)
    m.set(.solana, true)
    m.set(.base, false)
    #expect(m.refusal(.sellNear(amount: 1, to: "base")) == "Base is off in Settings › Modules.")
    #expect(m.refusal(.buyNear(usdc: 10)) == nil)
    m.set(.near, false)
    #expect(m.refusal(.buyNear(usdc: 10)) == "NEAR is off in Settings › Modules.")
}

@Test func hyperliquidSpotFollowsTheSwapsSwitch() {
    var m = Modules()
    #expect(m.refusal(.hlSpotBuy(symbol: "HYPE", usdc: 10)) == nil)
    m.set(.swaps, false)
    #expect(m.refusal(.hlSpotBuy(symbol: "HYPE", usdc: 10)) == "Swaps is off in Settings › Modules.")
    #expect(m.refusal(.hlSpotSell(symbol: "HYPE", amount: nil)) != nil)
    #expect(m.refusal(.swapOn("hyperliquid", from: "HYPE", to: "PURR", amount: 1)) != nil)
    m.set(.swaps, true)
    m.set(.hyperliquid, false)
    #expect(m.refusal(.hlSpotSell(symbol: "HYPE", amount: nil)) == "Hyperliquid is off in Settings › Modules.")
}

/// One token for another on an EVM network takes whichever of Uniswap, KyberSwap and Relay's swap is on there
/// (2026-10-28): with Uniswap off it still swaps; with all three off it's refused, in Uniswap's words.
@Test func aSwapOnANetworkNeedsOneOfItsWays() {
    var m = Modules()
    let swap = WalletCommand.swapOn("base", from: "DEGEN", to: "AERO", amount: 1)
    m.set(.uniswap, false)
    #expect(m.refusal(swap) == nil)
    m.set(.kyberswap, false)
    m.set(.relay, false)
    #expect(m.refusal(swap) == "Uniswap is off in Settings › Modules.")
    m.set(.kyberswap, true)
    #expect(m.refusal(swap) == nil)
}

@Test func aModuleNewToThisMacStaysOffUntilAnswered() {
    let known = Set(Modules.allIDs.filter { $0 != "network.bnb" })
    var m = Modules(known: known)
    #expect(!m.isOn(.bnb) && m.isOn(.base))
    #expect(m.newIDs == ["network.bnb"])
    #expect(m.refusal(.swapOn("bnb", from: "BNB", to: "USDC", amount: 1)) == "BNB Chain is off in Settings › Modules.")
    m.set(.bnb, true)
    #expect(m.isOn(.bnb) && m.newIDs.isEmpty)
    // Nothing recorded yet: every module as it was.
    #expect(Modules(known: nil).newIDs.isEmpty)
}

@Test func theStoredFormSkipsWhatItCantRead() {
    let m = Modules(stored: "network.base=0,,junk,kind.perps=1=2,protocol.kamino=1", known: nil)
    #expect(m.choices == ["network.base": false, "protocol.kamino": true])
    #expect(m.stored == "network.base=0,protocol.kamino=1")
    #expect(Modules(stored: "", known: nil) == Modules())
}

@Test func placesAndIdsNameTheirModules() {
    for n in ModuleNetwork.allCases { #expect(ModuleNetwork(place: n.rawValue) == n) }
    #expect(ModuleNetwork(place: "HL") == .hyperliquid && ModuleNetwork(place: "HyperEVM") == .hyperevm)
    #expect(ModuleNetwork(place: "sui") == nil)
    #expect(Modules.name("network.bnb") == "BNB Chain" && Modules.name("kind.earn") == "Earn · DeFi" && Modules.name("protocol.mpp") == "MPP · x402")
    #expect(Modules.name("network.mars") == "mars" && Modules.name("bogus") == "bogus")
    #expect(Modules.allIDs.count == ModuleNetwork.allCases.count + ModuleKind.allCases.count + ModuleProtocol.allCases.count)
}

@Test func theBriefNamesWhatIsOff() {
    var m = Modules()
    m.set(.tempo, false)
    m.set(.perps, false)
    m.set(.kamino, false)
    #expect(m.offSummary == "Tempo, Perps, Kamino")
    // A protocol whose kind is off is off for the brief by its kind, not listed again.
    #expect(!m.offSummary.contains("Hyperliquid"))
}
