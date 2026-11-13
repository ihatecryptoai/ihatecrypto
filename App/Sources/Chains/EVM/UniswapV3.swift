import Foundation
import WalletCore

/// Uniswap v3: Robinhood Chain from developers.uniswap.org (v3-robinhood-chain-deployments), 2026-09-27; Base and
/// Arbitrum are the canonical deployments, checked on-chain 2026-09-29, Ethereum and Polygon 2026-10-01 (router and
/// quoter report the v3 factory 0x1F98…F984 and their WETH9: WETH on Ethereum, WPOL on Polygon, which prices its gas too).
public enum UniswapV3 {
    public static let robinhoodFactory = "0x1f7d7550b1b028f7571e69a784071f0205fd2efa"
    public static let robinhoodQuoter = "0x33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7"
    public static let robinhoodRouter = "0xcaf681a66d020601342297493863e78c959e5cb2"
    /// Pool fees to try, in hundredths of a basis point: 0.05%, 0.3%, 1%, 0.01%.
    public static let fees: [UInt64] = [500, 3000, 10000, 100]

    /// QuoterV2, SwapRouter02 and wrapped ETH (the usual middle hop) on one chain.
    public struct Deployment: Sendable {
        public let quoter: String
        public let router: String
        public let weth: String
    }

    public static func on(_ chainId: UInt64) -> Deployment? {
        switch chainId {
        case 8453: Deployment(quoter: "0x3d4e44eb1374240ce5f1b871ab261cd16335b76a", router: "0x2626664c2603336e57b271c5c0b26f421741e481",
                              weth: "0x4200000000000000000000000000000000000006")
        case 42161: Deployment(quoter: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e", router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                               weth: "0x82af49447d8a07e3bd95bd0d56f35241523fbab1")
        case 1: Deployment(quoter: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e", router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                           weth: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")
        case 137: Deployment(quoter: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e", router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                             weth: "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270")
        // Optimism and HyperEVM (developers.uniswap.org v3 deployments; each checked on-chain 2026-10-07: the router's
        // WETH9() and factory() and the quoter's answer). HyperEVM's pools hold almost nothing — its swaps fill through
        // Relay — but the table names its router and WHYPE for the signer.
        case 10: Deployment(quoter: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e", router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                            weth: "0x4200000000000000000000000000000000000006")
        case 999: Deployment(quoter: "0x7dfd4f31be6814d2906bde155c3e1b146eac1468", router: "0x7adf4701abcdbc5dcf5cb58b526f897e048f0d11",
                             weth: "0x5555555555555555555555555555555555555555")
        // BNB Chain (checked 2026-10-08: SwapRouter02's WETH9() is WBNB and factory() the QuoterV2's, 0xdb1d…61f7; 10 USDC
        // quoted ≈ 0.0129 WBNB at the 0.01% pool).
        // Robinhood Chain: the router and quoter its stock trades use (router WETH9() 0x0bd7…ad73, both on factory
        // 0x1f7d…2efa, checked 2026-10-08), so any token there swaps for any other too.
        case 4663: Deployment(quoter: robinhoodQuoter, router: robinhoodRouter, weth: "0x0bd7d308f8e1639fab988df18a8011f41eacad73")
        case 56: Deployment(quoter: "0x78d78e420da98ad378d7799be8f4af69033eb077", router: "0xb971ef87ede563556b2ed4b1c0b0019111dd85d2",
                            weth: "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c")
        default: nil
        }
    }

    /// getPool(address,address,uint24)
    public static func getPool(_ a: String, _ b: String, fee: UInt64) throws -> Data {
        Data([0x16, 0x98, 0xee, 0x82]) + (try ABI.address(a)) + (try ABI.address(b)) + ABI.uint(fee)
    }

    /// QuoterV2.quoteExactInputSingle((tokenIn, tokenOut, amountIn, fee, sqrtPriceLimitX96)): word 0 of the answer is amountOut.
    /// Amounts are whole 256-bit words: 54 tokens of 18 decimals don't fit 64 bits.
    public static func quoteExactInputSingle(tokenIn: String, tokenOut: String, amountIn: Decimal, fee: UInt64) throws -> Data {
        Data([0xc6, 0xa5, 0x02, 0x6a]) + (try ABI.address(tokenIn)) + (try ABI.address(tokenOut)) + (try ABI.word(amountIn)) + ABI.uint(fee) + ABI.uint(0)
    }

    /// SwapRouter02.exactInputSingle((tokenIn, tokenOut, fee, recipient, amountIn, amountOutMinimum, sqrtPriceLimitX96)).
    public static func exactInputSingle(tokenIn: String, tokenOut: String, fee: UInt64, recipient: String, amountIn: Decimal, amountOutMinimum: Decimal) throws -> Data {
        Data([0x04, 0xe4, 0x5a, 0xaf]) + (try ABI.address(tokenIn)) + (try ABI.address(tokenOut)) + ABI.uint(fee) + (try ABI.address(recipient))
            + (try ABI.word(amountIn)) + (try ABI.word(amountOutMinimum)) + ABI.uint(0)
    }

    /// A route through pools: token, fee (3 bytes), token, fee, token… One pool is a path of two tokens.
    public static func path(_ tokens: [String], fees: [UInt64]) throws -> Data {
        guard tokens.count == fees.count + 1 else { throw WalletError("A Uniswap path needs one fee between each two tokens.") }
        var out = Data()
        for (i, token) in tokens.enumerated() {
            guard let bytes = Data(evmAddress: token) else { throw WalletError("Invalid address: \(token)") }
            out += bytes
            if i < fees.count { out += withUnsafeBytes(of: UInt32(fees[i]).bigEndian) { Data($0.dropFirst()) } }
        }
        return out
    }

    private static func bytes(_ d: Data) -> Data { ABI.uint(UInt64(d.count)) + d + Data(count: (32 - d.count % 32) % 32) }

    /// QuoterV2.quoteExactInput(bytes path, uint256 amountIn): word 0 of the answer is amountOut.
    public static func quoteExactInput(path: Data, amountIn: Decimal) throws -> Data {
        Data([0xcd, 0xca, 0x17, 0x53]) + ABI.uint(0x40) + (try ABI.word(amountIn)) + bytes(path)
    }

    /// SwapRouter02.exactInput((bytes path, address recipient, uint256 amountIn, uint256 amountOutMinimum)).
    public static func exactInput(path: Data, recipient: String, amountIn: Decimal, amountOutMinimum: Decimal) throws -> Data {
        Data([0xb8, 0x58, 0x18, 0x3f]) + ABI.uint(0x20) + ABI.uint(0x80) + (try ABI.address(recipient))
            + (try ABI.word(amountIn)) + (try ABI.word(amountOutMinimum)) + bytes(path)
    }
}
