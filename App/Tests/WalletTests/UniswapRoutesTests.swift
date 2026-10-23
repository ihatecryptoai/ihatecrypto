@testable import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import Testing
@testable import TradeExecution
import IhatecryptoCore
import WalletCore

private let me = "0x9858effd232b4033e47d90003d41ec34ecaeda94"
private let baseWETH = "0x4200000000000000000000000000000000000006", aero = "0x940181a94a35a4569e4529a3cdfb74e38fd98631"
private let baseUSDC = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"

/// Uniswap v2's calls byte for byte as Foundry's `cast calldata` writes them (2026-10-05).
@Test func v2CalldataMatchesCast() throws {
    #expect(try UniswapV2.getAmountsOut(1_000_000, path: [baseUSDC, baseWETH, aero]).hex
        == "d06ca61f00000000000000000000000000000000000000000000000000000000000f424000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000003000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda029130000000000000000000000004200000000000000000000000000000000000006000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd98631")
    #expect(try UniswapV2.swapExactTokens(amountIn: 1_000_000, amountOutMin: Decimal(string: "1234567890123456789")!, path: [baseUSDC, aero],
                                          to: me, deadline: 1_791_200_000).hex
        == "5c11d79500000000000000000000000000000000000000000000000000000000000f4240000000000000000000000000000000000000000000000000112210f47de9811500000000000000000000000000000000000000000000000000000000000000a00000000000000000000000009858effd232b4033e47d90003d41ec34ecaeda94000000000000000000000000000000000000000000000000000000006ac38b000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd98631")
    #expect(try UniswapV2.swapExactTokens(amountIn: Decimal(string: "5000000000000000000")!, amountOutMin: 1, path: [aero, baseWETH],
                                          to: me, deadline: 1_791_200_000, toETH: true).hex
        == "791ac9470000000000000000000000000000000000000000000000004563918244f40000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000a00000000000000000000000009858effd232b4033e47d90003d41ec34ecaeda94000000000000000000000000000000000000000000000000000000006ac38b000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd98631000000000000000000000000" + "4200000000000000000000000000000000000006")
    // getAmountsOut's answer: offset, three amounts; the last is what comes out.
    let answer = ABI.uint(0x20) + ABI.uint(3) + ABI.uint(1_000_000) + ABI.uint(400_000_000_000_000) + ABI.uint(777)
    #expect(UniswapV2.amountOut(answer) == 777)
    #expect(UniswapV2.amountOut(ABI.uint(0x20) + ABI.uint(9)) == nil)
}

/// The signer reads every v2 swap form: only this wallet may get what comes out, the router is a spender ihatecrypto knows
/// on each network, and no other contract becomes one.
@Test func signerReadsV2SwapsAndKnowsItsRouters() throws {
    for chain: UInt64 in [8453, 42161, 1, 137, 4663, 10, 999, 56] {
        let v2 = try #require(UniswapV2.on(chain))
        #expect(EVMCall.knownSpenders(chainId: chain).contains(v2.router))
        for toETH in [false, true] {
            let mine = try UniswapV2.swapExactTokens(amountIn: 1, amountOutMin: 1, path: [baseUSDC, aero], to: me, deadline: 1, toETH: toETH)
            #expect(EVMCall.decode(mine) == .swap(recipient: me))
            #expect(EVMCall.decode(mine).refusal(sender: me) == nil)
            let theirs = try UniswapV2.swapExactTokens(amountIn: 1, amountOutMin: 1, path: [baseUSDC, aero], to: baseWETH, deadline: 1, toETH: toETH)
            #expect(EVMCall.decode(theirs).refusal(sender: me) != nil)
        }
    }
    // The plain forms (no fee-on-transfer) share the layout: the recipient is still read and checked.
    var plain = try UniswapV2.swapExactTokens(amountIn: 1, amountOutMin: 1, path: [baseUSDC, aero], to: baseWETH, deadline: 1)
    plain.replaceSubrange(0..<4, with: Data([0x38, 0xed, 0x17, 0x39]))
    #expect(EVMCall.decode(plain) == .swap(recipient: baseWETH) && EVMCall.decode(plain).refusal(sender: me) != nil)
    #expect(!EVMCall.knownSpenders(chainId: 8453).contains(baseWETH))
}

/// Live (IHATECRYPTO_NET=1, reads only): PEPE's deepest pool is Uniswap v2 on Ethereum, so 0.1 WETH of PEPE takes v2's
/// pair, which gives more than v3's; AERO for USDC on Base still finds a route, whichever version wins.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func v2IsQuotedBesideV3AndV4() async throws {
    let e = Executor(wallet: TradeWallet(solana: "", evm: "0x00000000000000000000000000000000000f0a11"), signer: QuoteOnly(),
                     journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("v2live-\(UUID().uuidString).jsonl"))) { _ in }
    let pepe = "0x6982508145454ce325ddbe47a25d4ec3d2311933", weth = "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"
    let uni = try #require(UniswapV3.on(1)), tenth = Decimal(string: "100000000000000000")!
    let v2 = try #require(await e.bestV2(on: .ethereum, from: weth, to: pepe, amountIn: tenth))
    let v3 = await e.bestPath(on: EVMNetwork.ethereum.rpc, uni, from: weth, to: pepe, amountIn: tenth)
    let best = try #require(await e.bestUniswap(on: .ethereum, uni, from: weth, to: pepe, amountIn: tenth))
    print("0.1 WETH → PEPE: v2 \(v2.out) via \(v2.path.count) · v3 \(v3?.out ?? 0) · best \(best.title) \(best.out)\(best.impactText)")
    #expect(v2.path == [weth, pepe] && v2.out > (v3?.out ?? 0))
    #expect(best.v2 == [weth, pepe] && best.title == "Uniswap v2" && best.impact != nil)
    let baseUni = try #require(UniswapV3.on(8453))
    let aeroRoute = try #require(await e.bestUniswap(on: .base, baseUni, from: baseUSDC, to: aero, amountIn: 5_000_000))
    print("5 USDC → AERO on Base: \(aeroRoute.title) \(aeroRoute.out)")
    #expect(aeroRoute.out > 0)
}

private struct QuoteOnly: TradeSigner {
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }
}

/// v4 hooks: a pool may run one only when Uniswap's registry vouches for it on that network (vanillaHooks), its address
/// takes no swap delta and the pool's fee isn't the hook's; the signer applies the same rule, and none at all without the
/// network.
@Test func v4HooksOnlyFromTheRegistryOnTheirOwnNetwork() throws {
    // One the registry lists on Base only: the same address on another network is another contract, maybe not a vanilla one.
    let elsewhere = UniswapV4.vanillaHooks.filter { $0.key != 8453 }.values.reduce(Set<String>()) { $0.union($1) }
    let listed = try #require(UniswapV4.vanillaHooks[8453]?.sorted().first { !elsewhere.contains($0) })
    func key(_ hooks: String, fee: UInt32 = 3000) -> UniswapV4.PoolKey {
        UniswapV4.PoolKey(currency0: baseUSDC, currency1: aero, fee: fee, tickSpacing: 60, hooks: hooks)
    }
    #expect(UniswapV4.allowsHooks(key("0x0000000000000000000000000000000000000000"), chainId: 8453))
    #expect(UniswapV4.allowsHooks(key(listed), chainId: 8453))
    #expect(!UniswapV4.allowsHooks(key(listed, fee: 0x800000), chainId: 8453))           // the hook would set the fee
    #expect(!UniswapV4.allowsHooks(key(listed), chainId: 42161) && !UniswapV4.allowsHooks(key(listed), chainId: 1))
    #expect(!UniswapV4.allowsHooks(key("0x1111111111111111111111111111111111110080"), chainId: 8453))   // not in the registry
    // The permission bits in a hook's address: bit 3 and bit 2 let it take part of a swap.
    #expect(UniswapV4.takesSwapDelta("0x00000000000000000000000000000000000000c8") && UniswapV4.takesSwapDelta("0x0000000000000000000000000000000000000044"))
    #expect(!UniswapV4.takesSwapDelta("0x0000000000000000000000000000000000000080") && UniswapV4.takesSwapDelta("0xzz"))
    // Every listed hook on every network passes the same rules the generator applied.
    for (chain, hooks) in UniswapV4.vanillaHooks {
        #expect(!hooks.isEmpty, "network \(chain)")
        for h in hooks { #expect(h.count == 42 && h == h.lowercased() && !UniswapV4.takesSwapDelta(h)) }
    }
    // The signer: the app's own swap through that pool signs on Base, not elsewhere, and not without the network.
    let swap = try UniswapV4.execute(key(listed), zeroForOne: true, amountIn: 1_000_000, amountOutMinimum: 1, deadline: 1)
    #expect(EVMCall.decode(swap).refusal(sender: me, chainId: 8453) == nil)
    #expect(EVMCall.decode(swap).refusal(sender: me, chainId: 1)?.contains("hooks") == true)
    #expect(EVMCall.decode(swap).refusal(sender: me)?.contains("hooks") == true)
    let unlisted = try UniswapV4.execute(key("0x1111111111111111111111111111111111110080"), zeroForOne: true, amountIn: 1, amountOutMinimum: 1, deadline: 1)
    #expect(EVMCall.decode(unlisted).refusal(sender: me, chainId: 8453)?.contains("hooks") == true)
    #expect(!Batch7702.problems([.init(to: UniswapV4.on(8453)!.router, data: unlisted)], owner: me, chainId: 8453).isEmpty)
}

/// Coins sent with a call: the network's WETH wraps them back to this wallet, a v4 swap of native ETH takes exactly its
/// input; anything else with coins pays its `to`, which the signer then refuses unless approved. WETH's withdraw reads
/// as an unwrap; the prompt says what goes in once.
@Test func signerLetsCoinsGoOnlyBackToThisWallet() throws {
    let wrap = Data([0xd0, 0xe3, 0x0d, 0xb0]), wei: UInt64 = 1_000_000_000_000_000
    #expect(EVMCall.decode(wrap) == .wrap)
    #expect(EVMCall.decode(wrap).payees(to: baseWETH, value: wei, chainId: 8453).isEmpty)
    #expect(EVMCall.decode(wrap).payees(to: aero, value: wei, chainId: 8453) == [aero])              // not this network's WETH
    #expect(EVMCall.decode(wrap).payees(to: baseWETH, value: wei, chainId: 42161) == [baseWETH])     // Base's WETH on Arbitrum
    #expect(EVMCall.decode(wrap).summary(chainId: 8453, to: baseWETH, value: wei) == "wrap 0.001 ETH into WETH on Base")
    let unwrap = Data([0x2e, 0x1a, 0x7d, 0x4d]) + ABI.uint(wei)
    #expect(EVMCall.decode(unwrap) == .unwrap(amount: ABI.uint(wei)))
    #expect(EVMCall.decode(unwrap).payees(to: baseWETH, value: 0, chainId: 8453).isEmpty)
    #expect(EVMCall.decode(unwrap).summary(chainId: 8453, to: baseWETH, value: 0).hasPrefix("unwrap 0.001 WETH"))
    // A v4 swap of native ETH through Base's Universal Router: exactly its input, nothing more, nowhere else.
    let router = try #require(UniswapV4.on(8453)).router
    let native = UniswapV4.PoolKey(currency0: UniswapV4.native, currency1: aero, fee: 3000, tickSpacing: 60, hooks: UniswapV4.native)
    let ethIn = try UniswapV4.execute(native, zeroForOne: true, amountIn: Decimal(wei), amountOutMinimum: 1, deadline: 1)
    #expect(EVMCall.decode(ethIn).payees(to: router, value: wei, chainId: 8453).isEmpty)
    #expect(EVMCall.decode(ethIn).payees(to: router, value: wei + 1, chainId: 8453) == [router])
    #expect(EVMCall.decode(ethIn).payees(to: aero, value: wei, chainId: 8453) == [aero])
    #expect(EVMCall.decode(ethIn).summary(chainId: 8453, to: router, value: wei) == "swap 0.001 ETH on Uniswap v4 on Base, paying this wallet")
    // Coins with a swap that pays in a token: the router would keep them.
    let tokenIn = try UniswapV4.execute(native, zeroForOne: false, amountIn: Decimal(wei), amountOutMinimum: 1, deadline: 1)
    #expect(EVMCall.decode(tokenIn).payees(to: router, value: wei, chainId: 8453) == [router])
    // Coins with any other call still pay its `to`.
    #expect(EVMCall.decode(try UniswapV2.swapExactTokens(amountIn: 1, amountOutMin: 1, path: [baseWETH, aero], to: me, deadline: 1))
        .payees(to: try #require(UniswapV2.on(8453)).router, value: wei, chainId: 8453).count == 1)
}

/// Live (IHATECRYPTO_NET=1, reads only): USDC.e routes to Circle's USDC on Polygon and Arbitrum through Uniswap, near 1:1.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func bridgedUSDCSwapsForCirclesOnUniswap() async throws {
    let e = Executor(wallet: TradeWallet(solana: "", evm: "0x00000000000000000000000000000000000f0a11"), signer: QuoteOnly(),
                     journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("usdce-\(UUID().uuidString).jsonl"))) { _ in }
    for net in [EVMNetwork.polygon, .arbitrum] {
        let token = try await e.evmToken("USDC.e", on: net), uni = try #require(UniswapV3.on(net.rpc.chainId))
        #expect(token.address == Holdings.bridgedUSDC[net.key])
        let route = try #require(await e.bestUniswap(on: net, uni, from: token.address, to: net.usdc, amountIn: 100_000_000))
        print("100 USDC.e → USDC on \(net.name): \(route.out) via \(route.title)\(route.impactText)")
        #expect(route.out > 99_000_000)
    }
}

/// A revoke — an approve of zero, the token's or Permit2's — gives nobody anything: the signer asks no one to be allowed for it;
/// anything above zero to an unknown spender still needs that.
/// approve(spender, 0) is an ERC-20 revoke or an NFT's token id 0 handed over (ERC-721 shares the selector): its spender is
/// asked about like any other (once; a known spender or one already allowed passes). A Permit2 allowance of zero gives nobody anything.
@Test func signerLetsARevokeGoWithoutAnApproval() throws {
    let stranger = "0x1111111111111111111111111111111111111111"
    #expect(EVMCall.decode(try ERC20.approve(spender: stranger, units: 0)).payees(to: baseUSDC, value: 0, chainId: 8453) == [stranger])
    #expect(EVMCall.decode(try ERC20.approve(spender: "0x" + String(repeating: "0", count: 40), units: 0)).payees(to: baseUSDC, value: 0, chainId: 8453).isEmpty)
    #expect(EVMCall.decode(try ERC20.approve(spender: stranger, units: 1)).payees(to: baseUSDC, value: 0, chainId: 8453) == [stranger])
    let p2zero = try UniswapV4.permit2Approve(token: baseUSDC, spender: stranger, amount: 0, expiration: 0)
    #expect(EVMCall.decode(p2zero).payees(to: UniswapV4.permit2, value: 0, chainId: 8453).isEmpty)
    #expect(EVMCall.decode(p2zero).refusal(sender: me, chainId: 8453) == nil)
}

/// Live (IHATECRYPTO_NET=1, reads only, public data): the approvals the test wallet and the main wallet still have,
/// read from each network's Blockscout and the chain; every one listed is above zero.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func approvalsAreReadOnEveryNetwork() async throws {
    for owner in ["0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", "0x90F79bf6EB2c4f870365E785982E1f101E93b906"] {
        let e = Executor(wallet: TradeWallet(solana: "", evm: owner), signer: QuoteOnly(),
                         journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("appr-\(UUID().uuidString).jsonl"))) { _ in }
        let read = await e.approvals()
        print("\(owner.prefix(8)): \(read.list.count) approvals, unread \(read.unread)")
        for a in read.list.prefix(10) { print("  ", a.chain, a.symbol, "→", a.spenderName ?? a.spender, a.unlimited ? "UNLIMITED" : "\(a.amount)", a.permit2 ? "Permit2" : "") }
        #expect(read.list.allSatisfy { $0.amount > 0 })
    }
}

/// The approvals finder reads 7702 batches back into their calls: what executeBatch wrote comes back the same, and an
/// input that isn't one is nil.
@Test func batchCallsReadBack() throws {
    let calls = [Batch7702.Call(to: "0x940181a94a35a4569e4529a3cdfb74e38fd98631", data: try ERC20.approve(spender: "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", units: 5)),
                 Batch7702.Call(to: "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", data: Data([1, 2, 3, 4, 5]))]
    #expect(Batch7702.calls(in: try Batch7702.executeBatch(calls)) == calls)
    #expect(Batch7702.calls(in: Data([0x34, 0xfc, 0xd5, 0xbe, 0])) == nil)
    #expect(Batch7702.calls(in: try ERC20.approve(spender: "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", units: 5)) == nil)
}

/// Circle's CCTP burn as the executor writes it, read back by the signer: the mint's recipient is a payee like any
/// other, a recipient that isn't an EVM address is refused, and the TokenMessenger is a spender ihatecrypto uses.
@Test func cctpBurnIsReadByTheSigner() throws {
    let me = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc"
    let data = try CCTP.burn(units: 5_000_000, to: 3, recipient: me, usdc: EVMNetwork.base.usdc, maxFee: 100_000)
    #expect(data.prefix(4).hex == "779b432d")
    guard case .cctpBurn(let r?, 3, let amount) = EVMCall.decode(data) else { Issue.record("not read as a burn"); return }
    #expect(r == me && ABI.uint(in: amount, word: 0) == 5_000_000)
    #expect(EVMCall.decode(data).payees(to: CCTP.tokenMessenger, value: 0, chainId: 8453) == [me])
    #expect(EVMCall.decode(data).refusal(sender: me, chainId: 8453) == nil)
    var solana = data
    solana.replaceSubrange(4 + 64..<4 + 76, with: Data(repeating: 7, count: 12))
    #expect(EVMCall.decode(solana).refusal(sender: me, chainId: 8453) != nil)
    #expect(EVMCall.knownSpenders(chainId: 8453).contains(CCTP.tokenMessenger))
    #expect(CCTP.domain(42161) == 3 && CCTP.domain(8453) == 6 && CCTP.domain(4663) == nil)
}

/// Live (IHATECRYPTO_NET=1, reads only): what CCTP lands for 20 and 1000 USDC from Base to Arbitrum now.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func cctpIsQuoted() async throws {
    let e = Executor(wallet: TradeWallet(solana: "", evm: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"), signer: QuoteOnly(),
                     journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("c-\(UUID().uuidString).jsonl"))) { _ in }
    for units: UInt64 in [20_000_000, 1_000_000_000] {
        let q = try #require(await e.cctpQuote(from: .base, to: .arbitrum, units: units))
        print("CCTP \(Decimal(units) / 1_000_000) USDC Base → Arbitrum: lands \(q.arrives), cap \(q.maxFee)")
        #expect(q.arrives > Decimal(units) / 1_000_000 * Decimal(string: "0.99")! && q.maxFee < units / 100)
    }
}

/// Aave's borrow and repay as the signer reads them: only this wallet's own debt; and the health-factor arithmetic the
/// card shows (Aave's: collateral × weighted threshold ÷ debt; liquidation where that reaches 1).
@Test func aaveBorrowIsReadAndPriced() throws {
    let me = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc", other = "0x" + String(repeating: "12", count: 20)
    let borrow = try AaveV3.borrow(asset: EVMRPC.baseUSDC, amount: 5_000_000, onBehalfOf: me)
    #expect(borrow.prefix(4).hex == "a415bcad" && EVMCall.decode(borrow).refusal(sender: me, chainId: 8453) == nil)
    #expect(EVMCall.decode(try AaveV3.borrow(asset: EVMRPC.baseUSDC, amount: 1, onBehalfOf: other)).refusal(sender: me, chainId: 8453) != nil)
    #expect(EVMCall.decode(try AaveV3.repay(asset: EVMRPC.baseUSDC, amount: nil, onBehalfOf: other)).refusal(sender: me, chainId: 8453) != nil)
    #expect(EVMCall.decode(try AaveV3.repay(asset: EVMRPC.baseUSDC, amount: nil, onBehalfOf: me)).payees(to: AaveV3.basePool, value: 0, chainId: 8453).isEmpty)
    // $3000 of ETH (1 ETH), threshold 0.83, $1000 borrowed: health 2.49; liquidation when ETH is $1204.82.
    #expect(AaveAccount.health(collateral: 3000, debt: 1000, threshold: Decimal(string: "0.83")!) == Decimal(string: "2.49")!)
    let liq = try #require(AaveAccount.liquidationPrice(debt: 1000, threshold: Decimal(string: "0.83")!, collateral: 3000, amount: 1, price: 3000))
    #expect(liq > Decimal(string: "1204.81")! && liq < Decimal(string: "1204.83")!)
    // $2000 of USDC beside the ETH covers the debt on its own: no ETH price liquidates it.
    #expect(AaveAccount.liquidationPrice(debt: 1000, threshold: Decimal(string: "0.83")!, collateral: 5000, amount: 1, price: 3000) == nil)
}
