import Foundation
import IhatecryptoCore
import SignerAPI
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
            status = reply
            signerError = nil
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
        defer { signing -= 1; changed() }
        let reply = try await SignerLink.shared.send(request)
        await readStatus()
        guard reply.ok else { throw WalletError(LinkTradeSigner.reason(reply)) }
        Task { await refresh() }
        return reply
    }

    /// A new 12-word phrase made inside the signer. The window shows it once, then never again (`phraseSaved`).
    func createWallet(test: Bool) async throws {
        let reply = try await signerCall(.createWallet(requirePresence: !test))
        guard let words = reply.phrase?.split(separator: " ").map(String.init), words.count == 12 else { return }
        phrase = words
    }

    func importPhrase(_ text: String) async throws {
        let words = text.lowercased().split { !$0.isLetter }.joined(separator: " ")
        _ = try Mnemonic.validate(words)
        try await signerCall(.importMnemonic(phrase: words, passphrase: "", requirePresence: true))
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
    }

    func removeAddress(name: String, network: String) async throws {
        let reply = try await signerCall(.removeAddress(name: name, network: network))
        book = reply.addresses ?? book
    }

    /// The saved entry money may go to by this name, on this network when it's given.
    func saved(_ name: String, on network: String? = nil) -> SignerAddress? {
        book.first { $0.name.lowercased() == name.lowercased() && (network == nil || $0.network == network) }
            ?? book.first { $0.name.lowercased() == name.lowercased() }
    }
}
