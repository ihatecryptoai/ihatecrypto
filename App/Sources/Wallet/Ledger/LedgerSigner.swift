import CryptoKit
import EVMChain
import Foundation
import HyperliquidSigning
import SolanaChain
import TradeExecution
import TxGuard
import WalletCore

/// A Ledger as the wallet a move signs with. The same guard the signing process runs checks each request first, then
/// the device shows it and waits for the person's buttons. What comes back is checked against the Ledger's own
/// address before it is used, so a swapped or confused device can't slip a stranger's signature into a move.
public struct LedgerSigner: TradeSigner {
    public let device: LedgerDevice
    public let wallet: TradeWallet
    public var evmPath = LedgerDevice.evmPath
    public var solanaPath = LedgerDevice.solanaPath
    /// «Nano S Plus», «Flex»…: Ledger's certificates are per model.
    public var model: String?
    /// What the person has to do on the device now («Open the Solana app…», «Review … on your Ledger»); nil when done.
    public let prompt: @Sendable (String?) -> Void
    /// The trading key the Ledger approved for its Hyperliquid account, in the signing process: orders, cancels and
    /// leverage go there without the device. Nil while there is none or it is paused.
    public var agent: (@Sendable (MsgPack, UInt64) async throws -> EVMSignature)?

    public init(device: LedgerDevice, wallet: TradeWallet, evmPath: String = LedgerDevice.evmPath, solanaPath: String = LedgerDevice.solanaPath,
                model: String? = nil, agent: (@Sendable (MsgPack, UInt64) async throws -> EVMSignature)? = nil,
                prompt: @escaping @Sendable (String?) -> Void = { _ in }) {
        (self.device, self.wallet, self.evmPath, self.solanaPath, self.model, self.agent, self.prompt) = (device, wallet, evmPath, solanaPath, model, agent, prompt)
    }

    /// `hash`: what the device shows when it can't show the details (blind signing), so the person can compare it
    /// with the one ihatecrypto checked: the Solana message's SHA-256 in base58, the EVM transaction's hash, the
    /// typed data's message hash.
    func onDevice<T: Sendable>(_ app: String, _ what: String, hash: String, _ body: () async throws -> T) async throws -> T {
        defer { prompt(nil) }
        prompt("Open the \(app) app on your Ledger (allow it on the device if it asks).")
        try await device.open(app)
        prompt("Review on your Ledger and approve: \(what). If it shows a hash, it is \(hash.prefix(6))…\(hash.suffix(4)).")
        return try await body()
    }

    static func refuse(_ verdict: Verdict) -> ExecutionError { ExecutionError(verdict.problems.joined(separator: " ")) }

    public func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data {
        let verdict = SolanaGuard.check(try SolanaMessage.parse(transaction: wire), accounts: accounts,
                                        policy: SolanaPolicy(owner: wallet.solana, recipients: Set(recipients)), tokenOwners: tokenOwners)
        guard verdict.ok else { throw Self.refuse(verdict) }
        let payer = try Base58.decode(wallet.solana)
        let (message, slot) = try SolanaTransaction.unsigned(wire, payer: payer)
        let hash = Base58.encode(Data(SHA256.hash(data: message)))
        let transfer = Self.tokenTransfer(try SolanaMessage.parse(transaction: wire), accounts: accounts, owners: recipients + [wallet.solana])
        let what = transfer.map { "a token transfer to \($0.owner.prefix(4))…\($0.owner.suffix(4))" } ?? verdict.checks.first { $0.contains("→") } ?? "a Solana transaction"
        let certificate = transfer == nil ? nil : try? await LedgerDescriptors.trustedNameCertificate(model: model)
        let signature = try await onDevice("Solana", what, hash: hash) {
            // A USDC transfer shows its amount and recipient when Ledger vouches for whose token account it pays;
            // without that (no network, a token Ledger doesn't know), it stays blind-signed.
            if let transfer, let certificate {
                do {
                    try await device.loadCertificate(certificate, app: "Solana")
                    let challenge = try await device.challenge(app: "Solana")
                    let tlv = try await LedgerDescriptors.solanaTokenAccount(owner: transfer.owner, mint: transfer.mint, tokenAccount: transfer.account, challenge: challenge)
                    try await device.provideTrustedName(tlv, app: "Solana")
                } catch let e as LedgerError where e.declined { throw e } catch {}
            }
            return try await device.signSolana(message, path: solanaPath)
        }
        guard try Curve25519.Signing.PublicKey(rawRepresentation: payer).isValidSignature(signature, for: message) else {
            throw LedgerError("The Ledger's signature isn't from \(wallet.solana): is it the Ledger you connected?")
        }
        var signed = wire
        signed.replaceSubrange(slot, with: signature)
        return signed
    }

    /// The first SPL token transfer in the message and whose account it pays, when the payee is one of `owners` (our
    /// own or an approved address): its associated account for the mint must be exactly the destination.
    static func tokenTransfer(_ m: SolanaMessage, accounts: [String], owners: [String]) -> (account: String, owner: String, mint: String)? {
        let programs: Set<String> = [SPL.tokenProgram, SPL.token2022Program]
        for ix in m.instructions where ix.programIndex < accounts.count && programs.contains(accounts[ix.programIndex]) {
            // TransferChecked: source, mint, destination, owner.
            guard ix.data.first == 12, ix.accounts.count >= 4, ix.accounts[1] < accounts.count, ix.accounts[2] < accounts.count else { continue }
            let mint = accounts[ix.accounts[1]], destination = accounts[ix.accounts[2]]
            let program = accounts[ix.programIndex]
            if let owner = owners.first(where: { (try? SPL.associatedAccount(owner: $0, mint: mint, program: program)) == destination }) {
                return (destination, owner, mint)
            }
        }
        return nil
    }

    /// The same calldata rules as the signing process: known networks, no unlimited approvals, tokens only to us or
    /// to an approved address.
    public func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                        to: String, data: Data, recipients: [String]) async throws -> Data {
        guard EVMRPC.signable.contains(chainId) else { throw ExecutionError("ihatecrypto doesn't sign for network \(chainId).") }
        let call = EVMCall.decode(data)
        if let refusal = call.refusal(sender: wallet.evm, chainId: chainId) { throw ExecutionError(refusal) }
        if let payee = call.payees(to: to, value: 0, chainId: chainId).first(where: { p in !(recipients + [wallet.evm]).contains { $0.lowercased() == p.lowercased() } }) {
            throw ExecutionError("Tokens or the right to spend them would go to an unapproved address \(payee).")
        }
        let tx = EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                gasLimit: gasLimit, to: to, value: 0, data: data)
        let hash = try tx.signingHash()
        // A token transfer or approval shows «Send 25 USDC» / «Approve 2 USDC» when Ledger's descriptor for the token
        // comes first; without it the Ethereum app asks for blind signing.
        let token: Data? = switch call {
        case .transfer, .approve: try? await LedgerDescriptors.erc20(chainId: chainId, contract: to)
        default: nil
        }
        let raw = try await onDevice("Ethereum", call.summary(chainId: chainId, to: to, value: 0), hash: "0x" + hash.hex) {
            if let token { try? await device.provideERC20(token) }
            return try await device.signEVMTransaction(try tx.unsignedPayload(), path: evmPath)
        }
        // The device's first byte is the parity for a typed transaction; recovering both settles it either way.
        for parity in [Int(raw.v & 1), Int(raw.v & 1) ^ 1] {
            let signature = EVMSignature(r: raw.r, s: raw.s, v: 27 + parity)
            if try EVMSigner.recover(digest: hash, signature: signature).lowercased() == wallet.evm.lowercased() {
                return try tx.signed(with: signature).raw
            }
        }
        throw LedgerError("The Ledger's signature isn't from \(wallet.evm): is it the Ledger you connected?")
    }

    /// Typed data in full, so the Ledger shows each field by name; by its two hashes on an app too old for that.
    /// The signature must recover to the Ledger's address.
    func typed(_ typed: EIP712.Typed, _ what: String) async throws -> EVMSignature {
        let parts = try typed.parts()
        let signature = try await onDevice("Ethereum", what, hash: "0x" + parts.message.hex) {
            do { return try await device.signTypedData(typed, path: evmPath) }
            catch let e as LedgerError where e.status == 0x6d00 || e.status == 0x6e00 {
                return try await device.signTypedDataHashes(parts, path: evmPath)
            } catch let e as LedgerError where e.status == 0x6a80 {
                throw LedgerError("The Ethereum app won't show this typed data: turn on «Raw messages» in its settings on the Ledger (it then shows every field), or «Blind signing».", status: 0x6a80)
            }
        }
        guard try EVMSigner.recover(digest: parts.digest, signature: signature).lowercased() == wallet.evm.lowercased() else {
            throw LedgerError("The Ledger's signature isn't from \(wallet.evm): is it the Ledger you connected?")
        }
        return signature
    }

    /// A move out of a Safe multisig, shown field by field (SafeTx); the same rules as the signing process's (Safe.refusal).
    public func signSafeMove(_ move: Safe.Move, recipients: [String]) async throws -> EVMSignature {
        if let why = Safe.refusal(move, approved: Set((recipients + [wallet.evm, move.safe]).map { $0.lowercased() })) { throw ExecutionError(why) }
        return try await typed(move.typed, move.summary)
    }

    public func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature {
        let verdict = HLGuard.check(action, owner: wallet.evm, recipients: [])
        guard verdict.ok else { throw Self.refuse(verdict) }
        if let agent, HLSigner.isTrading(action) { return try await agent(action, nonce) }
        let what = HLAction.type(action) == "setReferrer" ? "ihatecrypto's referral code on Hyperliquid (4% off its fees)" : "a Hyperliquid order"
        return try await typed(try HLSigner.l1Typed(action, nonce: nonce, mainnet: true), what)
    }

    /// The one approval of a trading key for the Ledger's Hyperliquid account: the device shows the key's address,
    /// which the person compares with the one on the canvas. The key can trade but not withdraw.
    public func approveAgent(_ agent: String, name: String, nonce: UInt64) async throws -> (MsgPack, EVMSignature) {
        let (action, typed) = HLSigner.approveAgentTyped(agent: agent, name: name, nonce: nonce, mainnet: true)
        return (action, try await self.typed(typed, "let the trading key \(agent.prefix(6))…\(agent.suffix(4)) place orders for you on Hyperliquid; it can't withdraw"))
    }

    public func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature {
        let verdict = HLGuard.check(action, owner: wallet.evm, recipients: Set(recipients))
        guard verdict.ok else { throw Self.refuse(verdict) }
        let (type, fields) = try HLSigner.userAction(action)
        let what = type != "ApproveBuilderFee" ? "a Hyperliquid transfer"
            : HLAction.text(action, "maxFeeRate") == HLBuilder.revoke ? "taking back ihatecrypto's fee approval" : "ihatecrypto's \(HLBuilder.rate) fee on your Hyperliquid trades"
        return try await typed(HLSigner.userTyped(type, fields: fields, mainnet: true), what)
    }

    public func signRelayMapping(wallet w: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature {
        guard w.lowercased() == wallet.evm.lowercased(), depositor.lowercased() == wallet.evm.lowercased() else {
            throw ExecutionError("The deposit mapping isn't for your Ledger: \(w), \(depositor).")
        }
        return try await typed(try HLSigner.relayNonceMappingTyped(wallet: w, depositor: depositor, id: id, nonce: nonce), "link a withdrawal to a Relay order")
    }

    /// The same rules as the signing process (Permit3009.refusal); the Ledger shows each field of the typed data.
    public func signReceiveAuthorization(_ permit: Permit3009) async throws -> EVMSignature {
        if let refusal = permit.refusal(owner: wallet.evm, now: UInt64(Date().timeIntervalSince1970)) { throw ExecutionError(refusal) }
        return try await typed(try permit.typed(), permit.summary)
    }

    public func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data {
        throw ExecutionError("NEAR isn't signed on a Ledger in ihatecrypto yet: switch to one of ihatecrypto's own wallets for NEAR.")
    }
}
