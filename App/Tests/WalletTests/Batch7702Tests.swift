@testable import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import Testing
@testable import TradeExecution
import WalletCore

/// anvil's first well-known key (public test vector, never funded by ihatecrypto).
private let key = Data(hex: "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")!
private let owner = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
private let baseUSDC = EVMRPC.baseUSDC
private let router = "0x2626664c2603336e57b271c5c0b26f421741e481"
private let approve = try! ERC20.approve(spender: router, units: 1_500_000)
private let calls = [Batch7702.Call(to: baseUSDC, data: approve), Batch7702.Call(to: router, data: Data(hex: "1234567890")!)]

/// Foundry's cast built the same batch (calldata), authorization (wallet sign-auth) and type-4 transaction (mktx), byte for byte.
@Test func batchCalldataAuthorizationAndTransactionMatchCast() throws {
    #expect(try Batch7702.executeBatch(calls).hex == "34fcd5be0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000120000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000044095ea7b30000000000000000000000002626664c2603336e57b271c5c0b26f421741e481000000000000000000000000000000000000000000000000000000000016e360000000000000000000000000000000000000000000000000000000000000000000000000000000002626664c2603336e57b271c5c0b26f421741e4810000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000051234567890000000000000000000000000000000000000000000000000000000")
    let digest = Batch7702.authorizationDigest(chainId: 8453, nonce: 8)
    #expect(digest.hex == "bc6cf1a2532c5a9d49f8046500eddc3dfd84869cb6c4e3ee29dde65214be1ddb")
    let auth = try EVMSigner.sign(digest: digest, privateKey: key)
    #expect(auth.v == 28 && auth.r.hex == "2051bf111c51e7d98ef0e135f5ab9c6d9964aca8d13c6aa5dbf726eddba9cdb9"
            && auth.s.hex == "6b630c1fab85719012adc4843da6cc2d65348b61af8a052ef4f174437d1f3b48")
    // Nonce 7 for the transaction, 8 for the authorization: the sender's nonce goes up first.
    let tx = try Batch7702.signed(chainId: 8453, nonce: 7, maxPriorityFee: 1_000_000, maxFee: 2_000_000, gasLimit: 200_000, owner: owner,
                                  calls: calls, privateKey: key)
    #expect(tx.raw.hex == "04f902d182210507830f4240831e848083030d4094f39fd6e51aad88f6f4ce6ab8827279cfffb9226680b9020434fcd5be0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000120000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000044095ea7b30000000000000000000000002626664c2603336e57b271c5c0b26f421741e481000000000000000000000000000000000000000000000000000000000016e360000000000000000000000000000000000000000000000000000000000000000000000000000000002626664c2603336e57b271c5c0b26f421741e4810000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000051234567890000000000000000000000000000000000000000000000000000000c0f85ef85c822105944cd241e8d1510e30b2076397afc7508ae59c66c90801a02051bf111c51e7d98ef0e135f5ab9c6d9964aca8d13c6aa5dbf726eddba9cdb9a06b630c1fab85719012adc4843da6cc2d65348b61af8a052ef4f174437d1f3b4880a0f331a0811039069fa235f2cb9bc5cdb2b29077a43ddb57f323619dd1efc4f8dba030e48527c916ef3437c96d164b974932232dc52622ae25371872c330dc329686")
}

@Test func batchRulesRefuseWhatOneCallWouldBeRefusedAndMore() throws {
    let swap = try UniswapV3.exactInput(path: try UniswapV3.path([baseUSDC, "0x4200000000000000000000000000000000000006"], fees: [500]),
                                        recipient: owner, amountIn: 1_500_000, amountOutMinimum: 1)
    let good = [Batch7702.Call(to: baseUSDC, data: approve), Batch7702.Call(to: router, data: swap)]
    #expect(Batch7702.problems(good, owner: owner, chainId: 8453).isEmpty)
    for chain: UInt64 in [1, 137, 42161, 8453, 4663, 10] { #expect(Batch7702.problems(good, owner: owner, chainId: chain).isEmpty) }
    // Networks without the audited account or Prague: Tempo, HyperEVM, and chain 0 (an authorization good on every network).
    for chain: UInt64 in [4217, 999, 56, 0] { #expect(!Batch7702.problems(good, owner: owner, chainId: chain).isEmpty) }
    // A call into the wallet itself would run the batch account's executeBatch with calls nobody read.
    #expect(!Batch7702.problems(good + [.init(to: owner.lowercased(), data: Data([1, 2, 3, 4]))], owner: owner, chainId: 8453).isEmpty)
    // The one-call rules hold inside: an unlimited approve, a swap paying someone else.
    let unlimited = try ERC20.approve(spender: router, units: Decimal(string: "1000000000000000000000000000000")!)
    #expect(!Batch7702.problems([.init(to: baseUSDC, data: unlimited)] + good.suffix(1), owner: owner, chainId: 8453).isEmpty)
    let thief = "0x1111111111111111111111111111111111111111"
    let stolen = try UniswapV3.exactInput(path: try UniswapV3.path([baseUSDC, "0x4200000000000000000000000000000000000006"], fees: [500]),
                                          recipient: thief, amountIn: 1_500_000, amountOutMinimum: 1)
    #expect(!Batch7702.problems(good.prefix(1) + [.init(to: router, data: stolen)], owner: owner, chainId: 8453).isEmpty)
    // Empty, or more calls than any move makes.
    #expect(!Batch7702.problems([], owner: owner, chainId: 8453).isEmpty)
    #expect(!Batch7702.problems(Array(repeating: good[0], count: Batch7702.maxCalls + 1), owner: owner, chainId: 8453).isEmpty)
    // A plain transaction into the wallet's own address with calldata is refused too; a plain coin transfer to itself isn't.
    #expect(Batch7702.selfCall(to: owner.lowercased(), data: Data([0x34, 0xfc, 0xd5, 0xbe]), owner: owner) != nil)
    #expect(Batch7702.selfCall(to: owner, data: Data(), owner: owner) == nil)
    #expect(Batch7702.selfCall(to: router, data: swap, owner: owner) == nil)
}

/// Uniswap v4 off Robinhood: on each network its Universal Router 2.1.2 and Permit2 are spenders ihatecrypto knows, and the
/// three calls of a v4 buy (approve to Permit2, Permit2's approve to the router, the swap paying this wallet) pass the
/// signer's batch rules with no payee to approve; a swap that pays someone else doesn't.
@Test func v4BuyOnEveryNetworkPassesTheSignersRules() throws {
    let key = UniswapV4.PoolKey(currency0: baseUSDC, currency1: "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf", fee: 500, tickSpacing: 10,
                                hooks: "0x0000000000000000000000000000000000000000")
    for chain: UInt64 in [8453, 42161, 1, 137, 4663, 10] {
        let v4 = try #require(UniswapV4.on(chain))
        #expect(EVMCall.knownSpenders(chainId: chain).isSuperset(of: [v4.router, UniswapV4.permit2]))
        let calls = [Batch7702.Call(to: baseUSDC, data: try ERC20.approve(spender: UniswapV4.permit2, units: 4_000_000)),
                     .init(to: UniswapV4.permit2, data: try UniswapV4.permit2Approve(token: baseUSDC, spender: v4.router, amount: 4_000_000,
                                                                                    expiration: UInt64(Date().timeIntervalSince1970) + 1800)),
                     .init(to: v4.router, data: try UniswapV4.execute(key, zeroForOne: true, amountIn: 4_000_000, amountOutMinimum: 4_000,
                                                                     deadline: UInt64(Date().timeIntervalSince1970) + 600))]
        #expect(Batch7702.problems(calls, owner: owner, chainId: chain).isEmpty)
        #expect(calls.flatMap { EVMCall.decode($0.data).payees(to: $0.to, value: 0, chainId: chain) }.isEmpty, "chain \(chain)")
    }
    // Permit2's approve to a router ihatecrypto doesn't use names that spender as a payee: approved in person or refused.
    let stranger = "0x1111111111111111111111111111111111111111"
    let loose = try UniswapV4.permit2Approve(token: baseUSDC, spender: stranger, amount: 1, expiration: UInt64(Date().timeIntervalSince1970) + 60)
    #expect(EVMCall.decode(loose).payees(to: UniswapV4.permit2, value: 0, chainId: 8453) == [stranger])
}

/// The floor under a Uniswap swap follows its price impact: half a percent in a deep pool, up to 3% in a thin one.
@Test func swapToleranceFollowsPriceImpact() {
    let d = { Decimal(string: $0)! }
    #expect(UniRoute.tolerance(impact: nil) == d("0.01"))
    #expect(UniRoute.tolerance(impact: d("0.0004")) == d("0.005"))
    #expect(UniRoute.tolerance(impact: d("0.012")) == d("0.01"))
    #expect(UniRoute.tolerance(impact: d("0.03")) == d("0.025"))
    #expect(UniRoute.tolerance(impact: d("0.2")) == d("0.03"))
    var deep = UniRoute(v3: (Data(), 1), v4: nil, out: 1, impact: d("0.0004"))
    #expect(deep.impactText.isEmpty)
    deep.impact = d("0.012")
    #expect(deep.impactText == " · price impact 1.2%")
}

/// Batching waits for the user's switch (Settings › Wallet, off by default): off, approve and call go as before.
@Test func batchingIsTheUsersChoiceAndOffByDefault() async {
    let saved = UserDefaults.standard.object(forKey: Executor.batchKey)
    defer { UserDefaults.standard.set(saved, forKey: Executor.batchKey) }
    UserDefaults.standard.removeObject(forKey: Executor.batchKey)
    let e = Executor(wallet: TradeWallet(solana: "", evm: owner), signer: ForkSigner(key: key, address: owner),
                     journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("switch-\(UUID().uuidString).jsonl"))) { _ in }
    #expect(!Executor.batchesOn)
    #expect(await !e.canBatch(.base))
    UserDefaults.standard.set(true, forKey: Executor.batchKey)
    #expect(await e.canBatch(.base))
    #expect(await e.canBatch(.robinhood))
    #expect(await !e.canBatch(.tempo))
}

/// A user operation for ERC-4337's EntryPoint: once delegated, the account runs whatever it says, so it's never signed,
/// whatever the site names its domain or type.
@Test func userOperationsAreNeverSigned() throws {
    func typed(_ name: String, _ primary: String, _ contract: String) throws -> TypedData {
        try TypedData.parse(Data(#"""
        {"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],
         "\#(primary)":[{"name":"sender","type":"address"},{"name":"nonce","type":"uint256"},{"name":"initCode","type":"bytes"},{"name":"callData","type":"bytes"},{"name":"accountGasLimits","type":"bytes32"},{"name":"preVerificationGas","type":"uint256"},{"name":"gasFees","type":"bytes32"},{"name":"paymasterAndData","type":"bytes"}]},
         "primaryType":"\#(primary)",
         "domain":{"name":"\#(name)","version":"1","chainId":8453,"verifyingContract":"\#(contract)"},
         "message":{"sender":"\#(owner)","nonce":"0","initCode":"0x","callData":"0x34fcd5be","accountGasLimits":"0x0000000000000000000000000000000000000000000000000000000000000000","preVerificationGas":"0","gasFees":"0x0000000000000000000000000000000000000000000000000000000000000000","paymasterAndData":"0x"}}
        """#.utf8))
    }
    #expect(try typed("ERC4337", "PackedUserOperation", "0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108").refusal != nil)
    #expect(try typed("Totally Legit Login", "Login", "0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108").refusal != nil)
    #expect(try typed("ERC4337", "Login", "0x1111111111111111111111111111111111111111").refusal != nil)
    #expect(try typed("Some App", "PackedUserOperation", "0x1111111111111111111111111111111111111111").refusal != nil)
}

// MARK: - Fork run

/// Signs with a throwaway key under the same rules as the signing process (Signer.swift), for the fork run.
struct ForkSigner: TradeSigner {
    let key: Data
    let address: String
    var batches = true
    var signsBatches: Bool { batches }

    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }

    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data {
        let call = EVMCall.decode(data)
        if let why = call.refusal(sender: address, chainId: chainId) ?? Batch7702.selfCall(to: to, data: data, owner: address) { throw ExecutionError(why) }
        if let payee = call.payees(to: to, value: 0, chainId: chainId).first(where: { $0.lowercased() != address.lowercased() }) { throw ExecutionError("pays \(payee)") }
        return try EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee, gasLimit: gasLimit,
                                  to: to, value: 0, data: data).signed(privateKey: key).raw
    }

    /// Coins sent along, under the signing process's rules: only back to this wallet (a wrap, a native v4 swap).
    func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, value: UInt64,
                        data: Data, recipients: [String]) async throws -> Data {
        let call = EVMCall.decode(data)
        if let why = call.refusal(sender: address, chainId: chainId) ?? Batch7702.selfCall(to: to, data: data, owner: address) { throw ExecutionError(why) }
        if let payee = call.payees(to: to, value: value, chainId: chainId).first(where: { $0.lowercased() != address.lowercased() }) { throw ExecutionError("pays \(payee)") }
        return try EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee, gasLimit: gasLimit,
                                  to: to, value: value, data: data).signed(privateKey: key).raw
    }

    func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                      calls: [Batch7702.Call], recipients: [String]) async throws -> Data {
        if let why = Batch7702.problems(calls, owner: address, chainId: chainId).first { throw ExecutionError(why) }
        for c in calls {
            if let payee = EVMCall.decode(c.data).payees(to: c.to, value: 0, chainId: chainId).first(where: { $0.lowercased() != address.lowercased() }) {
                throw ExecutionError("pays \(payee)")
            }
        }
        return try Batch7702.signed(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee, gasLimit: gasLimit,
                                    owner: address, calls: calls, privateKey: key).raw
    }
}

/// The executor's own approve + Uniswap swap on an anvil fork of Base (App/Tools/fork7702.sh starts it and funds a fresh
/// key): one type-4 transaction, the wallet delegated to the audited account, the allowance spent to zero; a second buy
/// is one transaction again; a signer that doesn't batch still works on the now-delegated wallet, in two.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL"] != nil))
func forkApproveAndSwapGoAsOneTransaction() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let chain = UInt64(env["IHATECRYPTO_ANVIL_CHAIN"] ?? "8453")!
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: chain)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let uni = try #require(UniswapV3.on(chain)), weth = uni.weth
    let baseUSDC = try #require(EVMNetwork.of(chainId: chain)).usdc
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("fork7702-\(UUID().uuidString).jsonl"))
    func executor(batches: Bool) -> Executor {
        Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me, batches: batches), journal: journal) { print("·", $0) }
    }
    func buy(_ units: UInt64, with e: Executor) async throws -> String {
        let best = try #require(await e.bestPath(on: rpc, uni, from: baseUSDC, to: weth, amountIn: Decimal(units)))
        return try await e.approveAndSend(rpc, token: baseUSDC, spender: uni.router, units: Decimal(units), approveNote: "Approve exactly USDC to Uniswap",
                                          to: uni.router, data: try UniswapV3.exactInput(path: best.path, recipient: me, amountIn: Decimal(units),
                                                                                          amountOutMinimum: (best.out * Decimal(string: "0.99")!).whole),
                                          note: "Buy WETH on Uniswap")
    }
    func allowance() async throws -> Decimal { ABI.decimal(in: try await rpc.read(baseUSDC, try ERC20.allowance(owner: me, spender: uni.router)), word: 0) ?? -1 }
    func receiptType(_ hash: String) async throws -> String { ((try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any])?["type"] as? String ?? "?" }

    // The wrapped coin as a Decimal: 3 USDC buys ~20 WPOL on Polygon, past tokenBalance's 64 bits.
    let usdc0 = try await rpc.tokenBalance(baseUSDC, owner: me), weth0 = try await executor(batches: true).balance(weth, on: rpc)
    let nonce0 = try await rpc.nonce(me)
    // 1. The first buy: approve + swap as one type-4 transaction. The nonce goes up by two: the transaction's, then the
    // authorization's (EIP-7702 bumps the authority's nonce when it applies one) — which shows the delegation took.
    let first = try await buy(3_000_000, with: executor(batches: true))
    #expect(try await rpc.nonce(me) == nonce0 + 2)
    #expect(try await receiptType(first) == "0x4")
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == usdc0 - 3_000_000)
    #expect(try await executor(batches: true).balance(weth, on: rpc) > weth0)
    #expect(try await allowance() == 0)
    let code = (try await rpc.call("eth_getCode", [me, "latest"])) as? String
    #expect(code?.lowercased() == "0xef0100" + Batch7702.delegate.dropFirst(2))
    // 2. A second buy: one transaction again, the delegation kept.
    let second = try await buy(2_000_000, with: executor(batches: true))
    #expect(try await rpc.nonce(me) == nonce0 + 4)
    #expect(try await receiptType(second) == "0x4")
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == usdc0 - 5_000_000)
    #expect(try await allowance() == 0)
    // 3. A signer that doesn't batch (a Ledger): approve, then swap, two type-2 transactions from the delegated wallet.
    let third = try await buy(1_000_000, with: executor(batches: false))
    #expect(try await rpc.nonce(me) == nonce0 + 6)
    #expect(try await receiptType(third) == "0x2")
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == usdc0 - 6_000_000)
    // 4. A swap that would fail (more USDC than the wallet has) stops at the dry run: nothing signed, nonce unchanged.
    let held = try await rpc.tokenBalance(baseUSDC, owner: me)
    await #expect(throws: (any Error).self) { try await buy(held + 1_000_000, with: executor(batches: true)) }
    #expect(try await rpc.nonce(me) == nonce0 + 6)
    // 5. The delegated wallet takes coins as before (a gas top-up, a withdrawal): the batch account's receive().
    let friend = "0x00000000000000000000000000000000000f0a11"
    _ = try await rpc.call("anvil_setBalance", [friend, "0xde0b6b3a7640000"])
    _ = try await rpc.call("anvil_impersonateAccount", [friend])
    let eth0 = try await rpc.balance(me)
    let sent = try #require(try await rpc.call("eth_sendTransaction", [["from": friend, "to": me, "value": "0x38d7ea4c68000"]]) as? String)
    #expect(try await rpc.waitForReceipt(sent))
    #expect(try await rpc.balance(me) == eth0 + 1_000_000_000_000_000)
    // 6. The signer refuses an unlimited approve inside a batch.
    let unlimited = try ERC20.approve(spender: uni.router, units: Decimal(string: "1000000000000000000000000000000")!)
    await #expect(throws: (any Error).self) {
        try await ForkSigner(key: key, address: me).signEVMBatch(chainId: chain, nonce: 0, maxPriorityFee: 1, maxFee: 1, gasLimit: 100_000,
                                                                 calls: [.init(to: baseUSDC, data: unlimited), .init(to: uni.router, data: Data([1]))], recipients: [])
    }
}

/// KyberSwap's route on an anvil fork of Base (Tools/fork7702.sh): its API builds the call for the fork's fresh key, the
/// executor reads it back (this wallet, these tokens, this amount, its floor), approves exactly that to the router and
/// swaps — every call held to the signer's rules (ForkSigner) — and the allowance is spent to zero.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL"] != nil && (ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] ?? "8453") == "8453"))
func forkKyberSwapPaysThisWallet() async throws {
    let env = ProcessInfo.processInfo.environment
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let b = EVMNetwork.base
    let net = EVMNetwork(name: b.name, key: b.key, rpc: rpc, usdc: b.usdc, relayChain: b.relayChain, explorer: b.explorer, gasFloor: b.gasFloor, topupUSD: b.topupUSD)
    let aero = EVMTokens.Token(symbol: "AERO", name: "Aerodrome", address: "0x940181a94a35a4569e4529a3cdfb74e38fd98631", decimals: 18)
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forkkyber-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    let usdc0 = try await rpc.tokenBalance(b.usdc, owner: me), aero0 = try await e.balance(aero.address, on: rpc)
    let route = try await KyberSwap.route(chainId: 8453, from: b.usdc, to: aero.address, amount: 2_000_000)
    print(try await e.kyberSwap(net, route, tokenIn: Executor.usdcToken(net), tokenOut: aero, units: 2_000_000, what: "2 USDC"))
    #expect(try await rpc.tokenBalance(b.usdc, owner: me) == usdc0 - 2_000_000)
    #expect(try await e.balance(aero.address, on: rpc) - aero0 >= (route.out * (1 - UniRoute.tolerance(impact: route.impact))).whole)
    let allowance = ABI.decimal(in: try await rpc.read(b.usdc, try ERC20.allowance(owner: me, spender: KyberSwap.router)), word: 0)
    #expect(allowance == 0)
}

/// Robinhood Chain's Uniswap v4 buy on an anvil fork (NET=robinhood Tools/fork7702.sh): the approve to Permit2,
/// Permit2's approve to the router and the swap go as one type-4 transaction; a second buy within Permit2's half hour is
/// the swap alone when nothing needs approving.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "4663"))
func forkRobinhoodV4SwapIsOneTransaction() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 4663)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let usdg = EVMRPC.robinhoodUSDG, ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"
    let key4 = UniswapV4.PoolKey(currency0: ai, currency1: usdg, fee: 2300, tickSpacing: 23, hooks: "0x0000000000000000000000000000000000000000")
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("fork7702-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    func buy(_ units: Decimal) async throws -> String {
        let quote = try await rpc.read(UniswapV4.robinhoodQuoter, try UniswapV4.quoteExactInputSingle(key4, zeroForOne: false, amount: units))
        let out = try #require(ABI.decimal(in: quote, word: 0))
        return try await e.v4Swap((key4, false, out), tokenIn: usdg, units: units, minOut: (out * 99 / 100).rounded(0, .down), what: "USDG",
                                  note: "Buy AI on Uniswap v4", rpc: rpc)
    }
    func receiptType(_ hash: String) async throws -> String { ((try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any])?["type"] as? String ?? "?" }
    // AI has 18 decimals: its balance doesn't fit tokenBalance's 64 bits, so it's read as a Decimal.
    let usdg0 = try await rpc.tokenBalance(usdg, owner: me), ai0 = try await e.balance(ai, on: rpc), nonce0 = try await rpc.nonce(me)
    let first = try await buy(5_000_000)
    #expect(try await receiptType(first) == "0x4")
    #expect(try await rpc.nonce(me) == nonce0 + 2)   // the transaction and the authorization
    #expect(try await rpc.tokenBalance(usdg, owner: me) == usdg0 - 5_000_000)
    #expect(try await e.balance(ai, on: rpc) > ai0)
    let toPermit2 = ABI.decimal(in: try await rpc.read(usdg, try ERC20.allowance(owner: me, spender: UniswapV4.permit2)), word: 0)
    #expect(toPermit2 == 0)   // the exact approve, spent by the swap
    let code = (try await rpc.call("eth_getCode", [me, "latest"])) as? String
    #expect(code?.lowercased() == "0xef0100" + Batch7702.delegate.dropFirst(2))
    // Again: the approve to Permit2 and Permit2's own are needed again (both spent), so one batch again.
    let second = try await buy(3_000_000)
    #expect(try await receiptType(second) == "0x4")
    #expect(try await rpc.nonce(me) == nonce0 + 4)
    #expect(try await rpc.tokenBalance(usdg, owner: me) == usdg0 - 8_000_000)
}

/// Aave's supply, Morpho's deposit and Relay's bridge deposit, each with its exact approve, on an anvil fork of Base
/// (Tools/fork7702.sh): one type-4 transaction each, the allowance spent to zero, the position or the deposit there.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "8453"))
func forkDepositsGoAsOneTransaction() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("fork7702-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    func receiptType(_ hash: String) async throws -> String { ((try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any])?["type"] as? String ?? "?" }
    func allowance(_ spender: String) async throws -> Decimal { ABI.decimal(in: try await rpc.read(baseUSDC, try ERC20.allowance(owner: me, spender: spender)), word: 0) ?? -1 }
    func held(_ token: String) async throws -> Decimal { try await e.balance(token, on: rpc) }

    // Aave v3: approve + supply.
    let aave = AaveV3.base
    let a0 = try await held(aave.aUSDC), u0 = try await rpc.tokenBalance(baseUSDC, owner: me)
    let supplied = try await e.approveAndSend(rpc, token: baseUSDC, spender: aave.pool, units: 3_000_000, approveNote: "Approve exactly 3 USDC to Aave",
                                              to: aave.pool, data: try AaveV3.supply(asset: baseUSDC, amount: 3_000_000, onBehalfOf: me), note: "Aave supply 3 USDC")
    #expect(try await receiptType(supplied) == "0x4")
    #expect(try await held(aave.aUSDC) - a0 >= 2_999_990)   // aTokens round on the liquidity index: a unit or two less
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == u0 - 3_000_000)
    #expect(try await allowance(aave.pool) == 0)

    // Morpho (Gauntlet USDC Prime): approve + deposit, the shares to this wallet.
    let vault = MorphoVault.base.address
    let s0 = try await held(vault)
    let deposited = try await e.approveAndSend(rpc, token: baseUSDC, spender: vault, units: 3_000_000, approveNote: "Approve exactly 3 USDC to Morpho",
                                               to: vault, data: try MorphoVault.deposit(assets: 3_000_000, receiver: me), note: "Morpho deposit 3 USDC")
    #expect(try await receiptType(deposited) == "0x4")
    #expect(try await held(vault) > s0)
    #expect(try await allowance(vault) == 0)

    // Relay: approve + depositErc20 to its depository, as quoted live for this wallet (Base → Solana).
    var request = URLRequest(url: URL(string: "https://api.relay.link/quote")!)
    request.httpMethod = "POST"
    request.setValue("application/json", forHTTPHeaderField: "content-type")
    request.httpBody = try JSONSerialization.data(withJSONObject: [
        "user": me, "originChainId": 8453, "destinationChainId": 792703809, "originCurrency": baseUSDC,
        "destinationCurrency": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "recipient": "3YqEpfo3c818GhvbQ1UmVY1nJxw16vtu4JB9peJXT94k",
        "amount": "2000000", "tradeType": "EXACT_INPUT"])
    let quote = try #require(try JSONSerialization.jsonObject(with: try await URLSession.shared.data(for: request).0) as? [String: Any])
    let items = (quote["steps"] as? [[String: Any]] ?? []).filter { $0["id"] as? String == "deposit" }.flatMap { $0["items"] as? [[String: Any]] ?? [] }
    let tx = try #require(items.first?["data"] as? [String: Any])
    let to = try #require(tx["to"] as? String), data = try #require(Data(hex: tx["data"] as? String ?? ""))
    #expect(data.prefix(4).hex == "e8017952")
    let u1 = try await rpc.tokenBalance(baseUSDC, owner: me)
    let bridged = try await e.approveAndSend(rpc, token: baseUSDC, spender: to, units: 2_000_000, approveNote: "Approve exactly 2 USDC to Relay",
                                             to: to, data: data, note: "Relay deposit 2 USDC")
    #expect(try await receiptType(bridged) == "0x4")
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == u1 - 2_000_000)
    #expect(try await allowance(to) == 0)
}

/// Uniswap v4 on Base (UniswapV4.on: Universal Router 2.1.2) on an anvil fork: cbBTC for USDC through the hookless pool the
/// executor finds (DexScreener + Blockscout, live) and quotes on the fork — the approve to Permit2, Permit2's approve to the
/// router and the swap as one type-4 transaction on the fork, the cbBTC at this wallet.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "8453"))
func forkBaseV4BuyIsOneTransaction() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let cbBTC = "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf"
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("fork7702-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    // Found and quoted on Base itself: a cold fork fetches the pool's state lazily, past the quote's timeout.
    let route = try #require(await e.v4Route(baseUSDC, cbBTC, amountIn: 4_000_000, rpc: .base), "no hookless cbBTC/USDC v4 pool found")
    print("v4 pool fee \(route.key.fee) spacing \(route.key.tickSpacing): 4 USDC → \(route.out) sats")
    let usdc0 = try await rpc.tokenBalance(baseUSDC, owner: me), btc0 = try await e.balance(cbBTC, on: rpc)
    let hash = try await e.v4Swap(route, tokenIn: baseUSDC, units: 4_000_000, minOut: (route.out * Decimal(string: "0.99")!).whole, what: "4 USDC",
                                  note: "Buy cbBTC on Uniswap v4", rpc: rpc)
    let receipt = (try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any]
    #expect(receipt?["type"] as? String == "0x4")
    #expect(try await rpc.tokenBalance(baseUSDC, owner: me) == usdc0 - 4_000_000)
    #expect(try await e.balance(cbBTC, on: rpc) - btc0 >= route.out * Decimal(string: "0.99")!)
    #expect(ABI.decimal(in: try await rpc.read(baseUSDC, try ERC20.allowance(owner: me, spender: UniswapV4.permit2)), word: 0) == 0)
}

/// Uniswap v2 on an anvil fork of Ethereum (NET=ethereum Tools/fork7702.sh), through the executor's own swapUniswap:
/// 3 USDC into PEPE by v2's best path (its deepest pool is v2's), approve and swap as two transactions; then all the PEPE
/// back to USDC as one type-4 transaction; the allowance spent to zero both times, and a v2 swap paying another address
/// refused by the signer's rules.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "1"))
func forkV2BuysAndSellsPEPE() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 1)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let uni = try #require(UniswapV3.on(1)), v2 = try #require(UniswapV2.on(1))
    let usdc = EVMNetwork.ethereum.usdc, pepe = "0x6982508145454ce325ddbe47a25d4ec3d2311933"
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forkv2-\(UUID().uuidString).jsonl"))
    func executor(batches: Bool) -> Executor {
        Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me, batches: batches), journal: journal) { print("·", $0) }
    }
    func receiptType(_ hash: String) async throws -> String { ((try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any])?["type"] as? String ?? "?" }
    func allowance(_ token: String) async throws -> Decimal { ABI.decimal(in: try await rpc.read(token, try ERC20.allowance(owner: me, spender: v2.router)), word: 0) ?? -1 }

    let e = executor(batches: false)
    let usdc0 = try await rpc.tokenBalance(usdc, owner: me), nonce0 = try await rpc.nonce(me)
    // 1. Buy: v2's best path for 3 USDC, quoted on the fork, swapped by the executor.
    let buy = try #require(await e.bestV2(on: rpc, from: usdc, to: pepe, amountIn: 3_000_000))
    print("3 USDC → PEPE on v2: \(buy.out) via \(buy.path)")
    let route = UniRoute(v3: nil, v4: nil, v2: buy.path, out: buy.out)
    let hash = try await e.swapUniswap(route, on: .ethereum, uni, tokenIn: usdc, units: 3_000_000, what: "3 USDC", note: "Buy PEPE", rpc: rpc)
    #expect(try await receiptType(hash) == "0x2")
    #expect(try await rpc.nonce(me) == nonce0 + 2)                   // the approve, then the swap
    #expect(try await rpc.tokenBalance(usdc, owner: me) == usdc0 - 3_000_000)
    let held = try await e.balance(pepe, on: rpc)
    #expect(held >= (buy.out * (1 - route.tolerance)).whole)
    #expect(try await allowance(usdc) == 0)
    // 2. Sell it all back: approve + swap as one type-4 transaction.
    let sell = try #require(await e.bestV2(on: rpc, from: pepe, to: usdc, amountIn: held))
    let back = UniRoute(v3: nil, v4: nil, v2: sell.path, out: sell.out)
    let sold = try await executor(batches: true).swapUniswap(back, on: .ethereum, uni, tokenIn: pepe, units: held, what: "PEPE", note: "Sell PEPE", rpc: rpc)
    #expect(try await receiptType(sold) == "0x4")
    #expect(try await e.balance(pepe, on: rpc) == 0)
    let usdc2 = try await rpc.tokenBalance(usdc, owner: me)
    print("sold back: \(usdc2 - (usdc0 - 3_000_000)) USDC units of 3000000")
    #expect(usdc2 > usdc0 - 3_000_000 + 2_900_000)                  // two v2 fees of 0.3% and a little impact
    #expect(try await allowance(pepe) == 0)
    // 3. The same swap paying another address never signs.
    let theirs = try UniswapV2.swapExactTokens(amountIn: 1, amountOutMin: 1, path: sell.path, to: "0x00000000000000000000000000000000000f0a11", deadline: 9_999_999_999)
    await #expect(throws: (any Error).self) {
        try await ForkSigner(key: key, address: me).signEVM(chainId: 1, nonce: 0, maxPriorityFee: 1, maxFee: 1, gasLimit: 100_000, to: v2.router, data: theirs, recipients: [])
    }
}

/// A v4 pool with a hook Uniswap's registry vouches for, on an anvil fork of Base (NET=base Tools/fork7702.sh): Doppler's
/// UniswapV4MulticurveInitializerHook (afterSwap only observes), the PRQUANGD/WETH pool. Its key read the way the executor
/// reads it (PositionManager has none: the Initialize event on Base's Blockscout), then 0.0001 WETH swapped through it by
/// the executor's v4Swap as one type-4 transaction, signed under the signer's rules for Base. The same pool with a hook the
/// registry doesn't list is refused before anything is signed.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "8453"))
func forkV4SwapThroughAVouchedHook() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let weth = "0x4200000000000000000000000000000000000006", token = "0x112f133ae3a57bcc79b21a6319fecc2e5cedd862"
    let id = "0xc37ed370d35fef114571b5788820e89bc80f408ee552ffde5f664a8f1463d065"
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forkhook-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    // The key from Base itself, as the executor finds it; it hashes back to the id and its hook may run there.
    let poolKey = try #require(await Executor.v4Key(id, chainId: 8453))
    #expect(poolKey.id == id && poolKey.hooks == "0x892d3c2b4abeaaf67d52a7b29783e2161b7cad40" && poolKey.hasHooks)
    #expect(UniswapV4.allowsHooks(poolKey, chainId: 8453))
    // 0.001 WETH for this wallet on the fork: WETH9 keeps balances at slot 3.
    let slot = "0x" + Keccak.hash256(try ABI.address(me) + ABI.uint(3)).hex
    _ = try await rpc.call("anvil_setStorageAt", [weth, slot, "0x" + String(format: "%064llx", 1_000_000_000_000_000)])
    let units: Decimal = 100_000_000_000_000
    let zeroForOne = weth == poolKey.currency0
    let quoted = try #require(await e.v4Quote(poolKey, zeroForOne: zeroForOne, amount: units, rpc: rpc))
    print("0.0001 WETH → \(quoted) PRQUANGD through the vouched hook")
    let nonce0 = try await rpc.nonce(me), before = try await e.balance(token, on: rpc)
    let hash = try await e.v4Swap((poolKey, zeroForOne, quoted), tokenIn: weth, units: units, minOut: (quoted * Decimal(string: "0.97")!).whole,
                                  what: "0.0001 WETH", note: "Buy PRQUANGD on Uniswap v4", rpc: rpc)
    let type = ((try await rpc.call("eth_getTransactionReceipt", [hash])) as? [String: Any])?["type"] as? String
    #expect(type == "0x4")
    #expect(try await e.balance(token, on: rpc) - before >= (quoted * Decimal(string: "0.97")!).whole)
    // The same pool behind a hook the registry doesn't list: refused at signing, the nonce untouched.
    let nonce1 = try await rpc.nonce(me)
    #expect(nonce1 > nonce0)
    let unlisted = UniswapV4.PoolKey(currency0: poolKey.currency0, currency1: poolKey.currency1, fee: poolKey.fee, tickSpacing: poolKey.tickSpacing,
                                     hooks: "0x1111111111111111111111111111111111110080")
    await #expect(throws: (any Error).self) {
        _ = try await e.v4Swap((unlisted, zeroForOne, quoted), tokenIn: weth, units: units, minOut: 1, what: "0.0001 WETH", note: "Buy", rpc: rpc)
    }
    #expect(try await rpc.nonce(me) == nonce1)
}

/// One token for another on an anvil fork of Base (NET=base Tools/fork7702.sh), through the executor's own swapOn on a copy
/// of Base whose node is the fork: ETH → AERO (a v4 pool of ETH itself, or wrapped first, whichever gives more) and all
/// of it back to ETH; USDC → DEGEN → AERO without ETH; ETH ↔ WETH as a wrap and an unwrap. No WETH is left behind, and the
/// signer's rules refuse coins sent to anything but the network's WETH or a v4 swap of them.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "8453"))
func forkSwapsTokenForToken() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)   // the user's switch, on for the fork runs
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let b = EVMNetwork.base
    let net = EVMNetwork(name: b.name, key: b.key, rpc: rpc, usdc: b.usdc, relayChain: b.relayChain, explorer: b.explorer, gasFloor: b.gasFloor, topupUSD: b.topupUSD)
    let weth = "0x4200000000000000000000000000000000000006", aero = "0x940181a94a35a4569e4529a3cdfb74e38fd98631"
    let degen = "0x4ed4e862860bed51a9570b96d89af5e1b0efefed"
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forkpair-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    func held(_ t: String) async throws -> Decimal { try await e.balance(t, on: rpc) }
    let weth0 = try await held(weth)

    // 1. ETH → AERO.
    let eth0 = try await rpc.balance(me)
    print(try await e.swapOn(net, from: "ETH", to: "AERO", amount: Decimal(string: "0.001")!))
    let aero1 = try await held(aero)
    #expect(aero1 > 0)
    #expect(try await rpc.balance(me) < eth0 - 1_000_000_000_000_000 + 1)            // the 0.001 ETH and the fees
    #expect(try await held(weth) == weth0)                                          // nothing left wrapped
    // 2. All the AERO back to ETH.
    let eth1 = try await rpc.balance(me)
    print(try await e.swapOn(net, from: "AERO", to: "ETH", amount: nil))
    #expect(try await held(aero) == 0)
    #expect(try await rpc.balance(me) > eth1)
    #expect(try await held(weth) == weth0)                                          // unwrapped, or never wrapped
    // 3. Token for token without the coin: 2 USDC → DEGEN, then all the DEGEN → AERO.
    let usdc0 = try await rpc.tokenBalance(net.usdc, owner: me)
    print(try await e.swapOn(net, from: "USDC", to: degen, amount: 2))
    #expect(try await rpc.tokenBalance(net.usdc, owner: me) == usdc0 - 2_000_000)
    #expect(try await held(degen) > 0)
    print(try await e.swapOn(net, from: degen, to: "AERO", amount: nil))
    #expect(try await held(degen) == 0)
    #expect(try await held(aero) > 0)
    // 4. ETH ↔ WETH: a wrap, then an unwrap of all of it.
    print(try await e.swapOn(net, from: "ETH", to: "WETH", amount: Decimal(string: "0.0005")!))
    #expect(try await held(weth) == weth0 + 500_000_000_000_000)
    print(try await e.swapOn(net, from: "WETH", to: "ETH", amount: nil))
    #expect(try await held(weth) == 0)
    // 5. Coins sent with a call go only into the network's WETH or a v4 swap of them, exactly its input.
    let signer = ForkSigner(key: key, address: me)
    await #expect(throws: (any Error).self) {
        _ = try await signer.signEVMPayable(chainId: 8453, nonce: 0, maxPriorityFee: 1, maxFee: 1, gasLimit: 100_000, to: aero, value: 1,
                                            data: Data([0xd0, 0xe3, 0x0d, 0xb0]), recipients: [])
    }
    let native = UniswapV4.PoolKey(currency0: UniswapV4.native, currency1: aero, fee: 3000, tickSpacing: 60, hooks: UniswapV4.native)
    let swap = try UniswapV4.execute(native, zeroForOne: true, amountIn: 1_000, amountOutMinimum: 1, deadline: 9_999_999_999)
    _ = try await signer.signEVMPayable(chainId: 8453, nonce: 0, maxPriorityFee: 1, maxFee: 1, gasLimit: 100_000, to: UniswapV4.on(8453)!.router,
                                        value: 1_000, data: swap, recipients: [])
    await #expect(throws: (any Error).self) {
        _ = try await signer.signEVMPayable(chainId: 8453, nonce: 0, maxPriorityFee: 1, maxFee: 1, gasLimit: 100_000, to: UniswapV4.on(8453)!.router,
                                            value: 1_001, data: swap, recipients: [])
    }
}

/// Borrowing on Aave on an anvil fork of Base (Tools/fork7702.sh, TESTS=forkBorrowsAgainstETHAndRepays): 0.01 ETH in as
/// collateral, a borrow that would leave a health factor under 1.5 refused before anything is signed, 5 USDC borrowed
/// with the liquidation price of ETH under today's, then the whole debt paid back.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL_CHAIN"] == "8453"))
func forkBorrowsAgainstETHAndRepays() async throws {
    let env = ProcessInfo.processInfo.environment
    UserDefaults.standard.set(true, forKey: Executor.batchKey)
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: 8453)
    let key = Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!
    let me = try EVMAccount.address(privateKey: key)
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forkborrow-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me), signer: ForkSigner(key: key, address: me), journal: journal) { print("·", $0) }
    let m = AaveV3.Market(chain: "base", chainName: "Base", rpc: rpc, usdc: baseUSDC, provider: AaveV3.baseProvider, pool: AaveV3.basePool, aUSDC: AaveV3.baseAUSDC)

    print(try await e.aaveSupplyETH(m, eth: Decimal(string: "0.01")!))
    let a = try await e.aaveAccount(m)
    #expect(a.collateral > 10 && a.debt == 0 && a.available > 5)
    await #expect(throws: ExecutionError.self) { try await e.aaveBorrow(m, usdc: a.available.rounded(2, .down)) }
    #expect(try await e.aaveAccount(m).debt == 0)

    let p = try await e.aaveBorrowPreview(m, usdc: 5)
    let price = a.collateral / Decimal(string: "0.01")!
    print("collateral \(a.collateral), health after \(p.after ?? -1), liquidation \(p.liquidation ?? -1), ETH ≈ \(price)")
    #expect((p.after ?? 0) >= 1.5 && (p.liquidation ?? 0) > 0 && (p.liquidation ?? .greatestFiniteMagnitude) < price)
    print(try await e.aaveBorrow(m, usdc: 5))
    let b = try await e.aaveAccount(m)
    #expect(b.debt > Decimal(string: "4.99")! && b.debt < Decimal(string: "5.01")!)

    print(try await e.aaveRepay(m, usdc: nil))
    #expect(try await e.aaveAccount(m).debt == 0)
}
