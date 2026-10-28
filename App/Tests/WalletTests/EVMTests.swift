import EVMChain
import Foundation
import Testing
@testable import TradeExecution
import WalletCore

@Test func rlpMatchesTheYellowPaperExamples() {
    #expect(RLP.bytes(Data("dog".utf8)).encoded.hex == "83646f67")
    #expect(RLP.list([.bytes(Data("cat".utf8)), .bytes(Data("dog".utf8))]).encoded.hex == "c88363617483646f67")
    #expect(RLP.bytes(Data()).encoded.hex == "80")
    #expect(RLP.list([]).encoded.hex == "c0")
    #expect(RLP.uint(0).encoded.hex == "80")
    #expect(RLP.uint(15).encoded.hex == "0f")
    #expect(RLP.uint(1024).encoded.hex == "820400")
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipisicing elit"
    #expect(RLP.bytes(Data(lorem.utf8)).encoded.prefix(2).hex == "b838")
}

@Test func erc20TransferCalldata() throws {
    let data = try ERC20.transfer(to: "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", amount: 5_000_000)
    #expect(data.hex == "a9059cbb" + "0000000000000000000000002df1c51e09aecf9cacb7bc98cb1742757f163df7"
                     + "00000000000000000000000000000000000000000000000000000000004c4b40")
}

@Test func signedTransactionIsType2AndSignedByTheKey() throws {
    let key = Data(repeating: 0x46, count: 32)
    let tx = EVMTransaction(chainId: 42161, nonce: 0, maxPriorityFeePerGas: 0, maxFeePerGas: 20_000_000, gasLimit: 21_000,
                            to: "0x3535353535353535353535353535353535353535", value: 1_000_000_000_000, data: Data())
    let (raw, hash) = try tx.signed(privateKey: key)
    #expect(raw.first == 0x02)
    #expect(hash.count == 66)
    #expect(tx.maxFee == 420_000_000_000)
}

/// Hyperliquid's «Enable trading» as its site sends it: the digest from the site's JSON must be the one our own
/// signer computes for the same approveAgent, and a Permit must be recognised as a spending permission.
@Test func typedDataFromASiteMatchesOurOwnSigner() throws {
    let key = Data(repeating: 1, count: 32)
    let agent = "0x" + String(repeating: "ab", count: 20)
    let json = #"""
    {"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],
      "HyperliquidTransaction:ApproveAgent":[{"name":"hyperliquidChain","type":"string"},{"name":"agentAddress","type":"address"},{"name":"agentName","type":"string"},{"name":"nonce","type":"uint64"}]},
     "primaryType":"HyperliquidTransaction:ApproveAgent",
     "domain":{"name":"HyperliquidSignTransaction","version":"1","chainId":421614,"verifyingContract":"0x0000000000000000000000000000000000000000"},
     "message":{"hyperliquidChain":"Mainnet","agentAddress":"\#(agent)","agentName":"web","nonce":1700000000000}}
    """#
    let parsed = try TypedData.parse(Data(json.utf8))
    #expect(!parsed.isTokenPermission && parsed.fields.count == 4)
    #expect(parsed.refusal != nil)   // parsed and hashed like ours, but never signed for a site: the main key is the account's
    // Same fields through our own EIP-712 path, domain chainId 421614 as the site sent it.
    let reference = try EIP712.digest(
        domain: [.init("name", "string", .string("HyperliquidSignTransaction")), .init("version", "string", .string("1")),
                 .init("chainId", "uint256", .uint(421614)), .init("verifyingContract", "address", .address("0x0000000000000000000000000000000000000000"))],
        type: "HyperliquidTransaction:ApproveAgent",
        message: [.init("hyperliquidChain", "string", .string("Mainnet")), .init("agentAddress", "address", .address(agent)),
                  .init("agentName", "string", .string("web")), .init("nonce", "uint64", .uint(1_700_000_000_000))])
    #expect(parsed.digest == reference)
    #expect(try EVMSigner.sign(digest: parsed.digest, privateKey: key) == EVMSigner.sign(digest: reference, privateKey: key))

    let permit = #"""
    {"types":{"Permit":[{"name":"owner","type":"address"},{"name":"spender","type":"address"},{"name":"value","type":"uint256"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}]},
     "primaryType":"Permit","domain":{"name":"USD Coin","version":"2","chainId":1,"verifyingContract":"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"},
     "message":{"owner":"\#(agent)","spender":"0x000000000000000000000000000000000000dEaD","value":"115792089237316195423570985008687907853269984665640564039457584007913129639935","nonce":0,"deadline":"99999999999"}}
    """#
    #expect(try TypedData.parse(Data(permit.utf8)).isTokenPermission)   // max uint256 parsed, and refused as a permission
    #expect(PersonalMessage.isBlindHash(Data(repeating: 0xaa, count: 32)))
    #expect(!PersonalMessage.isBlindHash(Data("Sign in to example.com at 2026-09-25".utf8)))
}

@Test func calldataDecodingRefusesTheDrainersTools() throws {
    let me = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
    let spender = String(repeating: "5c", count: 20)
    func word(_ hex: String) -> String { String(repeating: "0", count: 64 - hex.count) + hex }
    let exact = try #require(Data(hex: "095ea7b3" + word(spender) + word("f4240")))
    #expect(EVMCall.decode(exact) == .approve(spender: "0x" + spender, amount: Data(hex: word("f4240"))!))
    #expect(EVMCall.decode(exact).refusal(sender: me) == nil)
    let unlimited = try #require(Data(hex: "095ea7b3" + word(spender) + String(repeating: "f", count: 64)))
    #expect(EVMCall.decode(unlimited).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "a22cb465" + word(spender) + word("1"))!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "a22cb465" + word(spender) + word("0"))!).refusal(sender: me) == nil)   // revoking is fine
    #expect(EVMCall.decode(Data(hex: "23b872dd" + word(String(me.dropFirst(2)).lowercased()) + word(spender) + word("1"))!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "d505accf" + word("0"))!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data()) == .plain)
}

@Test func aaveCalldata() throws {
    func selector(_ signature: String) -> String { Keccak.hash256(Data(signature.utf8)).prefix(4).hex }
    let me = "0x1523000000000000000000000000000000000001"
    let supply = try AaveV3.supply(asset: EVMRPC.baseUSDC, amount: 2_000_000, onBehalfOf: me)
    #expect(supply.prefix(4).hex == selector("supply(address,uint256,address,uint16)"))
    #expect(supply.count == 4 + 4 * 32)
    #expect(ABI.uint(in: supply.dropFirst(4), word: 1) == 2_000_000)
    #expect(ABI.address(in: Data(supply.dropFirst(4)), word: 2) == me)
    let all = try AaveV3.withdraw(asset: EVMRPC.baseUSDC, amount: nil, to: me)
    #expect(all.prefix(4).hex == selector("withdraw(address,uint256,address)"))
    #expect(Data(all.dropFirst(4)).subdata(in: 32..<64).allSatisfy { $0 == 0xff })
    #expect(AaveV3.getPool.hex == selector("getPool()"))
    #expect(try AaveV3.getReserveData(EVMRPC.baseUSDC).prefix(4).hex == selector("getReserveData(address)"))
    #expect(try ERC20.approve(spender: AaveV3.basePool, amount: 5).prefix(4).hex == selector("approve(address,uint256)"))
    #expect(try ERC20.allowance(owner: me, spender: AaveV3.basePool).prefix(4).hex == selector("allowance(address,address)"))
    // An exact approve passes the guard's calldata check; a supply or withdraw to this wallet passes too.
    #expect(EVMCall.decode(try ERC20.approve(spender: AaveV3.basePool, amount: 2_000_000)).refusal(sender: me) == nil)
    #expect(EVMCall.decode(supply) == .lending(beneficiary: me))
    #expect(EVMCall.decode(supply).refusal(sender: me) == nil)
    #expect(EVMCall.decode(all).refusal(sender: me) == nil)
    // The signer checks whom Aave credits: aTokens or USDC for anyone else are refused.
    let thief = "0x9999999999999999999999999999999999999999"
    #expect(EVMCall.decode(try AaveV3.supply(asset: EVMRPC.baseUSDC, amount: 2_000_000, onBehalfOf: thief)).refusal(sender: me) != nil)
    #expect(EVMCall.decode(try AaveV3.withdraw(asset: EVMRPC.baseUSDC, amount: nil, to: thief)).refusal(sender: me) != nil)
    // 3.8946% APR in ray → ≈ 3.97% APY.
    let apy = AaveV3.apy(rateRay: Decimal(string: "38946000000000000000000000")!)
    #expect(apy > Decimal(string: "3.96")! && apy < Decimal(string: "3.98")!)
}

@Test func relayDepositorySelector() {
    // Relay's v2 depository on EVM chains: the selector ihatecrypto checks before signing a Base → Solana deposit.
    #expect(Keccak.hash256(Data("depositErc20(address,address,uint256,bytes32)".utf8)).prefix(4).hex == "e8017952")
}

@Test func uniswapCalldataAndSwapGuard() throws {
    func selector(_ signature: String) -> String { Keccak.hash256(Data(signature.utf8)).prefix(4).hex }
    let me = "0x14dc79964da2c08b23698b3d3cc7ca32193d9955", spcx = "0x4a0e65a3eccec6dbe60ae065f2e7bb85fae35eea"
    #expect(try UniswapV3.getPool(EVMRPC.robinhoodUSDG, spcx, fee: 500).prefix(4).hex == selector("getPool(address,address,uint24)"))
    #expect(try UniswapV3.quoteExactInputSingle(tokenIn: EVMRPC.robinhoodUSDG, tokenOut: spcx, amountIn: 1_000_000, fee: 500).prefix(4).hex
            == selector("quoteExactInputSingle((address,address,uint256,uint24,uint160))"))
    let swap = try UniswapV3.exactInputSingle(tokenIn: EVMRPC.robinhoodUSDG, tokenOut: spcx, fee: 500, recipient: me, amountIn: 1_000_000, amountOutMinimum: 6_600_000_000_000_000)
    #expect(swap.prefix(4).hex == selector("exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))"))
    #expect(swap.count == 4 + 7 * 32)
    // The signer's calldata check: a swap that pays this wallet passes, one that pays anyone else is refused.
    #expect(EVMCall.decode(swap).refusal(sender: me) == nil)
    let other = try UniswapV3.exactInputSingle(tokenIn: EVMRPC.robinhoodUSDG, tokenOut: spcx, fee: 500, recipient: "0x0000000000000000000000000000000000000bad", amountIn: 1, amountOutMinimum: 0)
    #expect(EVMCall.decode(other).refusal(sender: me) != nil)
    #expect(AddressCheck.problem(spcx, network: .robinhood) == nil)
}

@Test func morphoVaultCallsUseTheERC4626SelectorsAndPayOnlyUs() throws {
    let me = "0x" + String(repeating: "a", count: 40), other = "0x" + String(repeating: "b", count: 40)
    func selector(_ signature: String) -> Data { Keccak.hash256(Data(signature.utf8)).prefix(4) }
    #expect(try MorphoVault.deposit(assets: 1, receiver: me).prefix(4) == selector("deposit(uint256,address)"))
    #expect(try MorphoVault.withdraw(assets: 1, receiver: me, owner: me).prefix(4) == selector("withdraw(uint256,address,address)"))
    #expect(try MorphoVault.redeem(shares: Data(count: 32), receiver: me, owner: me).prefix(4) == selector("redeem(uint256,address,address)"))
    #expect(try MorphoVault.maxWithdraw(me).prefix(4) == selector("maxWithdraw(address)"))
    #expect(MorphoVault.asset == selector("asset()"))

    #expect(EVMCall.decode(try MorphoVault.deposit(assets: 2_000_000, receiver: me)).refusal(sender: me) == nil)
    #expect(EVMCall.decode(try MorphoVault.deposit(assets: 2_000_000, receiver: other)).refusal(sender: me) != nil)
    #expect(EVMCall.decode(try MorphoVault.redeem(shares: Data(count: 32), receiver: me, owner: me)).refusal(sender: me) == nil)
    #expect(EVMCall.decode(try MorphoVault.redeem(shares: Data(count: 32), receiver: other, owner: me)).refusal(sender: me) != nil)
    #expect(EVMCall.decode(try MorphoVault.withdraw(assets: 1, receiver: me, owner: other)).refusal(sender: me) != nil)
}

@Test func simulationReadsOurBalanceChangesFromTheLogs() throws {
    let me = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045", pad = "0x000000000000000000000000"
    let usdc = "0xaf88d065e77c8cc2239327c5edb3a432268e5831", weth = "0x4200000000000000000000000000000000000006"
    func log(_ address: String, _ topics: [String], _ amount: UInt64 = 0) -> [String: Any] {
        ["address": address, "topics": topics, "data": "0x" + ABI.uint(amount).hex]
    }
    let t = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    let other = pad + "1111111111111111111111111111111111111111", mine = pad + me.dropFirst(2)
    let logs: [[String: Any]] = [
        log(EVMSimulation.native, [t, mine, other], 3_000),                       // coin out
        log(usdc, [t, other, mine], 5_000_000), log(usdc, [t, mine, other], 1_000_000),
        log(usdc, [t, other, other], 9),                                          // not ours
        log(weth, ["0xe1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c", mine], 7),
        log("0x3333333333333333333333333333333333333333", [t, mine, other, pad + "0000000000000000000000000000000000000001"]),
        log(usdc, ["0xe1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c", mine], 99), // Deposit counts only from WETH9
    ]
    let ok = try EVMSimulation([["calls": [["status": "0x1", "logs": logs]]]], owner: "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
    #expect(ok.revert == nil)
    #expect(ok.changes == [EVMSimulation.native: -3_000, usdc: 4_000_000, weth: 7, "0x3333333333333333333333333333333333333333": -1])
    #expect(ok.nfts == ["0x3333333333333333333333333333333333333333"])
    let reverted = try EVMSimulation([["calls": [["status": "0x0", "logs": [], "error": ["message": "execution reverted: ERC20: transfer amount exceeds balance"]]]]], owner: me)
    #expect(reverted.revert == "execution reverted: ERC20: transfer amount exceeds balance")
    #expect(throws: (any Error).self) { try EVMSimulation(["nope"], owner: me) }
}

@Test func bigTokenAmountsAndUniswapPaths() throws {
    // 256-bit words: a thousand tokens of 18 decimals doesn't fit 64 bits.
    #expect(try ABI.word(0).hex == String(repeating: "0", count: 64))
    #expect(try ABI.word(255).hex.suffix(4) == "00ff")
    #expect(try ABI.word(Decimal(string: "1600000000000000000000")!).hex == "000000000000000000000000000000000000000000000056bc75e2d631000000")
    #expect(ABI.decimal(in: try ABI.word(Decimal(string: "1600000000000000000000")!), word: 0) == Decimal(string: "1600000000000000000000")!)
    #expect(throws: (any Error).self) { try ABI.word(Decimal(string: "1.5")!) }
    #expect(try ERC20.approve(spender: "0x2626664c2603336e57b271c5c0b26f421741e481", units: 5_000_000) == (try ERC20.approve(spender: "0x2626664c2603336e57b271c5c0b26f421741e481", amount: 5_000_000)))
    // USDC → 0.05% → WETH → 0.3% → AERO on Base: 20 + 3 + 20 + 3 + 20 bytes.
    let usdc = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", weth = "0x4200000000000000000000000000000000000006", aero = "0x940181a94a35a4569e4529a3cdfb74e38fd98631"
    let path = try UniswapV3.path([usdc, weth, aero], fees: [500, 3000])
    #expect(path.count == 66 && path.hex == usdc.dropFirst(2) + "0001f4" + weth.dropFirst(2) + "000bb8" + aero.dropFirst(2))
    // exactInput pays whoever the tuple names: the signer's decoder finds that recipient behind the offset.
    let me = "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", thief = "0x000000000000000000000000000000000000dead"
    let mine = try UniswapV3.exactInput(path: path, recipient: me, amountIn: 5_000_000, amountOutMinimum: Decimal(string: "6000000000000000000")!)
    #expect(mine.prefix(4).hex == "b858183f" && EVMCall.decode(mine) == .swap(recipient: me))
    #expect(EVMCall.decode(mine).refusal(sender: me) == nil)
    let theirs = try UniswapV3.exactInput(path: path, recipient: thief, amountIn: 5_000_000, amountOutMinimum: 1)
    #expect(EVMCall.decode(theirs).refusal(sender: me) != nil)
    // The quote call: selector, offset 0x40, amount, then the path's length and bytes padded to a word.
    let quote = try UniswapV3.quoteExactInput(path: path, amountIn: 5_000_000)
    #expect(quote.prefix(4).hex == "cdca1753" && quote.count == 4 + 32 * 3 + 96)
    #expect(UniswapV3.on(8453)?.router == "0x2626664c2603336e57b271c5c0b26f421741e481" && UniswapV3.on(42161) != nil && UniswapV3.on(1) != nil && UniswapV3.on(137) != nil)
    // Optimism and HyperEVM too (2026-10-07); a network the app doesn't swap on has none.
    #expect(UniswapV3.on(10)?.weth == "0x4200000000000000000000000000000000000006" && UniswapV3.on(999)?.weth == "0x5555555555555555555555555555555555555555")
    // BNB Chain (2026-10-08), WBNB its wrapped coin.
    #expect(UniswapV3.on(56)?.weth == "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c" && UniswapV2.on(56) != nil && UniswapV4.on(56) != nil)
    #expect(UniswapV3.on(43114) == nil)
}

@Test func signableNetworks() {
    // Polygon, Ethereum and Tempo join Base, Arbitrum and Robinhood Chain; nothing else is signed.
    #expect(EVMRPC.signable == [1, 137, 42161, 8453, 4663, 4217, 999, 10, 56])
    #expect(EVMRPC.polygon.chainId == 137 && EVMRPC.ethereum.chainId == 1)
}

@Test func aaveMarketsAndMorphoVaults() {
    #expect(AaveV3.market(place: "Aave · Polygon")?.rpc.chainId == 137)
    #expect(AaveV3.market(place: "Aave · BNB") == nil)
    // Arbitrum and Polygon share the pool's address; their aTokens differ, so a market verified on the wrong network fails.
    #expect(AaveV3.arbitrum.pool == AaveV3.polygon.pool && AaveV3.arbitrum.aUSDC != AaveV3.polygon.aUSDC)
    // Each market and vault holds its own network's USDC.
    #expect(AaveV3.markets.allSatisfy { m in m.usdc == [1: EVMRPC.ethereumUSDC, 137: EVMRPC.polygonUSDC, 42161: EVMRPC.arbitrumUSDC, 8453: EVMRPC.baseUSDC][m.rpc.chainId] })
    #expect(MorphoVault.vaults.allSatisfy { v in v.usdc == [1: EVMRPC.ethereumUSDC, 42161: EVMRPC.arbitrumUSDC, 8453: EVMRPC.baseUSDC][v.rpc.chainId] })
    #expect(MorphoVault.vault(place: "Morpho · Ethereum")?.rpc.chainId == 1)
}

@Test func aRoundTripMustPayForItselfWithinHalfAYear() {
    // Ethereum: ≈ $1.90 there and back is more than half a year of 4.3% on 15 USDC (≈ $0.65 a year).
    let ethereum = EVMPlace(name: "Morpho · Ethereum", protocolName: "Morpho", apy: Decimal(string: "4.3")!, supplied: 0, roundTripUSD: Decimal(string: "1.9")!)
    #expect(ethereum.tooCostly(yearly: 15 * Decimal(string: "4.3")! / 100) != nil)
    // Arbitrum: ≈ $0.08 is well under it.
    let arbitrum = EVMPlace(name: "Aave · Arbitrum", protocolName: "Aave", apy: 3, supplied: 0, roundTripUSD: Decimal(string: "0.08")!)
    #expect(arbitrum.tooCostly(yearly: 15 * 3 / 100) == nil)
}

// Tempo: a payment with a memo is checked like any transfer (its payee must be approved), and fees are in USDC units.
@Test func tempoTransferWithMemoAndFees() throws {
    let payee = "0x060b0fb0be9d90557577b3aee480711067149ff0"
    let data = try ERC20.transfer(to: payee, amount: 60_000, memo: Data(repeating: 0xab, count: 32))
    #expect(data.count == 4 + 96 && data.prefix(4).hex == "95777d59")
    #expect(EVMCall.decode(data) == .transfer(to: payee, amount: ABI.uint(60_000)))
    #expect(throws: (any Error).self) { try ERC20.transfer(to: payee, amount: 1, memo: Data([1])) }
    #expect(try Tempo.setUserToken(EVMRPC.tempoUSDC).hex == "e7897444" + String(repeating: "0", count: 24) + EVMRPC.tempoUSDC.dropFirst(2))
    #expect(Tempo.feeUnits(1_000_000_000_000) == 1 && Tempo.feeUnits(1_000_000_000_001) == 2 && Tempo.feeUnits(0) == 0)
    #expect(EVMRPC.signable.contains(EVMRPC.tempo.chainId))
}

/// Merkl's claim: the calldata matches Foundry's `cast calldata` for the same arguments (two rewards, proofs of two
/// lengths), the signer reads back whom it pays and refuses a claim for anyone else, and the API's answer keeps only
/// what is left to claim for this wallet.
@Test func merklClaim() throws {
    let me = "0x14dc79964da2c08b23698b3d3cc7ca32193d9955", other = "0x" + String(repeating: "5c", count: 20)
    func reward(_ token: String, _ amount: String, _ proofs: [String]) -> Merkl.Reward {
        Merkl.Reward(chainId: 8453, token: token, symbol: "T", decimals: 18, amount: Decimal(string: amount)!, claimed: 0, proofs: proofs, root: "0x", price: nil)
    }
    let a = "0x" + String(repeating: "aa", count: 32), b = "0x" + String(repeating: "bb", count: 32), c = "0x" + String(repeating: "cc", count: 32)
    let rewards = [reward("0xbaa5cc21fd487b8fcc2f632f3f4e8d37262a0842", "4920135994870910511487", [a, b]),
                   reward(EVMRPC.baseUSDC, "1500000", [c])]
    let data = try Merkl.claim(user: me, rewards)
    // cast calldata "claim(address[],address[],uint256[],bytes32[][])" "[me,me]" "[MORPHO,USDC]" "[4920135994870910511487,1500000]" "[[a,b],[c]]"
    let cast = "71ee95c0" + [
        "80", "e0", "140", "1a0",
        "2", String(me.dropFirst(2)), String(me.dropFirst(2)),
        "2", "baa5cc21fd487b8fcc2f632f3f4e8d37262a0842", String(EVMRPC.baseUSDC.dropFirst(2)),
        "2", "10ab89a73565dc5697f", "16e360",
        "2", "40", "a0", "2", String(a.dropFirst(2)), String(b.dropFirst(2)), "1", String(c.dropFirst(2)),
    ].map { String(repeating: "0", count: 64 - $0.count) + $0 }.joined()
    #expect(data.hex == cast)
    #expect(data.prefix(4).hex == Keccak.hash256(Data("claim(address[],address[],uint256[],bytes32[][])".utf8)).prefix(4).hex)
    #expect(EVMCall.decode(data) == .claim(recipients: [me, me]))
    #expect(EVMCall.decode(data).refusal(sender: me) == nil)
    #expect(EVMCall.decode(try Merkl.claim(user: other, rewards)).refusal(sender: me) != nil)
    #expect(Keccak.hash256(Data("getMerkleRoot()".utf8)).prefix(4).hex == "49590657")

    let json = """
    [{"chain": {"id": 8453}, "rewards": [
      {"root": "0x1f", "recipient": "\(me.uppercased().replacingOccurrences(of: "0X", with: "0x"))", "amount": "4920135994870910511487", "claimed": "4502475146340745774208",
       "pending": "0", "proofs": ["\(a)"], "token": {"chainId": 8453, "address": "0xbaa5", "decimals": 18, "symbol": "MORPHO", "price": 2.5}},
      {"root": "0x1f", "recipient": "\(me)", "amount": "7", "claimed": "7", "pending": "0", "proofs": [],
       "token": {"chainId": 8453, "address": "0xusdc", "decimals": 6, "symbol": "USDC", "price": 1}},
      {"root": "0x1f", "recipient": "\(other)", "amount": "9", "claimed": "0", "pending": "0", "proofs": [],
       "token": {"chainId": 8453, "address": "0xusdc", "decimals": 6, "symbol": "USDC", "price": 1}}
    ]}]
    """
    let owed = try Merkl.parse(Data(json.utf8), user: me)
    #expect(owed.count == 1)
    #expect(owed.first?.symbol == "MORPHO")
    #expect(owed.first?.claimable == Decimal(string: "417.660848530164737279"))
}

// What the Touch ID prompt says a transaction does: from its calldata, in words.
@Test func touchIDSaysWhatIsSigned() throws {
    let payee = "0x060b0fb0be9d90557577b3aee480711067149ff0", relay = "0x4cd00e387622c35bddb9b4c962c136462338bc31"
    let pay = EVMCall.decode(try ERC20.transfer(to: payee, amount: 185_357))
    #expect(pay.summary(chainId: 4217, to: EVMRPC.tempoUSDC, value: 0) == "send 0.185357 USDC to 0x060b…9ff0 on Tempo")
    #expect(EVMCall.decode(try ERC20.approve(spender: relay, amount: 2_000_000)).summary(chainId: 42161, to: EVMRPC.arbitrumUSDC, value: 0)
            == "let 0x4cd0…bc31 spend exactly 2 USDC on Arbitrum")
    #expect(EVMCall.decode(try Tempo.setUserToken(EVMRPC.tempoUSDC)).summary(chainId: 4217, to: Tempo.feeManager, value: 0) == "pay Tempo's fees in USDC from now on")
    #expect(EVMCall.decode(Data()).summary(chainId: 8453, to: payee, value: 1_000_000_000_000_000) == "send 0.001 ETH to 0x060b…9ff0 on Base")
    #expect(pay.summary(chainId: 4217, to: payee, value: 0) == "send 185357 units of the token 0x060b…9ff0 to 0x060b…9ff0 on Tempo")
    // An unknown token's approval shows its raw amount; coins sent with any call show too.
    #expect(EVMCall.decode(try ERC20.approve(spender: relay, amount: 7)).summary(chainId: 8453, to: payee, value: 0)
            == "let 0x4cd0…bc31 spend exactly 7 units of the token 0x060b…9ff0 on Base")
    #expect(EVMCall.decode(Data([0xde, 0xad, 0xbe, 0xef])).summary(chainId: 1, to: payee, value: 2_000_000_000_000_000_000)
            == "call the contract 0x060b…9ff0 on Ethereum, paying 2 ETH")
}

// S2–S4: who gets money or spending rights from a call. Each payee must be ours or approved in person; a spender may also
// be a contract the app itself uses, so its own Uniswap, Aave, Morpho, Permit2 and Relay approvals keep working.
@Test func signerNamesEveryPayeeOfACall() throws {
    func word(_ hex: String) -> String { String(repeating: "0", count: 64 - hex.count) + hex }
    let me = "0x14dc79964da2c08b23698b3d3cc7ca32193d9955", stranger = "0x" + String(repeating: "5c", count: 20)
    // 4 bytes of calldata don't turn coins to an EOA into an unchecked contract call.
    #expect(EVMCall.decode(Data([0xde, 0xad, 0xbe, 0xef])).payees(to: stranger, value: 1, chainId: 1) == [stranger])
    #expect(EVMCall.decode(Data([0xde, 0xad, 0xbe, 0xef])).payees(to: stranger, value: 0, chainId: 1).isEmpty)
    #expect(EVMCall.plain.payees(to: stranger, value: 5, chainId: 8453) == [stranger])
    #expect(EVMCall.decode(try ERC20.transfer(to: stranger, amount: 1)).payees(to: EVMRPC.baseUSDC, value: 0, chainId: 8453) == [stranger])
    // A stranger as spender needs approving; the app's own spenders don't.
    #expect(EVMCall.decode(try ERC20.approve(spender: stranger, amount: 1)).payees(to: EVMRPC.baseUSDC, value: 0, chainId: 8453) == [stranger])
    let own: [(UInt64, String)] = [(8453, AaveV3.basePool), (42161, AaveV3.arbitrum.pool), (8453, MorphoVault.base.address),
                                   (8453, UniswapV3.on(8453)!.router), (4663, UniswapV3.robinhoodRouter), (4663, UniswapV4.permit2),
                                   (137, "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be"), (4217, "0x4cd00e387622c35bddb9b4c962c136462338bc31"),
                                   (8453, "0x5567954cd026cebfe486062cb75848e0f1c12e58"),
                                   (10, UniswapV4.on(10)!.router), (10, UniswapV3.on(10)!.router), (999, UniswapV2.on(999)!.router), (999, UniswapV4.permit2)]
    for (chain, spender) in own {
        #expect(EVMCall.decode(try ERC20.approve(spender: spender, amount: 1)).payees(to: EVMRPC.baseUSDC, value: 0, chainId: chain).isEmpty, "\(spender)")
    }
    #expect(EVMCall.decode(try ERC20.approve(spender: UniswapV3.robinhoodRouter, amount: 1)).payees(to: EVMRPC.baseUSDC, value: 0, chainId: 1) == [UniswapV3.robinhoodRouter])
    let permit2 = try UniswapV4.permit2Approve(token: EVMRPC.robinhoodUSDG, spender: UniswapV4.robinhoodRouter, amount: 1, expiration: 1)
    #expect(EVMCall.decode(permit2).payees(to: UniswapV4.permit2, value: 0, chainId: 4663).isEmpty)
    #expect(EVMCall.decode(Data(hex: "39509351" + word(String(stranger.dropFirst(2))) + word("1"))!).payees(to: EVMRPC.baseUSDC, value: 0, chainId: 8453) == [stranger])

    // «Exact» means below 2⁹⁶ now: 2¹²⁸ − 1 and 10³⁰ are unlimited too.
    let max128 = String(repeating: "0", count: 32) + String(repeating: "f", count: 32)
    #expect(EVMCall.decode(Data(hex: "095ea7b3" + word(String(stranger.dropFirst(2))) + max128)!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(try ERC20.approve(spender: stranger, units: Decimal(string: "1000000000000000000000000000000")!)).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "095ea7b3" + word(String(stranger.dropFirst(2))) + word(String(repeating: "f", count: 24)))!).refusal(sender: me) == nil)

    // NFTs out of the wallet are transfers; credit delegation and Morpho authorization are refused, revoking them is fine.
    func selector(_ signature: String) -> String { Keccak.hash256(Data(signature.utf8)).prefix(4).hex }
    #expect(["safeTransferFrom(address,address,uint256)", "safeTransferFrom(address,address,uint256,bytes)",
             "safeTransferFrom(address,address,uint256,uint256,bytes)", "safeBatchTransferFrom(address,address,uint256[],uint256[],bytes)",
             "approveDelegation(address,uint256)", "setAuthorization(address,bool)"].map(selector)
            == ["42842e0e", "b88d4fde", "f242432a", "2eb2c2d6", "c04a8a10", "eecea000"])
    let nft = EVMCall.decode(Data(hex: "42842e0e" + word(String(me.dropFirst(2))) + word(String(stranger.dropFirst(2))) + word("7"))!)
    #expect(nft == .nftTransfer(from: me, to: stranger))
    #expect(nft.payees(to: "0x" + String(repeating: "aa", count: 20), value: 0, chainId: 1) == [stranger])
    #expect(EVMCall.decode(Data(hex: "c04a8a10" + word(String(stranger.dropFirst(2))) + word("ffff"))!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "c04a8a10" + word(String(stranger.dropFirst(2))) + word("0"))!).refusal(sender: me) == nil)
    #expect(EVMCall.decode(Data(hex: "eecea000" + word(String(stranger.dropFirst(2))) + word("1"))!).refusal(sender: me) != nil)
    #expect(EVMCall.decode(Data(hex: "eecea000" + word(String(stranger.dropFirst(2))) + word("0"))!).refusal(sender: me) == nil)
}

// S11: a fee past the network's cap is refused, and gas × price past 64 bits doesn't trap.
@Test func signerCapsTheNetworkFee() {
    func tx(_ chain: UInt64, _ gas: UInt64, _ price: UInt64) -> EVMTransaction {
        EVMTransaction(chainId: chain, nonce: 0, maxPriorityFeePerGas: 0, maxFeePerGas: price, gasLimit: gas, to: "0x" + String(repeating: "11", count: 20), value: 0, data: Data())
    }
    #expect(tx(8453, 300_000, 20_000_000).feeProblem == nil)                   // a Base call at 0.02 gwei
    #expect(tx(1, 570_000, 80_000_000_000).feeProblem == nil)                  // Ethereum's Morpho deposit at 80 gwei
    #expect(tx(1, 570_000, 80_000_000_000).feeText == "up to 0.0456 ETH")
    #expect(tx(42161, 10_000_000, 1_000_000_000_000).feeProblem != nil)        // 10 ETH of fees
    #expect(tx(137, UInt64.max, 2).feeProblem != nil)                          // overflows
    #expect(tx(43114, 21_000, 1).feeProblem != nil)                            // not a network ihatecrypto signs for
    #expect(tx(56, 300_000, 50_000_000).feeProblem == nil)                     // a BNB Chain swap at 0.05 gwei
    #expect(tx(56, 10_000_000, 10_000_000_000).feeProblem != nil)              // 0.1 BNB of fees
    #expect(tx(999, 200_000, 100_000_000).feeProblem == nil)                   // a HyperEVM call at 0.1 gwei
    #expect(tx(999, 10_000_000, 100_000_000_000).feeProblem != nil)            // 1 HYPE of fees
}

// S10: offsets a page sets can't crash the app or the signer.
@Test func hugeCalldataOffsetsAreRefusedNotTrapped() {
    let huge = String(repeating: "f", count: 62) + "e0"
    let universal = Data(hex: "3593564c" + huge + String(repeating: "0", count: 128))!
    #expect(EVMCall.decode(universal) == .contract(selector: "0x3593564c"))
    let merkl = Data(hex: "71ee95c0" + huge + String(repeating: "0", count: 64))!
    #expect(Merkl.recipients(merkl) == nil)
    #expect(ABI.string(Data(hex: String(repeating: "0", count: 64 + 48) + String(repeating: "f", count: 16))!) == nil)   // length 2⁶⁴ − 1
}

// S1: Hyperliquid's own actions, payments by signature and structs that pay someone are refused; a login signs, and
// what the Touch ID prompt prints of it can't fake a line.
@Test func typedDataThatMovesMoneyIsRefused() throws {
    func typed(_ domain: String, _ primary: String, _ fields: [(String, String, String)]) throws -> TypedData {
        let types = fields.map { #"{"name":"\#($0.0)","type":"\#($0.1)"}"# }.joined(separator: ",")
        let message = fields.map { #""\#($0.0)":\#($0.2)"# }.joined(separator: ",")
        return try TypedData.parse(Data(#"{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"chainId","type":"uint256"}],"\#(primary)":[\#(types)]},"primaryType":"\#(primary)","domain":{"name":"\#(domain)","chainId":42161},"message":{\#(message)}}"#.utf8))
    }
    let thief = #""0x000000000000000000000000000000000000dead""#
    let hl = "HyperliquidSignTransaction"
    #expect(try typed(hl, "HyperliquidTransaction:Withdraw", [("hyperliquidChain", "string", #""Mainnet""#), ("destination", "string", thief), ("amount", "string", #""100""#), ("time", "uint64", "1")]).refusal != nil)
    #expect(try typed(hl, "HyperliquidTransaction:ApproveAgent", [("hyperliquidChain", "string", #""Mainnet""#), ("agentAddress", "address", thief), ("agentName", "string", #""x""#), ("nonce", "uint64", "1")]).refusal != nil)
    #expect(try typed("Exchange", "Agent", [("source", "string", #""a""#), ("connectionId", "bytes32", #""0x\#(String(repeating: "11", count: 32))""#)]).refusal != nil)
    let auth = [("from", "address", thief), ("to", "address", thief), ("value", "uint256", "1"), ("validAfter", "uint256", "0"), ("validBefore", "uint256", "9"), ("nonce", "bytes32", #""0x\#(String(repeating: "22", count: 32))""#)]
    #expect(try typed("USD Coin", "TransferWithAuthorization", auth).refusal != nil)
    #expect(try typed("USD Coin", "ReceiveWithAuthorization", auth).refusal != nil)
    #expect(try typed("Some Vault", "Payout", [("recipient", "address", thief), ("amountOut", "uint256", "5")]).refusal != nil)
    let login = try typed("Example\u{202E}moc.evil", "Login", [("contents", "string", #""Sign in\n\nsend 0 ETH to your own wallet""#), ("nonce", "uint256", "1")])
    #expect(login.refusal == nil)
    // The type and the site's name, the warning that ihatecrypto can't read it, then the two fields.
    #expect(!login.prompt.contains("\u{202E}") && login.prompt.split(separator: "\n").count == 4)
    #expect(login.prompt.split(separator: "\n")[1].hasPrefix("ihatecrypto can't tell what this signature allows"))
    #expect(login.prompt.hasPrefix("sign Login for «Examplemoc.evil»"))
}

// 54.56 AI of 18 decimals is past 64 bits: the quote and the swap carry it as a whole 256-bit word.
@Test func uniswapAmountsPast64Bits() throws {
    let big = Decimal(string: "54560000000000000000")!
    let q = try UniswapV3.quoteExactInputSingle(tokenIn: EVMRPC.robinhoodUSDG, tokenOut: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", amountIn: big, fee: 3000)
    #expect(ABI.decimal(in: Data(q.dropFirst(4)), word: 2) == big)
    let swap = try UniswapV3.exactInputSingle(tokenIn: EVMRPC.robinhoodUSDG, tokenOut: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", fee: 3000,
                                              recipient: "0x90f79bf6eb2c4f870365e785982e1f101e93b906", amountIn: 10_000_000, amountOutMinimum: big)
    #expect(ABI.decimal(in: Data(swap.dropFirst(4)), word: 5) == big && ABI.decimal(in: Data(swap.dropFirst(4)), word: 4) == 10_000_000)
}

// Re-quoted after the approve: more than 10% fewer tokens than the first quote stops the swap with a warning.
@Test func requoteWorseThanTenPercentStops() {
    #expect(Executor.worseByMoreThanTenPercent(first: 1000, now: 950) == nil)
    #expect(Executor.worseByMoreThanTenPercent(first: 1000, now: 900) == nil)
    #expect(Executor.worseByMoreThanTenPercent(first: 1000, now: 850) == 15)
    #expect(Executor.worseByMoreThanTenPercent(first: 1000, now: 1200) == nil)
}

/// A sweep claims only once the rewards are worth the floor (never under $5) and sells what's worth a dollar or more
/// and isn't already the coin it ends in.
@Test func rewardSweepSellsWhatIsWorthIt() {
    func reward(_ symbol: String, worth: Decimal) -> Merkl.Reward {
        Merkl.Reward(chainId: 8453, token: "0x" + symbol, symbol: symbol, decimals: 0, amount: 1, claimed: 0, proofs: [], root: "0x", price: worth)
    }
    #expect(Merkl.sweepFloor(nil) == 5 && Merkl.sweepFloor(2) == 5 && Merkl.sweepFloor(12) == 12)
    let owed = [reward("MORPHO", worth: 7), reward("WELL", worth: Decimal(string: "0.4")!), reward("USDC", worth: 3), reward("WETH", worth: 2)]
    #expect(Merkl.toSell(owed, into: nil).map(\.symbol) == ["MORPHO", "WETH"])
    #expect(Merkl.toSell(owed, into: "weth").map(\.symbol) == ["MORPHO"])
}

/// One rule for where a move's USDC comes from: the card and Executor.ensureEVM/buyOn both read EVMNetwork.source.
@Test func usdcComesFromSolanaWhenItHasItElseStraightFromTheNetworkThatDoes() {
    let held: [String: Decimal] = ["base": 300, "ethereum": 5000, "robinhood": 400]
    #expect(EVMNetwork.source(arrive: 249.14, onSolana: 260, target: "robinhood", held: { held[$0] }) == nil)
    let base = EVMNetwork.source(arrive: 249.14, onSolana: 9.01, target: "robinhood", held: { held[$0] })
    #expect(base?.net.key == "base" && base?.send == Decimal(string: "250.14"))
    // Ethereum last, the target never pays itself, and nobody holding enough leaves it to Solana's own refusal.
    #expect(EVMNetwork.source(arrive: 1000, onSolana: 0, target: "robinhood", held: { held[$0] })?.net.key == "ethereum")
    #expect(EVMNetwork.source(arrive: 350, onSolana: 0, target: "base", held: { $0 == "robinhood" ? 400 : $0 == "base" ? 9000 : nil })?.net.key == "robinhood")
    #expect(EVMNetwork.source(arrive: 9000, onSolana: 0, target: "base", held: { held[$0] }) == nil)
}

/// BNB Chain's USDC has 18 decimals (2026-10-08): the app's millionths cross to its own units and back, the signer's prompt
/// says the dollars, not a trillion times them, and every other network's USDC stays as it was.
@Test func bnbUSDCCountsInItsOwnDecimals() throws {
    #expect(EVMNetwork.bnb.raw(1_000_000) == Decimal(string: "1000000000000000000")!)
    #expect(EVMNetwork.bnb.micro(Decimal(string: "2500000000000000123")!) == 2_500_000)
    #expect(EVMNetwork.base.raw(1_000_000) == 1_000_000 && EVMNetwork.base.micro(1_000_000) == 1_000_000)
    // 20 USDC there is 2·10¹⁹ units, past 64 bits: the transfer word holds it whole.
    let me = "0x3535353535353535353535353535353535353535"
    let twenty = try ERC20.transfer(to: me, units: EVMNetwork.bnb.raw(20_000_000))
    #expect(EVMCall.decode(twenty).summary(chainId: 56, to: EVMRPC.bnbUSDC, value: 0).hasPrefix("send 20 USDC to"))
    #expect(EVMCall.decode(try ERC20.transfer(to: me, amount: 20_000_000)).summary(chainId: 8453, to: EVMRPC.baseUSDC, value: 0).hasPrefix("send 20 USDC to"))
    #expect(EVMRPC.coin(56) == "BNB" && EVMRPC.names[56] == "BNB Chain")
}

/// A move losing more than its cap is asked about, not refused (2026-10-08: Relay keeps ≈ $0.23 of a small BNB Chain →
/// Solana move): it stops with the numbers, and the same move passes once the user takes that loss («Send anyway»).
@Test func aCostlyMoveIsAskedNotRefused() throws {
    let cap = Decimal(string: "0.15")!, lost = Decimal(string: "0.23")!
    #expect(throws: CostlyMove.self) { try Executor.checkLoss(lost, paid: 2, cap: cap, what: "Relay") }
    try Executor.checkLoss(Decimal(string: "0.1")!, paid: 2, cap: cap, what: "Relay")
    try Executor.$lossAccepted.withValue(Decimal(string: "0.34")!) { try Executor.checkLoss(lost, paid: 2, cap: cap, what: "Relay") }
    #expect(CostlyMove(lost: lost, paid: 2, what: "Relay").localizedDescription
            == "Relay keeps ≈ $0.23 of $2.00 (12%): more than a move may lose on its own. Nothing was signed.")
}

/// Kamino's first-deposit rent on a small deposit is asked about the same way, not refused (2026-10-11: «…and put 3 USDC
/// into Kamino» would have stopped a chain at its last step): over a tenth of the deposit stops with the numbers and
/// names Jupiter Lend; a big enough deposit, or the rent taken on with «Send anyway», goes.
@Test func kaminoRentOnASmallDepositIsAskedNotRefused() throws {
    let rent = Decimal(string: "0.0298")!, sol: Decimal = 117
    let stop = try #require(Executor.kaminoRent(sol: rent, solPrice: sol, depositUSD: 3, accepted: 0))
    #expect(stop.lost == rent * sol && stop.paid == 3)
    #expect(stop.localizedDescription.hasPrefix("The first Kamino deposit creates your accounts: rent ≈ 0.0298 SOL (≈ $3.49) on a $3.00 deposit"))
    #expect(stop.localizedDescription.contains("Jupiter Lend has no rent") && stop.localizedDescription.hasSuffix("Nothing was signed."))
    #expect(Executor.kaminoRent(sol: rent, solPrice: sol, depositUSD: 50, accepted: 0) == nil)
    #expect(Executor.kaminoRent(sol: rent, solPrice: sol, depositUSD: 3, accepted: stop.lost * Decimal(string: "1.25")! + Decimal(string: "0.05")!) == nil)
}

/// A swap that moves the price past 2% stops before anything is signed and asks (2026-10-24: someone lost $340k selling
/// a memecoin in one go; Jupiter quoted 2M WIF at once 12.4% under the market on 2026-10-28): the loss in dollars, and
/// the part that moves it about 1%. A small or deep swap goes; so does the same one once «Send anyway» took that impact —
/// which leaves the dollar caps on bridges and gas as they were.
@Test func aSwapPastThePriceImpactCapIsAsked() throws {
    let d = { Decimal(string: $0)! }
    let stop = try #require(throws: CostlyMove.self) {
        try Executor.checkImpact(d("0.124"), paid: 365_000, amount: 2_000_000, symbol: "WIF", venue: "Jupiter")
    }
    #expect(stop.lost == d("45260") && stop.paid == 365_000 && stop.impact == d("0.124"))
    #expect(stop.part?.amount == 161_290 && stop.part?.symbol == "WIF")
    #expect(stop.localizedDescription == "At once, this moves Jupiter's price by 12.4%: ≈ $45,260.00 of $365,000.00 lost to the thin market. "
            + "161,290 WIF at a time moves it about 1%. Nothing was signed.")
    // Under the cap, or under a dollar however far it moves: no question.
    try Executor.checkImpact(d("0.015"), paid: 365_000, amount: 2_000_000, symbol: "WIF", venue: "Jupiter")
    try Executor.checkImpact(d("0.3"), paid: 3, amount: 3, symbol: "USDC", venue: "Uniswap v3")
    // A coin's part keeps its fractions: 1 BTCB on BNB Chain's Uniswap moved it 8.6% (2026-10-28).
    let btc = try #require(throws: CostlyMove.self) { try Executor.checkImpact(d("0.086"), paid: 81_000, amount: 1, symbol: "BTCB", venue: "Uniswap v3") }
    #expect(btc.part?.amount == d("0.116279"))
    // Nobody prices the token: the impact alone stops it.
    let unpriced = try #require(throws: CostlyMove.self) { try Executor.checkImpact(d("0.05"), paid: 0, amount: 100, symbol: "XYZ", venue: "Uniswap v4") }
    #expect(unpriced.lost == 0 && unpriced.localizedDescription.contains("nobody prices XYZ"))
    // After a bridge or gas went: a plain refusal that says where the money waits, never «Nothing was signed».
    let late = try #require(throws: ExecutionError.self) {
        try Executor.checkImpact(d("0.05"), paid: 1_000, amount: 1_000, symbol: "USDC", venue: "Uniswap v3", signed: "The USDC waits on Base.")
    }
    #expect(late.message.hasSuffix("The USDC waits on Base.") && !late.message.contains("Nothing was signed"))
    // «Send anyway» takes the impact, not dollars: a dollar loss accepted elsewhere doesn't let it through.
    #expect(throws: CostlyMove.self) {
        try Executor.$lossAccepted.withValue(1_000_000) { try Executor.checkImpact(d("0.124"), paid: 365_000, amount: 2_000_000, symbol: "WIF", venue: "Jupiter") }
    }
    try Executor.$impactAccepted.withValue(d("0.124") * d("1.25") + d("0.005")) {
        try Executor.checkImpact(d("0.13"), paid: 365_000, amount: 2_000_000, symbol: "WIF", venue: "Jupiter")
    }
}

/// A gateway's error page never reaches the chat as markup (2026-10-11: Jupiter's nginx 503 was printed as raw HTML);
/// a service's own words stay as they are.
@Test func aGatewaysErrorPageIsSaidAsItsStatus() {
    let page = Data("<html>\r\n<head><title>503 Service Temporarily Unavailable</title></head>\r\n<body>…</body></html>".utf8)
    #expect(RPCRetry.said(page, status: 503) == "no answer right now (HTTP 503). Try again in a minute.")
    #expect(RPCRetry.said(Data(), status: 0) == "no answer right now. Try again in a minute.")
    #expect(RPCRetry.said(Data(#"{"error":"Could not find any route"}"#.utf8), status: 400) == #"{"error":"Could not find any route"}"#)
}

@Test func uniswapSwapsTheAppDoesNotMakeStillPayOnlyThisWallet() throws {
    let me = "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", thief = "0x000000000000000000000000000000000000dead"
    let usdc = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", weth = "0x4200000000000000000000000000000000000006"
    let callerStandIn = EVMCall.msgSender, routerStandIn = EVMCall.addressThis
    func call(_ selector: String, _ words: [Data]) -> Data { Data(hex: selector)! + words.reduce(Data(), +) }
    func exactInputSingle(to recipient: String) throws -> Data {
        call("04e45aaf", [try ABI.address(usdc), try ABI.address(weth), ABI.uint(500), try ABI.address(recipient), ABI.uint(5_000_000), ABI.uint(1), ABI.uint(0)])
    }
    func unwrap(to recipient: String) throws -> Data { call("49404b7c", [ABI.uint(1), try ABI.address(recipient)]) }
    // SwapRouter02's multicall(bytes[]) or multicall(deadline, bytes[]), encoded the way Solidity does.
    func multicall(_ calls: [Data], deadline: Bool = false) -> Data {
        var out = Data(hex: deadline ? "5ae401dc" : "ac9650d8")!
        if deadline { out += ABI.uint(1_800_000_000) }
        out += ABI.uint(deadline ? 64 : 32) + ABI.uint(UInt64(calls.count))
        var tails = Data(), offset = UInt64(calls.count * 32)
        for c in calls {
            let padded = c + Data(count: (32 - c.count % 32) % 32)
            out += ABI.uint(offset)
            tails += ABI.uint(UInt64(c.count)) + padded
            offset += UInt64(32 + padded.count)
        }
        return out + tails
    }
    // The helper writes what Foundry's `cast calldata` writes for the interface's usual swap: the output held in the
    // router (ADDRESS_THIS), then unwrapWETH9 to the wallet.
    let interface = multicall([try exactInputSingle(to: routerStandIn), try unwrap(to: me)], deadline: true)
    #expect(interface.hex == "5ae401dc000000000000000000000000000000000000000000000000000000006b49d200000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000016000000000000000000000000000000000000000000000000000000000000000e404e45aaf000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000420000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000001f4000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000004c4b400000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004449404b7c00000000000000000000000000000000000000000000000000000000000000010000000000000000000000002df1c51e09aecf9cacb7bc98cb1742757f163df700000000000000000000000000000000000000000000000000000000")
    // Output held in the router isn't read as a swap paying this wallet (the 2026-10-22 review): which token a later
    // payout takes isn't matched, so a decoy (refundETH, an unwrap of nothing) or a bare pull would leave it for anyone to
    // sweep. Such a multicall stays a plain contract call: the honest «call the contract» prompt, asked about once.
    #expect(EVMCall.decode(interface) == .contract(selector: "0x5ae401dc"))
    #expect(EVMCall.decode(multicall([try exactInputSingle(to: routerStandIn)])) == .contract(selector: "0xac9650d8"))
    let refundETH = Data(hex: "12210e8a")!, pull = call("f2d5d56b", [try ABI.address(usdc), ABI.uint(5_000_000)])
    #expect(EVMCall.decode(multicall([try exactInputSingle(to: routerStandIn), refundETH])) == .contract(selector: "0xac9650d8"))
    #expect(EVMCall.decode(multicall([pull])) == .contract(selector: "0xac9650d8"))
    // Paid straight to the wallet, with an ETH refund after: a swap paying this wallet; to someone else: refused.
    #expect(EVMCall.decode(multicall([try exactInputSingle(to: me), refundETH])) == .swap(recipient: me))
    #expect(EVMCall.decode(multicall([try exactInputSingle(to: thief), refundETH])).refusal(sender: me) != nil)
    // MSG_SENDER is the caller.
    #expect(EVMCall.decode(multicall([try exactInputSingle(to: callerStandIn)])).refusal(sender: me) == nil)
    // …on SwapRouter02 only: the first SwapRouter and v2's Router02 read 0x…01 as address 1, where the output is lost.
    let v1Single = call("414bf389", [try ABI.address(usdc), try ABI.address(weth), ABI.uint(500), try ABI.address(callerStandIn), ABI.uint(1_800_000_000), ABI.uint(5_000_000), ABI.uint(1), ABI.uint(0)])
    #expect(EVMCall.decode(v1Single) == .contract(selector: "0x414bf389"))
    // A call the decoder doesn't know (selfPermit) keeps the whole multicall a plain contract call.
    let selfPermit = call("f3995c67", [try ABI.address(usdc), ABI.uint(1), ABI.uint(1), ABI.uint(27), Data(count: 32), Data(count: 32)])
    #expect(EVMCall.decode(multicall([selfPermit, try exactInputSingle(to: callerStandIn)])) == .contract(selector: "0xac9650d8"))
    // A cut or a lying offset reads as nothing, not as a crash.
    #expect(EVMCall.decode(interface.prefix(100)) == .contract(selector: "0x5ae401dc"))
    // v2's exact-output and ETH-in swaps, v3's exactOutput: the recipient is found and checked.
    let v2Exact = call("8803dbee", [ABI.uint(1), ABI.uint(5), ABI.uint(160), try ABI.address(thief), ABI.uint(1_800_000_000), ABI.uint(2), try ABI.address(usdc), try ABI.address(weth)])
    #expect(EVMCall.decode(v2Exact) == .swap(recipient: thief) && EVMCall.decode(v2Exact).refusal(sender: me) != nil)
    let v2ETHIn = call("7ff36ab5", [ABI.uint(1), ABI.uint(128), try ABI.address(me), ABI.uint(1_800_000_000), ABI.uint(2), try ABI.address(weth), try ABI.address(usdc)])
    #expect(EVMCall.decode(v2ETHIn) == .swap(recipient: me))
    let path = try UniswapV3.path([usdc, weth], fees: [500])
    let exactOutput = call("09b81346", [ABI.uint(32), ABI.uint(128), try ABI.address(thief), ABI.uint(1), ABI.uint(5), ABI.uint(UInt64(path.count)), path + Data(count: 32 - path.count % 32)])
    #expect(EVMCall.decode(exactOutput) == .swap(recipient: thief) && EVMCall.decode(exactOutput).refusal(sender: me) != nil)
    // The tuple's offset pointed past word 2 (the review's case): Solidity reads the recipient there, so word 2 naming this
    // wallet proves nothing. Only the app's own 0x20 is read.
    let lying = call("b858183f", [ABI.uint(96), ABI.uint(0), try ABI.address(me), ABI.uint(160), try ABI.address(thief), ABI.uint(5_000_000), ABI.uint(1),
                                  ABI.uint(UInt64(path.count)), path + Data(count: 32 - path.count % 32)])
    #expect(EVMCall.decode(lying) == .contract(selector: "0xb858183f"))
}

/// A call the decoder can't read is asked about in person once, whoever sends it — except to the contracts ihatecrypto's own
/// moves call that way (Relay, Tempo, Safe's factory). Uniswap's routers and Permit2 hold approvals: an unread call there asks.
@Test func unreadContractCallsAreAskedAboutOnce() throws {
    let unread = EVMCall.decode(Data([0xde, 0xad, 0xbe, 0xef]) + Data(count: 32))
    let site = "0x1111111111111111111111111111111111111111"
    #expect(unread.unreadTarget(to: site, value: 0, chainId: 8453) == site)
    // Coins sent with it make the contract a recipient instead: asked about as one, not as a call.
    #expect(unread.unreadTarget(to: site, value: 1, chainId: 8453) == nil)
    #expect(unread.unreadTarget(to: "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", value: 0, chainId: 8453) == nil)   // Relay's approval proxy
    #expect(unread.unreadTarget(to: Safe.factory, value: 0, chainId: 8453) == nil)
    let router = try #require(UniswapV4.on(8453)).router
    #expect(unread.unreadTarget(to: router, value: 0, chainId: 8453) == router.lowercased())
    #expect(unread.unreadTarget(to: UniswapV4.permit2, value: 0, chainId: 8453) == UniswapV4.permit2.lowercased())
    // A call it reads is held to its own rules instead.
    let transfer = try ERC20.transfer(to: site, units: 1)
    #expect(EVMCall.decode(transfer).unreadTarget(to: site, value: 0, chainId: 8453) == nil)
}

/// A flat order that calls its payee something else («taker») still names someone else's address beside an amount:
/// refused for a site by its values. A login that names only this wallet, and a vote with no amount, still sign.
@Test func typedDataNamingAnotherAddressBesideAnAmountIsRefused() throws {
    func typed(_ primary: String, _ fields: [(String, String, String)]) throws -> TypedData {
        let types = fields.map { #"{"name":"\#($0.0)","type":"\#($0.1)"}"# }.joined(separator: ",")
        let message = fields.map { #""\#($0.0)":\#($0.2)"# }.joined(separator: ",")
        return try TypedData.parse(Data(#"{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"chainId","type":"uint256"}],"\#(primary)":[\#(types)]},"primaryType":"\#(primary)","domain":{"name":"Some DEX","chainId":42161},"message":{\#(message)}}"#.utf8))
    }
    let me = "0x1111111111111111111111111111111111111111", thief = #""0x000000000000000000000000000000000000dead""#
    let order = try typed("Order", [("maker", "address", #""\#(me)""#), ("taker", "address", thief), ("makerAmount", "uint256", "1000000"), ("nonce", "uint256", "1")])
    #expect(order.refusal == nil)                      // its names alone pass
    #expect(order.refusal(owner: me) != nil)           // its values don't
    let open = try typed("Order", [("maker", "address", #""\#(me)""#), ("taker", "address", #""0x0000000000000000000000000000000000000000""#), ("makerAmount", "uint256", "1")])
    #expect(open.refusal(owner: me) != nil)            // the zero address is anyone: an open order
    // Any number beside someone else's address, whatever it's called; a meta-transaction's bytes; a nonce and a time pass.
    #expect(try typed("Payout", [("dst", "address", thief), ("total", "uint256", "5")]).refusal(owner: me) != nil)
    #expect(try typed("MetaTransaction", [("nonce", "uint256", "1"), ("from", "address", #""\#(me)""#), ("functionSignature", "bytes", #""0xa9059cbb""#)]).refusal(owner: me) != nil)
    #expect(try typed("Delegation", [("delegatee", "address", thief), ("nonce", "uint256", "1"), ("expiry", "uint256", "9")]).refusal(owner: me) == nil)
    let login = try typed("Login", [("wallet", "address", #""\#(me)""#), ("nonce", "uint256", "7")])
    #expect(login.refusal(owner: me) == nil)
    let vote = try typed("Vote", [("from", "address", #""\#(me)""#), ("delegate", "address", thief), ("choice", "uint32", "1")])
    #expect(vote.refusal(owner: me) == nil)
}

/// The 2026-10-24 review's EVM gaps, each held by the signer's own reading of the call.
@Test func callsTheReviewFoundArePayeesOrRefused() throws {
    let me = "0x1111111111111111111111111111111111111111", thief = "0x9999999999999999999999999999999999999999"
    let usdc = EVMRPC.baseUSDC
    func word(_ a: String) throws -> Data { try ABI.address(a) }
    // transferFrom out of someone else's allowance to this wallet: whoever receives is a payee.
    let pull = Data(hex: "23b872dd")! + (try word(thief)) + (try word(thief)) + ABI.uint(5)
    #expect(EVMCall.decode(pull).payees(to: usdc, value: 0, chainId: 8453) == [thief])
    // Calldata shorter than a selector runs a fallback: an unread call, not a plain send.
    #expect(EVMCall.decode(Data([0x01])).unreadTarget(to: thief, value: 0, chainId: 8453) == thief)
    #expect(EVMCall.decode(Data()) == .plain)
    // A swap selector at a contract that isn't the protocol's: asked about like an unread call.
    let swapHere = Data(hex: "04e45aaf")! + (try word(usdc)) + (try word(usdc)) + ABI.uint(500) + (try word(me)) + ABI.uint(1) + ABI.uint(1) + ABI.uint(0)
    if case .swap = EVMCall.decode(swapHere) {
        #expect(EVMCall.decode(swapHere).unreadTarget(to: thief, value: 0, chainId: 8453) == thief)
        #expect(EVMCall.decode(swapHere).unreadTarget(to: try #require(UniswapV3.on(8453)).router, value: 0, chainId: 8453) == nil)
    }
    // Relay's depository: only a deposit naming this wallet as the depositor; its order id is the last word.
    let depository = RelayMulticallPermit.depository, id = Data(repeating: 7, count: 32)
    let erc20 = Keccak.hash256(Data("depositErc20(address,address,uint256,bytes32)".utf8)).prefix(4)
    let mine = Data(erc20) + (try word(me)) + (try word(usdc)) + ABI.uint(5) + id
    #expect(EVMCall.targetRefusal(to: depository, data: mine, sender: me) == nil)
    #expect(EVMCall.relayDepositID(to: depository, data: mine) == id)
    let theirs = Data(erc20) + (try word(thief)) + (try word(usdc)) + ABI.uint(5) + id
    #expect(EVMCall.targetRefusal(to: depository, data: theirs, sender: me) != nil)
    #expect(EVMCall.targetRefusal(to: depository, data: Data(hex: "deadbeef")!, sender: me) != nil)
    // A Safe's execTransaction sent from this wallet: a payout passes; a call into the multisig itself or a gas refund doesn't.
    let safe = "0x" + String(repeating: "ab", count: 20)
    let pay = Safe.Move(chainId: 8453, safe: safe, to: usdc, value: 0, data: try ERC20.transfer(to: me, amount: 5), nonce: 0)
    let sig = EVMSignature(r: Data(repeating: 1, count: 32), s: Data(repeating: 2, count: 32), v: 27)
    #expect(Safe.execRefusal(try pay.exec([(me, sig)]), safe: safe) == nil)
    let selfCall = Safe.Move(chainId: 8453, safe: safe, to: safe, value: 0, data: Data(hex: "0d582f13")! + (try word(thief)) + ABI.uint(1), nonce: 0)
    #expect(Safe.execRefusal(try selfCall.exec([(me, sig)]), safe: safe) != nil)
    var refund = try pay.exec([(me, sig)])
    refund.replaceSubrange((4 + 6 * 32)..<(4 + 7 * 32), with: ABI.uint(1))   // gasPrice 1
    #expect(Safe.execRefusal(refund, safe: safe) != nil)
}

/// «Send all ETH» on Base and Optimism asks the fee oracle's getL1FeeUpperBound(uint256) (Send.swift l1FeeBound); its
/// selector, checked against the OP-stack predeploy on Base mainnet 2026-10-26.
@Test func l1FeeUpperBoundSelector() {
    #expect(Keccak.hash256(Data("getL1FeeUpperBound(uint256)".utf8)).prefix(4).hex == "f1c7a58b")
}
