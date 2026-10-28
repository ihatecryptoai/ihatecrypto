import Foundation
import HyperliquidSigning
@testable import SolanaChain
import Testing
import TxGuard
import WalletCore

private let me = "2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj"
private let deposit = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw"
private let scam = "ScamXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1"
private let unknownProgram = "DrainerProgram1111111111111111111111111111111"

/// A legacy message from readable parts, for feeding the guard hand-made attacks.
private func message(_ keys: [String], _ ixs: [(program: Int, accounts: [Int], data: [UInt8])], signers: UInt8 = 1) throws -> (SolanaMessage, [String]) {
    var m = Data([signers, 0, 0]) + SolanaTransaction.compact(keys.count)
    for key in keys { m += try Base58.decode(key).count == 32 ? try Base58.decode(key) : Data(count: 32) }
    m += Data(count: 32) + SolanaTransaction.compact(ixs.count)
    for ix in ixs {
        m += Data([UInt8(ix.program)]) + SolanaTransaction.compact(ix.accounts.count) + Data(ix.accounts.map(UInt8.init))
        m += SolanaTransaction.compact(ix.data.count) + Data(ix.data)
    }
    let parsed = try SolanaMessage.parse(message: m)
    return (parsed, keys) // keys as given, so fake-but-readable addresses survive
}

private func solTransfer(_ lamports: UInt64) -> [UInt8] { [2, 0, 0, 0] + withUnsafeBytes(of: lamports.littleEndian, Array.init) }
private let policy = SolanaPolicy(owner: me, recipients: [deposit])

@Test func guardPassesATransferToTheApprovedDeposit() throws {
    let (m, accounts) = try message([me, deposit, SolanaGuard.system], [(2, [0, 1], solTransfer(130_000_000))])
    let v = SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:])
    #expect(v.ok, "\(v.problems)")
}

@Test func guardStopsAScamRecipient() throws {
    let (m, accounts) = try message([me, scam, SolanaGuard.system], [(2, [0, 1], solTransfer(130_000_000))])
    #expect(SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:]).problems.first?.contains(scam) == true)
}

@Test func guardStopsSomeoneElsePayingOrSigning() throws {
    let (m, accounts) = try message([scam, me, SolanaGuard.system], [(2, [0, 1], solTransfer(1))])
    #expect(!SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:]).ok)
    let (two, accounts2) = try message([me, scam, SolanaGuard.system], [(2, [0, 1], solTransfer(1))], signers: 2)
    #expect(!SolanaGuard.check(two, accounts: accounts2, policy: policy, tokenOwners: [:]).ok)
}

@Test func guardStopsUnknownProgramsAndTokenApprovals() throws {
    let (drainer, a1) = try message([me, unknownProgram], [(1, [0], [0])])
    #expect(SolanaGuard.check(drainer, accounts: a1, policy: policy, tokenOwners: [:]).problems.first?.contains("not on the allowed list") == true)
    // SPL Approve (tag 4): hands spending rights to someone else.
    let myUSDC = "MyUSDCAccountXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1"
    let (approve, a2) = try message([me, myUSDC, scam, SPL.tokenProgram], [(3, [1, 2, 0], [4] + withUnsafeBytes(of: UInt64(1).littleEndian, Array.init))])
    #expect(!SolanaGuard.check(approve, accounts: a2, policy: policy, tokenOwners: [myUSDC: me]).ok)
}

@Test func guardChecksTokenTransfersAccountCreationAndClosing() throws {
    let myUSDC = "MyUSDCAccountXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1"
    let depositUSDC = "DepositUSDCXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1"
    let scamUSDC = "ScamUSDCXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1"
    let owners = [myUSDC: me, depositUSDC: deposit]
    let keys = [me, myUSDC, depositUSDC, scamUSDC, deposit, scam, SPL.tokenProgram, SPL.associatedTokenProgram]
    // TransferChecked(source, mint, dest, authority) to the deposit's account: fine; to a scam account: stopped.
    let ok = try message(keys, [(6, [1, 6, 2, 0], [12, 0, 0, 0, 0, 0, 0, 0, 0, 6])])
    #expect(SolanaGuard.check(ok.0, accounts: ok.1, policy: policy, tokenOwners: owners).ok)
    let bad = try message(keys, [(6, [1, 6, 3, 0], [12, 0, 0, 0, 0, 0, 0, 0, 0, 6])])
    #expect(!SolanaGuard.check(bad.0, accounts: bad.1, policy: policy, tokenOwners: owners).ok)
    // Creating a token account for a stranger, closing ours into a stranger's wallet: stopped.
    let create = try message(keys, [(7, [0, 3, 5, 6], [1])])
    #expect(!SolanaGuard.check(create.0, accounts: create.1, policy: policy, tokenOwners: owners).ok)
    let close = try message(keys, [(6, [1, 5, 0], [9])])
    #expect(!SolanaGuard.check(close.0, accounts: close.1, policy: policy, tokenOwners: owners).ok)
}

@Test func guardStopsAnInflatedPriorityFee() throws {
    let limit: [UInt8] = [2] + withUnsafeBytes(of: UInt32(1_400_000).littleEndian, Array.init)
    let price: [UInt8] = [3] + withUnsafeBytes(of: UInt64(50_000_000).littleEndian, Array.init) // 0.07 SOL at the limit
    let (m, accounts) = try message([me, SolanaGuard.computeBudget], [(1, [], limit), (1, [], price)])
    #expect(SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:]).problems.first?.contains("Priority fee") == true)
}

// S10: a unit price of 2⁶³ times a limit of 2 passes 64 bits: refused, not a crash of the app or the signer.
@Test func guardRefusesAnOverflowingPriorityFee() throws {
    let limit: [UInt8] = [2] + withUnsafeBytes(of: UInt32(2).littleEndian, Array.init)
    let price: [UInt8] = [3] + withUnsafeBytes(of: (UInt64(1) << 63).littleEndian, Array.init)
    let (m, accounts) = try message([me, SolanaGuard.computeBudget], [(1, [], limit), (1, [], price)])
    #expect(SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:]).problems.first?.contains("Priority fee") == true)
}

// S12: a saved name or memo goes on the Touch ID prompt: no line breaks, bidi marks or invisible characters, 64 at most.
@Test func addressBookNamesCantFakeAPromptLine() {
    #expect(AddressCheck.labelProblem("Bybit") == nil)
    #expect(AddressCheck.labelProblem("Мама ❤️") == nil)
    #expect(AddressCheck.labelProblem("") == nil)
    #expect(AddressCheck.labelProblem("Bybit» on Solana: 7xKX\n\nmain") != nil)
    #expect(AddressCheck.labelProblem("main\u{202E}tib") != nil)
    #expect(AddressCheck.labelProblem("a\u{200B}b") != nil)
    #expect(AddressCheck.labelProblem(String(repeating: "x", count: 65)) != nil)
}

@Test func simulatedBalancesCatchARouteThatPaysSomeoneElse() {
    let rule = SolanaGuard.BalanceRule(label: "USDC", before: 1_000_000, after: 1_000_000, atLeast: 2_300_000)
    #expect(!SolanaGuard.balances([rule]).ok)
    #expect(SolanaGuard.balances([.init(label: "USDC", before: 0, after: 2_335_542, atLeast: 2_323_865)]).ok)
}

@Test func hyperliquidGuardAllowsTradingAndOnlyApprovedWithdrawals() {
    let owner = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
    let quoteDeposit = "0xBcd4042DE499D14e55001CcbB24a551F3b954096"
    func action(_ type: String, _ extra: [(String, MsgPack)] = []) -> MsgPack { .map([("type", .string(type))] + extra) }
    #expect(HLGuard.check(action("order"), owner: owner, recipients: []).ok)
    #expect(HLGuard.check(action("spotSend", [("destination", .string(quoteDeposit))]), owner: owner, recipients: [quoteDeposit]).ok)
    #expect(!HLGuard.check(action("spotSend", [("destination", .string("0x000000000000000000000000000000000000dEaD"))]), owner: owner, recipients: [quoteDeposit]).ok)
    #expect(!HLGuard.check(action("vaultTransfer"), owner: owner, recipients: []).ok)
    #expect(!HLGuard.check(action("approveAgent", [("agentAddress", .string("0xBAD"))]), owner: owner, recipients: [], agent: "0xGOOD").ok)
    // The trading key sets leverage without Touch ID: at most the risk rules' top, never Hyperliquid's own 40–50×.
    #expect(HLGuard.check(action("updateLeverage", [("asset", .uint(0)), ("isCross", .bool(false)), ("leverage", .uint(5))]), owner: owner, recipients: []).ok)
    #expect(HLGuard.check(action("updateLeverage", [("leverage", .uint(20))]), owner: owner, recipients: []).ok)
    #expect(!HLGuard.check(action("updateLeverage", [("leverage", .uint(21))]), owner: owner, recipients: []).ok)
    #expect(!HLGuard.check(action("updateLeverage", [("leverage", .uint(0))]), owner: owner, recipients: []).ok)
    #expect(!HLGuard.check(action("updateLeverage"), owner: owner, recipients: []).ok)
}

/// The trading key signs orders and the referral code without Touch ID: a fee only to ihatecrypto at its rate, only its code,
/// and a fee approval only for ihatecrypto's rate or its revocation.
@Test func hyperliquidGuardHoldsFeesAndCodeToIhatecrypto() throws {
    let owner = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
    let plain = try HLAction.order([HLOrder(asset: 0, isBuy: true, price: 100, size: 1, reduceOnly: false, kind: .limit(tif: "Ioc"))])
    func check(_ a: MsgPack) -> Bool { HLGuard.check(a, owner: owner, recipients: []).ok }
    func with(_ builder: MsgPack) -> MsgPack {
        guard case .map(let pairs) = plain else { return plain }
        return .map(pairs + [("builder", builder)])
    }
    #expect(check(HLAction.withBuilder(plain)))
    #expect(check(with(.map([("b", .string(HLBuilder.address)), ("f", .uint(10))]))))
    #expect(!check(with(.map([("b", .string(HLBuilder.address)), ("f", .uint(HLBuilder.fee + 1))]))))
    #expect(!check(with(.map([("b", .string("0x000000000000000000000000000000000000dead")), ("f", .uint(10))]))))
    #expect(!check(with(.map([("b", .string(HLBuilder.address)), ("f", .uint(10)), ("x", .uint(1))]))))
    #expect(!check(with(.string(HLBuilder.address))))

    #expect(check(HLBuilder.referrerAction))
    #expect(!check(.map([("type", .string("setReferrer")), ("code", .string("SOMEONE"))])))

    #expect(check(HLBuilder.approveAction(nonce: 1, mainnet: true)))
    #expect(check(HLBuilder.approveAction(maxFeeRate: HLBuilder.revoke, nonce: 1, mainnet: true)))
    #expect(!check(HLBuilder.approveAction(maxFeeRate: "0.1%", nonce: 1, mainnet: true)))
    let foreign = MsgPack.map([("type", .string("approveBuilderFee")), ("maxFeeRate", .string(HLBuilder.rate)),
                               ("builder", .string("0x000000000000000000000000000000000000dead")), ("nonce", .uint(1))])
    #expect(!check(foreign))
}

@Test func depositGuardRejectsAnAddressBoundToSomeoneElse() {
    let bound = ["recipient": "0xAttacker", "refundTo": me, "originAsset": "nep141:sol.omft.near",
                 "destinationAsset": "hc", "amount": "130000000"]
    let v = DepositGuard.check(bound: bound, recipient: "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955", refundTo: me,
                               origin: "nep141:sol.omft.near", destination: "hc", amount: "130000000")
    #expect(v.problems.count == 1)
    var good = bound
    good["recipient"] = "0x14dc79964da2c08b23698b3d3cc7ca32193d9955"
    #expect(DepositGuard.check(bound: good, recipient: "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955", refundTo: me,
                               origin: "nep141:sol.omft.near", destination: "hc", amount: "130000000").ok)
}

@Test func relayOnlyAcceptsDepositsFromOurWallet() throws {
    let relay = SolanaGuard.relayDepository
    let depositToken: [UInt8] = [0x0b, 0x9c, 0x60, 0xda, 0x27, 0xa3, 0xb4, 0x13] + withUnsafeBytes(of: UInt64(2_000_000).littleEndian, Array.init) + Array(repeating: 1, count: 32)
    let withdraw: [UInt8] = [0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0] + Array(repeating: 0, count: 8)
    // depository, sender, depositor: this wallet both times (a recovered deposit is refunded to the depositor).
    let ok = try message([me, relay], [(1, [1, 0, 0], depositToken)])
    #expect(SolanaGuard.check(ok.0, accounts: ok.1, policy: policy, tokenOwners: [:]).ok)
    #expect(SolanaGuard.relayDepositIDs(ok.0) == [Data(repeating: 1, count: 32)])
    let other = try message([me, relay], [(1, [1, 0, 0], withdraw)])
    #expect(!SolanaGuard.check(other.0, accounts: other.1, policy: policy, tokenOwners: [:]).ok)
    let notOurs = try message([me, scam, relay], [(2, [2, 0, 1], depositToken)])
    #expect(!SolanaGuard.check(notOurs.0, accounts: notOurs.1, policy: policy, tokenOwners: [:]).ok)
}

/// The signer has no network, so a lookup table's contents are the app's word: money may not go to an
/// account that only a table names, even an approved one.
@Test func signerRefusesMoneyToAnAccountOnlyALookupTableNames() throws {
    let (m, keys) = try message([me, SolanaGuard.system], [(1, [0, 2], solTransfer(1000))])
    let accounts = keys + [deposit] // index 2 would come from a lookup table
    #expect(SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:]).ok)
    var strict = policy
    strict.trustLookupTables = false
    #expect(!SolanaGuard.check(m, accounts: accounts, policy: strict, tokenOwners: [:]).ok)
}

@Test func vaultTransfersGoOnlyToHLP() {
    let me = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
    func transfer(_ vault: String) -> MsgPack {
        .map([("type", .string("vaultTransfer")), ("vaultAddress", .string(vault)), ("isDeposit", .bool(true)), ("usd", .uint(5_000_000))])
    }
    #expect(HLGuard.check(transfer(HLGuard.hlpVault), owner: me, recipients: []).ok)
    #expect(!HLGuard.check(transfer("0x" + String(repeating: "5c", count: 20)), owner: me, recipients: []).ok)
}

@Test func usdSendOnlyToTheApprovedAccount() {
    let me = "0x14dc79964da2c08b23698b3d3cc7ca32193d9955", friend = "0x742d35cc6634c0532925a3b844bc454e4438f44e"
    let action = HLSigner.usdSendAction(destination: friend, amount: "1.5", time: 1_790_000_000_000, mainnet: true)
    #expect(HLGuard.check(action, owner: me, recipients: [friend]).ok)
    // The same action with no approval, or approved for someone else, is refused.
    #expect(!HLGuard.check(action, owner: me, recipients: []).ok)
    #expect(!HLGuard.check(action, owner: me, recipients: ["0x0000000000000000000000000000000000000001"]).ok)
}

/// A Token-2022 transfer (PYUSD is one) passes to the approved owner's account, like an SPL Token one; to anyone else,
/// or an approval through that program, it doesn't.
@Test func guardTreatsToken2022LikeSPLToken() throws {
    let mint = "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo"
    let program = SPL.token2022Program
    let wire = try SPL.transferMessage(payer: me, to: deposit, mint: mint, amount: 1_000_000, decimals: 6, blockhash: me, program: program)
    let m = try SolanaMessage.parse(message: wire)
    let mine = try SPL.associatedAccount(owner: me, mint: mint, program: program)
    let theirs = try SPL.associatedAccount(owner: deposit, mint: mint, program: program)
    let classic = try SPL.associatedAccount(owner: deposit, mint: mint)
    #expect(m.staticKeys.contains(theirs) && !m.staticKeys.contains(classic))
    let owners = [mine: me, theirs: deposit]
    #expect(SolanaGuard.check(m, accounts: m.staticKeys, policy: policy, tokenOwners: owners).ok)
    let elsewhere = SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: me, recipients: []), tokenOwners: [mine: me])
    #expect(!elsewhere.ok)
    let (approve, accounts) = try message([me, mine, scam, program], [(3, [1, 2, 0], [4] + withUnsafeBytes(of: UInt64(5).littleEndian, Array.init))])
    #expect(!SolanaGuard.check(approve, accounts: accounts, policy: policy, tokenOwners: [mine: me]).ok)
}

/// «Send all» of a token closes our account after the transfer; the guard lets the rent come back only to us.
@Test func sendingAllOfATokenClosesOurAccountIntoThisWallet() throws {
    let mint = "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo"
    let wire = try SPL.transferMessage(payer: me, to: deposit, mint: mint, amount: 1_000_000, decimals: 6, blockhash: me, close: true)
    let m = try SolanaMessage.parse(message: wire)
    let mine = try SPL.associatedAccount(owner: me, mint: mint), theirs = try SPL.associatedAccount(owner: deposit, mint: mint)
    let v = SolanaGuard.check(m, accounts: m.staticKeys, policy: policy, tokenOwners: [mine: me, theirs: deposit])
    #expect(v.ok, "\(v.problems)")
    #expect(v.checks.contains { $0.hasPrefix("Temporary account closes") })
    // The same close with the rent going to someone else is refused.
    let (steal, accounts) = try message([me, mine, scam, SPL.tokenProgram], [(3, [1, 2, 0], [9])])
    #expect(!SolanaGuard.check(steal, accounts: accounts, policy: policy, tokenOwners: [mine: me]).ok)
}

@Test func jupiterSwapsPayOnlyThisWalletsOwnAccount() throws {
    let wsol = "So11111111111111111111111111111111111111112", usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    let source = try SPL.associatedAccount(owner: me, mint: usdcMint), out = try SPL.associatedAccount(owner: me, mint: wsol)
    let theirs = try SPL.associatedAccount(owner: deposit, mint: wsol)
    let jup = SolanaGuard.jupiterSwap, event = "D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf"
    // The tail Jupiter's route ends in, as lite-api built it for 1 USDC → SOL: in 1_000_000, quoted 9_386_507, 50 bp, no fee.
    func tail(quoted: UInt64, slippage: UInt16 = 50, fee: UInt8 = 0) -> [UInt8] {
        withUnsafeBytes(of: UInt64(1_000_000).littleEndian, Array.init) + withUnsafeBytes(of: quoted.littleEndian, Array.init)
            + withUnsafeBytes(of: slippage.littleEndian, Array.init) + [fee]
    }
    let route: [UInt8] = [0xe5, 0x17, 0xcb, 0x97, 0x7a, 0xe3, 0xad, 0x2a] + [1, 0, 0, 100, 0, 1]   // a one-step route plan
    // route's accounts: token program, user, source, user destination, destination (the program's id: none), mint,
    // platform fee (none), event authority, program.
    let keys = [me, SPL.tokenProgram, source, out, jup, wsol, event, theirs]
    let (m, accounts) = try message(keys, [(4, [1, 0, 2, 3, 4, 5, 4, 6, 4], route + tail(quoted: 9_386_507))])
    // The output account is this wallet's own for the mint: derived here, no claim needed. No amount on the prompt: the
    // program ignores bytes after its arguments, so a site could append a made-up quote (the 2026-10-22 review).
    let ok = SolanaGuard.check(m, accounts: accounts, policy: policy, tokenOwners: [:])
    #expect(ok.ok)
    #expect(ok.checks.contains("Jupiter swap → your account (SOL). The balance simulation checks the amount."))
    // The output sent to someone else's account, as the user destination or through the optional destination: refused.
    let (other, otherAccounts) = try message(keys, [(4, [1, 0, 2, 7, 4, 5, 4, 6, 4], route + tail(quoted: 9_386_507))])
    #expect(!SolanaGuard.check(other, accounts: otherAccounts, policy: policy, tokenOwners: [:]).ok)
    let (redirect, redirectAccounts) = try message(keys, [(4, [1, 0, 2, 3, 7, 5, 4, 6, 4], route + tail(quoted: 9_386_507))])
    #expect(!SolanaGuard.check(redirect, accounts: redirectAccounts, policy: policy, tokenOwners: [:]).ok)
    // shared_accounts_route: token program, program authority, user, source, program source, program destination,
    // destination, source mint, destination mint, platform fee, Token-2022, event authority, program.
    let shared: [UInt8] = [0xc1, 0x20, 0x9b, 0x33, 0x41, 0xd6, 0x9c, 0x81, 3] + [1, 0, 0, 100, 0, 1]
    let sharedKeys = [me, SPL.tokenProgram, deposit, source, theirs, out, usdcMint, wsol, jup, event]
    let (s, sAccounts) = try message(sharedKeys, [(8, [1, 2, 0, 3, 4, 4, 5, 6, 7, 8, 8, 9, 8], shared + tail(quoted: 9_386_507, fee: 20))])
    let sharedOK = SolanaGuard.check(s, accounts: sAccounts, policy: policy, tokenOwners: [:])
    #expect(sharedOK.ok && sharedOK.checks.contains("Jupiter swap → your account (SOL). The balance simulation checks the amount."))
    let (sTheirs, sTheirsAccounts) = try message(sharedKeys, [(8, [1, 2, 0, 3, 4, 4, 4, 6, 7, 8, 8, 9, 8], shared + tail(quoted: 9_386_507))])
    #expect(!SolanaGuard.check(sTheirs, accounts: sTheirsAccounts, policy: policy, tokenOwners: [:]).ok)
    // Every other swap form is held to its own output account too (exact_out_route: user destination 3, destination mint 6),
    // and an instruction that isn't a swap is refused.
    let exactOutRoute: [UInt8] = [0xd0, 0x33, 0xef, 0x97, 0x7b, 0x2b, 0xed, 0x5c]
    let (exactOut, exactOutAccounts) = try message(keys, [(4, [1, 0, 2, 3, 4, 5, 5, 6, 4], exactOutRoute + tail(quoted: 1))])
    #expect(SolanaGuard.check(exactOut, accounts: exactOutAccounts, policy: policy, tokenOwners: [:]).ok)
    let (exactTheirs, exactTheirsAccounts) = try message(keys, [(4, [1, 0, 2, 7, 4, 5, 5, 6, 4], exactOutRoute + tail(quoted: 1))])
    #expect(!SolanaGuard.check(exactTheirs, accounts: exactTheirsAccounts, policy: policy, tokenOwners: [:]).ok)
    let (claim, claimAccounts) = try message(keys, [(4, [0, 7, 4], [0x3e, 0xc6, 0xd6, 0xc1, 0xa6, 0x13, 0x6f, 0xd2])])
    #expect(!SolanaGuard.check(claim, accounts: claimAccounts, policy: policy, tokenOwners: [:]).ok)
}

/// Past its 7 days the trading key still signs what can't open or add to a position: cancels and reduce-only orders.
@Test func onlyCancelsAndReduceOnlyOrdersCountAsReducing() throws {
    let stop = HLOrder(asset: 0, isBuy: false, price: 90, size: 1, reduceOnly: true, kind: .trigger(price: 90, isMarket: true, tpsl: "sl"))
    let take = HLOrder(asset: 0, isBuy: false, price: 120, size: 1, reduceOnly: true, kind: .trigger(price: 120, isMarket: true, tpsl: "tp"))
    let entry = HLOrder(asset: 0, isBuy: true, price: 100, size: 1, reduceOnly: false, kind: .limit(tif: "Ioc"))
    #expect(HLAction.onlyReduces(try HLAction.order([stop, take], grouping: "positionTpsl")))
    #expect(HLAction.onlyReduces(HLAction.cancel([(asset: 0, oid: 7)])))
    #expect(!HLAction.onlyReduces(try HLAction.order([entry, stop, take], grouping: "normalTpsl")))
    #expect(!HLAction.onlyReduces(try HLAction.order([entry])))
    #expect(!HLAction.onlyReduces(.map([("type", .string("updateLeverage")), ("asset", .uint(0)), ("isCross", .bool(true)), ("leverage", .uint(20))])))
    #expect(!HLAction.onlyReduces(.map([("type", .string("order")), ("orders", .array([]))])))
}

/// Kamino's own deposit of 1 USDC for the public test wallet, as api.kamino.finance/ktx/klend/deposit built it on
/// 2026-10-23: its market sits in a lookup table the signer can't read, so its obligation (a static key) pins it.
private let kaminoDeposit = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAQAHDfA2J2JGp1ud4zSe1CsV4jL2UY/CD1/NTx1k6B+b0lj3QNLyfEYfKen+uhq4rNOUlqnGWwrmKnE/AflBqZO/Ve1aRnW1WggmL3Vf3UQOsZzVOKHVpZVsiwC6TyyASQbL2QJdjpysvXf+cnGWANufIaEAmn/fFCc/tIZaajBN8TKguSJSa23j1GNeN6zOWW7+FAVUuV3/wGHpq8erzrI0Zx3M7ZodtpC82UOFcwLkxH6bh0yN7njVop10qrBF/0KhgirLusdJNuJSIjry4kuNaAHHDA43Wl/ptAb7msWZE5sqeijp9PaxtFn7emfPXWJQfuJoNup09KhY5ahANQgjU4oCd6avlzObesiNGJLJBEb1AAIwkmb2LlPBGCRJggAAAIyXJY9OJInxuz0QKRSODYMLWhOZ2v8QhASOe9jb6fhZAwZGb+UhFzL/7K26csOb57yM5bvF9xJrLEObOkAAAADYsBAXY9PlHxJuYVbehd6MYTBZuERo0No/6KKiJRzHAQSyrLESWMzjaCxBi6hy/z35EQJxLxWvEra+abNDWwAIRyEjfpVN/N3e52di4+JWwE44ud+B8DgfaI2MVokGb7wJCgAFAkBCDwAJBgABABQSFwEBCAQCAAASDQAAAAC4OnEbAAAAAP4MBgAAAwwWEih1qbBFxRcPolpGdbVaCCYvdV/dRA6xnNU4odWllWyLALpPLIBJBsvZDAkAAAUTEhIDFhIK+wrnTBsLn2AAAAwLAAAFBxARBBMLFhIJiD8PutOYqKQADAYQEwwMDAYIAtqK60/JGWYMAhMFCCGEk+SXwEhZDBEABRMHEBQPDg0BDBcXFQQRCxDY4L8bzJdmr0BCDwAAAAAAAdPybyS0lhPzfd731GDYyADDreDn/OIlObPzdaF5EZpeBYVEhJaXBggCngUEBw=="

@Test func kaminoRunsOnlyInstructionsThatPayTheSignerOnItsOwnPosition() throws {
    let owner = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    #expect(SolanaGuard.kaminoObligation(owner: owner) == "EnxJnydiZPsyQx2ZmXdg7vfggsRLQWpogz19cP6R9SPb")
    let wire = try #require(Data(base64Encoded: kaminoDeposit))
    func verdict(_ wire: Data) throws -> Verdict {
        let m = try SolanaMessage.parse(transaction: wire)
        // The lookup table's entries as an app might claim them: the check mustn't depend on them.
        let looked = m.lookups.flatMap { $0.writable + $0.readonly }.indices.map { Base58.encode(Data(repeating: UInt8($0 + 1), count: 32)) }
        return SolanaGuard.check(m, accounts: m.staticKeys + looked, policy: SolanaPolicy(owner: owner, trustLookupTables: false), tokenOwners: [:])
    }
    let real = try verdict(wire)
    #expect(real.ok, "\(real.problems)")
    // Someone else's obligation (another market's, or another owner's) in its place.
    let obligation = try Base58.decode("EnxJnydiZPsyQx2ZmXdg7vfggsRLQWpogz19cP6R9SPb")
    let at = try #require(wire.range(of: obligation))
    var swapped = wire
    swapped.replaceSubrange(at, with: Data(repeating: 9, count: 32))
    #expect(try !verdict(swapped).ok)
    // deposit_reserve_liquidity, which mints the collateral to any account, in place of the obligation deposit.
    let v2 = Data([0xd8, 0xe0, 0xbf, 0x1b, 0xcc, 0x97, 0x66, 0xaf])
    let d = try #require(wire.range(of: v2))
    var mintsElsewhere = wire
    mintsElsewhere.replaceSubrange(d, with: Data([0xa9, 0xc9, 0x1e, 0x7e, 0x06, 0xcd, 0x66, 0x44]))
    #expect(try !verdict(mintsElsewhere).ok)
}

/// Jupiter Lend's deposit and withdrawal of 1 USDC and a Jupiter DCA order of 100 USDC → SOL for the public test wallet,
/// as lite-api.jup.ag and api.jup.ag built them on 2026-10-23: each pays only the signer's own accounts.
private let jupiterLendDeposit = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAkS8DYnYkanW53jNJ7UKxXiMvZRj8IPX81PHWToH5vSWPccj71J70/rvu0J/yTO1674UGxqeljWsbxMzyBLGGTIhUDS8nxGHynp/roauKzTlJapxlsK5ipxPwH5QamTv1XtZfXf/XyEr3Wopfe9zqFiLis6v4f8U6hxuQhWGjqLG95uxTRcaS26p8mN5qM4XiYgYQGzhBlklxtkIakhwHV1nAIRE+8/cf8FyV6ejOFzFvFRe8v54cSEIbvrhfgwTQQYeXyzPK1sDIBzsh2kxDdkBYXtOfawRnLIelHWBYy3JvqgC5ZIwUW4+UWatmCd5Vc8UzUhP8EscD1FvC2CJQgA1veA4oganO3BaPj9y3TzLqfiwk9dDCxaIuRcmdJ+MiQSAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABHJ/EQ+a2cgo929kauPWYlbJknEb75921PaoQRwAgT1Uepp778BbCkWRDvAOfH/e3tf8kYkOWoSREcIpwuparJSaJkbJuRq/cvVvsHRwHJwyp3lX50EzExEKvsHW7PCICMlyWPTiSJ8bs9ECkUjg2DC1oTmdr/EIQEjnvY2+n4Wcb6evO+2606PWXzaqvJdDGxu+TC0vbg5HymAgNFL11hCv4bkS5IXpX9FespN9/8SzejFtCmOBL/ArpJtMbBjR4K/h6X5FbLuS5kKiW1C6Y5b/JIFkvKdGljqEWSrNbMwwbd9uHXZaGT2cvhRs7reawctIXtX1s3kTqM9YV+/wCpCNTKvRKW8ADZ4CtpnFmGh8XjZiJJXy59jLiWltOq8WoBDxEAAgQOCgEGBQgLBwMQDBENCRDyI8aJUuHytkBCDwAAAAAA"
private let jupiterLendWithdraw = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAkT8DYnYkanW53jNJ7UKxXiMvZRj8IPX81PHWToH5vSWPccj71J70/rvu0J/yTO1674UGxqeljWsbxMzyBLGGTIhUDS8nxGHynp/roauKzTlJapxlsK5ipxPwH5QamTv1XtZfXf/XyEr3Wopfe9zqFiLis6v4f8U6hxuQhWGjqLG95uxTRcaS26p8mN5qM4XiYgYQGzhBlklxtkIakhwHV1nAIRE+8/cf8FyV6ejOFzFvFRe8v54cSEIbvrhfgwTQQYeXyzPK1sDIBzsh2kxDdkBYXtOfawRnLIelHWBYy3JvqgC5ZIwUW4+UWatmCd5Vc8UzUhP8EscD1FvC2CJQgA1veA4oganO3BaPj9y3TzLqfiwk9dDCxaIuRcmdJ+MiQSBDEN4MUqaL/buRQj3/nOWgSj23k04ooSfCTii9x91BEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEcn8RD5rZyCj3b2Rq49ZiVsmScRvvn3bU9qhBHACBPVR6mnvvwFsKRZEO8A58f97e1/yRiQ5ahJERwinC6lqslJomRsm5Gr9y9W+wdHAcnDKneVfnQTMTEQq+wdbs8IgIyXJY9OJInxuz0QKRSODYMLWhOZ2v8QhASOe9jb6fhZxvp6877brTo9ZfNqq8l0MbG75MLS9uDkfKYCA0UvXWEK/huRLkhelf0V6yk33/xLN6MW0KY4Ev8Cukm0xsGNHgr+HpfkVsu5LmQqJbULpjlv8kgWS8p0aWOoRZKs1szDBt324ddloZPZy+FGzut5rBy0he1fWzeROoz1hX7/AKmXNQm9Ur4PKo8EBMUD3U1E8ZHbXvOOAjdI/QFJYEEJLgEQEgAEAgsBDwYFCAwHCQMRDRIOChC3EkaclG2hIkBCDwAAAAAA"
private let jupiterDCAOrder = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAgN8DYnYkanW53jNJ7UKxXiMvZRj8IPX81PHWToH5vSWPdA0vJ8Rh8p6f66Gris05SWqcZbCuYqcT8B+UGpk79V7Yq3C3dSmy6SMXmaC86a+QNB8cM57M8nvXfavvug7xwr4nQ/DK+NdbsMGe0wCTT41rh+ExTnoFi4Cj6ruTycByXsYF21KZtx/G85tfoFGdt2xIhsqTLuddL1Tk70GQ1aUgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAwZGb+UhFzL/7K26csOb57yM5bvF9xJrLEObOkAAAAAGm4hX/quBhPtof2NGGMA12sQ53BrrO1WYoPAAAAAAAQbd9uHXZaGT2cvhRs7reawctIXtX1s3kTqM9YV+/wCpjJclj04kifG7PRApFI4NgwtaE5na/xCEBI572Nvp+FmwdagGY3nKb+NwN/8MKNVUpuTWNtnoUYz+brrpP1I2/rUn1F1kDj7BX2FdWw4jpUqWuD8Lggv3BjXyQ0rGQMwExvp6877brTo9ZfNqq8l0MbG75MLS9uDkfKYCA0UvXWFZnavCXYB4DeVN4DdHU86ACyRfTVP8tOOh1oPI9Z1GlQQGAAUCAAAAAAYACQNADQMAAAAAAAkGAAEADAUIAQELDQQAAAwHAQMCBQgJCgsrjncrbaI0C7EyNdtqAAAAAADh9QUAAAAAgPD6AgAAAACAUQEAAAAAAAAAAA=="

/// JupiterZ's fill (a market maker pays the fee, signs first and swaps at its quote; 2026-10-28): passes only with this
/// wallet as the taker and what it gets landing in its own account, or as SOL straight to it (the output account left
/// as the program's id, Anchor's «none»). Accounts as its IDL has them: taker, maker, taker's and maker's input, taker's
/// and maker's output, input mint and program, output mint and program, System.
@Test func jupiterZFillsPayOnlyThisWallet() throws {
    let wsol = "So11111111111111111111111111111111111111112", usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    let maker = deposit, z = SolanaGuard.jupiterZ
    let myUSDC = try SPL.associatedAccount(owner: me, mint: usdcMint), myWSOL = try SPL.associatedAccount(owner: me, mint: wsol)
    let makerUSDC = try SPL.associatedAccount(owner: maker, mint: usdcMint), makerWSOL = try SPL.associatedAccount(owner: maker, mint: wsol)
    // fill(input_amount 15 USDC, output_amount 0.148288542 SOL, expire_at) and the five bytes the program ignores.
    func le(_ n: UInt64) -> [UInt8] { withUnsafeBytes(of: n.littleEndian, Array.init) }
    let fill: [UInt8] = [0xa8, 0x60, 0xb7, 0xa3, 0x5c, 0x0a, 0x28, 0xa0] + le(15_000_000) + le(148_288_542) + le(1_793_192_203) + [0, 0, 0, 0, 0]
    let keys = [maker, me, myUSDC, makerUSDC, z, makerWSOL, SolanaGuard.system, usdcMint, SPL.tokenProgram, wsol, myWSOL]
    let sponsored = SolanaPolicy(owner: me, sponsor: maker)
    func check(_ accounts: [Int], data: [UInt8] = fill, policy: SolanaPolicy = sponsored, twice: Bool = false) throws -> Verdict {
        let (m, list) = try message(keys, [(4, accounts, data)] + (twice ? [(4, accounts, data)] : []), signers: 2)
        return SolanaGuard.check(m, accounts: list, policy: policy, tokenOwners: [:])
    }
    // USDC in, SOL out as lamports: the output account is the program's id.
    let sol = try check([1, 0, 2, 3, 4, 5, 7, 8, 9, 8, 6])
    #expect(sol.ok, "\(sol.problems)")
    #expect(sol.checks.contains("JupiterZ fill: 15 USDC for 0.148288542 SOL → your wallet, as SOL. The balance simulation checks it."))
    // The maker only swaps: no SOL of the wallet's goes to it, so the signer doesn't keep it as an address money may go to.
    let (m, list) = try message(keys, [(4, [1, 0, 2, 3, 4, 5, 7, 8, 9, 8, 6], fill)], signers: 2)
    #expect(!SolanaGuard.refundsSponsor(m, accounts: list, owner: me, sponsor: maker))
    let (refund, refundList) = try message([maker, me, SolanaGuard.system], [(2, [1, 0], solTransfer(2_039_280))], signers: 2)
    #expect(SolanaGuard.refundsSponsor(refund, accounts: refundList, owner: me, sponsor: maker))
    // Into this wallet's own wSOL account: fine too.
    #expect(try check([1, 0, 2, 3, 10, 5, 7, 8, 9, 8, 6]).ok)
    // The output to the maker's account, «none» for a mint that isn't SOL, someone else as the taker, or another
    // instruction of the program: refused.
    #expect(try !check([1, 0, 2, 3, 5, 5, 7, 8, 9, 8, 6]).ok)
    #expect(try !check([1, 0, 2, 3, 4, 5, 9, 8, 7, 8, 6]).ok)
    #expect(try !check([0, 1, 3, 2, 4, 5, 7, 8, 9, 8, 6]).ok)
    #expect(try !check([1, 0, 2, 3, 4, 5, 7, 8, 9, 8, 6], data: [0, 1, 2, 3, 4, 5, 6, 7] + [UInt8](repeating: 1, count: 29)).ok)
    // What it gives drawn on another account of the wallet (its wSOL for a USDC fill), «none» for an input that isn't SOL,
    // or a second fill riding along: refused — the app's simulation watches only the swap's own accounts.
    #expect(try !check([1, 0, 10, 3, 4, 5, 7, 8, 9, 8, 6]).ok)
    #expect(try !check([1, 0, 4, 3, 4, 5, 7, 8, 9, 8, 6]).ok)
    #expect(try !check([1, 0, 2, 3, 4, 5, 7, 8, 9, 8, 6], twice: true).ok)
    // Without the maker as the named fee payer the two signatures are refused, as for any transaction.
    #expect(try !check([1, 0, 2, 3, 4, 5, 7, 8, 9, 8, 6], policy: SolanaPolicy(owner: me)).ok)
}

@Test func jupiterLendAndDCAPayOnlyTheSignersOwnAccounts() throws {
    let owner = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    func verdict(_ wire: Data) throws -> Verdict {
        let m = try SolanaMessage.parse(transaction: wire)
        return SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: owner, trustLookupTables: false), tokenOwners: [:])
    }
    for b64 in [jupiterLendDeposit, jupiterLendWithdraw, jupiterDCAOrder] {
        let v = try verdict(try #require(Data(base64Encoded: b64)))
        #expect(v.ok, "\(v.problems)")
    }
    // Each one's payee swapped for someone else's account: the fToken account of the deposit, the USDC account of the
    // withdrawal, the order's own USDC source.
    let usdcATA = try Base58.decode("5N3f1tj9v1vc5TUZ8S7mCAnVmjVKrfnzXWhxLaxyZAgt"), fTokenATA = try Base58.decode("8TQFpKF11utuu39gVXVv7aGahirPsf4y72XQFkUNYnoM")
    for (b64, key) in [(jupiterLendDeposit, fTokenATA), (jupiterLendWithdraw, usdcATA), (jupiterDCAOrder, usdcATA)] {
        var wire = try #require(Data(base64Encoded: b64))
        let at = try #require(wire.range(of: key))
        wire.replaceSubrange(at, with: Data(repeating: 9, count: 32))
        #expect(try !verdict(wire).ok)
    }
}

/// A borrow and a repay on Kamino's Backpack market pass only on the owner's own position there, the one the guard
/// derives itself; the same instruction on anyone else's position, or with an instruction ihatecrypto doesn't use, is refused.
@Test func kaminoBackpackLoansRunOnlyOnTheOwnersPosition() throws {
    let owner = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    let mine = try #require(SolanaGuard.kaminoObligation(owner: owner, market: SolanaGuard.kaminoBackpackMarket))
    #expect(mine != SolanaGuard.kaminoObligation(owner: owner))
    func verdict(_ discriminator: [UInt8], obligation: String) throws -> Verdict {
        let ix = SolanaInstruction(program: SolanaGuard.kaminoLend, accounts: [
            .init(pubkey: owner, isSigner: true, isWritable: false), .init(pubkey: obligation, isSigner: false, isWritable: true),
            .init(pubkey: SolanaGuard.kaminoBackpackMarket, isSigner: false, isWritable: false),
        ], data: Data(discriminator) + Data(repeating: 0, count: 8))
        let wire = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([ix], payer: owner, blockhash: deposit))
        let m = try SolanaMessage.parse(transaction: wire)
        return SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: owner, trustLookupTables: false), tokenOwners: [:])
    }
    let borrowV2: [UInt8] = [0xa1, 0x80, 0x8f, 0xf5, 0xab, 0xc7, 0xc2, 0x06], repayV2: [UInt8] = [0x74, 0xae, 0xd5, 0x4c, 0xb4, 0x35, 0xd2, 0x90]
    let repay: [UInt8] = [0x91, 0xb2, 0x0d, 0xe1, 0x4c, 0xf0, 0x93, 0x48]
    for d in [borrowV2, repayV2, repay] {
        let ok = try verdict(d, obligation: mine)
        #expect(ok.ok, "\(ok.problems)")
        // Someone else's debt, or another wallet's position: refused.
        let other = try #require(SolanaGuard.kaminoObligation(owner: me, market: SolanaGuard.kaminoBackpackMarket))
        #expect(try !verdict(d, obligation: other).ok)
    }
    // A flash loan on the same position: not an instruction ihatecrypto uses.
    #expect(try !verdict([0x87, 0xe7, 0x34, 0xa7, 0x07, 0x34, 0xd4, 0xc1], obligation: mine).ok)
}
