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
    /// A scheduled task's run on the automation wallet: the signer is told which wallet signs, and allows it only a
    /// task's moves.
    var asWallet: String?

    init(asWallet: String? = nil, waiting: @escaping @Sendable (Bool) -> Void) {
        self.asWallet = asWallet
        self.waiting = waiting
    }

    private func send(_ request: SignerRequest) async throws -> SignerReply {
        waiting(true)
        defer { waiting(false) }
        return try await SignerLink.shared.send(asWallet.map { .asWallet(id: $0, request: request) } ?? request)
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

    /// A Relay order before the deposit that names it: the signer checks whom it pays (SignerRequest.relayOrder).
    func registerRelayOrder(_ order: Data) async throws {
        let reply = try await send(.relayOrder(order: order))
        guard reply.ok else { throw ExecutionError(Self.reason(reply)) }
    }

    // Dollars without gas, as the Mac's XPCTradeSigner: someone else pays the fee and the signer reads what it signs.

    func signSolanaSponsored(_ wire: Data, accounts: [String], tokenOwners: [String: String], sponsor: String) async throws -> Data {
        let reply = try await send(.signSolana(wire: wire, accounts: accounts, recipients: [], tokenOwners: tokenOwners, sponsor: sponsor))
        guard reply.ok, let signed = reply.signedWire else { throw ExecutionError(Self.reason(reply)) }
        return signed
    }

    func signSponsoredTransfer(_ t: SponsoredTransfer, recipients: [String]) async throws -> Data {
        let reply = try await send(.signSponsoredTransfer(chainId: t.chainId, nonce: t.nonce, gasLimit: t.gasLimit, token: t.token, to: t.to,
                                                          amount: t.amount, recipients: recipients))
        guard reply.ok, let raw = reply.signedRaw else { throw ExecutionError(Self.reason(reply)) }
        return raw
    }

    func signRelayMulticallPermit(_ m: RelayMulticallPermit) async throws -> [EVMSignature] {
        let reply = try await send(.signRelayMulticallPermit(chainId: m.chainId, token: m.token, value: m.value, validBefore: m.validBefore,
                                                             relayer: m.relayer, refundTo: m.refundTo, nftRecipient: m.nftRecipient, metadata: m.metadata,
                                                             calls: m.calls.map { SignerCall3(target: $0.target, allowFailure: $0.allowFailure, value: $0.value, data: $0.callData) }))
        guard reply.ok, let signatures = reply.signatures, signatures.count == 2 else { throw ExecutionError(Self.reason(reply)) }
        return signatures
    }

    func signReceiveAuthorization(_ p: Permit3009) async throws -> EVMSignature {
        let reply = try await send(.signReceiveAuthorization(chainId: p.chainId, token: p.token, to: p.to, value: p.value,
                                                             validBefore: p.validBefore, nonce: p.nonce))
        guard reply.ok, let signature = reply.signature else { throw ExecutionError(Self.reason(reply)) }
        return signature
    }
}
