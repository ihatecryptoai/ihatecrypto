import EVMChain
import GuardJev
import IhatecryptoCore
import SignerAPI
import SolanaChain
import SwiftUI
import TxGuard
import WalletCore
import WebKit

/// One request from a page, waiting for the user in the approval sheet.
struct SiteRequest: Identifiable {
    enum Kind { case connect, connectEVM, message(String), typedData(TypedData), transaction(TxReview, send: Bool), evmTransaction(EVMReview) }
    let id = UUID()
    let origin: String
    let site: SiteCheck.Verdict
    let kind: Kind
    let answer: CheckedContinuation<Bool, Never>
}

/// What a page's transaction would do, worked out before anything is signed.
struct TxReview {
    let wire: Data
    let accounts: [String]
    let tokenOwners: [String: String]
    /// Programs outside the built-in list: signed only if the user allows them here.
    let unknownPrograms: [String]
    let programNames: [String]
    let changes: [String]
    /// The same changes for the card: the token, its mint, the signed amount.
    var deltas: [(symbol: String, mint: String, amount: Decimal)] = []
    /// Every program it calls, by name, and whether it is on the list ihatecrypto signs for without asking.
    var programs: [(name: String, known: Bool)] = []
    let checks: [String]
    /// Any entry: the transaction is refused, whatever the user clicks.
    let problems: [String]
    /// Jev's second opinion on the site and the transaction; it can only ask for an extra confirmation.
    var jev: JevReview?
    var jevNote: String?
}

/// An EVM transaction from a site, decoded and simulated before anything is signed.
struct EVMReview {
    let chain: UInt64
    let to: String
    let value: UInt64
    let data: Data
    /// What it does, in words: «Allow 0xabc… to spend up to 5 USDC».
    let lines: [String]
    /// Simulated change of each of our balances: «USDC (0xaf88…5831): +5».
    let changes: [String]
    /// Foreign addresses money, or the right to spend it, goes to (EVMCall.payees), which the user must confirm.
    let recipients: [String]
    let gas: UInt64
    let problems: [String]
    var jev: JevReview?
}

@Observable
final class Browser {
    let webView: WKWebView
    private let bridge = Bridge()
    var address = "https://jup.ag"
    var title = ""
    var pending: SiteRequest?
    var host = ""
    /// The last page that failed to load, shown under the address bar.
    var loadError: String?
    /// A host on MetaMask's phishing list that the browser refused to open (Phishing.swift).
    var blocked: String?
    /// Origins the user connected in this session.
    private var connected: Set<String> = []
    private var connectedEVM: Set<String> = []
    /// EVM chain a site switched to (Arbitrum by default). Only reading goes to that chain's public RPC.
    private var evmChain: [String: UInt64] = [:]
    static let evmRPC: [UInt64: String] = [1: "https://ethereum-rpc.publicnode.com", 42161: "https://arb1.arbitrum.io/rpc", 8453: "https://mainnet.base.org"]
    /// arb1 crashes on eth_simulateV1; publicnode runs it but refuses receipts without a token (2026-09-29).
    static let evmSimulationRPC: [UInt64: String] = [42161: "https://arbitrum-one-rpc.publicnode.com"]
    static let readOnly: Set<String> = ["eth_call", "eth_blockNumber", "eth_getBalance", "eth_getCode", "eth_getTransactionCount", "eth_getTransactionReceipt",
                                        "eth_getTransactionByHash", "eth_estimateGas", "eth_gasPrice", "eth_feeHistory", "eth_getBlockByNumber",
                                        "eth_getLogs", "eth_maxPriorityFeePerGas", "eth_getStorageAt"]
    weak var workspace: Workspace?
    private let rpc = SolanaRPC()

    init() {
        let config = WKWebViewConfiguration()
        config.userContentController.addUserScript(WKUserScript(source: ProviderScript.source, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        config.userContentController.addScriptMessageHandler(bridge, contentWorld: .page, name: "ihatecrypto")
        config.websiteDataStore = .default()
        webView = WKWebView(frame: .zero, configuration: config)
        webView.customUserAgent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15"
        bridge.browser = self
        Task { await PhishingList.shared.refresh() }
    }

    /// The open page's origin, as WebKit names it for the page's requests.
    private var origin: String? {
        URL(string: address).flatMap { u in u.host().map { "\(u.scheme ?? "https")://\($0)\(u.port.map { ":\($0)" } ?? "")" } }
    }
    /// The open site has the wallet (Solana or EVM) in this session.
    var connectedHere: Bool { origin.map { connected.contains($0) || connectedEVM.contains($0) } ?? false }

    /// Takes the wallet away from the open site and reloads it, so the page forgets the address too.
    func disconnectHere() {
        guard let origin else { return }
        connected.remove(origin)
        connectedEVM.remove(origin)
        webView.reload()
    }

    func open(_ text: String) {
        var t = text.trimmingCharacters(in: .whitespaces)
        if !t.contains("://") { t = "https://" + t }
        guard let url = URL(string: t) else { return }
        blocked = nil
        address = url.absoluteString
        if url.isFileURL { webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent()) }
        else { webView.load(URLRequest(url: url)) }
    }

    func pageChanged() {
        title = webView.title ?? ""
        if let url = webView.url { address = url.absoluteString; host = url.host ?? "" }
    }

    // MARK: Requests from the page

    /// Every call from a page. `origin` comes from WebKit, not from the page, so a page cannot claim another's.
    func handle(_ method: String, _ params: [String: Any], origin: String, host: String) async throws -> Any {
        if workspace?.ledgerActive == true {
            throw SiteError((method == "eth" ? "4100|" : "") + "The browser signs with ihatecrypto's own wallets for now: switch from the Ledger to use a site.")
        }
        guard let owner = workspace?.signerWallet?.solana else {
            throw SiteError("The ihatecrypto wallet isn't in secure storage: save the key via “Addresses from phrase…”.")
        }
        guard !PhishingList.shared.contains(host) else {
            throw SiteError((method == "eth" ? "4100|" : "") + "\(host) is on MetaMask's list of phishing sites: ihatecrypto gives it no wallet.")
        }
        let site = SiteCheck.check(host: host)
        // A site's signature is a move of its own: its Touch ID doesn't carry over to the next request.
        let signs = ["signMessage", "signTransaction", "signAndSendTransaction"].contains(method)
            || (method == "eth" && ["personal_sign", "eth_signTypedData_v4", "eth_sendTransaction"].contains(params["method"] as? String ?? ""))
        defer { if signs { workspace?.endSigningSession() } }
        switch method {
        case "eth":
            return try await ethereum(params["method"] as? String ?? "", params["params"] as? String ?? "[]", origin: origin, site: site)
        case "connect":
            if !connected.contains(origin) {
                if params["silent"] as? Bool == true { return NSNull() }
                guard await ask(.connect, origin: origin, site: site) else { throw SiteError("The user declined.") }
                connected.insert(origin)
            }
            return ["address": owner, "publicKey": try Base58.decode(owner).base64EncodedString()]
        case "disconnect":
            connected.remove(origin)
            return NSNull()
        case "signMessage":
            guard connected.contains(origin), let b64 = params["message"] as? String, let bytes = Data(base64Encoded: b64) else { throw SiteError("Connect the site first.") }
            let text = String(data: bytes, encoding: .utf8) ?? "(not text: \(bytes.count) bytes)"
            guard await ask(.message(text), origin: origin, site: site) else { throw SiteError("The user declined.") }
            let reply = try await SignerClient().send(.signSolanaMessage(message: bytes))
            guard reply.ok, let signature = reply.signatureBytes else { throw SiteError(XPCTradeSigner.reason(reply)) }
            return ["signature": signature.base64EncodedString()]
        case "signTransaction", "signAndSendTransaction":
            guard connected.contains(origin), let b64 = params["transaction"] as? String, let wire = Data(base64Encoded: b64) else { throw SiteError("Connect the site first.") }
            let send = method == "signAndSendTransaction"
            var review = try await review(wire, owner: owner)
            if let ws = workspace, ws.jevScreening, let key = ws.jevKey {
                review.jev = try? await JevReviewer.site(host: host.isEmpty ? origin : host, title: title, programs: review.programNames, changes: review.changes, apiKey: key)
                if review.jev == nil { review.jevNote = "The second check didn't answer — the code checks decide." }
            } else {
                review.jevNote = "The second check isn't connected (Settings › Second check). The code checks decide."
            }
            guard await ask(.transaction(review, send: send), origin: origin, site: site), review.problems.isEmpty else { throw SiteError("Transaction declined.") }
            let reply = try await SignerClient().send(.signSolana(wire: wire, accounts: review.accounts, recipients: [], tokenOwners: review.tokenOwners,
                                                                  extraPrograms: review.unknownPrograms))
            guard reply.ok, let signed = reply.signedWire else { throw SiteError(XPCTradeSigner.reason(reply)) }
            // The signer must return the page's transaction with only our signature added.
            let prefix = 1 + 64
            guard signed.count == wire.count, signed.subdata(in: prefix..<signed.count) == wire.subdata(in: prefix..<wire.count) else {
                throw SiteError("What was signed isn't what was checked.")
            }
            let signature = signed.subdata(in: 1..<65)
            workspace?.reply("Site \(origin): transaction signed\(send ? " and sent" : "") — \(Base58.encode(signature).prefix(12))…", note: "browser")
            // A deposit on the site shows on the portfolio once it has landed.
            Task { [weak workspace] in
                try? await Task.sleep(for: .seconds(10))
                await workspace?.loadProtocols(force: true)
            }
            if send {
                _ = try await rpc.send(signed)
                return ["signature": signature.base64EncodedString()]
            }
            return ["signed": signed.base64EncodedString()]
        default:
            throw SiteError("ihatecrypto doesn't support \(method).")
        }
    }

    /// EIP-1193 requests. Answers are JSON text; errors are "code|message".
    private func ethereum(_ method: String, _ paramsJSON: String, origin: String, site: SiteCheck.Verdict) async throws -> String {
        guard let address = workspace?.signerWallet?.evm else { throw SiteError("4100|The ihatecrypto wallet isn't in secure storage.") }
        let args = (try? JSONSerialization.jsonObject(with: Data(paramsJSON.utf8))) as? [Any] ?? []
        let chain = evmChain[origin] ?? 42161
        func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed), as: UTF8.self) }
        func hex(_ s: EVMSignature) -> String { "0x" + s.r.hex + s.s.hex + String(format: "%02x", s.v) }
        switch method {
        case "eth_chainId": return try json("0x" + String(chain, radix: 16))
        case "net_version": return try json("\(chain)")
        case "eth_accounts": return try json(connectedEVM.contains(origin) ? [address] : [])
        case "eth_requestAccounts":
            if !connectedEVM.contains(origin) {
                guard await ask(.connectEVM, origin: origin, site: site) else { throw SiteError("4001|The user declined.") }
                connectedEVM.insert(origin)
            }
            return try json([address])
        case "wallet_switchEthereumChain":
            guard let id = (args.first as? [String: Any])?["chainId"] as? String, let n = UInt64(id.dropFirst(2), radix: 16), Self.evmRPC[n] != nil else {
                throw SiteError("4902|ihatecrypto works with Ethereum, Arbitrum and Base.")
            }
            evmChain[origin] = n
            return "null"
        case "personal_sign":
            guard connectedEVM.contains(origin), let raw = args.first as? String else { throw SiteError("4100|Connect the site first.") }
            let message = raw.hasPrefix("0x") ? (Data(hex: raw) ?? Data(raw.utf8)) : Data(raw.utf8)
            guard !PersonalMessage.isBlindHash(message) else { throw SiteError("4001|The site asks to sign a hash, not text: ihatecrypto doesn't sign that.") }
            guard await ask(.message(String(data: message, encoding: .utf8) ?? "\(message.count) bytes"), origin: origin, site: site) else { throw SiteError("4001|The user declined.") }
            let reply = try await SignerClient().send(.signPersonal(message: message))
            guard reply.ok, let s = reply.signature else { throw SiteError("4001|" + XPCTradeSigner.reason(reply)) }
            return try json(hex(s))
        case "eth_signTypedData_v4":
            guard connectedEVM.contains(origin), args.count >= 2 else { throw SiteError("4100|Connect the site first.") }
            let body: Data = if let s = args[1] as? String { Data(s.utf8) } else { try JSONSerialization.data(withJSONObject: args[1]) }
            let typed: TypedData
            do { typed = try TypedData.parse(body) } catch { throw SiteError("4200|\(error.localizedDescription)") }
            if let refusal = typed.refusal(owner: address) {
                workspace?.reply("Site \(origin) asked for a “\(typed.primaryType)” signature. Declined — \(refusal)", note: "browser")
                throw SiteError("4001|" + refusal)
            }
            guard await ask(.typedData(typed), origin: origin, site: site) else { throw SiteError("4001|The user declined.") }
            let reply = try await SignerClient().send(.signTypedData(json: body))
            guard reply.ok, let s = reply.signature else { throw SiteError("4001|" + XPCTradeSigner.reason(reply)) }
            return try json(hex(s))
        case "eth_sign":
            throw SiteError("4200|eth_sign signs a hash blindly — ihatecrypto doesn't support it.")
        case "eth_signTransaction":
            throw SiteError("4200|ihatecrypto signs EVM transactions only when it also sends them (eth_sendTransaction).")
        case "eth_sendTransaction":
            guard connectedEVM.contains(origin), let p = args.first as? [String: Any] else { throw SiteError("4100|Connect the site first.") }
            guard let to = p["to"] as? String, Data(hex: to)?.count == 20 else { throw SiteError("4200|ihatecrypto doesn't sign contract creation.") }
            guard (p["from"] as? String)?.lowercased() ?? address.lowercased() == address.lowercased() else { throw SiteError("4100|The sender isn't your address.") }
            let data = Data(hex: (p["data"] ?? p["input"]) as? String ?? "0x") ?? Data()
            guard let value = Self.quantity(p["value"]) else { throw SiteError("4200|The amount is larger than ihatecrypto will sign.") }
            guard let url = Self.evmRPC[chain].flatMap(URL.init(string:)) else { throw SiteError("4901|No RPC for chain \(chain).") }
            let rpc = EVMRPC(url: url, chainId: chain)
            var review = await evmReview(to: to, value: value, data: data, from: address, rpc: rpc)
            if let ws = workspace, ws.jevScreening, let key = ws.jevKey {
                review.jev = try? await JevReviewer.site(host: host(origin), title: title, programs: ["EVM \(chain): \(to)"], changes: review.lines + review.changes, apiKey: key)
            }
            guard await ask(.evmTransaction(review), origin: origin, site: site), review.problems.isEmpty else {
                if let refusal = review.problems.first { workspace?.reply("Site \(origin): EVM transaction declined — \(refusal)", note: "browser") }
                throw SiteError("4001|" + (review.problems.first ?? "The user declined."))
            }
            let nonce = try await rpc.nonce(address)
            let fees = try await rpc.fees()
            let reply = try await SignerClient().send(.signEVMTransaction(chainId: chain, nonce: nonce, maxPriorityFee: fees.maxPriority, maxFee: fees.maxFee,
                                                                         gasLimit: review.gas, to: to, value: value, data: data,
                                                                         recipients: review.recipients))
            guard reply.ok, let raw = reply.signedRaw else { throw SiteError("4001|" + XPCTradeSigner.reason(reply)) }
            do {
                let hash = try await rpc.send(raw)
                workspace?.reply("Site \(origin): EVM transaction sent on chain \(chain) — \(hash.prefix(14))…", note: "browser")
                return try json(hash)
            } catch {
                throw SiteError("-32000|The network rejected the transaction: \(error.localizedDescription)")
            }
        case let m where Self.readOnly.contains(m):
            guard let url = Self.evmRPC[chain].flatMap(URL.init(string:)) else { throw SiteError("4901|No RPC for chain \(chain).") }
            var request = URLRequest(url: url, timeoutInterval: 20)
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

    private func host(_ origin: String) -> String { URL(string: origin)?.host ?? origin }

    /// A JSON-RPC quantity ("0x…") as UInt64; nil when it does not fit.
    static func quantity(_ v: Any?) -> UInt64? {
        guard let s = v as? String, s.hasPrefix("0x") else { return v == nil ? 0 : (v as? NSNumber)?.uint64Value }
        return s == "0x" ? 0 : UInt64(s.dropFirst(2), radix: 16)
    }

    /// Decodes, names the token, simulates the balance changes and estimates gas. Problems mean «do not sign».
    func evmReview(to: String, value: UInt64, data: Data, from: String, rpc: EVMRPC) async -> EVMReview {
        let call = EVMCall.decode(data)
        var problems = [call.refusal(sender: from, chainId: rpc.chainId), EVMCall.targetRefusal(to: to, data: data, sender: from)].compactMap { $0 }
        var lines: [String] = []
        // The same payees the signer checks: each foreign one needs the tick here and Touch ID there. A contract whose call
        // ihatecrypto can't read is allowed the same way, once: the signer asks with its address on the prompt.
        var payees = call.payees(to: to, value: value, chainId: rpc.chainId)
        if let target = call.unreadTarget(to: to, value: value, chainId: rpc.chainId) { payees.append(target) }
        let recipients = Array(Set(payees.map { $0.lowercased() }.filter { $0 != from.lowercased() })).sorted()
        let native = rpc.chainId == 42161 || rpc.chainId == 1 || rpc.chainId == 8453 ? "ETH" : "coins"
        func token(_ to: String) async -> (symbol: String, decimals: Int) {
            let symbol = (try? await rpc.call("eth_call", [["to": to, "data": "0x95d89b41"], "latest"]) as? String).flatMap { hex -> String? in
                guard let d = Data(hex: hex), d.count >= 96 else { return nil }
                let length = Int(d[d.startIndex + 63])
                return String(data: d.dropFirst(64).prefix(length), encoding: .utf8)
            } ?? "token \(to.prefix(8))…"
            let decimals = (try? await rpc.call("eth_call", [["to": to, "data": "0x313ce567"], "latest"]) as? String).flatMap { UInt64($0.dropFirst(2), radix: 16) } ?? 18
            return (symbol, Int(decimals))
        }
        func amount(_ word: Data, _ decimals: Int) -> String {
            let n = word.reduce(Decimal(0)) { $0 * 256 + Decimal($1) }
            return Fmt.num(n / Decimal(sign: .plus, exponent: decimals, significand: 1), maxFraction: 6)
        }
        switch call {
        case .plain:
            lines.append("Transfer \(Fmt.num(Decimal(value) / 1_000_000_000_000_000_000, maxFraction: 8)) \(native) to \(to).")
        case .approve(let spender, let a), .increaseAllowance(let spender, let a):
            let t = await token(to)
            lines.append("Allow \(spender) to spend up to \(amount(a, t.decimals)) \(t.symbol).")
        case .transfer(let r, let a):
            let t = await token(to)
            lines.append("Transfer \(amount(a, t.decimals)) \(t.symbol) to \(r).")
        case .nftTransfer(let f, let r):
            lines.append("Move NFTs of \(to) from \(f.lowercased() == from.lowercased() ? "this wallet" : f) to \(r).")
        case .delegation(let d, let on):
            lines.append(on ? "Let \(d) borrow against or manage your lending positions." : "Revoke \(d)'s rights over your lending positions.")
        case .transferFrom(let f, let r, let a):
            let t = await token(to)
            lines.append("Move \(amount(a, t.decimals)) \(t.symbol) from \(f) to \(r).")
        case .setApprovalForAll(let op, let on):
            lines.append("\(on ? "Rights to all NFTs" : "Revoke NFT rights") for \(op).")
        case .permit:
            lines.append("Call to permit (permission to spend).")
        case .swap(let r):
            lines.append("Uniswap swap paying \(r.lowercased() == from.lowercased() ? "this wallet" : r).")
        case .kyberSwap(let k):
            let t = await token(k.srcToken)
            lines.append("KyberSwap swap of \(amount((try? ABI.word(k.amount)) ?? Data(count: 32), t.decimals)) \(t.symbol) paying \(k.dstReceiver.lowercased() == from.lowercased() ? "this wallet" : k.dstReceiver)\(k.feeReceivers.isEmpty ? "" : ", a fee to \(k.feeReceivers.joined(separator: ", "))").")
        case .borrow(let asset, let a, let owner):
            let t = await token(asset)
            lines.append("Borrow \(amount(a, t.decimals)) \(t.symbol) from \(to), the debt on \(owner.lowercased() == from.lowercased() ? "this wallet" : owner).")
        case .repay(let asset, let a, let owner):
            let t = await token(asset)
            lines.append("Repay \(a.allSatisfy { $0 == 0xff } ? "all the" : amount(a, t.decimals)) \(t.symbol) debt of \(owner.lowercased() == from.lowercased() ? "this wallet" : owner) to \(to).")
        case .multisigExec:
            lines.append("Carry out a signed move of the multisig \(to): " + call.summary(chainId: rpc.chainId, to: to, value: value) + ".")
        case .cctpBurn(let r, let domain, let a):
            lines.append("Circle's CCTP: burn \(amount(a, 6)) USDC here to mint on \(CCTP.chainName(domain)) to \(r.map { $0.lowercased() == from.lowercased() ? "this wallet" : $0 } ?? "a non-EVM account").")
        case .oftSend(let r, let eid, let a, let plain, _):
            let known = USDT0.on(rpc.chainId)?.oft == to.lowercased()
            lines.append("LayerZero: send \(known ? "\(amount(a, 6)) USDT" : "tokens of \(to)") to \(r.map { $0.lowercased() == from.lowercased() ? "this wallet" : $0 } ?? "an account that can't be read") on \(USDT0.networkName(eid: eid))\(plain ? "" : ", with options or a message of its own").")
        case .v4Swap(let s):
            let (a, b) = (await token(s.currencyIn), await token(s.currencyOut))
            func n(_ x: Decimal, _ d: Int) -> String { Fmt.num(x / Decimal(sign: .plus, exponent: d, significand: 1), maxFraction: 6) }
            lines.append("Uniswap v4 swap: \(n(s.amountIn, a.decimals)) \(a.symbol) for at least \(n(s.amountOutMinimum, b.decimals)) \(b.symbol), paid to this wallet.")
        case .permit2Approve(let contract, let spender, let a, let expiration):
            let t = await token(contract)
            lines.append("Allow \(spender) to spend up to \(amount(a, t.decimals)) \(t.symbol) through Permit2, until \(Fmt.date(Date(timeIntervalSince1970: TimeInterval(expiration)))).")
        case .vault(let r, let owner):
            lines.append("Vault \(owner == nil ? "deposit" : "withdrawal") crediting \(r.lowercased() == from.lowercased() ? "this wallet" : r).")
        case .lending(let b):
            lines.append("Lending pool call (Aave supply or withdraw) crediting \(b.lowercased() == from.lowercased() ? "this wallet" : b).")
        case .claim(let users):
            lines.append("Merkl rewards claim paying \(users.allSatisfy { $0.lowercased() == from.lowercased() } ? "this wallet" : users.joined(separator: ", ")).")
        case .wrap:
            lines.append("Wrap \(Fmt.num(Decimal(value) / 1_000_000_000_000_000_000, maxFraction: 8)) \(native) into its token at \(to)\((EVMCall.wrap.payees(to: to, value: value, chainId: rpc.chainId).isEmpty ? ", the network's own WETH" : " — not this network's WETH")).")
        case .unwrap(let a):
            lines.append("Unwrap \(amount(a, 18)) of the token at \(to) into \(native).")
        case .contract(let selector):
            lines.append("Contract call \(to), function \(selector)\(value > 0 ? ", with \(Fmt.num(Decimal(value) / 1_000_000_000_000_000_000, maxFraction: 8)) \(native)" : "").")
        }
        var changes: [String] = []
        let simulator = Self.evmSimulationRPC[rpc.chainId].flatMap(URL.init(string:)).map { EVMRPC(url: $0, chainId: rpc.chainId) } ?? rpc
        do {
            let sim = try await simulator.simulate(from: from, to: to, value: value, data: data)
            if let revert = sim.revert { problems.append("Simulation: the call would revert — \(revert)") }
            for (asset, delta) in sim.changes.sorted(by: { $0.key < $1.key }) {
                if asset == EVMSimulation.native { changes.insert("\(native): \(signed(delta / 1_000_000_000_000_000_000))", at: 0); continue }
                let t = await token(asset)
                let name = "\(t.symbol)\(sim.nfts.contains(asset) ? " NFT" : "") (\(asset.prefix(6))…\(asset.suffix(4)))"
                changes.append("\(name): \(signed(sim.nfts.contains(asset) ? delta : delta / Decimal(sign: .plus, exponent: t.decimals, significand: 1)))")
            }
            if sim.revert == nil, changes.isEmpty { changes.append("None of your balances change.") }
        } catch {
            // The node can't simulate: at least check that the call doesn't revert.
            changes.append("Unavailable: \(error.localizedDescription)")
            let tx: [String: Any] = ["from": from, "to": to, "value": "0x" + String(value, radix: 16), "data": "0x" + data.hex]
            do { _ = try await rpc.call("eth_call", [tx, "latest"]) } catch {
                problems.append("Simulation: the call would revert — \(error.localizedDescription)")
            }
        }
        var gas: UInt64 = 0
        if problems.isEmpty {
            do {
                gas = try await rpc.estimateGas(from: from, to: to, value: value, data: data) * 12 / 10
                if let fees = try? await rpc.fees() {
                    lines.append("Network fee up to \(Fmt.num(Decimal(gas) * Decimal(fees.maxFee) / 1_000_000_000_000_000_000, maxFraction: 8)) \(native).")
                }
            } catch {
                problems.append("Couldn't estimate gas: \(error.localizedDescription)")
            }
        }
        return EVMReview(chain: rpc.chainId, to: to, value: value, data: data, lines: lines, changes: changes, recipients: recipients, gas: gas, problems: problems)
    }

    private func ask(_ kind: SiteRequest.Kind, origin: String, site: SiteCheck.Verdict) async -> Bool {
        #if DEBUG
        // `-browserAutoApprove YES` (Debug builds): only for local test pages, and never over a refusal.
        if UserDefaults.standard.bool(forKey: "browserAutoApprove"), origin.hasPrefix("file://") {
            if case .transaction(let r, _) = kind { return r.problems.isEmpty }
            if case .evmTransaction(let r) = kind { return r.problems.isEmpty }
            return true
        }
        // `-browserAutoConnect YES`: local test pages only, connect and text only — transactions still ask.
        if UserDefaults.standard.bool(forKey: "browserAutoConnect"), origin.hasPrefix("file://") {
            if case .transaction = kind {} else { return true }
        }
        #endif
        // One request at a time: a page can't swap the one the user is reading for another. A second one is declined.
        guard pending == nil else { return false }
        return await withCheckedContinuation { continuation in
            pending = SiteRequest(origin: origin, site: site, kind: kind, answer: continuation)
        }
    }

    /// The user's answer to the request with this id, the one the sheet showed; any other is left alone.
    func answer(_ yes: Bool, to id: UUID) {
        guard let request = pending, request.id == id else { return }
        pending = nil
        request.answer.resume(returning: yes)
    }

    // MARK: Transaction review

    /// Programs, the guard's verdict and the simulated change of every balance of ours the transaction touches.
    func review(_ wire: Data, owner: String) async throws -> TxReview {
        let message = try SolanaMessage.parse(transaction: wire)
        let accounts = try await rpc.resolveAccounts(message)
        let programs = Set(message.instructions.compactMap { $0.programIndex < accounts.count ? accounts[$0.programIndex] : nil })
        let unknown = programs.filter { SolanaGuard.knownPrograms[$0] == nil }.sorted()
        var allowed = SolanaGuard.knownPrograms
        for p in unknown { allowed[p] = "unknown program \(p.prefix(6))…" }
        let tokens = try await tokenAccounts(owner)
        let touched = tokens.filter { accounts.contains($0.key) }
        let owners = touched.mapValues { _ in owner }
        let verdict = SolanaGuard.check(message, accounts: accounts,
                                        policy: SolanaPolicy(owner: owner, programs: allowed, trustLookupTables: false), tokenOwners: owners)
        let solBefore = try await rpc.balance(owner)
        let simulation = try await rpc.simulate(wire, watch: [owner] + touched.keys, sigVerify: false)
        var problems = verdict.problems
        var deltas: [(symbol: String, mint: String, amount: Decimal)] = []
        if let error = simulation.error {
            problems.append("Simulation failed: \(error).")
        } else {
            let solAfter = simulation.after[owner]?.lamports ?? solBefore
            deltas.append(("SOL", SpotAsset.all[0].mint, Decimal(Int64(solAfter) - Int64(solBefore)) / 1_000_000_000))
            for (address, t) in touched.sorted(by: { $0.key < $1.key }) {
                let after = simulation.after[address]?.token ?? t.amount
                let scale = Decimal(sign: .plus, exponent: t.decimals, significand: 1)
                let delta = Decimal(Int64(after) - Int64(t.amount)) / scale
                if delta != 0 { deltas.append((symbol(t.mint), t.mint, delta)) }
            }
        }
        // Programs ihatecrypto knows by name but doesn't sign for without asking are named, still marked as not on its list.
        let named = programs.sorted().map { p in
            (name: SolanaGuard.knownPrograms[p] ?? "\(p.prefix(4))…\(p.suffix(4))", known: SolanaGuard.knownPrograms[p] != nil)
        }
        // One line per kind of check: an instruction repeated to the same program said the same thing twice.
        var seen = Set<String>()
        var review = TxReview(wire: wire, accounts: accounts, tokenOwners: owners, unknownPrograms: unknown,
                              programNames: named.map { $0.known ? $0.name : "\($0.name) (not on ihatecrypto's list)" },
                              changes: deltas.map { "\($0.symbol): \(signed($0.amount))" }, checks: verdict.checks.filter { seen.insert($0).inserted },
                              problems: problems)
        review.deltas = deltas
        review.programs = named
        return review
    }

    private func signed(_ d: Decimal) -> String { (d > 0 ? "+" : d < 0 ? "−" : "") + Fmt.num(abs(d), maxFraction: 9) }

    private func symbol(_ mint: String) -> String {
        if mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" { return "USDC" }
        if let a = SpotAsset.all.first(where: { $0.mint == mint }) { return a.symbol }
        return "token \(mint.prefix(6))…"
    }

    /// Our token accounts, classic SPL and Token-2022 alike (the guard lets both run, so a drain through either shows):
    /// address → mint, raw amount, decimals.
    private func tokenAccounts(_ owner: String) async throws -> [String: (mint: String, amount: UInt64, decimals: Int)] {
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

    /// WebKit's side: page messages in, answers out as the promise's value or error.
    final class Bridge: NSObject, WKScriptMessageHandlerWithReply, WKNavigationDelegate {
        weak var browser: Browser?

        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) async -> (Any?, String?) {
            guard let browser, message.frameInfo.isMainFrame, let body = message.body as? [String: Any], let method = body["method"] as? String else {
                return (nil, "Request not accepted.")
            }
            let o = message.frameInfo.securityOrigin
            let origin = o.host.isEmpty ? "\(o.protocol)://" : "\(o.protocol)://\(o.host)\(o.port > 0 ? ":\(o.port)" : "")"
            do {
                return (try await browser.handle(method, body["params"] as? [String: Any] ?? [:], origin: origin, host: o.host), nil)
            } catch {
                return (nil, error.localizedDescription)
            }
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { browser?.loadError = nil; browser?.pageChanged() }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed(error) }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed(error) }

        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            browser?.loadError = "The page process crashed; press Reload."
            FileHandle.standardError.write(Data("ihatecrypto browser: web content process terminated\n".utf8))
        }

        private func failed(_ error: Error) {
            let e = error as NSError
            // Cancelled loads (a redirect replaced them) are not failures.
            guard e.code != NSURLErrorCancelled else { return }
            browser?.loadError = "The page didn't load: \(e.localizedDescription) (\(e.domain) \(e.code))"
            FileHandle.standardError.write(Data("ihatecrypto browser: \(browser?.loadError ?? "")\n".utf8))
        }
        func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) { browser?.pageChanged() }

        /// A page on MetaMask's phishing list never loads: not by a link, a redirect or a new window.
        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard navigationAction.targetFrame?.isMainFrame != false, let host = navigationAction.request.url?.host(),
                  PhishingList.shared.contains(host) else { return .allow }
            browser?.blocked = host
            FileHandle.standardError.write(Data("ihatecrypto browser: blocked \(host) (phishing list)\n".utf8))
            return .cancel
        }
    }

    func attachDelegate() { webView.navigationDelegate = bridge }
}

struct SiteError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

// MARK: - Window

struct BrowserView: View {
    @Environment(Workspace.self) private var ws
    @State private var typed = ""
    /// Its own window; false inside the panel on the canvas (WebPanel.swift).
    var window = true

    var body: some View {
        let browser = ws.browser
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button { browser.webView.goBack() } label: { Image(systemName: "chevron.left") }.buttonStyle(.hoverIcon)
                Button { browser.webView.reload() } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.hoverIcon)
                siteBadge(SiteCheck.check(host: browser.host))
                TextField("Site address", text: $typed)
                    .textFieldStyle(.plain)
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .background(RoundedRectangle(cornerRadius: 8).fill(.fill.tertiary))
                    .onSubmit { browser.open(typed) }
                if browser.connectedHere {
                    Button { browser.disconnectHere() } label: { Label("Connected · Disconnect", systemImage: "link") }
                        .buttonStyle(.plain).font(.ui(11, .semibold)).foregroundStyle(Brand.profit)
                        .help("The site sees your wallet's address this session. Disconnect takes it away and reloads the page.")
                }
                Text(ws.signerWallet.map { "wallet \($0.solana.prefix(4))…\($0.solana.suffix(4))" } ?? "no key")
                    .font(.ui(10)).foregroundStyle(.secondary)
            }
            .padding(10)
            if let host = browser.blocked {
                Label("\(host) is on MetaMask's list of phishing sites, so ihatecrypto didn't open it and offers it no wallet. If you typed it, check the address letter by letter.",
                      systemImage: "hand.raised.fill")
                    .font(.ui(12, .semibold)).foregroundStyle(Brand.blockedInk)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(10)
                    .background(Brand.blockedTint)
            }
            if let error = browser.loadError {
                Text(error).font(.ui(10)).foregroundStyle(.red).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12).padding(.bottom, 6)
            }
            WebViewHost(webView: browser.webView)
        }
        .frame(minWidth: window ? 900 : nil, minHeight: window ? 700 : nil)
        .onAppear {
            browser.workspace = ws
            browser.attachDelegate()
            if browser.webView.url == nil { browser.open(UserDefaults.standard.string(forKey: "openBrowser") ?? browser.address) }
            typed = browser.address
        }
        .onChange(of: browser.address) { typed = browser.address }
        .sheet(item: Binding(get: { browser.pending }, set: { if $0 == nil, let shown = browser.pending { browser.answer(false, to: shown.id) } })) { request in
            ApprovalSheet(request: request) { browser.answer($0, to: request.id) }
        }
    }

    @ViewBuilder private func siteBadge(_ verdict: SiteCheck.Verdict) -> some View {
        switch verdict {
        case .known: Image(systemName: "checkmark.seal.fill").foregroundStyle(.green).help("Known site")
        case .lookalike(let real): Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red).help("Looks like \(real), but it's a different address")
        case .unknown: Image(systemName: "questionmark.circle").foregroundStyle(.orange).help("Unfamiliar site: check the address")
        case .local: Image(systemName: "doc").foregroundStyle(.secondary)
        }
    }
}

struct WebViewHost: NSViewRepresentable {
    let webView: WKWebView
    func makeNSView(context: Context) -> WKWebView { webView }
    func updateNSView(_ nsView: WKWebView, context: Context) {}
}

/// What the site asks for, in words, with the simulated effect of a transaction. Refusals cannot be overridden.
/// A transaction reads top to bottom: what leaves and arrives (with dollars), what it calls, what was checked, and only
/// then what still needs the user's word — the programs not on ihatecrypto's list, a second check's warning.
struct ApprovalSheet: View {
    @Environment(Workspace.self) private var ws
    let request: SiteRequest
    let decide: (Bool) -> Void
    @State private var allowUnknown = false
    @State private var trustLookalike = false
    @State private var acknowledgeJev = false
    @State private var showChecks = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            if case .lookalike(let real) = request.site {
                banner("The address looks like \(real), but it's a different site — a common phishing trick.", "exclamationmark.octagon.fill",
                       ink: Brand.blockedInk, tint: Brand.blockedTint)
                Toggle("I understand this isn't \(real)", isOn: $trustLookalike)
            }
            switch request.kind {
            case .connect:
                grants([("eye", "Sees your Solana address"), ("lock.shield", "Can't get anything signed without a separate confirmation")])
            case .connectEVM:
                grants([("eye", "Sees your EVM address, which is also your Hyperliquid address"),
                        ("hand.raised", "ihatecrypto never signs token spending permissions (Permit)")])
            case .evmTransaction(let r):
                card(title: "What it does") { ForEach(r.lines, id: \.self) { line($0) } }
                card(title: "Your balances (simulated)") { ForEach(r.changes, id: \.self) { line($0) } }
                secondCheck(r.jev, nil)
                if !r.problems.isEmpty {
                    refused(r.problems)
                } else if r.recipients == [r.to.lowercased()], case .contract = EVMCall.decode(r.data) {
                    ask("ihatecrypto can't read what this contract does",
                        "\(r.to.prefix(10))…\(r.to.suffix(6)) — check it on the site. The signer asks for \(Presence.name) once for a contract it hasn't seen.",
                        toggle: "Allow this contract", isOn: $allowUnknown)
                } else if !r.recipients.isEmpty {
                    ask("Money, or the right to spend it, goes to an address that isn't yours",
                        r.recipients.map { "\($0.prefix(10))…\($0.suffix(6))" }.joined(separator: ", ") + " — check it on the site. The signer asks for \(Presence.name) for a new one.",
                        toggle: "Send to this address", isOn: $allowUnknown)
                }
                note("Simulated on the latest block, before the network fee. A token names itself — check its address. The signer decodes the call again.")
            case .typedData(let t):
                Text("\(t.primaryType) for “\(t.domainName)”, chain \(t.chainId)").font(.ui(12, .semibold))
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(t.fields.indices, id: \.self) { i in
                            Text("\(t.fields[i].name): \(t.fields[i].value)").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }.frame(maxHeight: 160)
                note("The signer computes the hash to sign from this same data. It rejects nested structures and token permissions.")
            case .message(let text):
                Text("Text to sign").font(.ui(10, .semibold)).foregroundStyle(.secondary)
                ScrollView { Text(text).font(.system(.callout, design: .monospaced)).frame(maxWidth: .infinity, alignment: .leading) }.frame(maxHeight: 140)
                note("Signing text doesn't move money. The signer rejects the “text” if it's a hidden transaction.")
            case .transaction(let r, let send):
                if let lead = summary(r) { Text(lead).font(.ui(16, .semibold)).fixedSize(horizontal: false, vertical: true) }
                if !r.deltas.isEmpty { card(title: "Your balances (simulated)") { ForEach(r.deltas.indices, id: \.self) { delta(r.deltas[$0], in: r) } } }
                calls(r.programs)
                checks(r.checks.filter { !$0.hasPrefix("Calls unknown program") })
                secondCheck(r.jev, r.jevNote)
                if !r.problems.isEmpty {
                    refused(r.problems)
                } else if !r.unknownPrograms.isEmpty {
                    let names = r.programs.filter { !$0.known }.map(\.name)
                    ask("\(list(names)) \(names.count == 1 ? "isn't" : "aren't") on ihatecrypto's list",
                        "Signing calls \(names.count == 1 ? "it" : "them") the way the site built the transaction; the balances above are the simulated result.",
                        toggle: "Allow \(names.count == 1 ? "it" : "them") for this one transaction", isOn: $allowUnknown)
                }
                note(send ? "After signing, ihatecrypto sends the transaction to the network." : "The site gets the signed transaction and sends it itself.")
            }
            HStack(spacing: 10) {
                Spacer()
                Button { decide(false) } label: { pill("Decline", ink: .primary, fill: Color.primary.opacity(0.07)) }
                    .keyboardShortcut(.cancelAction)
                Button { decide(true) } label: { pill(approveTitle, ink: Brand.onAccent, fill: Brand.accent) }
                    .keyboardShortcut(.defaultAction).disabled(!canApprove).opacity(canApprove ? 1 : 0.4)
            }
            .buttonStyle(.plain).pointerStyle(.link)
        }
        .font(.ui(13))
        .padding(24)
        .frame(width: 520)
    }

    /// The site, how ihatecrypto knows it, and the network the request is for.
    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            Text(host.prefix(1).uppercased()).font(.ui(22, .heavy))
                .frame(width: 48, height: 48)
                .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(.fill.tertiary))
            VStack(alignment: .leading, spacing: 3) {
                Text(heading.uppercased()).font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    Text(host).font(.ui(20, .heavy)).lineLimit(1).truncationMode(.middle)
                    switch request.site {
                    case .known: chip("Known site", "checkmark.seal.fill", ink: Brand.profit, tint: Brand.profit.opacity(0.12))
                    case .unknown: chip("Unfamiliar site", "questionmark.circle.fill", ink: Brand.jevInk, tint: Brand.jevTint)
                    case .lookalike: chip("Lookalike", "exclamationmark.octagon.fill", ink: Brand.blockedInk, tint: Brand.blockedTint)
                    case .local: EmptyView()
                    }
                }
                // The whole origin too: a phishing copy differs in what the big line leaves out.
                Text(request.origin).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
            }
            Spacer(minLength: 8)
            if let network {
                HStack(spacing: 5) {
                    TokenLogo(name: network.logo, size: 14)
                    Text(network.name).font(.ui(11, .semibold))
                }
                .padding(.horizontal, 9).padding(.vertical, 5)
                .background(Capsule().fill(.fill.tertiary))
            }
        }
    }

    private var network: (name: String, logo: String)? {
        switch request.kind {
        case .connect, .message, .transaction: ("Solana", "sol")
        case .evmTransaction(let r): (["1": "Ethereum", "42161": "Arbitrum", "8453": "Base"]["\(r.chain)"] ?? "chain \(r.chain)", "eth")
        case .typedData, .connectEVM: nil
        }
    }

    /// «You send 5 USDC to Kamino Vault»: what leaves, what arrives, and the program it's for — fee-sized SOL left out.
    private func summary(_ r: TxReview) -> String? {
        let moved = r.deltas.filter { !feeLike($0, in: r) }
        let out = moved.filter { $0.amount < 0 }.map { "\(Fmt.num(-$0.amount, maxFraction: 6)) \($0.symbol)" }
        let into = moved.filter { $0.amount > 0 }.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol)" }
        let infra: Set<String> = ["System", "Compute Budget", "SPL Token", "Associated Token", "Address Lookup Table"]
        // Programs of one protocol («Kamino Vault», «Kamino Farms») are that protocol: the money goes to Kamino.
        let work = r.programs.map(\.name).filter { !infra.contains($0) && !$0.contains("…") }
        let brands = Set(work.map { String($0.split(separator: " ").first ?? Substring($0)) })
        let target = work.count > 1 && brands.count == 1 ? brands.first : work.first
        let to = target.map { " to \($0)" } ?? ""
        switch (out.isEmpty, into.isEmpty) {
        case (false, true): return "You send \(list(out))\(to)"
        case (false, false): return "You send \(list(out)) and get \(list(into))"
        case (true, false): return "You get \(list(into))"
        case (true, true): return r.deltas.isEmpty ? nil : "Only the network fee"
        }
    }

    /// SOL that only pays the fee and rent beside a real move: shown with that note, not as a send.
    private func feeLike(_ d: (symbol: String, mint: String, amount: Decimal), in r: TxReview) -> Bool {
        d.symbol == "SOL" && d.amount < 0 && -d.amount < Decimal(string: "0.05")! && r.deltas.count > 1
    }

    private func delta(_ d: (symbol: String, mint: String, amount: Decimal), in r: TxReview) -> some View {
        let price: Decimal? = Stablecoins.isDollar(d.symbol) ? 1
            : d.symbol == "SOL" ? ws.markets["SOL"]?.markPx : ws.holdings.first { $0.contract == d.mint }?.price
        return HStack(spacing: 10) {
            TokenLogo(name: d.symbol.lowercased(), size: 26)
            VStack(alignment: .leading, spacing: 1) {
                Text(d.symbol).font(.ui(13, .semibold))
                if feeLike(d, in: r) { Text("network fee and rent for new accounts").font(.ui(10)).foregroundStyle(.secondary) }
            }
            Spacer(minLength: 8)
            VStack(alignment: .trailing, spacing: 1) {
                Text((d.amount > 0 ? "+" : "−") + Fmt.num(abs(d.amount), maxFraction: 6)).font(.ui(14, .bold)).monospacedDigit()
                    .foregroundStyle(d.amount > 0 ? Brand.profit : feeLike(d, in: r) ? Color.primary : Brand.loss)
                if let price { Text("≈ \(Fmt.usd(abs(d.amount) * price))").font(.ui(10)).foregroundStyle(.secondary).monospacedDigit() }
            }
        }
        .padding(.vertical, 2)
    }

    /// The programs it calls: the ones that do the work by name (amber when not on ihatecrypto's list), Solana's own
    /// plumbing on one quiet line.
    private func calls(_ programs: [(name: String, known: Bool)]) -> some View {
        let infra: Set<String> = ["System", "Compute Budget", "SPL Token", "Associated Token", "Address Lookup Table"]
        let work = programs.filter { !infra.contains($0.name) }
        let plumbing = programs.filter { infra.contains($0.name) }.map(\.name)
        return card(title: "It calls") {
            ForEach(work.indices, id: \.self) { i in
                HStack(spacing: 8) {
                    Image(systemName: work[i].known ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                        .foregroundStyle(work[i].known ? Brand.profit : Brand.jevInk).font(.system(size: 12))
                    Text(work[i].name).font(.ui(13, .semibold))
                    Spacer(minLength: 8)
                    Text(work[i].known ? "on ihatecrypto's list" : "not on ihatecrypto's list").font(.ui(10)).foregroundStyle(.secondary)
                }
            }
            if !plumbing.isEmpty {
                Text("and Solana's own: " + plumbing.joined(separator: ", ")).font(.ui(11)).foregroundStyle(.secondary)
            }
        }
    }

    /// What the code checked, folded to one line until asked.
    @ViewBuilder private func checks(_ items: [String]) -> some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Button { withAnimation(.easeOut(duration: 0.15)) { showChecks.toggle() } } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "checkmark.shield.fill").foregroundStyle(Brand.profit)
                        Text("\(items.count) \(items.count == 1 ? "check" : "checks") passed").font(.ui(12, .semibold))
                        Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                            .rotationEffect(.degrees(showChecks ? 90 : 0))
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain).pointerStyle(.link)
                if showChecks {
                    ForEach(items, id: \.self) { Text("· " + $0).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                        .padding(.leading, 22)
                }
            }
        }
    }

    /// The second check's word on the site and the transaction: a quiet line when it has no concern, a warning that
    /// needs a tick when it has.
    @ViewBuilder private func secondCheck(_ jev: JevReview?, _ note: String?) -> some View {
        if let jev, jev.concern {
            ask("The second check has a concern", jev.text, toggle: "I understand the second check's warning", isOn: $acknowledgeJev)
        } else if let text = jev?.text ?? note {
            Label { Text("Second check: " + text) } icon: { Image(systemName: jev == nil ? "info.circle" : "checkmark.seal") }
                .font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// What still needs the user's word: amber, with the one tick that lets Sign through.
    private func ask(_ title: String, _ detail: String, toggle: String, isOn: Binding<Bool>) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(title, systemImage: "exclamationmark.triangle.fill").font(.ui(13, .semibold)).foregroundStyle(Brand.jevInk)
            Text(detail).font(.ui(12)).foregroundStyle(Brand.jevInk.opacity(0.85)).fixedSize(horizontal: false, vertical: true)
            Toggle(toggle, isOn: isOn).font(.ui(12, .semibold))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Brand.jevTint))
    }

    private func refused(_ problems: [String]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Refused — it can't be signed", systemImage: "xmark.octagon.fill").font(.ui(13, .semibold))
            ForEach(problems, id: \.self) { Text("· " + $0).font(.ui(12)).fixedSize(horizontal: false, vertical: true) }
        }
        .foregroundStyle(Brand.blockedInk)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Brand.blockedTint))
    }

    private func card(title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased()).font(.ui(10, .semibold)).tracking(1).foregroundStyle(.secondary)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Brand.cardBorder))
    }

    private func line(_ text: String) -> some View { Text(text).fixedSize(horizontal: false, vertical: true) }
    private func note(_ text: String) -> some View { Text(text).font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
    private func list(_ items: [String]) -> String { items.count < 2 ? items.joined() : items.dropLast().joined(separator: ", ") + " and " + items.last! }

    /// The domain without «www.», the part a phishing copy changes.
    private var host: String {
        let host = URL(string: request.origin)?.host ?? request.origin
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }

    private func chip(_ text: String, _ symbol: String, ink: Color, tint: Color) -> some View {
        Label(text, systemImage: symbol).font(.ui(11, .semibold)).foregroundStyle(ink)
            .padding(.horizontal, 8).padding(.vertical, 4)
            .background(Capsule().fill(tint))
            .fixedSize()
    }

    private func banner(_ text: String, _ symbol: String, ink: Color, tint: Color) -> some View {
        Label(text, systemImage: symbol).font(.ui(12, .semibold)).foregroundStyle(ink)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12).padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(tint))
    }

    /// What connecting gives the site, one row each.
    private func grants(_ rows: [(symbol: String, text: String)]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(rows.indices, id: \.self) { i in
                if i > 0 { Divider().padding(.leading, 44) }
                HStack(spacing: 12) {
                    Image(systemName: rows[i].symbol).font(.system(size: 14, weight: .semibold)).foregroundStyle(.secondary).frame(width: 20)
                    Text(rows[i].text).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 12).padding(.vertical, 11)
            }
        }
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Brand.cardBorder))
    }

    private func pill(_ title: String, ink: Color, fill: Color) -> some View {
        Text(title).font(.ui(13, .semibold)).foregroundStyle(ink)
            .padding(.horizontal, 20).padding(.vertical, 9)
            .background(Capsule().fill(fill))
            .contentShape(Capsule())
    }

    private var heading: String {
        switch request.kind {
        case .connect, .connectEVM: "Connect site"
        case .message: "Sign text"
        case .typedData: "Sign data"
        case .evmTransaction: "Sign EVM transaction"
        case .transaction: "Sign transaction"
        }
    }

    private var approveTitle: String {
        if case .connect = request.kind { return "Connect" }
        if case .connectEVM = request.kind { return "Connect" }
        return "Sign"
    }

    private var canApprove: Bool {
        if case .lookalike = request.site, !trustLookalike { return false }
        if case .transaction(let r, _) = request.kind {
            return r.problems.isEmpty && (r.unknownPrograms.isEmpty || allowUnknown) && (r.jev?.concern != true || acknowledgeJev)
        }
        if case .evmTransaction(let r) = request.kind {
            return r.problems.isEmpty && (r.recipients.isEmpty || allowUnknown) && (r.jev?.concern != true || acknowledgeJev)
        }
        return true
    }
}
