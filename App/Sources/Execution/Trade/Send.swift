import EVMChain
import ExecutionCore
import HyperliquidSigning
import Foundation
import IhatecryptoCore
import RelayBridge
import SolanaChain
import TxGuard
import WalletCore

/// USDC to an address from the user's address book: on Solana directly, on Base from Base USDC (bridged from Solana
/// when short). The recipient is passed to the guard and the signer as the one approved address; nothing else may
/// receive money, and the simulation must show exactly the amount leaving.
extension Executor {
    public enum Destination: Sendable {
        case solana(String)
        case base(String)
        case arbitrum(String)
        /// USDG on Robinhood Chain.
        case robinhood(String)
        /// Another Hyperliquid account: USDC from the perp balance; what it lacks is bridged in first.
        case hyperliquid(String)
        case tempo(String)
        case ethereum(String)
        case polygon(String)
        case hyperevm(String)
        case optimism(String)
        case bnb(String)
    }

    public func sendUSDC(_ amount: Decimal, to destination: Destination, label: String) async throws -> String {
        let units = NSDecimalNumber(decimal: (amount * 1_000_000).rounded(0, .down)).uint64Value
        guard units > 0 else { throw ExecutionError("Nothing to send.") }
        switch destination {
        case .solana(let owner): return try await sendSolanaUSDC(units, to: owner, label: label)
        case .base(let recipient): return try await sendEVMUSDC(units, on: .base, to: recipient, label: label)
        case .arbitrum(let recipient): return try await sendEVMUSDC(units, on: .arbitrum, to: recipient, label: label)
        case .robinhood(let recipient): return try await sendEVMUSDC(units, on: .robinhood, to: recipient, label: label)
        case .hyperliquid(let recipient): return try await sendHyperliquidUSDC(amount, to: recipient, label: label)
        case .tempo(let recipient): return try await sendEVMUSDC(units, on: .tempo, to: recipient, label: label)
        case .ethereum(let recipient): return try await sendEVMUSDC(units, on: .ethereum, to: recipient, label: label)
        case .polygon(let recipient): return try await sendEVMUSDC(units, on: .polygon, to: recipient, label: label)
        case .hyperevm(let recipient): return try await sendEVMUSDC(units, on: .hyperevm, to: recipient, label: label)
        case .optimism(let recipient): return try await sendEVMUSDC(units, on: .optimism, to: recipient, label: label)
        case .bnb(let recipient): return try await sendEVMUSDC(units, on: .bnb, to: recipient, label: label)
        }
    }

    /// A payment for a service on Tempo (MPP «charge», push mode): USDC to the payee named in the service's challenge,
    /// with the memo it asked for; what's missing comes from Solana first. The hash is the proof the service checks.
    public func payTempo(_ units: UInt64, to payee: String, memo: Data?, label: String) async throws -> String {
        guard units > 0 else { throw ExecutionError("Nothing to pay.") }
        let net = EVMNetwork.tempo
        // Short there: a dollar comes over (or the price, if more), so the next requests don't each pay a bridge.
        let there = try await net.usdcBalance(wallet.evm)
        try await ensureEVM(net, usdc: there >= units + net.feeReserve ? units : max(units, Self.tempoFloat))
        let data = try memo.map { try ERC20.transfer(to: payee, amount: units, memo: $0) } ?? ERC20.transfer(to: payee, amount: units)
        return try await sendEVM(net.rpc, to: net.usdc, data: data, note: "Pay \(Self.usdc(units)) USDC to \(label)", recipients: [payee])
    }

    /// What a payment brings to Tempo when it's short there: a dollar, the user's ask (2026-09-30: «предлагай сразу
    /// перевести 1 бакс… чтобы по многу раз не переводить»).
    public static let tempoFloat: UInt64 = 1_000_000

    private func sendEVMUSDC(_ units: UInt64, on net: EVMNetwork, to recipient: String, label: String) async throws -> String {
        try await ensureEVM(net, usdc: units)
        let hash = try await sendEVM(net.rpc, to: net.usdc, data: try ERC20.transfer(to: recipient, units: net.raw(units)),
                                     note: "Send \(Self.usdc(units)) USDC to \(label)", recipients: [recipient])
        return "Sent \(Self.usdc(units)) USDC to \(label) on \(net.name). \(net.explorer)/tx/\(hash)"
    }

    /// usdSend from the perp balance, signed by the main key (the trading key can't move money out); the guard and the
    /// signer accept only this destination.
    private func sendHyperliquidUSDC(_ amount: Decimal, to destination: String, label: String) async throws -> String {
        // To the micro-dollar, not the cent: «send all» leaves nothing behind.
        let usdc = amount.rounded(6, .down)
        let to = destination.lowercased()
        guard to != wallet.evm.lowercased() else { throw ExecutionError("That is this wallet's own Hyperliquid account.") }
        // The first transfer to an account Hyperliquid hasn't seen yet loses 1 USDC on the way (its ledger, 2026-10-22: sent
        // 2, fee 1, 1 arrived): 1 more goes, so the amount asked arrives.
        let opening: Decimal = (try? await Hyperliquid.isNewAccount(to)) == true ? 1 : 0
        try await ensureHyperliquid(usdc: usdc + opening)
        let free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        guard usdc > 0, usdc + opening <= free else {
            throw ExecutionError("\(Fmt.usd(free)) free on Hyperliquid; can't send \(Fmt.num(usdc))"
                                 + (opening > 0 ? " plus the 1 USDC Hyperliquid keeps once for a new account." : "."))
        }
        let time = UInt64(Date().timeIntervalSince1970 * 1000)
        let action = HLSigner.usdSendAction(destination: to, amount: "\(usdc + opening)", time: time, mainnet: true)
        let signature = try await signer.signHyperliquidUser(action, recipients: [to])
        try journal.append(.init(chain: "hyperliquid", id: "\(time)", kind: .signed, note: "send \(Fmt.num(usdc)) USDC to \(label)"))
        let reply = HLReply.parse(try await HLExchange.send(action: action, nonce: time, signature: signature, mainnet: true))
        if case .error(let e) = reply {
            try journal.append(.init(chain: "hyperliquid", id: "\(time)", kind: .failed, note: e))
            throw ExecutionError("Hyperliquid refused the transfer: \(e).")
        }
        try journal.append(.init(chain: "hyperliquid", id: "\(time)", kind: .confirmed, note: "\(reply)"))
        return "Sent \(Fmt.num(usdc)) USDC to \(label) on Hyperliquid" + (opening > 0 ? ", plus 1 USDC Hyperliquid kept once for opening their account." : ".")
    }

    /// «move 11 USDC from Solana to Hyperliquid»: that much USDC from Solana over Relay to the wallet's own Hyperliquid account;
    /// the first deposit opens it (Relay's price then takes the ≈ $1 opening fee). Waits until it lands.
    public func solanaToHyperliquid(usdc: Decimal) async throws -> String {
        guard usdc >= Self.minBridgeUSDC else { throw ExecutionError("At least \(Fmt.num(Self.minBridgeUSDC)) USDC: less isn't worth the bridge.") }
        let (step, changes) = try await prepareBridge(.send(usdc))
        report("\(Fmt.num(step.spend)) USDC from Solana → ≈ \(Fmt.num(step.arriving)) USDC on Hyperliquid. " + changes.joined(separator: " "))
        try await sendBridge(step)
        let now = (try? await Hyperliquid.balance(user: wallet.evm)).map { $0.withdrawable + $0.spotUSDC }
        return "Moved ≈ \(Fmt.num(step.arriving)) USDC to Hyperliquid" + (now.map { ": \(Fmt.num($0)) USDC free there now." } ?? ".")
            + " Its smallest order is $10. Say “withdraw USDC to Solana” to bring it back."
    }

    /// What Hyperliquid lacks for a send comes over first, as ensureEVM does for an EVM network: from Solana over Relay,
    /// else straight from the EVM network holding it. «Send 150 USDC to HL wallet» with $3.96 there and the money on
    /// Solana failed at the signature (2026-10-22).
    func ensureHyperliquid(usdc want: Decimal) async throws {
        let free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        guard want > free else { return }
        let short = want - free
        let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        if onSolana < short + Decimal(string: "0.1")! {
            let idle = Dictionary(await idleUSDC().map { ($0.net.key, $0.usdc) }, uniquingKeysWith: +)
            if let (from, send) = EVMNetwork.source(arrive: short, onSolana: onSolana, target: "hyperliquid", held: { idle[$0] }) {
                report("Hyperliquid has \(Fmt.usd(free)) and Solana \(Fmt.num(onSolana)) USDC: bringing \(Fmt.num(send)) straight from \(from.name).")
                report(try await bridge(from, to: nil, hyperliquid: true, usdc: send))
                // Inside a chain the bridge returns before it lands; the send needs the money there.
                if let landings = Self.landings { for line in try await landings.wait(for: "hyperliquid") { report(line) } }
                return
            }
        }
        let (step, changes) = try await prepareBridge(.arrive(Self.bridgeArrive(need: short, margin: 0, solanaUSDC: onSolana)))
        report("Hyperliquid has \(Fmt.usd(free)): \(Fmt.num(step.spend)) USDC from Solana → \(Fmt.num(step.arriving)) USDC there first. " + changes.joined(separator: " "))
        try await sendBridge(step)
    }

    static func usdc(_ units: UInt64) -> String { Fmt.num(Decimal(units) / 1_000_000, maxFraction: 6) }

    func sendSolanaUSDC(_ units: UInt64, to owner: String, label: String) async throws -> String {
        try await sendSPL(mint: Relay.Currency.solanaUSDC, units: units, decimals: 6, symbol: "USDC", to: owner, label: label)
    }

    /// An SPL token from the wallet's own account to `owner`'s, made for them when they have none (≈ 0.002 SOL of rent,
    /// paid here); the simulation must show exactly `units` leaving. `program`: the mint's, SPL Token or Token-2022.
    func sendSPL(mint: String, units: UInt64, decimals: UInt8, symbol: String, program: String = SPL.tokenProgram,
                 to owner: String, label: String, close: Bool = false) async throws -> String {
        func f(_ u: UInt64) -> String { Fmt.num(Decimal(u) / pow(Decimal(10), Int(decimals)), maxFraction: Int(decimals)) }
        let have = try await rpc.tokenBalance(owner: wallet.solana, mint: mint)
        guard have >= units else { throw ExecutionError("\(f(have)) \(symbol) on Solana; the transfer needs \(f(units)).") }
        let (blockhash, lastValid) = try await rpc.latestBlockhash()
        let unsigned = SolanaTransaction.unsigned(try SPL.transferMessage(payer: wallet.solana, to: owner, mint: mint, amount: units, decimals: decimals,
                                                                          blockhash: blockhash, program: program, close: close && units == have))
        let message = try SolanaMessage.parse(transaction: unsigned)
        let mine = try SPL.associatedAccount(owner: wallet.solana, mint: mint, program: program)
        let owners = [mine: wallet.solana, try SPL.associatedAccount(owner: owner, mint: mint, program: program): owner]
        var verdict = SolanaGuard.check(message, accounts: message.staticKeys, policy: SolanaPolicy(owner: wallet.solana, recipients: [owner]), tokenOwners: owners)
        let solBefore = try await rpc.balance(wallet.solana)
        let simulation = try await rpc.simulate(unsigned, watch: [mine, wallet.solana], sigVerify: false)
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let error = simulation.error { throw ExecutionError("The simulation failed, so nothing was signed: \(error)") }
        verdict.merge(SolanaGuard.balances([
            .init(label: symbol, before: Int64(have), after: Int64(simulation.after[mine]?.token ?? 0), atLeast: -Int64(units)),
            // The fee, plus the recipient's token account rent (≈ 0.002 SOL) when they have none yet.
            .init(label: "lamports", before: Int64(solBefore), after: Int64(simulation.after[wallet.solana]?.lamports ?? 0), atLeast: -3_000_000),
        ]))
        guard verdict.ok else { throw GuardStop("The guard stopped the transfer: " + verdict.problems.joined(separator: " ")) }
        let signed = try await signer.signSolana(unsigned, accounts: message.staticKeys, recipients: [owner], tokenOwners: owners)
        guard signed.count == unsigned.count, signed.subdata(in: 65..<signed.count) == unsigned.subdata(in: 65..<unsigned.count) else {
            throw GuardStop("What was signed isn't what was checked.")
        }
        let signature = Base58.encode(signed.subdata(in: 1..<65))
        try journal.append(.init(chain: "solana", id: signature, kind: .signed, note: "send \(f(units)) \(symbol) to \(label); valid until block \(lastValid)"))
        switch try await broadcastSolana(signed, signature: signature, lastValid: lastValid) {
        case .confirmed:
            try journal.append(.init(chain: "solana", id: signature, kind: .confirmed))
            return "Sent \(f(units)) \(symbol) to \(label) on Solana. https://solscan.io/tx/\(signature)"
        case .failed(let e):
            try journal.append(.init(chain: "solana", id: signature, kind: .failed, note: e))
            throw ExecutionError("The transfer failed: \(e). The money stayed.")
        case .expired:
            try journal.append(.init(chain: "solana", id: signature, kind: .expired))
            throw ExecutionError("The transfer didn't make it into a block; nothing moved. You can retry.")
        }
    }
}

/// Native NEAR and SOL to saved addresses, and closing the wallet's NEAR account. The receiver is passed to the
/// signer as the one approved address (a new one asks for Touch ID there); the signer builds NEAR transactions itself.
extension Executor {
    /// NEAR to a saved NEAR account; nil = all but the account's storage and 0.01 NEAR — a transfer that opens a new
    /// implicit account prepays ≈ 0.0076 NEAR of gas at NEAR's pessimistic price (measured 2026-09-28), refunded after.
    public func sendNear(_ requested: Decimal?, to receiver: String, account: String, label: String) async throws -> String {
        guard receiver != account else { throw ExecutionError("That is this wallet's own NEAR account.") }
        // A named account can't be created by a transfer: sending to one that doesn't exist would only fail.
        if receiver.wholeMatch(of: /[0-9a-f]{64}/) == nil, (try? await Near.balance(account: receiver))?.near ?? 0 == 0 {
            throw ExecutionError("\(receiver) doesn't exist on NEAR: check the name in Addresses.")
        }
        let context = try await Near.txContext(account: account)
        let micro: UInt128 = 1_000_000_000_000_000_000   // 10⁻⁶ NEAR
        let balance = UInt128(context.balance) ?? 0
        let keep = UInt128(context.storageBytes) * 10_000_000_000_000_000_000 + 10_000_000_000_000_000_000_000
        let free = balance > keep ? (balance - keep) / micro * micro : 0
        let yocto = requested.flatMap { UInt128(NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).stringValue) }.map { $0 * micro } ?? free
        guard yocto > 0, yocto <= free else {
            throw ExecutionError("\(NearTransaction.near(free)) NEAR can be sent: the account keeps \(NearTransaction.near(keep)) for its storage and the fee.")
        }
        let signed = try await signer.signNearTransfer(amount: yocto, receiver: receiver, recipients: [receiver], nonce: context.nonce, blockHash: context.blockHash)
        let hash = try await Near.send(signed)
        try journal.append(.init(chain: "near", id: hash, kind: .confirmed, note: "send \(NearTransaction.near(yocto)) NEAR to \(label)"))
        return "Sent \(NearTransaction.near(yocto)) NEAR to \(label). https://nearblocks.io/txns/\(hash)"
    }

    /// Deletes the wallet's NEAR account; NEAR sends everything on it, storage included, to `beneficiary`. Refused
    /// unless the beneficiary exists: NEAR would burn the balance otherwise.
    public func closeNear(into beneficiary: String, account: String, label: String) async throws -> String {
        guard beneficiary != account else { throw ExecutionError("An account can't be closed into itself.") }
        guard (try? await Near.balance(account: beneficiary))?.near ?? 0 > 0 else {
            throw ExecutionError("\(label) doesn't exist on NEAR yet: closing into it would burn the NEAR. Send it a little NEAR first.")
        }
        let context = try await Near.txContext(account: account)
        let signed = try await signer.signNearDeleteAccount(beneficiary: beneficiary, recipients: [beneficiary], nonce: context.nonce, blockHash: context.blockHash)
        let hash = try await Near.send(signed)
        try journal.append(.init(chain: "near", id: hash, kind: .confirmed, note: "NEAR account closed into \(label)"))
        return "Closed your NEAR account: its \(NearTransaction.near(UInt128(context.balance) ?? 0)) NEAR went to \(label). https://nearblocks.io/txns/\(hash)"
    }

    /// SOL to a saved Solana address: only that recipient (the guard and the signer check), and the simulation must
    /// show exactly this SOL leaving, plus the fee. nil = all of it: the wallet's empty token accounts close in the same
    /// transaction and their rent goes too, so only nothing stays (Solana keeps an account either empty or above its
    /// rent minimum, so «all but a little» can't be sent).
    public func sendSOL(_ sol: Decimal?, to owner: String, label: String) async throws -> String {
        guard owner != wallet.solana else { throw ExecutionError("That is this wallet's own address.") }
        let have = try await rpc.balance(wallet.solana)
        let empties = sol == nil ? try await emptyTokenAccounts() : []
        let all = have + empties.reduce(0) { $0 + $1.lamports }
        // No priority fee on «all»: the fee is then exactly Solana's 5,000 lamports a signature, and nothing is left over.
        let lamports = sol.map { NSDecimalNumber(decimal: ($0 * 1_000_000_000).rounded(0, .down)).uint64Value } ?? (all > 5_000 ? all - 5_000 : 0)
        let shown = Fmt.num(Decimal(lamports) / 1_000_000_000, maxFraction: 9)
        guard lamports > 0 else { throw ExecutionError("Nothing to send.") }
        guard sol == nil || have >= lamports + 20_000 else {
            throw ExecutionError("\(Fmt.num(Decimal(have) / 1_000_000_000, maxFraction: 9)) SOL on Solana: not enough for \(shown) and the fee.")
        }
        // An empty account opens only with Solana's rent minimum; less than that is refused by the network.
        if try await rpc.balance(owner) == 0, lamports < 890_880 {
            throw ExecutionError("\(label)'s Solana account is empty: Solana opens it only with at least 0.00089088 SOL (its rent minimum). Send at least that.")
        }
        let (blockhash, lastValid) = try await rpc.latestBlockhash()
        let transfer = SolanaInstruction(program: SolanaGuard.system, accounts: [
            .init(pubkey: wallet.solana, isSigner: true, isWritable: true), .init(pubkey: owner, isSigner: false, isWritable: true),
        ], data: Data([2, 0, 0, 0]) + withUnsafeBytes(of: lamports.littleEndian) { Data($0) })
        // CloseAccount(account, destination, owner): each empty token account's rent comes back first, then all of it goes.
        let closes = empties.map { a in
            SolanaInstruction(program: SPL.tokenProgram, accounts: [
                .init(pubkey: a.address, isSigner: false, isWritable: true), .init(pubkey: wallet.solana, isSigner: false, isWritable: true),
                .init(pubkey: wallet.solana, isSigner: true, isWritable: false),
            ], data: Data([9]))
        }
        let unsigned = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage(closes + [transfer], payer: wallet.solana, blockhash: blockhash,
                                                                                       microLamportsPerCU: sol == nil ? 0 : 20_000))
        let message = try SolanaMessage.parse(transaction: unsigned)
        let owners = Dictionary(uniqueKeysWithValues: empties.map { ($0.address, wallet.solana) })
        var verdict = SolanaGuard.check(message, accounts: message.staticKeys, policy: SolanaPolicy(owner: wallet.solana, recipients: [owner]), tokenOwners: owners)
        let simulation = try await rpc.simulate(unsigned, watch: [wallet.solana], sigVerify: false)
        if let error = simulation.error { throw ExecutionError("The simulation failed, so nothing was signed: \(error)") }
        verdict.merge(SolanaGuard.balances([
            .init(label: "lamports", before: Int64(have), after: Int64(simulation.after[wallet.solana]?.lamports ?? 0), atLeast: -Int64(lamports) - 20_000),
        ]))
        // «All» means nothing stays: a fee other than counted shows here, before anything is signed.
        if sol == nil, let left = simulation.after[wallet.solana]?.lamports, left != 0 {
            throw ExecutionError("The simulation leaves \(Fmt.num(Decimal(left) / 1_000_000_000, maxFraction: 9)) SOL behind, so nothing was signed. Send a number instead.")
        }
        guard verdict.ok else { throw GuardStop("The guard stopped the transfer: " + verdict.problems.joined(separator: " ")) }
        let signed = try await signer.signSolana(unsigned, accounts: message.staticKeys, recipients: [owner], tokenOwners: owners)
        guard signed.count == unsigned.count, signed.subdata(in: 65..<signed.count) == unsigned.subdata(in: 65..<unsigned.count) else {
            throw GuardStop("What was signed isn't what was checked.")
        }
        let signature = Base58.encode(signed.subdata(in: 1..<65))
        try journal.append(.init(chain: "solana", id: signature, kind: .signed, note: "send \(shown) SOL to \(label); valid until block \(lastValid)"))
        switch try await broadcastSolana(signed, signature: signature, lastValid: lastValid) {
        case .confirmed:
            try journal.append(.init(chain: "solana", id: signature, kind: .confirmed))
            let closed = empties.isEmpty ? "" : " \(empties.count) empty token account\(empties.count == 1 ? "" : "s") closed, their rent went with it."
            return "Sent \(shown) SOL to \(label) on Solana.\(closed) https://solscan.io/tx/\(signature)"
        case .failed(let e):
            try journal.append(.init(chain: "solana", id: signature, kind: .failed, note: e))
            throw ExecutionError("The transfer failed: \(e). The SOL stayed.")
        case .expired:
            try journal.append(.init(chain: "solana", id: signature, kind: .expired))
            throw ExecutionError("The transfer didn't make it into a block; nothing moved. You can retry.")
        }
    }
}

/// Any token the wallet holds on one network to a saved address, as it is — no swap, no bridge: Solana's SPL and
/// Token-2022 tokens, an EVM network's ERC-20s (stock tokens, Morpho's and Aave's shares, memecoins) and its own coin.
/// The token is named by ticker, contract or mint and read from the chain. Only `recipient` may receive it: the guard
/// and the signer check.
extension Executor {
    /// nil amount = all of it; of a network's own coin, all but the fee.
    public func sendToken(_ query: String, amount: Decimal?, on network: String, to recipient: String, label: String) async throws -> String {
        if network == "solana" { return try await sendSolanaToken(query, amount: amount, to: recipient, label: label) }
        guard let net = EVMNetwork.named(network) else {
            throw ExecutionError("ihatecrypto sends tokens as they are on Solana and the EVM networks, not on \(network).")
        }
        guard recipient.lowercased() != wallet.evm.lowercased() else { throw ExecutionError("That is this wallet's own address.") }
        let (contract, symbol) = try await heldEVM(query, on: net)
        if contract == "native" { return try await sendNative(amount, on: net, to: recipient, label: label) }
        guard let decimals = ABI.uint(in: try await net.rpc.read(contract, Data([0x31, 0x3c, 0xe5, 0x67])), word: 0), decimals <= 36 else {
            throw ExecutionError("\(symbol) on \(net.name) doesn't answer decimals(): not a token ihatecrypto can send.")
        }
        let scale = pow(Decimal(10), Int(decimals))
        func f(_ units: Decimal) -> String { Fmt.num(units / scale, maxFraction: Int(decimals)) }
        let have = try await balance(contract, on: net.rpc)
        var units = amount.map { ($0 * scale).rounded(0, .down) } ?? have
        // All of the coin fees are paid in (Tempo's USDC): what this very transfer may cost stays for its fee, a tenth
        // over, not a whole cent.
        if amount == nil, contract.lowercased() == net.feeToken {
            let gas = try await net.rpc.estimateGas(from: wallet.evm, to: contract, value: 0, data: try ERC20.transfer(to: recipient, units: have))
            let fee = Decimal(Tempo.feeUnits((gas + gas / 4) * (try await net.rpc.fees()).maxFee))
            units = max(0, have - fee * Decimal(string: "1.1")!).rounded(0, .down)
        }
        guard units > 0 else { throw ExecutionError("No \(symbol) on \(net.name) to send.") }
        guard units <= have else { throw ExecutionError("\(f(have)) \(symbol) on \(net.name); can't send \(f(units)).") }
        let hash = try await sendEVM(net.rpc, to: contract, data: try ERC20.transfer(to: recipient, units: units),
                                     note: "Send \(f(units)) \(symbol) to \(label)", recipients: [recipient])
        return "Sent \(f(units)) \(symbol) to \(label) on \(net.name). \(net.explorer)/tx/\(hash)"
    }

    /// The token on `net` the user named: the network's coin, its dollar (USDC, USDG on Robinhood Chain), a contract,
    /// or a ticker among what the wallet holds there (Holdings.pick).
    private func heldEVM(_ query: String, on net: EVMNetwork) async throws -> (contract: String, symbol: String) {
        let q = query.uppercased()
        if q == net.gasCoin, net.feeToken == nil { return ("native", net.gasCoin) }
        if (q == "USDC" && net.key != "robinhood") || (q == "USDG" && net.key == "robinhood") { return (net.usdc, q) }
        if query.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil {
            return (query, ABI.string(try await net.rpc.read(query, Data([0x95, 0xd8, 0x9b, 0x41]))) ?? String(query.prefix(10)) + "…")
        }
        // A dollar coin by its ticker is the real one, read from the node: Blockscout's list of a wallet's tokens came back
        // empty on Polygon and stale on Base while the USDT sat there (2026-10-13).
        if let coin = Stablecoins.find(query, on: net.key), coin.address.hasPrefix("0x") { return (coin.address, coin.symbol) }
        let held = await Holdings.evm(net.key, address: wallet.evm, native: false, skip: [])
        if let held, case .one(let h) = Holdings.pick(query, in: held) { return (h.contract, h.symbol) }
        if let held, case .several(let list) = Holdings.pick(query, in: held) {
            throw ExecutionError("Several tokens on \(net.name) are called \(query): " + list.map { "\($0.contract) (\(Fmt.num($0.amount)))" }.joined(separator: ", ") + ". Name it by its contract.")
        }
        // Not in the list (it lags a token that just arrived) or no list: a token the lists know by that ticker, when the
        // node says this wallet holds it.
        if let t = try? await EVMTokens.find(query, chainId: net.rpc.chainId), (try? await balance(t.address, on: net.rpc)) ?? 0 > 0 {
            return (t.address, t.symbol)
        }
        throw ExecutionError(held == nil ? "Couldn't read the tokens on \(net.name) just now: name it by its contract, or try again."
                                         : "This wallet holds no \(query) on \(net.name).")
    }

    /// The network's own coin to a saved address: a plain transfer, the fee on top of it. nil = all of it but the fee:
    /// a plain transfer's gas is exact, so only the part of the fee the network doesn't take stays, a speck.
    private func sendNative(_ amount: Decimal?, on net: EVMNetwork, to recipient: String, label: String) async throws -> String {
        func f(_ w: UInt64) -> String { Fmt.num(Decimal(w) / pow(Decimal(10), 18), maxFraction: 18) }
        let rpc = net.rpc
        let have = try await rpc.balance(wallet.evm)
        let (tip, maxFee) = try await rpc.fees()
        let wei: UInt64, gasLimit: UInt64
        if let amount {
            let weiDecimal = (amount * pow(Decimal(10), 18)).rounded(0, .down)
            guard weiDecimal > 0, weiDecimal < Decimal(UInt64.max) else { throw ExecutionError("Not an amount of \(net.gasCoin) to send: \(Fmt.num(amount, maxFraction: 18)).") }
            wei = NSDecimalNumber(decimal: weiDecimal).uint64Value
            let estimate = try await rpc.estimateGas(from: wallet.evm, to: recipient, value: wei, data: Data())
            gasLimit = estimate + estimate / 4
        } else {
            let estimate = try await rpc.estimateGas(from: wallet.evm, to: recipient, value: 1, data: Data())
            gasLimit = estimate == 21_000 ? estimate : estimate + estimate / 4
            let fee = gasLimit * maxFee + (try await l1FeeBound(net))
            guard have > fee else { throw ExecutionError("\(f(have)) \(net.gasCoin) on \(net.name): no more than the fee (up to \(f(fee))), so nothing goes.") }
            wei = have - fee
        }
        guard wei + gasLimit * maxFee <= have else {
            throw ExecutionError("\(f(have)) \(net.gasCoin) on \(net.name): not enough for \(f(wei)) and the fee (up to \(f(gasLimit * maxFee))).")
        }
        let note = "Send \(f(wei)) \(net.gasCoin) to \(label)"
        let nonce = try await nextNonce(rpc)
        let raw = try await signer.signEVMTransfer(chainId: rpc.chainId, nonce: nonce, maxPriorityFee: tip, maxFee: maxFee,
                                                   gasLimit: gasLimit, to: recipient, value: wei, recipients: [recipient])
        let hash = "0x" + Keccak.hash256(raw).hex
        try journal.append(.init(chain: net.key, id: hash, kind: .signed, note: note))
        _ = try await rpc.send(raw)
        guard try await rpc.waitForReceipt(hash) else {
            try journal.append(.init(chain: net.key, id: hash, kind: .failed, note: note))
            throw ExecutionError("\(note) failed on \(net.name): \(net.explorer)/tx/\(hash)")
        }
        try journal.append(.init(chain: net.key, id: hash, kind: .confirmed))
        confirmed(nonce + 1, rpc)
        return "Sent \(f(wei)) \(net.gasCoin) to \(label) on \(net.name). \(net.explorer)/tx/\(hash)"
    }

    /// What an OP-stack network (Base, Optimism) charges on top of gas for posting a plain transfer to Ethereum: the
    /// oracle's upper bound for a 150-byte transaction. 0 elsewhere (Arbitrum's estimate holds its L1 part).
    private func l1FeeBound(_ net: EVMNetwork) async throws -> UInt64 {
        guard ["base", "optimism"].contains(net.key) else { return 0 }
        let call = Keccak.hash256(Data("getL1FeeUpperBound(uint256)".utf8)).prefix(4) + ABI.uint(150)
        return ABI.uint(in: try await net.rpc.read("0x420000000000000000000000000000000000000F", Data(call)), word: 0) ?? 0
    }

    /// An SPL or Token-2022 token, or SOL with or without a number, from the Solana wallet.
    private func sendSolanaToken(_ query: String, amount: Decimal?, to owner: String, label: String) async throws -> String {
        let q = query.uppercased()
        if q == "SOL" { return try await sendSOL(amount, to: owner, label: label) }
        guard owner != wallet.solana else { throw ExecutionError("That is this wallet's own address.") }
        let mint: String, symbol: String
        if q == "USDC" { (mint, symbol) = (Relay.Currency.solanaUSDC, "USDC") }
        // A dollar coin by its ticker is the real one (USDT is Tether's mint), and needs no read of the whole wallet: that read,
        // right after a chain's swap, was throttled by the public node (HTTP 429) and the send failed.
        else if let coin = Stablecoins.byNetwork["solana"]?.first(where: { $0.symbol.uppercased() == q }) { (mint, symbol) = (coin.address, coin.symbol) }
        else if query.wholeMatch(of: /[1-9A-HJ-NP-Za-km-z]{32,44}/) != nil { (mint, symbol) = (query, String(query.prefix(4)) + "…") }
        else {
            let held = try await IhatecryptoCore.Solana.balance(owner: wallet.solana, lendShares: true).tokens
            switch Holdings.pick(query, in: held) {
            case .one(let h): (mint, symbol) = (h.contract, h.symbol)
            case .none: throw ExecutionError("This wallet holds no \(query) on Solana.")
            case .several(let list):
                throw ExecutionError("Several tokens on Solana are called \(query): " + list.map { "\($0.contract) (\(Fmt.num($0.amount)))" }.joined(separator: ", ") + ". Name it by its mint.")
            }
        }
        // The mint's owner is its token program: SPL Token or Token-2022, and the accounts derive from it.
        let info = try await rpc.call("getAccountInfo", [mint, ["encoding": "base64", "commitment": "confirmed"]]) as? [String: Any]
        guard let program = (info?["value"] as? [String: Any])?["owner"] as? String, [SPL.tokenProgram, SPL.token2022Program].contains(program) else {
            throw ExecutionError("\(mint) isn't a token mint on Solana.")
        }
        let mine = try SPL.associatedAccount(owner: wallet.solana, mint: mint, program: program)
        let value = (try? await rpc.call("getTokenAccountBalance", [mine, ["commitment": "confirmed"]]) as? [String: Any])?["value"] as? [String: Any]
        guard let value, let have = (value["amount"] as? String).flatMap(UInt64.init), let decimals = (value["decimals"] as? NSNumber)?.uint8Value else {
            throw ExecutionError("This wallet holds no \(symbol) on Solana.")
        }
        let wanted = amount.map { ($0 * pow(Decimal(10), Int(decimals))).rounded(0, .down) }
        guard wanted.map({ $0 > 0 && $0 <= Decimal(UInt64.max) }) ?? (have > 0) else { throw ExecutionError("Nothing to send.") }
        let units = wanted.map { NSDecimalNumber(decimal: $0).uint64Value } ?? have
        // All of a token: its account closes and the rent (≈ 0.002 SOL) comes back. USDC's stays: the wallet lives on it.
        return try await sendSPL(mint: mint, units: units, decimals: decimals, symbol: symbol, program: program, to: owner, label: label,
                                 close: amount == nil && mint != Relay.Currency.solanaUSDC && program == SPL.tokenProgram)
    }

    /// The wallet's empty classic SPL token accounts, not frozen, up to 20 (what fits one transaction).
    // ponytail: classic SPL only; an empty Token-2022 account can hold withheld fees that make closing it fail.
    func emptyTokenAccounts() async throws -> [(address: String, lamports: UInt64)] {
        let r = try await rpc.call("getTokenAccountsByOwner", [wallet.solana, ["programId": SPL.tokenProgram], ["encoding": "jsonParsed", "commitment": "confirmed"]]) as? [String: Any]
        let empty: [(String, UInt64)] = (r?["value"] as? [[String: Any]] ?? []).compactMap { item in
            guard let address = item["pubkey"] as? String, let account = item["account"] as? [String: Any],
                  let info = ((account["data"] as? [String: Any])?["parsed"] as? [String: Any])?["info"] as? [String: Any],
                  (info["tokenAmount"] as? [String: Any])?["amount"] as? String == "0", info["state"] as? String != "frozen",
                  let lamports = (account["lamports"] as? NSNumber)?.uint64Value else { return nil }
            return (address, lamports)
        }
        return Array(empty.prefix(20))
    }
}
