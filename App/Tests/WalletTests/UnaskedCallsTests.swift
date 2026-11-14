@testable import EVMChain
import Foundation
import HyperliquidSigning
import Testing
import TxGuard
import WalletCore

// The 2026-11-13 review's second pass: calls that were signed unasked, or under a name that wasn't theirs.

private let me = "0x1111111111111111111111111111111111111111", other = "0x2222222222222222222222222222222222222222"

/// WETH9's deposit() and withdraw(amount) are a wrap and an unwrap only at the network's own wrapped coin, and only
/// exactly as long as they are: at another contract the signer asks about the call, and a tail isn't read as one.
@Test func wrapAndUnwrapAreReadOnlyAtTheNetworksWrappedCoin() throws {
    let weth = try #require(UniswapV2.on(8453)).weth
    let wrap = try #require(Data(hex: "d0e30db0")), unwrap = try #require(Data(hex: "2e1a7d4d")) + ABI.uint(5)
    #expect(EVMCall.decode(wrap) == .wrap && EVMCall.decode(unwrap) == .unwrap(amount: ABI.uint(5)))
    for call in [wrap, unwrap].map(EVMCall.decode) {
        #expect(call.unreadTarget(to: weth, value: 0, chainId: 8453) == nil)
        #expect(call.unreadTarget(to: other, value: 0, chainId: 8453) == other)
    }
    // Coins sent to another contract make it a recipient: asked about as one, not as a call.
    #expect(EVMCall.wrap.unreadTarget(to: other, value: 1, chainId: 8453) == nil)
    #expect(EVMCall.wrap.payees(to: other, value: 1, chainId: 8453) == [other])
    #expect(EVMCall.decode(wrap + ABI.uint(1)) == .contract(selector: "0xd0e30db0"))
    #expect(EVMCall.decode(unwrap + ABI.uint(1)) == .contract(selector: "0x2e1a7d4d"))
}

/// Tempo's exchange, its fee manager and Safe's factory are called with no one asked about the contract: each takes
/// only the one call ihatecrypto makes there, and the exchange's swap names its amounts and asks like any swap.
@Test func ownCalleesTakeOnlyIhatecryptosOwnCalls() throws {
    let tempo = EVMRPC.tempo.chainId
    let usdc = try #require(Tempo.stables["USDC"]), path = try #require(Tempo.stables["PATHUSD"])
    let swap = try Tempo.swap(usdc, path, units: 5_000_000, minOut: 4_990_000), call = EVMCall.decode(swap)
    #expect(call.swaps)
    #expect(call.summary(chainId: tempo, to: Tempo.exchange, value: 0) == "swap 5 USDC for at least 4.99 PATHUSD on Tempo's exchange, paying this wallet")
    #expect(call.refusal(sender: me, chainId: tempo) == nil)
    #expect(call.unreadTarget(to: Tempo.exchange, value: 0, chainId: tempo) == nil)
    #expect(call.unreadTarget(to: other, value: 0, chainId: tempo) == other)
    // The automation wallet's budget holds it to the approval it counted.
    let pull = try #require(call.pulls(to: Tempo.exchange, data: swap, chainId: tempo))
    #expect(pull.token == usdc && pull.units == 5_000_000)
    #expect(EVMCall.targetRefusal(to: Tempo.exchange, data: swap, sender: me) == nil)
    #expect(EVMCall.targetRefusal(to: Tempo.exchange, data: swap + ABI.uint(1), sender: me) != nil)
    #expect(EVMCall.targetRefusal(to: Tempo.exchange, data: try #require(Data(hex: "deadbeef")) + swap.dropFirst(4), sender: me) != nil)
    // A Uniswap swap's calldata reads as a swap too: at the exchange it would be named one of Tempo's.
    let uniswap = try #require(Data(hex: "04e45aaf")) + (try ABI.address(usdc)) + (try ABI.address(path)) + ABI.uint(500) + (try ABI.address(me)) + ABI.uint(5) + ABI.uint(4) + ABI.uint(0)
    #expect(EVMCall.decode(uniswap).swaps && EVMCall.targetRefusal(to: Tempo.exchange, data: uniswap, sender: me) != nil)

    let fees = try Tempo.setUserToken(EVMRPC.tempoUSDC)
    #expect(EVMCall.targetRefusal(to: Tempo.feeManager, data: fees, sender: me) == nil)
    #expect(EVMCall.targetRefusal(to: Tempo.feeManager, data: try Tempo.setUserToken(path), sender: me) != nil)
    #expect(EVMCall.targetRefusal(to: Tempo.feeManager, data: fees + ABI.uint(0), sender: me) != nil)
    #expect(EVMCall.targetRefusal(to: Tempo.feeManager, data: try Tempo.userToken(me), sender: me) != nil)

    let make = try Safe.create(owners: [me, other], threshold: 1, salt: 1)
    #expect(EVMCall.targetRefusal(to: Safe.factory, data: make, sender: me) == nil)
    var copy = make
    copy.replaceSubrange(16..<36, with: Data(repeating: 9, count: 20))   // another contract's code as the multisig's
    #expect(EVMCall.targetRefusal(to: Safe.factory, data: copy, sender: me) != nil)
    #expect(EVMCall.targetRefusal(to: Safe.factory, data: try #require(Data(hex: "deadbeef")) + make.dropFirst(4), sender: me) != nil)
    // The same bytes anywhere else are that contract's own business.
    #expect(EVMCall.targetRefusal(to: other, data: try #require(Data(hex: "deadbeef")), sender: me) == nil)
}

/// A deposit for a Relay order is the order's own coin and amount, on Solana and by signature as on EVM.
@Test func aRelayDepositIsItsOrdersOwnInput() {
    let mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", tether = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
    func order(_ currency: String, _ amount: String, on chain: String = "solana") -> RelayOrder {
        RelayOrder(id: Data(count: 32), inputChain: chain, inputCurrency: currency, inputAmount: amount, outputChain: "arbitrum", outputCurrency: other,
                   recipient: me, minimumAmount: "1", deadline: 2_000_000_000)
    }
    #expect(order(mint, "600000").inputProblem(coin: mint, units: 600_000) == nil)
    #expect(order(mint, "600000").inputProblem(coin: mint, units: 600_001) != nil)
    #expect(order(mint, "600000").inputProblem(coin: mint, units: 599_999) != nil)
    #expect(order(mint, "600000").inputProblem(coin: tether, units: 600_000) != nil)
    // Solana's own coin is the System program's address in an order.
    #expect(order("11111111111111111111111111111111", "5000000").inputProblem(coin: "SOL", units: 5_000_000) == nil)
    #expect(order("11111111111111111111111111111111", "5000000").inputProblem(coin: mint, units: 5_000_000) != nil)
    #expect(order(mint, "5000000").inputProblem(coin: "SOL", units: 5_000_000) != nil)
    // What the prompt of a deposit says leaves, in the order's own words.
    #expect(order(EVMRPC.baseUSDC, "1500000", on: "base").takes == "1.5 USDC from base")
    #expect(order(mint, "600000").takes == "0.6 USDC from solana")
    #expect(order("11111111111111111111111111111111", "5000000").takes == "0.005 SOL from solana")
    // The payment by signature: the dollar's contract, lowercase, as the order has it.
    #expect(order(EVMRPC.baseUSDC, "1500000", on: "base").inputProblem(coin: EVMRPC.baseUSDC, units: 1_500_000) == nil)
    #expect(order(EVMRPC.baseUSDC, "1500000", on: "base").inputProblem(coin: EVMRPC.baseUSDC, units: 9_000_000) != nil)
}

/// The main key's prompt names a cancel and a leverage change, and the session either opens covers no order: an
/// order's own covers that market's stops, cancels and leverage, and no other market's.
@Test func aCancelOrALeverageChangeNamesItselfAndCoversNoOrder() throws {
    let cancel = HLAction.cancel([(3, 77)]), two = HLAction.cancel([(3, 77), (3, 78)])
    let leverage = MsgPack.map([("type", .string("updateLeverage")), ("asset", .uint(3)), ("isCross", .bool(false)), ("leverage", .uint(5))])
    let order = try HLAction.order([HLOrder(asset: 3, isBuy: true, price: 100, size: 1, reduceOnly: false, kind: .limit(tif: "Ioc"))])
    let elsewhere = try HLAction.order([HLOrder(asset: 4, isBuy: true, price: 100, size: 1, reduceOnly: false, kind: .limit(tif: "Ioc"))])
    #expect(HLGuard.name(cancel) == "cancel an order on Hyperliquid market #3")
    #expect(HLGuard.name(two) == "cancel 2 orders on Hyperliquid market #3")
    #expect(HLGuard.name(leverage) == "set leverage to 5× on Hyperliquid market #3")
    #expect(HLGuard.name(HLAction.twapCancel(asset: 3, id: 9)) == "stop a TWAP on Hyperliquid market #3")
    #expect(HLGuard.name(order) == "sign a Hyperliquid order")
    for quiet in [cancel, leverage, HLAction.twapCancel(asset: 3, id: 9)] {
        #expect(!HLGuard.scope(quiet).isEmpty)
        #expect(!HLGuard.scope(order).isSubset(of: HLGuard.scope(quiet)))
        #expect(HLGuard.scope(quiet).isSubset(of: HLGuard.scope(order)))
        #expect(!HLGuard.scope(quiet).isSubset(of: HLGuard.scope(elsewhere)))
    }
    #expect(!HLGuard.scope(cancel).isSubset(of: HLGuard.scope(leverage)) && !HLGuard.scope(leverage).isSubset(of: HLGuard.scope(cancel)))
    #expect(!HLGuard.scope(order).isSubset(of: HLGuard.scope(elsewhere)))
    // A stop of the same trade rides its entry's prompt, as before.
    let stop = try HLAction.trailingStop(asset: 3, isBuy: false, size: 1, pct: 2)
    #expect(HLGuard.scope(stop).isSubset(of: HLGuard.scope(order)))
}

/// A trailing stop and a TWAP take no fee field: one that came with either would pay whoever it names, on a signature
/// the trading key gives without a prompt.
@Test func aTrailingStopAndATWAPNameNoOneToTakeAFee() throws {
    let owner = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
    let stop = try HLAction.trailingStop(asset: 3, isBuy: false, size: 1, pct: 2)
    let twap = try HLAction.twapOrder(asset: 3, isBuy: true, size: 1, minutes: 30, randomize: false)
    for action in [stop, twap] {
        #expect(HLGuard.check(action, owner: owner, recipients: []).ok)
        guard case .map(let pairs) = action else { Issue.record("not a map"); continue }
        for builder in [HLBuilder.address, "0x000000000000000000000000000000000000dead"] {
            let fee = MsgPack.map(pairs + [("builder", .map([("b", .string(builder)), ("f", .uint(10))]))])
            #expect(!HLGuard.check(fee, owner: owner, recipients: []).ok)
        }
    }
}
