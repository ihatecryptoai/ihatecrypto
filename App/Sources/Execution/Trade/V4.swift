import EVMChain
import Foundation
import IhatecryptoCore
import RelayBridge

/// Uniswap v4 on Robinhood Chain, for the stock and token buys and sales (Stocks.swift): the pool without hooks for a
/// pair, its quote, and the swap — the input to Permit2 (exact), Permit2 to the Universal Router (exact, for half an hour),
/// then one V4_SWAP that pays this wallet (the signer reads it back field by field: UniswapV4.refusal).
/// Every way a trade on Robinhood Chain can go, quoted for one amount: Uniswap v4's pool without hooks, v3's best pool,
/// Relay's swap (only for a token DexScreener lists: its checks need that price).
struct StockWays {
    var v4: Executor.V4Route?
    var v3: (fee: UInt64, out: Decimal)?
    var relay: Decimal?
    /// What each gives, the most first: «v4», «v3», «relay».
    var ranked: [(name: String, out: Decimal)] {
        [("v4", v4?.out), ("v3", v3?.out), ("relay", relay)].compactMap { name, out in out.map { (name, $0) } }.filter { $0.1 > 0 }.sorted { $0.1 > $1.1 }
    }
    static func title(_ way: String) -> String { way == "v4" ? "Uniswap v4" : way == "v3" ? "Uniswap v3" : "Relay's swap" }
}

extension Executor {
    typealias V4Route = (key: UniswapV4.PoolKey, zeroForOne: Bool, out: Decimal)

    /// All three ways for `units` of what goes in (USDG for a buy, the token for a sale), quoted at once.
    func stockWays(_ token: RobinhoodStocks.Token, buy: Bool, units: Decimal, listing: TokenLookup.Info?) async -> StockWays {
        let usdg = EVMRPC.robinhoodUSDG, me = wallet.evm
        let (tokenIn, tokenOut) = buy ? (usdg, token.address) : (token.address, usdg)
        async let v4 = v4Route(tokenIn, tokenOut, amountIn: units)
        async let v3 = try? bestPool(tokenIn, tokenOut, amountIn: units)
        async let relay: Decimal? = listing == nil ? nil : (try? await Relay.quote(user: me, recipient: me, originChain: Relay.Chain.robinhood,
                                                                                     destinationChain: Relay.Chain.robinhood, originCurrency: tokenIn,
                                                                                     destinationCurrency: tokenOut, amount: 0, units: "\(units)"))
            .flatMap { Decimal(string: $0.amountOut) }
        return StockWays(v4: await v4, v3: await v3 ?? nil, relay: await relay)
    }

    /// The ways for the card while it waits for Confirm, the best first, nothing signed: what each gives and its worth
    /// in dollars (a buy: the dollars in, scaled by what each gives against the best).
    public func stockQuotes(_ symbolOrContract: String, usdc: Decimal?, sell: Decimal?) async throws -> [(way: String, gets: String, worth: Decimal)] {
        let (token, listing) = try await RobinhoodStocks.resolve(symbolOrContract)
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        if let usdc {
            var units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
            let there = try await EVMRPC.robinhood.tokenBalance(EVMRPC.robinhoodUSDG, owner: wallet.evm)
            if there < units, there > 0, units - there <= max(1_000_000, units / 50) { units = there }
            let ranked = await stockWays(token, buy: true, units: Decimal(units), listing: listing).ranked
            guard let top = ranked.first?.out else { return [] }
            // Six places: a dollar of a $400 stock is 0.0025 of it, and two places showed «0 TSLA».
            return ranked.map { ($0.name, "\(Fmt.num($0.out / scale, maxFraction: 6)) \(token.symbol)", Decimal(units) / 1_000_000 * $0.out / top) }
        }
        let held = try await balance(token.address, on: EVMRPC.robinhood)
        let wanted = (sell.map { min($0 * scale, held) } ?? held).rounded(0, .down)
        guard wanted > 0 else { return [] }
        return await stockWays(token, buy: false, units: wanted, listing: listing).ranked.map { ($0.name, "\(Fmt.num($0.out / 1_000_000)) USDG", $0.out / 1_000_000) }
    }

    /// «Uniswap v4 ≈ 1,906 AI · Relay's swap ≈ 1,880 · Uniswap v3 ≈ 1,372»: what the report says before the best is taken.
    func waysLine(_ ways: StockWays, _ unit: (Decimal) -> String) -> String {
        ways.ranked.map { "\(StockWays.title($0.name)) ≈ \(unit($0.out))" }.joined(separator: " · ")
    }

    /// The v4 pool for tokenIn → tokenOut on `rpc`'s network and what it gives for `amountIn`; nil when there's none
    /// without hooks, or with hooks Uniswap's registry vouches for (UniswapV4.allowsHooks).
    /// Every such pool of the pair is quoted for this amount and the one that gives most is taken: the deepest isn't
    /// always the best price (AI/USDG on Robinhood has five).
    func v4Route(_ tokenIn: String, _ tokenOut: String, amountIn: Decimal, rpc: EVMRPC = .robinhood) async -> V4Route? {
        var best: V4Route?
        for key in await Self.v4Pools(tokenIn, tokenOut, chainId: rpc.chainId) {
            let zeroForOne = tokenIn.lowercased() == key.currency0
            if let out = await v4Quote(key, zeroForOne: zeroForOne, amount: amountIn, rpc: rpc), out > (best?.out ?? 0) { best = (key, zeroForOne, out) }
        }
        return best
    }

    /// V4Quoter on the chain itself.
    func v4Quote(_ key: UniswapV4.PoolKey, zeroForOne: Bool, amount: Decimal, rpc: EVMRPC = .robinhood) async -> Decimal? {
        guard let quoter = UniswapV4.on(rpc.chainId)?.quoter, let call = try? UniswapV4.quoteExactInputSingle(key, zeroForOne: zeroForOne, amount: amount),
              let answer = try? await rpc.read(quoter, call) else { return nil }
        return ABI.decimal(in: answer, word: 0)
    }

    /// The pair's v4 pools from DexScreener (a v4 pair's address is its pool id), the deepest first; the first whose key
    /// the PoolManager's Initialize event gives, hashes back to that id and has no hooks. One side is the network's dollar.
    /// token-pairs lists a token's pools (30 at most); tokens/v1 gave only its one biggest pair, so a v4 pool showed only
    /// when it was that (AI on Robinhood: 1 of 5 AI/USDG pools; Base's cbBTC/USDC v4 pools: none).
    static func v4Pool(_ a: String, _ b: String, chainId: UInt64 = 4663) async -> UniswapV4.PoolKey? {
        await v4Pools(a, b, chainId: chainId).first
    }

    /// The pair's v4 pools among its five deepest whose hooks may run (none, or vouched for: UniswapV4.allowsHooks), the
    /// deepest first.
    static func v4Pools(_ a: String, _ b: String, chainId: UInt64 = 4663) async -> [UniswapV4.PoolKey] {
        guard let net = EVMNetwork.of(chainId: chainId), UniswapV4.on(chainId) != nil else { return [] }
        let pair = Set([a.lowercased(), b.lowercased()])
        // DexScreener lists a token's pools: the side that's neither the dollar nor the network's coin (UniswapV4.native).
        let token = [a, b].first { ![net.usdc.lowercased(), UniswapV4.native].contains($0.lowercased()) } ?? a
        guard let url = URL(string: "https://api.dexscreener.com/token-pairs/v1/\(net.key)/\(token)"),
              let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: url, timeoutInterval: 15)),
              let pairs = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] else { return [] }
        func side(_ p: [String: Any], _ k: String) -> String? { ((p[k] as? [String: Any])?["address"] as? String)?.lowercased() }
        func depth(_ p: [String: Any]) -> Double { ((p["liquidity"] as? [String: Any])?["usd"] as? NSNumber)?.doubleValue ?? 0 }
        let ids = pairs.filter { p in
            p["dexId"] as? String == "uniswap" && (p["labels"] as? [String])?.contains("v4") == true
                && Set([side(p, "baseToken"), side(p, "quoteToken")].compactMap { $0 }) == pair
        }
        .sorted { depth($0) > depth($1) }.compactMap { ($0["pairAddress"] as? String)?.lowercased() }
        var keys: [UniswapV4.PoolKey] = []
        for id in ids.prefix(5) {
            if let key = await v4Key(id, chainId: chainId), key.id == id, UniswapV4.allowsHooks(key, chainId: chainId) { keys.append(key) }
        }
        return keys
    }

    /// A v4 pool's key: the PositionManager's poolKeys(bytes25 id) on the network's own node first (every pool that took
    /// liquidity through it is there), else the PoolManager's Initialize event on the network's Blockscout (it reads the
    /// whole history; public nodes cap a log query at a range of blocks), its 429 waited out twice, 2 s apart — Blockscout
    /// turned away every lookup for minutes after a few dozen. The caller checks the key hashes back to the id. A key
    /// never changes, so each is asked once a launch (V4Keys).
    static func v4Key(_ id: String, chainId: UInt64 = 4663) async -> UniswapV4.PoolKey? {
        if let known = await V4Keys.shared.key(chainId, id) { return known }
        guard let v4 = UniswapV4.on(chainId) else { return nil }
        if let rpc = EVMNetwork.of(chainId: chainId)?.rpc, let raw = Data(hex: String(id.dropFirst(2))), raw.count == 32,
           let answer = try? await rpc.read(v4.positionManager, Data([0x86, 0xb6, 0xbe, 0x7d]) + raw.prefix(25) + Data(count: 7)),
           let c0 = ABI.address(in: answer, word: 0), let c1 = ABI.address(in: answer, word: 1), c1 != "0x" + String(repeating: "0", count: 40),
           let fee = ABI.uint(in: answer, word: 2), fee < 1 << 24, let hooks = ABI.address(in: answer, word: 4) {
            let spacing = Int32(truncatingIfNeeded: answer.subdata(in: 120..<128).reduce(Int64(0)) { $0 << 8 | Int64($1) })
            let key = UniswapV4.PoolKey(currency0: c0, currency1: c1, fee: UInt32(fee), tickSpacing: spacing, hooks: hooks)
            if key.id == id.lowercased() {
                await V4Keys.shared.keep(chainId, id, key)
                return key
            }
        }
        guard !v4.blockscout.isEmpty else { return nil }
        var url = URLComponents(string: "https://\(v4.blockscout)/api")
        url?.queryItems = [("module", "logs"), ("action", "getLogs"), ("fromBlock", "0"), ("toBlock", "latest"),
                           ("address", v4.poolManager), ("topic0", UniswapV4.initializeTopic), ("topic1", id),
                           ("topic0_1_opr", "and")].map { URLQueryItem(name: $0.0, value: $0.1) }
        guard let link = url?.url else { return nil }
        var request = URLRequest(url: link, timeoutInterval: 20)
        request.setValue(Holdings.browser["User-Agent"], forHTTPHeaderField: "User-Agent")
        var answer: Data?
        for attempt in 0..<3 {
            guard let (data, response) = try? await URLSession.shared.data(for: request) else { return nil }
            if (response as? HTTPURLResponse)?.statusCode != 429 { answer = data; break }
            if attempt < 2 { try? await Task.sleep(for: .seconds(2)) }
        }
        guard let data = answer,
              let log = (((try? JSONSerialization.jsonObject(with: data)) as? [String: Any])?["result"] as? [[String: Any]])?.first,
              let topics = log["topics"] as? [String], topics.count == 4, let hex = log["data"] as? String,
              let words = Data(hex: String(hex.dropFirst(2))), words.count >= 96,
              let c0 = Data(hex: String(topics[2].dropFirst(2))).flatMap({ ABI.address(in: $0, word: 0) }),
              let c1 = Data(hex: String(topics[3].dropFirst(2))).flatMap({ ABI.address(in: $0, word: 0) }),
              let fee = ABI.uint(in: words, word: 0), fee < 1 << 24, let hooks = ABI.address(in: words, word: 2) else { return nil }
        let spacing = Int32(truncatingIfNeeded: words.subdata(in: 56..<64).reduce(Int64(0)) { $0 << 8 | Int64($1) })
        let key = UniswapV4.PoolKey(currency0: c0, currency1: c1, fee: UInt32(fee), tickSpacing: spacing, hooks: hooks)
        await V4Keys.shared.keep(chainId, id, key)
        return key
    }

    /// The swap itself; returns its transaction. `rpc`: the network's node (Robinhood Chain's by default), or a fork in a test.
    func v4Swap(_ route: V4Route, tokenIn: String, units: Decimal, minOut: Decimal, what: String, note: String,
                rpc: EVMRPC = .robinhood) async throws -> String {
        guard let router = UniswapV4.on(rpc.chainId)?.router else { throw ExecutionError("No Uniswap v4 on network \(rpc.chainId). Nothing was signed.") }
        let me = wallet.evm
        // Permit2's own allowance for the router: exactly this much, for half an hour, unless one that covers it is still on.
        func allowed() async throws -> (amount: Decimal, until: UInt64) {
            let answer = try await rpc.read(UniswapV4.permit2, try UniswapV4.permit2Allowance(owner: me, token: tokenIn, spender: router))
            return (ABI.decimal(in: answer, word: 0) ?? 0, ABI.uint(in: answer, word: 1) ?? 0)
        }
        let now = UInt64(Date().timeIntervalSince1970)
        let swap = try UniswapV4.execute(route.key, zeroForOne: route.zeroForOne, amountIn: units, amountOutMinimum: minOut, deadline: now + 600)
        // The network's own coin goes in as the call's value: no approve, nothing for Permit2 (the signer lets the coins go
        // only into this swap, exactly its input: EVMCall.returnsCoins).
        if tokenIn.lowercased() == UniswapV4.native {
            guard units > 0, units <= Decimal(UInt64.max) else { throw ExecutionError("At most 18 \(EVMRPC.coin(rpc.chainId)) at once. Nothing was signed.") }
            return try await sendEVM(rpc, to: router, data: swap, note: note, value: NSDecimalNumber(decimal: units).uint64Value)
        }
        // The three as one transaction where it batches: the approve to Permit2, Permit2's to the router, the swap.
        if canBatch(rpc) {
            var calls: [Batch7702.Call] = []
            let toPermit2 = ABI.decimal(in: try await rpc.read(tokenIn, try ERC20.allowance(owner: me, spender: UniswapV4.permit2)), word: 0) ?? 0
            if toPermit2 < units { calls.append(.init(to: tokenIn, data: try ERC20.approve(spender: UniswapV4.permit2, units: units))) }
            let current = try await allowed()
            if current.amount < units || current.until < now + 120 {
                calls.append(.init(to: UniswapV4.permit2, data: try UniswapV4.permit2Approve(token: tokenIn, spender: router, amount: units, expiration: now + 1800)))
            }
            guard calls.isEmpty else { return try await sendEVM(rpc, to: me, data: Data(), note: note, batch: calls + [.init(to: router, data: swap)]) }
            return try await sendEVM(rpc, to: router, data: swap, note: note)
        }
        try await approve(rpc, token: tokenIn, spender: UniswapV4.permit2, units: units, note: "Approve exactly \(what) to Permit2")
        let current = try await allowed()
        if current.amount < units || current.until < now + 120 {
            _ = try await sendEVM(rpc, to: UniswapV4.permit2,
                                  data: try UniswapV4.permit2Approve(token: tokenIn, spender: router, amount: units, expiration: now + 1800),
                                  note: "Permit2: let Uniswap's router take exactly \(what), for 30 minutes")
            var seen = false
            for _ in 0..<15 where !seen {
                let a = try await allowed()
                seen = a.amount >= units && a.until >= now + 120
                if !seen { try await Task.sleep(for: .seconds(1)) }
            }
            guard seen else { throw ExecutionError("Permit2's approval went through but the network doesn't show it yet. Try again in a minute; nothing was swapped.") }
        }
        return try await sendEVM(rpc, to: router, data: swap, note: note)
    }
}

/// Pool keys read from Blockscout this launch, by network and pool id: a key never changes.
actor V4Keys {
    static let shared = V4Keys()
    private var known: [String: UniswapV4.PoolKey] = [:]
    func key(_ chainId: UInt64, _ id: String) -> UniswapV4.PoolKey? { known["\(chainId):\(id.lowercased())"] }
    func keep(_ chainId: UInt64, _ id: String, _ key: UniswapV4.PoolKey) { known["\(chainId):\(id.lowercased())"] = key }
}
