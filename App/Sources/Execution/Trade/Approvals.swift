import EVMChain
import Foundation
import IhatecryptoCore
import WalletCore

/// The approvals this wallet has given on its EVM networks — who may still spend which token — and their revoking.
/// Found from the wallet's own transactions on each network's Blockscout (approve calls, Permit2's too), then read from
/// the chain as they stand now: an approve spent by a swap is zero again, an old one a site asked for may not be.
public struct TokenApproval: Sendable, Identifiable, Equatable {
    public var id: String { "\(chain):\(token):\(spender):\(permit2)" }
    public let chain: String
    public let token: String
    public let symbol: String
    public let decimals: Int
    public let spender: String
    /// Uniswap's router, Relay, Aave… — nil for a contract ihatecrypto doesn't know.
    public let spenderName: String?
    /// The allowance in the token's smallest units.
    public let amount: Decimal
    /// Given through Permit2 (its own allowance, with an expiry) rather than to the spender itself.
    public let permit2: Bool
    /// ≥ 2⁹⁶ units: the drainers' «unlimited».
    public var unlimited: Bool { amount >= Decimal(sign: .plus, exponent: 28, significand: 7.9) }
}

extension Executor {
    /// The networks approvals are read on: those with a Blockscout ihatecrypto reads.
    public static let approvalNetworks = ["base", "arbitrum", "ethereum", "polygon", "robinhood"]

    /// Who a spender is, when ihatecrypto knows it.
    public static func spenderName(_ spender: String, chainId: UInt64) -> String? {
        let s = spender.lowercased()
        if s == UniswapV4.permit2 { return "Permit2 (Uniswap)" }
        if s == UniswapV3.on(chainId)?.router || (chainId == 4663 && s == UniswapV3.robinhoodRouter) { return "Uniswap v3 router" }
        if s == UniswapV2.on(chainId)?.router { return "Uniswap v2 router" }
        if s == UniswapV4.on(chainId)?.router { return "Uniswap's Universal Router" }
        if s == "0x4cd00e387622c35bddb9b4c962c136462338bc31" || Permit3009.relayProxies.contains(s) { return "Relay" }
        if AaveV3.markets.contains(where: { $0.rpc.chainId == chainId && $0.pool.lowercased() == s }) { return "Aave pool" }
        if MorphoVault.vaults.contains(where: { $0.rpc.chainId == chainId && $0.address.lowercased() == s }) { return "Morpho vault" }
        return nil
    }

    /// Every approval this wallet still has on its networks, the unlimited ones first, then by network. A network whose
    /// Blockscout doesn't answer is left out (said by the caller as «couldn't read»).
    public func approvals() async -> (list: [TokenApproval], unread: [String]) {
        await withTaskGroup(of: (String, [TokenApproval]?).self) { group in
            for chain in Self.approvalNetworks { group.addTask { (chain, await self.approvals(on: chain)) } }
            var list: [TokenApproval] = [], unread: [String] = []
            for await (chain, found) in group { if let found { list += found } else { unread.append(chain) } }
            return (list.sorted { ($0.unlimited ? 0 : 1, $0.chain, $0.symbol) < ($1.unlimited ? 0 : 1, $1.chain, $1.symbol) }, unread.sorted())
        }
    }

    /// One network's: the (token, spender) pairs the wallet's own transactions approved — a token's approve or
    /// increaseAllowance, Permit2's approve, each also inside a 7702 batch — each read now; nil when Blockscout didn't
    /// answer. Its v2 list, not the logs API: that one takes 10 calls an hour without a key (measured 2026-10-06).
    // ponytail: approvals given by a signature (EIP-2612 permit, Permit2's permit) leave no transaction of the wallet's
    // and aren't found; the signer refuses both. The newest 1000 transactions at most, and the allowances are read one
    // at a time (a wallet with a hundred approvals waits a minute or two; read them in parallel if that bites).
    func approvals(on chain: String) async -> [TokenApproval]? {
        guard let host = Holdings.blockscout[chain], let net = EVMNetwork.named(chain) else { return [] }
        let me = wallet.evm.lowercased()
        var pairs: Set<String> = []
        var next: [String: Any]?
        for page in 0..<20 {
            var url = URLComponents(string: "https://\(host)/api/v2/addresses/\(wallet.evm)/transactions")!
            url.queryItems = [URLQueryItem(name: "filter", value: "from")] + (next ?? [:]).map { URLQueryItem(name: $0.key, value: "\($0.value)") }
            guard page == 0 || next != nil, let u = url.url else { break }
            var r = URLRequest(url: u, timeoutInterval: 30)
            r.setValue(Holdings.browser["User-Agent"], forHTTPHeaderField: "User-Agent")
            var list: [String: Any]?
            for attempt in 0..<4 {
                guard let (data, response) = try? await URLSession.shared.data(for: r) else { break }
                let code = (response as? HTTPURLResponse)?.statusCode ?? 0
                if code == 429 || code >= 500, attempt < 3 { try? await Task.sleep(for: .seconds(2 << attempt)); continue }
                list = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
                break
            }
            guard let items = list?["items"] as? [[String: Any]] else { return nil }
            for t in items {
                guard let to = ((t["to"] as? [String: Any])?["hash"] as? String)?.lowercased(), let input = (t["raw_input"] as? String).flatMap({ Data(hex: $0) }) else { continue }
                let calls = to == me ? Batch7702.calls(in: input) ?? [] : [Batch7702.Call(to: to, data: input)]
                for c in calls {
                    switch EVMCall.decode(c.data) {
                    case .approve(let spender, _), .increaseAllowance(let spender, _): pairs.insert("\(c.to.lowercased())|\(spender.lowercased())|0")
                    case .permit2Approve(let token, let spender, _, _) where c.to.lowercased() == UniswapV4.permit2: pairs.insert("\(token.lowercased())|\(spender.lowercased())|1")
                    default: break
                    }
                }
            }
            next = list?["next_page_params"] as? [String: Any]
        }
        var out: [TokenApproval] = []
        var tokenInfo: [String: (String, Int)] = [:]
        for pair in pairs {
            let p = pair.split(separator: "|").map(String.init)
            let (token, spender, permit2) = (p[0], p[1], p[2] == "1")
            let answer: Data
            do {
                answer = permit2 ? try await net.rpc.read(UniswapV4.permit2, try UniswapV4.permit2Allowance(owner: wallet.evm, token: token, spender: spender))
                                 : try await net.rpc.read(token, try ERC20.allowance(owner: wallet.evm, spender: spender))
            } catch {
                // A revert is an answer — no ERC-20 allowance there (an NFT's approve shares the selector). Any other
                // failure isn't a zero: the network is said unread rather than shown clean.
                if "\(error)".lowercased().contains("revert") { continue }
                return nil
            }
            // Permit2's allowance counts until it expires.
            let expired = permit2 && (ABI.uint(in: answer, word: 1) ?? 0) <= UInt64(Date().timeIntervalSince1970)
            guard !expired, let amount = ABI.decimal(in: answer, word: 0), amount > 0 else { continue }
            if tokenInfo[token] == nil {
                let symbol = (try? await net.rpc.read(token, Data([0x95, 0xd8, 0x9b, 0x41]))).flatMap { ABI.string($0) } ?? "\(token.prefix(6))…\(token.suffix(4))"
                let decimals = (try? await net.rpc.read(token, Data([0x31, 0x3c, 0xe5, 0x67]))).flatMap { ABI.uint(in: $0, word: 0) }.map(Int.init) ?? 18
                tokenInfo[token] = (Holdings.bridgedUSDC[chain] == token ? "USDC.e" : symbol, min(decimals, 36))
            }
            let info = tokenInfo[token]!
            out.append(TokenApproval(chain: chain, token: token, symbol: info.0, decimals: info.1, spender: spender,
                                     spenderName: Self.spenderName(spender, chainId: net.rpc.chainId), amount: amount, permit2: permit2))
        }
        return out
    }

    /// Sets an approval to zero: the token's approve(spender, 0), or Permit2's own approve(token, spender, 0, 0). Nothing
    /// is spent and nobody gains a right, so the signer asks no recipient for it.
    public func revokeApproval(_ net: EVMNetwork, token: String, spender: String, permit2: Bool) async throws -> String {
        let data = permit2 ? try UniswapV4.permit2Approve(token: token, spender: spender, amount: 0, expiration: 0) : try ERC20.approve(spender: spender, units: 0)
        let hash = try await sendEVM(net.rpc, to: permit2 ? UniswapV4.permit2 : token, data: data,
                                     note: "Revoke \(spender.prefix(6))…\(spender.suffix(4))'s \(permit2 ? "Permit2 " : "")approval of \(token.prefix(6))…\(token.suffix(4)) on \(net.name)")
        return "Revoked: \(Self.spenderName(spender, chainId: net.rpc.chainId) ?? spender) can no longer spend that token on \(net.name)\(permit2 ? " through Permit2" : ""). \(net.explorer)/tx/\(hash)"
    }
}
