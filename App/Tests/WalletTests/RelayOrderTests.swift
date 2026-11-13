import EVMChain
import Foundation
import Testing
import WalletCore

/// Relay orders as api.relay.link quoted them on 2026-10-24 for the public test wallet («abandon … about»): Solana → Base,
/// Hyperliquid → Base and Base → Solana, each with the orderId the quote gave.
private let relayOrders: [(id: String, json: String)] = [
    ("0x9152ea5c8536a888468b70453ceb0ffa04863f504323891056659c5afe531b56", #"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0x96b62513f6c215d0ac3606a62a62791754ab9b97fbb5ca42da9e22faa6d19095","inputs":[{"payment":{"chainId":"solana","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","amount":"5000000","weight":"1"},"refunds":[{"chainId":"solana","recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"0","deadline":1793396777,"extraData":"0x"},{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396777,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"}]}],"output":{"chainId":"base","payments":[{"recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878026","expectedAmount":"4977577"}],"calls":[],"deadline":1793396777,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},"fees":[]}"#),
    ("0xf127e4feda47952372d7e607a05dbeeb394eb8f0c953ef85eab2dc32fbe5b546", #"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0x7ddac287d557d21935a3eeefd355c80b564e09715cf6b571f47aafda82a6d97d","inputs":[{"payment":{"chainId":"hyperliquid","currency":"0x00000000000000000000000000000000","amount":"500000000","weight":"1"},"refunds":[{"chainId":"hyperliquid","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x00000000000000000000000000000000","minimumAmount":"0","deadline":1793396850,"extraData":"0x"},{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396850,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"}]}],"output":{"chainId":"base","payments":[{"recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878146","expectedAmount":"4977700"}],"calls":[],"deadline":1793396850,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},"fees":[]}"#),
    ("0x098dbfaf667b9a37d851bd79adee07393cd99499fc091c5785c0b80b9eb6a2f2", #"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0xff566540576a54aeb2c99679f6fb8af8a8b306110ce0b8e10bcadd3585022963","inputs":[{"payment":{"chainId":"base","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","amount":"5000000","weight":"1"},"refunds":[{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396849,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},{"chainId":"solana","recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"0","deadline":1793396849,"extraData":"0x"}]}],"output":{"chainId":"solana","payments":[{"recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"4878735","expectedAmount":"4978301"}],"calls":[],"deadline":1793396849,"extraData":"0x"},"fees":[]}"#),
]
private let evm = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94", solana = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"

/// The signer recomputes a Relay order's id itself: the 32 bytes its deposit names are the hash of an order paying this wallet.
@Test func relayOrderIdsAreRecomputedFromTheOrder() throws {
    for o in relayOrders {
        let order = try RelayOrder.read(Data(o.json.utf8), evm: evm, solana: solana, now: 1_700_000_000)
        #expect("0x" + order.id.hex == o.id)
    }
    // Base → Solana pays the Solana address, Solana → Base the EVM one.
    #expect(try RelayOrder.read(Data(relayOrders[0].json.utf8), evm: evm, solana: solana, now: 1_700_000_000).recipient == evm.lowercased())
}

/// Another payee changes the id; a refund to someone else, a call on the other network, a fee or an expired order is refused.
@Test func relayOrdersThatPaySomeoneElseAreRefused() throws {
    let base = relayOrders[0].json
    let thief = "0x000000000000000000000000000000000000dEaD"
    // The payee changed: the order still reads (its payee is asked about in person), but its id is no longer the deposit's.
    let swapped = base.replacingOccurrences(of: #""payments":[{"recipient":"\#(evm)""#, with: #""payments":[{"recipient":"\#(thief)""#)
    #expect(swapped != base)
    let other = try RelayOrder.read(Data(swapped.utf8), evm: evm, solana: solana, now: 1_700_000_000)
    #expect("0x" + other.id.hex != relayOrders[0].id && other.recipient == thief.lowercased())
    // Its refunds go to this wallet only.
    #expect(throws: RelayOrder.Refusal.self) { try RelayOrder.read(Data(base.utf8), evm: thief, solana: solana, now: 1_700_000_000) }
    #expect(throws: RelayOrder.Refusal.self) {
        try RelayOrder.read(Data(base.replacingOccurrences(of: #""calls":[]"#, with: #""calls":["0x1234"]"#).utf8), evm: evm, solana: solana, now: 1_700_000_000)
    }
    #expect(throws: RelayOrder.Refusal.self) { try RelayOrder.read(Data(base.utf8), evm: evm, solana: solana, now: 4_000_000_000) }
}

/// What an order takes and gives back, for the automation wallet's limits: its bridge to the person's own wallet spends
/// the deposit less the order's minimum, and only when both sides are dollars the signer counts (the 2026-11-10 review).
@Test func relayOrdersSayWhatComesBackInDollars() throws {
    let read = { (i: Int) in try RelayOrder.read(Data(relayOrders[i].json.utf8), evm: evm, solana: solana, now: 1_700_000_000) }
    // Solana → Base, Hyperliquid → Base, Base → Solana: 5 USDC in, at least 4.878… USDC out.
    let orders = try [read(0), read(1), read(2)]
    #expect(orders.map(\.inputCurrency) == ["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "0x00000000000000000000000000000000", EVMRPC.baseUSDC])
    #expect(orders.map(\.inputAmount) == ["5000000", "500000000", "5000000"])
    #expect(orders.map(\.minimumDollars) == [Decimal(string: "4.878026"), Decimal(string: "4.878146"), Decimal(string: "4.878735")])
    // Paid in anything else — another token, the network's coin — nothing is counted as coming back.
    let other = relayOrders[0].json.replacingOccurrences(of: #""currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878026""#,
                                                         with: #""currency":"0x0000000000000000000000000000000000000000","minimumAmount":"4878026""#)
    #expect(other != relayOrders[0].json)
    #expect(try RelayOrder.read(Data(other.utf8), evm: evm, solana: solana, now: 1_700_000_000).minimumDollars == nil)
}

/// The deposit's prompt says what the order pays out, in the order's own words: dollars where it pays the network's
/// dollar, raw units and the coin's address where it doesn't — never the app's «you get».
@Test func relayOrdersSayWhatTheyPayOnThePrompt() throws {
    let order = try RelayOrder.read(Data(relayOrders[0].json.utf8), evm: evm, solana: solana, now: 1_700_000_000)
    #expect(order.pays == "at least 4.878026 USDC to \(evm.lowercased()) on base")
    #expect(order.summary == "bridge 5 USDC from solana through Relay: at least 4.878026 USDC to \(evm.lowercased()) on base")
    let coin = relayOrders[0].json.replacingOccurrences(of: #""currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878026""#,
                                                        with: #""currency":"0x0000000000000000000000000000000000000000","minimumAmount":"4878026""#)
    #expect(try RelayOrder.read(Data(coin.utf8), evm: evm, solana: solana, now: 1_700_000_000).pays
            == "at least 4878026 units of 0x0000000000000000000000000000000000000000 to \(evm.lowercased()) on base")
    // Hyperliquid's USDC is in 1e-8.
    #expect(try RelayOrder.read(Data(relayOrders[1].json.utf8), evm: evm, solana: solana, now: 1_700_000_000).summary.hasPrefix("bridge 5 USDC from hyperliquid"))
}


/// An amount the prompt would read one way and the id's hash another is refused: a digit with an invisible mark on it
/// («5000000» + a joiner hashed as 5000000 and showed as 50000000), a hex, a JSON number.
@Test func relayOrderAmountsArePlainDigits() throws {
    let base = relayOrders[0].json
    for odd in [#""minimumAmount":"4878026\u200d""#, #""minimumAmount":"4\u0301878026""#, #""minimumAmount":"0x4a6e8a""#, #""minimumAmount":4878026"#,
                #""minimumAmount":"""#, #""minimumAmount":"٤٨٧٨٠٢٦""#] {
        let changed = base.replacingOccurrences(of: #""minimumAmount":"4878026""#, with: odd)
        #expect(changed != base)
        // Refused by the id's hash or by the order's own reading, whichever meets it first.
        #expect(throws: (any Error).self) { try RelayOrder.read(Data(changed.utf8), evm: evm, solana: solana, now: 1_700_000_000) }
    }
    let input = base.replacingOccurrences(of: #""amount":"5000000""#, with: #""amount":"5000000\u200d""#)
    #expect(input != base)
    #expect(throws: (any Error).self) { try RelayOrder.read(Data(input.utf8), evm: evm, solana: solana, now: 1_700_000_000) }
    // The typed-data reader takes the digits 0–9 only, too.
    #expect(throws: (any Error).self) { try TypedData.word("12\u{200D}3") }
    #expect(throws: (any Error).self) { try TypedData.word("١٢٣") }
    #expect(try TypedData.word("123") == ABI.uint(123) && TypedData.word("0x7b") == ABI.uint(123) && TypedData.word(123) == ABI.uint(123))
}
