import AppKit
import EVMChain
import IhatecryptoCore
import SwiftUI
import WalletCore

/// A token the user watches: by its contract on one chain. Name and symbol come from the chain itself where ihatecrypto can
/// read it (EVM), the icon and the price from DexScreener. Kept on this Mac (tokens.json).
nonisolated struct FavoriteToken: Codable, Identifiable, Hashable {
    var id: String { chain + ":" + address.lowercased() }
    let chain: String
    let address: String
    let name: String
    let symbol: String
    var image: URL?
    var added = Date()

    var rpc: EVMRPC? { ["robinhood": EVMRPC.robinhood, "base": .base, "arbitrum": .arbitrum][chain] }
    var chainTitle: String { ["robinhood": "Robinhood Chain", "base": "Base", "arbitrum": "Arbitrum", "solana": "Solana", "ethereum": "Ethereum"][chain] ?? chain.capitalized }
    var chainLogo: String { ["robinhood": "robinhood", "base": "base", "arbitrum": "arb", "solana": "sol", "ethereum": "eth"][chain] ?? "usdc" }
    var explorer: URL? {
        switch chain {
        case "robinhood": URL(string: "https://robinhoodchain.blockscout.com/token/\(address)")
        case "base": URL(string: "https://basescan.org/token/\(address)")
        case "arbitrum": URL(string: "https://arbiscan.io/token/\(address)")
        case "solana": URL(string: "https://solscan.io/token/\(address)")
        default: URL(string: "https://etherscan.io/token/\(address)")
        }
    }
}

extension Workspace {
    /// «добавь в favorite вот этот токен на робингуде 0x…», «remove token AI from favorites».
    func tokenCommand(_ command: TokenCommand, note: String? = nil) async {
        switch command {
        case .show(let address, let chain):
            guard let info = await openTokenCard(address, chain: chain) else {
                reply("DexScreener doesn’t know this token\(chain.map { " on \($0)" } ?? ""): no pool trades it, so there’s no chart.", note: note)
                return
            }
            reply("\(info.name) (\(info.symbol)) is on the canvas: \(info.priceUSD.map(tokenPrice) ?? "no price")"
                  + (info.change24h.map { ", \(signedPct($0)) today" } ?? "") + ".", note: note)
        case .unfavorite(let symbol):
            let before = favoriteTokens.count
            favoriteTokens.removeAll { $0.symbol.uppercased() == symbol }
            reply(favoriteTokens.count < before ? "\(symbol) is no longer in your favorite tokens." : "There’s no \(symbol) among your favorite tokens.", note: note)
        case .favorite(let address, let chain):
            guard let infos = try? await TokenLookup.lookup(address), let info = infos.first(where: { chain == nil || $0.chain == chain }) ?? infos.first else {
                // No pool and not a token the wallet holds: most likely a wallet, so its save goes into the field to be named.
                if !isOwnWallet(address), !holdings.contains(where: { $0.contract.lowercased() == address.lowercased() }) {
                    reply(say("No pool trades \(address), so it isn’t a token to watch. If it’s a wallet, give it a name — the line is in the field.",
                              "Ни один пул не торгует \(address), так что это не токен. Если это кошелёк, дай ему имя — строка уже в поле."), note: note)
                    suggest("save \(address) as Name" + (address.hasPrefix("0x") ? " on Base" : ""), select: "Name")
                    return
                }
                reply("DexScreener doesn’t know this contract\(chain.map { " on \($0)" } ?? ""): no pool trades it, so there’s no price to watch.", note: note)
                return
            }
            var token = FavoriteToken(chain: info.chain, address: info.address, name: info.name, symbol: info.symbol, image: info.imageURL)
            // What the contract says about itself wins over any listing.
            if let rpc = token.rpc, let s = ABI.string((try? await rpc.read(address, Data([0x95, 0xd8, 0x9b, 0x41]))) ?? Data()),
               let n = ABI.string((try? await rpc.read(address, Data([0x06, 0xfd, 0xde, 0x03]))) ?? Data()) {
                token = FavoriteToken(chain: info.chain, address: info.address, name: n, symbol: s, image: info.imageURL)
            }
            if !favoriteTokens.contains(where: { $0.id == token.id }) { favoriteTokens.append(token) }
            tokenInfo[token.id] = info
            await loadTokenBalances()
            let moved = info.change24h.map { ", \(signedPct($0)) today" } ?? ""
            reply("⭐ \(token.name) (\(token.symbol)) on \(token.chainTitle) is in your favorites: \(info.priceUSD.map(tokenPrice) ?? "no price")\(moved)"
                  + (info.liquidityUSD.map { ", liquidity \(Fmt.usd(($0 / 1_000_000).rounded(1, .plain)))M" } ?? "")
                  + (chain != nil && chain != info.chain ? ". It trades on \(info.chain), not \(chain!)." : ".")
                  + " It’s a contract, not an address to send money to.", note: note)
        }
    }

    /// A Solana token to buy or sell by its mint: refused while Safe search is on; otherwise looked up on Jupiter (decimals,
    /// Token-2022) and its risks said before the card. nil = said why not.
    /// A Solana token named by ticker: the Jupiter-verified one trading under it, then the same checks as by mint.
    func spotToken(_ asset: SpotAsset, note: String?) async -> SpotAsset? {
        guard asset.mint.isEmpty else { return await spotToken(asset.mint, note: note) }
        guard let mint = await TokenResearch.solanaMint(symbol: asset.symbol) else {
            reply(say("Jupiter has no verified token called \(asset.symbol) on Solana, so nothing is bought by that name — namesakes are the usual fakes. Name it by its mint instead (with Safe search off).",
                      "У Jupiter нет проверенного токена \(asset.symbol) на Solana, поэтому по имени ничего не покупаю — одноимённые токены обычно подделки. Назови его mint-адресом (с выключенным Safe search)."), note: note)
            return nil
        }
        return await spotToken(mint, note: note)
    }

    /// A Solana token a swap names — USDC, a ticker or a mint — checked as a buy is (Jupiter's verified list, Safe search),
    /// and kept for the card's quote and the run. nil: said why in the chat (`note`), or quietly when `note` is nil and
    /// it isn't known yet.
    func solanaAsset(_ ref: String, note: String?, quiet: Bool = false) async -> SpotAsset? {
        if ref.uppercased() == "USDC" { return .usdc }
        if let known = spotAssets[ref] { return known }
        guard !quiet else { return nil }
        // USDT, USDS and USDe by their mints: a ticker search can land on a lookalike.
        let ref2 = Stablecoins.find(ref, on: "solana")?.address ?? ref
        guard let asset = await spotToken(ref2.count >= 32 ? .byMint(ref2) : .bySymbol(ref2), note: note) else { return nil }
        spotAssets[ref] = asset
        return asset
    }

    func spotToken(_ mint: String, note: String?) async -> SpotAsset? {
        guard let t = await TokenResearch.solanaToken(mint) else {
            reply(say("Jupiter doesn’t know the token \(mint), so there’s no route to trade it — nothing moved.",
                      "Jupiter не знает токен \(mint), поэтому торговать им нельзя — ничего не ушло."), note: note)
            return nil
        }
        // Safe search on: Jupiter-verified tokens only.
        guard !safeSearch || t.verified else {
            reply(say("Safe search is on, and Jupiter hasn’t verified \(t.name) (\(t.symbol)), so ihatecrypto won’t buy it. To trade an unverified token by its mint, turn Safe search off in Settings › General.",
                      "Включён Safe search, а Jupiter не проверял \(t.name) (\(t.symbol)), поэтому ihatecrypto его не купит. Чтобы торговать непроверенным токеном по mint-адресу, выключи Safe search в Settings › General."), note: note)
            return nil
        }
        // Ondo's own tokens: not the freeze, mint and thin-pool warnings, which an issuer's token always has (its market
        // makers, not the pools, set the price); its terms go on the buy's card (Canvases, the route's footer).
        let warnings = t.verified && Ondo.issued(mint) ? [] : t.warnings(russian: modulesRussian)
        if !warnings.isEmpty { reply("⚠ \(t.name) (\(t.symbol)): " + warnings.joined(separator: "; ") + ".", note: note) }
        return SpotAsset(symbol: t.symbol, mint: mint, decimals: t.decimals, token2022: t.token2022)
    }

    /// Prices and icons for the favorites, at most once a minute.
    func refreshTokens() async {
        guard !favoriteTokens.isEmpty, Date().timeIntervalSince(tokensCheckedAt) > 60 else { return }
        tokensCheckedAt = Date()
        // Collected first, assigned once: every assignment after an await is one more redraw of the window.
        var found: [String: TokenLookup.Info] = [:]
        for t in favoriteTokens {
            if let info = try? await TokenLookup.lookup(t.address).first(where: { $0.chain == t.chain }) { found[t.id] = info }
        }
        // Prices first: the balances take two RPC calls per token and shouldn't hold them back (tokenInfo skips an
        // unchanged merge, so a quiet refresh still redraws nothing).
        tokenInfo.merge(found) { $1 }
        await loadTokenBalances()
    }

    /// What the wallet holds of each favorite on the EVM chains ihatecrypto reads.
    func loadTokenBalances() async {
        guard isEVMAddress(hlAddress) else { return }
        var held: [String: Decimal] = [:]
        for t in favoriteTokens {
            guard let rpc = t.rpc, let raw = try? await rpc.read(t.address, try ERC20.balanceOf(hlAddress)), let units = ABI.decimal(in: raw, word: 0),
                  let d = ABI.uint(in: (try? await rpc.read(t.address, Data([0x31, 0x3c, 0xe5, 0x67]))) ?? Data(), word: 0) else { continue }
            held[t.id] = units / Decimal(sign: .plus, exponent: Int(d), significand: 1)
        }
        tokenBalances.merge(held) { $1 }
    }
}

/// The token's icon from DexScreener, with the chain's logo as a small badge.
struct TokenIcon: View {
    let token: FavoriteToken
    var size: CGFloat = 34

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            RemoteIcon(url: token.image, size: size) {
                Circle().fill(Brand.accent.opacity(0.14)).frame(width: size, height: size)
                    .overlay(Text(token.symbol.prefix(2)).font(.ui(size * 0.36, .bold)).foregroundStyle(Brand.accentInk))
            }
            TokenLogo(name: token.chainLogo, size: size * 0.42).offset(x: 3, y: 3)
        }
        .frame(width: size, height: size)
    }
}

/// The watched tokens as one table: a row per token — its price, 24 h,
/// liquidity and what you hold — sorted by any column. A click opens the token's board above (chart, Buy, Sell, Ask in
/// chat); the right-click menu has the same, plus Copy contract and Remove.
struct FavoriteTokensTable: View {
    @Environment(Workspace.self) private var ws
    let tokens: [FavoriteToken]
    @AppStorage("tokenSort") private var sort = "change"
    @AppStorage("tokenSortUp") private var up = false

    static let price: CGFloat = 84, change: CGFloat = 62, liquidity: CGFloat = 58, held: CGFloat = 72

    /// A held token is valued as the wallet values it (Jupiter on Solana, the portfolio's price elsewhere): DexScreener's
    /// deepest pair can carry a broken price — SPX (Wormhole) at $390 and +111,356 % on 2026-10-16. Only a token the
    /// wallet doesn't hold takes DexScreener's numbers.
    struct Row: Identifiable {
        let token: FavoriteToken
        let info: TokenLookup.Info?
        let holding: Holding?
        let balance: Decimal?
        var id: String { token.id }
        var held: Decimal? { holding?.amount ?? balance }
        var price: Decimal? { holding?.price ?? info?.priceUSD }
        var change: Decimal? { holding.map { $0.change24h } ?? info?.change24h }
        var usd: Decimal? { held.flatMap { h in price.map { $0 * h } } }
    }

    private var rows: [Row] {
        let list = tokens.map { t in
            Row(token: t, info: ws.tokenInfo[t.id],
                holding: ws.holdings.first { $0.chain == t.chain && $0.contract.lowercased() == t.address.lowercased() && $0.price != nil },
                balance: ws.tokenBalances[t.id])
        }
        if sort == "name" {
            return list.sorted { ($0.token.symbol.localizedCaseInsensitiveCompare($1.token.symbol) == .orderedAscending) == up }
        }
        func key(_ r: Row) -> Decimal? {
            switch sort {
            case "price": r.price
            case "liquidity": r.info?.liquidityUSD
            case "held": r.usd
            default: r.change
            }
        }
        // A token with nothing to sort by goes last, either way.
        return list.sorted { a, b in
            switch (key(a), key(b)) {
            case (let x?, let y?): up ? x < y : x > y
            case (_?, nil): true
            default: false
            }
        }
    }

    var body: some View {
        let list = rows
        let held = list.compactMap(\.usd).filter { $0 > 0 }
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                column("TOKEN", "name").frame(maxWidth: .infinity, alignment: .leading)
                column("PRICE", "price").frame(width: Self.price, alignment: .trailing)
                column("24 H", "change").frame(width: Self.change, alignment: .trailing)
                column("LIQUIDITY", "liquidity").frame(width: Self.liquidity, alignment: .trailing)
                column("YOU HOLD", "held").frame(width: Self.held, alignment: .trailing)
            }
            .padding(.horizontal, 14).frame(height: 32)
            .background(Color.primary.opacity(0.04))
            ForEach(list) { r in
                Divider()
                FavoriteTokenRow(row: r)
            }
            if !held.isEmpty {
                Divider()
                HStack {
                    Text("Held in \(held.count) of \(list.count)").font(.ui(11)).foregroundStyle(.secondary)
                    Spacer()
                    Text(masked(Fmt.usd(held.reduce(0, +)))).font(.ui(13, .bold)).monospacedDigit()
                }
                .padding(.horizontal, 14).frame(height: 40)
                .background(Color.primary.opacity(0.04))
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .background(RoundedRectangle(cornerRadius: 14).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Brand.cardBorder))
    }

    /// A column's title sorts by it; again turns the order around.
    private func column(_ title: String, _ key: String) -> some View {
        Button {
            if sort == key { up.toggle() } else { sort = key; up = key == "name" }
        } label: {
            HStack(spacing: 3) {
                Text(title)
                if sort == key { Image(systemName: up ? "arrow.up" : "arrow.down").font(.system(size: 8, weight: .bold)) }
            }
            .font(.ui(9.5, .semibold)).tracking(0.8)
            .foregroundStyle(sort == key ? Brand.accentInk : .secondary)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).pointerStyle(.link)
        .accessibilityLabel("Sort by \(title.lowercased())")
    }
}

/// One watched token in the table. The edge lights on hover and stays while its board is open.
private struct FavoriteTokenRow: View {
    @Environment(Workspace.self) private var ws
    let row: FavoriteTokensTable.Row
    @State private var hover = false

    var body: some View {
        let t = row.token, info = row.info
        let place = TokenBoard.place(chain: t.chain, address: t.address)
        let open = ws.detail == place
        Button { ws.toggleDetail(place) } label: {
            HStack(spacing: 8) {
                HStack(spacing: 10) {
                    TokenIcon(token: t, size: 26)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(t.symbol).font(.ui(13, .bold))
                        Text(t.chainTitle).font(.ui(10)).foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Text(row.price.map(tokenPrice) ?? "—").font(.ui(12.5, .semibold))
                    .frame(width: FavoriteTokensTable.price, alignment: .trailing)
                Text(row.change.map(signedPct) ?? "—").font(.ui(12, .bold))
                    .foregroundStyle(row.change.map { $0 < 0 ? Brand.loss : Brand.profit } ?? .secondary)
                    .frame(width: FavoriteTokensTable.change, alignment: .trailing)
                Text(info?.liquidityUSD.map(AssetStats.compact) ?? "—").font(.ui(12)).foregroundStyle(.secondary)
                    .frame(width: FavoriteTokensTable.liquidity, alignment: .trailing)
                Text(row.usd.map { $0 > 0 ? masked(Fmt.usd($0)) : "—" } ?? row.held.map { $0 > 0 ? masked(Fmt.num($0, maxFraction: 2)) : "—" } ?? "—")
                    .font(.ui(12, .semibold)).foregroundStyle(row.held ?? 0 > 0 ? .primary : .tertiary)
                    .frame(width: FavoriteTokensTable.held, alignment: .trailing)
            }
            .lineLimit(1).minimumScaleFactor(0.7).monospacedDigit()
            .padding(.horizontal, 14).frame(height: 50)
            .contentShape(Rectangle())
            .background(Color.primary.opacity(open ? 0.06 : hover ? 0.035 : 0))
            .overlay {
                RoundedRectangle(cornerRadius: 8).strokeBorder(Brand.accent, lineWidth: 1.5).padding(2)
                    .opacity(open ? 1 : hover ? 0.6 : 0)
            }
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .pointerStyle(.link)
        .help("\(t.name) on \(t.chainTitle): chart, buy, sell")
        .accessibilityLabel("\(t.symbol) on \(t.chainTitle)")
        .contextMenu {
            Group {
                Button { ws.toggleDetail(place) } label: { Label(open ? "Close details" : "Details", systemImage: "info.circle") }
                Button { ws.askAbout(symbol: t.symbol, chain: t.chain, address: t.address) } label: { Label("Ask in chat", systemImage: "bubble.left") }
                let verified = ws.holdings.first { $0.chain == t.chain && $0.contract.lowercased() == t.address.lowercased() }?.verified ?? false
                ForEach(ws.tokenPhrases(chain: t.chain, address: t.address, symbol: t.symbol, stock: TokenBoard.isStock(t.symbol), verified: verified), id: \.phrase) { p in
                    Button { ws.suggest(p.phrase) } label: { Label(p.title, systemImage: p.icon) }
                }
                Divider()
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(t.address, forType: .string)
                } label: { Label("Copy contract", systemImage: "doc.on.doc") }
                if let e = t.explorer { Link(destination: e) { Label("Explorer", systemImage: "arrow.up.right.square") } }
                Button(role: .destructive) {
                    withAnimation(.snappy) { ws.favoriteTokens.removeAll { $0.id == t.id } }
                } label: { Label("Remove from favorites", systemImage: "star.slash") }
            }
            .labelStyle(.titleAndIcon)
        }
    }
}

/// The board's one «+» (the owner, 2026-10-16: «+ Token и + Address — это же практически то же самое»): paste anything
/// and ihatecrypto says what it is — a token DexScreener prices (added to Tokens at once) or a wallet address (named and
/// saved under Touch ID in the editor). Under the field, the tokens the wallet holds and doesn't watch yet, a click each.
struct AddFavoriteButton: View {
    @Environment(Workspace.self) private var ws
    /// Opens the address editor with this address in it.
    let saveAddress: (String) -> Void
    @State private var open = false
    @State private var text = ""
    @State private var found: Found?
    @State private var adding = false
    @State private var retry = 0

    enum Found: Equatable {
        case token(TokenLookup.Info), address(String)
        /// DexScreener didn't answer: whether it's a token isn't known.
        case unknown(String)
    }

    private var address: String? { AddressBookCommand.address(in: text) }

    /// The network an address is saved on by default: Base for a 0x one (one save counts on every EVM network), else
    /// the first whose shape it has. The editor lets it be changed.
    static func network(for address: String) -> AddressCheck.Network {
        if address.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil { return .base }
        return AddressCheck.Network.allCases.first { AddressCheck.problem(address, network: $0) == nil } ?? .solana
    }

    var body: some View {
        Button { open = true } label: { Image(systemName: "plus").font(.system(size: 13, weight: .bold)).frame(width: 18, height: 18) }
            .buttonStyle(.borderedProminent)
            .help("Add a token or a wallet")
            .accessibilityLabel("Add to favorites")
            .popover(isPresented: $open, arrowEdge: .bottom) {
                panel.frame(width: 380, alignment: .leading).padding(16)
            }
    }

    private var panel: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add to favorites").font(.ui(14, .bold))
            HStack {
                TextField("A token’s contract or a wallet address", text: $text)
                    .font(.system(size: 11.5, design: .monospaced)).textFieldStyle(.roundedBorder)
                Button("Paste") { text = NSPasteboard.general.string(forType: .string)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "" }
            }
            result
            held
            Label("Or in the chat: «add 0x… to favorites», «save 0x… as Masha on Base».", systemImage: "bubble.left")
                .font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .task(id: "\(address ?? "")·\(retry)") { await look() }
    }

    /// A token if a pool trades it, else an address. Waits a moment while it's being typed.
    private func look() async {
        found = nil
        guard let a = address else { return }
        guard a.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil || AddressCheck.problem(a, network: .solana) == nil else {
            found = .address(a)   // Bitcoin, TRON, NEAR… are never a DexScreener token.
            return
        }
        try? await Task.sleep(for: .milliseconds(250))
        guard !Task.isCancelled else { return }
        do {
            let infos = try await TokenLookup.lookup(a)
            guard !Task.isCancelled else { return }
            found = infos.first.map(Found.token) ?? .address(a)
        } catch {
            if !Task.isCancelled { found = .unknown(a) }
        }
    }

    @ViewBuilder private var result: some View {
        if !text.isEmpty, address == nil {
            Label("That isn’t an address: paste a 0x…, a Solana key, bc1…, T… or a .near name.", systemImage: "questionmark.circle")
                .font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        } else if address != nil, found == nil {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Looking it up on DexScreener…").font(.ui(10)).foregroundStyle(.secondary)
            }
        } else if case .token(let info)? = found {
            token(info)
        } else if case .address(let a)? = found {
            wallet(a)
        } else if case .unknown(let a)? = found {
            Label("DexScreener didn’t answer, so it isn’t known yet whether this is a token.", systemImage: "wifi.exclamationmark")
                .font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("Try again") { retry += 1 }
                Spacer()
                Button("Save it as an address…") { close(); saveAddress(a) }
            }
        }
    }

    private func token(_ info: TokenLookup.Info) -> some View {
        let t = FavoriteToken(chain: info.chain, address: info.address, name: info.name, symbol: info.symbol, image: info.imageURL)
        let have = ws.favoriteTokens.contains { $0.id == t.id }
        return VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 10) {
                kind("TOKEN", "found on DexScreener", Brand.accentInk)
                HStack(spacing: 10) {
                    TokenIcon(token: t, size: 32)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(info.name).font(.ui(13, .bold)).lineLimit(1)
                        Text("\(info.symbol) · \(t.chainTitle)").font(.ui(10)).foregroundStyle(.secondary)
                    }
                    Spacer()
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(info.priceUSD.map(tokenPrice) ?? "no price").font(.ui(13, .bold)).monospacedDigit()
                        if let c = info.change24h { Text(signedPct(c)).font(.ui(10, .semibold)).foregroundStyle(c < 0 ? Brand.loss : Brand.profit) }
                    }
                }
                Text("A contract to watch, not an address to send money to.").font(.ui(10)).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.primary.opacity(0.05)))
            HStack {
                Button("Save it as an address instead") { close(); saveAddress(info.address) }
                    .buttonStyle(.link).font(.ui(10))
                Spacer()
                if adding { ProgressView().controlSize(.small) }
                Button {
                    adding = true
                    Task {
                        await ws.tokenCommand(.favorite(address: info.address, chain: info.chain))
                        adding = false
                        close()
                    }
                } label: { Label(have ? "In your tokens" : "Add token", systemImage: "star.fill") }
                    .buttonStyle(.borderedProminent).disabled(have || adding)
            }
        }
    }

    private func wallet(_ a: String) -> some View {
        let network = Self.network(for: a)
        let own = ws.isOwnWallet(a)
        let same = ws.addressBook.first { $0.address.lowercased() == a.lowercased() }
        return VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 10) {
                kind("WALLET ADDRESS", network.title + (a.hasPrefix("0x") || network == .solana ? " · no token trades at it" : ""), .primary)
                AddressLines(address: a)
                if own {
                    Label("This is your own wallet: it’s under Wallets already.", systemImage: "person.crop.circle.badge.checkmark")
                        .font(.ui(10)).foregroundStyle(.secondary)
                } else if let same {
                    Label("Already saved as “\(same.name)”.", systemImage: "checkmark.circle").font(.ui(10)).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.primary.opacity(0.05)))
            HStack {
                Spacer()
                Button { close(); saveAddress(a) } label: { Label("Name it and save…", systemImage: Presence.icon) }
                    .buttonStyle(.borderedProminent).disabled(own || same != nil)
                    .help("A name, its network; the \(Presence.short) prompt shows the whole address before it's saved")
            }
        }
    }

    /// The tokens the wallet holds that aren't watched: one click puts each in Tokens. Dollars and gas coins aren't offered.
    @ViewBuilder private var held: some View {
        let watched = Set(ws.favoriteTokens.map(\.id))
        let list = ws.holdings.filter { h in
            !h.native && h.contract != "native" && !["hyperliquid", "near"].contains(h.chain) && h.looksReal
                && !Stablecoins.dollars.contains(h.symbol.uppercased()) && !watched.contains(h.chain + ":" + h.contract.lowercased())
        }
        .sorted { ($0.usd ?? 0) > ($1.usd ?? 0) }.prefix(5)
        if address == nil, !list.isEmpty {
            Divider()
            Text("TOKENS YOU HOLD").font(.ui(9.5, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            VStack(spacing: 2) {
                ForEach(Array(list)) { h in
                    let t = FavoriteToken(chain: h.chain, address: h.contract, name: h.symbol, symbol: h.symbol, image: h.icon)
                    Button {
                        adding = true
                        Task {
                            await ws.tokenCommand(.favorite(address: h.contract, chain: h.chain))
                            adding = false
                        }
                    } label: {
                        HStack(spacing: 10) {
                            TokenIcon(token: t, size: 24)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(h.symbol).font(.ui(12, .semibold))
                                Text(t.chainTitle).font(.ui(10)).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Text(h.usd.map { masked(Fmt.usd($0)) } ?? "").font(.ui(11)).foregroundStyle(.secondary).monospacedDigit()
                            Image(systemName: "star").font(.system(size: 13)).foregroundStyle(Color(hex: 0xF5B301))
                        }
                        .padding(.horizontal, 8).frame(height: 38).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain).modifier(HoverLift(radius: 8)).disabled(adding)
                    .help("Watch \(h.symbol)").accessibilityLabel("Watch \(h.symbol)")
                }
            }
        }
    }

    private func kind(_ title: String, _ note: String, _ color: Color) -> some View {
        HStack(spacing: 8) {
            Text(title).font(.ui(9.5, .bold)).tracking(1).foregroundStyle(color)
                .padding(.horizontal, 7).padding(.vertical, 3)
                .background(RoundedRectangle(cornerRadius: 5).fill(color.opacity(0.12)))
            Text(note).font(.ui(10)).foregroundStyle(.secondary).lineLimit(1)
        }
    }

    private func close() {
        open = false
        text = ""
        found = nil
    }
}

/// Token prices keep their meaningful digits: $148.64, $0.2366, $0.00001234.
func tokenPrice(_ d: Decimal) -> String {
    if d >= 1 { return Fmt.usd(d) }
    var digits = 2
    var x = d
    while x < 1, x > 0, digits < 10 { x *= 10; digits += 1 }
    return "$" + Fmt.num(d, maxFraction: min(digits + 2, 12))
}

/// The rosette with a check: the real token, verified by a list that can't be faked by opening a pool (Jupiter's on
/// Solana, Robinhood's registry for stock tokens, a market list elsewhere). Copies under the same name never get it.
/// Blue like the verified marks people already know, so it reads at a glance.
struct VerifiedBadge: View {
    var size: CGFloat = 12
    var source = "the token list"

    var body: some View {
        Image(systemName: "checkmark.seal.fill")
            .font(.system(size: size, weight: .semibold))
            .symbolRenderingMode(.palette)
            .foregroundStyle(.white, Color(hex: 0x1D9BF0))
            .help("Verified by \(source): the real token, not a copy under its name")
            .accessibilityLabel("verified")
    }
}
