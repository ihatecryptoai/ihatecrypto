import EVMChain
import Foundation
import GuardJev
import IhatecryptoCore
import SignerAPI
import SolanaChain
import TradeExecution
import TxGuard
import WalletCore

/// A site's request waiting for the person (the Mac's SiteRequest): its own block on the canvas, apart from the person's
/// card, answered once by its id (Consent.swift).
final class SiteAsk {
    let view: SiteAskView
    private var continuation: CheckedContinuation<Bool, Never>?
    init(_ view: SiteAskView, _ c: CheckedContinuation<Bool, Never>) { self.view = view; continuation = c }
    func answer(_ yes: Bool) {
        continuation?.resume(returning: yes)
        continuation = nil
    }
}

nonisolated struct SiteError: LocalizedError {
    let text: String
    init(_ text: String) { self.text = text }
    var errorDescription: String? { text }
}

/// MetaMask's list of phishing sites (eth-phishing-detect), read once a day into phishing.json: a page on it gets no
/// wallet. Until the first download the list is empty; the lookalike check runs either way.
final class PhishingList {
    static let shared = PhishingList()
    static let source = URL(string: "https://raw.githubusercontent.com/MetaMask/eth-phishing-detect/main/src/config.json")!
    private var blocked: Set<String> = []
    private var allowed: Set<String> = []
    private var read: Date?

    func contains(_ host: String) async -> Bool {
        if read == nil || Date().timeIntervalSince(read!) > 86_400 { await refresh() }
        return SiteCheck.listed(host: host, blocked: blocked, allowed: allowed)
    }

    private func refresh() async {
        read = Date()
        let file = Store.url("phishing.json")
        let saved = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        var bytes = try? Data(contentsOf: file)
        if saved.map({ Date().timeIntervalSince($0) > 86_400 }) ?? true,
           let (data, response) = try? await URLSession.shared.data(from: Self.source), (response as? HTTPURLResponse)?.statusCode == 200 {
            bytes = data
            try? data.write(to: file, options: .atomic)
        }
        guard let bytes, let j = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any], let b = j["blacklist"] as? [String] else { return }
        blocked = Set(b)
        allowed = Set(j["whitelist"] as? [String] ?? [])
    }
}

/// The dApp browser's requests, as the Mac's Browser handles them: the window's browser (a webview with the provider
/// script) passes each call with the page's origin, which it reads itself, never from the page. Every request that
/// gives the site an address or a signature is a card on the canvas; nothing is signed before Confirm, and the
/// signer checks each transaction again.
extension Engine {
    static let readOnly: Set<String> = ["eth_call", "eth_blockNumber", "eth_getBalance", "eth_getCode", "eth_getTransactionCount", "eth_getTransactionReceipt",
                                        "eth_getTransactionByHash", "eth_estimateGas", "eth_gasPrice", "eth_feeHistory", "eth_getBlockByNumber",
                                        "eth_getLogs", "eth_maxPriorityFeePerGas", "eth_getStorageAt"]
    /// The EVM networks a site may use, as the Mac's browser (Ethereum, Arbitrum, Base): where the balance simulation is
    /// known to run, so a card never shows only «Unavailable».
    static let siteChains: Set<UInt64> = [1, 42161, 8453]
    /// arb1 crashes on eth_simulateV1; publicnode runs it.
    static let simulationRPC: [UInt64: String] = [42161: "https://arbitrum-one-rpc.publicnode.com"]

    /// One call from a page: the answer as JSON text, or an error «code|message» for EIP-1193.
    func dapp(_ method: String, params text: String, origin: String, title: String = "") async throws -> String {
        guard let url = URL(string: origin), url.scheme == "https", let host = url.host() else { throw SiteError("Only https sites get the wallet.") }
        let params = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
        // Disconnect only takes access away, so it works whatever wallet is on — a Ledger, the automation wallet, none
        // (the Mac's disconnectHere). The bar's takes the site's wallet on every chain; a page's own Solana disconnect
        // leaves its EVM connection alone.
        if method == "disconnect" {
            sites.solana.remove(origin)
            if params["everywhere"] as? Bool == true {
                sites.evm.remove(origin)
                sites.chain[origin] = nil
            }
            return "null"
        }
        let eth = method == "eth"
        guard let wallet = activeWallet, wallet.automation != true else { throw SiteError((eth ? "4100|" : "") + say("Add a wallet first.", "Сначала добавь кошелёк.")) }
        guard !ledgerActive else {
            throw SiteError((eth ? "4100|" : "") + say("The browser signs with ihatecrypto's own wallets for now: switch from the Ledger to use a site.",
                                                       "Браузер пока подписывает только кошельками ihatecrypto: переключись с Ledger, чтобы пользоваться сайтом."))
        }
        guard !(await PhishingList.shared.contains(host)) else {
            throw SiteError((eth ? "4100|" : "") + "\(host) is on MetaMask's list of phishing sites: ihatecrypto gives it no wallet.")
        }
        let site = SiteCheck.check(host: host)
        let signs = ["signMessage", "signTransaction", "signAndSendTransaction"].contains(method)
            || (eth && ["personal_sign", "eth_signTypedData_v4", "eth_sendTransaction"].contains(params["method"] as? String ?? ""))
        // A site's signature is a move of its own: its Windows Hello doesn't carry over to the next request.
        defer { if signs { Task { _ = try? await SignerLink.shared.send(.endSession) } } }
        func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed), as: UTF8.self) }
        switch method {
        case "eth":
            return try await ethereum(params["method"] as? String ?? "", params["params"] as? String ?? "[]", origin: origin, host: host, title: title, site: site)
        case "connect":
            guard let owner = wallet.solana else { throw SiteError(wallet.missing("solana") ?? "No Solana address.") }
            if !sites.solana.contains(origin) {
                if params["silent"] as? Bool == true { return "null" }
                guard await ask(host, origin: origin, site: site, connect: true, title: say("\(host) asks to connect to your Solana wallet", "\(host) просит подключить твой кошелёк Solana"),
                                lines: [say("It sees your address and can ask you to sign; nothing is signed without your Confirm.", "Сайт увидит адрес и сможет просить подпись; без твоего «Подтвердить» ничего не подписывается."), owner])
                else { throw SiteError("The user declined.") }
                sites.solana.insert(origin)
            }
            return try json(["address": owner, "publicKey": try Base58.decode(owner).base64EncodedString()])
        case "signMessage":
            guard sites.solana.contains(origin), let b64 = params["message"] as? String, let bytes = Data(base64Encoded: b64) else { throw SiteError("Connect the site first.") }
            let text = String(data: bytes, encoding: .utf8) ?? "(not text: \(bytes.count) bytes)"
            guard await ask(host, origin: origin, site: site, title: say("\(host) asks to sign a message", "\(host) просит подписать сообщение"),
                            lines: [text, say("Signing text doesn't move money. The signer refuses it if it's a hidden transaction.", "Подпись текста не двигает деньги. Подписант откажет, если это скрытая транзакция.")])
            else { throw SiteError("The user declined.") }
            let reply = try await SignerLink.shared.send(.signSolanaMessage(message: bytes))
            guard reply.ok, let signature = reply.signatureBytes else { throw SiteError(LinkTradeSigner.reason(reply)) }
            return try json(["signature": signature.base64EncodedString()])
        case "signTransaction", "signAndSendTransaction":
            guard let owner = wallet.solana, sites.solana.contains(origin), let b64 = params["transaction"] as? String, let wire = Data(base64Encoded: b64) else {
                throw SiteError("Connect the site first.")
            }
            let send = method == "signAndSendTransaction"
            let review = try await solanaReview(wire, owner: owner, host: host, title: title)
            guard review.problems.isEmpty else {
                pin("result", say("\(host): transaction refused", "\(host): транзакция отклонена"),
                    lines: review.problems + [review.lead].compactMap { $0 } + review.changes.map { say("Your balance, simulated: ", "Твой баланс, по симуляции: ") + $0 } + review.lines)
                reply(say("\(host) sent a transaction ihatecrypto won't sign: ", "\(host) прислал транзакцию, которую ihatecrypto не подпишет: ") + review.problems.joined(separator: " "), note: "browser")
                throw SiteError("Transaction declined: " + review.problems.joined(separator: " "))
            }
            guard await ask(host, origin: origin, site: site, title: (send ? say("\(host) asks to sign and send a transaction", "\(host) просит подписать и отправить транзакцию")
                                                         : say("\(host) asks to sign a transaction", "\(host) просит подписать транзакцию")),
                            lines: review.lines, warnings: review.warnings, ticks: review.ticks, review: review,
                            note: send ? say("After signing, ihatecrypto sends the transaction to the network.", "После подписи ihatecrypto сам отправит транзакцию в сеть.")
                                       : say("The site gets the signed transaction and sends it itself.", "Сайт получит подписанную транзакцию и отправит её сам."))
            else { throw SiteError("Transaction declined.") }
            let reply = try await SignerLink.shared.send(.signSolana(wire: wire, accounts: review.accounts, recipients: [], tokenOwners: review.tokenOwners,
                                                                     extraPrograms: review.unknownPrograms))
            guard reply.ok, let signed = reply.signedWire else { throw SiteError(LinkTradeSigner.reason(reply)) }
            // The signer must return the page's transaction with only our signature added.
            let prefix = 1 + 64
            guard signed.count == wire.count, signed.subdata(in: prefix..<signed.count) == wire.subdata(in: prefix..<wire.count) else {
                throw SiteError("What was signed isn't what was checked.")
            }
            let signature = signed.subdata(in: 1..<65)
            self.reply(say("\(host): transaction signed\(send ? " and sent" : "") — \(Base58.encode(signature).prefix(12))…", "\(host): транзакция подписана\(send ? " и отправлена" : "") — \(Base58.encode(signature).prefix(12))…"), note: "browser")
            Task { try? await Task.sleep(for: .seconds(10)); await refresh() }
            if send {
                _ = try await SolanaRPC().send(signed)
                return try json(["signature": signature.base64EncodedString()])
            }
            return try json(["signed": signed.base64EncodedString()])
        default:
            throw SiteError("ihatecrypto doesn't support \(method).")
        }
    }

    /// A request waits on the canvas for the person (the Mac's ask): one at a time, so a page can't swap the one being
    /// read for another, and never in the place of their own card — it has its block and its answer (Consent.swift),
    /// with a tick for each risk it carries before Approve counts.
    func ask(_ host: String, origin: String, site: SiteCheck.Verdict, connect: Bool = false, title: String, lines: [String],
                     warnings: [String] = [], ticks: [SiteTick] = [], review: SiteReview? = nil, note: String? = nil) async -> Bool {
        guard siteAsk == nil else { return false }
        var warnings = warnings, ticks = ticks
        var check = "known", real: String?
        switch site {
        case .lookalike(let r): check = "lookalike"; real = r; ticks.insert(Self.lookalikeTick(r, say: say), at: 0)
        case .unknown: check = "unknown"; warnings.append(say("ihatecrypto doesn't know \(host): check the address bar.", "ihatecrypto не знает \(host): проверь адрес в строке."))
        case .known: break
        case .local: check = "local"
        }
        let view = SiteAskView(id: UUID().uuidString, origin: origin, host: host, check: check, real: real, title: title, lines: lines, warnings: warnings,
                               ticks: ticks, approve: connect ? say("Connect", "Подключить") : say("Sign", "Подписать"), canvas: canvasID,
                               lead: review?.lead, balances: review?.balances ?? [], checks: review?.checks ?? [], note: note)
        // Its block says the same in words: the lead, each balance and the checks, beside the lines.
        let said = [view.lead].compactMap { $0 } + view.balances.map { "\($0.symbol) \($0.amount)" + ($0.usd.map { " ≈ \($0)" } ?? "") + ($0.note.map { " · \($0)" } ?? "") }
            + lines + view.checks + [note].compactMap { $0 }
        out.event("showMain", ["why": "site"])
        // Only while the window isn't in front: in front, the request's own card is the ask (the Mac shows its sheet alone).
        notify(.confirm, host, say("Asks for your wallet: look at ihatecrypto's window.", "Просит кошелёк: посмотри в окно ihatecrypto."), whenAway: true)
        // The window's call gives up after 900 s (main.rs dapp_call): declined before that, never signed for a page that left.
        Task { await expireSite(view.id) }
        return await withCheckedContinuation { c in
            siteAsk = SiteAsk(view, c)
            pin(Board(id: view.id, kind: "move", title: title, lines: (warnings + ticks.map(\.title)).map { "⚠ " + $0 },
                      steps: [Board.Step(label: say("SITE · browser", "САЙТ · браузер"), title: title, lines: said)], state: "waiting"))
            changed()
        }
    }

    /// EIP-1193 requests. Answers are JSON text; errors are «code|message».
    private func ethereum(_ method: String, _ paramsJSON: String, origin: String, host: String, title: String, site: SiteCheck.Verdict) async throws -> String {
        guard let address = activeWallet?.evm else { throw SiteError("4100|" + (activeWallet?.missing("evm") ?? "No EVM address.")) }
        let args = (try? JSONSerialization.jsonObject(with: Data(paramsJSON.utf8))) as? [Any] ?? []
        let chain = sites.chain[origin] ?? 42161
        func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed), as: UTF8.self) }
        func hex(_ s: EVMSignature) -> String { "0x" + s.r.hex + s.s.hex + String(format: "%02x", s.v) }
        func rpc() throws -> EVMRPC {
            guard Self.siteChains.contains(chain), let n = EVMNetwork.all.first(where: { $0.rpc.chainId == chain }) else { throw SiteError("4901|No RPC for chain \(chain).") }
            return n.rpc
        }
        switch method {
        case "eth_chainId": return try json("0x" + String(chain, radix: 16))
        case "net_version": return try json("\(chain)")
        case "eth_accounts": return try json(sites.evm.contains(origin) ? [address] : [])
        case "eth_requestAccounts":
            if !sites.evm.contains(origin) {
                guard await ask(host, origin: origin, site: site, connect: true, title: say("\(host) asks to connect to your EVM wallet", "\(host) просит подключить твой EVM-кошелёк"),
                                lines: [say("It sees your EVM address, which is also your Hyperliquid address, and can ask you to sign; nothing is signed without your Confirm.",
                                            "Сайт увидит EVM-адрес — он же твой адрес на Hyperliquid — и сможет просить подпись; без твоего «Подтвердить» ничего не подписывается."),
                                        say("ihatecrypto never signs token spending permissions (Permit).", "ihatecrypto никогда не подписывает разрешения на трату токенов (Permit)."), address])
                else { throw SiteError("4001|The user declined.") }
                sites.evm.insert(origin)
            }
            return try json([address])
        case "wallet_switchEthereumChain":
            guard let id = (args.first as? [String: Any])?["chainId"] as? String, let n = UInt64(id.dropFirst(2), radix: 16),
                  Self.siteChains.contains(n), EVMNetwork.all.contains(where: { $0.rpc.chainId == n }) else {
                throw SiteError("4902|ihatecrypto works with Ethereum, Arbitrum and Base.")
            }
            sites.chain[origin] = n
            return "null"
        case "personal_sign":
            guard sites.evm.contains(origin), let raw = args.first as? String else { throw SiteError("4100|Connect the site first.") }
            let message = raw.hasPrefix("0x") ? (Data(hex: raw) ?? Data(raw.utf8)) : Data(raw.utf8)
            guard !PersonalMessage.isBlindHash(message) else { throw SiteError("4001|The site asks to sign a hash, not text: ihatecrypto doesn't sign that.") }
            guard await ask(host, origin: origin, site: site, title: say("\(host) asks to sign a message", "\(host) просит подписать сообщение"),
                            lines: [String(data: message, encoding: .utf8) ?? "\(message.count) bytes",
                                    say("Signing text doesn't move money. The signer refuses it if it's a hidden transaction.", "Подпись текста не двигает деньги. Подписант откажет, если это скрытая транзакция.")])
            else { throw SiteError("4001|The user declined.") }
            let reply = try await SignerLink.shared.send(.signPersonal(message: message))
            guard reply.ok, let s = reply.signature else { throw SiteError("4001|" + LinkTradeSigner.reason(reply)) }
            return try json(hex(s))
        case "eth_signTypedData_v4":
            guard sites.evm.contains(origin), args.count >= 2 else { throw SiteError("4100|Connect the site first.") }
            let body: Data = if let s = args[1] as? String { Data(s.utf8) } else { try JSONSerialization.data(withJSONObject: args[1]) }
            let typed: TypedData
            do { typed = try TypedData.parse(body) } catch { throw SiteError("4200|\(error.localizedDescription)") }
            if let refusal = typed.refusal(owner: evmAddress) {
                reply("Site \(host) asked for a “\(typed.primaryType)” signature. Declined — \(refusal)", note: "browser")
                throw SiteError("4001|" + refusal)
            }
            guard await ask(host, origin: origin, site: site, title: say("\(host) asks to sign “\(typed.primaryType)”", "\(host) просит подписать «\(typed.primaryType)»"),
                            lines: ["\(typed.primaryType) for “\(typed.domainName)”, chain \(typed.chainId)"] + typed.fields.map { "\($0.name): \($0.value)" }
                                + [say("The signer computes what it signs from this same data, and refuses token permissions.", "Подписант считает подпись из этих же данных и отказывает в разрешениях на токены.")])
            else { throw SiteError("4001|The user declined.") }
            let reply = try await SignerLink.shared.send(.signTypedData(json: body))
            guard reply.ok, let s = reply.signature else { throw SiteError("4001|" + LinkTradeSigner.reason(reply)) }
            return try json(hex(s))
        case "eth_sign":
            throw SiteError("4200|eth_sign signs a hash blindly — ihatecrypto doesn't support it.")
        case "eth_signTransaction":
            throw SiteError("4200|ihatecrypto signs EVM transactions only when it also sends them (eth_sendTransaction).")
        case "eth_sendTransaction":
            guard sites.evm.contains(origin), let p = args.first as? [String: Any] else { throw SiteError("4100|Connect the site first.") }
            guard let to = p["to"] as? String, Data(hex: to)?.count == 20 else { throw SiteError("4200|ihatecrypto doesn't sign contract creation.") }
            guard (p["from"] as? String)?.lowercased() ?? address.lowercased() == address.lowercased() else { throw SiteError("4100|The sender isn't your address.") }
            let data = Data(hex: (p["data"] ?? p["input"]) as? String ?? "0x") ?? Data()
            guard let value = Self.quantity(p["value"]) else { throw SiteError("4200|The amount is larger than ihatecrypto will sign.") }
            let rpc = try rpc()
            let review = await evmReview(to: to, value: value, data: data, from: address, rpc: rpc, host: host, title: title)
            guard review.problems.isEmpty else {
                pin("result", say("\(host): EVM transaction refused", "\(host): EVM-транзакция отклонена"), lines: review.problems + review.lines)
                reply("Site \(host): EVM transaction declined — \(review.problems.joined(separator: " "))", note: "browser")
                throw SiteError("4001|" + (review.problems.first ?? "Declined."))
            }
            guard await ask(host, origin: origin, site: site, title: say("\(host) asks to send a transaction on \(EVMNetwork.all.first { $0.rpc.chainId == chain }?.name ?? "chain \(chain)")",
                                                         "\(host) просит отправить транзакцию в \(EVMNetwork.all.first { $0.rpc.chainId == chain }?.name ?? "сети \(chain)")"),
                            lines: review.lines + review.changes.map { say("Balance: ", "Баланс: ") + $0 }, warnings: review.warnings, ticks: review.ticks,
                            note: say("Simulated on the latest block, before the network fee. A token names itself — check its address. The signer decodes the call again.",
                                      "Симуляция на последнем блоке, без комиссии сети. Токен называет себя сам — проверь его адрес. Подписант разберёт вызов ещё раз."))
            else { throw SiteError("4001|The user declined.") }
            let nonce = try await rpc.nonce(address)
            let fees = try await rpc.fees()
            let reply = try await SignerLink.shared.send(.signEVMTransaction(chainId: chain, nonce: nonce, maxPriorityFee: fees.maxPriority, maxFee: fees.maxFee,
                                                                            gasLimit: review.gas, to: to, value: value, data: data, recipients: review.recipients))
            guard reply.ok, let raw = reply.signedRaw else { throw SiteError("4001|" + LinkTradeSigner.reason(reply)) }
            do {
                let hash = try await rpc.send(raw)
                self.reply("Site \(host): EVM transaction sent on chain \(chain) — \(hash.prefix(14))…", note: "browser")
                return try json(hash)
            } catch {
                throw SiteError("-32000|The network rejected the transaction: \(error.localizedDescription)")
            }
        case let m where Self.readOnly.contains(m):
            // The JSON-RPC body is made here and only bytes cross to the network.
            var request = URLRequest(url: try rpc().url, timeoutInterval: 20)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "method": m, "params": args])
            let (data, _) = try await URLSession.shared.data(for: request)
            guard let j = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw SiteError("4900|The RPC didn't respond.") }
            if let e = j["error"] as? [String: Any] { throw SiteError("-32000|\(e["message"] ?? "RPC error")") }
            return try json(j["result"] ?? NSNull())
        default:
            throw SiteError("4200|ihatecrypto doesn't support \(method).")
        }
    }

    /// A JSON-RPC quantity («0x…») as UInt64; nil when it does not fit.
    static func quantity(_ v: Any?) -> UInt64? {
        guard let s = v as? String, s.hasPrefix("0x") else { return v == nil ? 0 : (v as? NSNumber)?.uint64Value }
        return s == "0x" ? 0 : UInt64(s.dropFirst(2), radix: 16)
    }

    struct SiteReview {
        var lines: [String]
        var changes: [String]
        var warnings: [String]
        var problems: [String]
        var recipients: [String] = []
        var gas: UInt64 = 0
        var accounts: [String] = []
        var tokenOwners: [String: String] = [:]
        var unknownPrograms: [String] = []
        /// What needs the person's tick before Approve counts (Consent.swift).
        var ticks: [SiteTick] = []
        /// The card's lead, balances and folded checks (a Solana transaction, the Mac's sheet).
        var lead: String?
        var balances: [SiteBalance] = []
        var checks: [String] = []
    }

    /// Decodes the call, names the token, simulates our balances and estimates gas, as the Mac does. Problems mean «don't sign».
    func evmReview(to: String, value: UInt64, data: Data, from: String, rpc: EVMRPC, host: String = "", title: String = "") async -> SiteReview {
        let call = EVMCall.decode(data)
        var problems = [call.refusal(sender: from, chainId: rpc.chainId), EVMCall.targetRefusal(to: to, data: data, sender: from)].compactMap { $0 }
        var payees = call.payees(to: to, value: value, chainId: rpc.chainId)
        if let target = call.unreadTarget(to: to, value: value, chainId: rpc.chainId) { payees.append(target) }
        let recipients = Array(Set(payees.map { $0.lowercased() }.filter { $0 != from.lowercased() })).sorted()
        let native = EVMNetwork.all.first { $0.rpc.chainId == rpc.chainId }?.gasCoin ?? "coins"
        func token(_ address: String) async -> (symbol: String, decimals: Int) {
            let symbol = (try? await rpc.read(address, Data([0x95, 0xd8, 0x9b, 0x41]))).flatMap(ABI.string) ?? "token \(address.prefix(8))…"
            let decimals = (try? await rpc.read(address, Data([0x31, 0x3c, 0xe5, 0x67]))).flatMap { ABI.uint(in: $0, word: 0) } ?? 18
            return (symbol, Int(decimals))
        }
        var lines = [await callLine(call, to: to, value: value, from: from, chainId: rpc.chainId, native: native, token: token)]
        if !recipients.isEmpty {
            lines.append(say("Money or the right to spend it goes to: ", "Деньги или право их тратить уходят: ") + recipients.joined(separator: ", "))
        }
        var changes: [String] = []
        let simulator = Self.simulationRPC[rpc.chainId].flatMap(URL.init(string:)).map { EVMRPC(url: $0, chainId: rpc.chainId) } ?? rpc
        func signed(_ d: Decimal) -> String { (d > 0 ? "+" : d < 0 ? "−" : "") + Fmt.num(abs(d), maxFraction: 9) }
        do {
            let sim = try await simulator.simulate(from: from, to: to, value: value, data: data)
            if let revert = sim.revert { problems.append("Simulation: the call would revert — \(revert)") }
            for (asset, delta) in sim.changes.sorted(by: { $0.key < $1.key }) {
                if asset == EVMSimulation.native { changes.insert("\(native): \(signed(delta / 1_000_000_000_000_000_000))", at: 0); continue }
                let t = await token(asset)
                let name = "\(t.symbol)\(sim.nfts.contains(asset) ? " NFT" : "") (\(asset.prefix(6))…\(asset.suffix(4)))"
                changes.append("\(name): \(signed(sim.nfts.contains(asset) ? delta : delta / Decimal(sign: .plus, exponent: t.decimals, significand: 1)))")
            }
            if sim.revert == nil, changes.isEmpty { changes.append(say("None of your balances change.", "Твои балансы не меняются.")) }
        } catch {
            changes.append("Unavailable: \(error.localizedDescription)")
            let tx: [String: Any] = ["from": from, "to": to, "value": "0x" + String(value, radix: 16), "data": "0x" + data.hex]
            do { _ = try await rpc.call("eth_call", [tx, "latest"]) } catch { problems.append("Simulation: the call would revert — \(error.localizedDescription)") }
        }
        var gas: UInt64 = 0
        if problems.isEmpty {
            do {
                gas = try await rpc.estimateGas(from: from, to: to, value: value, data: data) * 12 / 10
                if let fees = try? await rpc.fees() {
                    lines.append(say("Network fee up to ", "Комиссия сети до ") + "\(Fmt.num(Decimal(gas) * Decimal(fees.maxFee) / 1_000_000_000_000_000_000, maxFraction: 8)) \(native)")
                }
            } catch { problems.append("Couldn't estimate gas: \(error.localizedDescription)") }
        }
        var warnings: [String] = []
        var unread = false
        if case .contract = call { unread = true }
        var ticks = [Self.payeeTick(to: to, recipients: recipients, unread: unread, say: say)].compactMap { $0 }
        await siteSecondCheck(host: host, title: title, programs: ["EVM \(rpc.chainId): \(to)"], changes: lines + changes, lines: &lines, ticks: &ticks)
        return SiteReview(lines: lines, changes: changes, warnings: warnings, problems: problems, recipients: recipients, gas: gas, ticks: ticks)
    }

    /// What the call does, each token by its symbol and decimals (the Mac's evmReview lines), not raw units.
    func callLine(_ call: EVMCall, to: String, value: UInt64, from: String, chainId: UInt64, native: String,
                          token: (String) async -> (symbol: String, decimals: Int)) async -> String {
        func amount(_ word: Data, _ decimals: Int) -> String {
            if word.allSatisfy({ $0 == 0xff }) { return say("unlimited", "без лимита") }
            let n = word.reduce(Decimal(0)) { $0 * 256 + Decimal($1) }
            return Fmt.num(n / Decimal(sign: .plus, exponent: decimals, significand: 1), maxFraction: 6)
        }
        func coins(_ wei: UInt64) -> String { Fmt.num(Decimal(wei) / 1_000_000_000_000_000_000, maxFraction: 8) }
        func whose(_ a: String) -> String { a.lowercased() == from.lowercased() ? "this wallet" : a }
        switch call {
        case .plain:
            return "Transfer \(coins(value)) \(native) to \(to)."
        case .approve(let spender, let a), .increaseAllowance(let spender, let a):
            let t = await token(to)
            return "Allow \(spender) to spend up to \(amount(a, t.decimals)) \(t.symbol)."
        case .transfer(let r, let a):
            let t = await token(to)
            return "Transfer \(amount(a, t.decimals)) \(t.symbol) to \(r)."
        case .nftTransfer(let f, let r):
            return "Move NFTs of \(to) from \(whose(f)) to \(r)."
        case .delegation(let d, let on):
            return on ? "Let \(d) borrow against or manage your lending positions." : "Revoke \(d)'s rights over your lending positions."
        case .transferFrom(let f, let r, let a):
            let t = await token(to)
            return "Move \(amount(a, t.decimals)) \(t.symbol) from \(f) to \(r)."
        case .setApprovalForAll(let op, let on):
            return "\(on ? "Rights to all NFTs" : "Revoke NFT rights") for \(op)."
        case .permit:
            return "Call to permit (permission to spend)."
        case .swap(let r):
            return "Uniswap swap paying \(whose(r))."
        case .borrow(let asset, let a, let owner):
            let t = await token(asset)
            return "Borrow \(amount(a, t.decimals)) \(t.symbol) from \(to), the debt on \(whose(owner))."
        case .repay(let asset, let a, let owner):
            let t = await token(asset)
            return "Repay \(a.allSatisfy { $0 == 0xff } ? "all the" : amount(a, t.decimals)) \(t.symbol) debt of \(whose(owner)) to \(to)."
        case .multisigExec:
            return "Carry out a signed move of the multisig \(to): " + call.summary(chainId: chainId, to: to, value: value) + "."
        case .cctpBurn(let r, let domain, let a):
            return "Circle's CCTP: burn \(amount(a, 6)) USDC here to mint on \(CCTP.chainName(domain)) to \(r.map(whose) ?? "a non-EVM account")."
        case .oftSend(let r, let eid, let a, let plain, _):
            let known = USDT0.on(chainId)?.oft == to.lowercased()
            return "LayerZero: send \(known ? "\(amount(a, 6)) USDT" : "tokens of \(to)") to \(r.map(whose) ?? "an account that can't be read") on \(USDT0.networkName(eid: eid))\(plain ? "" : ", with options or a message of its own")."
        case .v4Swap(let s):
            let (a, b) = (await token(s.currencyIn), await token(s.currencyOut))
            func n(_ x: Decimal, _ d: Int) -> String { Fmt.num(x / Decimal(sign: .plus, exponent: d, significand: 1), maxFraction: 6) }
            return "Uniswap v4 swap: \(n(s.amountIn, a.decimals)) \(a.symbol) for at least \(n(s.amountOutMinimum, b.decimals)) \(b.symbol), paid to this wallet."
        case .permit2Approve(let contract, let spender, let a, let expiration):
            let t = await token(contract)
            return "Allow \(spender) to spend up to \(amount(a, t.decimals)) \(t.symbol) through Permit2, until \(Fmt.date(Date(timeIntervalSince1970: TimeInterval(expiration))))."
        case .vault(let r, let owner):
            return "Vault \(owner == nil ? "deposit" : "withdrawal") crediting \(whose(r))."
        case .lending(let b):
            return "Lending pool call (Aave supply or withdraw) crediting \(whose(b))."
        case .claim(let users):
            return "Merkl rewards claim paying \(users.allSatisfy { $0.lowercased() == from.lowercased() } ? "this wallet" : users.joined(separator: ", "))."
        case .wrap:
            return "Wrap \(coins(value)) \(native) into its token at \(to)\(EVMCall.wrap.payees(to: to, value: value, chainId: chainId).isEmpty ? ", the network's own WETH" : " — not this network's WETH")."
        case .unwrap(let a):
            return "Unwrap \(amount(a, 18)) of the token at \(to) into \(native)."
        case .contract(let selector):
            return "Contract call \(to), function \(selector)\(value > 0 ? ", with \(coins(value)) \(native)" : "")."
        }
    }

    /// Programs, the guard's verdict and the simulated change of every balance of ours the transaction touches.
    func solanaReview(_ wire: Data, owner: String, host: String, title: String = "") async throws -> SiteReview {
        let rpc = SolanaRPC()
        let message = try SolanaMessage.parse(transaction: wire)
        let accounts = try await rpc.resolveAccounts(message)
        let programs = Set(message.instructions.compactMap { $0.programIndex < accounts.count ? accounts[$0.programIndex] : nil })
        let unknown = programs.filter { SolanaGuard.knownPrograms[$0] == nil }.sorted()
        var allowed = SolanaGuard.knownPrograms
        for p in unknown { allowed[p] = "unknown program \(p.prefix(6))…" }
        let tokens = try await tokenAccounts(owner, rpc)
        let touched = tokens.filter { accounts.contains($0.key) }
        let owners = touched.mapValues { _ in owner }
        let verdict = SolanaGuard.check(message, accounts: accounts, policy: SolanaPolicy(owner: owner, programs: allowed, trustLookupTables: false), tokenOwners: owners)
        let solBefore = try await rpc.balance(owner)
        let simulation = try await rpc.simulate(wire, watch: [owner] + touched.keys, sigVerify: false)
        var problems = verdict.problems
        var deltas: [(symbol: String, mint: String, amount: Decimal)] = []
        func signed(_ d: Decimal) -> String { (d > 0 ? "+" : d < 0 ? "−" : "") + Fmt.num(abs(d), maxFraction: 9) }
        if let error = simulation.error {
            problems.append("Simulation failed: \(error).")
        } else {
            let solAfter = simulation.after[owner]?.lamports ?? solBefore
            deltas.append(("SOL", SpotAsset.all[0].mint, Decimal(Int64(solAfter) - Int64(solBefore)) / 1_000_000_000))
            for (address, t) in touched.sorted(by: { $0.key < $1.key }) {
                let after = simulation.after[address]?.token ?? t.amount
                let delta = Decimal(Int64(after) - Int64(t.amount)) / Decimal(sign: .plus, exponent: t.decimals, significand: 1)
                if delta != 0 {
                    deltas.append((t.mint == SpotAsset.usdc.mint ? "USDC" : SpotAsset.all.first { $0.mint == t.mint }?.symbol ?? "token \(t.mint.prefix(6))…", t.mint, delta))
                }
            }
        }
        let changes = deltas.map { "\($0.symbol): \(signed($0.amount))" }
        let programNames = programs.sorted().map { (name: SolanaGuard.knownPrograms[$0] ?? "\($0.prefix(4))…\($0.suffix(4))", known: SolanaGuard.knownPrograms[$0] != nil) }
        let named = programNames.map { $0.known ? $0.name : $0.name + say(" (not on ihatecrypto's list)", " (нет в списке ihatecrypto)") }
        var seen = Set<String>()
        var lines = [Self.callsLine(programNames, say: say)]
        let checks = verdict.checks.filter { !$0.hasPrefix("Calls unknown program") && seen.insert($0).inserted }
        var ticks = [Self.programsTick(unknown, say: say)].compactMap { $0 }
        await siteSecondCheck(host: host, title: title, programs: named, changes: changes, lines: &lines, ticks: &ticks)
        var review = SiteReview(lines: lines, changes: changes, warnings: [], problems: problems, accounts: accounts, tokenOwners: owners, unknownPrograms: unknown, ticks: ticks)
        review.lead = Self.solanaLead(deltas, programs: programNames.map(\.name), say: say)
        review.balances = deltas.map { d in
            let fee = Self.feeLike(d.symbol, d.amount, count: deltas.count)
            let price: Decimal? = Stablecoins.isDollar(d.symbol) ? 1 : d.symbol == "SOL" ? markets["SOL"]?.markPx : heldSolanaPrice(d.mint)
            return SiteBalance(symbol: d.symbol, amount: signed(d.amount), up: d.amount > 0, usd: price.map { Fmt.usd(abs(d.amount) * $0) },
                               note: fee ? say("network fee and rent for new accounts", "комиссия сети и залог за новые счета") : nil)
        }
        review.checks = checks
        return review
    }

    /// SOL that only pays the fee and rent beside a real move (the Mac's feeLike): said so, not read as a send.
    nonisolated static func feeLike(_ symbol: String, _ amount: Decimal, count: Int) -> Bool {
        symbol == "SOL" && amount < 0 && -amount < Decimal(string: "0.05")! && count > 1
    }

    /// «You send 5 USDC to Kamino Vault», «You send 5 USDC and get 0.02 SOL», «You get …», «Only the network fee» (the
    /// Mac's summary): what leaves and arrives, fee-sized SOL left out, and the protocol it goes to.
    nonisolated static func solanaLead(_ deltas: [(symbol: String, mint: String, amount: Decimal)], programs: [String], say: (String, String) -> String) -> String? {
        let moved = deltas.filter { !feeLike($0.symbol, $0.amount, count: deltas.count) }
        let out = moved.filter { $0.amount < 0 }.map { "\(Fmt.num(-$0.amount, maxFraction: 6)) \($0.symbol)" }
        let into = moved.filter { $0.amount > 0 }.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol)" }
        // Programs of one protocol («Kamino Vault», «Kamino Farms») are that protocol: the money goes to Kamino.
        let work = programs.filter { !solanaPlumbing.contains($0) && !$0.contains("…") }
        let brands = Set(work.map { String($0.split(separator: " ").first ?? Substring($0)) })
        let target = work.count > 1 && brands.count == 1 ? brands.first : work.first
        func list(_ items: [String]) -> String { items.count < 2 ? items.joined() : items.dropLast().joined(separator: ", ") + say(" and ", " и ") + items.last! }
        switch (out.isEmpty, into.isEmpty) {
        case (false, true): return say("You send \(list(out))", "Отправляешь \(list(out))") + (target.map { say(" to \($0)", " в \($0)") } ?? "")
        case (false, false): return say("You send \(list(out)) and get \(list(into))", "Отдаёшь \(list(out)), получаешь \(list(into))")
        case (true, false): return say("You get \(list(into))", "Получаешь \(list(into))")
        case (true, true): return deltas.isEmpty ? nil : say("Only the network fee", "Только комиссия сети")
        }
    }

    /// Solana's own programs: one quiet line after the ones that do the work.
    nonisolated static let solanaPlumbing: Set<String> = ["System", "Compute Budget", "SPL Token", "Associated Token", "Address Lookup Table"]

    /// «It calls: Kamino Vault · 7xKX…sAsU (not on ihatecrypto's list) · and Solana's own: System, Compute Budget».
    nonisolated static func callsLine(_ programs: [(name: String, known: Bool)], say: (String, String) -> String) -> String {
        let work = programs.filter { !solanaPlumbing.contains($0.name) }.map { $0.known ? $0.name : $0.name + say(" (not on ihatecrypto's list)", " (нет в списке ihatecrypto)") }
        let plumbing = programs.map(\.name).filter { solanaPlumbing.contains($0) }
        return say("It calls: ", "Вызывает: ") + (work + (plumbing.isEmpty ? [] : [say("and Solana's own: ", "и служебные Solana: ") + plumbing.joined(separator: ", ")])).joined(separator: " · ")
    }

    /// A held Solana token's price, from the portfolio's row for its mint: the ≈ $ beside a site's balance change.
    func heldSolanaPrice(_ mint: String) -> Decimal? {
        guard let row = portfolio.networks.first(where: { $0.key == "solana" })?.rows.first(where: { $0.contract == mint }), row.value > 0, let usd = row.usd else { return nil }
        return usd / row.value
    }

    /// The second check on a site's transaction (the Mac's jevNote): its concern as a warning, and when it didn't answer
    /// or isn't connected the card says so — the code checks decide.
    private func siteSecondCheck(host: String, title: String, programs: [String], changes: [String], lines: inout [String], ticks: inout [SiteTick]) async {
        guard let key = jevKey, secondCheck else {
            lines.append(say("The second check isn't connected (Settings › AI model › Second check). The code checks decide.",
                             "Вторая проверка не подключена (Настройки › Модель ИИ › Вторая проверка). Решают проверки кода."))
            return
        }
        guard let r = try? await JevReviewer.site(host: host, title: title, programs: programs, changes: changes, apiKey: key) else {
            lines.append(say("The second check didn't answer — the code checks decide.", "Вторая проверка не ответила — решают проверки кода."))
            return
        }
        if r.concern { ticks.append(Self.jevTick(r.text, say: say)) }
    }

    /// Our token accounts, classic SPL and Token-2022 alike: address → mint, raw amount, decimals.
    private func tokenAccounts(_ owner: String, _ rpc: SolanaRPC) async throws -> [String: (mint: String, amount: UInt64, decimals: Int)] {
        var items: [[String: Any]] = []
        for program in [SPL.tokenProgram, SPL.token2022Program] {
            let r = try await rpc.call("getTokenAccountsByOwner", [owner, ["programId": program], ["encoding": "jsonParsed", "commitment": "confirmed"]]) as? [String: Any]
            items += r?["value"] as? [[String: Any]] ?? []
        }
        var out: [String: (String, UInt64, Int)] = [:]
        for item in items {
            guard let address = item["pubkey"] as? String,
                  let info = ((((item["account"] as? [String: Any])?["data"] as? [String: Any])?["parsed"] as? [String: Any])?["info"] as? [String: Any]),
                  let mint = info["mint"] as? String, let amount = info["tokenAmount"] as? [String: Any] else { continue }
            out[address] = (mint, UInt64(amount["amount"] as? String ?? "0") ?? 0, (amount["decimals"] as? NSNumber)?.intValue ?? 0)
        }
        return out
    }
}

extension Engine {
    /// What the browser's bar shows for the page open in it: whether this run connected it, how ihatecrypto knows the
    /// site («known», «lookalike» of `real`, «unknown», «local»), and whether MetaMask's list names it as phishing.
    /// Strings, as every answer: «true» or «false».
    func siteState(_ origin: String) async -> [String: String] {
        guard let url = URL(string: origin), let host = url.host() else { return ["connected": "false", "check": "unknown", "blocked": "false"] }
        let connected = sites.solana.contains(origin) || sites.evm.contains(origin)
        var out = ["connected": String(connected), "blocked": String(await PhishingList.shared.contains(host))]
        switch SiteCheck.check(host: host) {
        case .known: out["check"] = "known"
        case .lookalike(let real): out["check"] = "lookalike"; out["real"] = real
        case .unknown: out["check"] = "unknown"
        case .local: out["check"] = "local"
        }
        return out
    }
}

/// The sites connected in this run: their origins, by chain, and the EVM chain each switched to (Arbitrum by default).
struct ConnectedSites {
    var solana: Set<String> = []
    var evm: Set<String> = []
    var chain: [String: UInt64] = [:]
}
