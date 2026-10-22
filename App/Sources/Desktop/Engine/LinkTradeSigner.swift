import EVMChain
import Foundation
import HyperliquidSigning
import SignerAPI
import TradeExecution
import WalletCore

/// What the Executor signs with, as the Mac's XPCTradeSigner: every method one SignerRequest, and a refusal said with the
/// signer's own reasons.
nonisolated struct LinkTradeSigner: TradeSigner {
    /// While a signature waits (Windows Hello may be up), the window says so.
    let waiting: @Sendable (Bool) -> Void

    private func send(_ request: SignerRequest) async throws -> SignerReply {
        waiting(true)
        defer { waiting(false) }
        return try await SignerLink.shared.send(request)
    }

    static func reason(_ reply: SignerReply) -> String {
        ([reply.error ?? "The signer refused."] + reply.problems).joined(separator: " ")
    }

    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data {
        let reply = try await send(.signSolana(wire: wire, accounts: accounts, recipients: recipients, tokenOwners: tokenOwners))
        guard reply.ok, let signed = reply.signedWire else { throw ExecutionError(Self.reason(reply)) }
        return signed
    }

    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature {
        let reply = try await send(.signHyperliquidL1(action: action, nonce: nonce, mainnet: true))
        guard reply.ok, let signature = reply.signature else { throw ExecutionError(Self.reason(reply)) }
        return signature
    }

    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature {
        let reply = try await send(.signHyperliquidUser(action: action, mainnet: true, recipients: recipients))
        guard reply.ok, let signature = reply.signature else { throw ExecutionError(Self.reason(reply)) }
        return signature
    }

    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature {
        let reply = try await send(.signRelayNonceMapping(wallet: wallet, depositor: depositor, id: id, nonce: nonce))
        guard reply.ok, let signature = reply.signature else { throw ExecutionError(Self.reason(reply)) }
        return signature
    }

    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                 to: String, data: Data, recipients: [String]) async throws -> Data {
        let reply = try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                       gasLimit: gasLimit, to: to, value: 0, data: data, recipients: recipients))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    var signsBatches: Bool { true }

    func signEVMBatch(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                      calls: [Batch7702.Call], recipients: [String]) async throws -> Data {
        let reply = try await send(.signEVMBatch(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee, gasLimit: gasLimit,
                                                 calls: calls.map { SignerCall(to: $0.to, data: $0.data) }, recipients: recipients))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signEVMPayable(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                        to: String, value: UInt64, data: Data, recipients: [String]) async throws -> Data {
        let reply = try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                       gasLimit: gasLimit, to: to, value: value, data: data, recipients: recipients))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signEVMTransfer(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                         to: String, value: UInt64, recipients: [String]) async throws -> Data {
        let reply = try await send(.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee,
                                                       gasLimit: gasLimit, to: to, value: value, data: Data(), recipients: recipients))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await send(.signNearIntentDeposit(amount: "\(amount)", depositAddress: depositAddress, nonce: nonce, blockHash: blockHash))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signNearTransfer(amount: UInt128, receiver: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await send(.signNearTransfer(amount: "\(amount)", receiver: receiver, recipients: recipients, nonce: nonce, blockHash: blockHash))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signNearDeleteAccount(beneficiary: String, recipients: [String], nonce: UInt64, blockHash: Data) async throws -> Data {
        let reply = try await send(.signNearDeleteAccount(beneficiary: beneficiary, recipients: recipients, nonce: nonce, blockHash: blockHash))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature {
        let reply = try await send(.signSafeMove(chainId: move.chainId, safe: move.safe, to: move.to, value: move.value, data: move.data,
                                                 nonce: move.nonce, recipients: recipients))
        guard reply.ok, let signature = reply.signature else { throw ExecutionError(Self.reason(reply)) }
        return signature
    }
}
