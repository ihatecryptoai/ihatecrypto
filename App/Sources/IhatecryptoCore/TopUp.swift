import Foundation

/// A top-up of this wallet: what lands on which of its networks. USDC is what ihatecrypto trades with; the network's own
/// coin pays the fees; «any coin» and «any amount» leave both to the sender.
public struct TopUpOrder: Equatable, Sendable {
    /// A network the wallet receives on: Solana, or one of the EVM networks ihatecrypto moves money on (the same 0x address).
    public struct Network: Hashable, Sendable {
        public let key: String
        public let title: String
        /// nil on Solana.
        public let chainId: UInt64?
        /// Its dollar: Circle's USDC mint or contract; Robinhood Chain's is USDG.
        public let usdc: String
        public let usdcSymbol: String
        public let usdcDecimals: Int
        /// The coin fees are paid in; nil on Tempo, which pays them in its dollar.
        public let coin: String?
        public let explorer: String
        /// A public node, for a browser wallet that doesn't know the network yet.
        public let rpc: String
        /// What a transfer there costs and how soon it lands.
        public let cost: String

        public init(key: String, title: String, chainId: UInt64?, usdc: String, usdcSymbol: String = "USDC", usdcDecimals: Int = 6,
                    coin: String?, explorer: String, rpc: String, cost: String = "fee under 1¢ · lands in seconds") {
            (self.key, self.title, self.chainId, self.usdc, self.usdcSymbol, self.usdcDecimals) = (key, title, chainId, usdc, usdcSymbol, usdcDecimals)
            (self.coin, self.explorer, self.rpc, self.cost) = (coin, explorer, rpc, cost)
        }

        public static let solana = Network(key: "solana", title: "Solana", chainId: nil, usdc: Solana.usdcMint, coin: "SOL",
                                           explorer: "https://solscan.io", rpc: "")
    }

    public enum Coin: String, Sendable { case usdc, native, any }

    public var network: Network
    public var coin: Coin
    /// nil: any amount, the sender's wallet asks.
    public var amount: Decimal?
    /// This wallet's address on `network`.
    public var recipient: String

    /// Below this much SOL a Solana wallet can't sign its moves for long (fees, a new token account's rent): USDC alone
    /// there is a dead end (the owner, 2026-10-23: «закинул USDC, а оказалось, что надо было ещё и газ»).
    public static let solanaFeeFloor: UInt64 = 5_000_000
    /// What a top-up of USDC on Solana from a browser wallet brings along, in the same approval, to a wallet below it.
    public static let solanaFeeTopUp: UInt64 = 10_000_000

    public init(network: Network, coin: Coin, amount: Decimal?, recipient: String) {
        (self.network, self.coin, self.amount, self.recipient) = (network, network.coin == nil && coin == .native ? .usdc : coin, amount, recipient)
    }

    public var symbol: String { coin == .usdc ? network.usdcSymbol : coin == .native ? network.coin ?? "" : "any coin" }
    public var decimals: Int { coin == .usdc ? network.usdcDecimals : network.chainId == nil ? 9 : 18 }

    /// The amount in the coin's smallest units; nil for any coin or amount, nothing, too much, or more digits than the coin has.
    public var units: String? {
        guard coin != .any, let amount, amount > 0, amount <= 1_000_000 else { return nil }
        var scaled = amount * pow(10, decimals), whole = Decimal()
        NSDecimalRound(&whole, &scaled, 0, .plain)
        return whole == scaled ? NSDecimalNumber(decimal: whole).stringValue : nil
    }

    /// For a phone wallet's camera: Solana Pay's transfer request (Phantom, Solflare, Backpack) or EIP-681 (MetaMask,
    /// Rainbow, Coinbase Wallet), with the coin, the network and the amount filled in when they are chosen — without
    /// an amount the wallet asks for it. Exchanges' scanners want the bare address, which the address card keeps.
    public var payLink: String? {
        // A typed amount the coin can't hold makes no link.
        if coin != .any, amount != nil, units == nil { return nil }
        guard let chain = network.chainId else {
            return "solana:\(recipient)?" + (units != nil ? amount.map { "amount=\($0)&" } ?? "" : "")
                + (coin == .usdc ? "spl-token=\(network.usdc)&" : "") + "label=ihatecrypto&message=Top%20up"
        }
        switch coin {
        case .usdc: return "ethereum:\(network.usdc)@\(chain)/transfer?address=\(recipient)" + (units.map { "&uint256=\($0)" } ?? "")
        case .native: return "ethereum:\(recipient)@\(chain)" + (units.map { "?value=\($0)" } ?? "")
        case .any: return "ethereum:\(recipient)@\(chain)"
        }
    }

    /// Where a sent transfer shows.
    public func explorer(_ id: String) -> URL? { URL(string: "\(network.explorer)/tx/\(id)") }
}
