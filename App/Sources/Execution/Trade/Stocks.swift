import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import RelayBridge
import TxGuard
import WalletCore

/// Robinhood Stock Tokens on Robinhood Chain: the registry says which contract is the real one; the chain confirms it.
public enum RobinhoodStocks {
    public struct Token: Sendable, Equatable {
        public let symbol: String
        public let name: String
        public let address: String
        public let decimals: Int
        public let logo: URL?
    }

    /// Robinhood's own asset registry (api.robinhood.com/rhj/assets, documented at docs.robinhood.com/chain):
    /// active tokens deployed on chain 4663.
    public static func registry() async throws -> [Token] {
        let (data, _) = try await URLSession.shared.data(from: URL(string: "https://api.robinhood.com/rhj/assets")!)
        return parse(data)
    }

    public static func parse(_ data: Data) -> [Token] {
        guard let assets = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["assets"] as? [[String: Any]] else { return [] }
        return assets.compactMap { a in
            guard a["status"] as? String == "ASSET_STATUS_ACTIVE", let symbol = a["tokenSymbol"] as? String,
                  let d = (a["deployments"] as? [[String: Any]])?.first(where: { ($0["chainId"] as? NSNumber)?.intValue == 4663 }),
                  let address = d["contractAddress"] as? String else { return nil }
            return Token(symbol: symbol, name: (a["tokenName"] as? String ?? symbol).replacingOccurrences(of: " • Robinhood Token", with: ""),
                         address: address, decimals: (a["tokenDecimals"] as? NSNumber)?.intValue ?? 18, logo: (a["logoUrl"] as? String).flatMap(URL.init))
        }
    }

    /// Robinhood's own bid and ask for the token (api.robinhood.com/rhj/prices/{symbol}).
    public static func quote(_ symbol: String) async throws -> (bid: Decimal, ask: Decimal)? {
        let (data, _) = try await URLSession.shared.data(from: URL(string: "https://api.robinhood.com/rhj/prices/\(symbol)")!)
        guard let q = ((try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["quotes"] as? [[String: Any]])?.first,
              let bid = (q["bid"] as? String).flatMap({ Decimal(string: $0) }), let ask = (q["ask"] as? String).flatMap({ Decimal(string: $0) }) else { return nil }
        return (bid, ask)
    }

    /// The registry's token, confirmed on-chain: same symbol and decimals at that address.
    public static func verified(_ symbol: String, rpc: EVMRPC = .robinhood) async throws -> Token {
        guard let token = try await registry().first(where: { $0.symbol.uppercased() == symbol.uppercased() }) else {
            throw ExecutionError("\(symbol) isn't an active Robinhood Stock Token.")
        }
        let onChainSymbol = ABI.string(try await rpc.read(token.address, Data([0x95, 0xd8, 0x9b, 0x41])))
        let onChainDecimals = ABI.uint(in: try await rpc.read(token.address, Data([0x31, 0x3c, 0xe5, 0x67])), word: 0)
        guard onChainSymbol == token.symbol, onChainDecimals == UInt64(token.decimals) else {
            throw ExecutionError("The \(symbol) contract on Robinhood Chain doesn't match Robinhood's registry. Nothing was signed.")
        }
        return token
    }

    /// A registry symbol, or any token by its contract («купи 0x… на $2»): it must trade on Robinhood Chain with at
    /// least $50k of liquidity (DexScreener), and its symbol and decimals are read from the contract itself.
    public static func resolve(_ symbolOrContract: String, rpc: EVMRPC = .robinhood) async throws -> (token: Token, listing: TokenLookup.Info?) {
        guard symbolOrContract.hasPrefix("0x") else { return (try await verified(symbolOrContract, rpc: rpc), nil) }
        let address = symbolOrContract
        guard let info = try await TokenLookup.lookup(address).first(where: { $0.chain == "robinhood" }) else {
            throw ExecutionError("This contract doesn't trade on Robinhood Chain, and by contract ihatecrypto buys only there for now.")
        }
        guard let liquidity = info.liquidityUSD, liquidity >= 50_000 else {
            throw ExecutionError("\(info.symbol) has only \(Fmt.usd(info.liquidityUSD ?? 0)) of liquidity: too thin to trade safely.")
        }
        guard let decimals = ABI.uint(in: try await rpc.read(address, Data([0x31, 0x3c, 0xe5, 0x67])), word: 0), decimals <= 36 else {
            throw ExecutionError("The contract doesn't answer decimals(): not a token ihatecrypto can trade.")
        }
        let symbol = ABI.string(try await rpc.read(address, Data([0x95, 0xd8, 0x9b, 0x41]))) ?? info.symbol
        let name = ABI.string(try await rpc.read(address, Data([0x06, 0xfd, 0xde, 0x03]))) ?? info.name
        return (Token(symbol: symbol, name: name, address: address, decimals: Int(decimals), logo: info.imageURL), info)
    }
}

extension Executor {
    func balance(_ token: String, on rpc: EVMRPC) async throws -> Decimal {
        ABI.decimal(in: try await rpc.read(token, try ERC20.balanceOf(wallet.evm)), word: 0) ?? 0
    }

    /// The Uniswap v3 pool with the most output for this trade, and that output.
    /// Amounts in the tokens' smallest units, as 256-bit words: 54 AI of 18 decimals is past 64 bits.
    func bestPool(_ tokenIn: String, _ tokenOut: String, amountIn: Decimal) async throws -> (fee: UInt64, out: Decimal)? {
        var best: (UInt64, Decimal)?
        for fee in UniswapV3.fees {
            guard let answer = try? await EVMRPC.robinhood.read(UniswapV3.robinhoodQuoter,
                    try UniswapV3.quoteExactInputSingle(tokenIn: tokenIn, tokenOut: tokenOut, amountIn: amountIn, fee: fee)),
                  let out = ABI.decimal(in: answer, word: 0), out > (best?.1 ?? 0) else { continue }
            best = (fee, out)
        }
        return best.map { (fee: $0.0, out: $0.1) }
    }

    /// Buys a Stock Token (or any token by its contract) with USDC from Solana: Relay brings USDG (and gas) to
    /// Robinhood Chain, Uniswap swaps it. The price must sit within Robinhood's own bid–ask, or DexScreener's price for a
    /// contract (±5%); the swap pays only this wallet (the signer checks).
    public func buyStock(_ symbolOrContract: String, usdc: Decimal) async throws -> String {
        let rpc = EVMRPC.robinhood, usdg = EVMRPC.robinhoodUSDG, me = wallet.evm
        let (token, listing) = try await RobinhoodStocks.resolve(symbolOrContract)
        let symbol = token.symbol
        var units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units >= 500_000 else { throw ExecutionError("At least 0.5 USDC: less isn't worth the bridge.") }
        // A few cents short on Robinhood Chain (300 asked, 299.86 USDG there after a bridge's fee): the buy spends what's
        // there, never a bridge from Solana for the cents. Up to $1 or 2% short.
        let there = try await rpc.tokenBalance(usdg, owner: me)
        if there < units, there > 0, units - there <= max(1_000_000, units / 50) { units = there }
        // Every way quoted for this amount — Uniswap v4's pool without hooks, v3's best pool, Relay's swap — and the one that
        // gives the most taken (AI for 300 USDG: v4 1,906, Relay 1,880, v3's thin pool at $0.22 a token). The card showed the
        // same comparison (stockQuotes); it's asked again here, right before signing.
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let ways = await stockWays(token, buy: true, units: Decimal(units), listing: listing)
        let v3 = ways.v3, v4 = ways.v4
        guard let best = ways.ranked.first else {
            guard let listing else { throw ExecutionError(noPool(symbol, listing)) }
            return try await relaySwap(token, buy: true, units: Decimal(units), listing: listing)
        }
        report(waysLine(ways) { "\(Fmt.num($0 / scale, maxFraction: 2)) \(symbol)" } + ": taking \(StockWays.title(best.name)).")
        if best.name == "relay", let listing { return try await relaySwap(token, buy: true, units: Decimal(units), listing: listing) }
        let useV4 = best.name == "v4", out = best.out
        let price = (Decimal(units) / 1_000_000) / (out / scale)
        var band = listing?.priceUSD.map { (bid: $0, ask: $0) }
        if listing == nil { band = try await RobinhoodStocks.quote(symbol) }
        if let q = band, price > q.ask * Decimal(string: "1.05")! || price < q.bid * Decimal(string: "0.95")! {
            // Relay's swap has its own checks (within 5% of DexScreener's price, at most 3% lost).
            if let listing {
                report("Uniswap would pay \(Fmt.usd(price)) a token for this much, DexScreener says \(Fmt.usd(q.bid)): its pools are thin. Relay's swap goes through the deeper ones.")
                return try await relaySwap(token, buy: true, units: Decimal(units), listing: listing)
            }
            throw ExecutionError("Uniswap's price \(Fmt.usd(price)) is outside Robinhood's own \(Fmt.usd(q.bid))–\(Fmt.usd(q.ask)). Nothing was signed.")
        }
        report("\(token.name) (\(symbol)): ≈ \(Fmt.usd(price)) a token on Uniswap \(useV4 ? "v4" : "v3 (pool fee \(Fmt.num(Decimal(v3?.fee ?? 0) / 10_000))%)").")
        try await ensureEVM(.robinhood, usdc: units)
        let before = try await balance(token.address, on: rpc)
        let hash: String
        // Quoted again with the money there: the price may have moved while the bridge ran.
        if useV4, let route = v4 {
            guard let fresh = await v4Quote(route.key, zeroForOne: route.zeroForOne, amount: Decimal(units)) else {
                throw ExecutionError("Uniswap v4 no longer quotes \(symbol); the USDG waits on Robinhood Chain. Nothing was swapped.")
            }
            hash = try await v4Swap(route, tokenIn: usdg, units: Decimal(units), minOut: (fresh * 99 / 100).rounded(0, .down),
                                    what: "\(Fmt.num(Decimal(units) / 1_000_000)) USDG", note: "Buy \(symbol) for \(Fmt.num(Decimal(units) / 1_000_000)) USDG on Uniswap v4")
        } else {
            guard let fresh = try await bestPool(usdg, token.address, amountIn: Decimal(units)) else { throw ExecutionError(noPool(symbol, listing)) }
            let minimum = (fresh.out * 99 / 100).rounded(0, .down)
            hash = try await approveAndSend(rpc, token: usdg, spender: UniswapV3.robinhoodRouter, units: Decimal(units),
                                            approveNote: "Approve exactly \(Fmt.num(Decimal(units) / 1_000_000)) USDG to Uniswap", to: UniswapV3.robinhoodRouter,
                                            data: try UniswapV3.exactInputSingle(tokenIn: usdg, tokenOut: token.address, fee: fresh.fee, recipient: me,
                                                                                 amountIn: Decimal(units), amountOutMinimum: minimum),
                                            note: "Buy \(symbol) for \(Fmt.num(Decimal(units) / 1_000_000)) USDG")
        }
        var after = try await balance(token.address, on: rpc)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await balance(token.address, on: rpc)
        }
        let got = (after - before) / scale
        traded?("robinhood:\(token.address.lowercased())", got, Decimal(units) / 1_000_000)
        return "Bought \(Fmt.num(got, maxFraction: 6)) \(symbol) (\(token.name)) for \(Fmt.num(Decimal(units) / 1_000_000)) USDG — ≈ \(Fmt.usd(got > 0 ? (Decimal(units) / 1_000_000) / got : price)) a token. You hold \(Fmt.num(after / scale, maxFraction: 6)) \(symbol) on Robinhood Chain. \(EVMNetwork.robinhood.explorer)/tx/\(hash)"
    }

    /// A token Uniswap v3 doesn't pool with USDG, bought by Relay's swap on Robinhood Chain: an exact USDG approve to the
    /// approval proxy Relay publishes for the chain, then its call to that same proxy, from and to this wallet; the price
    /// within 5% of DexScreener's and at most 3% (or $0.15) lost on the way.
    /// A token Uniswap v3 doesn't pool with USDG, bought (`buy`) or sold by Relay's swap on Robinhood Chain: an exact approve of
    /// what goes in to the approval proxy Relay publishes for the chain, then its call to that same proxy, from and to this
    /// wallet; the price within 5% of DexScreener's and at most 3% (or $0.15) lost. `units`: what goes in, smallest units.
    private func relaySwap(_ token: RobinhoodStocks.Token, buy: Bool, units: Decimal, listing: TokenLookup.Info) async throws -> String {
        let rpc = EVMRPC.robinhood, usdg = EVMRPC.robinhoodUSDG, me = wallet.evm, chain = Relay.Chain.robinhood
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let tokenIn = buy ? usdg : token.address, tokenOut = buy ? token.address : usdg
        let inScale: Decimal = buy ? 1_000_000 : scale, outScale: Decimal = buy ? scale : 1_000_000
        let (inSymbol, outSymbol) = buy ? ("USDG", token.symbol) : (token.symbol, "USDG")
        let amountIn = units / inScale
        try await ensureEVM(.robinhood, usdc: buy ? NSDecimalNumber(decimal: units).uint64Value : 0)
        guard let proxy = try await Relay.approvalProxy(chainId: chain)?.lowercased() else { throw ExecutionError("Relay publishes no approval proxy for Robinhood Chain.") }
        /// Relay's quote with every check: from and to this wallet, its call to the proxy after an exact approve, the price.
        func checked() async throws -> (quote: Relay.Quote, call: Relay.EVMStep, price: Decimal, out: Decimal) {
            let quote = try await Relay.quote(user: me, recipient: me, originChain: chain, destinationChain: chain,
                                              originCurrency: tokenIn, destinationCurrency: tokenOut, amount: 0, units: "\(units)")
            let binding = BindingGuard.check(["sender": quote.sender.lowercased(), "recipient": quote.recipient.lowercased()],
                                             expected: ["sender": me.lowercased(), "recipient": me.lowercased()], subject: "Relay quote")
            guard binding.ok else { throw GuardStop(binding.problems.joined(separator: " ")) }
            guard let call = quote.evm.last, call.to.lowercased() == proxy, call.chainId == chain, call.value == "0" || call.value == "<null>",
                  quote.evm.dropLast().allSatisfy({ exactApprove($0, token: tokenIn.lowercased(), spender: proxy, units: units) }) else {
                throw ExecutionError("Relay's transactions don't go to the approval proxy it publishes for Robinhood Chain, for exactly this amount. Nothing was signed.")
            }
            let out = (Decimal(string: quote.amountOut) ?? 0) / outScale
            // What goes in and what comes out, in dollars: USDG counts as a dollar, the token at Relay's own valuation.
            let worthIn = buy ? amountIn : Decimal(string: quote.amountInUSD) ?? 0, worthOut = buy ? Decimal(string: quote.amountOutUSD) ?? 0 : out
            guard out > 0 else { throw ExecutionError("Relay doesn't quote \(inSymbol) now. Nothing was signed.") }
            try Self.checkLoss(worthIn - worthOut, paid: worthIn, cap: max(worthIn * Decimal(string: "0.03")!, Decimal(string: "0.15")!), what: "Relay's swap")
            let price = buy ? amountIn / out : out / amountIn
            if let p = listing.priceUSD, price > p * Decimal(string: "1.05")! || price < p * Decimal(string: "0.95")! {
                throw ExecutionError("Relay's price \(Fmt.usd(price)) is off DexScreener's \(Fmt.usd(p)) by more than 5%. Nothing was signed.")
            }
            return (quote, call, price, out)
        }
        var q = try await checked()
        let first = q
        report("\(token.name) (\(token.symbol)): no Uniswap v3 pool with USDG, so Relay \(buy ? "buys" : "sells") it on Robinhood Chain: ≈ \(Fmt.usd(q.price)) a token.")
        // Batched (EIP-7702), the approve goes with the swap; otherwise first, on its own.
        let approveNote = "Approve exactly \(Fmt.num(amountIn, maxFraction: 6)) \(inSymbol) to Relay", batched = canBatch(rpc)
        if !batched { try await approve(rpc, token: tokenIn, spender: proxy, units: units, note: approveNote) }
        func held() async throws -> Decimal { buy ? try await balance(token.address, on: rpc) : Decimal(try await rpc.tokenBalance(usdg, owner: me)) }
        let before = try await held()
        // Relay's route is priced for the moment it's quoted: after the approve's wait it's asked again, and a dry run that
        // still reverts (a pool moved: «T8: Insufficient Liquidity» on SHROOM) asks once more. The dry run comes before any
        // signature, so a retry signs nothing twice.
        var hash = ""
        for attempt in 0..<3 {
            q = try await checked()
            // Asked again, the price may have moved: more than 10% less out than when the move started stops here and says
            // so, rather than trading at a price nobody saw.
            if let drop = Self.worseByMoreThanTenPercent(first: first.out, now: q.out) {
                throw ExecutionError("⚠️ Asked again, Relay gives \(Fmt.num(q.out, maxFraction: 2)) \(outSymbol) for \(Fmt.num(amountIn, maxFraction: 6)) \(inSymbol) instead of "
                                     + "\(Fmt.num(first.out, maxFraction: 2)) (−\(Fmt.num(drop))%): more than 10% worse, so nothing was swapped. "
                                     + (batched ? "" : "The exact approve stays; ") + "say «do it» again to \(buy ? "buy" : "sell") at the new price.")
            }
            do {
                hash = try await approveAndSend(rpc, token: tokenIn, spender: proxy, units: units, approveNote: approveNote, to: q.call.to, data: q.call.data,
                                                note: "Relay: \(buy ? "buy" : "sell") \(token.symbol) for \(buy ? "\(Fmt.num(amountIn)) USDG" : "USDG")")
                break
            } catch where "\(error.localizedDescription)".contains("eth_estimateGas") {
                guard attempt < 2 else {
                    throw ExecutionError("Relay couldn't route \(Fmt.num(amountIn, maxFraction: 6)) \(inSymbol) into \(outSymbol) just now (\(error.localizedDescription)). "
                                         + "Nothing was swapped\(batched ? "" : "; the exact approve stays for the next try"). Try again in a minute, or with less.")
                }
                report("Relay's route for \(token.symbol) moved before the swap: asking it again.")
                try await Task.sleep(for: .seconds(2))
            }
        }
        try journal.append(.init(chain: "relay", id: q.quote.requestId, kind: .signed, note: "Robinhood \(inSymbol)→\(outSymbol), tx \(hash)"))
        var after = try await held()
        for _ in 0..<30 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await held()
        }
        guard after > before else { throw ExecutionError("Relay took the \(inSymbol) but no \(outSymbol) arrived yet. Order \(q.quote.requestId) is in the journal. \(EVMNetwork.robinhood.explorer)/tx/\(hash)") }
        let got = (after - before) / outScale
        traded?("robinhood:\(token.address.lowercased())", buy ? got : -amountIn, buy ? amountIn : got)
        if buy {
            return "Bought \(Fmt.num(got, maxFraction: 6)) \(token.symbol) (\(token.name)) for \(Fmt.num(amountIn)) USDG via Relay — ≈ \(Fmt.usd(amountIn / got)) a token. "
                + "You hold \(Fmt.num(after / scale, maxFraction: 6)) \(token.symbol) on Robinhood Chain. \(EVMNetwork.robinhood.explorer)/tx/\(hash)"
        }
        return "Sold \(Fmt.num(amountIn, maxFraction: 6)) \(token.symbol) for \(Fmt.num(got)) USDG via Relay. Say “move USDG from Robinhood to Solana” to bring it back. "
            + "\(EVMNetwork.robinhood.explorer)/tx/\(hash)"
    }

    /// How much worse (%, one decimal) a fresh quote is than the first, when that's more than 10%; nil otherwise.
    public static func worseByMoreThanTenPercent(first: Decimal, now: Decimal) -> Decimal? {
        guard first > 0, now < first * Decimal(string: "0.9")! else { return nil }
        return ((1 - now / first) * 100).rounded(1, .plain)
    }

    /// An EVM step that is exactly «approve `units` of `token` to `spender`».
    private func exactApprove(_ step: Relay.EVMStep, token: String, spender: String, units: Decimal) -> Bool {
        let args = Data(step.data.dropFirst(4))
        return step.to.lowercased() == token && step.data.prefix(4).hex == "095ea7b3"
            && ABI.address(in: args, word: 0)?.lowercased() == spender && ABI.decimal(in: args, word: 1) == units
    }

    private func noPool(_ symbol: String, _ listing: TokenLookup.Info?) -> String {
        listing == nil ? "No Uniswap pool trades \(symbol) for USDG." : "No Uniswap v3 pool on Robinhood Chain trades \(symbol) for USDG."
    }

    /// Sells a Stock Token for USDG on Robinhood Chain; nil = all of it.
    public func sellStock(_ symbolOrContract: String, amount: Decimal?) async throws -> String {
        let rpc = EVMRPC.robinhood, usdg = EVMRPC.robinhoodUSDG, me = wallet.evm
        let (token, listing) = try await RobinhoodStocks.resolve(symbolOrContract)
        let symbol = token.symbol
        let scale = Decimal(sign: .plus, exponent: token.decimals, significand: 1)
        let held = try await balance(token.address, on: rpc)
        let wanted = (amount.map { min($0 * scale, held) } ?? held).rounded(0, .down)
        guard wanted > 0 else { throw ExecutionError("No \(symbol) on Robinhood Chain.") }
        // The way that pays the most of v4's pool, v3's best pool and Relay's swap; one far under the market (a thin pool:
        // selling 1,880 AI into v3's would have lost about a third) goes through Relay's deep pools instead.
        let ways = await stockWays(token, buy: false, units: wanted, listing: listing)
        let v3 = ways.v3, v4 = ways.v4
        guard let best = ways.ranked.first else {
            guard let listing else { throw ExecutionError(noPool(symbol, listing)) }
            return try await relaySwap(token, buy: false, units: wanted, listing: listing)
        }
        report(waysLine(ways) { "\(Fmt.num($0 / 1_000_000)) USDG" } + ": taking \(StockWays.title(best.name)).")
        if best.name == "relay", let listing { return try await relaySwap(token, buy: false, units: wanted, listing: listing) }
        let useV4 = best.name == "v4", out = best.out
        let got = (out / 1_000_000) / (wanted / scale)
        var band = listing?.priceUSD.map { (bid: $0, ask: $0) }
        if listing == nil { band = try await RobinhoodStocks.quote(symbol) }
        if let q = band, got < q.bid * Decimal(string: "0.95")! {
            if let listing {
                report("Uniswap would pay \(Fmt.usd(got)) a token for this much, DexScreener says \(Fmt.usd(q.bid)): its pools are thin. Relay's swap goes through the deeper ones.")
                return try await relaySwap(token, buy: false, units: wanted, listing: listing)
            }
            throw ExecutionError("Uniswap would pay \(Fmt.usd(got)) a token, more than 5% under Robinhood's own bid \(Fmt.usd(q.bid)). Nothing was signed.")
        }
        try await ensureEVM(.robinhood, usdc: 0)
        let before = try await rpc.tokenBalance(usdg, owner: me)
        let hash: String
        let tokens = "\(Fmt.num(wanted / scale, maxFraction: 6)) \(symbol)"
        if useV4, let route = v4 {
            guard let fresh = await v4Quote(route.key, zeroForOne: route.zeroForOne, amount: wanted) else {
                throw ExecutionError("Uniswap v4 no longer quotes \(symbol). Nothing was swapped.")
            }
            hash = try await v4Swap(route, tokenIn: token.address, units: wanted, minOut: (fresh * 99 / 100).rounded(0, .down), what: tokens,
                                    note: "Sell \(tokens) on Uniswap v4")
        } else {
            guard let pool = v3 else { throw ExecutionError(noPool(symbol, listing)) }
            hash = try await approveAndSend(rpc, token: token.address, spender: UniswapV3.robinhoodRouter, units: wanted,
                                            approveNote: "Approve exactly \(tokens) to Uniswap", to: UniswapV3.robinhoodRouter,
                                            data: try UniswapV3.exactInputSingle(tokenIn: token.address, tokenOut: usdg, fee: pool.fee, recipient: me,
                                                                                 amountIn: wanted, amountOutMinimum: (pool.out * 99 / 100).rounded(0, .down)),
                                            note: "Sell \(tokens)")
        }
        var after = try await rpc.tokenBalance(usdg, owner: me)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await rpc.tokenBalance(usdg, owner: me)
        }
        traded?("robinhood:\(token.address.lowercased())", -wanted / scale, Decimal(after > before ? after - before : 0) / 1_000_000)
        return "Sold \(Fmt.num(wanted / scale, maxFraction: 6)) \(symbol) for \(Fmt.num(Decimal(after - before) / 1_000_000)) USDG. Say “move USDG from Robinhood to Solana” to bring it back. \(EVMNetwork.robinhood.explorer)/tx/\(hash)"
    }
}
