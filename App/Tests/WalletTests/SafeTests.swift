@testable import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import Testing
@testable import TradeExecution
import WalletCore

/// anvil's first well-known key (public test vector, never funded by ihatecrypto).
private let key = Data(hex: "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")!
private let owner = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
private let dead = "0x000000000000000000000000000000000000dead"

/// Both vectors are Safe 1.4.1's own answers on Base (eth_call, 2026-10-09): the factory's createProxyWithNonce simulated,
/// and SafeL2's getTransactionHash called on the singleton itself (its domain is its own address).
@Test func safeAddressAndMoveHashMatchTheContracts() throws {
    let owners = ["0x" + String(repeating: "11", count: 20), "0x" + String(repeating: "22", count: 20), "0x" + String(repeating: "33", count: 20)]
    #expect(try Safe.setup(owners: owners, threshold: 2).count == 420)
    #expect(try Safe.address(owners: owners, threshold: 2, salt: 42) == "0xdbcf0e3023809d9436b468e63fd86cc61b4dddaf")
    let move = Safe.Move(chainId: 8453, safe: Safe.singleton, to: EVMRPC.baseUSDC, value: 0, data: try ERC20.transfer(to: dead, amount: 1_000_000), nonce: 7)
    #expect(try move.hash().hex == "a2f0968324819faabd2ea549d929ed4b2d0606130f6368938548963ddacecca3")
}

@Test func safeExecCarriesSortedSignaturesAndReadsBack() throws {
    let move = Safe.Move(chainId: 8453, safe: "0x" + String(repeating: "ab", count: 20), to: EVMRPC.baseUSDC, value: 0,
                         data: try ERC20.transfer(to: dead, amount: 5), nonce: 0)
    let sig = try EVMSigner.sign(digest: try move.hash(), privateKey: key)
    #expect(try EVMSigner.recover(digest: try move.hash(), signature: sig).lowercased() == owner)
    let other = EVMSignature(r: Data(repeating: 1, count: 32), s: Data(repeating: 2, count: 32), v: 27)
    let calldata = try move.exec([(owner, sig), ("0x0000000000000000000000000000000000000001", other)])
    let read = try #require(Safe.readExec(calldata))
    #expect(read.to == EVMRPC.baseUSDC && read.data == move.data && read.operation == 0)
    // Signatures go by owner, ascending: 0x…01 first, then the anvil key's.
    let tail = calldata.suffix(192)
    #expect(Data(tail.prefix(32)) == Data(repeating: 0, count: 31) + Data([130]) && Data(tail.dropFirst(32).prefix(32)) == other.r)
    // The signing process reads the same call: the multisig pays `dead` 5 units of USDC.
    let call = EVMCall.decode(calldata)
    #expect(call.payees(to: move.safe, value: 0, chainId: 8453) == [dead])
    #expect(call.refusal(sender: owner, chainId: 8453) == nil)
    #expect(call.summary(chainId: 8453, to: move.safe, value: 0).contains("send 0.000005 USDC to 0x0000…dead"))
}

/// A multisig's move carries a uint256 of wei: 100 ETH reads as 100 ETH on the prompt, not the 18.4 a UInt64 holds.
@Test func multisigSummaryShowsCoinsPast64Bits() throws {
    let safe = "0x" + String(repeating: "ab", count: 20)
    var exec = try Safe.Move(chainId: 8453, safe: safe, to: dead, value: 1, data: Data(), nonce: 0).exec([])
    func pay(_ wei: Data) -> String {
        exec.replaceSubrange((4 + 32)..<(4 + 64), with: wei)
        return EVMCall.decode(exec).summary(chainId: 8453, to: safe, value: 0)
    }
    #expect(pay(try ABI.word(Decimal(string: "100000000000000000000")!)) == "carry out the multisig 0xabab…abab's signed move: send 100 ETH to 0x0000…dead on Base")
    #expect(EVMCall.decode(exec).payees(to: safe, value: 0, chainId: 8453) == [dead])
    #expect(pay(Data(count: 23) + [1] + Data(count: 8)).hasSuffix("send 18.446744073709551616 ETH to 0x0000…dead on Base"))   // 2⁶⁴
    #expect(pay(ABI.uint(5)).hasSuffix("send 0.000000000000000005 ETH to 0x0000…dead on Base"))
}

@Test func safeSignsOnlyPlainPaymentsToApprovedAddresses() throws {
    let safe = "0x" + String(repeating: "ab", count: 20)
    let approved: Set<String> = [owner, safe, dead]
    let pay = Safe.Move(chainId: 8453, safe: safe, to: EVMRPC.baseUSDC, value: 0, data: try ERC20.transfer(to: dead, amount: 5), nonce: 0)
    #expect(Safe.refusal(pay, approved: approved) == nil)
    #expect(Safe.refusal(Safe.Move(chainId: 8453, safe: safe, to: dead, value: 1, data: Data(), nonce: 0), approved: approved) == nil)
    // To someone not approved, the multisig itself (its keys), another contract's call, a token transfer with coins, an unknown network.
    #expect(Safe.refusal(pay, approved: [owner, safe]) != nil)
    #expect(Safe.refusal(Safe.Move(chainId: 8453, safe: safe, to: safe, value: 0, data: Data([0x0d, 0x58, 0x2f, 0x13]), nonce: 0), approved: approved) != nil)
    #expect(Safe.refusal(Safe.Move(chainId: 8453, safe: safe, to: EVMRPC.baseUSDC, value: 0, data: try ERC20.approve(spender: dead, amount: 5), nonce: 0), approved: approved) != nil)
    #expect(Safe.refusal(Safe.Move(chainId: 8453, safe: safe, to: EVMRPC.baseUSDC, value: 1, data: pay.data, nonce: 0), approved: approved) != nil)
    #expect(Safe.refusal(Safe.Move(chainId: 5, safe: safe, to: dead, value: 1, data: Data(), nonce: 0), approved: approved) != nil)
    // Keys: distinct addresses, a threshold within them.
    #expect(Safe.ownersProblem([owner, owner], threshold: 1) != nil && Safe.ownersProblem([owner], threshold: 2) != nil && Safe.ownersProblem([owner, dead], threshold: 2) == nil)
}

@Test func safeExecByDelegatecallIsRefused() throws {
    let move = Safe.Move(chainId: 8453, safe: "0x" + String(repeating: "ab", count: 20), to: dead, value: 0, data: Data(), nonce: 0)
    var calldata = try move.exec([])
    calldata[4 + 3 * 32 + 31] = 1   // operation: DELEGATECALL
    #expect(EVMCall.decode(calldata).refusal(sender: owner, chainId: 8453) != nil)
}

// MARK: - Fork run

/// A throwaway key under the signing process's rules (Signer.swift): money only to `recipients` or itself, a multisig move
/// only as Safe.refusal allows.
private struct ForkKey: TradeSigner {
    let key: Data
    var address: String { (try? EVMAccount.address(privateKey: key))?.lowercased() ?? "" }

    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }

    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data {
        let call = EVMCall.decode(data)
        if let why = call.refusal(sender: address, chainId: chainId) { throw ExecutionError(why) }
        if let why = EVMCall.targetRefusal(to: to, data: data, sender: address) { throw ExecutionError(why) }
        let ok = Set((recipients + [address]).map { $0.lowercased() })
        if let payee = call.payees(to: to, value: 0, chainId: chainId).first(where: { !ok.contains($0.lowercased()) }) { throw ExecutionError("pays \(payee)") }
        return try EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee, gasLimit: gasLimit,
                                  to: to, value: 0, data: data).signed(privateKey: key).raw
    }

    func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature {
        if let why = Safe.refusal(move, approved: Set((recipients + [address, move.safe]).map { $0.lowercased() })) { throw ExecutionError(why) }
        return try EVMSigner.sign(digest: try move.hash(), privateKey: key)
    }
}

/// The executor makes a 2-of-3 multisig on an anvil fork of Base (TESTS=forkMultisigMakesAndPays Tools/fork7702.sh): at
/// the address Safe.address said, with those keys and that threshold; then pays USDC and ETH out of it with two keys'
/// signatures, and refuses to send with one.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_ANVIL"] != nil))
func forkMultisigMakesAndPays() async throws {
    let env = ProcessInfo.processInfo.environment
    let rpc = EVMRPC(url: URL(string: env["IHATECRYPTO_ANVIL"]!)!, chainId: UInt64(env["IHATECRYPTO_ANVIL_CHAIN"] ?? "8453")!)
    let base = EVMNetwork.base
    let net = EVMNetwork(name: base.name, key: base.key, rpc: rpc, usdc: base.usdc, relayChain: base.relayChain, explorer: base.explorer,
                         gasFloor: base.gasFloor, topupUSD: base.topupUSD)
    let me = ForkKey(key: Data(hex: env["IHATECRYPTO_ANVIL_KEY"]!)!)
    let phone = ForkKey(key: Data((0..<32).map { _ in UInt8.random(in: 1...255) })), friend = ForkKey(key: Data((0..<32).map { _ in UInt8.random(in: 1...255) }))
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("forksafe-\(UUID().uuidString).jsonl"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: me.address), signer: me, journal: journal) { print("·", $0) }
    let owners = [me.address, phone.address, friend.address], salt = UInt64(Date().timeIntervalSince1970)

    let safe = try await e.createMultisig(net, owners: owners, threshold: 2, salt: salt)
    #expect(safe == (try Safe.address(owners: owners, threshold: 2, salt: salt)))
    #expect(try await Executor.deployed(safe, on: net))
    #expect(Set(try #require(Safe.owners(in: try await rpc.read(safe, Safe.ownersCall)))) == Set(owners))
    #expect(ABI.uint(in: try await rpc.read(safe, Safe.thresholdCall), word: 0) == 2)
    // A key: an EIP-7702-delegated wallet still signs with its own key; the multisig itself is a contract.
    let delegated = "0x000000000000000000000000000000000000c0de"
    _ = try await rpc.call("anvil_setCode", [delegated, "0xef010063c0c19a282a1b52b07dd5a65b58948a07dae32b"])
    #expect(try await !Executor.isContract(delegated, on: net))
    #expect(try await Executor.isContract(safe, on: net))
    #expect(try await !Executor.isContract(phone.address, on: net))
    // Made already: left as it is, no second transaction.
    #expect(try await e.createMultisig(net, owners: owners, threshold: 2, salt: salt) == safe)

    // Money in, by the fork's own hand: 5 USDC (FiatToken's balances sit at slot 9) and 0.001 ETH.
    let slot = Keccak.hash256((try ABI.address(safe)) + ABI.uint(9))
    _ = try await rpc.call("anvil_setStorageAt", [base.usdc, "0x" + slot.hex, "0x" + ABI.uint(5_000_000).hex])
    _ = try await rpc.call("anvil_setBalance", [safe, "0x38d7ea4c68000"])
    let dead = "0x000000000000000000000000000000000000beef"
    let before = try await rpc.tokenBalance(base.usdc, owner: dead)

    _ = try await e.multisigPay(net, safe: safe, to: base.usdc, value: 0, data: try ERC20.transfer(to: dead, amount: 2_000_000),
                                cosigners: [me, phone], recipients: [dead], note: "2 USDC out of the multisig")
    #expect(try await rpc.tokenBalance(base.usdc, owner: dead) == before + 2_000_000)
    #expect(try await rpc.tokenBalance(base.usdc, owner: safe) == 3_000_000)
    _ = try await e.multisigPay(net, safe: safe, to: dead, value: 100_000_000_000_000, data: Data(),
                                cosigners: [friend, me], recipients: [dead], note: "0.0001 ETH out of the multisig")
    #expect(ABI.uint(in: try await rpc.read(safe, Safe.nonceCall), word: 0) == 2)
    // One key isn't enough, and a stranger's signature isn't a key's.
    await #expect(throws: (any Error).self) {
        _ = try await e.multisigPay(net, safe: safe, to: base.usdc, value: 0, data: try ERC20.transfer(to: dead, amount: 1), cosigners: [me], recipients: [dead], note: "one key")
    }
    await #expect(throws: (any Error).self) {
        _ = try await e.multisigPay(net, safe: safe, to: base.usdc, value: 0, data: try ERC20.transfer(to: dead, amount: 1),
                                    cosigners: [me, ForkKey(key: Data(repeating: 7, count: 32))], recipients: [dead], note: "a stranger")
    }
}

/// A site can't have a multisig's move or message signed through typed data: the multisig's own steps sign moves.
@Test func sitesNeverGetASafeSignature() throws {
    for (primary, fields, message) in [("SafeMessage", #"[{"name":"message","type":"bytes"}]"#, #"{"message":"0x1234"}"#),
                                       ("SafeTx", #"[{"name":"to","type":"address"},{"name":"value","type":"uint256"},{"name":"data","type":"bytes"},{"name":"operation","type":"uint8"},{"name":"safeTxGas","type":"uint256"},{"name":"baseGas","type":"uint256"},{"name":"gasPrice","type":"uint256"},{"name":"gasToken","type":"address"},{"name":"refundReceiver","type":"address"},{"name":"nonce","type":"uint256"}]"#,
                                        #"{"to":"0x000000000000000000000000000000000000dEaD","value":"0","data":"0x","operation":"1","safeTxGas":"0","baseGas":"0","gasPrice":"0","gasToken":"0x0000000000000000000000000000000000000000","refundReceiver":"0x0000000000000000000000000000000000000000","nonce":"0"}"#)] {
        let json = #"{"types":{"EIP712Domain":[{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],"\#(primary)":\#(fields)},"primaryType":"\#(primary)","domain":{"chainId":8453,"verifyingContract":"0xabababababababababababababababababababab"},"message":\#(message)}"#
        #expect(try TypedData.parse(Data(json.utf8)).refusal != nil, "\(primary)")
    }
}
