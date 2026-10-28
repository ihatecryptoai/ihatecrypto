import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import RelayBridge
import TxGuard
import WalletCore

/// Tokens on Base and Arbitrum by ticker: Uniswap's default token list (tokens.uniswap.org), the curated list its own
/// app offers. A ticker outside it is bought by its contract.
public enum EVMTokens {
    public struct Token: Sendable, Equatable {
        public let symbol: String
        public let name: String
        public let address: String
        public let decimals: Int
    }

    /// HyperEVM's own: Uniswap's list has nothing for chain 999. Unit's bridged coins, Kinetiq's kHYPE, USD₮0 and WHYPE,
    /// each address, symbol and decimals read on-chain 2026-10-07.
    static let hyperevm = [Token(symbol: "UBTC", name: "Unit Bitcoin", address: "0x9fdbda0a5e284c32744d2f17ee5c74b284993463", decimals: 8),
                           Token(symbol: "UETH", name: "Unit Ethereum", address: "0xbe6727b535545c67d5caa73dea54865b92cf7907", decimals: 18),
                           Token(symbol: "USOL", name: "Unit Solana", address: "0x068f321fa8fb9f0d135f290ef6a3e2813e1c8a29", decimals: 9),
                           Token(symbol: "kHYPE", name: "Kinetiq Staked HYPE", address: "0xfd739d4e423301ce9385c1fb8850539d657c296d", decimals: 18),
                           Token(symbol: "USDT0", name: "USD₮0", address: "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", decimals: 6),
                           Token(symbol: "WHYPE", name: "Wrapped HYPE", address: "0x5555555555555555555555555555555555555555", decimals: 18)]

    /// BNB Chain's own beside Uniswap's list, which has neither CAKE nor BTCB nor Binance-Peg ETH: read on-chain 2026-10-08.
    static let bnb = [Token(symbol: "CAKE", name: "PancakeSwap Token", address: "0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82", decimals: 18),
                      Token(symbol: "BTCB", name: "Binance-Peg BTCB", address: "0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c", decimals: 18),
                      Token(symbol: "ETH", name: "Binance-Peg Ethereum", address: "0x2170ed0880ac9a755fd29b2688956bd959f933f8", decimals: 18)]

    public static func find(_ symbol: String, chainId: UInt64) async throws -> Token? {
        let want = symbol.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        // The big five stablecoins by ihatecrypto's own table: Uniswap's list misses USDe and USDS on most networks.
        if let key = EVMNetwork.all.first(where: { $0.rpc.chainId == chainId })?.key, let c = Stablecoins.find(want, on: key) {
            return Token(symbol: c.symbol, name: c.symbol, address: c.address, decimals: c.decimals)
        }
        if chainId == 999 { return hyperevm.first { $0.symbol.uppercased() == want } }
        if chainId == 56, let own = bnb.first(where: { $0.symbol.uppercased() == want }) { return own }
        let (data, _) = try await URLSession.shared.data(from: URL(string: "https://tokens.uniswap.org")!)
        return find(data, symbol: symbol, chainId: chainId)
    }

    static func find(_ data: Data, symbol: String, chainId: UInt64) -> Token? {
        let tokens = ((try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["tokens"] as? [[String: Any]]) ?? []
        let want = symbol.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        guard let t = tokens.first(where: { ($0["chainId"] as? NSNumber)?.uint64Value == chainId && ($0["symbol"] as? String)?.uppercased() == want }),
              let address = t["address"] as? String, let decimals = (t["decimals"] as? NSNumber)?.intValue else { return nil }
        return Token(symbol: t["symbol"] as? String ?? symbol, name: t["name"] as? String ?? symbol, address: address.lowercased(), decimals: decimals)
    }
}

/// Buying and selling tokens on Base and Arbitrum with the money on Solana. Two ways each time, and the one that
/// leaves more is taken: Relay swaps on the way in one step, or Relay bridges USDC and Uniswap swaps on the chain
/// (its gas, and a gas top-up when there's none, counted against it).
extension Executor {
    /// One way to do the trade: what comes out in the target's smallest units, and what it's worth after its gas, in USD.
    struct SwapWay {
        let name: String
        let out: Decimal
        let worth: Decimal
        let line: String
    }

    /// A ticker from Uniswap's list, or a contract that trades on the chain with $50k+ of liquidity; decimals are read
    /// from the contract itself.
    func evmToken(_ ref: String, on net: EVMNetwork) async throws -> EVMTokens.Token {
        let rpc = net.rpc
        if !ref.hasPrefix("0x") {
            let want = ref.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$"))
            // Robinhood Chain's dollar and its Stock Tokens by Robinhood's own registry.
            if rpc.chainId == 4663 {
                if want == "USDG" { return EVMTokens.Token(symbol: "USDG", name: "Global Dollar", address: EVMRPC.robinhoodUSDG, decimals: 6) }
                if let s = try? await RobinhoodStocks.verified(want, rpc: rpc) {
                    return EVMTokens.Token(symbol: s.symbol, name: s.name, address: s.address.lowercased(), decimals: s.decimals)
                }
            }
            guard let t = try await EVMTokens.find(ref, chainId: rpc.chainId) else {
                // Not on a list, but in the wallet there under that ticker: its contract, with the contract's own checks.
                if let held = await Holdings.evm(net.key, address: wallet.evm, native: false, skip: [])?
                    .first(where: { $0.symbol.uppercased() == want && $0.contract.hasPrefix("0x") }) {
                    return try await evmToken(held.contract, on: net)
                }
                throw ExecutionError("\(ref.uppercased()) isn't on \(rpc.chainId == 999 ? "ihatecrypto's HyperEVM list (UBTC, UETH, USOL, kHYPE, USDT0)" : "Uniswap's token list") for \(net.name). To buy it anyway, name it by its contract: “buy token 0x… on \(net.key) for 5 USDC”.")
            }
            guard ABI.uint(in: try await rpc.read(t.address, Data([0x31, 0x3c, 0xe5, 0x67])), word: 0) == UInt64(t.decimals) else {
                throw ExecutionError("The \(t.symbol) contract on \(net.name) doesn't match Uniswap's list. Nothing was signed.")
            }
            return t
        }
        guard let info = try await TokenLookup.lookup(ref).first(where: { $0.chain == net.key }) else {
            throw ExecutionError("This contract doesn't trade on \(net.name).")
        }
        guard let liquidity = info.liquidityUSD, liquidity >= 50_000 else {
            throw ExecutionError("\(info.symbol) has only \(Fmt.usd(info.liquidityUSD ?? 0)) of liquidity on \(net.name): too thin to trade safely.")
        }
        guard let decimals = ABI.uint(in: try await rpc.read(ref, Data([0x31, 0x3c, 0xe5, 0x67])), word: 0), decimals <= 36 else {
            throw ExecutionError("The contract doesn't answer decimals(): not a token ihatecrypto can trade.")
        }
        let symbol = ABI.string(try await rpc.read(ref, Data([0x95, 0xd8, 0x9b, 0x41]))) ?? info.symbol
        return EVMTokens.Token(symbol: symbol, name: info.name, address: ref.lowercased(), decimals: Int(decimals))
    }

    /// The Uniswap v3 route with the most output: one pool, or two through WETH.
    func bestPath(on rpc: EVMRPC, _ uni: UniswapV3.Deployment, from a: String, to b: String, amountIn: Decimal) async -> (path: Data, out: Decimal, hops: Int)? {
        var routes = UniswapV3.fees.map { ([a, b], [$0]) }
        if a.lowercased() != uni.weth, b.lowercased() != uni.weth {
            for f1: UInt64 in [500, 3000, 10000] { for f2: UInt64 in [500, 3000, 10000] { routes.append(([a, uni.weth, b], [f1, f2])) } }
        }
        return await withTaskGroup(of: (Data, Decimal, Int)?.self) { group in
            for (tokens, fees) in routes {
                group.addTask {
                    guard let path = try? UniswapV3.path(tokens, fees: fees),
                          let answer = try? await rpc.read(uni.quoter, try UniswapV3.quoteExactInput(path: path, amountIn: amountIn)),
                          let out = ABI.decimal(in: answer, word: 0), out > 0 else { return nil }
                    return (path, out, fees.count)
                }
            }
            var best: (path: Data, out: Decimal, hops: Int)?
            for await r in group { if let r, r.1 > (best?.out ?? 0) { best = (r.0, r.1, r.2) } }
            return best
        }
    }

    /// Uniswap v2's best path: its pool of the pair, or two through WETH, quoted by the router (before any transfer tax).
    func bestV2(on rpc: EVMRPC, from a: String, to b: String, amountIn: Decimal) async -> (path: [String], out: Decimal)? {
        guard let v2 = UniswapV2.on(rpc.chainId) else { return nil }
        var paths = [[a, b]]
        if a.lowercased() != v2.weth, b.lowercased() != v2.weth { paths.append([a, v2.weth, b]) }
        return await withTaskGroup(of: ([String], Decimal)?.self) { group in
            for path in paths {
                group.addTask {
                    guard let answer = try? await rpc.read(v2.router, try UniswapV2.getAmountsOut(amountIn, path: path)),
                          let out = UniswapV2.amountOut(answer), out > 0 else { return nil }
                    return (path, out)
                }
            }
            var best: (path: [String], out: Decimal)?
            for await r in group { if let r, r.1 > (best?.out ?? 0) { best = (r.0, r.1) } }
            return best
        }
    }

    /// The best of Uniswap's v3 path (one pool or two through WETH), its v4 pool without hooks (UniswapV4.on) and its v2
    /// path (UniswapV2.on), for `amountIn` of `a` into `b` on the network: all quoted at once, the one that gives most taken.
    func bestUniswap(on net: EVMNetwork, _ uni: UniswapV3.Deployment, from a: String, to b: String, amountIn: Decimal) async -> UniRoute? {
        async let v3 = bestPath(on: net.rpc, uni, from: a, to: b, amountIn: amountIn)
        async let v4 = UniswapV4.on(net.rpc.chainId) == nil ? nil : v4Route(a, b, amountIn: amountIn, rpc: net.rpc)
        async let v2 = bestV2(on: net.rpc, from: a, to: b, amountIn: amountIn)
        let (p3, p4, p2) = await (v3, v4, v2)
        var route: UniRoute
        let top = max(p3?.out ?? 0, p4?.out ?? 0, p2?.out ?? 0)
        if top <= 0 { return nil }
        if let p4, p4.out == top { route = UniRoute(v3: nil, v4: p4, out: p4.out) }
        else if let p3, p3.out == top { route = UniRoute(v3: (p3.path, p3.hops), v4: nil, out: p3.out) }
        else if let p2 { route = UniRoute(v3: nil, v4: nil, v2: p2.path, out: p2.out) }
        else { return nil }
        // Price impact: the trade's own price against a twentieth of it on the same route.
        let small = (amountIn / 20).whole
        if small > 0 {
            let smallOut: Decimal? = if let v4 = route.v4 {
                await v4Quote(v4.key, zeroForOne: v4.zeroForOne, amount: small, rpc: net.rpc)
            } else if let v3 = route.v3, let answer = try? await net.rpc.read(uni.quoter, try UniswapV3.quoteExactInput(path: v3.path, amountIn: small)) {
                ABI.decimal(in: answer, word: 0)
            } else if let path = route.v2, let v2 = UniswapV2.on(net.rpc.chainId),
                      let answer = try? await net.rpc.read(v2.router, try UniswapV2.getAmountsOut(small, path: path)) {
                UniswapV2.amountOut(answer)
            } else { nil }
            if let smallOut, smallOut > 0 { route.impact = max(0, 1 - (route.out / amountIn) / (smallOut / small)) }
        }
        return route
    }

    /// The swap by `route` for `units` of `tokenIn`, for at least what it quoted less the route's tolerance (UniRoute:
    /// 0.5% to 3% by its price impact), paying only this wallet: v3's exactInput or v2's router with its exact approve, or
    /// v4's swap through Permit2 (v4Swap); batched where the wallet can (EIP-7702). `rpc`: the network's node, or a fork in a test.
    func swapUniswap(_ route: UniRoute, on net: EVMNetwork, _ uni: UniswapV3.Deployment, tokenIn: String, units: Decimal, what: String,
                     note: String, rpc: EVMRPC? = nil) async throws -> String {
        let rpc = rpc ?? net.rpc
        let minOut = (route.out * (1 - route.tolerance)).whole
        if let v4 = route.v4 {
            return try await v4Swap(v4, tokenIn: tokenIn, units: units, minOut: minOut, what: what, note: note + " on Uniswap v4", rpc: rpc)
        }
        if let path = route.v2 {
            guard let v2 = UniswapV2.on(rpc.chainId) else { throw ExecutionError("No Uniswap v2 on \(net.name). Nothing was signed.") }
            let swap = try UniswapV2.swapExactTokens(amountIn: units, amountOutMin: minOut, path: path, to: wallet.evm,
                                                     deadline: UInt64(Date().timeIntervalSince1970) + 600)
            return try await approveAndSend(rpc, token: tokenIn, spender: v2.router, units: units, approveNote: "Approve exactly \(what) to Uniswap v2",
                                            to: v2.router, data: swap, note: note + " on Uniswap v2")
        }
        guard let v3 = route.v3 else { throw ExecutionError("Uniswap's route went away. Nothing was signed.") }
        return try await approveAndSend(rpc, token: tokenIn, spender: uni.router, units: units, approveNote: "Approve exactly \(what) to Uniswap",
                                        to: uni.router, data: try UniswapV3.exactInput(path: v3.path, recipient: wallet.evm, amountIn: units, amountOutMinimum: minOut),
                                        note: note + " on Uniswap")
    }

    /// A Uniswap route checked against impactCap before anything is signed (a bridge or a gas top-up included):
    /// `paid` what goes in, in USD at the market price; `amount` in the input's own units.
    func holdsImpact(_ route: UniRoute, paid: Decimal, amount: Decimal, symbol: String, signed: String? = nil) throws {
        // ponytail: an impact that couldn't be read (the twentieth's quote failed) passes; the floor is still 1% under the quote.
        try Self.checkImpact(route.impact ?? 0, paid: paid, amount: amount, symbol: symbol, venue: route.title, signed: signed)
    }

    /// What a sale for `usdc` was worth before the route's price impact took its share.
    static func beforeImpact(_ usdc: Decimal, _ route: UniRoute) -> Decimal {
        guard let impact = route.impact, impact < 1 else { return usdc }
        return usdc / (1 - impact)
    }

    /// What `gas` units cost on the chain now, in USD (ETH priced through Uniswap's WETH/USDC pool).
    func gasUSD(_ gas: UInt64, on net: EVMNetwork, _ uni: UniswapV3.Deployment) async throws -> Decimal {
        let price = try await net.rpc.quantity("eth_gasPrice", [])
        // 0.001 ETH → USDC, times a thousand: the price of one ETH.
        let milli = Decimal(1_000_000_000_000_000)
        let usdc = await bestPath(on: net.rpc, uni, from: uni.weth, to: net.usdc, amountIn: milli)?.out ?? 0
        let ethUSD = Decimal(net.micro(usdc)) / 1_000_000 * 1000
        return Decimal(gas) * Decimal(price) / 1_000_000_000_000_000_000 * ethUSD
    }

    /// Which ways Settings › Modules allows: Relay's swap needs Relay; the other needs Relay and Uniswap.
    private func allowedWays(_ net: EVMNetwork) -> (relay: Bool, uniswap: Bool) {
        let place = ModuleNetwork(place: net.key).map { [$0] } ?? []
        let relay = Modules.current.refusal(.relay, via: place) == nil
        return (relay, relay && Modules.current.refusal(.uniswap, via: place) == nil)
    }

    /// The way a trade takes: the one that leaves more, or only the one the person picked on the route's step
    /// ("relay" = Relay's swap, "uniswap" = Relay's bridge + Uniswap). A pick only narrows the two quoted ways.
    static func pick(_ ways: [SwapWay], prefer: String?) -> SwapWay? {
        (prefer.map { p in ways.filter { $0.name == p } } ?? ways).max(by: { $0.worth < $1.worth })
    }

    /// What the refusal says when the picked way has no quote: the other one is still there to choose.
    private static func unquoted(_ prefer: String?, _ what: String) -> String? {
        prefer.map { "\($0 == "relay" ? "Relay's swap" : "Relay + Uniswap") doesn't quote \(what) right now. Nothing was signed; pick «The best one» on the route's step to take the other way." }
    }

    /// Both ways of a buy (`usdc`) or a sale (`sell`, nil = all) quoted now for the route's card, the one that leaves
    /// more first; nothing is signed. `gets` is what arrives («2.41 AERO», «1.97 USDC»), `worth` its USD after gas.
    /// "here" is the one way when the trade runs on the network's own USDC (`buyHere`, or a sale that keeps the USDC).
    public func swapQuotes(_ net: EVMNetwork, _ ref: String, usdc: Decimal?, sell: Decimal?, from: String? = nil, keep: Bool = false) async throws
        -> [(way: String, gets: String, worth: Decimal)] {
        guard let uni = UniswapV3.on(net.rpc.chainId) else { return [] }
        let token = try await evmToken(ref, on: net)
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        if let usdc {
            let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
            if let here = try await buyHere(net, token, units: units, from: from, uni) {
                return [("here", "\(Fmt.num(here.route.out / scale, maxFraction: 6)) \(token.symbol)\(here.route.impactText)", usdc)]
            }
            return try await buyWays(net, token, units: units, uni).ways.sorted { $0.worth > $1.worth }
                .map { ($0.name, "\(Fmt.num($0.out / scale, maxFraction: 6)) \(token.symbol)", $0.worth) }
        }
        let held = try await balance(token.address, on: net.rpc)
        let units = (sell.map { min($0 * scale, held) } ?? held).whole
        guard units > 0 else { return [] }
        if keep {
            guard let best = await bestUniswap(on: net, uni, from: token.address, to: net.usdc, amountIn: units) else { return [] }
            let got = Decimal(net.micro(best.out)) / 1_000_000
            return [("here", "\(Fmt.num(got)) USDC\(best.impactText)", got)]
        }
        return try await sellWays(net, token, units: units, uni).ways.sorted { $0.worth > $1.worth }
            .map { ($0.name, "\(Fmt.num($0.out / 1_000_000)) USDC", $0.worth) }
    }

    /// Buys a token on an EVM network for `usdc`: with the USDC already there when it holds enough (or `from` names it),
    /// with another EVM network's USDC brought over first (`from`), else from Solana, the better of the two ways (or the
    /// one picked on the route's step).
    public func buyOn(_ net: EVMNetwork, _ ref: String, usdc: Decimal, prefer: String? = nil, from: String? = nil) async throws -> String {
        guard let uni = UniswapV3.on(net.rpc.chainId) else { throw ExecutionError("ihatecrypto swaps on Solana, Base, Arbitrum, Polygon, Ethereum, Optimism, BNB Chain and HyperEVM.") }
        let token = try await evmToken(ref, on: net), me = wallet.evm
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units >= 500_000 else { throw ExecutionError("At least 0.5 USDC: less isn't worth the bridge.") }
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        func amount(_ raw: Decimal) -> String { "\(Fmt.num(raw / scale, maxFraction: 6)) \(token.symbol)" }
        // Solana short and the network without the USDC: it comes straight from the EVM network that holds it (the card
        // shows the same, EVMNetwork.source), not from a Solana that can't pay.
        var from = from
        if from == nil, prefer == nil, Decimal(try await net.usdcBalance(me)) / 1_000_000 < usdc {
            let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
            let idle = Dictionary(await idleUSDC().map { ($0.net.key, $0.usdc) }, uniquingKeysWith: +)
            from = EVMNetwork.source(arrive: usdc, onSolana: onSolana, target: net.key, held: { idle[$0] })?.net.key
        }
        // A way picked on the route's step is one of Solana's: only then is the money on the network passed over.
        if prefer == nil || from != nil, let here = try await buyHere(net, token, units: units, from: from, uni) {
            try holdsImpact(here.route, paid: usdc, amount: usdc, symbol: "USDC")
            var spend = units
            if let src = here.src {
                let before = try await net.usdcBalance(me)
                report(try await bridge(src, to: net, usdc: usdc))
                // What arrived pays: Relay's fee comes out of it.
                let now = try await net.usdcBalance(me)
                spend = min(units, now > before ? now - before : 0)
            }
            return try await buyThere(net, token, units: spend, uni)
        }
        let (ways, uniswap) = try await buyWays(net, token, units: units, uni)
        guard let way = Self.pick(ways, prefer: prefer) else {
            throw ExecutionError(Self.unquoted(prefer, "\(token.symbol) on \(net.name)")
                ?? "Neither Relay nor Uniswap quotes \(token.symbol) on \(net.name) for \(Fmt.num(usdc)) USDC right now. Nothing was signed.")
        }
        if way.name == "uniswap", let plan = uniswap { try holdsImpact(plan.route, paid: usdc, amount: usdc, symbol: "USDC") }
        report("\(Fmt.num(usdc)) USDC → \(token.symbol) on \(net.name). " + ways.map(\.line).joined(separator: "; ") + ". Taking \(way.name == "relay" ? "Relay's swap" : "the bridge and Uniswap")\(prefer == nil ? "" : ", as picked").")

        let before = try await balance(token.address, on: net.rpc)
        var hash = ""
        if way.name == "relay" {
            let (step, changes) = try await prepareBridge(.send(usdc), to: net.relayChain, currency: token.address, decimals: token.decimals, swap: true)
            report(changes.joined(separator: " "))
            try await sendBridge(step)
        } else if let plan = uniswap {
            let there = try await net.usdcBalance(me)
            try await ensureEVM(net, usdc: there + plan.arrive)
            // Quote again with the money there: the price may have moved while the bridge ran.
            guard let fresh = await bestUniswap(on: net, uni, from: net.usdc, to: token.address, amountIn: net.raw(plan.arrive)) else {
                throw ExecutionError("Uniswap no longer quotes \(token.symbol); the USDC waits on \(net.name).")
            }
            try holdsImpact(fresh, paid: Decimal(plan.arrive) / 1_000_000, amount: Decimal(plan.arrive) / 1_000_000, symbol: "USDC",
                            signed: "The USDC that came waits on \(net.name); nothing was swapped.")
            hash = try await swapUniswap(fresh, on: net, uni, tokenIn: net.usdc, units: net.raw(plan.arrive),
                                         what: "\(Fmt.num(Decimal(plan.arrive) / 1_000_000)) USDC",
                                         note: "Buy \(token.symbol) for \(Fmt.num(Decimal(plan.arrive) / 1_000_000)) USDC")
        }
        var after = try await balance(token.address, on: net.rpc)
        let started = Date()
        while after <= before, Date().timeIntervalSince(started) < 240 {
            try await Task.sleep(for: .seconds(2))
            after = try await balance(token.address, on: net.rpc)
        }
        guard after > before else { throw ExecutionError("\(token.symbol) didn't arrive on \(net.name) within 4 minutes. The order is in the journal.") }
        traded?("\(net.key):\(token.address.lowercased())", (after - before) / scale, usdc)
        return "Bought \(amount(after - before)) on \(net.name) for \(Fmt.num(usdc)) USDC via \(way.name == "relay" ? "Relay's swap" : "Relay + Uniswap"). "
            + "You hold \(amount(after)) there.\(hash.isEmpty ? "" : " \(net.explorer)/tx/\(hash)")"
    }

    /// Whether a buy is paid on the network itself, and Uniswap's path there: `from` nil = when the network holds the
    /// USDC and Uniswap has a pool (else nil: Solana's ways); «solana» = never; the network's key = it must; another EVM
    /// network's key = its USDC comes over first (`src`). A named place that can't pay is refused, never swapped for another.
    func buyHere(_ net: EVMNetwork, _ token: EVMTokens.Token, units: UInt64, from: String?, _ uni: UniswapV3.Deployment) async throws
        -> (route: UniRoute, src: EVMNetwork?)? {
        guard from != "solana" else { return nil }
        let me = wallet.evm, want = Fmt.num(Decimal(units) / 1_000_000)
        let src = from.flatMap(EVMNetwork.named).flatMap { $0.key == net.key ? nil : $0 }
        if from != nil, from != net.key, src == nil { throw ExecutionError("ihatecrypto doesn't pay for a buy on \(net.name) from \(from ?? "there"). Nothing was signed.") }
        let there = try await (src ?? net).usdcBalance(me)
        let best = allowedWays(net).uniswap ? await bestUniswap(on: net, uni, from: net.usdc, to: token.address, amountIn: net.raw(units)) : nil
        if let best, there >= units { return (best, src) }
        guard from != nil else { return nil }
        throw ExecutionError(there < units
            ? "\(Fmt.num(Decimal(there) / 1_000_000)) USDC on \((src ?? net).name); the buy needs \(want). Nothing was signed."
            : "Uniswap has no pool for \(token.symbol) on \(net.name)\(allowedWays(net).uniswap ? "" : " (Uniswap is off in Settings › Modules)"); "
                + "“buy \(token.symbol) on \(net.key) for \(want) USDC from solana” takes Relay's swap. Nothing was signed.")
    }

    /// A buy with USDC already on the network: an exact approve and Uniswap's best path, which pays only this wallet.
    private func buyThere(_ net: EVMNetwork, _ token: EVMTokens.Token, units: UInt64, _ uni: UniswapV3.Deployment) async throws -> String {
        let me = wallet.evm, scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1), usdc = Fmt.num(Decimal(units) / 1_000_000, maxFraction: 6)
        func amount(_ raw: Decimal) -> String { "\(Fmt.num(raw / scale, maxFraction: 6)) \(token.symbol)" }
        // Gas only: the USDC is there.
        try await ensureEVM(net, usdc: units)
        guard units > 0, let best = await bestUniswap(on: net, uni, from: net.usdc, to: token.address, amountIn: net.raw(units)) else {
            throw ExecutionError("Uniswap doesn't quote \(token.symbol) on \(net.name) now; the USDC stays there. Nothing was swapped.")
        }
        try holdsImpact(best, paid: Decimal(units) / 1_000_000, amount: Decimal(units) / 1_000_000, symbol: "USDC",
                        signed: "The USDC stays on \(net.name); nothing was swapped.")
        report("\(usdc) USDC on \(net.name) → \(token.symbol) on \(best.title): ≈ \(amount(best.out))\(best.impactText), no bridge.")
        let before = try await balance(token.address, on: net.rpc)
        let hash = try await swapUniswap(best, on: net, uni, tokenIn: net.usdc, units: net.raw(units), what: "\(usdc) USDC",
                                         note: "Buy \(token.symbol) for \(usdc) USDC")
        var after = try await balance(token.address, on: net.rpc)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await balance(token.address, on: net.rpc)
        }
        // ponytail: the USDC swapped here is the cost; a bridge that brought it first adds its few cents of fee nowhere.
        if after > before { traded?("\(net.key):\(token.address.lowercased())", (after - before) / scale, Decimal(units) / 1_000_000) }
        return "Bought \(amount(after > before ? after - before : 0)) on \(net.name) for \(usdc) USDC that was there, on \(best.title). You hold \(amount(after)) there. \(net.explorer)/tx/\(hash)"
    }

    /// Both ways to buy for `units` of USDC from Solana, quoted and valued (nothing is signed): Relay's swap, and Relay's
    /// bridge (with a gas top-up when there's no ETH) followed by Uniswap's best path, its gas counted against it.
    func buyWays(_ net: EVMNetwork, _ token: EVMTokens.Token, units: UInt64, _ uni: UniswapV3.Deployment) async throws
        -> (ways: [SwapWay], uniswap: (arrive: UInt64, topup: Bool, route: UniRoute)?) {
        let me = wallet.evm, scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let allowed = allowedWays(net)
        guard allowed.relay else { throw ExecutionError(Modules.current.refusal(.relay) ?? "Relay is off in Settings › Modules.") }
        func amount(_ raw: Decimal) -> String { "\(Fmt.num(raw / scale, maxFraction: 6)) \(token.symbol)" }

        // Relay: Solana USDC in, the token out on the chain.
        let relay = try? await Relay.quote(user: wallet.solana, recipient: me, originChain: Relay.Chain.solana, destinationChain: net.relayChain,
                                           originCurrency: Relay.Currency.solanaUSDC, destinationCurrency: token.address, amount: units)
        let relayOut = relay.flatMap { Decimal(string: $0.amountOut) } ?? 0
        let relayWorth = relay.flatMap { Decimal(string: $0.amountOutUSD) } ?? 0
        // The token's price for comparing: Relay's own valuation, else the Uniswap route's.
        var price = relayOut > 0 ? relayWorth / (relayOut / scale) : 0
        var ways: [SwapWay] = []
        if relayOut > 0 { ways.append(SwapWay(name: "relay", out: relayOut, worth: relayWorth, line: "Relay swap: \(amount(relayOut)) ≈ \(Fmt.usd(relayWorth))")) }

        // Relay bridge + Uniswap: the USDC that arrives (a gas top-up rides along when there's no ETH), then the best pool path.
        var uniswap: (arrive: UInt64, topup: Bool, route: UniRoute)?
        if allowed.uniswap {
            let topup = try await net.rpc.balance(me) < net.gasFloor
            if let bridge = try? await Relay.quote(user: wallet.solana, recipient: me, originChain: Relay.Chain.solana, destinationChain: net.relayChain,
                                                   originCurrency: Relay.Currency.solanaUSDC, destinationCurrency: net.usdc, amount: units,
                                                   topupGasUSD: topup ? net.topupUSD : nil),
               let out = Decimal(string: bridge.amountOut), case let arrive = net.micro(out), arrive > 0,
               let best = await bestUniswap(on: net, uni, from: net.usdc, to: token.address, amountIn: net.raw(arrive)) {
                if price == 0 { price = Decimal(arrive) / 1_000_000 / (best.out / scale) }
                let gas = try await gasUSD(best.gas, on: net, uni)
                let worth = best.out / scale * price - gas + (topup ? net.topupUSD : 0)
                ways.append(SwapWay(name: "uniswap", out: best.out, worth: worth,
                                    line: "Relay bridge + \(best.title): \(amount(best.out))\(best.impactText) ≈ \(Fmt.usd(worth)) after \(Fmt.usd(gas)) of gas\(topup ? " (\(net.gasCoin) for gas rides along)" : "")"))
                uniswap = (arrive, topup, best)
            }
        }
        return (ways, uniswap)
    }

    /// Sells a token on an EVM network back to USDC on Solana; nil = all of it. The better of Relay's swap-and-bridge
    /// and Uniswap on the chain followed by the Relay bridge. `keep`: Uniswap there, and the USDC stays on the network.
    public func sellOn(_ net: EVMNetwork, _ ref: String, amount: Decimal?, prefer: String? = nil, keep: Bool = false) async throws -> String {
        guard let uni = UniswapV3.on(net.rpc.chainId) else { throw ExecutionError("ihatecrypto swaps on Solana, Base, Arbitrum, Polygon, Ethereum, Optimism, BNB Chain and HyperEVM.") }
        let token = try await evmToken(ref, on: net), me = wallet.evm
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let held = try await balance(token.address, on: net.rpc)
        let units = (amount.map { min($0 * scale, held) } ?? held).whole
        guard units > 0 else { throw ExecutionError("No \(token.symbol) on \(net.name).") }
        func tokens(_ raw: Decimal) -> String { "\(Fmt.num(raw / scale, maxFraction: 6)) \(token.symbol)" }
        if keep {
            guard allowedWays(net).uniswap, let best = await bestUniswap(on: net, uni, from: token.address, to: net.usdc, amountIn: units) else {
                throw ExecutionError("Uniswap has no pool to sell \(token.symbol) for USDC on \(net.name)\(allowedWays(net).uniswap ? "" : " (Uniswap is off in Settings › Modules)"); "
                                     + "without «keep», Relay's swap sells it for USDC on Solana. Nothing was signed.")
            }
            try holdsImpact(best, paid: Self.beforeImpact(Decimal(net.micro(best.out)) / 1_000_000, best), amount: units / scale, symbol: token.symbol)
            report("\(tokens(units)) on \(net.name) → ≈ \(Fmt.num(Decimal(net.micro(best.out)) / 1_000_000)) USDC there\(best.impactText), on \(best.title).")
            try await ensureEVM(net, usdc: 0)
            let sold = try await sellThere(net, token, units: units, route: best, uni)
            traded?("\(net.key):\(token.address.lowercased())", -units / scale, sold.got)
            return sold.text
        }
        let (ways, uniswap) = try await sellWays(net, token, units: units, uni)
        guard let way = Self.pick(ways, prefer: prefer) else {
            throw ExecutionError(Self.unquoted(prefer, tokens(units) + " on " + net.name)
                ?? "Neither Relay nor Uniswap quotes \(tokens(units)) on \(net.name) right now. Nothing was signed.")
        }
        if way.name == "uniswap", let plan = uniswap {
            try holdsImpact(plan, paid: Self.beforeImpact(Decimal(net.micro(plan.out)) / 1_000_000, plan), amount: units / scale, symbol: token.symbol)
        }
        report("\(tokens(units)) on \(net.name) → USDC on Solana. " + ways.map(\.line).joined(separator: "; ") + ". Taking \(way.name == "relay" ? "Relay's swap" : "Uniswap and the bridge")\(prefer == nil ? "" : ", as picked").")
        // Both ways sign on the chain: ETH for gas first (a top-up from Solana when there's none).
        try await ensureEVM(net, usdc: 0)

        guard way.name == "relay" else {
            guard let plan = uniswap else { throw ExecutionError("Uniswap's route went away. Nothing was signed.") }
            let fresh = await bestUniswap(on: net, uni, from: token.address, to: net.usdc, amountIn: units)
            if let fresh {
                try holdsImpact(fresh, paid: Self.beforeImpact(Decimal(net.micro(fresh.out)) / 1_000_000, fresh), amount: units / scale, symbol: token.symbol,
                                signed: "\(token.symbol) stays on \(net.name); nothing was swapped.")
            }
            let sold = try await sellThere(net, token, units: units, route: fresh ?? plan, uni)
            report(sold.text)
            traded?("\(net.key):\(token.address.lowercased())", -units / scale, sold.got)
            return try await evmToSolana(net, usdc: sold.got)
        }
        let text = try await relaySell(net, token: token, units: units)
        traded?("\(net.key):\(token.address.lowercased())", -units / scale, 0)
        return text
    }

    /// The Uniswap sale on the network: an exact approve of the tokens, then the swap to USDC at this wallet.
    private func sellThere(_ net: EVMNetwork, _ token: EVMTokens.Token, units: Decimal, route: UniRoute, _ uni: UniswapV3.Deployment) async throws
        -> (got: Decimal, text: String) {
        let me = wallet.evm, scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let tokens = "\(Fmt.num(units / scale, maxFraction: 6)) \(token.symbol)"
        let before = try await net.usdcBalance(me)
        let hash = try await swapUniswap(route, on: net, uni, tokenIn: token.address, units: units, what: tokens, note: "Sell \(tokens)")
        var after = try await net.usdcBalance(me)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await net.usdcBalance(me)
        }
        let got = Decimal(after > before ? after - before : 0) / 1_000_000
        return (got, "Sold \(tokens) for \(Fmt.num(got)) USDC on \(net.name), on \(route.title). \(net.explorer)/tx/\(hash)")
    }

    /// Both ways to sell `units` back to USDC on Solana, quoted and valued (nothing is signed): Relay's swap-and-bridge,
    /// and Uniswap's best path to USDC on the chain followed by Relay's bridge; each one's gas counted against it.
    func sellWays(_ net: EVMNetwork, _ token: EVMTokens.Token, units: Decimal, _ uni: UniswapV3.Deployment) async throws
        -> (ways: [SwapWay], uniswap: UniRoute?) {
        let me = wallet.evm
        let allowed = allowedWays(net)
        guard allowed.relay else { throw ExecutionError(Modules.current.refusal(.relay) ?? "Relay is off in Settings › Modules.") }
        var ways: [SwapWay] = []
        let relay = try? await Relay.quote(user: me, recipient: wallet.solana, originChain: net.relayChain, destinationChain: Relay.Chain.solana,
                                           originCurrency: token.address, destinationCurrency: Relay.Currency.solanaUSDC, amount: 0, units: "\(units)")
        if let relay, let out = Decimal(string: relay.amountOut), out > 0 {
            let gas = try await gasUSD(50_000 + 300_000, on: net, uni)
            ways.append(SwapWay(name: "relay", out: out, worth: out / 1_000_000 - gas,
                                line: "Relay swap: \(Fmt.num(out / 1_000_000)) USDC on Solana after \(Fmt.usd(gas)) of gas"))
        }
        var uniswap: UniRoute?
        if allowed.uniswap, let best = await bestUniswap(on: net, uni, from: token.address, to: net.usdc, amountIn: units),
           let bridge = try? await Relay.quote(user: me, recipient: wallet.solana, originChain: net.relayChain, destinationChain: Relay.Chain.solana,
                                               originCurrency: net.usdc, destinationCurrency: Relay.Currency.solanaUSDC, amount: 0, units: "\(net.raw(net.micro(best.out)))"),
           let out = Decimal(string: bridge.amountOut), out > 0 {
            let gas = try await gasUSD(50_000 + best.gas + 120_000, on: net, uni)
            ways.append(SwapWay(name: "uniswap", out: out, worth: out / 1_000_000 - gas,
                                line: "\(best.title) + Relay bridge: \(Fmt.num(out / 1_000_000)) USDC on Solana\(best.impactText) after \(Fmt.usd(gas)) of gas"))
            uniswap = best
        }
        return (ways, uniswap)
    }

    /// One token for another on a network, quoted: both sides (the network's own coin as its wrapped token), the amount in,
    /// and the route that gives most — Uniswap's best for the pair (v2, v3, v4), or a v4 pool of the coin itself.
    struct PairSwap {
        let tokenIn: EVMTokens.Token
        let tokenOut: EVMTokens.Token
        /// The network's own coin (ETH; POL on Polygon), not its wrapped token.
        let coinIn: Bool
        let coinOut: Bool
        let units: Decimal
        /// nil for the coin into its own wrapped token or back: a wrap or an unwrap, no swap — or for Relay's swap.
        let route: UniRoute?
        /// What Relay's swap on the network gives, in the output's smallest units, when Uniswap has no pool for the pair
        /// (HyperEVM's money sits on other exchanges, which Relay reaches).
        var relayOut: Decimal? = nil
        /// The route is a v4 pool of the coin itself: it goes in as the call's value or comes out as coins, unwrapped.
        var direct: Bool { route?.v4?.key.currency0 == UniswapV4.native }
    }

    /// Whether `ref` names the network's own coin: ETH, or POL (MATIC) on Polygon — never its wrapped token, which is a token.
    static func isCoin(_ ref: String, on net: EVMNetwork) -> Bool {
        let r = ref.uppercased().trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        return r == net.gasCoin.uppercased() || (net.gasCoin == "POL" && r == "MATIC")
    }

    /// `swapOn`'s quote, for the card too: nothing is signed.
    /// `quoteOnly`: for the card, priced at the amount asked even when the wallet holds less (the run checks it).
    func pairSwap(_ net: EVMNetwork, from a: String, to b: String, amount: Decimal?, quoteOnly: Bool = false) async throws -> PairSwap {
        guard let uni = UniswapV3.on(net.rpc.chainId), let weth = UniswapV2.on(net.rpc.chainId)?.weth else {
            throw ExecutionError("ihatecrypto swaps one token for another on Base, Arbitrum, Ethereum, Polygon, Optimism, BNB Chain and HyperEVM.")
        }
        let rpc = net.rpc
        func side(_ ref: String) async throws -> (EVMTokens.Token, Bool) {
            Self.isCoin(ref, on: net) ? (EVMTokens.Token(symbol: net.gasCoin, name: net.gasCoin, address: weth, decimals: 18), true)
                : (try await evmToken(ref, on: net), false)
        }
        let (tokenIn, coinIn) = try await side(a), (tokenOut, coinOut) = try await side(b)
        guard tokenIn.address != tokenOut.address || coinIn != coinOut else { throw ExecutionError("That's the same token on both sides. Nothing to swap.") }
        let scale = Decimal(sign: .plus, exponent: tokenIn.decimals, significand: 1)
        let held = coinIn ? Decimal(try await rpc.balance(wallet.evm)) : try await balance(tokenIn.address, on: rpc)
        let units: Decimal
        if let amount {
            units = (amount * scale).whole
            guard units <= held || quoteOnly else { throw ExecutionError("Only \(Fmt.num(held / scale, maxFraction: 8)) \(tokenIn.symbol) on \(net.name). Nothing was signed.") }
        } else if coinIn {
            // All of the coin but what this swap's fees may take, twice over.
            let (_, maxFee) = try await rpc.fees()
            units = max(0, held - Decimal(maxFee) * 1_200_000)
        } else { units = held }
        guard units > 0 else { throw ExecutionError("No \(tokenIn.symbol) to swap on \(net.name).") }
        guard !coinIn || units <= Decimal(UInt64.max) else { throw ExecutionError("At most 18 \(net.gasCoin) at once. Nothing was signed.") }
        // The coin and its wrapped token are the same money: a wrap or an unwrap.
        if tokenIn.address == tokenOut.address { return PairSwap(tokenIn: tokenIn, tokenOut: tokenOut, coinIn: coinIn, coinOut: coinOut, units: units, route: nil) }
        async let wrapped = bestUniswap(on: net, uni, from: tokenIn.address, to: tokenOut.address, amountIn: units)
        async let coinPool = coinIn || coinOut
            ? v4Route(coinIn ? UniswapV4.native : tokenIn.address, coinOut ? UniswapV4.native : tokenOut.address, amountIn: units, rpc: rpc) : nil
        var route = await wrapped
        if let pool = await coinPool, pool.out > (route?.out ?? 0) {
            var direct = UniRoute(v3: nil, v4: pool, out: pool.out)
            let small = (units / 20).whole
            if small > 0, let smallOut = await v4Quote(pool.key, zeroForOne: pool.zeroForOne, amount: small, rpc: rpc), smallOut > 0 {
                direct.impact = max(0, 1 - (pool.out / units) / (smallOut / small))
            }
            route = direct
        }
        // No Uniswap pool, or only a thin one (price impact past impactCap: USDS on Arbitrum, USDT0 on Optimism sit in dead
        // v2 pools): Relay's swap on the network itself, if it quotes the pair and gives more (the coin goes in wrapped).
        if route == nil || (route?.impact ?? 0) > Self.impactCap, allowedWays(net).relay,
           let out = try? await relayPairQuote(net, from: tokenIn.address, to: coinOut ? UniswapV4.native : tokenOut.address, units: units, capped: !quoteOnly).out,
           out > route?.out ?? 0 {
            return PairSwap(tokenIn: tokenIn, tokenOut: tokenOut, coinIn: coinIn, coinOut: coinOut, units: units, route: nil, relayOut: out)
        }
        guard let route else {
            throw ExecutionError("Neither Uniswap nor Relay swaps \(tokenIn.symbol) for \(tokenOut.symbol) on \(net.name) right now. Nothing was signed.")
        }
        return PairSwap(tokenIn: tokenIn, tokenOut: tokenOut, coinIn: coinIn, coinOut: coinOut, units: units, route: route)
    }

    /// Relay's swap on one network, from and to this wallet: its quote, checked — the sender and the recipient are this
    /// wallet, the steps an exact approve of what goes in to the approval proxy Relay publishes for the network and then
    /// a call to that same proxy with no coins, and at most 3% (or $0.15) lost by Relay's own valuation. `to` 0x0…0 is
    /// the network's coin.
    /// `capped` false: for the card, which shows what it costs instead of stopping at it.
    func relayPairQuote(_ net: EVMNetwork, from: String, to: String, units: Decimal, capped: Bool = true) async throws -> (quote: Relay.Quote, call: Relay.EVMStep, proxy: String, out: Decimal) {
        let me = wallet.evm
        let quote = try await relayQuote(user: me, recipient: me, originChain: net.relayChain, destinationChain: net.relayChain,
                                          originCurrency: from, destinationCurrency: to, amount: 0, units: "\(units)")
        let binding = BindingGuard.check(["sender": quote.sender.lowercased(), "recipient": quote.recipient.lowercased()],
                                         expected: ["sender": me.lowercased(), "recipient": me.lowercased()], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        guard let proxy = try await Relay.approvalProxy(chainId: net.relayChain)?.lowercased(),
              let call = quote.evm.last, call.to.lowercased() == proxy, call.chainId == net.relayChain, call.value == "0" || call.value == "<null>",
              quote.evm.dropLast().allSatisfy({ $0.to.lowercased() == from.lowercased() && $0.data.prefix(4).hex == "095ea7b3"
                  && ABI.address(in: Data($0.data.dropFirst(4)), word: 0)?.lowercased() == proxy && ABI.decimal(in: Data($0.data.dropFirst(4)), word: 1) == units }) else {
            throw ExecutionError("Relay's transactions don't go to the approval proxy it publishes for \(net.name), for exactly this amount. Nothing was signed.")
        }
        let out = Decimal(string: quote.amountOut) ?? 0
        let worthIn = Decimal(string: quote.amountInUSD) ?? 0, worthOut = Decimal(string: quote.amountOutUSD) ?? 0
        guard out > 0 else { throw ExecutionError("Relay doesn't quote this swap on \(net.name) now. Nothing was signed.") }
        if capped { try Self.checkLoss(worthIn - worthOut, paid: worthIn, cap: max(worthIn * Decimal(string: "0.03")!, Decimal(string: "0.15")!), what: "Relay's swap") }
        return (quote, call, proxy, out)
    }

    /// What the card shows while a token-for-token swap waits: «≈ 25.31 AERO · price impact 0.4%», and the route's name.
    public func pairQuote(_ net: EVMNetwork, from a: String, to b: String, amount: Decimal?) async throws -> (gets: String, way: String) {
        if net.key == "tempo" {
            let q: (units: UInt64, out: UInt64, from: String, to: String)
            do { q = try await tempoQuote(from: a, to: b, amount: amount, quoteOnly: true) } catch {
                guard let r = try await tempoRelay(from: a, to: b, amount: amount, quoteOnly: true) else { throw error }
                return ("\(Fmt.num(r.out / 1_000_000, maxFraction: 6)) \(r.y.symbol)", "Relay's swap")
            }
            return ("\(Fmt.num(Decimal(q.out) / 1_000_000, maxFraction: 6)) \(q.to)", "Tempo's exchange")
        }
        let p = try await pairSwap(net, from: a, to: b, amount: amount, quoteOnly: true)
        let scale = Decimal(sign: .plus, exponent: p.tokenOut.decimals, significand: 1)
        let out = p.route?.out ?? p.relayOut ?? p.units
        return ("\(Fmt.num(out / scale, maxFraction: 6)) \(p.tokenOut.symbol)\(p.route?.impactText ?? "")",
                p.route.map { $0.title } ?? (p.relayOut != nil ? "Relay's swap" : p.coinIn ? "wrap" : "unwrap"))
    }

    /// «swap 0.01 ETH for AERO on Base», «swap all DEGEN for AERO on base»: one token for another on the network, straight —
    /// the best of Uniswap v2, v3 and v4 for the pair (pairSwap), no USDC in the middle, paying only this wallet. The
    /// network's own coin goes into a v4 pool of it as the call's value, or is wrapped first (its WETH, one transaction);
    /// it comes out of such a pool as coins, or is unwrapped after. nil amount = all of it (of the coin, all but the fees).
    public func swapOn(_ net: EVMNetwork, from a: String, to b: String, amount: Decimal?) async throws -> String {
        if net.key == "tempo" { return try await tempoSwap(from: a, to: b, amount: amount) }
        let p = try await pairSwap(net, from: a, to: b, amount: amount)
        guard let uni = UniswapV3.on(net.rpc.chainId), let weth = UniswapV2.on(net.rpc.chainId)?.weth else { throw ExecutionError("No Uniswap on \(net.name).") }
        let rpc = net.rpc, me = wallet.evm
        if let route = p.route, (route.impact ?? 0) > Self.impactCap {
            let amount = p.units / Decimal(sign: .plus, exponent: p.tokenIn.decimals, significand: 1)
            let price = await Holdings.llamaPrices(net.key, [p.tokenIn.address])[p.tokenIn.address.lowercased()]?.price ?? 0
            try holdsImpact(route, paid: amount * price, amount: amount, symbol: p.coinIn ? net.gasCoin : p.tokenIn.symbol)
        }
        func show(_ raw: Decimal, _ t: EVMTokens.Token) -> String { "\(Fmt.num(raw / Decimal(sign: .plus, exponent: t.decimals, significand: 1), maxFraction: 8)) \(t.symbol)" }
        func held() async throws -> Decimal { p.coinOut ? Decimal(try await rpc.balance(me)) : try await balance(p.tokenOut.address, on: rpc) }
        let what = show(p.units, p.tokenIn), wrappedName = "W" + net.gasCoin
        guard p.units <= Decimal(UInt64.max) || !p.coinIn else { throw ExecutionError("At most 18 \(net.gasCoin) at once. Nothing was signed.") }
        let wei = p.coinIn ? NSDecimalNumber(decimal: p.units).uint64Value : 0
        try await ensureEVM(net, usdc: 0)
        let before = try await held()
        var last: String
        // What came out, when the coin's balance can't say it (its fees come out of the same coin): the WETH the swap paid
        // before it was unwrapped; a v4 pool of the coin, its quote.
        var received: Decimal?
        if p.relayOut != nil {
            // Relay's swap: the coin is wrapped first (Relay takes its wrapped token through the proxy), then asked again with
            // the money in place — the price may have moved — and every check run on that quote.
            if p.coinIn { _ = try await sendEVM(rpc, to: weth, data: Data([0xd0, 0xe3, 0x0d, 0xb0]), note: "Wrap \(what) into \(wrappedName)", value: wei) }
            let q = try await relayPairQuote(net, from: p.tokenIn.address, to: p.coinOut ? UniswapV4.native : p.tokenOut.address, units: p.units)
            report("\(what) → \(p.tokenOut.symbol) on \(net.name) through Relay's swap (no Uniswap pool there): ≈ \(show(q.out, p.tokenOut)).")
            last = try await approveAndSend(rpc, token: p.tokenIn.address, spender: q.proxy, units: p.units,
                                            approveNote: "Approve exactly \(p.coinIn ? "\(Fmt.num(p.units / 1e18, maxFraction: 8)) \(wrappedName)" : what) to Relay",
                                            to: q.call.to, data: q.call.data, note: "Relay: swap \(what) for \(p.tokenOut.symbol) on \(net.name)")
            try journal.append(.init(chain: "relay", id: q.quote.requestId, kind: .signed, note: "\(net.name) \(p.tokenIn.symbol)→\(p.tokenOut.symbol), tx \(last)"))
        } else if let route = p.route {
            report("\(what) → \(p.tokenOut.symbol) on \(net.name), on \(route.title)\(p.direct ? " (\(net.gasCoin) itself, no wrap)" : ""): ≈ \(show(route.out, p.tokenOut))\(route.impactText).")
            if p.direct {
                last = try await swapUniswap(route, on: net, uni, tokenIn: p.coinIn ? UniswapV4.native : p.tokenIn.address, units: p.units, what: what,
                                             note: "Swap \(what) for \(p.tokenOut.symbol)")
                if p.coinOut { received = route.out }
            } else {
                if p.coinIn { _ = try await sendEVM(rpc, to: weth, data: Data([0xd0, 0xe3, 0x0d, 0xb0]), note: "Wrap \(what) into \(wrappedName)", value: wei) }
                let wethBefore = p.coinOut ? try await balance(weth, on: rpc) : 0
                // The coin is its wrapped token by now: that's what the approve lets Uniswap take.
                last = try await swapUniswap(route, on: net, uni, tokenIn: p.tokenIn.address, units: p.units,
                                             what: p.coinIn ? "\(Fmt.num(p.units / 1e18, maxFraction: 8)) \(wrappedName)" : what,
                                             note: "Swap \(what) for \(p.tokenOut.symbol)")
                if p.coinOut {
                    // A node can answer from before the swap's block: it said 0, the unwrap was skipped and the WETH stayed
                    // in the wallet (2026-10-22, after an EIP-7702 batch on Base). So it's asked again while nothing shows.
                    var got = try await balance(weth, on: rpc) - wethBefore
                    for _ in 0..<10 where got <= 0 {
                        try await Task.sleep(for: .seconds(1))
                        got = try await balance(weth, on: rpc) - wethBefore
                    }
                    received = got
                    if got > 0 {
                        last = try await sendEVM(rpc, to: weth, data: Data([0x2e, 0x1a, 0x7d, 0x4d]) + (try ABI.word(got)), note: "Unwrap \(Fmt.num(got / 1e18, maxFraction: 8)) \(wrappedName) into \(net.gasCoin)")
                    }
                }
            }
        } else if p.coinIn {
            last = try await sendEVM(rpc, to: weth, data: Data([0xd0, 0xe3, 0x0d, 0xb0]), note: "Wrap \(what) into \(wrappedName)", value: wei)
        } else {
            last = try await sendEVM(rpc, to: weth, data: Data([0x2e, 0x1a, 0x7d, 0x4d]) + (try ABI.word(p.units)), note: "Unwrap \(what) into \(net.gasCoin)")
            received = p.units
        }
        var after = try await held()
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await held()
        }
        // ponytail: a token-for-token swap has no dollar cost to record; the P/L by token counts only buys and sales for USDC.
        let got = received ?? (after > before ? after - before : 0)
        let approx = p.direct && p.coinOut ? "≈ " : ""
        return "Swapped \(what) for \(approx)\(show(got, p.tokenOut)) on \(net.name)\(p.route.map { ", on \($0.title)" } ?? ""). You hold \(show(after, p.tokenOut)) there. \(net.explorer)/tx/\(last)"
    }

    /// Relay's own swap-and-bridge from a token on the chain: an exact approve to the approval proxy Relay publishes for
    /// that chain, then its call, which must go to that same proxy. The quote must be from this wallet to our Solana address.
    private func relaySell(_ net: EVMNetwork, token: EVMTokens.Token, units: Decimal) async throws -> String {
        let me = wallet.evm, scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let quote = try await relayQuote(user: me, recipient: wallet.solana, originChain: net.relayChain, destinationChain: Relay.Chain.solana,
                                          originCurrency: token.address, destinationCurrency: Relay.Currency.solanaUSDC, amount: 0, units: "\(units)")
        let binding = BindingGuard.check(["sender": quote.sender, "recipient": quote.recipient],
                                         expected: ["sender": me, "recipient": wallet.solana], subject: "Relay quote")
        guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
        guard let proxy = try await Relay.approvalProxy(chainId: net.relayChain)?.lowercased(),
              let call = quote.evm.last, call.to.lowercased() == proxy, call.chainId == net.relayChain, call.value == "0" || call.value == "<null>",
              quote.evm.dropLast().allSatisfy({ $0.to.lowercased() == token.address && $0.data.prefix(4).hex == "095ea7b3"
                  && ABI.address(in: Data($0.data.dropFirst(4)), word: 0)?.lowercased() == proxy && ABI.decimal(in: Data($0.data.dropFirst(4)), word: 1) == units }) else {
            throw ExecutionError("Relay's transactions don't go to the approval proxy it publishes for \(net.name), for exactly this amount. Nothing was signed.")
        }
        let arriving = (Decimal(string: quote.amountOut) ?? 0) / 1_000_000
        let worthIn = Decimal(string: quote.amountInUSD) ?? 0
        guard arriving > 0 else { throw ExecutionError("Relay doesn't quote \(token.symbol) for USDC now. Nothing was signed.") }
        try Self.checkLoss(worthIn - arriving, paid: worthIn, cap: max(worthIn * Decimal(string: "0.03")!, Decimal(string: "0.15")!), what: "Relay's swap")
        let mint = Relay.Currency.solanaUSDC
        let solBefore = try await rpc.tokenBalance(owner: wallet.solana, mint: mint)
        let hash = try await approveAndSend(net.rpc, token: token.address, spender: proxy, units: units,
                                            approveNote: "Approve exactly \(Fmt.num(units / scale, maxFraction: 6)) \(token.symbol) to Relay",
                                            to: call.to, data: call.data, note: "Relay: sell \(Fmt.num(units / scale, maxFraction: 6)) \(token.symbol) on \(net.name)")
        try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: "\(net.name) \(token.symbol)→Solana USDC, tx \(hash)"))
        let started = Date()
        let target = solBefore + NSDecimalNumber(decimal: arriving * 990_000).uint64Value
        while Date().timeIntervalSince(started) < 240 {
            let status = try await Relay.status(requestId: quote.requestId).status
            if ["refund", "failure"].contains(status) {
                try journal.append(.init(chain: "relay", id: quote.requestId, kind: .failed, note: status))
                throw ExecutionError("Relay returned \(status). Check \(token.symbol) on \(net.name). \(net.explorer)/tx/\(hash)")
            }
            let now = try await rpc.tokenBalance(owner: wallet.solana, mint: mint)
            if now >= target {
                try journal.append(.init(chain: "relay", id: quote.requestId, kind: .settled, note: status))
                return "Sold \(Fmt.num(units / scale, maxFraction: 6)) \(token.symbol) on \(net.name) via Relay: \(Fmt.num(Decimal(now - solBefore) / 1_000_000)) USDC arrived on Solana in \(Int(Date().timeIntervalSince(started))) s. \(net.explorer)/tx/\(hash)"
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw ExecutionError("Relay didn't credit Solana within 4 minutes. Order \(quote.requestId) is in the journal. \(net.explorer)/tx/\(hash)")
    }
}

/// Uniswap on an EVM network for one trade: v3's best path, v4's pool without hooks or v2's path, whichever gives most.
struct UniRoute {
    var v3: (path: Data, hops: Int)?
    var v4: Executor.V4Route?
    /// v2's path: the pair's pool, or two through WETH.
    var v2: [String]?
    let out: Decimal
    /// How much worse the trade's price is than a twentieth of it on the same route, 0…1; nil when it couldn't be read.
    var impact: Decimal?
    var title: String { v4 != nil ? "Uniswap v4" : v2 != nil ? "Uniswap v2" : "Uniswap v3" }
    /// How far below the quote the swap may still go through: 0.5% in a deep pool (impact under 0.5%), 1% under 2%,
    /// past that half the impact plus 1%, at most 3%; 1% when the impact is unknown. A tighter floor leaves less to take
    /// for anyone moving the price around the swap; a thin pool moves more on its own between the quote and the block.
    var tolerance: Decimal { Self.tolerance(impact: impact) }
    static func tolerance(impact: Decimal?) -> Decimal {
        guard let impact else { return Decimal(string: "0.01")! }
        if impact < Decimal(string: "0.005")! { return Decimal(string: "0.005")! }
        if impact < Decimal(string: "0.02")! { return Decimal(string: "0.01")! }
        return min(impact / 2 + Decimal(string: "0.01")!, Decimal(string: "0.03")!)
    }
    /// « · price impact 1.2%» from a tenth of a percent up, for the card and the report.
    var impactText: String {
        guard let impact, impact >= Decimal(string: "0.001")! else { return "" }
        return " · price impact \(Fmt.num((impact * 100).rounded(impact < Decimal(string: "0.1")! ? 2 : 1, .plain)))%"
    }
    /// Gas the swap takes with its approves: v4's goes through Permit2 (an approve to it, Permit2's own, the swap).
    var gas: UInt64 {
        if v4 != nil { return 50_000 + 60_000 + 180_000 }
        if let v2 { return 50_000 + (v2.count == 2 ? 130_000 : 200_000) }
        return 50_000 + ((v3?.hops ?? 1) == 1 ? 160_000 : 240_000)
    }
}
