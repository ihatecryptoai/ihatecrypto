import Foundation
import P256K
import WalletCore

/// Recursive Length Prefix, Ethereum's serialization.
public indirect enum RLP: Sendable {
    case bytes(Data)
    case list([RLP])

    /// Integers are big-endian with no leading zeros; zero is the empty string.
    public static func uint(_ n: UInt64) -> RLP {
        var be = withUnsafeBytes(of: n.bigEndian) { Data($0) }
        while be.first == 0 { be.removeFirst() }
        return .bytes(be)
    }

    public static func uint(bigEndian bytes: Data) -> RLP {
        var b = bytes
        while b.first == 0 { b.removeFirst() }
        return .bytes(b)
    }

    public var encoded: Data {
        switch self {
        case .bytes(let d):
            if d.count == 1, d[d.startIndex] < 0x80 { return d }
            return Self.header(d.count, short: 0x80, long: 0xb7) + d
        case .list(let items):
            let body = items.reduce(Data()) { $0 + $1.encoded }
            return Self.header(body.count, short: 0xc0, long: 0xf7) + body
        }
    }

    static func header(_ length: Int, short: UInt8, long: UInt8) -> Data {
        if length < 56 { return Data([short + UInt8(length)]) }
        var be = withUnsafeBytes(of: UInt64(length).bigEndian) { Data($0) }
        while be.first == 0 { be.removeFirst() }
        return Data([long + UInt8(be.count)]) + be
    }
}

/// EIP-1559 (type 2) transaction; with `authorizations`, EIP-7702's type 4.
/// ponytail: value in wei as UInt64 caps a single native transfer at ~18 ETH; widen when that matters.
public struct EVMTransaction: Sendable {
    /// EIP-7702: this address may run `address`'s code. The key signed keccak256(0x05 ‖ rlp([chainId, address, nonce])).
    public struct Authorization: Sendable {
        public let chainId: UInt64
        public let address: String
        public let nonce: UInt64
        public let signature: EVMSignature
        public init(chainId: UInt64, address: String, nonce: UInt64, signature: EVMSignature) {
            (self.chainId, self.address, self.nonce, self.signature) = (chainId, address, nonce, signature)
        }
    }

    public var chainId: UInt64
    public var nonce: UInt64
    public var maxPriorityFeePerGas: UInt64
    public var maxFeePerGas: UInt64
    public var gasLimit: UInt64
    public var to: String
    public var value: UInt64
    public var data: Data
    public var authorizations: [Authorization] = []

    public init(chainId: UInt64, nonce: UInt64, maxPriorityFeePerGas: UInt64, maxFeePerGas: UInt64,
                gasLimit: UInt64, to: String, value: UInt64, data: Data) {
        self.chainId = chainId
        self.nonce = nonce
        self.maxPriorityFeePerGas = maxPriorityFeePerGas
        self.maxFeePerGas = maxFeePerGas
        self.gasLimit = gasLimit
        self.to = to
        self.value = value
        self.data = data
    }

    /// 2, or 4 when it carries authorizations.
    public var type: UInt8 { authorizations.isEmpty ? 2 : 4 }

    func fields() throws -> [RLP] {
        guard let to = Data(hex: to), to.count == 20 else { throw WalletError("Invalid recipient address: \(self.to)") }
        let base: [RLP] = [.uint(chainId), .uint(nonce), .uint(maxPriorityFeePerGas), .uint(maxFeePerGas), .uint(gasLimit),
                           .bytes(to), .uint(value), .bytes(data), .list([])]
        guard !authorizations.isEmpty else { return base }
        return base + [.list(try authorizations.map { a in
            guard let address = Data(hex: a.address), address.count == 20 else { throw WalletError("Invalid authorization address: \(a.address)") }
            return .list([.uint(a.chainId), .bytes(address), .uint(a.nonce), .uint(UInt64(a.signature.v - 27)),
                          .uint(bigEndian: a.signature.r), .uint(bigEndian: a.signature.s)])
        })]
    }

    /// type ‖ rlp(fields): the unsigned transaction, as a hardware wallet reads it.
    public func unsignedPayload() throws -> Data { Data([type]) + RLP.list(try fields()).encoded }

    /// keccak256(type ‖ rlp(fields)) — what the key signs.
    public func signingHash() throws -> Data { Keccak.hash256(try unsignedPayload()) }

    /// Raw signed transaction and its hash (the transaction id).
    public func signed(privateKey: Data) throws -> (raw: Data, hash: String) {
        try signed(with: try EVMSigner.sign(digest: try signingHash(), privateKey: privateKey))
    }

    /// The transaction with a signature made elsewhere (a Ledger), v = 27 or 28.
    public func signed(with signature: EVMSignature) throws -> (raw: Data, hash: String) {
        let raw = Data([type]) + RLP.list(try fields() + [
            .uint(UInt64(signature.v - 27)), .uint(bigEndian: signature.r), .uint(bigEndian: signature.s),
        ]).encoded
        return (raw, "0x" + Keccak.hash256(raw).hex)
    }

    /// Most this transaction can cost in fees, wei.
    public var maxFee: UInt64 { gasLimit * maxFeePerGas }

    /// The most gas × max fee per gas may come to, per network, in its smallest unit: far above a real fee, so only a
    /// broken or hostile request reaches it. Ethereum 0.05 ETH (≈ $150; a 570k-gas Morpho deposit at a 43 gwei base fee),
    /// Arbitrum, Base and Robinhood Chain 0.01 ETH (≈ $30; their calls cost cents), Polygon 10 POL (≈ $3, ten times a call
    /// at 1000 gwei; 64 bits of wei hold 18 POL at most), Tempo $10 (its fees are counted in attodollars), HyperEVM 0.02 HYPE
    /// (≈ $2; a call costs a tenth of a cent at 0.1 gwei), BNB Chain 0.01 BNB (≈ $8; a swap costs a cent at 0.05 gwei).
    public static let feeCap: [UInt64: UInt64] = [1: 50_000_000_000_000_000, 42161: 10_000_000_000_000_000, 8453: 10_000_000_000_000_000,
                                                  4663: 10_000_000_000_000_000, 137: 10_000_000_000_000_000_000, 4217: 10_000_000_000_000_000_000,
                                                  999: 20_000_000_000_000_000, 10: 10_000_000_000_000_000,
                                                  56: 10_000_000_000_000_000]

    /// Why the signer won't sign this fee, or nil: past the network's cap, or past 64 bits.
    public var feeProblem: String? {
        let (fee, overflow) = gasLimit.multipliedReportingOverflow(by: maxFeePerGas)
        guard let cap = Self.feeCap[chainId], !overflow, fee <= cap else {
            return "A network fee of \(feeText) is more than ihatecrypto signs on \(EVMRPC.names[chainId] ?? "network \(chainId)")."
        }
        return nil
    }

    /// «up to 0.000042 ETH»: the fee ceiling, for the Touch ID prompt.
    public var feeText: String {
        let (fee, overflow) = gasLimit.multipliedReportingOverflow(by: maxFeePerGas)
        return overflow ? "over 18 \(EVMRPC.coin(chainId))" : "up to \(NSDecimalNumber(decimal: Decimal(fee) / pow(10, 18)).stringValue) \(EVMRPC.coin(chainId))"
    }
}

public enum ERC20 {
    /// transfer(address,uint256)
    public static func transfer(to: String, amount: UInt64) throws -> Data {
        guard let recipient = Data(hex: to), recipient.count == 20 else { throw WalletError("Invalid address: \(to)") }
        return Data([0xa9, 0x05, 0x9c, 0xbb]) + Data(count: 12) + recipient + Data(count: 24) + withUnsafeBytes(of: amount.bigEndian) { Data($0) }
    }

    /// The same for amounts past 64 bits (a thousand tokens of 18 decimals), in the token's smallest units.
    public static func transfer(to: String, units: Decimal) throws -> Data {
        Data([0xa9, 0x05, 0x9c, 0xbb]) + (try ABI.address(to)) + (try ABI.word(units))
    }

    /// balanceOf(address)
    public static func balanceOf(_ owner: String) throws -> Data {
        guard let who = Data(hex: owner), who.count == 20 else { throw WalletError("Invalid address: \(owner)") }
        return Data([0x70, 0xa0, 0x82, 0x31]) + Data(count: 12) + who
    }

    /// approve(address,uint256) — always an exact amount; the guard refuses unlimited ones anyway.
    public static func approve(spender: String, amount: UInt64) throws -> Data {
        Data([0x09, 0x5e, 0xa7, 0xb3]) + (try ABI.address(spender)) + ABI.uint(amount)
    }

    /// The same for amounts past 64 bits (a thousand tokens of 18 decimals), in the token's smallest units.
    public static func approve(spender: String, units: Decimal) throws -> Data {
        Data([0x09, 0x5e, 0xa7, 0xb3]) + (try ABI.address(spender)) + (try ABI.word(units))
    }

    /// allowance(address owner, address spender)
    public static func allowance(owner: String, spender: String) throws -> Data {
        Data([0xdd, 0x62, 0xed, 0x3e]) + (try ABI.address(owner)) + (try ABI.address(spender))
    }

    /// transferWithMemo(address,uint256,bytes32): Tempo's TIP-20 transfer carrying a reference the payee asked for.
    public static func transfer(to: String, amount: UInt64, memo: Data) throws -> Data {
        guard memo.count == 32 else { throw WalletError("A memo is 32 bytes.") }
        return Data([0x95, 0x77, 0x7d, 0x59]) + (try ABI.address(to)) + ABI.uint(amount) + memo
    }
}

/// Tempo's fee rules: a plain transfer of a TIP-20 stablecoin pays its fee in that coin; any other call pays in the
/// account's chosen fee token, else pathUSD — which this wallet doesn't hold. So before its first other call (an approve,
/// Relay's deposit) the wallet picks USDC once; that call itself pays in the coin it picks.
public enum Tempo {
    /// The FeeManager precompile.
    public static let feeManager = "0xfeec000000000000000000000000000000000000"

    /// setUserToken(address)
    public static func setUserToken(_ token: String) throws -> Data { Data([0xe7, 0x89, 0x74, 0x44]) + (try ABI.address(token)) }

    /// userTokens(address): the fee token `owner` chose; the zero address when none.
    public static func userToken(_ owner: String) throws -> Data { Data([0xed, 0x49, 0x8f, 0xa8]) + (try ABI.address(owner)) }

    /// A fee in a 6-decimal stablecoin's units from gas × price in attodollars: ceil(x / 10¹²).
    public static func feeUnits(_ attodollars: UInt64) -> UInt64 { (attodollars + 999_999_999_999) / 1_000_000_000_000 }

    /// Tempo's own stablecoin exchange (a precompile): an order book between TIP-20 dollars, each pair routed through
    /// the tokens' quote tokens; a swap pays the caller (tempo.xyz/developers/docs/protocol/exchange, 2026-10-06).
    public static let exchange = "0xdec0000000000000000000000000000000000000"
    /// The dollars it trades that ihatecrypto names, 6 decimals each. USDT0 had no liquidity there (2026-10-06).
    public static let stables = ["USDC": "0x20c000000000000000000000b9537d11c60e8b50", "PATHUSD": "0x20c0000000000000000000000000000000000000",
                                 "OUSD": "0x20c0000000000000000000006a37da5c996874be", "USDT0": "0x20c00000000000000000000014f22ca97301eb73"]

    /// quoteSwapExactAmountIn(tokenIn, tokenOut, amountIn) → amountOut.
    public static func quote(_ tokenIn: String, _ tokenOut: String, units: UInt64) throws -> Data {
        Data([0xe7, 0xc9, 0x8f, 0x1a]) + (try ABI.address(tokenIn)) + (try ABI.address(tokenOut)) + ABI.uint(units)
    }

    /// swapExactAmountIn(tokenIn, tokenOut, amountIn, minAmountOut): what comes out goes to the caller.
    public static func swap(_ tokenIn: String, _ tokenOut: String, units: UInt64, minOut: UInt64) throws -> Data {
        Data([0xf8, 0x85, 0x6c, 0x0f]) + (try ABI.address(tokenIn)) + (try ABI.address(tokenOut)) + ABI.uint(units) + ABI.uint(minOut)
    }
}

/// 32-byte ABI words.
public enum ABI {
    public static func address(_ a: String) throws -> Data {
        guard let bytes = Data(hex: a), bytes.count == 20 else { throw WalletError("Invalid address: \(a)") }
        return Data(count: 12) + bytes
    }

    public static func uint(_ n: UInt64) -> Data { Data(count: 24) + withUnsafeBytes(of: n.bigEndian) { Data($0) } }

    /// A whole, non-negative number as a 256-bit word. Decimal keeps 38 digits: exact for any token amount a wallet
    /// holds (10²⁷ units is a billion tokens of 18 decimals).
    public static func word(_ n: Decimal) throws -> Data {
        var x = n, bytes = [UInt8]()
        guard x >= 0, x == x.whole else { throw WalletError("Not a whole token amount: \(n)") }
        while x > 0 {
            let q = (x / 256).whole
            bytes.insert(UInt8(truncating: NSDecimalNumber(decimal: x - q * 256)), at: 0)
            x = q
        }
        guard bytes.count <= 32 else { throw WalletError("Amount too large for a word: \(n)") }
        return Data(count: 32 - bytes.count) + bytes
    }

    /// ERC-20 `string` return value (symbol(), name()).
    public static func string(_ data: Data) -> String? {
        let data = Data(data)
        guard data.count >= 64, let length = uint(in: data, word: 1), length <= UInt64(data.count), data.count >= 64 + Int(length) else { return nil }
        return String(data: data.subdata(in: 64..<(64 + Int(length))), encoding: .utf8)
    }

    /// Word `i` of a return value as an address.
    public static func address(in data: Data, word i: Int) -> String? {
        let data = Data(data)   // a slice keeps its parent's indices
        return data.count >= (i + 1) * 32 ? "0x" + data.subdata(in: (i * 32 + 12)..<((i + 1) * 32)).hex : nil
    }

    /// A `bytes[]` argument whose offset sits at byte `at` of the arguments; nil when any part runs out of the data.
    public static func bytesArray(_ args: Data, at: Int) -> [Data]? {
        let data = Data(args)   // a slice keeps its parent's indices
        func number(_ offset: Int) -> Int? {
            guard offset >= 0, offset % 32 == 0, let n = uint(in: data, word: offset / 32), n < UInt64(data.count) else { return nil }
            return Int(n)
        }
        // Offsets inside a bytes[] are 32-byte aligned; a misaligned or oversized one is someone's crafted data.
        guard let base = number(at), let count = number(base), count <= 64 else { return nil }
        var out: [Data] = []
        for i in 0..<count {
            guard let relative = number(base + 32 + i * 32), let length = number(base + 32 + relative) else { return nil }
            let start = base + 64 + relative
            guard start + length <= data.count else { return nil }
            out.append(data.subdata(in: start..<(start + length)))
        }
        return out
    }

    /// Word `i` as a number, nil when it doesn't fit 64 bits.
    public static func uint(in data: Data, word i: Int) -> UInt64? {
        let data = Data(data)   // a slice keeps its parent's indices
        guard data.count >= (i + 1) * 32, data.subdata(in: (i * 32)..<(i * 32 + 24)).allSatisfy({ $0 == 0 }) else { return nil }
        return data.subdata(in: (i * 32 + 24)..<((i + 1) * 32)).reduce(0) { $0 << 8 | UInt64($1) }
    }

    /// Word `i` as a Decimal (for 256-bit values such as Aave's ray rates).
    public static func decimal(in data: Data, word i: Int) -> Decimal? {
        let data = Data(data)   // a slice keeps its parent's indices
        guard data.count >= (i + 1) * 32 else { return nil }
        return data.subdata(in: (i * 32)..<((i + 1) * 32)).reduce(Decimal(0)) { $0 * 256 + Decimal($1) }
    }
}

/// Ethereum JSON-RPC for one chain.
public struct EVMRPC: Sendable {
    public let url: URL
    public let chainId: UInt64
    /// Asked in turn when `url` throttles or drops a call: each retry goes to the next node instead of waiting on the
    /// same one. base.drpc.org answered «You reached Public endpoint rate limit» to every call for minutes, and each read
    /// sat through 15 s of retries before failing (a spread took 50 s and lost the Base vaults).
    public let fallbacks: [URL]
    /// Where signed transactions go instead, when set: a private relay that keeps them out of the public mempool.
    public let sendURL: URL?
    /// The app's own node for its network: the user's node set in Settings (EVMNodes) is asked before `url`. A node made
    /// for one URL (a fork, a check) never is.
    public let followsSettings: Bool

    public init(url: URL, chainId: UInt64, fallbacks: [URL] = [], sendURL: URL? = nil, followsSettings: Bool = false) {
        self.url = url
        self.chainId = chainId
        self.fallbacks = fallbacks
        self.sendURL = sendURL
        self.followsSettings = followsSettings
    }

    /// Every node a call may go to, in turn: the user's own (when this follows Settings), then `url`, then the fallbacks.
    public var nodes: [URL] { (followsSettings ? EVMNodes.custom(chainId).map { [$0] } ?? [] : []) + [url] + fallbacks }

    /// A node that can't answer this call at all — no such method, a key it turns away — while another node may: the call
    /// goes on to the next one. A revert is never that.
    static func notThisNode(_ message: String) -> Bool {
        let m = message.lowercased()
        guard !m.contains("revert") else { return false }
        return ["method not found", "not supported", "unsupported method", "method not allowed", "not whitelisted", "unauthorized",
                "invalid api key", "api key", "forbidden", "authenticat"].contains { m.contains($0) }
    }

    /// The node's «insufficient funds for gas * price + value: address 0x… have 120 want 340» (geth, Arbitrum) or «EVM
    /// error: OutOfFunds» (reth: Base, measured 2026-10-06) in words, with what to do; nil for anything else (a token's
    /// own revert included).
    static func shortOfGas(_ message: String, chainId: UInt64) -> String? {
        let m = message.lowercased()
        guard m.contains("insufficient funds") || m.contains("outoffunds"), !m.contains("revert") else { return nil }
        let coin = coin(chainId)
        // Wei, so 18 decimals; Tempo's fee is in a USD token, said without amounts.
        func amount(_ pattern: Regex<(Substring, Substring)>) -> String? {
            guard coin != "USD", let match = m.firstMatch(of: pattern), let wei = Decimal(string: String(match.1)) else { return nil }
            return "\(wei / 1_000_000_000_000_000_000) \(coin)"
        }
        let said = [amount(/have (\d+)/).map { "has \($0)" }, amount(/want (\d+)/).map { "needs \($0) with the fee" }].compactMap { $0 }
        return "Not enough \(coin) for gas: the wallet \(said.isEmpty ? "can't pay this network's fee" : said.joined(separator: " and "))."
            + " Send it a little \(coin), then try again."
    }

    /// Fallbacks (2026-10-05): each served calls, estimates with a state override, fees, nonces, receipts and
    /// eth_simulateV1, and 30 calls at once — a node without one of them would turn a retry into a failure. Not used:
    /// 1rpc (a daily cap: «You've reached the usage limit» after an hour of checks), publicnode on Arbitrum and Base
    /// (refuses receipts) and Polygon (timeouts). Solana has no free node that serves token accounts too: it keeps its one.
    public static let arbitrum = EVMRPC(url: URL(string: "https://arb1.arbitrum.io/rpc")!, chainId: 42161,
                                        fallbacks: [URL(string: "https://arbitrum.drpc.org")!], followsSettings: true)
    public static let arbitrumUSDC = "0xaf88d065e77c8cc2239327c5edb3a432268e5831"
    public static let robinhood = EVMRPC(url: URL(string: "https://rpc.mainnet.chain.robinhood.com")!, chainId: 4663,
                                         fallbacks: [URL(string: "https://robinhood.drpc.org")!], followsSettings: true)
    /// USDG (Global Dollar), Robinhood Chain's dollar, 6 decimals — docs.robinhood.com/chain/contracts.
    public static let robinhoodUSDG = "0x5fc5360d0400a0fd4f2af552add042d716f1d168"
    /// mainnet.base.org rate-limits after a few calls and publicnode refuses receipts without a token;
    /// drpc served calls, estimates and receipts in a burst test (2026-09-26).
    /// Coinbase's own node takes over while drpc throttles (it limits too, so it isn't the first).
    public static let base = EVMRPC(url: URL(string: "https://base.drpc.org")!, chainId: 8453,
                                    fallbacks: [URL(string: "https://mainnet.base.org")!, URL(string: "https://base.gateway.tenderly.co")!],
                                    followsSettings: true)
    /// Circle's native USDC on Base (6 decimals).
    public static let baseUSDC = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
    /// Polygon PoS, gas in POL; drpc serves calls, estimates and receipts (checked 2026-09-29).
    public static let polygon = EVMRPC(url: URL(string: "https://polygon.drpc.org")!, chainId: 137, fallbacks: [URL(string: "https://polygon.gateway.tenderly.co")!],
                                       followsSettings: true)
    /// Circle's native USDC on Polygon, not the bridged USDC.e.
    public static let polygonUSDC = "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359"
    /// Ethereum mainnet: a supply costs dollars of gas here, so the planners weigh it before choosing it.
    /// Its transactions go through MEV Blocker (rpc.mevblocker.io): out of the public mempool, so a swap can't be
    /// front-run or sandwiched; it simulates each first and refuses one that would revert, unpaid; it takes type-4
    /// (EIP-7702) transactions — Flashbots Protect took an unfunded one without a word (checked 2026-10-05).
    public static let ethereum = EVMRPC(url: URL(string: "https://eth.drpc.org")!, chainId: 1,
                                        fallbacks: [URL(string: "https://ethereum-rpc.publicnode.com")!],
                                        sendURL: URL(string: "https://rpc.mevblocker.io")!, followsSettings: true)
    public static let ethereumUSDC = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    /// Tempo, Stripe's payments chain: no coin of its own, fees come out of a USD stablecoin (Tempo.feeManager), and
    /// eth_getBalance answers a placeholder (4242…42) for every address, so it's never read there (checked 2026-09-30).
    public static let tempo = EVMRPC(url: URL(string: "https://rpc.mainnet.tempo.xyz")!, chainId: 4217, followsSettings: true)
    /// «Bridged USDC (Stargate)», 6 decimals: the USDC Relay delivers and MPP services charge in.
    public static let tempoUSDC = "0x20c000000000000000000000b9537d11c60e8b50"

    /// HyperEVM, Hyperliquid's EVM (chain 999), gas in HYPE at 0.1 gwei: drpc first (the official node allows 100 calls a
    /// minute an address), then Hyperliquid's own and Hypurrscan's; each served fees and estimates (checked 2026-10-06).
    public static let hyperevm = EVMRPC(url: URL(string: "https://hyperliquid.drpc.org")!, chainId: 999,
                                        fallbacks: [URL(string: "https://rpc.hyperliquid.xyz/evm")!, URL(string: "https://rpc.hypurrscan.io")!],
                                        followsSettings: true)
    /// Circle's native USDC on HyperEVM, 6 decimals: the one Relay delivers (its /chains, 2026-10-06).
    public static let hyperevmUSDC = "0xb88339cb7199b77e23db6e890353e22632ba630f"

    /// Optimism (OP Mainnet), gas in ETH at ≈ 0.001 gwei plus its L1 data fee; drpc, then Optimism's own node (2026-10-06).
    public static let optimism = EVMRPC(url: URL(string: "https://optimism.drpc.org")!, chainId: 10,
                                        fallbacks: [URL(string: "https://mainnet.optimism.io")!], followsSettings: true)
    /// Circle's native USDC on Optimism, the one Relay delivers.
    public static let optimismUSDC = "0x0b2c639c533813f4aa9d7837caf62653d097ff85"

    /// BNB Chain (BSC), gas in BNB at 0.05 gwei: drpc, then publicnode and BNB Chain's own seed node (each answered
    /// chain id 56 and fees, 2026-10-08).
    public static let bnb = EVMRPC(url: URL(string: "https://bsc.drpc.org")!, chainId: 56,
                                   fallbacks: [URL(string: "https://bsc-rpc.publicnode.com")!, URL(string: "https://bsc-dataseed.bnbchain.org")!],
                                   followsSettings: true)
    /// Binance-Peg USDC on BNB Chain, the one Relay delivers — 18 decimals, not 6 (its decimals(), 2026-10-08).
    public static let bnbUSDC = "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d"

    /// The only networks the signer signs for: the app's signing process and the CLI check the same list.
    public static let signable: Set<UInt64> = [1, 137, 42161, 8453, 4663, 4217, 999, 10, 56]

    /// The networks by name, for what the signer says it signs.
    public static let names: [UInt64: String] = [1: "Ethereum", 137: "Polygon", 42161: "Arbitrum", 8453: "Base", 4663: "Robinhood Chain", 4217: "Tempo",
                                                 999: "HyperEVM", 10: "Optimism", 56: "BNB Chain"]
    /// What a network's fees and `value` are counted in: Polygon's POL, HyperEVM's HYPE, BNB Chain's BNB, Tempo's dollars
    /// (it has no coin), else ETH.
    public static func coin(_ chainId: UInt64) -> String {
        chainId == 137 ? "POL" : chainId == 999 ? "HYPE" : chainId == 56 ? "BNB" : chainId == 4217 ? "USD" : "ETH"
    }
    /// The dollars ihatecrypto moves, by contract: their symbol for the Touch ID prompt.
    public static let dollars: [String: String] = [baseUSDC: "USDC", arbitrumUSDC: "USDC", polygonUSDC: "USDC", ethereumUSDC: "USDC",
                                                   tempoUSDC: "USDC", robinhoodUSDG: "USDG", hyperevmUSDC: "USDC", optimismUSDC: "USDC", bnbUSDC: "USDC"]
    /// Their decimals: 6, but BNB Chain's Binance-Peg USDC has 18 — the prompt would say a trillion times the amount.
    public static func dollarDecimals(_ token: String) -> Int16 { token.lowercased() == bnbUSDC ? 18 : 6 }

    /// eth_call: the raw return value.
    public func read(_ to: String, _ data: Data) async throws -> Data {
        let hex = try await call("eth_call", [["to": to, "data": "0x" + data.hex], "latest"]) as? String ?? "0x"
        return Data(hex: String(hex.dropFirst(2))) ?? Data()
    }

    public func call(_ method: String, _ params: [Any]) async throws -> Any {
        let nodes = self.nodes
        var attempt = 0
        var request = URLRequest(url: nodes[0], timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "method": method, "params": params])
        // Public nodes throttle bursts, drop connections and have bad seconds: wait and ask again, up to about half a
        // minute (reads and sends alike are safe to repeat — a resent raw transaction is the same transaction). What
        // the node answered on purpose (a revert, a short balance) is final at once.
        return try await RPCRetry.run {
            request.url = nodes[attempt % nodes.count]
            attempt += 1
            return try await URLSession.shared.data(for: request)
        } answer: { data, status in
            // A key turned away (401, 403) or a call a node doesn't serve goes on to the next node, when there is one.
            let elsewhere = nodes.count > 1
            guard let reply = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
                throw RPCRetry.Again(status >= 500 || status == 429 || (elsewhere && (status == 401 || status == 403)),
                                     WalletError("EVM RPC \(method): HTTP \(status), response isn't JSON: \(String(decoding: data.prefix(120), as: UTF8.self))"))
            }
            if let error = reply["error"] as? [String: Any] {
                let message = error["message"] as? String ?? "\(error)"
                if let short = Self.shortOfGas(message, chainId: chainId) { throw WalletError(short) }
                throw RPCRetry.Again(status == 429 || RPCRetry.transient(message) || (elsewhere && (status == 401 || status == 403 || Self.notThisNode(message))),
                                     WalletError("EVM RPC \(method): \(message)"))
            }
            return reply["result"] ?? NSNull()
        }
    }

    public func quantity(_ method: String, _ params: [Any]) async throws -> UInt64 {
        let answer = try await call(method, params)
        guard let hex = answer as? String, let n = UInt64(hex.dropFirst(2), radix: 16) else {
            throw WalletError("EVM RPC \(method): not a number (\(String(String(describing: answer).prefix(60))))")
        }
        return n
    }

    public func balance(_ address: String) async throws -> UInt64 { try await quantity("eth_getBalance", [address, "latest"]) }
    public func nonce(_ address: String) async throws -> UInt64 { try await quantity("eth_getTransactionCount", [address, "pending"]) }

    public func tokenBalance(_ token: String, owner: String) async throws -> UInt64 {
        let hex = try await call("eth_call", [["to": token, "data": "0x" + (try ERC20.balanceOf(owner)).hex], "latest"]) as? String ?? "0x0"
        return UInt64(String(hex.dropFirst(2).drop(while: { $0 == "0" })), radix: 16) ?? 0
    }

    /// Current fees with headroom: max fee = 2 × base fee + tip.
    public func fees() async throws -> (maxPriority: UInt64, maxFee: UInt64) {
        let base = try await quantity("eth_gasPrice", [])
        let tip = (try? await quantity("eth_maxPriorityFeePerGas", [])) ?? 0
        return (tip, base * 2 + tip)
    }

    /// eth_estimateGas as a dry run: fails if the call would revert or the balance is short.
    /// `runningCodeOf`: the dry run as if `from` were already delegated to that contract (EIP-7702), by a state override
    /// of its code — every node ihatecrypto uses takes one; drpc drops a transaction's authorizationList from an estimate.
    public func estimateGas(from: String, to: String, value: UInt64, data: Data, runningCodeOf delegate: String? = nil) async throws -> UInt64 {
        let tx: [String: String] = ["from": from, "to": to, "value": "0x" + String(value, radix: 16), "data": "0x" + data.hex]
        guard let delegate else { return try await quantity("eth_estimateGas", [tx]) }
        return try await quantity("eth_estimateGas", [tx, "latest", [from: ["code": "0xef0100" + delegate.dropFirst(2).lowercased()]]])
    }

    public func send(_ raw: Data) async throws -> String {
        let node = sendURL.map { EVMRPC(url: $0, chainId: chainId) } ?? self
        do {
            guard let hash = try await node.call("eth_sendRawTransaction", ["0x" + raw.hex]) as? String else { throw WalletError("No transaction hash") }
            return hash
        } catch let error as WalletError where error.message.lowercased().contains("already known") || error.message.lowercased().contains("known transaction") {
            // A retry after the first send got through: it's the same transaction, waiting for its block.
            return "0x" + Keccak.hash256(raw).hex
        }
    }

    /// Runs the transaction on the latest block without sending it (eth_simulateV1): what `from` gains and loses.
    public func simulate(from: String, to: String, value: UInt64, data: Data) async throws -> EVMSimulation {
        try await simulate(from: from, calls: [(to, value, data)])
    }

    /// Several calls one after another in one block (an approve, then the call that spends it): the changes of all of
    /// them together, or the first one's revert.
    public func simulate(from: String, calls: [(to: String, value: UInt64, data: Data)]) async throws -> EVMSimulation {
        let txs: [[String: Any]] = calls.map { ["from": from, "to": $0.to, "value": "0x" + String($0.value, radix: 16), "data": "0x" + $0.data.hex] }
        return try EVMSimulation(try await call("eth_simulateV1", [["blockStateCalls": [["calls": txs]], "traceTransfers": true], "latest"]), owner: from)
    }

    /// Waits for a receipt; true if the transaction succeeded.
    public func waitForReceipt(_ hash: String, timeout: TimeInterval = 120) async throws -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let receipt = try await call("eth_getTransactionReceipt", [hash]) as? [String: Any], let status = receipt["status"] as? String {
                return status == "0x1"
            }
            try await Task.sleep(for: .seconds(1))
        }
        throw WalletError("No receipt after \(Int(timeout)) s: \(hash)")
    }
}

extension Decimal {
    /// Rounded down to a whole number.
    public var whole: Decimal {
        var x = self, out = Decimal()
        NSDecimalRound(&out, &x, 0, .down)
        return out
    }
}

/// One eth_simulateV1 result, read as one address's balance changes. With traceTransfers the node reports native
/// coin moves as Transfer logs of 0xeeee…eeee, which no contract can emit. Token moves are their contracts' own Transfer
/// logs (plus WETH9's Deposit/Withdrawal, which emit none) — a contract can log anything, so a token is shown by its address.
public struct EVMSimulation: Equatable, Sendable {
    public static let native = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    /// WETH9 on Ethereum and Base; Arbitrum's WETH logs a Transfer on wrap.
    /// WETH on Ethereum and the OP chains, WHYPE on HyperEVM, WBNB on BNB Chain (the same WETH9 code).
    static let weth9: Set<String> = ["0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "0x4200000000000000000000000000000000000006",
                                     "0x5555555555555555555555555555555555555555", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"]
    static let transfer = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    static let deposit = "0xe1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c"
    static let withdrawal = "0x7fcf532c15f0a6db0bd6d0e038bea71d30d808c7d98cb3bf7268a95bf5081b65"

    /// Why the call reverts; nil when it succeeds.
    public var revert: String?
    /// Net change per asset contract (`native` for the coin), in raw units; an NFT collection counts items. No zeros.
    public var changes: [String: Decimal] = [:]
    /// ERC-721 collections among `changes`.
    public var nfts: Set<String> = []
    /// Rights this wallet hands out in the simulated calls, read from the events standard contracts emit: an approve, all
    /// NFTs, Permit2, an operator, Morpho's authorization, Aave's credit delegation, a contract's ownership. A balance
    /// that doesn't move can still be given away: this is what the card's balances can't show.
    public var grants: [Grant] = []

    public struct Grant: Equatable, Sendable {
        /// approve, nft, all, permit2, operator, authorization, credit, ownership
        public let kind: String
        /// The contract that logged it (the token, Permit2, Morpho…).
        public let asset: String
        /// Who gets the right.
        public let to: String
        public let unlimited: Bool
        public init(kind: String, asset: String, to: String, unlimited: Bool) { (self.kind, self.asset, self.to, self.unlimited) = (kind, asset, to, unlimited) }
    }

    /// (event, kind, topic of the owner, topic of whoever gets the right; nil = word 0 of the data says how much or whether)
    static let grantEvents: [(topic: String, kind: String, owner: Int, to: Int)] = [
        ("Approval(address,address,uint256)", "approve", 1, 2), ("ApprovalForAll(address,address,bool)", "all", 1, 2),
        ("Approval(address,address,address,uint160,uint48)", "permit2", 1, 3), ("OperatorSet(address,address,bool)", "operator", 1, 2),
        ("SetAuthorization(address,address,address,bool)", "authorization", 2, 3),
        ("BorrowAllowanceDelegated(address,address,address,uint256)", "credit", 1, 2), ("OwnershipTransferred(address,address)", "ownership", 1, 2),
    ].map { ("0x" + Keccak.hash256(Data($0.0.utf8)).hex, $0.1, $0.2, $0.3) }

    public init(_ result: Any, owner: String) throws {
        guard let calls = (result as? [[String: Any]])?.first?["calls"] as? [[String: Any]], !calls.isEmpty else {
            throw WalletError("eth_simulateV1: unexpected reply")
        }
        if let failed = calls.firstIndex(where: { $0["status"] as? String != "0x1" }) {
            let why = (calls[failed]["error"] as? [String: Any])?["message"] as? String ?? "reverted"
            revert = calls.count > 1 ? "call \(failed + 1) of \(calls.count): \(why)" : why
            return
        }
        let me = owner.lowercased().dropFirst(2)
        for log in calls.flatMap({ $0["logs"] as? [[String: Any]] ?? [] }) {
            guard let asset = (log["address"] as? String)?.lowercased(), let topics = log["topics"] as? [String], let event = topics.first else { continue }
            if let g = Self.grant(asset: asset, topics: topics.map { $0.lowercased() }, data: Data(hex: log["data"] as? String ?? "0x") ?? Data(), me: String(me)) {
                grants.append(g)
            }
            let amount = Data(hex: log["data"] as? String ?? "0x").flatMap { ABI.decimal(in: $0, word: 0) } ?? 0
            var from = "", to = "", n = amount
            switch (event, topics.count) {
            case (Self.transfer, 3): (from, to) = (topics[1], topics[2])
            case (Self.transfer, 4): (from, to, n) = (topics[1], topics[2], 1); nfts.insert(asset)
            case (Self.deposit, 2) where Self.weth9.contains(asset): to = topics[1]
            case (Self.withdrawal, 2) where Self.weth9.contains(asset): from = topics[1]
            default: continue
            }
            if from.lowercased().hasSuffix(me) { changes[asset, default: 0] -= n }
            if to.lowercased().hasSuffix(me) { changes[asset, default: 0] += n }
        }
        changes = changes.filter { $0.value != 0 }
    }

    /// A right `me` gives in this log, or nil (another owner, or a revoke: zero, false).
    static func grant(asset: String, topics: [String], data: Data, me: String) -> Grant? {
        guard let e = grantEvents.first(where: { $0.topic == topics.first }), topics.count > max(e.owner, e.to), topics[e.owner].hasSuffix(me) else { return nil }
        let to = "0x" + topics[e.to].suffix(40)
        // An ERC-721 approve carries its token id as a fourth topic: one NFT, no amount in the data.
        if e.kind == "approve", topics.count == 4 { return Grant(kind: "nft", asset: asset, to: to, unlimited: false) }
        if e.kind == "ownership" { return Grant(kind: e.kind, asset: asset, to: to, unlimited: true) }
        let word = data.count >= 32 ? Data(data.prefix(32)) : Data(count: 32)
        guard word.contains(where: { $0 != 0 }) else { return nil }
        // A bool says only yes; an amount past 2^128 is as good as unlimited (max uint256, max uint160).
        let flag = ["all", "operator", "authorization"].contains(e.kind)
        return Grant(kind: e.kind, asset: asset, to: to, unlimited: flag || word.prefix(16).contains { $0 != 0 })
    }
}

/// What an EVM transaction's calldata asks for, as far as token safety goes. The drainers' tools — unlimited
/// approvals, operator rights over all NFTs, permit calls, pulling tokens out of the sender — are refused.
public enum EVMCall: Equatable, Sendable {
    /// Empty data: a plain transfer of the native coin.
    case plain
    case approve(spender: String, amount: Data)
    case increaseAllowance(spender: String, amount: Data)
    case transfer(to: String, amount: Data)
    case transferFrom(from: String, to: String, amount: Data)
    case setApprovalForAll(operator: String, approved: Bool)
    case permit
    /// Any other contract function, by selector.
    /// A Uniswap swap (v3's SwapRouter02 exactInput/exactInputSingle, v2's Router02): who gets what comes out.
    case swap(recipient: String)
    /// An ERC-4626 vault (Morpho): who gets the shares or the assets, and whose shares are spent (nil for a deposit).
    case vault(receiver: String, owner: String?)
    /// A lending pool (Aave's supply, deposit or withdraw): who gets the aTokens or the withdrawn asset.
    case lending(beneficiary: String)
    /// Merkl's Distributor.claim: whom each reward is paid to.
    case claim(recipients: [String])
    /// Permit2.approve: lets `spender` pull up to `amount` of `token` until `expiration` (unix seconds).
    case permit2Approve(token: String, spender: String, amount: Data, expiration: UInt64)
    /// The Universal Router's v4 swap in exactly this app's shape (UniswapV4.decodeExecute): it pays the caller.
    case v4Swap(UniswapV4.Swap)
    /// KyberSwap's router swap (KyberSwap.decode): who gets what comes out, and who else takes a fee.
    case kyberSwap(KyberSwap.Swap)
    /// ERC-721 or ERC-1155 safeTransferFrom / safeBatchTransferFrom: NFTs (or game items) from `from` to `to`.
    case nftTransfer(from: String, to: String)
    /// Aave's approveDelegation or Morpho Blue's setAuthorization: `to` may borrow against, or manage, this wallet's positions.
    case delegation(to: String, granted: Bool)
    /// WETH9.deposit(): the coins sent with it come back to the sender as the wrapped token.
    case wrap
    /// WETH9.withdraw(amount): that much of the wrapped token back to the sender as the coin.
    case unwrap(amount: Data)
    /// Circle's CCTP burn: `amount` of USDC minted on `domain` to `recipient` (nil: not an EVM address).
    case cctpBurn(recipient: String?, domain: UInt32, amount: Data)
    /// A LayerZero OFT's send (Tether's USDT0): `amount` of the token leaves for `recipient` on the network `eid` (nil: not
    /// an EVM address, or unreadable); `plain`: no options, compose message or command with it; `refund` gets back what
    /// LayerZero doesn't take of the fee sent as the call's value.
    case oftSend(recipient: String?, eid: UInt32, amount: Data, plain: Bool, refund: String)
    /// Aave's borrow: `amount` of `asset` sent to the caller, the debt on `onBehalfOf`; its repay pays down that debt.
    case borrow(asset: String, amount: Data, onBehalfOf: String)
    case repay(asset: String, amount: Data, onBehalfOf: String)
    /// Safe's execTransaction: the multisig at the call's `to` makes this move (to, value wei as a word, data), signed by its owners.
    case multisigExec(to: String, value: Data, data: Data, operation: UInt8)
    case contract(selector: String)

    public static func decode(_ data: Data) -> EVMCall {
        // Calldata shorter than a selector still runs a contract's fallback: only none at all is a plain send.
        guard data.count >= 4 else { return data.isEmpty ? .plain : .contract(selector: "0x" + data.hex) }
        if let e = Safe.readExec(data) { return .multisigExec(to: e.to, value: e.value, data: e.data, operation: e.operation) }
        let selector = data.prefix(4).hex
        let args = data.dropFirst(4)
        func word(_ i: Int) -> Data? { args.count >= (i + 1) * 32 ? Data(args.dropFirst(i * 32).prefix(32)) : nil }
        func address(_ i: Int) -> String? { word(i).map { "0x" + $0.suffix(20).hex } }
        // A swap paying word i's address. `literal`: a router that reads 0x…01 as address 1, not as the caller (only
        // SwapRouter02 does): the output would be lost, so it isn't read as a swap paying this wallet.
        func payee(_ i: Int, literal: Bool = false) -> EVMCall? {
            guard let r = address(i), !literal || r.lowercased() != Self.msgSender else { return nil }
            return .swap(recipient: r)
        }
        switch selector {
        case "095ea7b3": if let s = address(0), let a = word(1) { return .approve(spender: s, amount: a) }
        case "39509351": if let s = address(0), let a = word(1) { return .increaseAllowance(spender: s, amount: a) }
        // Tempo's transferWithMemo pays like transfer: the same recipient check applies.
        case "a9059cbb", "95777d59": if let t = address(0), let a = word(1) { return .transfer(to: t, amount: a) }
        case "23b872dd": if let f = address(0), let t = address(1), let a = word(2) { return .transferFrom(from: f, to: t, amount: a) }
        case "a22cb465": if let o = address(0), let b = word(1) { return .setApprovalForAll(operator: o, approved: b.last == 1) }
        case "d505accf", "8fcbaf0c": return .permit
        // Uniswap v3's exactInputSingle and exactOutputSingle, SwapRouter02's and the first SwapRouter's (with a deadline):
        // a tuple of plain fields, the recipient fourth.
        case "04e45aaf", "5023b4df": if let swap = payee(3) { return swap }
        case "414bf389", "db3e2198": if let swap = payee(3, literal: true) { return swap }
        // exactInput and exactOutput((bytes path, address recipient, …)), both routers: the tuple sits behind an offset,
        // read only where the app's own encoder puts it (0x20). A site could point it elsewhere: the router would pay the
        // recipient there while word 2 named this wallet.
        case "b858183f", "09b81346": if ABI.uint(in: Data(args), word: 0) == 32, let swap = payee(2) { return swap }
        case "c04b8d59", "f28c0498": if ABI.uint(in: Data(args), word: 0) == 32, let swap = payee(2, literal: true) { return swap }
        // Uniswap v2's Router02: swapExactTokensForTokens, …ForETH, swapTokensForExactTokens, …ForExactETH and the
        // fee-on-transfer forms, all (amount, amount, path, to, deadline); SwapRouter02's v2 swaps, (amount, amount, path,
        // to): `to` gets what comes out.
        case "38ed1739", "5c11d795", "18cbafe5", "791ac947", "8803dbee", "4a25d94a": if let swap = payee(3, literal: true) { return swap }
        case "472b43f3", "42712a67": if let swap = payee(3) { return swap }
        // The ETH-in forms, (amount, path, to, deadline): swapExactETHForTokens, swapETHForExactTokens, the fee-on-transfer one.
        case "7ff36ab5", "fb3bdb41", "b6f9de95": if let swap = payee(2, literal: true) { return swap }
        // SwapRouter02's multicall(bytes[]), with a deadline or a previous block hash: read only when every call in it is
        // a swap or a payout this decoder knows; then it is the swap paying whoever its calls pay.
        case "ac9650d8", "5ae401dc", "1f0464d1":
            if let r = Self.multicallRecipient(Data(args), arrayWord: selector == "ac9650d8" ? 0 : 1) { return .swap(recipient: r) }
        case "6e553f65": if let r = address(1) { return .vault(receiver: r, owner: nil) }
        case "b460af94", "ba087652": if let r = address(1), let o = address(2) { return .vault(receiver: r, owner: o) }
        // Aave: supply/deposit(asset, amount, onBehalfOf, referral) and withdraw(asset, amount, to): the third word.
        case "617ba037", "e8eda9df", "69328dec": if let b = address(2) { return .lending(beneficiary: b) }
        case "71ee95c0": if let r = Merkl.recipients(data) { return .claim(recipients: r) }
        case "87517c45": if let t = address(0), let s = address(1), let a = word(2), let e = ABI.uint(in: Data(args), word: 3) { return .permit2Approve(token: t, spender: s, amount: a, expiration: e) }
        // Any other Universal Router call stays a plain contract call (a site's own swap), read as before.
        case "3593564c": if let swap = UniswapV4.decodeExecute(data) { return .v4Swap(swap) }
        case "e21fd0e9": if let swap = KyberSwap.decode(data) { return .kyberSwap(swap) }
        case "42842e0e", "b88d4fde", "f242432a", "2eb2c2d6": if let f = address(0), let t = address(1) { return .nftTransfer(from: f, to: t) }
        case "c04a8a10": if let d = address(0), let a = word(1) { return .delegation(to: d, granted: a.contains { $0 != 0 }) }
        case "eecea000": if let d = address(0), let b = word(1) { return .delegation(to: d, granted: b.contains { $0 != 0 }) }
        case "d0e30db0": return .wrap
        case "2e1a7d4d": if let a = word(0) { return .unwrap(amount: a) }
        case "8e0250ee", "779b432d": if let b = CCTP.readBurn(data) { return .cctpBurn(recipient: b.recipient, domain: b.domain, amount: b.amount) }
        // A send that can't be read is still a send: it's refused, not signed as some contract's call.
        case "c7c7f5b3":
            let s = USDT0.readSend(data)
            return .oftSend(recipient: s?.recipient, eid: s?.eid ?? 0, amount: s?.amount ?? Data(count: 32), plain: s?.plain ?? false, refund: s?.refund ?? "")
        case "a415bcad": if let a = address(0), let x = word(1), let o = address(4) { return .borrow(asset: a, amount: x, onBehalfOf: o) }
        case "573ade81": if let a = address(0), let x = word(1), let o = address(3) { return .repay(asset: a, amount: x, onBehalfOf: o) }
        default: break
        }
        return .contract(selector: "0x" + selector)
    }

    /// SwapRouter02's stand-ins for a recipient: MSG_SENDER is the caller, ADDRESS_THIS the router itself, which holds
    /// the output for a later call of the same multicall to pay out.
    public static let msgSender = "0x0000000000000000000000000000000000000001", addressThis = "0x0000000000000000000000000000000000000002"

    /// Whom a SwapRouter02 multicall pays: the first payee that isn't the caller (so the swap check refuses it), else the
    /// caller. nil when a call in it is one this decoder doesn't know, or when anything is left in the router (a swap to
    /// ADDRESS_THIS, wrapETH, pull): whether a later payout takes that same token isn't read here, and a decoy payout
    /// (refundETH, an unwrap of nothing) would leave it for anyone to sweep. It stays a plain contract call then.
    static func multicallRecipient(_ args: Data, arrayWord: Int) -> String? {
        guard let calls = ABI.bytesArray(args, at: arrayWord * 32), !calls.isEmpty else { return nil }
        var payees: [String] = []
        for call in calls {
            guard call.count >= 4 else { return nil }
            let inner = call.dropFirst(4)
            func address(_ i: Int) -> String? { inner.count >= (i + 1) * 32 ? "0x" + Data(inner.dropFirst(i * 32).prefix(32)).suffix(20).hex : nil }
            switch call.prefix(4).hex {
            // unwrapWETH9(min, recipient), sweepToken(token, min, recipient): pay out what the router holds.
            case "49404b7c": guard let r = address(1) else { return nil }; payees.append(r)
            case "df2ab5bb": guard let r = address(2) else { return nil }; payees.append(r)
            // unwrapWETH9(min), sweepToken(token, min), refundETH(): to the caller.
            case "49616997", "e90a182f", "12210e8a": payees.append(msgSender)
            // wrapETH(value), pull(token, value): into the router, from the caller.
            case "1c58db4f", "f2d5d56b": return nil
            default:
                guard case .swap(let r) = decode(Data(call)), r.lowercased() != addressThis else { return nil }
                payees.append(r)
            }
        }
        // Read whole (the 2026-10-24 review): a multicall paying this wallet in one swap and someone else in the next names
        // two payees besides the caller, and comes back as «several payees» — never this wallet — so the swap check refuses it.
        let named = Set(payees.map { $0.lowercased() }).subtracting([msgSender])
        if named.count > 1 { return "several payees" }
        return named.first ?? msgSender
    }

    /// What the transaction does, in words for the Touch ID prompt, which reads «ihatecrypto is trying to <this>»:
    /// «send 0.185357 USDC to 0x060b…9Ff0 on Tempo». Read from the calldata by the signer itself, never from the app's
    /// description of it; a token or contract it doesn't know is named by its address.
    public func summary(chainId: UInt64, to: String, value: UInt64) -> String {
        let net = "on " + (EVMRPC.names[chainId] ?? "network \(chainId)")
        func short(_ a: String) -> String { a.count > 12 ? "\(a.prefix(6))…\(a.suffix(4))" : a }
        func units(_ word: Data, _ decimals: Int16) -> String {
            let whole = word.reduce(Decimal(0)) { $0 * 256 + Decimal($1) }
            return NSDecimalNumber(decimal: whole / pow(10, Int(decimals))).stringValue
        }
        let coins = "\(units(ABI.uint(value), 18)) \(EVMRPC.coin(chainId))"
        if case .plain = self { return "send \(coins) to \(short(to)) \(net)" }
        // A token the signer can't name shows its raw units: an amount on the prompt, never none.
        func amount(_ word: Data, of token: String) -> String {
            EVMRPC.dollars[token.lowercased()].map { "\(units(word, EVMRPC.dollarDecimals(token))) \($0)" } ?? "\(units(word, 0)) units of the token \(short(token))"
        }
        // Coins that go into a wrap or a native swap are its input, not a payment: said once, in the sentence.
        switch self {
        case .wrap: return "wrap \(coins) into W\(EVMRPC.coin(chainId)) \(net)"
        case .v4Swap where value > 0: return "swap \(coins) on Uniswap v4 \(net), paying this wallet"
        default: break
        }
        let what = switch self {
        case .transfer(let recipient, let a): "send \(amount(a, of: to)) to \(short(recipient)) \(net)"
        case .approve(let spender, let a): "let \(short(spender)) spend exactly \(amount(a, of: to)) \(net)"
        case .increaseAllowance(let spender, let a): "let \(short(spender)) spend \(amount(a, of: to)) more \(net)"
        case .swap: "swap on Uniswap \(net), paying this wallet"
        case .v4Swap: "swap on Uniswap v4 \(net), paying this wallet"
        case .kyberSwap(let k): "swap \(amount((try? ABI.word(k.amount)) ?? Data(count: 32), of: k.srcToken)) on KyberSwap \(net) for at least "
            + "\(amount((try? ABI.word(k.minReturn)) ?? Data(count: 32), of: k.dstToken)), paying this wallet"
        case .permit2Approve(let token, let spender, let a, _): "let \(short(spender)) spend exactly \(amount(a, of: token)) through Permit2 \(net)"
        case .unwrap(let a): "unwrap \(units(a, 18)) W\(EVMRPC.coin(chainId)) of \(short(to)) into \(EVMRPC.coin(chainId)) \(net)"
        case .vault(_, let owner): "\(owner == nil ? "deposit into" : "withdraw from") the vault \(short(to)) \(net)"
        case .lending: "use the lending pool \(short(to)) \(net), for this wallet"
        case .nftTransfer(_, let recipient): "send NFTs of \(short(to)) to \(short(recipient)) \(net)"
        case .cctpBurn(let recipient, let domain, let a): "move \(units(a, 6)) USDC \(net) to \(recipient.map(short) ?? "?") on \(CCTP.chainName(domain)) through Circle's CCTP"
        case .oftSend(let recipient, let eid, let a, _, _):
            "move \(USDT0.on(chainId)?.oft == to.lowercased() ? "\(units(a, 6)) USDT" : "\(units(a, 0)) units of the token of \(short(to))") \(net) "
                + "to \(recipient.map(short) ?? "?") on \(USDT0.networkName(eid: eid)) through LayerZero"
        case .borrow(let asset, let a, _): "borrow \(amount(a, of: asset)) from the lending pool \(short(to)) \(net), a debt of this wallet's"
        case .repay(let asset, let a, _): a.allSatisfy({ $0 == 0xff }) ? "repay all of this wallet's \(EVMRPC.dollars[asset.lowercased()] ?? short(asset)) debt to \(short(to)) \(net)"
            : "repay \(amount(a, of: asset)) of this wallet's debt to \(short(to)) \(net)"
        case .multisigExec(let target, let v, let d, _):
            "carry out the multisig \(short(to))'s signed move: " + EVMCall.decode(d).summary(chainId: chainId, to: target, value: ABI.uint(in: v, word: 0) ?? .max)
        case .contract where to.lowercased() == Tempo.feeManager: "pay Tempo's fees in USDC from now on"
        case .contract where to.lowercased() == Tempo.exchange: "swap dollars on Tempo's exchange, paying this wallet"
        default: "call the contract \(short(to)) \(net)"
        }
        return value > 0 ? what + ", paying \(coins)" : what
    }

    /// An allowance none of ihatecrypto's own approvals reaches: ≥ 2⁹⁶ units (79 billion tokens of 18 decimals). Drainers
    /// ask for the maximum, 2¹²⁸ − 1 or 10³⁰ — past any balance; the old ≥ 2¹²⁸ line let the last two through.
    /// ponytail: more than 79 billion of an 18-decimal token can't be approved in one go; compare with the balance if that matters.
    static func unlimited(_ amount: Data) -> Bool { amount.prefix(20).contains { $0 != 0 } }

    /// Contracts ihatecrypto itself lets spend its tokens on a network: Uniswap's routers (v2, v3, and v4's with Permit2),
    /// Aave's pools, Morpho's vaults, Relay's depository and approval proxy (one address on every network, api.relay.link/chains
    /// 2026-10-03), Circle's CCTP TokenMessenger, USDT0's adapter on Ethereum, Tempo's exchange. An approve to any other spender
    /// needs it approved in person, like a recipient.
    public static func knownSpenders(chainId: UInt64) -> Set<String> {
        // Relay's depository and its approval proxies (v3, and v3.2 from 2026-10-26).
        var s = ["0x4cd00e387622c35bddb9b4c962c136462338bc31"] + Permit3009.relayProxies
        s += AaveV3.markets.filter { $0.rpc.chainId == chainId }.map(\.pool) + MorphoVault.vaults.filter { $0.rpc.chainId == chainId }.map(\.address)
        if CCTP.domain(chainId) != nil { s.append(CCTP.tokenMessenger) }
        if let usdt0 = USDT0.on(chainId), usdt0.approves { s.append(usdt0.oft) }
        if chainId == EVMRPC.tempo.chainId { s.append(Tempo.exchange) }
        if let uni = UniswapV3.on(chainId) { s.append(uni.router) }
        if let v2 = UniswapV2.on(chainId) { s.append(v2.router) }
        if chainId == EVMRPC.robinhood.chainId { s.append(UniswapV3.robinhoodRouter) }
        if let v4 = UniswapV4.on(chainId) { s += [v4.router, UniswapV4.permit2] }
        if KyberSwap.chains[chainId] != nil { s.append(KyberSwap.router) }
        return Set(s.map { $0.lowercased() })
    }

    /// Contracts ihatecrypto's own moves call with calldata the signer doesn't read: Relay's depository and approval proxies
    /// (a quote's call), Tempo's exchange and fee manager, Safe's factory. Uniswap's routers, Permit2, Aave and the rest get
    /// only calls the decoder reads from ihatecrypto; an unread one there (a site's) could spend what was approved to them.
    static let ownUnreadCallees: Set<String> = Set([RelayMulticallPermit.depository, Tempo.exchange, Tempo.feeManager, Safe.factory].map { $0.lowercased() })
        .union(Permit3009.relayProxies)

    /// The contract of a call the signer can't read, to be allowed in person once, whoever asks for it (a site, the chat's
    /// `call`): nil when the call is read, pays that contract (then it's a recipient), or is one of ihatecrypto's own.
    public func unreadTarget(to: String, value: UInt64, chainId: UInt64) -> String? {
        let target = to.lowercased()
        guard !payees(to: to, value: value, chainId: chainId).contains(where: { $0.lowercased() == target }) else { return nil }
        // A protocol's call read by its selector alone (a swap, a vault, a claim…) is that protocol's only at its own
        // contract: at another, the selector is the contract's word for whatever it does — asked about like an unread call.
        let byProtocol = switch self {
        case .swap, .v4Swap, .kyberSwap, .vault, .lending, .claim, .borrow, .repay, .cctpBurn, .oftSend, .permit2Approve: true
        default: false
        }
        if byProtocol { return Self.protocolContracts(chainId: chainId).contains(target) ? nil : target }
        guard case .contract = self, !Self.ownUnreadCallees.contains(target) else { return nil }
        return target
    }

    /// The contracts whose calls ihatecrypto reads by protocol: its known spenders (routers, pools, vaults, CCTP, Permit2),
    /// Merkl's distributor and USDT0's own OFT.
    static func protocolContracts(chainId: UInt64) -> Set<String> {
        knownSpenders(chainId: chainId).union([Merkl.distributor.lowercased()]).union([USDT0.on(chainId)?.oft].compactMap { $0?.lowercased() })
    }

    /// What the contract called makes wrong with this call, beyond the call itself: Relay's depository takes only deposits
    /// naming this wallet as the depositor (depositNative, depositErc20 with or without an amount), and a Safe's
    /// execTransaction must be a plain payout (Safe.execRefusal) — the 2026-10-24 review.
    public static func targetRefusal(to: String, data: Data, sender: String) -> String? {
        // KyberSwap's router gets only the swap the signer reads (an exact approve to it goes to the token, not here).
        if to.lowercased() == KyberSwap.router, KyberSwap.decode(data) == nil {
            return "A call to KyberSwap's router that isn't the swap ihatecrypto reads. ihatecrypto only swaps there."
        }
        if to.lowercased() == RelayMulticallPermit.depository {
            let selector = data.prefix(4).hex
            guard relayDeposits.contains(selector) else { return "A call to Relay's depository that isn't a deposit. ihatecrypto only deposits there." }
            guard ABI.address(in: Data(data.dropFirst(4)), word: 0)?.lowercased() == sender.lowercased() else {
                return "The Relay deposit names someone else as the depositor."
            }
        }
        return Safe.execRefusal(data, safe: to)
    }

    /// The Relay order id a call into Relay's depository deposits for: its last word (depositNative, both depositErc20).
    public static func relayDepositID(to: String, data: Data) -> Data? {
        guard to.lowercased() == RelayMulticallPermit.depository, relayDeposits.contains(data.prefix(4).hex), data.count >= 4 + 64,
              (data.count - 4) % 32 == 0 else { return nil }
        return Data(data.suffix(32))
    }

    static let relayDeposits: Set<String> = Set(["depositNative(address,bytes32)", "depositErc20(address,address,uint256,bytes32)",
                                                 "depositErc20(address,address,bytes32)"].map { Keccak.hash256(Data($0.utf8)).prefix(4).hex })

    /// Who gets money, or the right to spend it, from this call: `to` when it carries coins (unless they come straight back,
    /// `returnsCoins`), a token's or NFT's recipient, and a spender that isn't one of `knownSpenders`. The signer signs only
    /// when each is the wallet's own or approved in person.
    public func payees(to: String, value: UInt64, chainId: UInt64) -> [String] {
        var out = value > 0 && !returnsCoins(to: to, value: value, chainId: chainId) ? [to] : []
        switch self {
        case .transfer(let r, _), .nftTransfer(_, let r): out.append(r)
        // Someone else's tokens this wallet may spend, sent on: whoever gets them is a payee too.
        case .transferFrom(_, let r, _): out.append(r)
        // What CCTP mints goes to its recipient: this wallet, or an address allowed in person.
        case .cctpBurn(let r?, _, _): out.append(r)
        // What an OFT sends goes to its recipient on the other network, and the fee it didn't need to `refund`.
        case .oftSend(let r?, _, _, _, let refund): out += [r, refund]
        // What the multisig pays out: the coins it sends (a word past 64 bits pays `target` too) and its call's own payees.
        case .multisigExec(let target, let v, let d, _):
            let coins = ABI.uint(in: v, word: 0)
            if coins != 0 { out.append(target) }
            out += EVMCall.decode(d).payees(to: target, value: 0, chainId: chainId)
        // approve(spender, 0) is an ERC-20 revoke, or an NFT's token id 0 handed over (ERC-721 shares the selector): its
        // spender is always one to allow, unless it's nobody (the zero address clears an NFT's approval).
        case .approve(let s, _):
            if s.lowercased() != "0x" + String(repeating: "0", count: 40), !Self.knownSpenders(chainId: chainId).contains(s.lowercased()) { out.append(s) }
        // An increase or a Permit2 allowance of zero gives nobody anything: no one to allow for it.
        case .increaseAllowance(let s, let a), .permit2Approve(_, let s, let a, _):
            if a.contains(where: { $0 != 0 }), !Self.knownSpenders(chainId: chainId).contains(s.lowercased()) { out.append(s) }
        default: break
        }
        return out
    }

    /// Coins sent with this call that come back to the sender, so `to` isn't paid: a wrap by the network's own WETH (the one
    /// Uniswap's v2 router uses there), or a v4 swap of the network's coin through its Universal Router whose input is exactly
    /// that value (SETTLE_ALL in the zero currency; the router keeps nothing over, TAKE_ALL pays the caller).
    func returnsCoins(to: String, value: UInt64, chainId: UInt64) -> Bool {
        switch self {
        case .wrap: to.lowercased() == UniswapV2.on(chainId)?.weth
        case .v4Swap(let s): to.lowercased() == UniswapV4.on(chainId)?.router && s.currencyIn == UniswapV4.native
            && s.amountIn == Decimal(value) && s.settleMax == s.amountIn
        // A plain send through one of USDT0's own OFTs: the value is LayerZero's message fee, and what it doesn't take
        // goes to `refund`, a payee of its own.
        case .oftSend(_, _, _, let plain, _): plain && USDT0.on(chainId)?.oft == to.lowercased()
        default: false
        }
    }

    /// Why this must not be signed, or nil. `sender` is the user's own address; `chainId` the network, which lets a v4 swap
    /// run a hook Uniswap's registry vouches for there (UniswapV4.allowsHooks): without it, no hook at all.
    public func refusal(sender: String, chainId: UInt64? = nil) -> String? {
        // MSG_SENDER pays the caller on SwapRouter02; on another contract it is address 1, where the output is lost to everyone.
        if case .swap(let recipient) = self, recipient.lowercased() != sender.lowercased(), recipient.lowercased() != Self.msgSender {
            return "The swap would pay \(recipient), not this wallet."
        }
        if case .lending(let beneficiary) = self, beneficiary.lowercased() != sender.lowercased() {
            return "The lending pool would credit \(beneficiary), not this wallet."
        }
        if case .v4Swap(let swap) = self, let why = UniswapV4.refusal(swap, chainId: chainId) { return why }
        if case .kyberSwap(let swap) = self, let why = KyberSwap.refusal(swap, sender: sender) { return why }
        if case .multisigExec(_, _, _, let operation) = self, operation != 0 {
            return "A multisig move by DELEGATECALL runs another contract's code as the multisig. Refused."
        }
        if case .cctpBurn(nil, _, _) = self { return "The CCTP burn mints to an account that isn't an EVM address. ihatecrypto mints only to EVM addresses." }
        if case .oftSend(nil, _, _, _, _) = self { return "The LayerZero send can't be read, or pays an account that isn't an EVM address. ihatecrypto sends only to EVM addresses." }
        if case .oftSend(_, _, _, false, _) = self { return "The LayerZero send carries options or a message, which can pay someone else on the other network. ihatecrypto sends only plain ones." }
        if case .borrow(_, _, let owner) = self, owner.lowercased() != sender.lowercased() { return "The borrow would put the debt on \(owner), not this wallet." }
        if case .repay(_, _, let owner) = self, owner.lowercased() != sender.lowercased() { return "The repay would pay \(owner)'s debt, not this wallet's." }
        if case .permit2Approve(_, _, let amount, let expiration) = self {
            if Self.unlimited(amount) { return "Unlimited spending through Permit2 — the drainers' main trick. ihatecrypto only approves exact amounts." }
            if expiration > UInt64(Date().timeIntervalSince1970) + 86_400 { return "A Permit2 approval for longer than a day. ihatecrypto approves for the one swap only." }
        }
        if case .claim(let recipients) = self, let other = recipients.first(where: { $0.lowercased() != sender.lowercased() }) ?? (recipients.isEmpty ? "nobody" : nil) {
            return "The rewards claim would pay \(other), not this wallet."
        }
        if case .vault(let receiver, let owner) = self {
            if receiver.lowercased() != sender.lowercased() { return "The vault would credit \(receiver), not this wallet." }
            if let owner, owner.lowercased() != sender.lowercased() { return "The vault call spends shares of \(owner), not this wallet's." }
        }
        return switch self {
        case .approve(_, let a) where Self.unlimited(a), .increaseAllowance(_, let a) where Self.unlimited(a):
            "Unlimited token spending approval — the drainers' main trick. ihatecrypto only approves exact amounts."
        case .setApprovalForAll(_, true):
            "Rights to all your NFTs in this contract (setApprovalForAll) — an NFT drainer trick. Refused."
        case .delegation(let to, true):
            "Lets \(to) borrow against or manage your lending positions (Aave credit delegation, Morpho authorization). Refused."
        case .permit:
            "A permit call grants approval to spend your tokens. Refused."
        case .transferFrom(let from, _, _) where from.lowercased() == sender.lowercased():
            "transferFrom from your address in your name — this is how tokens are pulled through an existing approval. Refused."
        default:
            nil
        }
    }
}
