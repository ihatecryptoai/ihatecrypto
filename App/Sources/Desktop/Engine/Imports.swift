import EVMChain
import IhatecryptoCore
import Foundation
import RelayBridge
import TradeExecution
import WalletCore

/// Importing, read before anything is kept (the Mac's ImportWalletView): a private key shown as its address to compare
/// with Phantom or MetaMask before «Save as a wallet», and a phrase's first addresses shown without the signer when the
/// person only wants to see its balances. Neither keeps or sends the secret anywhere.
extension Engine {
    /// The key's chain and address; an error says what's wrong with what was pasted.
    func keyAddress(_ text: String) throws -> [String: String] {
        let key = try ImportedKey.parse(text)
        return ["chain": key.chain, "address": try key.address()]
    }

    /// A phrase's first three addresses, as MetaMask and Phantom list them: shown, nothing saved.
    func phraseAccounts(_ text: String, passphrase: String) throws -> [String: String] {
        let words = text.lowercased().split { !$0.isLetter }.joined(separator: " ")
        _ = try Mnemonic.validate(words)
        let accounts = try Self.firstAccounts(words, passphrase: passphrase)
        return ["accounts": String(data: try JSONSerialization.data(withJSONObject: accounts), encoding: .utf8) ?? "[]"]
    }
}

// MARK: A multisig's cost to make, per network

extension Engine {
    /// What making the drafted multisig costs on each network it names (the Mac's multisigFees): the Safe's deploy gas
    /// at the network's price now, in dollars where Relay prices its coin.
    func multisigFees() async -> [String: String] {
        guard let d = multisigDraft else { return [:] }
        var out: [String: String] = [:]
        for key in d.networks {
            guard let net = EVMNetwork.named(key), let price = try? await net.rpc.quantity("eth_gasPrice", []) else { continue }
            let gas = Decimal(260_000 + 25_000 * d.keys.count), coins = gas * Decimal(price) / 1_000_000_000_000_000_000
            if net.feeToken != nil { out[key] = say("paid in USDC", "в USDC"); continue }
            let usd = ((try? await Relay.nativePrice(chainId: net.rpc.chainId)) ?? nil).map { $0 * coins }
            out[key] = usd.map { $0 < Decimal(string: "0.01")! ? "< $0.01" : "≈ " + Fmt.usd($0) } ?? "≈ \(Fmt.num(coins, maxFraction: 6)) \(net.gasCoin)"
        }
        return out
    }
}
