import Foundation
import WalletCore

/// Uniswap v4 (developers.uniswap.org v4 deployments): one PoolManager a network holds every pool; a swap goes through
/// the Universal Router's V4_SWAP command, which pulls the input through Permit2. The formats are the deployed router's own
/// (its verified source on Blockscout): ExactInputSingleParams carries `minHopPriceX36`, which older docs and the first
/// Universal Router don't have — so it's Universal Router 2.1.2 on every network, whose verified source has the same
/// struct and the same SWAP_EXACT_IN_SINGLE 0x06, SETTLE_ALL 0x0c, TAKE_ALL 0x0f as Robinhood Chain's 2.1.1 (checked
/// 2026-10-05). Robinhood Chain moved to 2.1.2 (Uniswap/universal-router deploy-addresses/robinhood.json, 2026-09-18,
/// its poolManager() read on-chain 2026-11-05): Permit2's allowance is per router, so v4Swap reads it and asks anew.
public enum UniswapV4 {
    public static let robinhoodPoolManager = "0x8366a39cc670b4001a1121b8f6a443a643e40951"
    public static let robinhoodQuoter = "0x8dc178efb8111bb0973dd9d722ebeff267c98f94"
    public static let robinhoodRouter = "0x204faca1764b154221e35c0d20abb3c525710498"
    public static let permit2 = "0x000000000022d473030f116ddee9f6b43ac78ba3"
    /// The currency v4 pools name the network's own coin by (ETH, POL): paid with the call's value, taken as coins.
    public static let native = "0x0000000000000000000000000000000000000000"

    /// A network's PoolManager, V4Quoter, Universal Router and PositionManager (whose poolKeys names a pool's key), and the
    /// Blockscout that reads the PoolManager's whole log.
    public struct Deployment: Sendable {
        public let poolManager: String
        public let quoter: String
        public let router: String
        public let positionManager: String
        public let blockscout: String
    }

    public static func on(_ chainId: UInt64) -> Deployment? {
        switch chainId {
        case 4663: Deployment(poolManager: robinhoodPoolManager, quoter: robinhoodQuoter, router: robinhoodRouter,
                              positionManager: "0x58daec3116aae6d93017baaea7749052e8a04fa7", blockscout: "robinhoodchain.blockscout.com")
        case 8453: Deployment(poolManager: "0x498581ff718922c3f8e6a244956af099b2652b2b", quoter: "0x0d5e0f971ed27fbff6c2837bf31316121532048d",
                              router: "0xd6145b2d3f379919e8cdeda7b97e37c4b2ca9c40", positionManager: "0x7c5f5a4bbd8fd63184577525326123b519429bdc",
                              blockscout: "base.blockscout.com")
        case 42161: Deployment(poolManager: "0x360e68faccca8ca495c1b759fd9eee466db9fb32", quoter: "0x3972c00f7ed4885e145823eb7c655375d275a1c5",
                               router: "0x2d01411773c8c24805306e89a41f7855c3c4fe65", positionManager: "0xd88f38f930b7952f2db2432cb002e7abbf3dd869",
                               blockscout: "arbitrum.blockscout.com")
        case 1: Deployment(poolManager: "0x000000000004444c5dc75cb358380d2e3de08a90", quoter: "0x52f0e24d1c21c8a0cb1e5a5dd6198556bd9e1203",
                           router: "0x23617e59a5925b2a4bf75d73ff6711cd0b29de85", positionManager: "0xbd216513d74c8cf14cf4747e6aaa6420ff64ee9e",
                           blockscout: "eth.blockscout.com")
        case 137: Deployment(poolManager: "0x67366782805870060151383f4bbff9dab53e5cd6", quoter: "0xb3d5c3dfc3a7aebff71895a7191796bffc2c81b9",
                             router: "0xdc264714f68d84cf29bc605589405e78bdbe7c9f", positionManager: "0x1ec2ebf4f37e7363fdfe3551602425af0b3ceef9",
                             blockscout: "polygon.blockscout.com")
        // Optimism and HyperEVM, Universal Router 2.1.2 (the same build as Base's; its V4Quoter and PositionManager name the
        // PoolManager, checked 2026-10-07). HyperEVM has no Blockscout: a pool's key comes from the PositionManager alone.
        case 10: Deployment(poolManager: "0x9a13f98cb987694c9f086b1f5eb990eea8264ec3", quoter: "0x1f3131a13296fb91c90870043742c3cdbff1a8d7",
                            router: "0xc09255d86db563cbc11c2fcf4a0c512e160111b4", positionManager: "0x3c3ea4b57a46241e54610e5f022e5c45859a1017",
                            blockscout: "explorer.optimism.io")
        case 999: Deployment(poolManager: "0x12d4fd9c5dedd00ab8a0bce2cf0167bbf94b6b1f", quoter: "0x108bfe38532c98f8cd8d7ab49c9ddaa3675a1c5e",
                             router: "0xed270a1bcdc63cf1356d695e7f40961d4819d6bf", positionManager: "0x0d7ab5b3db668128aff6f70c4ebc71d7d4da9bf9",
                             blockscout: "")
        // BNB Chain: Universal Router 2.1.2 from Uniswap's deploy-addresses/bsc.json (its poolManager(), the V4Quoter's and
        // the PositionManager's all 0x28e2…e9df, checked 2026-10-08). No Blockscout either.
        case 56: Deployment(poolManager: "0x28e2ea090877bf75740558f6bfb36a5ffee9e9df", quoter: "0x9f75dd27d6664c475b90e105573e550ff69437b0",
                            router: "0xdc264714f68d84cf29bc605589405e78bdbe7c9f", positionManager: "0x7a4a5c919ae2541aed11041a1aeee68f1287f95b",
                            blockscout: "")
        default: nil
        }
    }
    /// keccak256("Initialize(bytes32,address,address,uint24,int24,address,uint160,int24)"): the PoolManager's event that
    /// names a pool's key once, when it is made.
    public static let initializeTopic = "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438"

    /// A pool's identity: its two currencies (the lower address first), fee, tick spacing and hooks contract.
    public struct PoolKey: Equatable, Sendable {
        public let currency0: String
        public let currency1: String
        public let fee: UInt32
        public let tickSpacing: Int32
        public let hooks: String

        public init(currency0: String, currency1: String, fee: UInt32, tickSpacing: Int32, hooks: String) {
            self.currency0 = currency0.lowercased(); self.currency1 = currency1.lowercased()
            self.fee = fee; self.tickSpacing = tickSpacing; self.hooks = hooks.lowercased()
        }

        /// abi.encode(currency0, currency1, fee, tickSpacing, hooks): five words, tick spacing sign-extended.
        public func encoded() throws -> Data {
            let spacing = tickSpacing >= 0 ? ABI.uint(UInt64(tickSpacing)) : Data(repeating: 0xff, count: 24) + withUnsafeBytes(of: Int64(tickSpacing).bigEndian) { Data($0) }
            return try ABI.address(currency0) + ABI.address(currency1) + ABI.uint(UInt64(fee)) + spacing + ABI.address(hooks)
        }

        /// keccak256(abi.encode(key)): the id DexScreener and the PoolManager's events know the pool by.
        public var id: String? { (try? encoded()).map { "0x" + Keccak.hash256($0).hex } }
        public var hasHooks: Bool { hooks != "0x" + String(repeating: "0", count: 40) }
    }

    /// One exact-input swap in one pool, as the app sends it and as the signer reads it back.
    public struct Swap: Equatable, Sendable {
        public let key: PoolKey
        public let zeroForOne: Bool
        public let amountIn: Decimal
        public let amountOutMinimum: Decimal
        /// SETTLE_ALL: what is paid and at most how much; TAKE_ALL: what comes out and at least how much.
        public let settleCurrency: String
        public let settleMax: Decimal
        public let takeCurrency: String
        public let takeMin: Decimal
        public let deadline: UInt64
        public var currencyIn: String { zeroForOne ? key.currency0 : key.currency1 }
        public var currencyOut: String { zeroForOne ? key.currency1 : key.currency0 }
    }

    /// V4Quoter.quoteExactInputSingle((key, zeroForOne, exactAmount, hookData)): word 0 of the answer is amountOut.
    public static func quoteExactInputSingle(_ key: PoolKey, zeroForOne: Bool, amount: Decimal) throws -> Data {
        // The struct has a dynamic member: an offset to it, then key (5), zeroForOne, amount, hookData's offset, its length 0.
        Data([0xaa, 0x9d, 0x21, 0xcb]) + ABI.uint(0x20) + (try key.encoded()) + ABI.uint(zeroForOne ? 1 : 0) + (try ABI.word(amount))
            + ABI.uint(0x100) + ABI.uint(0)
    }

    /// The Universal Router's commands and the v4 actions this app sends, nothing else.
    static let v4SwapCommand: UInt8 = 0x10
    static let actions = Data([0x06, 0x0c, 0x0f])   // SWAP_EXACT_IN_SINGLE, SETTLE_ALL, TAKE_ALL

    /// UniversalRouter.execute(commands = [V4_SWAP], inputs = [abi.encode(actions, params)], deadline): the input is paid
    /// from this wallet through Permit2 (SETTLE_ALL, at most `amountIn`) and everything that comes out goes to the caller
    /// (TAKE_ALL pays msgSender, at least `amountOutMinimum`): there is no recipient to set.
    public static func execute(_ key: PoolKey, zeroForOne: Bool, amountIn: Decimal, amountOutMinimum: Decimal, deadline: UInt64) throws -> Data {
        let currencyIn = zeroForOne ? key.currency0 : key.currency1, currencyOut = zeroForOne ? key.currency1 : key.currency0
        // abi.encode(ExactInputSingleParams): offset to the struct; key, zeroForOne, amountIn, amountOutMinimum,
        // minHopPriceX36 (0: amountOutMinimum is the check), hookData at 10 words, empty.
        let swap = ABI.uint(0x20) + (try key.encoded()) + ABI.uint(zeroForOne ? 1 : 0) + (try ABI.word(amountIn)) + (try ABI.word(amountOutMinimum))
            + ABI.uint(0) + ABI.uint(0x140) + ABI.uint(0)
        let settle = try ABI.address(currencyIn) + ABI.word(amountIn)
        let take = try ABI.address(currencyOut) + ABI.word(amountOutMinimum)
        let input = Self.encodeBytesAndArray(actions, [swap, settle, take])
        let commands = Data([v4SwapCommand])
        return Data([0x35, 0x93, 0x56, 0x4c]) + ABI.uint(0x60) + ABI.uint(UInt64(0x60 + bytesValue(commands).count)) + ABI.uint(deadline)
            + bytesValue(commands) + dynamicArray([input])
    }

    /// Permit2.approve(token, spender, uint160 amount, uint48 expiration).
    public static func permit2Approve(token: String, spender: String, amount: Decimal, expiration: UInt64) throws -> Data {
        Data([0x87, 0x51, 0x7c, 0x45]) + (try ABI.address(token)) + (try ABI.address(spender)) + (try ABI.word(amount)) + ABI.uint(expiration)
    }

    /// Permit2.allowance(owner, token, spender) → (amount, expiration, nonce).
    public static func permit2Allowance(owner: String, token: String, spender: String) throws -> Data {
        Data([0x92, 0x7d, 0xa1, 0x05]) + (try ABI.address(owner)) + (try ABI.address(token)) + (try ABI.address(spender))
    }

    // MARK: Reading a call back (the signer)

    /// The swap in an execute call when it is exactly this app's shape — one V4_SWAP command whose actions are
    /// SWAP_EXACT_IN_SINGLE, SETTLE_ALL, TAKE_ALL, each well formed — else nil.
    public static func decodeExecute(_ data: Data) -> Swap? {
        let data = Data(data)
        guard data.count >= 4, data.prefix(4) == Data([0x35, 0x93, 0x56, 0x4c]) else { return nil }
        let args = Data(data.dropFirst(4))
        guard let commands = bytesAt(args, headWord: 0), commands == Data([v4SwapCommand]),
              let inputs = arrayAt(args, headWord: 1), inputs.count == 1,
              let deadline = ABI.uint(in: args, word: 2) else { return nil }
        let input = inputs[0]
        guard let actions = bytesAt(input, headWord: 0), actions == Self.actions,
              let params = arrayAt(input, headWord: 1), params.count == 3 else { return nil }
        // The swap's struct, behind its offset.
        guard let at = ABI.uint(in: params[0], word: 0), at == 0x20 else { return nil }
        let s = Data(params[0].dropFirst(0x20))
        // key (5), zeroForOne, amountIn, amountOutMinimum, minHopPriceX36, hookData's offset, its length: 11 words.
        guard s.count >= 11 * 32,
              let c0 = ABI.address(in: s, word: 0), let c1 = ABI.address(in: s, word: 1), let fee = ABI.uint(in: s, word: 2), fee < 1 << 24,
              let hooks = ABI.address(in: s, word: 4), let dir = ABI.uint(in: s, word: 5), dir <= 1,
              let amountIn = ABI.decimal(in: s, word: 6), let minOut = ABI.decimal(in: s, word: 7),
              ABI.uint(in: s, word: 9) == 0x140, ABI.uint(in: s, word: 10) == 0 else { return nil }
        let spacingWord = s.subdata(in: (3 * 32)..<(4 * 32))
        let spacing = Int32(truncatingIfNeeded: spacingWord.suffix(8).reduce(Int64(0)) { $0 << 8 | Int64($1) })
        guard params[1].count == 64, params[2].count == 64,
              let settleCurrency = ABI.address(in: params[1], word: 0), let settleMax = ABI.decimal(in: params[1], word: 1),
              let takeCurrency = ABI.address(in: params[2], word: 0), let takeMin = ABI.decimal(in: params[2], word: 1) else { return nil }
        return Swap(key: PoolKey(currency0: c0, currency1: c1, fee: UInt32(fee), tickSpacing: spacing, hooks: hooks), zeroForOne: dir == 1,
                    amountIn: amountIn, amountOutMinimum: minOut, settleCurrency: settleCurrency, settleMax: settleMax,
                    takeCurrency: takeCurrency, takeMin: takeMin, deadline: deadline)
    }

    /// Whether a swap may run through the pool's hooks contract: a pool without one; or a hook Uniswap's registry marks as a
    /// vanilla swap on this network (vanillaHooks: once allowed, the swap goes as in a pool without hooks), whose address —
    /// which carries its permissions — takes no delta from a swap, in a pool whose fee isn't set by the hook.
    public static func allowsHooks(_ key: PoolKey, chainId: UInt64) -> Bool {
        guard key.hasHooks else { return true }
        return key.fee & dynamicFee == 0 && !takesSwapDelta(key.hooks) && vanillaHooks[chainId]?.contains(key.hooks) == true
    }

    /// LPFeeLibrary.DYNAMIC_FEE_FLAG in a pool's fee: its hook sets the fee each swap.
    static let dynamicFee: UInt32 = 0x800000

    /// Hooks.BEFORE_SWAP_RETURNS_DELTA (bit 3) or AFTER_SWAP_RETURNS_DELTA (bit 2) in the address's low bits: the hook may take
    /// part of what a swap pays or gets. The PoolManager calls a hook only for the permissions its address carries.
    static func takesSwapDelta(_ hooks: String) -> Bool {
        guard let low = UInt16(hooks.suffix(4), radix: 16) else { return true }
        return low & 0b1100 != 0
    }

    /// Why the signer must not sign this swap, or nil: it pays from and to this wallet by construction (SETTLE_ALL and
    /// TAKE_ALL use the caller), so what's left is that it pays and takes the pool's own currencies, the most it may take
    /// is what goes in, something must come out, and no hooks contract runs in the middle but one `allowsHooks` vouches
    /// for on `chainId` (none without it).
    public static func refusal(_ s: Swap, chainId: UInt64? = nil) -> String? {
        if s.key.hasHooks, !(chainId.map { allowsHooks(s.key, chainId: $0) } ?? false) {
            return "The Uniswap v4 pool runs a hooks contract (\(s.key.hooks)) that isn't on Uniswap's list of hooks that swap like a pool without one: ihatecrypto doesn't swap there."
        }
        if s.settleCurrency.lowercased() != s.currencyIn { return "The swap would pay in \(s.settleCurrency), not the pool's \(s.currencyIn)." }
        if s.takeCurrency.lowercased() != s.currencyOut { return "The swap would take \(s.takeCurrency), not the pool's \(s.currencyOut)." }
        if s.settleMax != s.amountIn { return "The swap may take more than it swaps (\(s.settleMax) vs \(s.amountIn))." }
        if s.amountOutMinimum <= 0 || s.takeMin <= 0 { return "The swap has no minimum out: it could pay nothing back." }
        return nil
    }

    // MARK: ABI pieces

    static func bytesValue(_ d: Data) -> Data { ABI.uint(UInt64(d.count)) + d + Data(count: (32 - d.count % 32) % 32) }

    /// bytes[]: its length, an offset to each element (from after the length), the elements.
    static func dynamicArray(_ items: [Data]) -> Data {
        var head = Data(), tail = Data()
        for item in items {
            head += ABI.uint(UInt64(items.count * 32 + tail.count))
            tail += bytesValue(item)
        }
        return ABI.uint(UInt64(items.count)) + head + tail
    }

    /// abi.encode(bytes a, bytes[] b).
    static func encodeBytesAndArray(_ a: Data, _ b: [Data]) -> Data {
        ABI.uint(0x40) + ABI.uint(UInt64(0x40 + bytesValue(a).count)) + bytesValue(a) + dynamicArray(b)
    }

    /// The `bytes` whose offset is head word `i` of `d`. Offsets and lengths are the caller's numbers: each is held to the
    /// data's size before it becomes an Int, so 2⁶⁴ − 1 is refused instead of trapping.
    static func bytesAt(_ d: Data, headWord i: Int) -> Data? {
        guard let at = ABI.uint(in: d, word: i), at % 32 == 0, at < UInt64(d.count), Int(at) + 32 <= d.count,
              let length = ABI.uint(in: d.subdata(in: Int(at)..<d.count), word: 0), length <= UInt64(d.count),
              Int(at) + 32 + Int(length) <= d.count else { return nil }
        return d.subdata(in: (Int(at) + 32)..<(Int(at) + 32 + Int(length)))
    }

    /// The `bytes[]` whose offset is head word `i` of `d`.
    static func arrayAt(_ d: Data, headWord i: Int) -> [Data]? {
        guard let at = ABI.uint(in: d, word: i), at % 32 == 0, at < UInt64(d.count), Int(at) + 32 <= d.count else { return nil }
        let array = d.subdata(in: Int(at)..<d.count)
        guard let count = ABI.uint(in: array, word: 0), count <= 16 else { return nil }
        let body = array.dropFirst(32)
        var out: [Data] = []
        for k in 0..<Int(count) {
            guard let item = bytesAt(Data(body), headWord: k) else { return nil }
            out.append(item)
        }
        return out
    }
}
