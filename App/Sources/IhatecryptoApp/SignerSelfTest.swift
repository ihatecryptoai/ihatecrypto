import CryptoKit
import EVMChain
import Foundation
import HyperliquidSigning
import SignerAPI
import SolanaChain
import WalletCore

/// Runs inside ihatecrypto.app against the embedded signing process with the public BIP-39 test phrase:
/// import, known addresses, the guard refusing what it must, signatures that verify, forget.
/// Prints one line per check; the app quits with 0 only if all pass.
enum SignerSelfTest {
    static let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"

    static func run() async -> Bool {
        var failures = 0
        func check(_ ok: Bool, _ what: String) {
            print("\(ok ? "✓" : "✗") \(what)")
            if !ok { failures += 1 }
        }
        do {
            guard SignerClient.isAvailable else { print("✗ no signer in the bundle"); return false }
            let signer = SignerClient()
            let status = try await signer.send(.status)
            check(status.ok, "signer responds")
            if status.hasWallet == true, status.solanaAddress != "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk" {
                print("✗ secure storage holds another wallet; the self-test leaves it alone")
                return false
            }

            let imported = try await signer.send(.importMnemonic(phrase: phrase, passphrase: "", requirePresence: false))
            check(imported.evmAddress == "0x9858EfFD232B4033E47d90003D41EC34EcaEda94", "EVM address from storage matches the reference")
            check(imported.solanaAddress == "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", "Solana address matches the reference")
            check(try await signer.send(.status).hasWallet == true, "key is in the Keychain")

            // The app derives the same public test keys only to build transactions and compare signatures.
            let seed = try Mnemonic.seed(phrase)
            let solanaKey = try Derivation.ed25519(seed: seed, path: WalletPath.solana(0))
            let evmKey = try Derivation.secp256k1(seed: seed, path: WalletPath.evm(0))
            let owner = try SolanaAccount.address(privateKey: solanaKey)
            let scam = Base58.encode(Curve25519.Signing.PrivateKey().publicKey.rawRepresentation)
            let blockhash = Base58.encode(Data(repeating: 7, count: 32))

            // SOL to a stranger: refused unless the user approved that recipient.
            let toScam = try SolanaTransaction.transfer(from: solanaKey, to: scam, lamports: 1000, blockhash: blockhash).wire
            let keys = try SolanaMessage.parse(transaction: toScam).staticKeys
            let refused = try await signer.send(.signSolana(wire: toScam, accounts: keys, recipients: [], tokenOwners: [:]))
            check(!refused.ok && refused.signedWire == nil && !refused.problems.isEmpty, "transfer to a foreign address not signed: \(refused.problems.first ?? "")")
            let approved = try await signer.send(.signSolana(wire: toScam, accounts: keys, recipients: [scam], tokenOwners: [:]))
            check(approved.ok && verifies(approved.signedWire, owner: owner), "transfer to an approved address signed; the signature verifies with the wallet key")

            // USDC to a stranger's token account that the app claims is ours: the signer re-derives and refuses.
            let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            let ours = try SPL.associatedAccount(owner: owner, mint: usdc)
            let theirs = try SPL.associatedAccount(owner: scam, mint: usdc)
            let transfer = SolanaInstruction(program: SPL.tokenProgram, accounts: [
                .init(pubkey: ours, isSigner: false, isWritable: true), .init(pubkey: usdc, isSigner: false, isWritable: false),
                .init(pubkey: theirs, isSigner: false, isWritable: true), .init(pubkey: owner, isSigner: true, isWritable: false),
            ], data: Data([12]) + withUnsafeBytes(of: UInt64(1_000_000).littleEndian) { Data($0) } + Data([6]))
            let spl = try SolanaTransaction.compile([transfer], privateKey: solanaKey, blockhash: blockhash).wire
            let splKeys = try SolanaMessage.parse(transaction: spl).staticKeys
            let lie = try await signer.send(.signSolana(wire: spl, accounts: splKeys, recipients: [], tokenOwners: [ours: owner, theirs: owner]))
            check(!lie.ok, "false “this is your token account” rejected: \(lie.problems.first ?? lie.error ?? "")")

            // What is signed must be exactly what was checked.
            let tail = try? await signer.send(.signSolana(wire: toScam + Data([0]), accounts: keys, recipients: [scam], tokenOwners: [:]))
            check(tail?.ok != true, "extra bytes after the message rejected")
            var swapped = keys
            swapped.swapAt(1, 2)
            let mismatch = try? await signer.send(.signSolana(wire: toScam, accounts: swapped, recipients: [scam], tokenOwners: [:]))
            check(mismatch?.ok != true, "swapped account list rejected")
            // Solana's v1 (message first, signatures last): refused in words, and the signer answers after it.
            let v1 = Data([0x81, 1, 0, 1]) + Data(count: 4) + Data(repeating: 7, count: 32) + Data([0, 2])
                + (try Base58.decode(owner)) + (try Base58.decode(scam)) + Data(count: 64)
            let v1Reply = try? await signer.send(.signSolana(wire: v1, accounts: [owner, scam], recipients: [scam], tokenOwners: [:]))
            let alive = (try? await signer.send(.status))?.ok == true
            check(v1Reply?.ok == false && v1Reply?.error?.contains("v1") == true && alive, "v1 refused, the signer still answers: \(v1Reply?.error ?? "no reply")")

            // Hyperliquid: orders sign, money out only to an approved address, signatures equal the reference signer's.
            let cancel = HLAction.cancel([(asset: 0, oid: 1)])
            let l1 = try await signer.send(.signHyperliquidL1(action: cancel, nonce: 1_700_000_000_000, mainnet: false))
            check(l1.signature == (try HLSigner.signL1(cancel, privateKey: evmKey, nonce: 1_700_000_000_000, mainnet: false)), "Hyperliquid order cancel: signature matches the reference")

            let scamEVM = "0x" + String(repeating: "5c", count: 20)
            let (send, reference) = try HLSigner.sendAsset(destination: scamEVM, sourceDex: "", destinationDex: "spot", token: "USDC:0x6d1e7cde53ba9467b783cb7c530ce054",
                                                           amount: "1", nonce: 1_700_000_000_001, privateKey: evmKey, mainnet: false)
            let blocked = try await signer.send(.signHyperliquidUser(action: send, mainnet: false, recipients: []))
            check(!blocked.ok && blocked.signature == nil, "sendAsset to a foreign address not signed")
            let allowed = try await signer.send(.signHyperliquidUser(action: send, mainnet: false, recipients: [scamEVM]))
            check(allowed.signature == reference, "sendAsset to an approved address: signature matches the reference")
            let (move, moveRef) = try HLSigner.usdClassTransfer(amount: 1, toPerp: true, nonce: 1_700_000_000_002, privateKey: evmKey, mainnet: false)
            check(try await signer.send(.signHyperliquidUser(action: move, mainnet: false, recipients: [])).signature == moveRef, "spot → perp transfer: signature matches")
            let (agent, _) = try HLSigner.approveAgent(agent: scamEVM, name: "x", nonce: 1_700_000_000_003, privateKey: evmKey, mainnet: false)
            check(try await signer.send(.signHyperliquidUser(action: agent, mainnet: false, recipients: [scamEVM])).ok == false, "approval of a foreign agent not signed")

            // EVM for sites: a login text signs like the reference, a Permit and a bare hash do not sign at all.
            let login = Data("Sign in to example.com".utf8)
            let personal = try await signer.send(.signPersonal(message: login))
            check(personal.signature == (try EVMSigner.sign(digest: PersonalMessage.digest(login), privateKey: evmKey)), "site login (personal_sign): signature matches the reference")
            check(try await signer.send(.signPersonal(message: Data(repeating: 0xaa, count: 32))).ok == false, "blind 32-byte hash not signed")
            let permit = Data(#"{"types":{"Permit":[{"name":"owner","type":"address"},{"name":"spender","type":"address"},{"name":"value","type":"uint256"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}]},"primaryType":"Permit","domain":{"name":"USD Coin","version":"2","chainId":1,"verifyingContract":"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"},"message":{"owner":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","spender":"0x000000000000000000000000000000000000dEaD","value":"1000000","nonce":0,"deadline":"99999999999"}}"#.utf8)
            let permitReply = try await signer.send(.signTypedData(json: permit))
            check(!permitReply.ok && permitReply.signature == nil, "Permit (permission to spend tokens) not signed")

            // EVM transactions from sites: an exact approve to an approved spender signs exactly like our own builder; to a
            // spender nobody approved, or unlimited, it does not.
            let arbitrumUSDC = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
            let word = { (hex: String) in Data(hex: String(repeating: "0", count: 64 - hex.count) + hex)! }
            let exact = Data(hex: "095ea7b3")! + word(String(repeating: "5c", count: 20)) + word("f4240")
            check(try await signer.send(.signEVMTransaction(chainId: 42161, nonce: 0, maxPriorityFee: 1, maxFee: 20_000_000, gasLimit: 60_000,
                                                            to: arbitrumUSDC, value: 0, data: exact, recipients: [])).ok == false, "approve to an unapproved spender not signed")
            let evmTx = try await signer.send(.signEVMTransaction(chainId: 42161, nonce: 0, maxPriorityFee: 1, maxFee: 20_000_000, gasLimit: 60_000,
                                                                  to: arbitrumUSDC, value: 0, data: exact, recipients: [scamEVM]))
            let referenceTx = try EVMTransaction(chainId: 42161, nonce: 0, maxPriorityFeePerGas: 1, maxFeePerGas: 20_000_000, gasLimit: 60_000,
                                               to: arbitrumUSDC, value: 0, data: exact).signed(privateKey: evmKey).raw
            check(evmTx.signedRaw == referenceTx, "exact-amount approve: transaction matches the reference")
            let unlimited = Data(hex: "095ea7b3")! + word(String(repeating: "5c", count: 20)) + Data(repeating: 0xff, count: 32)
            check(try await signer.send(.signEVMTransaction(chainId: 42161, nonce: 0, maxPriorityFee: 1, maxFee: 20_000_000, gasLimit: 60_000,
                                                            to: arbitrumUSDC, value: 0, data: unlimited, recipients: [])).ok == false, "unlimited approve not signed")

            // NEAR → 1Click: the signer builds the wrap.near transaction itself; it must equal the reference and verify.
            let nearPublic = try Curve25519.Signing.PrivateKey(rawRepresentation: Derivation.ed25519(seed: seed, path: WalletPath.near(0))).publicKey
            let nearAccount = nearPublic.rawRepresentation.hex, deposit = String(repeating: "ab", count: 32), hash32 = Data(repeating: 9, count: 32)
            let nearBody = NearTransaction.serialize(signer: nearAccount, publicKey: nearPublic.rawRepresentation, nonce: 5, receiver: NearTransaction.wrapContract,
                                                     blockHash: hash32, actions: NearTransaction.intentDeposit(account: nearAccount, depositAddress: deposit,
                                                                                                             amount: 100_000_000_000_000_000_000_000))
            let nearTx = try await signer.send(.signNearIntentDeposit(amount: "100000000000000000000000", depositAddress: deposit, nonce: 5, blockHash: hash32))
            check(nearTx.signedRaw.map { $0.dropLast(65) == nearBody && nearPublic.isValidSignature($0.suffix(64), for: Data(SHA256.hash(data: nearBody))) } == true,
                  "NEAR → 1Click deposit: the wrap.near transaction matches the reference and its signature verifies")
            check(try await signer.send(.signNearIntentDeposit(amount: "1", depositAddress: "evil.near", nonce: 5, blockHash: hash32)).ok == false,
                  "NEAR to a named account instead of a 1Click deposit not signed")

            // A Ledger's trading key: made in the signer for the Ledger's account, orders only, off until turned on, paused by lock.
            let ledger = "0x" + String(repeating: "1d", count: 20)
            let made = try await signer.send(.ledgerAgent(owner: ledger, create: true))
            check(made.ok && made.agentAddress != nil, "Ledger trading key made in the signer: \(made.agentAddress ?? "none")")
            _ = try await signer.send(.setLedgerAgentActive(owner: ledger, active: false))
            let off = try? await signer.send(.signLedgerAgentL1(owner: ledger, action: cancel, nonce: 1_700_000_000_004))
            check(off?.signature == nil, "Ledger trading key off: nothing signed")
            _ = try await signer.send(.setLedgerAgentActive(owner: ledger, active: true))
            let order = try await signer.send(.signLedgerAgentL1(owner: ledger, action: cancel, nonce: 1_700_000_000_005))
            let signedBy = try order.signature.map { try EVMSigner.recover(digest: try HLSigner.l1Parts(cancel, nonce: 1_700_000_000_005, mainnet: true).digest, signature: $0) }
            check(signedBy != nil && signedBy == made.agentAddress, "Ledger trading key signs an order, as itself")
            let withdraw = HLSigner.usdSendAction(destination: scamEVM, amount: "1", time: 1_700_000_000_006, mainnet: true)
            check((try? await signer.send(.signLedgerAgentL1(owner: ledger, action: withdraw, nonce: 1_700_000_000_006)))?.signature == nil,
                  "Ledger trading key refuses a withdrawal")
            _ = try await signer.send(.lock)
            check((try? await signer.send(.signLedgerAgentL1(owner: ledger, action: cancel, nonce: 1_700_000_000_007)))?.signature == nil,
                  "lock pauses the Ledger trading key")
            check(try await signer.send(.ledgerAgent(owner: ledger, create: false)).agentActive == false, "paused key reported off")

            _ = try await signer.send(.forget)
            check(try await signer.send(.status).hasWallet == false, "key removed from the Keychain")
        } catch {
            print("✗ \(error.localizedDescription)")
            failures += 1
        }
        print(failures == 0 ? "All passed." : "Failures: \(failures)")
        return failures == 0
    }

    static func verifies(_ wire: Data?, owner: String) -> Bool {
        guard let wire, wire.count > 65, let key = try? Curve25519.Signing.PublicKey(rawRepresentation: Base58.decode(owner)) else { return false }
        return key.isValidSignature(wire.subdata(in: 1..<65), for: wire.subdata(in: 65..<wire.count))
    }
}
