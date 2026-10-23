import EVMChain
import Foundation
import GuardJev
import IhatecryptoCore
import SignerAPI
import SolanaChain
import TradeExecution
import TxGuard
import WalletCore

/// A site's request waiting for the person: its card on the canvas, answered by Confirm or Cancel, once.
final class SiteAsk {
    private var continuation: CheckedContinuation<Bool, Never>?
    init(_ c: CheckedContinuation<Bool, Never>) { continuation = c }
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
    /// arb1 crashes on eth_simulateV1; publicnode runs it.
    static let simulationRPC: [UInt64: String] = [42161: "https://arbitrum-one-rpc.publicnode.com"]

    /// One call from a page: the answer as JSON text, or an error «code|message» for EIP-1193.
    func dapp(_ method: String, params text: String, origin: String) async throws -> String {
        guard let url = URL(string: origin), url.scheme == "https", let host = url.host() else { throw SiteError("Only https sites get the wallet.") }
        let params = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
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
            return try await ethereum(params["method"] as? String ?? "", params["params"] as? String ?? "[]", origin: origin, host: host, site: site)
        case "connect":
            guard let owner = wallet.solana else { throw SiteError(wallet.missing("solana") ?? "No Solana address.") }
            if !sites.solana.contains(origin) {
                if params["silent"] as? Bool == true { return "null" }
                guard await ask(host, site: site, title: say("\(host) asks to connect to your Solana wallet", "\(host) просит подключить твой кошелёк Solana"),
                                lines: [say("It sees your address and can ask you to sign; nothing is signed without your Confirm.", "Сайт увидит адрес и сможет просить подпись; без твоего «Подтвердить» ничего не подписывается."), owner])
                else { throw SiteError("The user declined.") }
                sites.solana.insert(origin)
            }
            return try json(["address": owner, "publicKey": try Base58.decode(owner).base64EncodedString()])
        case "disconnect":
            sites.solana.remove(origin)
            // The bar's Disconnect takes the wallet away from the site on every chain, as the Mac's disconnectHere; a
            // page's own Solana disconnect leaves its EVM connection alone. Either only takes access away.
            if params["everywhere"] as? Bool == true {
                sites.evm.remove(origin)
                sites.chain[origin] = nil
            }
            return "null"
        case "signMessage":
            guard sites.solana.contains(origin), let b64 = params["message"] as? String, let bytes = Data(base64Encoded: b64) else { throw SiteError("Connect the site first.") }
            let text = String(data: bytes, encoding: .utf8) ?? "(not text: \(bytes.count) bytes)"
            guard await ask(host, site: site, title: say("\(host) asks to sign a message", "\(host) просит подписать сообщение"),
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
            let review = try await solanaReview(wire, owner: owner, host: host)
            guard review.problems.isEmpty else {
                pin("result", say("\(host): transaction refused", "\(host): транзакция отклонена"), lines: review.problems + review.lines)
                reply(say("\(host) sent a transaction ihatecrypto won't sign: ", "\(host) прислал транзакцию, которую ihatecrypto не подпишет: ") + review.problems.joined(separator: " "), note: "browser")
                throw SiteError("Transaction declined: " + review.problems.joined(separator: " "))
            }
            guard await ask(host, site: site, title: (send ? say("\(host) asks to sign and send a transaction", "\(host) просит подписать и отправить транзакцию")
                                                         : say("\(host) asks to sign a transaction", "\(host) просит подписать транзакцию")),
                            lines: review.lines, warnings: review.warnings)
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

    /// A request waits on the canvas for Confirm; one at a time, so a page can't swap the one being read for another.
    private func ask(_ host: String, site: SiteCheck.Verdict, title: String, lines: [String], warnings: [String] = []) async -> Bool {
        if case .site? = pending { return false }
        var warnings = warnings
        switch site {
        case .lookalike(let real): warnings.insert(say("This address looks like \(real) but isn't it: a lookalike site steals wallets.", "Адрес похож на \(real), но это не он: сайты-двойники крадут кошельки."), at: 0)
        case .unknown: warnings.append(say("ihatecrypto doesn't know \(host): check the address bar.", "ihatecrypto не знает \(host): проверь адрес в строке."))
        case .known, .local: break
        }
        out.event("showMain", ["why": "site"])
        out.event("notify", ["title": host, "body": say("Asks for your wallet: look at ihatecrypto's window.", "Просит кошелёк: посмотри в окно ihatecrypto.")])
        return await withCheckedContinuation { c in
            show(.site(SiteAsk(c)), title: title, lines: lines, warnings: warnings)
        }
    }

    /// EIP-1193 requests. Answers are JSON text; errors are «code|message».
    private func ethereum(_ method: String, _ paramsJSON: String, origin: String, host: String, site: SiteCheck.Verdict) async throws -> String {
        guard let address = activeWallet?.evm else { throw SiteError("4100|" + (activeWallet?.missing("evm") ?? "No EVM address.")) }
        let args = (try? JSONSerialization.jsonObject(with: Data(paramsJSON.utf8))) as? [Any] ?? []
        let chain = sites.chain[origin] ?? 42161
        func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed), as: UTF8.self) }
        func hex(_ s: EVMSignature) -> String { "0x" + s.r.hex + s.s.hex + String(format: "%02x", s.v) }
        func rpc() throws -> EVMRPC {
            guard let n = EVMNetwork.all.first(where: { $0.rpc.chainId == chain }) else { throw SiteError("4901|No RPC for chain \(chain).") }
            return n.rpc
        }
        switch method {
        case "eth_chainId": return try json("0x" + String(chain, radix: 16))
        case "net_version": return try json("\(chain)")
        case "eth_accounts": return try json(sites.evm.contains(origin) ? [address] : [])
        case "eth_requestAccounts":
            if !sites.evm.contains(origin) {
                guard await ask(host, site: site, title: say("\(host) asks to connect to your EVM wallet", "\(host) просит подключить твой EVM-кошелёк"),
                                lines: [say("It sees your address and can ask you to sign; nothing is signed without your Confirm.", "Сайт увидит адрес и сможет просить подпись; без твоего «Подтвердить» ничего не подписывается."), address])
                else { throw SiteError("4001|The user declined.") }
                sites.evm.insert(origin)
            }
            return try json([address])
        case "wallet_switchEthereumChain":
            guard let id = (args.first as? [String: Any])?["chainId"] as? String, let n = UInt64(id.dropFirst(2), radix: 16),
                  EVMNetwork.all.contains(where: { $0.rpc.chainId == n }) else {
                throw SiteError("4902|ihatecrypto works with " + EVMNetwork.all.map(\.name).joined(separator: ", ") + ".")
            }
            sites.chain[origin] = n
            return "null"
        case "personal_sign":
            guard sites.evm.contains(origin), let raw = args.first as? String else { throw SiteError("4100|Connect the site first.") }
            let message = raw.hasPrefix("0x") ? (Data(hex: raw) ?? Data(raw.utf8)) : Data(raw.utf8)
            guard !PersonalMessage.isBlindHash(message) else { throw SiteError("4001|The site asks to sign a hash, not text: ihatecrypto doesn't sign that.") }
            guard await ask(host, site: site, title: say("\(host) asks to sign a message", "\(host) просит подписать сообщение"),
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
            guard await ask(host, site: site, title: say("\(host) asks to sign “\(typed.primaryType)”", "\(host) просит подписать «\(typed.primaryType)»"),
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
            let review = await evmReview(to: to, value: value, data: data, from: address, rpc: rpc)
            guard review.problems.isEmpty else {
                pin("result", say("\(host): EVM transaction refused", "\(host): EVM-транзакция отклонена"), lines: review.problems + review.lines)
                reply("Site \(host): EVM transaction declined — \(review.problems.joined(separator: " "))", note: "browser")
                throw SiteError("4001|" + (review.problems.first ?? "Declined."))
            }
            guard await ask(host, site: site, title: say("\(host) asks to send a transaction on \(EVMNetwork.all.first { $0.rpc.chainId == chain }?.name ?? "chain \(chain)")",
                                                         "\(host) просит отправить транзакцию в \(EVMNetwork.all.first { $0.rpc.chainId == chain }?.name ?? "сети \(chain)")"),
                            lines: review.lines + review.changes.map { say("Balance: ", "Баланс: ") + $0 }, warnings: review.warnings)
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
    }

    /// Decodes the call, names the token, simulates our balances and estimates gas, as the Mac does. Problems mean «don't sign».
    func evmReview(to: String, value: UInt64, data: Data, from: String, rpc: EVMRPC) async -> SiteReview {
        let call = EVMCall.decode(data)
        var problems = call.refusal(sender: from, chainId: rpc.chainId).map { [$0] } ?? []
        var payees = call.payees(to: to, value: value, chainId: rpc.chainId)
        if let target = call.unreadTarget(to: to, value: value, chainId: rpc.chainId) { payees.append(target) }
        let recipients = Array(Set(payees.map { $0.lowercased() }.filter { $0 != from.lowercased() })).sorted()
        let native = EVMNetwork.all.first { $0.rpc.chainId == rpc.chainId }?.gasCoin ?? "coins"
        func token(_ address: String) async -> (symbol: String, decimals: Int) {
            let symbol = (try? await rpc.read(address, Data([0x95, 0xd8, 0x9b, 0x41]))).flatMap(ABI.string) ?? "token \(address.prefix(8))…"
            let decimals = (try? await rpc.read(address, Data([0x31, 0x3c, 0xe5, 0x67]))).flatMap { ABI.uint(in: $0, word: 0) } ?? 18
            return (symbol, Int(decimals))
        }
        var lines = [call.summary(chainId: rpc.chainId, to: to, value: value)]
        if case .approve(_, let word) = call {
            let t = await token(to)
            let n = word.reduce(Decimal(0)) { $0 * 256 + Decimal($1) } / Decimal(sign: .plus, exponent: t.decimals, significand: 1)
            lines.append(say("Token: \(t.symbol), up to \(word.allSatisfy { $0 == 0xff } ? "unlimited" : Fmt.num(n, maxFraction: 6))", "Токен: \(t.symbol), до \(word.allSatisfy { $0 == 0xff } ? "без лимита" : Fmt.num(n, maxFraction: 6))"))
        }
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
        if case .contract = call { warnings.append(say("ihatecrypto can't read this contract call: sign only if you trust the site.", "ihatecrypto не может прочитать этот вызов контракта: подписывай, только если доверяешь сайту.")) }
        if let key = jevKey, secondCheck, let r = try? await JevReviewer.site(host: "", title: "", programs: ["EVM \(rpc.chainId): \(to)"], changes: lines + changes, apiKey: key), r.concern {
            warnings.append("Jev: " + r.text)
        }
        return SiteReview(lines: lines, changes: changes, warnings: warnings, problems: problems, recipients: recipients, gas: gas)
    }

    /// Programs, the guard's verdict and the simulated change of every balance of ours the transaction touches.
    func solanaReview(_ wire: Data, owner: String, host: String) async throws -> SiteReview {
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
        var changes: [String] = []
        func signed(_ d: Decimal) -> String { (d > 0 ? "+" : d < 0 ? "−" : "") + Fmt.num(abs(d), maxFraction: 9) }
        if let error = simulation.error {
            problems.append("Simulation failed: \(error).")
        } else {
            let solAfter = simulation.after[owner]?.lamports ?? solBefore
            changes.append("SOL: " + signed(Decimal(Int64(solAfter) - Int64(solBefore)) / 1_000_000_000))
            for (address, t) in touched.sorted(by: { $0.key < $1.key }) {
                let after = simulation.after[address]?.token ?? t.amount
                let delta = Decimal(Int64(after) - Int64(t.amount)) / Decimal(sign: .plus, exponent: t.decimals, significand: 1)
                if delta != 0 {
                    let symbol = t.mint == SpotAsset.usdc.mint ? "USDC" : SpotAsset.all.first { $0.mint == t.mint }?.symbol ?? "token \(t.mint.prefix(6))…"
                    changes.append("\(symbol): \(signed(delta))")
                }
            }
        }
        let named = programs.sorted().map { SolanaGuard.knownPrograms[$0] ?? "\($0.prefix(4))…\($0.suffix(4))" + say(" (not on ihatecrypto's list)", " (нет в списке ihatecrypto)") }
        var seen = Set<String>()
        var lines = changes.map { say("Your balance, simulated: ", "Твой баланс, по симуляции: ") + $0 }
        lines.append(say("Calls: ", "Вызывает: ") + named.joined(separator: ", "))
        lines += verdict.checks.filter { !$0.hasPrefix("Calls unknown program") && seen.insert($0).inserted }
        var warnings: [String] = []
        if !unknown.isEmpty {
            warnings.append(say("\(unknown.count == 1 ? "A program isn't" : "Programs aren't") on ihatecrypto's list: signing calls them the way the site built the transaction.",
                                "\(unknown.count == 1 ? "Программы нет" : "Программ нет") в списке ihatecrypto: подпись вызовет их так, как сайт собрал транзакцию."))
        }
        if let key = jevKey, secondCheck, let r = try? await JevReviewer.site(host: host, title: "", programs: named, changes: changes, apiKey: key), r.concern {
            warnings.append("Jev: " + r.text)
        }
        return SiteReview(lines: lines, changes: changes, warnings: warnings, problems: problems, accounts: accounts, tokenOwners: owners, unknownPrograms: unknown)
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
