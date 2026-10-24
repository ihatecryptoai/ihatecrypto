import Foundation
import HyperliquidSigning
import IhatecryptoCore
import SignerAPI
import TradeExecution
import WalletCore

/// Wallets and the address book: everything here is the signer's, read and changed only through it. The engine keeps
/// what the signer last said, for the window to draw.
extension Engine {
    func startSigner() async {
        guard SignerLink.isAvailable else {
            signerError = say("The signer isn't installed next to the engine: reinstall ihatecrypto.", "Подписант не установлен рядом с движком: переустанови ihatecrypto.")
            return
        }
        await readStatus()
        await readBook()
    }

    func readStatus() async {
        do {
            let reply = try await SignerLink.shared.send(.status)
            guard reply.ok else { throw WalletError(LinkTradeSigner.reason(reply)) }
            statusHeard(reply)
            lockHeard(codeSet: reply.codeSet == true)
            signerError = nil
            await readLedgerAgent()
        } catch {
            signerError = error.localizedDescription
        }
        changed()
    }

    func readBook() async {
        if let reply = try? await SignerLink.shared.send(.addressBook), reply.ok { book = reply.addresses ?? [] }
        changed()
    }

    /// A request that changes the signer's state: its refusal becomes the error the window shows, and the status is read
    /// again after it either way.
    @discardableResult
    func signerCall(_ request: SignerRequest) async throws -> SignerReply {
        signing += 1
        changed()
        // Windows Hello may ask: the window lets the signer, and Hello's dialog, come forward (as for a move's signature).
        if let pid = await SignerLink.shared.pid { out.event("signerForeground", ["pid": "\(pid)"]) }
        defer { signing -= 1; changed() }
        let reply = try await SignerLink.shared.send(request)
        await readStatus()
        guard reply.ok else { throw WalletError(LinkTradeSigner.reason(reply)) }
        Task { await refresh() }
        return reply
    }

    private static var creatingWallet = false

    /// A new 12-word phrase made inside the signer. The window shows it once, then never again (`phraseSaved`).
    func createWallet(test: Bool) async throws {
        // One at a time, and never over words not yet written down: a second click made a second wallet whose words
        // replaced the first's, so the first wallet's were never shown (the Mac's Create stays busy while the signer works).
        guard !Self.creatingWallet, phrase == nil else {
            throw WalletError(say("A new wallet is being made: write its words down first.", "Новый кошелёк уже создаётся: сначала запиши его слова."))
        }
        Self.creatingWallet = true
        defer { Self.creatingWallet = false }
        let reply = try await signerCall(.createWallet(requirePresence: !test))
        guard let words = reply.phrase?.split(separator: " ").map(String.init), words.count == 12 else { return }
        phrase = words
    }

    /// A phrase, with its BIP-39 passphrase if the wallet has one (taken as typed: spaces count), goes to the signer; the
    /// answer carries the first addresses it makes, to compare with the wallet it came from (the Mac's ImportWalletView).
    func importPhrase(_ text: String, passphrase: String = "") async throws -> [String: String] {
        let words = text.lowercased().split { !$0.isLetter }.joined(separator: " ")
        _ = try Mnemonic.validate(words)
        try await signerCall(.importMnemonic(phrase: words, passphrase: passphrase, requirePresence: true))
        let accounts = try Self.firstAccounts(words, passphrase: passphrase)
        return ["accounts": String(data: try JSONSerialization.data(withJSONObject: accounts), encoding: .utf8) ?? "[]"]
    }

    /// The first `count` EVM (m/44'/60'/0'/0/i) and Solana (m/44'/501'/i'/0') addresses a phrase makes, as MetaMask and
    /// Phantom list them.
    nonisolated static func firstAccounts(_ words: String, passphrase: String, count: Int = 3) throws -> [[String: String]] {
        let seed = try Mnemonic.seed(words, passphrase: passphrase)
        return try (0..<count).map { i in
            ["evm": try EVMAccount.address(privateKey: Derivation.secp256k1(seed: seed, path: WalletPath.evm(i))),
             "solana": try SolanaAccount.address(privateKey: Derivation.ed25519(seed: seed, path: WalletPath.solana(i)))]
        }
    }

    /// Whether the wallet's Hyperliquid account has had a deposit: only then does Hyperliquid take a trading key, so the
    /// onboarding shows its trading steps only then (the Mac's hlFunded). Empty when it can't be read.
    func hlFunded() async -> [String: String] {
        guard !watching, let evm = activeWallet?.evm, let new = try? await Hyperliquid.isNewAccount(evm) else { return ["funded": ""] }
        return ["funded": new ? "false" : "true"]
    }

    /// Dollars where the fee coin is missing (GasWays.swift), for the onboarding's Top up: the first move there would stop.
    var stuckView: [StuckView] {
        stuckDollars.map { StuckView(network: $0.network, key: $0.key, coin: $0.coin, usd: $0.usd, usdc: $0.usdc, ways: $0.ways.count) }
    }

    /// The rail's «Switch» and Settings' «Use»: said in the chat, a refusal too, as the Mac's switchWallet. The automation
    /// wallet goes to the signer as on the Mac, so its refusal says why («signs only scheduled tasks' moves»), with the
    /// Ledger left as it was. A refusal throws the chat's own line: Settings, which covers the chat here, shows it as its
    /// toast, and the onboarding's Ledger goes on only after a switch; the rail, beside the chat, leaves it there.
    func useWallet(_ id: String) async throws {
        if let w = switchableWallets.first(where: { $0.id == id }) {
            await switchWallet(to: w)
        } else if status?.wallets?.contains(where: { $0.id == id }) == true {
            do { try await signerCall(.setActiveWallet(id: id)) } catch {
                reply(say("The wallet wasn't switched: ", "Кошелёк не переключён: ") + error.localizedDescription)
            }
        } else {
            reply(say("That wallet isn't here any more.", "Этого кошелька здесь больше нет."))
        }
        let now = ledgerActive ? LedgerAccount.id : status?.activeWallet
        if now != id { throw WalletError(messages.last { !$0.fromUser }?.text ?? say("The wallet wasn't switched.", "Кошелёк не переключён.")) }
    }

    func importKey(_ key: String) async throws {
        try await signerCall(.importPrivateKey(key: key.trimmingCharacters(in: .whitespacesAndNewlines)))
    }

    /// A saved address goes into the signer's book after Windows Hello with the whole address on the prompt.
    func saveAddress(_ entry: SignerAddress) async throws {
        guard let net = AddressCheck.Network(rawValue: entry.network) else { throw WalletError(say("Pick the address's network.", "Выбери сеть адреса.")) }
        if let problem = AddressCheck.problem(entry.address, network: net) { throw WalletError(problem) }
        let reply = try await signerCall(.saveAddresses([entry]))
        book = reply.addresses ?? book
        // What a save from the chat brought (an ENS name's note, an exchange's logo) goes beside it now that it's saved.
        let key = Self.extraKey(name: entry.name, network: entry.network)
        if let extra = Self.savingExtras.removeValue(forKey: key), extra != AddressExtra(), addressExtras[key] == nil { addressExtras[key] = extra }
    }

    /// Out of the signer's book only: an edit (editAddress) moves the picture, the «from chat» mark and the star to the new
    /// name after this, as the Mac's edit keeps them on the same entry.
    func removeAddress(name: String, network: String) async throws {
        let reply = try await signerCall(.removeAddress(name: name, network: network))
        book = reply.addresses ?? book
    }

    /// The book's Delete: the entry, and its picture, category, note and star with it, as the Mac's removal does.
    func deleteAddress(name: String, network: String) async throws {
        let gone = book.first { $0.name.lowercased() == name.lowercased() && $0.network == network }
        try await removeAddress(name: name, network: network)
        var extras = addressExtras
        extras[Self.extraKey(name: name, network: network)] = nil
        addressExtras = extras
        if let gone { starred.remove(Self.starKey(gone)) }
    }

    /// The saved entry money may go to by this name, on this network when it's given.
    func saved(_ name: String, on network: String? = nil) -> SignerAddress? {
        book.first { $0.name.lowercased() == name.lowercased() && (network == nil || $0.network == network) }
            ?? book.first { $0.name.lowercased() == name.lowercased() }
    }
}

nonisolated struct StuckView: Encodable, Equatable {
    var network: String
    var key: String
    var coin: String
    /// What to add of the coin, in dollars, and the dollars there.
    var usd: Decimal
    var usdc: Decimal
    /// Cards the chat offers to move them without the coin; 0: only adding it works there.
    var ways: Int
}
