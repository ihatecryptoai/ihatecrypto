import EVMChain
import IhatecryptoCore
import SwiftUI
import TradeExecution
import WalletCore

/// A Stock Token held on Robinhood Chain, priced by Robinhood's own quote, or a token bought by its contract.
struct StockHolding: Identifiable, Equatable {
    var id: String { symbol }
    let symbol: String
    let name: String
    /// The contract on Robinhood Chain.
    let address: String
    let amount: Decimal
    let price: Decimal?
    let logo: URL?
    var usd: Decimal { price.map { $0 * amount } ?? 0 }
}

extension Workspace {
    /// Symbols bought through ihatecrypto; their balances are read with every refresh.
    func rememberStock(_ symbol: String) {
        var list = UserDefaults.standard.stringArray(forKey: "stockTokens") ?? []
        if !list.contains(symbol.uppercased()) { list.append(symbol.uppercased()) }
        UserDefaults.standard.set(list, forKey: "stockTokens")
        Task { await loadRobinhood() }
    }

    /// USDG, ETH for gas, the remembered Stock Tokens and the favorite tokens held on Robinhood Chain.
    func loadRobinhood() async {
        guard isEVMAddress(hlAddress) else { return }
        let rpc = EVMRPC.robinhood, me = hlAddress
        guard let usdg = try? await rpc.tokenBalance(EVMRPC.robinhoodUSDG, owner: me), let eth = try? await rpc.balance(me) else { return }
        var holdings: [StockHolding] = []
        let symbols = UserDefaults.standard.stringArray(forKey: "stockTokens") ?? []
        if !symbols.isEmpty, let registry = try? await RobinhoodStocks.registry() {
            for symbol in symbols {
                guard let t = registry.first(where: { $0.symbol == symbol }),
                      let raw = try? await rpc.read(t.address, try ERC20.balanceOf(me)), let units = ABI.decimal(in: raw, word: 0), units > 0 else { continue }
                let amount = units / Decimal(sign: .plus, exponent: t.decimals, significand: 1)
                let price = (try? await RobinhoodStocks.quote(symbol))?.bid
                holdings.append(StockHolding(symbol: symbol, name: t.name, address: t.address, amount: amount, price: price, logo: t.logo))
            }
        }
        // Tokens bought by their contract (favorites on Robinhood Chain), priced by DexScreener.
        // Collected first, assigned together: every assignment after an await is one more redraw of the window.
        var held: [String: Decimal] = [:], found: [String: TokenLookup.Info] = [:]
        for t in favoriteTokens where t.chain == "robinhood" {
            guard let raw = try? await rpc.read(t.address, try ERC20.balanceOf(me)), let units = ABI.decimal(in: raw, word: 0), units > 0,
                  let d = ABI.decimals(try? await rpc.read(t.address, Data([0x31, 0x3c, 0xe5, 0x67]))) else { continue }
            let amount = units / Decimal(sign: .plus, exponent: d, significand: 1)
            held[t.id] = amount
            if tokenInfo[t.id]?.priceUSD == nil, let info = try? await TokenLookup.lookup(t.address).first(where: { $0.chain == "robinhood" }) {
                found[t.id] = info
            }
            holdings.append(StockHolding(symbol: t.symbol, name: t.name, address: t.address, amount: amount, price: (found[t.id] ?? tokenInfo[t.id])?.priceUSD, logo: t.image))
        }
        land { [self] in
            guard me == hlAddress else { return }
            tokenBalances.merge(held) { $1 }
            tokenInfo.merge(found) { $1 }
            robinhood = (Decimal(usdg) / 1_000_000, Decimal(eth) / Decimal(sign: .plus, exponent: 18, significand: 1), holdings)
        }
    }

    var robinhoodTotal: Decimal { robinhood.map { $0.usdg + $0.stocks.reduce(0) { $0 + $1.usd } } ?? 0 }
}

/// The company's logo bundled by ticker, else the registry's (Robinhood's own) logo, or a drawn badge while it loads.
struct StockLogo: View {
    let symbol: String
    let url: URL?
    var size: CGFloat = 26

    var body: some View {
        // Robinhood's registry gives every Stock Token the same Robinhood logo, so the company's own logo, when
        // bundled by ticker (Logos/spcx.svg, tsla.png…), comes first.
        if let bundled = TokenLogo.name(forAsset: symbol) {
            TokenLogo(name: bundled, size: size)
        } else {
            RemoteIcon(url: url, size: size, contentMode: .fit) {
                AssetBadge(coin: symbol, size: size, dark: true)
            }
        }
    }
}
