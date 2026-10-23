import EVMChain
import Foundation
import IhatecryptoCore
import SolanaChain
import TradeExecution
import WalletCore
#if os(Windows)
import WinSDK
#elseif canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

/// Top up, as the Mac's ReceiveBoard and TopUpPage: what this wallet holds of the chosen coin on the chosen network
/// (read from the network itself, for «now → after»), whether dollars there would be stuck without the gas coin, and —
/// once a way is used — the balance watched every 4 s until money lands. «From a browser wallet» is a page this PC serves
/// to its own browser on 127.0.0.1 (TopUpPage below), so the recipient comes from here and no website can swap it.
nonisolated struct TopUpView: Encodable, Equatable {
    struct Sent: Encodable, Equatable { var id: String; var link: String? }
    /// A network the wallet receives on, for the window's chips and its pay links (TopUpOrder.payLink).
    struct Net: Encodable, Equatable {
        var key: String
        var title: String
        var chainId: UInt64?
        var usdc: String
        var usdcSymbol: String
        var usdcDecimals: Int
        var coin: String?
        var recipient: String
        /// Dollars there move by signature, Relay paying the gas (Permit3009): the gas line says so.
        var bySignature: Bool
    }
    var networks: [Net] = []
    var network: String
    var coin: String
    /// The coin at this wallet there, by symbol (USDC and the gas coin for «any coin»); empty until the network answers.
    var held: [String: Decimal] = [:]
    /// The network's gas coin when the wallet holds less of it than a move there needs.
    var gasShort: String?
    /// A way was used (a QR or an address shown, the page opened): the balance is watched.
    var waiting = false
    /// «Landed on Solana: +20 USDC, 5 → 25».
    var arrived: String?
    /// What the browser page said it sent.
    var sent: Sent?
}

extension TopUpOrder.Network {
    init(_ n: EVMNetwork) {
        self.init(key: n.key, title: n.name, chainId: n.rpc.chainId, usdc: n.usdc, usdcSymbol: EVMRPC.dollars[n.usdc] ?? "USDC",
                  usdcDecimals: n.usdcDecimals, coin: n.feeToken == nil ? n.gasCoin : nil, explorer: n.explorer, rpc: n.rpc.url.absoluteString,
                  // A USDC transfer on Ethereum: ≈ 1¢ at 0.07 gwei (2026-10-15), ≈ 15¢ at 0.74 (2026-09-29); the rest stay under a cent.
                  cost: n.key == "ethereum" ? "fee 1–20¢ · lands in ~15 s" : "fee under 1¢ · lands in seconds")
    }
}

extension Engine {
    /// Solana, then the EVM networks as a search for money takes them; a network off in Settings › Modules isn't offered.
    var topUpNetworks: [TopUpOrder.Network] {
        let solana = solanaAddress.map { AddressCheck.problem($0, network: .solana) == nil } == true && Modules.current.isOn(.solana)
        let evm = evmAddress.map { AddressCheck.problem($0, network: .base) == nil } == true
        return (solana ? [.solana] : []) + (evm ? EVMNetwork.searched.map(TopUpOrder.Network.init) : [])
    }

    func topUpOrder(network: String?, coin: String?, amount: String?) throws -> TopUpOrder {
        let nets = topUpNetworks
        guard let net = nets.first(where: { $0.key == network }) ?? nets.first else {
            throw WalletError(say("Add a wallet first: Settings › Wallet.", "Сначала добавь кошелёк: Настройки › Кошелёк."))
        }
        let typed = (amount ?? "").replacingOccurrences(of: ",", with: ".").trimmingCharacters(in: .whitespaces)
        return TopUpOrder(network: net, coin: TopUpOrder.Coin(rawValue: coin ?? "") ?? .usdc, amount: typed.isEmpty ? nil : Decimal(string: typed) ?? 0,
                          recipient: (net.chainId == nil ? solanaAddress : evmAddress) ?? "")
    }

    /// The page shows `network`/`coin`; `watch` once a way is used. The same three again change nothing.
    func topUp(network: String?, coin: String?, watch: Bool) throws {
        let o = try topUpOrder(network: network, coin: coin, amount: nil)
        TopUpPage.shared.show(o, watch: watch, engine: self)
    }

    /// The browser-wallet way: a page for this order at a new address; a page opened before stops working.
    func topUpOpen(network: String?, coin: String?, amount: String?) async throws -> String {
        let o = try topUpOrder(network: network, coin: coin, amount: amount)
        guard o.units != nil else {
            throw WalletError(say("Pick a coin and an amount above zero first.", "Сначала выбери монету и сумму больше нуля."))
        }
        let url = try TopUpPage.shared.open(o, engine: self)
        TopUpPage.shared.show(o, watch: true, engine: self)
        return url
    }

    func topUpStop() { TopUpPage.shared.stop() }
}

@MainActor final class TopUpPage {
    static let shared = TopUpPage()
    private(set) var view: TopUpView?
    private weak var engine: Engine?
    /// What's on screen and watched.
    private var shown: (order: TopUpOrder, watch: Bool)?
    private var watching: Task<Void, Never>?
    /// The page being served: its order and its one-time path.
    private var server: PageServer?
    private var order: TopUpOrder?
    private var secret = ""

    func show(_ o: TopUpOrder, watch: Bool, engine: Engine) {
        self.engine = engine
        if let shown, shown.order.network == o.network, shown.order.coin == o.coin, shown.watch || !watch { return }
        let same = view?.network == o.network.key && view?.coin == o.coin.rawValue
        let nets = engine.topUpNetworks.map { n in
            TopUpView.Net(key: n.key, title: n.title, chainId: n.chainId, usdc: n.usdc, usdcSymbol: n.usdcSymbol, usdcDecimals: n.usdcDecimals, coin: n.coin,
                          recipient: (n.chainId == nil ? engine.solanaAddress : engine.evmAddress) ?? "",
                          bySignature: n.chainId.map { Permit3009.supports(chainId: $0, token: n.usdc) } ?? false)
        }
        view = TopUpView(networks: nets, network: o.network.key, coin: o.coin.rawValue, held: same ? view?.held ?? [:] : [:], gasShort: same ? view?.gasShort : nil,
                         waiting: watch, arrived: nil, sent: same ? view?.sent : nil)
        shown = (o, watch)
        watching?.cancel()
        watching = Task { await self.watch(o, waiting: watch) }
        engine.changed()
    }

    func stop() {
        watching?.cancel()
        server?.stop()
        (watching, server, order, secret, shown, view) = (nil, nil, nil, "", nil, nil)
        engine?.changed()
    }

    /// Reads the order's balance (shown as now → after) and its gas; once a way is used, again every 4 s for up to 20 min.
    /// When it rises, a line says what landed and the whole portfolio is read again (which says in the chat what came in).
    private func watch(_ o: TopUpOrder, waiting: Bool) async {
        async let gas = Self.gasShort(o)
        let before = await Self.balances(o)
        let short = await gas
        guard !Task.isCancelled else { return }
        view?.held = before
        view?.gasShort = short
        engine?.changed()
        guard waiting, !before.isEmpty else { return }
        for _ in 0..<300 {
            try? await Task.sleep(for: .seconds(4))
            if Task.isCancelled { return }
            let now = await Self.balances(o)
            let up = now.compactMap { symbol, value in before[symbol].flatMap { value > $0 ? (symbol, $0, value) : nil } }
            guard !up.isEmpty, !Task.isCancelled else { continue }
            let digits = { (s: String) in s == o.network.usdcSymbol ? 2 : 6 }
            let lines = up.map { s, was, after in
                "+\(Fmt.num(after - was, maxFraction: digits(s))) \(s), \(Fmt.num(was, maxFraction: digits(s))) → \(Fmt.num(after, maxFraction: digits(s)))"
            }.joined(separator: "; ")
            view?.held = now
            view?.arrived = (engine?.say("Landed on \(o.network.title): ", "Пришло в \(o.network.title): ") ?? "") + lines
            engine?.changed()
            await engine?.refresh()
            return
        }
    }

    // MARK: Reading the network

    /// The order's coin at its recipient, by symbol, from the network: USDC and the gas coin for «any coin». Empty when
    /// the network didn't answer.
    nonisolated static func balances(_ o: TopUpOrder) async -> [String: Decimal] {
        let net = o.network, coins: [TopUpOrder.Coin] = o.coin == .any ? [.usdc, .native] : [o.coin]
        var out: [String: Decimal] = [:]
        for coin in coins where coin == .usdc || net.coin != nil {
            let one = TopUpOrder(network: net, coin: coin, amount: nil, recipient: o.recipient)
            var raw: Decimal?
            if net.chainId == nil {
                raw = coin == .usdc ? (try? await SolanaRPC().tokenBalance(owner: o.recipient, mint: net.usdc)).map { Decimal($0) }
                                    : (try? await SolanaRPC().balance(o.recipient)).map { Decimal($0) }
            } else if let rpc = EVMNetwork.named(net.key)?.rpc {
                raw = coin == .usdc ? (try? await rpc.read(net.usdc, ERC20.balanceOf(o.recipient))).flatMap { ABI.decimal(in: $0, word: 0) }
                    : (try? await rpc.call("eth_getBalance", [o.recipient, "latest"]) as? String)
                        .map { $0.dropFirst(2).reduce(Decimal(0)) { $0 * 16 + Decimal(Int(String($1), radix: 16) ?? 0) } }
            }
            if let raw { out[one.symbol] = raw / pow(10, one.decimals) }
        }
        return out
    }

    /// The network's gas coin when this wallet holds less of it than a move needs there (nil: enough, or no gas coin).
    nonisolated static func gasShort(_ o: TopUpOrder) async -> String? {
        guard let coin = o.network.coin else { return nil }
        let floor = o.network.chainId == nil ? Decimal(TopUpOrder.solanaFeeFloor) / 1_000_000_000
            : EVMNetwork.named(o.network.key).map { Decimal($0.gasFloor) / 1_000_000_000_000_000_000 } ?? 0
        guard let held = await balances(TopUpOrder(network: o.network, coin: .native, amount: nil, recipient: o.recipient))[coin] else { return nil }
        return held < floor ? coin : nil
    }

    // MARK: The page

    /// Serves a page for `o` at a new random path and returns its address; the path before it stops working.
    func open(_ o: TopUpOrder, engine: Engine) throws -> String {
        self.engine = engine
        if server == nil { server = try PageServer.start { request in await TopUpPage.shared.handle(request) } }
        guard let server else { throw WalletError("The page couldn't start.") }
        order = o
        view?.sent = nil
        secret = (0..<16).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
        return "http://127.0.0.1:\(server.port)/\(secret)"
    }

    /// Only this page, as the Mac's: its own address in Host (a name pointed at 127.0.0.1 shows another, so a site can't
    /// rebind to it), its own origin on a POST (no other site can post here), the order's path, which changes with each
    /// page, and the recipient always the order's.
    func handle(_ r: PageRequest) async -> Data {
        func text(_ status: Int, _ s: String) -> Data { PageServer.response(status, "text/plain; charset=utf-8", Data(s.utf8)) }
        func json(_ value: Any) -> Data {
            PageServer.response(200, "application/json", (try? JSONSerialization.data(withJSONObject: value)) ?? Data("{}".utf8))
        }
        guard let order, let server, !secret.isEmpty else { return text(403, "This page has closed: open it again from ihatecrypto.") }
        let root = "/" + secret
        guard Self.allows(r, port: server.port) else { return text(403, "This page has closed: open it again from ihatecrypto.") }
        let body = (try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any]
        switch (r.method, r.path) {
        case ("GET", root):
            return PageServer.response(200, "text/html; charset=utf-8", Data(Self.page(order).utf8))
        case ("POST", root + "/solana"):
            guard order.network.chainId == nil, let from = body?["from"] as? String, AddressCheck.problem(from, network: .solana) == nil else {
                return text(400, "No Solana account")
            }
            // The page's amount («Send more» picks a new one); the recipient is always this order's.
            guard let units = (body?["units"] as? String).flatMap(UInt64.init), units > 0 else { return text(400, "No amount to send") }
            do {
                return json(["transaction": try await Self.solanaTransfer(order, units: units, from: from).base64EncodedString()])
            } catch {
                return text(502, (error as? WalletError)?.message ?? error.localizedDescription)
            }
        case ("POST", root + "/sent"):
            guard let id = body?["id"] as? String, id.wholeMatch(of: /0x[0-9a-fA-F]{64}|[1-9A-HJ-NP-Za-km-z]{64,90}/) != nil else {
                return text(400, "No transfer id")
            }
            if view?.network == order.network.key { view?.sent = TopUpView.Sent(id: id, link: order.explorer(id)?.absoluteString) }
            engine?.changed()
            return json([:])
        case ("POST", root + "/balance"):
            // The order's coin at the sending account and at this wallet, for «Send more»'s now → after.
            guard let from = body?["from"] as? String,
                  order.network.chainId == nil ? AddressCheck.problem(from, network: .solana) == nil : from.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else {
                return text(400, "No account")
            }
            async let mine = Self.balances(order)
            async let theirs = Self.balances(TopUpOrder(network: order.network, coin: order.coin, amount: nil, recipient: from))
            return json(["to": await mine[order.symbol], "from": await theirs[order.symbol]].compactMapValues { $0.map { "\($0)" } })
        default:
            return text(404, "Not here")
        }
    }

    /// The request comes from this page itself: its own address in Host, and on a POST its own origin.
    nonisolated static func allows(_ r: PageRequest, port: UInt16) -> Bool {
        let me = "127.0.0.1:\(port)"
        return r.headers["host"] == me && (r.method == "GET" || r.headers["origin"] == "http://\(me)")
    }

    /// The transfer as the connected wallet will sign it: `from` pays the fee, USDC goes by TransferChecked into this
    /// wallet's account (made if missing), SOL by a system transfer. USDC to a wallet with almost no SOL brings
    /// `TopUpOrder.solanaFeeTopUp` along in the same transaction: USDC alone can't move on Solana.
    nonisolated static func solanaTransfer(_ order: TopUpOrder, units: UInt64, from: String) async throws -> Data {
        let rpc = SolanaRPC()
        let blockhash = try await rpc.latestBlockhash().blockhash
        if order.coin == .native {
            let transfer = SolanaInstruction(program: system, accounts: [.init(pubkey: from, isSigner: true, isWritable: true),
                                                                         .init(pubkey: order.recipient, isSigner: false, isWritable: true)],
                                             data: le(UInt32(2)) + le(units))
            return SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([transfer], payer: from, blockhash: blockhash))
        }
        if let sol = try? await rpc.balance(order.recipient), sol < TopUpOrder.solanaFeeFloor {
            return SolanaTransaction.unsigned(try usdcWithFees(order, units: units, from: from, blockhash: blockhash))
        }
        return SolanaTransaction.unsigned(try SPL.transferMessage(payer: from, to: order.recipient, mint: order.network.usdc, amount: units, decimals: 6, blockhash: blockhash))
    }

    nonisolated static let system = "11111111111111111111111111111111"
    nonisolated static func le<T: FixedWidthInteger>(_ n: T) -> Data { withUnsafeBytes(of: n.littleEndian) { Data($0) } }

    /// USDC into this wallet's account (made if missing) and `TopUpOrder.solanaFeeTopUp` SOL to the wallet itself, in one
    /// message `from` signs and pays for.
    nonisolated static func usdcWithFees(_ order: TopUpOrder, units: UInt64, from: String, blockhash: String) throws -> Data {
        let mint = order.network.usdc
        let mine = try SPL.associatedAccount(owner: from, mint: mint), theirs = try SPL.associatedAccount(owner: order.recipient, mint: mint)
        let create = SolanaInstruction(program: SPL.associatedTokenProgram, accounts: [
            .init(pubkey: from, isSigner: true, isWritable: true), .init(pubkey: theirs, isSigner: false, isWritable: true),
            .init(pubkey: order.recipient, isSigner: false, isWritable: false), .init(pubkey: mint, isSigner: false, isWritable: false),
            .init(pubkey: system, isSigner: false, isWritable: false), .init(pubkey: SPL.tokenProgram, isSigner: false, isWritable: false),
        ], data: Data([1]))
        let usdc = SolanaInstruction(program: SPL.tokenProgram, accounts: [
            .init(pubkey: mine, isSigner: false, isWritable: true), .init(pubkey: mint, isSigner: false, isWritable: false),
            .init(pubkey: theirs, isSigner: false, isWritable: true), .init(pubkey: from, isSigner: true, isWritable: false),
        ], data: Data([12]) + le(units) + Data([6]))
        let fees = SolanaInstruction(program: system, accounts: [
            .init(pubkey: from, isSigner: true, isWritable: true), .init(pubkey: order.recipient, isSigner: false, isWritable: true),
        ], data: le(UInt32(2)) + le(TopUpOrder.solanaFeeTopUp))
        return try SolanaTransaction.compileMessage([create, usdc, fees], payer: from, blockhash: blockhash)
    }

    /// The page: the order, the wallets this browser has (Wallet Standard for Solana, EIP-6963 for EVM), one button each.
    nonisolated static func page(_ order: TopUpOrder) -> String {
        let ends = AddressCheck.ends(order.recipient)
        let net = order.network
        // An EVM wallet is switched to the network, or told about it first (wallet_addEthereumChain) when it doesn't know it.
        let chain: [String: Any] = net.chainId.map { id in
            ["chainId": "0x" + String(id, radix: 16), "chainName": net.title, "rpcUrls": [net.rpc], "blockExplorerUrls": [net.explorer],
             "nativeCurrency": ["name": net.coin ?? "USD", "symbol": net.coin ?? "USD", "decimals": 18]]
        } ?? [:]
        let config: [String: Any] = [
            "solana": net.chainId == nil, "chain": chain, "recipient": order.recipient, "units": order.units ?? "0",
            "token": net.chainId != nil && order.coin == .usdc ? net.usdc : "",
            "explorer": order.explorer("").map(\.absoluteString) ?? "",
            "amount": order.amount.map { "\($0)" } ?? "", "symbol": order.symbol, "decimals": order.decimals,
            "digits": order.coin == .usdc ? 2 : 6,
        ]
        // `<` escaped in the inline JSON: nothing in it can close the script.
        let json = ((try? JSONSerialization.data(withJSONObject: config)).map { String(decoding: $0, as: UTF8.self) } ?? "{}")
            .replacingOccurrences(of: "<", with: "\\u003c")
        return pageHTML
            .replacingOccurrences(of: "{{amount}}", with: order.amount.map { "\($0)" } ?? "")
            .replacingOccurrences(of: "{{symbol}}", with: order.symbol)
            .replacingOccurrences(of: "{{network}}", with: net.title)
            .replacingOccurrences(of: "{{cost}}", with: net.cost)
            .replacingOccurrences(of: "{{start}}", with: ends.start).replacingOccurrences(of: "{{end}}", with: ends.end)
            .replacingOccurrences(of: "{{recipient}}", with: order.recipient)
            .replacingOccurrences(of: "{{wallets}}", with: net.chainId == nil ? "Phantom, Solflare or Backpack" : "MetaMask, Rabby, Phantom or Coinbase Wallet")
            .replacingOccurrences(of: "{{config}}", with: json)
    }

    nonisolated static let pageHTML = #"""
    <!doctype html>
    <html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
    <title>Top up ihatecrypto</title>
    <style>
    :root{--bg:#0d0e11;--card:#18191d;--line:#2a2b31;--text:#f4f4f5;--dim:#9a9ba3;--accent:#ff7a2f;--ok:#4ade80;--bad:#f87171}
    @media (prefers-color-scheme:light){:root{--bg:#f3f3f1;--card:#fff;--line:#e4e4e1;--text:#141417;--dim:#6b6c72;--ok:#16a34a;--bad:#dc2626}}
    *{box-sizing:border-box}body{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);font:15px/1.45 "Segoe UI Variable","Segoe UI",system-ui,sans-serif;padding:24px 16px}
    main{width:min(440px,100%);background:var(--card);border:1px solid var(--line);border-radius:26px;padding:26px}
    header{display:flex;align-items:center;gap:10px;font-weight:700;font-size:16px}header i{width:30px;height:30px;border-radius:50%;background:var(--accent);display:inline-block}
    .amount{font-size:38px;font-weight:800;letter-spacing:-.02em;margin:22px 0 2px}.dim{color:var(--dim)}
    .to{margin:16px 0;padding:12px 14px;border-radius:14px;background:var(--bg);font-size:13px}
    .to b{font:600 15px ui-monospace,Consolas,monospace}.to code{display:block;margin-top:4px;font-size:11px;color:var(--dim);word-break:break-all}
    h2{font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:var(--dim);margin:20px 0 8px;font-weight:600}
    button{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:12px;width:100%;padding:12px 14px;margin-bottom:8px;border:1.5px solid var(--line);border-radius:14px;cursor:pointer;font-weight:600}
    button:hover:not(:disabled){border-color:var(--accent)}button:disabled{opacity:.45;cursor:default}button img{width:28px;height:28px;border-radius:7px}
    button span{flex:1}button i{font-style:normal;color:var(--dim)}
    #status{min-height:22px;font-size:14px;margin-top:6px}#status.ok{color:var(--ok)}#status.bad{color:var(--bad)}#status a{color:inherit}
    footer{margin-top:18px;font-size:12px;color:var(--dim)}
    [hidden]{display:none!important}#more{margin-top:14px}.bal{margin-bottom:12px;padding:10px 14px;border-radius:14px;background:var(--bg);font-size:13px}
    .bal div{display:flex;justify-content:space-between;gap:12px;padding:2px 0}.bal b{font-weight:600;text-align:right}
    .field{display:flex;align-items:baseline;gap:8px;margin-bottom:10px;padding-bottom:4px;border-bottom:1.5px solid var(--line);font-size:30px;font-weight:800}
    .field:focus-within{border-color:var(--accent)}.field input{all:unset;flex:1;min-width:0}
    #warn{color:var(--bad);font-size:13px;min-height:18px}
    button.go{justify-content:center;background:var(--accent);border-color:var(--accent);color:#141417}
    #cancel{display:block;text-align:center;font-size:13px;color:var(--dim);cursor:pointer}
    </style></head><body><main>
    <header><i></i>Top up ihatecrypto</header>
    <div class="amount">{{amount}} {{symbol}}</div><div class="dim">on {{network}} · {{cost}}</div>
    <div class="to">To your ihatecrypto wallet <b>{{start}} … {{end}}</b><code>{{recipient}}</code></div>
    <h2>Top up from</h2><div id="wallets"></div>
    <div id="none" class="dim" hidden>No wallet found in this browser. Install {{wallets}}, or scan the QR in ihatecrypto with your phone.</div>
    <div id="status"></div>
    <section id="more" hidden>
      <div class="bal"><div><span>Your ihatecrypto wallet</span><b id="toBal">…</b></div><div><span id="fromName"></span><b id="fromBal">…</b></div></div>
      <button id="again"><span>Send more</span><i>+</i></button>
      <form id="form" hidden>
        <label class="field"><input id="amount" inputmode="decimal" autocomplete="off" aria-label="Amount"><span>{{symbol}}</span></label>
        <div id="warn"></div>
        <button class="go" id="go" type="submit"></button>
        <a id="cancel">Cancel</a>
      </form>
    </section>
    <footer>This page comes from ihatecrypto on this PC, not from a website: the address above can't be swapped. Your wallet shows the transfer before you approve it.</footer>
    </main><script>
    const C = {{config}};
    const list = document.getElementById('wallets'), statusLine = document.getElementById('status'), seen = new Set();
    const say = (html, kind) => { statusLine.innerHTML = html; statusLine.className = kind || ''; };
    const post = async (what, body) => {
      const r = await fetch(location.pathname + '/' + what, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      if (!r.ok) throw new Error(await r.text());
      return r.json();
    };
    const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
    const base58 = bytes => {
      let n = 0n, s = '';
      for (const b of bytes) n = n * 256n + BigInt(b);
      while (n > 0n) { s = B58[Number(n % 58n)] + s; n /= 58n; }
      for (const b of bytes) { if (b) break; s = '1' + s; }
      return s;
    };
    // «12.5» → the coin's smallest units; 0n for nothing, a typo or more digits than the coin has.
    const toUnits = s => {
      const m = /^\s*(\d*)(?:[.,](\d*))?\s*$/.exec(s);
      if (!m || !(m[1] || m[2]) || (m[2] || '').length > C.decimals) return 0n;
      return BigInt((m[1] || '0') + (m[2] || '').padEnd(C.decimals, '0'));
    };
    const coins = u => Number(u) / 10 ** C.decimals;
    const fmt = v => v == null || isNaN(v) ? '…' : v.toLocaleString('en-US', { maximumFractionDigits: C.digits }) + ' ' + C.symbol;
    const $ = id => document.getElementById(id);
    // The wallet that sent last ({ name, pay, from }), which «Send more» sends from again; balances as last read.
    let units = BigInt(C.units), last = null, bal = {};
    function render() {
      const n = $('form').hidden ? 0 : coins(toUnits($('amount').value));
      const line = (now, after) => fmt(now) + (n && now != null && after >= 0 ? ' → ' + fmt(after) : '');
      $('toBal').textContent = line(bal.to, bal.to + n);
      $('fromBal').textContent = line(bal.from, bal.from - n);
      const short = n > 0 && bal.from != null && n > bal.from;
      $('warn').textContent = short ? last.name + ' has only ' + fmt(bal.from) : '';
      $('go').textContent = n ? 'Confirm · send ' + fmt(n) : 'Type an amount';
      $('go').disabled = !n || short;
    }
    async function balances() {
      if (!last) return;
      const b = await post('balance', { from: last.from }).catch(() => null);
      // A side the network didn't answer keeps its last value.
      if (b) { bal = { to: b.to == null ? bal.to : Number(b.to), from: b.from == null ? bal.from : Number(b.from) }; render(); }
    }
    const escape = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
    async function send(name, pay) {
      for (const x of document.querySelectorAll('button')) x.disabled = true;
      try {
        say('Connecting ' + escape(name) + '…');
        const { id, from } = await pay(units, () => say('Approve the transfer in ' + escape(name) + '…'));
        // Sent is sent: a closed page (the card was closed meanwhile) only means ihatecrypto learns it from the balance.
        await post('sent', { id }).catch(() => {});
        last = { name, pay, from };
        say('Sent ' + fmt(coins(units)) + ' ✓ ihatecrypto shows it the moment it lands. You can close this tab. <a target="_blank" rel="noreferrer" href="' + escape(C.explorer + id) + '">See it ↗</a>', 'ok');
        $('fromName').textContent = name;
        $('more').hidden = false;
        $('form').hidden = true;
        $('again').hidden = $('again').disabled = false;
        render();
        balances();
      } catch (e) {
        const no = e && (e.code === 4001 || /reject|denied|cancel|declined/i.test(e.message || ''));
        say(no ? 'Cancelled in the wallet. Nothing was sent.' : 'It didn’t go through: ' + escape((e && e.message) || e), 'bad');
        for (const x of document.querySelectorAll('button')) x.disabled = false;
        render();
      }
    }
    // «Send more»: the amount picked here, its balances now → after, Confirm, then the wallet asks again.
    $('again').onclick = () => {
      $('again').hidden = true;
      $('form').hidden = false;
      $('amount').value = C.amount;
      $('amount').focus();
      $('amount').select();
      render();
      balances();
    };
    $('cancel').onclick = () => { $('form').hidden = true; $('again').hidden = false; render(); };
    $('amount').oninput = render;
    $('form').onsubmit = e => {
      e.preventDefault();
      units = toUnits($('amount').value);
      if (units > 0n && last) send(last.name, last.pay);
    };
    // What lands shows here too while the tab is open.
    setInterval(() => { if (!document.hidden) balances(); }, 5000);
    function wallet(name, icon, pay) {
      if (seen.has(name)) return;
      seen.add(name);
      $('none').hidden = true;
      const b = document.createElement('button');
      b.innerHTML = (icon ? '<img alt="">' : '') + '<span></span><i>→</i>';
      if (icon) b.querySelector('img').src = icon;
      b.querySelector('span').textContent = name;
      b.onclick = () => send(name, pay);
      list.append(b);
    }
    if (C.solana) {
      // Wallet Standard: wallets register when the page says it's ready, or announce themselves when they load later.
      const api = { register: (...ws) => { ws.forEach(solana); return () => {}; } };
      window.addEventListener('wallet-standard:register-wallet', e => e.detail(api));
      window.dispatchEvent(new CustomEvent('wallet-standard:app-ready', { detail: api }));
      function solana(w) {
        const f = w.features || {};
        if (!f['standard:connect'] || !f['solana:signAndSendTransaction'] || !(w.chains || []).includes('solana:mainnet')) return;
        wallet(w.name, w.icon, async (units, approve) => {
          const { accounts } = await f['standard:connect'].connect();
          const account = accounts[0] || w.accounts[0];
          const { transaction } = await post('solana', { from: account.address, units: units.toString() });
          approve();
          const bytes = Uint8Array.from(atob(transaction), c => c.charCodeAt(0));
          const [out] = await f['solana:signAndSendTransaction'].signAndSendTransaction({ account, chain: 'solana:mainnet', transaction: bytes });
          return { id: base58(out.signature), from: account.address };
        });
      }
    } else {
      // EIP-6963: every EVM wallet announces itself; window.ethereum is the fallback for an older one.
      window.addEventListener('eip6963:announceProvider', e => evm(e.detail.info.name, e.detail.info.icon, e.detail.provider));
      window.dispatchEvent(new Event('eip6963:requestProvider'));
      setTimeout(() => { if (!seen.size && window.ethereum) evm('Browser wallet', '', window.ethereum); }, 800);
      function evm(name, icon, p) {
        wallet(name, icon, async (units, approve) => {
          const [from] = await p.request({ method: 'eth_requestAccounts' });
          try { await p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: C.chain.chainId }] }); }
          catch (e) {
            if (e.code !== 4902) throw e;
            await p.request({ method: 'wallet_addEthereumChain', params: [C.chain] });
          }
          approve();
          const amount = units.toString(16);
          const tx = C.token
            ? { from, to: C.token, data: '0xa9059cbb' + C.recipient.slice(2).toLowerCase().padStart(64, '0') + amount.padStart(64, '0') }
            : { from, to: C.recipient, value: '0x' + amount };
          return { id: await p.request({ method: 'eth_sendTransaction', params: [tx] }), from };
        });
      }
    }
    setTimeout(() => { if (!seen.size) $('none').hidden = false; }, 1500);
    </script></body></html>
    """#
}

/// A request to the top-up page: its method, path, lowercased headers and body.
nonisolated struct PageRequest: Sendable {
    var method: String
    var path: String
    var headers: [String: String]
    var body: Data

    /// One HTTP/1.1 request from what was read; nil while it isn't whole yet (its head, then `Content-Length` bytes).
    static func parse(_ data: Data) -> PageRequest? {
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let lines = String(decoding: data[..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
        let first = lines.first?.split(separator: " ").map(String.init) ?? []
        guard first.count >= 2 else { return nil }
        var headers: [String: String] = [:]
        for line in lines.dropFirst() {
            guard let colon = line.firstIndex(of: ":") else { continue }
            headers[line[..<colon].lowercased()] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
        }
        let body = data[end.upperBound...]
        guard body.count >= Int(headers["content-length"] ?? "0") ?? 0 else { return nil }
        return PageRequest(method: first[0], path: String(first[1].split(separator: "?").first ?? ""), headers: headers, body: Data(body))
    }
}

/// The page's server: a socket bound to 127.0.0.1 itself, so nothing on the network can connect; each connection is one
/// request read whole (64 KB at most, 5 s at most), answered and closed.
nonisolated final class PageServer: @unchecked Sendable {
    #if os(Windows)
    typealias Handle = SOCKET
    static let invalid = INVALID_SOCKET
    static func close(_ s: Handle) { closesocket(s) }
    #else
    typealias Handle = Int32
    static let invalid: Int32 = -1
    static func close(_ s: Handle) {
        #if canImport(Darwin)
        _ = Darwin.close(s)
        #else
        _ = Glibc.close(s)
        #endif
    }
    #endif

    let port: UInt16
    private let listening: Handle

    private init(port: UInt16, listening: Handle) {
        self.port = port
        self.listening = listening
    }

    static func start(_ handle: @escaping @Sendable (PageRequest) async -> Data) throws -> PageServer {
        #if os(Windows)
        var wsa = WSADATA()
        _ = WSAStartup(0x0202, &wsa)
        let s = socket(AF_INET, SOCK_STREAM, Int32(IPPROTO_TCP.rawValue))
        #elseif canImport(Darwin)
        let s = socket(AF_INET, SOCK_STREAM, 0)
        #else
        let s = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #endif
        guard s != invalid else { throw WalletError("The page couldn't open a socket.") }
        var address = sockaddr_in()
        address.sin_family = numericCast(AF_INET)
        address.sin_port = 0
        #if os(Windows)
        address.sin_addr.S_un.S_addr = UInt32(0x7F00_0001).bigEndian
        #else
        address.sin_addr.s_addr = UInt32(0x7F00_0001).bigEndian
        #endif
        let size = MemoryLayout<sockaddr_in>.size
        let bound = withUnsafePointer(to: &address) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(s, $0, numericCast(size)) } }
        guard bound == 0, listen(s, 8) == 0 else {
            close(s)
            throw WalletError("The page couldn't start.")
        }
        var actual = sockaddr_in()
        #if os(Windows)
        var length = Int32(size)
        #else
        var length = socklen_t(size)
        #endif
        _ = withUnsafeMutablePointer(to: &actual) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(s, $0, &length) } }
        let server = PageServer(port: UInt16(bigEndian: actual.sin_port), listening: s)
        // ponytail: one connection at a time on one thread; a page sends a few small requests, so they never queue long.
        Thread.detachNewThread {
            while true {
                let c = accept(s, nil, nil)
                guard c != invalid else { return }
                server.serve(c, handle)
            }
        }
        return server
    }

    func stop() { Self.close(listening) }

    private func serve(_ c: Handle, _ handle: @escaping @Sendable (PageRequest) async -> Data) {
        defer { Self.close(c) }
        #if os(Windows)
        var timeout = DWORD(5000)
        _ = withUnsafePointer(to: &timeout) { $0.withMemoryRebound(to: CChar.self, capacity: 4) { setsockopt(c, SOL_SOCKET, SO_RCVTIMEO, $0, 4) } }
        #else
        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        _ = setsockopt(c, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        #endif
        var data = Data(), buffer = [UInt8](repeating: 0, count: 16_384)
        var request: PageRequest?
        while request == nil, data.count < 65_536 {
            #if os(Windows)
            let got = Int(buffer.withUnsafeMutableBytes { recv(c, $0.baseAddress?.assumingMemoryBound(to: CChar.self), Int32($0.count), 0) })
            #else
            let got = buffer.withUnsafeMutableBytes { recv(c, $0.baseAddress, $0.count, 0) }
            #endif
            guard got > 0 else { return }
            data.append(contentsOf: buffer[0..<got])
            request = PageRequest.parse(data)
        }
        guard let request else { return }
        let reply = Reply(), done = DispatchSemaphore(value: 0)
        Task { reply.data = await handle(request); done.signal() }
        done.wait()
        var sent = 0
        let bytes = [UInt8](reply.data)
        while sent < bytes.count {
            #if os(Windows)
            let n = Int(bytes[sent...].withUnsafeBytes { send(c, $0.baseAddress?.assumingMemoryBound(to: CChar.self), Int32($0.count), 0) })
            #else
            let n = bytes[sent...].withUnsafeBytes { send(c, $0.baseAddress, $0.count, 0) }
            #endif
            guard n > 0 else { return }
            sent += n
        }
    }

    private final class Reply: @unchecked Sendable { var data = Data() }

    static func response(_ status: Int, _ type: String, _ body: Data) -> Data {
        let head = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\nContent-Type: \(type)\r\nContent-Length: \(body.count)\r\n"
            + "Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n"
        return Data(head.utf8) + body
    }
}
