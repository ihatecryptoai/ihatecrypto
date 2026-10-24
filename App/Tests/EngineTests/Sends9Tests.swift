@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// Where Windows sent and the Mac refused (audit7 S7, S8), and whom a send finds (G20). In-process, the address book,
/// the multisig and the Ledger set by hand; nothing goes to the network or the signer.
@MainActor @Suite(.serialized) struct Sends9Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("sends9-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    // Public test data only: Hardhat's accounts, the abandon-about phrase's Solana address, a NEAR implicit account of zeros.
    static let safe = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let sol = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    static let near = String(repeating: "0", count: 64)

    private func fresh(book: [SignerAddress] = [], safeOn: [String] = []) {
        engine.settings.language = "en"
        engine.pending = nil
        engine.card = nil
        engine.book = book
        engine.multisigs = safeOn.isEmpty ? [] : [Multisig(address: Self.safe.lowercased(), keys: [], threshold: 1, salt: 0, networks: safeOn)]
    }

    private func last() -> String { engine.messages.last?.text ?? "" }

    /// S7: the multisig answers only on a network it's made on — an older build's «Multisig» book entry included.
    @Test func aMultisigNeverAnswersOnANetworkItIsntMadeOn() {
        fresh(book: [SignerAddress(name: "Multisig", network: "base", address: Self.safe)], safeOn: ["base"])
        // An older build's own «Multisig» entry gives way to the Safe itself (multisigEntry), as on the Mac.
        #expect(engine.entry("Multisig", network: "base")?.address.lowercased() == Self.safe.lowercased())
        #expect(engine.entry("Multisig", network: nil)?.network == "base")
        #expect(engine.entry("Multisig", network: "arbitrum") == nil)
        #expect(engine.entry("Multisig", network: "hyperliquid") == nil)
        #expect(engine.entry("the multisig", network: "arbitrum") == nil)
        // The send itself: said, no card.
        engine.prepareTransfer(10, to: "Multisig", network: "arbitrum")
        #expect(engine.card == nil && engine.pending == nil)
        #expect(last().contains("There’s no “Multisig” in Addresses"), "\(last())")
        // Made on Arbitrum too: there it answers.
        fresh(safeOn: ["base", "arbitrum"])
        #expect(engine.entry("Multisig", network: "arbitrum")?.address.lowercased() == Self.safe.lowercased())
    }

    /// S8: an address that needs a memo or tag is refused for SOL and NEAR — alone, in a chain and at the move itself
    /// (where a scheduled run's step arrives).
    @Test func aMemoAddressIsRefusedForSOLAndNEARAloneInAChainAndAtTheMove() async {
        fresh(book: [SignerAddress(name: "Binance", network: "solana", address: Self.sol, memo: "104"),
                     SignerAddress(name: "Binance", network: "near", address: Self.near, memo: "77"),
                     SignerAddress(name: "Mom", network: "solana", address: Self.sol)])
        for c in [WalletCommand.sendSol(name: "Binance", amount: Decimal(string: "0.05")!), .sendNear(name: "Binance", amount: 1), .closeNear(name: "Binance")] {
            #expect(throws: (any Error).self) { try engine.nativeRecipient(c) }
            await engine.handleWallet(c)
            #expect(engine.card == nil, "\(c)")
            #expect(last().contains("needs a memo"), "\(last())")
            do {
                try await engine.move(c)
                Issue.record("\(c) moved")
            } catch {
                #expect(error.localizedDescription.contains("needs a memo"), "\(error)")
            }
        }
        // In a chain: refused before its card.
        _ = await engine.chain(["send 0.05 SOL to Binance", "send 0.01 SOL to Mom"], note: nil)
        #expect(engine.card == nil && engine.pending == nil)
        #expect(last().contains("needs a memo"), "\(last())")
        // An address with no memo goes on.
        #expect((try? engine.nativeRecipient(.sendSol(name: "Mom", amount: 1)))?.address == Self.sol)
    }

    /// G20: SOL and NEAR find recipients as the Mac's savedAddress — on that network, another wallet here, a name that
    /// contains the word — and «my Ledger» is the Ledger by what it is.
    @Test func solAndNearRecipientsAreFoundAsOnTheMac() throws {
        fresh(book: [SignerAddress(name: "Bob", network: "base", address: Self.safe),
                     SignerAddress(name: "Alice savings", network: "solana", address: Self.sol)])
        // Saved only on Base: no NEAR address for him (before, the card drew and the signer refused it).
        #expect(throws: (any Error).self) { try engine.nativeRecipient(.sendNear(name: "Bob", amount: 1)) }
        // A name that contains the word.
        #expect(engine.savedAddress("Alice", on: "solana")?.address == Self.sol)
        #expect(engine.savedAddress("", on: "solana") == nil)
        // «my Ledger» is the Ledger by what it is, whatever it's called (no ledger.json written: it's the app's own file).
        #expect(Engine.meansLedger(Engine.ownKey("my Ledger"), label: "Cold"))
        #expect(Engine.meansLedger(Engine.ownKey("леджер"), label: "Cold"))
        #expect(Engine.meansLedger(Engine.ownKey("Cold"), label: "Cold"))
        #expect(!Engine.meansLedger(Engine.ownKey("my Bob"), label: "Cold"))
    }

    /// A contract call in a chain: its card names the steps after it, which ride on its Confirm; another card's Confirm
    /// never runs them.
    @Test func aContractCallHoldsTheStepsAfterItOnItsCard() async throws {
        fresh()
        let call = try #require(ContractCall.parse("call arbitrum \(Self.safe) deposit(uint256,address) 10e6 me approving 10 USDC"))
        engine.show(.wallet(.contractCall(call)), title: "call")
        let id = try #require(engine.card?.id)
        engine.holdAfterCall(["send 0.01 SOL to Mom"])
        #expect(engine.chainState.afterCall?.board == id && engine.chainState.afterCall?.rest == ["send 0.01 SOL to Mom"])
        #expect(engine.card?.lines.last?.contains("Then, on this Confirm") == true, "\(engine.card?.lines ?? [])")
        // Another board's Confirm: held as it was.
        await engine.continueAfterCall("another")
        #expect(engine.chainState.afterCall != nil)
        engine.cancelPending()
        engine.chainState.afterCall = nil
    }

    /// The exchange's logo from its favicon: the widest PNG frame of 32 px or more, never a BMP one.
    @Test func aFaviconsPNGFrameIsItsLogo() {
        let png = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] + [UInt8](repeating: 7, count: 24))
        func ico(_ frames: [(width: UInt8, data: Data)]) -> Data {
            var d = Data([0, 0, 1, 0, UInt8(frames.count), 0])
            var offset = 6 + frames.count * 16
            for f in frames {
                let size = f.data.count
                d += [f.width, f.width, 0, 0, 1, 0, 32, 0]
                d += [UInt8(size & 0xff), UInt8(size >> 8 & 0xff), 0, 0, UInt8(offset & 0xff), UInt8(offset >> 8 & 0xff), 0, 0]
                offset += size
            }
            for f in frames { d += f.data }
            return d
        }
        let bmp = Data([0x28, 0, 0, 0] + [UInt8](repeating: 1, count: 28))
        #expect(Engine.pngInIcon(ico([(16, png), (48, png), (32, bmp)])) == png)
        #expect(Engine.pngInIcon(ico([(16, png)])) == nil)
        #expect(Engine.pngInIcon(ico([(64, bmp)])) == nil)
        #expect(Engine.pngInIcon(png) == png)
        #expect(Engine.pngInIcon(Data([1, 2, 3])) == nil)
    }
}
