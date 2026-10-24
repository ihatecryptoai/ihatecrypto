import Foundation
import IhatecryptoCore
import SignerAPI
import WalletCore

/// «show token 0x…», «add 0x… to favorites», «remove token AERO from favorites»: the shared TokenCommand and
/// DexScreener's TokenLookup, as on the Mac; the favorites' prices refresh with the balances.
nonisolated struct FavoriteToken: Codable, Equatable {
    var chain: String
    var address: String
    var name: String
    var symbol: String
    var image: String?
    var price: Decimal?
    var change24h: Decimal?
    var liquidity: Decimal?
    var id: String { chain + ":" + address.lowercased() }
}

extension Engine {
    static func starKey(_ e: SignerAddress) -> String { e.network + ":" + e.address.lowercased() }

    /// A saved address starred in Favorites › Wallets (kept here: the address book itself is the signer's).
    func star(_ e: SignerAddress, _ on: Bool) {
        if on { starred.insert(Self.starKey(e)) } else { starred.remove(Self.starKey(e)) }
        pin(Board(kind: "favorites", title: "favorites"))
    }

    func tokenCommand(_ command: TokenCommand, note: String?) async {
        switch command {
        case .show(let address, let chain):
            guard let infos = try? await TokenLookup.lookup(address), let info = infos.first(where: { chain == nil || $0.chain == chain }) ?? infos.first else {
                reply(say("DexScreener doesn’t know this token\(chain.map { " on \($0)" } ?? ""): no pool trades it, so there’s no price.",
                          "DexScreener не знает этот токен\(chain.map { " на \($0)" } ?? ""): ни один пул им не торгует, цены нет."), note: note)
                return
            }
            _ = await openToken(info.address, chain: info.chain)
            reply(describe(info) + say(" It’s a contract, not an address to send money to.", " Это контракт, а не адрес для перевода денег."), note: note ?? "DexScreener")
        case .unfavorite(let symbol):
            let before = favorites.count
            favorites.removeAll { $0.symbol.uppercased() == symbol }
            reply(favorites.count < before ? say("\(symbol) is no longer in your favorites.", "\(symbol) убран из избранного.")
                                           : say("There’s no \(symbol) among your favorites.", "\(symbol) нет в избранном."), note: note)
        case .favorite(let address, let chain):
            guard let infos = try? await TokenLookup.lookup(address), let info = infos.first(where: { chain == nil || $0.chain == chain }) ?? infos.first else {
                reply(say("No pool trades \(address), so it isn’t a token to watch. If it’s a wallet, save it in Addresses.",
                          "Ни один пул не торгует \(address), так что это не токен. Если это кошелёк, сохрани его в адресах."), note: note)
                return
            }
            let token = FavoriteToken(chain: info.chain, address: info.address, name: info.name, symbol: info.symbol, image: info.imageURL?.absoluteString,
                                      price: info.priceUSD, change24h: info.change24h, liquidity: info.liquidityUSD)
            favorites.removeAll { $0.id == token.id }
            favorites.append(token)
            pin(Board(kind: "favorites", title: "favorites"))
            reply("⭐ " + describe(info) + say(" It’s in your favorites now.", " Теперь в избранном."), note: note ?? "DexScreener")
        }
    }

    private func describe(_ info: TokenLookup.Info) -> String {
        var line = "\(info.name) (\(info.symbol)) " + say("on", "на") + " \(info.chain): " + (info.priceUSD.map { Fmt.price($0) } ?? say("no price", "нет цены"))
        if let c = info.change24h { line += ", \(signed(c)) " + say("today", "за сутки") }
        if let l = info.liquidityUSD { line += ", " + say("liquidity", "ликвидность") + " \(Fmt.usd((l / 1_000_000).rounded(2, .plain)))M" }
        if TokenLookup.suspicious(info.symbol, contract: info.address, genuine: []) {
            line += say(". ⚠ Its name looks like a well-known coin's but the contract isn't that coin's.", ". ⚠ Имя похоже на известную монету, но контракт не её.")
        }
        return line + "."
    }

    /// The board's one «+» (TokensUI.AddFavoriteButton): what a pasted text is — a token DexScreener prices, a wallet
    /// address (with the network it's saved on by default: Base for a 0x one), or not known while DexScreener is silent.
    func lookToken(_ text: String) async -> [String: String] {
        guard let a = AddressBookCommand.address(in: text) else { return ["kind": "invalid"] }
        let network = a.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil ? "base"
            : AddressCheck.Network.allCases.first { AddressCheck.problem(a, network: $0) == nil }?.rawValue ?? "solana"
        // Bitcoin, TRON, NEAR… are never a DexScreener token.
        guard network == "base" || network == "solana" else { return ["kind": "address", "address": a, "network": network] }
        do {
            guard let info = try await TokenLookup.lookup(a).first else { return ["kind": "address", "address": a, "network": network] }
            var out = ["kind": "token", "address": info.address, "chain": info.chain, "name": info.name, "symbol": info.symbol]
            out["image"] = info.imageURL?.absoluteString
            out["price"] = info.priceUSD.map { "\($0)" }
            out["change24h"] = info.change24h.map { "\($0)" }
            out["liquidity"] = info.liquidityUSD.map { "\($0)" }
            return out
        } catch {
            return ["kind": "unknown", "address": a, "network": network]
        }
    }

    /// The favorites' prices, with the balances (DexScreener, one request each).
    func refreshFavorites() async {
        guard !favorites.isEmpty else { return }
        var next = favorites
        for i in next.indices {
            if let info = (try? await TokenLookup.lookup(next[i].address))?.first(where: { $0.chain == next[i].chain }) {
                next[i].price = info.priceUSD
                next[i].change24h = info.change24h
                next[i].liquidity = info.liquidityUSD
            }
        }
        if next != favorites { favorites = next }
    }
}
