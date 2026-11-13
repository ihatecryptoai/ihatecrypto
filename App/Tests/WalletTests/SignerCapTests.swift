import EVMChain
import Foundation
import SignerAPI
import SolanaChain
import Testing
import TxGuard
import WalletCore

/// The card's limits at the signer: what the guard reads leaving a transfer fits the card, and the same transfer with a
/// doubled amount or another approved payee doesn't — the gaps the 2026-11-13 attack drill left to the prompt alone.
@Test func capHoldsATransferToItsCard() throws {
    let me = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    let them = Base58.encode(Data(repeating: 7, count: 32)), other = Base58.encode(Data(repeating: 9, count: 32))
    func flows(to owner: String, units: UInt64) throws -> [(asset: String, units: Decimal, to: String?)] {
        let wire = SolanaTransaction.unsigned(try SPL.transferMessage(payer: me, to: owner, mint: usdc, amount: units, decimals: 6, blockhash: me))
        let m = try SolanaMessage.parse(transaction: wire)
        let owners = [try SPL.associatedAccount(owner: me, mint: usdc): me, try SPL.associatedAccount(owner: owner, mint: usdc): owner]
        let v = SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: me, recipients: [them, other]), tokenOwners: owners)
        #expect(v.ok)
        return v.outflows.map { ($0.asset, Decimal($0.units), $0.to) }
    }
    let cap = SignerCap(asset: usdc, units: 500_000 as UInt64, payee: them, card: "0.5 USDC to Them")
    #expect(cap.refusal(try flows(to: them, units: 500_000)) == nil)
    #expect(cap.refusal(try flows(to: them, units: 499_999)) == nil)
    #expect(cap.refusal(try flows(to: them, units: 1_000_000))?.contains("at most 500000") == true)
    #expect(cap.refusal(try flows(to: other, units: 500_000))?.contains(other) == true)
}

@Test func capHoldsASwapAndOtherCoins() {
    let usdc = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
    let swap = SignerCap(asset: usdc.lowercased(), units: Decimal(100_000), payee: nil, card: "0.1 USDC → SOL")
    #expect(swap.refusal([(usdc, 100_000, nil)]) == nil)
    #expect(swap.refusal([(usdc, 100_000, "0x" + String(repeating: "ab", count: 20))]) != nil)
    #expect(swap.refusal([(usdc, 100_000, nil), ("native", 1, "0x" + String(repeating: "ab", count: 20))])?.contains("moves only") == true)
    #expect(swap.refusal([]) != nil)
    let send = SignerCap(asset: "native", units: UInt64(5), payee: "0xABCDEF0000000000000000000000000000000001", card: "5 wei")
    #expect(send.refusal([("native", 5, "0xabcdef0000000000000000000000000000000001")]) == nil)
    #expect(SignerCap(asset: "SOL", units: "lots", payee: nil, card: "?").refusal([]) != nil)
}

private let me = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94", dead = "0x000000000000000000000000000000000000dEaD"

/// What the signer reads leaving from EVM calls under a card (EVMCall.cardFlows), and the card's verdict on it.
private func held(_ cap: SignerCap, _ chain: UInt64, _ calls: [(String, UInt64, Data)], order: RelayOrder? = nil) -> String? {
    var flows: [(asset: String, units: Decimal, to: String?)] = []
    if let why = EVMCall.cardFlows(chain, calls.map { (to: $0.0, value: $0.1, data: $0.2) }, payee: cap.payee, owner: me,
                                   order: { id in order?.id == id ? order : nil }, into: &flows) { return "call: " + why }
    return cap.refusal(flows)
}

/// An EVM swap's card: the right given to a router ihatecrypto knows counts in full, its call only beside that right.
@Test func capHoldsAnEVMSwapByItsRight() throws {
    let usdc = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", router = try #require(UniswapV3.on(42161)).router
    let cap = SignerCap(asset: usdc.lowercased(), units: 1_000_000 as UInt64, payee: nil, card: "1 USDC → ARB")
    let swap = try UniswapV3.exactInput(path: Data(count: 43), recipient: me, amountIn: 5_000_000, amountOutMinimum: 1)
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: router, units: 1_000_000))]) == nil)
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: router, units: 2_000_000))])?.contains("at most 1000000") == true)
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: dead, units: 1))])?.hasPrefix("call:") == true)
    // In one batch the swap pulls only what the right allows, whatever its own amount says.
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: router, units: 1_000_000)), (router, 0, swap)]) == nil)
    // Alone, or to another router than the one given the right, it can't be counted.
    #expect(held(cap, 42161, [(router, 0, swap)])?.hasPrefix("call:") == true)
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: KyberSwap.router, units: 1_000_000)), (router, 0, swap)])?.hasPrefix("call:") == true)
    // Permit2's own right is the road to the router's: counted once.
    let v4 = try #require(UniswapV4.on(42161)).router
    let road = [(usdc, UInt64(0), try ERC20.approve(spender: UniswapV4.permit2, units: 1_000_000)),
                (UniswapV4.permit2, UInt64(0), try UniswapV4.permit2Approve(token: usdc, spender: v4, amount: 1_000_000, expiration: 1))]
    #expect(held(cap, 42161, road) == nil)
    // The larger of the two rights counts, and the router is reached through Permit2's right alone.
    let wide = [(usdc, UInt64(0), try ERC20.approve(spender: UniswapV4.permit2, units: 2_000_000)), road[1]]
    #expect(held(cap, 42161, wide)?.contains("at most") == true)
    #expect(held(cap, 42161, [road[0], (v4, 0, Data([0x35, 0x93, 0x56, 0x4c]))]) == nil)
    // A transfer card gives no swap.
    let send = SignerCap(asset: usdc.lowercased(), units: 1_000_000 as UInt64, payee: me, card: "1 USDC to me")
    #expect(held(send, 42161, [(usdc, 0, try ERC20.approve(spender: router, units: 1_000_000)), (router, 0, swap)])?.hasPrefix("call:") == true)
}

/// A bridge's card: the burn or deposit pays the card's payee, at most its amount, the right before it counted once.
@Test func capHoldsABridgeToItsPayee() throws {
    let usdc = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
    let cap = SignerCap(asset: usdc.lowercased(), units: 1_000_000 as UInt64, payee: me, card: "1 USDC to Base")
    func burn(_ units: UInt64, to r: String) throws -> Data { try CCTP.burn(units: units, to: 6, recipient: r, usdc: usdc, maxFee: 100) }
    #expect(held(cap, 42161, [(usdc, 0, try ERC20.approve(spender: CCTP.tokenMessenger, units: 1_000_000)), (CCTP.tokenMessenger, 0, try burn(1_000_000, to: me))]) == nil)
    #expect(held(cap, 42161, [(CCTP.tokenMessenger, 0, try burn(1_000_000, to: me))]) == nil)
    #expect(held(cap, 42161, [(CCTP.tokenMessenger, 0, try burn(2_000_000, to: me))])?.contains("at most") == true)
    #expect(held(cap, 42161, [(CCTP.tokenMessenger, 0, try burn(1_000_000, to: dead))])?.contains(dead.lowercased()) == true)
    // A right given after the burn doesn't bound it: the burn ran on a right left from before.
    #expect(held(cap, 42161, [(CCTP.tokenMessenger, 0, try burn(1_000_000, to: me)), (usdc, 0, try ERC20.approve(spender: CCTP.tokenMessenger, units: 1))])?.contains("at most") == true)

    // Relay's deposit on Base for an order the signer holds: its payee is the order's (a Solana address here).
    let json = #"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0xff566540576a54aeb2c99679f6fb8af8a8b306110ce0b8e10bcadd3585022963","inputs":[{"payment":{"chainId":"base","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","amount":"5000000","weight":"1"},"refunds":[{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396849,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},{"chainId":"solana","recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"0","deadline":1793396849,"extraData":"0x"}]}],"output":{"chainId":"solana","payments":[{"recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"4878735","expectedAmount":"4978301"}],"calls":[],"deadline":1793396849,"extraData":"0x"},"fees":[]}"#
    let order = try RelayOrder.read(Data(json.utf8), evm: me, solana: "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", now: 1_700_000_000)
    let base = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", depository = RelayMulticallPermit.depository
    let deposit = Data([0xe8, 0x01, 0x79, 0x52]) + (try ABI.address(me)) + (try ABI.address(base))
        + (try ABI.word(Decimal(5_000_000))) + order.id
    let toSolana = SignerCap(asset: base, units: 5_000_000 as UInt64, payee: "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", card: "5 USDC to Solana")
    #expect(held(toSolana, 8453, [(depository, 0, deposit)], order: order) == nil)
    #expect(held(toSolana, 8453, [(depository, 0, deposit)])?.hasPrefix("call:") == true)
    let elsewhere = SignerCap(asset: base, units: 5_000_000 as UInt64, payee: me, card: "5 USDC to me")
    #expect(held(elsewhere, 8453, [(depository, 0, deposit)], order: order) != nil)
}
