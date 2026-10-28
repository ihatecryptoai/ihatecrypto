import DeFiJupiterLend
import DeFiKamino
import DeFiYields
import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import JupiterSwap
import IhatecryptoCore
import OneClickKit
import RelayBridge
import SolanaChain
import TxGuard
import WalletCore

/// Moving money between the user's own places: Hyperliquid back to Solana, Solana USDC into a vault and out.
extension Executor {
    /// Relay from the Hyperliquid perp balance to our Solana address. `usdc: nil` = everything withdrawable.
    public func withdrawToSolana(usdc requested: Decimal?) async throws -> String {
        var free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        // Margin left on a HIP-3 exchange (xyz, after an oil or stock trade) comes back to the main balance first;
        // what an open position there uses stays (only the withdrawable part moves).
        if requested.map({ $0 > free }) ?? true {
            for dex in Hyperliquid.hip3Dexes {
                let idle = (try await Hyperliquid.dexBalance(user: wallet.evm, dex: dex).withdrawable).rounded(2, .down)
                guard idle > 0 else { continue }
                try await moveMargin(from: dex, to: "", usdc: idle)
            }
            free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        }
        let usdc = (requested ?? free).rounded(2, .down)
        guard usdc > 0, usdc <= free else { throw ExecutionError("\(Fmt.usd(free)) free on Hyperliquid; can't withdraw \(Fmt.num(usdc)).") }
        let units = NSDecimalNumber(decimal: usdc * 100_000_000).uint64Value // Hyperliquid perp USDC: 8 decimals in Relay
        let quote = try await relayQuote(user: wallet.evm, recipient: wallet.solana, originChain: Relay.Chain.hyperliquid,
                                          destinationChain: Relay.Chain.solana, originCurrency: Relay.Currency.hyperliquidUSDC,
                                          destinationCurrency: Relay.Currency.solanaUSDC, amount: units)
        let arriving = Decimal(string: quote.amountOutFormatted) ?? 0
        let before = try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)
        let started = Date()
        try await sendFromHyperliquid(quote, usdc: usdc, units: units, recipient: wallet.solana, lost: usdc - arriving,
                                      note: "Hyperliquid→Solana USDC \(usdc)",
                                      line: "Relay withdrawal: \(Fmt.num(usdc)) USDC from Hyperliquid → \(Fmt.num(arriving)) USDC on Solana, fee \(Decimal(string: quote.feesUSD).map { Fmt.usd($0) } ?? quote.feesUSD).")
        let requestId = quote.requestId
        let landing: @Sendable () async throws -> String = { [self] in try await withdrawalLanding(requestId, before: before, started: started) }
        // Inside a chain the next step is signed while this lands on Solana (Landings).
        if let landings = Self.landings {
            landings.add("solana") { try await landing() }
            return "Sent \(Fmt.num(usdc)) USDC from Hyperliquid: ≈ \(Fmt.num(arriving)) lands on Solana in seconds, and what needs it waits for it."
        }
        return try await landing()
    }

    /// Relay's Hyperliquid steps for `quote`, checked and sent: the depository binding signed and posted, then the sendAsset
    /// of exactly `usdc` to Relay's depository. The quote must be from this wallet to `recipient`, losing at most 3% (≤ $1).
    private func sendFromHyperliquid(_ quote: Relay.Quote, usdc: Decimal, units: UInt64, recipient: String, lost: Decimal, note: String,
                                     line: String) async throws {
        guard let hl = quote.hyperliquid else { throw ExecutionError("The Relay quote has no Hyperliquid steps.") }
        let published = try await Relay.depository(chainId: Relay.Chain.hyperliquid)?.lowercased()
        // An EVM address in any case; a Solana one exactly (base58 letters differ by case).
        let evm = recipient.hasPrefix("0x"), to = evm ? quote.recipient.lowercased() : quote.recipient
        var verdict = BindingGuard.check(
            ["sender": quote.sender, "recipient": to, "amountIn": quote.amountIn, "wallet": hl.wallet, "depositor": hl.depositor,
             "destination": hl.destination, "published depository": published ?? "none", "token": hl.token],
            expected: ["sender": wallet.evm, "recipient": evm ? recipient.lowercased() : recipient, "amountIn": "\(units)", "wallet": wallet.evm, "depositor": wallet.evm,
                       "destination": Relay.hyperliquidDepository, "published depository": Relay.hyperliquidDepository,
                       "token": "USDC:0x6d1e7cde53ba9467b783cb7c530ce054"],
            subject: "Relay quote")
        if Decimal(string: hl.amount) != usdc { verdict.problems.append("sendAsset amount \(hl.amount) instead of \(usdc).") }
        if lost > min(usdc * Decimal(string: "0.03")!, 1) { verdict.problems.append("Relay charges \(Fmt.num(lost)) USDC: more than allowed.") }
        guard verdict.ok else { throw GuardStop("The guard stopped the withdrawal: " + verdict.problems.joined(separator: " ")) }
        report(line)

        guard let id = Data(hex: hl.mappingId) else { throw ExecutionError("Invalid Relay mapping id.") }
        let mapping = try await signer.signRelayMapping(wallet: hl.wallet, depositor: hl.depositor, id: id, nonce: hl.mappingNonce)
        try await Relay.authorize(body: hl.authorizeBody, signature: "0x" + mapping.r.hex + mapping.s.hex + String(format: "%02x", mapping.v))

        let action = HLSigner.sendAssetAction(destination: hl.destination, sourceDex: hl.sourceDex, destinationDex: hl.destinationDex,
                                              token: hl.token, amount: hl.amount, nonce: hl.nonce, mainnet: true)
        let signature = try await signer.signHyperliquidUser(action, recipients: [Relay.hyperliquidDepository])
        try journal.append(.init(chain: "relay", id: quote.requestId, kind: .signed, note: note))
        let reply = HLReply.parse(try await HLExchange.send(action: action, nonce: hl.nonce, signature: signature, mainnet: true))
        if case .error(let e) = reply {
            try journal.append(.init(chain: "relay", id: quote.requestId, kind: .failed, note: e))
            throw ExecutionError("Hyperliquid rejected the transfer: \(e).")
        }
    }

    /// `usd` of `net`'s own coin for gas, out of the Hyperliquid balance over Relay: no gas anywhere is needed, so it brings
    /// gas where the dollars can't move by signature (BNB Chain, HyperEVM). Waits until the coin is there.
    func hyperliquidGas(_ net: EVMNetwork, usd: Decimal) async throws -> String {
        let usdc = usd.rounded(2, .up), units = NSDecimalNumber(decimal: usdc * 100_000_000).uint64Value
        let free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        guard usdc <= free else { throw ExecutionError("\(Fmt.usd(free)) free on Hyperliquid; gas for \(net.name) needs \(Fmt.usd(usdc)).") }
        let quote = try await relayQuote(user: wallet.evm, recipient: wallet.evm, originChain: Relay.Chain.hyperliquid, destinationChain: net.relayChain,
                                          originCurrency: Relay.Currency.hyperliquidUSDC, destinationCurrency: "0x0000000000000000000000000000000000000000",
                                          amount: units)
        let worth = Decimal(string: quote.amountOutUSD) ?? 0
        guard worth > 0 else { throw ExecutionError("Relay doesn't quote \(net.gasCoin) on \(net.name) from Hyperliquid now. Nothing was signed.") }
        let before = try await net.rpc.balance(wallet.evm)
        try await sendFromHyperliquid(quote, usdc: usdc, units: units, recipient: wallet.evm, lost: usdc - worth, note: "Hyperliquid→\(net.name) \(net.gasCoin) \(usdc)",
                                      line: "Gas for \(net.name) from Hyperliquid: \(Fmt.num(usdc)) USDC → ≈ \(quote.amountOutFormatted) \(net.gasCoin) over Relay.")
        let started = Date()
        while Date().timeIntervalSince(started) < 180 {
            let status = try await Relay.status(requestId: quote.requestId).status
            if ["refund", "failure"].contains(status) {
                try journal.append(.init(chain: "relay", id: quote.requestId, kind: .failed, note: status))
                throw ExecutionError("Relay returned \(status): the USDC should come back to Hyperliquid.")
            }
            if status == "success", try await net.rpc.balance(wallet.evm) > before {
                try journal.append(.init(chain: "relay", id: quote.requestId, kind: .settled, note: status))
                return "Gas from Hyperliquid on \(net.name) in \(Int(Date().timeIntervalSince(started))) s."
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw ExecutionError("Relay didn't bring the gas within 3 minutes. Order \(quote.requestId) is in the journal.")
    }

    /// A Relay withdrawal from Hyperliquid, sent: Relay's word until it lands on Solana.
    private func withdrawalLanding(_ requestId: String, before: UInt64, started: Date) async throws -> String {
        while Date().timeIntervalSince(started) < 180 {
            let status = try await Relay.status(requestId: requestId).status
            if status == "success" {
                try journal.append(.init(chain: "relay", id: requestId, kind: .settled, note: status))
                let after = try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)
                let line = "Withdrawal done in \(Int(Date().timeIntervalSince(started))) s: Solana USDC +\(Fmt.num(Decimal(Int64(after) - Int64(before)) / 1_000_000, maxFraction: 6))."
                report(line)
                return line
            }
            if ["refund", "failure"].contains(status) {
                try journal.append(.init(chain: "relay", id: requestId, kind: .failed, note: status))
                throw ExecutionError("Relay returned \(status): the money should come back to Hyperliquid.")
            }
            try await Task.sleep(for: .seconds(1))
        }
        throw ExecutionError("Relay didn't finish the withdrawal within 3 minutes. Order \(requestId) is in the journal.")
    }

    /// Jupiter Lend Earn, USDC vault. `usdc: nil` on withdraw = the whole position.
    public func lend(usdc requested: Decimal?, deposit: Bool) async throws -> String {
        try await lend("USDC", amount: requested, deposit: deposit)
    }

    /// Jupiter Lend Earn for any of its assets: USDC, SOL (the jlWSOL vault), USDT, JupUSD… `amount` in the asset;
    /// withdraw with nil takes everything. After a full withdraw the empty share account is closed and its rent returns.
    public func lend(_ symbol: String, amount requested: Decimal?, deposit: Bool) async throws -> String {
        let wanted = symbol.uppercased() == "SOL" ? "WSOL" : symbol.uppercased()
        guard let vault = try await JupiterLend.vaults().first(where: { $0.assetSymbol.uppercased() == wanted }) else {
            throw ExecutionError("Jupiter Lend has no \(symbol) vault.")
        }
        let native = vault.assetMint == "So11111111111111111111111111111111111111112"
        let name = native ? "SOL" : vault.assetSymbol
        let scale = Decimal(sign: .plus, exponent: vault.decimals, significand: 1)
        let myAsset = try SPL.associatedAccount(owner: wallet.solana, mint: vault.assetMint)
        let myShares = try SPL.associatedAccount(owner: wallet.solana, mint: vault.address)
        let owners = [myAsset: wallet.solana, myShares: wallet.solana]
        let position = try await JupiterLend.positions(user: wallet.solana).first { $0.vault == vault.symbol }
        let inVault = position?.assets ?? 0
        var units: UInt64
        if let requested { units = NSDecimalNumber(decimal: (requested * scale).rounded(0, .down)).uint64Value }
        else if deposit { throw ExecutionError("How much should go into the vault?") }
        else { units = inVault }
        if deposit {
            let have = native ? try await rpc.balance(wallet.solana) : try await rpc.tokenBalance(owner: wallet.solana, mint: vault.assetMint)
            let spendable = native ? (have > 12_000_000 ? have - 12_000_000 : 0) : have   // SOL: keep fees and account rent
            guard units > 0, units <= spendable else {
                throw ExecutionError("You have \(Fmt.num(Decimal(spendable) / scale, maxFraction: 6)) \(name)\(native ? " (0.012 SOL is kept for fees and the account)" : ""); can't deposit \(Fmt.num(Decimal(units) / scale, maxFraction: 6)).")
            }
            // The API's deposit expects our share account to exist; Jupiter's own site creates it first.
            if (try? await rpc.call("getAccountInfo", [myShares, ["encoding": "base64"]]) as? [String: Any])?["value"] is NSNull {
                let create = SolanaInstruction(program: SPL.associatedTokenProgram, accounts: [
                    .init(pubkey: wallet.solana, isSigner: true, isWritable: true), .init(pubkey: myShares, isSigner: false, isWritable: true),
                    .init(pubkey: wallet.solana, isSigner: false, isWritable: false), .init(pubkey: vault.address, isSigner: false, isWritable: false),
                    .init(pubkey: SolanaGuard.system, isSigner: false, isWritable: false), .init(pubkey: SPL.tokenProgram, isSigner: false, isWritable: false),
                ], data: Data([1])) // CreateIdempotent
                let (hash, lastValid) = try await rpc.latestBlockhash()
                let wire = SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([create], payer: wallet.solana, blockhash: hash))
                try await signAndSend(wire, lastValid: lastValid, owners: owners, rules: [], note: "\(vault.symbol) share account")
            }
            if native {
                // Jupiter Lend takes wrapped SOL from our wSOL account; it does not wrap by itself.
                let wrap = [
                    SolanaInstruction(program: SPL.associatedTokenProgram, accounts: [
                        .init(pubkey: wallet.solana, isSigner: true, isWritable: true), .init(pubkey: myAsset, isSigner: false, isWritable: true),
                        .init(pubkey: wallet.solana, isSigner: false, isWritable: false), .init(pubkey: vault.assetMint, isSigner: false, isWritable: false),
                        .init(pubkey: SolanaGuard.system, isSigner: false, isWritable: false), .init(pubkey: SPL.tokenProgram, isSigner: false, isWritable: false),
                    ], data: Data([1])),
                    SolanaInstruction(program: SolanaGuard.system, accounts: [
                        .init(pubkey: wallet.solana, isSigner: true, isWritable: true), .init(pubkey: myAsset, isSigner: false, isWritable: true),
                    ], data: Data([2, 0, 0, 0]) + withUnsafeBytes(of: units.littleEndian) { Data($0) }),
                    SolanaInstruction(program: SPL.tokenProgram, accounts: [.init(pubkey: myAsset, isSigner: false, isWritable: true)], data: Data([17])), // SyncNative
                ]
                let (hash, lastValid) = try await rpc.latestBlockhash()
                try await signAndSend(SolanaTransaction.unsigned(try SolanaTransaction.compileMessage(wrap, payer: wallet.solana, blockhash: hash)),
                                      lastValid: lastValid, owners: owners, rules: [(myAsset, "wSOL", true, Int64(units))], note: "wrap SOL",
                                      lamportsAtLeast: -Int64(units) - 8_000_000)
            }
        } else {
            units = min(units, inVault)
            guard units > 0 else {
                // Nothing in the vault; an empty share account left from an earlier attempt still holds rent.
                let line = "The \(vault.symbol) vault is empty." + (try await closeIfEmpty(myShares, mint: vault.address, label: "\(vault.symbol) share account") ?? "")
                report(line)
                return line
            }
        }
        // Everything out goes by shares: withdrawing the reported amount fails by a rounding unit.
        let all = !deposit && units >= inVault
        let wire = deposit
            ? try await JupiterLend.depositTransaction(asset: vault.assetMint, user: wallet.solana, amount: units)
            : all ? try await JupiterLend.redeemTransaction(asset: vault.assetMint, user: wallet.solana, shares: position?.shares ?? 0)
            : try await JupiterLend.withdrawTransaction(asset: vault.assetMint, user: wallet.solana, amount: units)
        let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
        // SOL goes through our wSOL account (wrapped before the deposit, unwrapped after the withdraw), so the rule
        // is the same token rule for every asset.
        let before = Int64(try await rpc.tokenBalance(owner: wallet.solana, mint: vault.assetMint))
        var rules: [(String, String, Bool, Int64)] = deposit ? [(myShares, "\(vault.symbol) shares", true, 1)] : []
        rules.append((myAsset, native ? "wSOL" : name, true, deposit ? -Int64(units) : Int64(units) - (all ? 2 : 1)))
        try await signAndSend(wire, lastValid: lastValid, owners: owners, rules: rules, note: "Jupiter Lend \(deposit ? "deposit" : "withdraw") \(name)")
        let after = Int64(try await rpc.tokenBalance(owner: wallet.solana, mint: vault.assetMint))
        var line = deposit
            ? "Deposited \(Fmt.num(Decimal(units) / scale, maxFraction: 9)) \(name) into the \(vault.symbol) vault (\(Fmt.num(vault.apy))% APY)."
            : "Withdrawn from the \(vault.symbol) vault: \(name) +\(Fmt.num(Decimal(after - before) / scale, maxFraction: 9))\(native ? " (net of fees)" : "")."
        if !deposit {
            // Emptied the vault: the share account's rent comes back. For SOL, closing the wSOL account unwraps it.
            line += try await closeIfEmpty(myShares, mint: vault.address, label: "\(vault.symbol) share account") ?? ""
            if native { line += try await closeWrappedSOL(myAsset) }
        }
        report(line)
        return line
    }

    // MARK: Jupiter DCA

    /// Jupiter's on-chain DCA: `usdc` into `asset` in `orders` swaps `every` seconds apart, the first right away. One
    /// signature moves the whole amount into an order account of the wallet's own; Jupiter's keepers swap, and the coin
    /// comes to the wallet after each. `dryRun`: built, guarded and simulated, not signed.
    public func jupiterDCA(_ asset: SpotAsset, usdc: Decimal, orders: Int, every: Int, dryRun: Bool = false) async throws -> String {
        if let problem = WalletCommand.jupiterDCAProblem(usdc: usdc, orders: orders) { throw ExecutionError(problem) }
        guard asset.resolved, asset.mint != SpotAsset.usdc.mint else { throw ExecutionError("Which coin should the DCA buy?") }
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        let have = try await rpc.tokenBalance(owner: wallet.solana, mint: SpotAsset.usdc.mint)
        // A dry run still shows what the guard says of Jupiter's transaction when the wallet is short.
        guard units <= have || dryRun else {
            throw ExecutionError("You have \(Fmt.num(Decimal(have) / 1_000_000)) USDC on Solana; the DCA puts in \(Fmt.num(usdc)) at once.")
        }
        let wire = try await JupiterRecurring.createOrder(user: wallet.solana, input: SpotAsset.usdc.mint, output: asset.mint,
                                                          amount: units, orders: orders, every: every).wire
        let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
        let myUSDC = try SPL.associatedAccount(owner: wallet.solana, mint: SpotAsset.usdc.mint)
        let myCoin = try SPL.associatedAccount(owner: wallet.solana, mint: asset.mint)
        // Exactly the amount leaves the USDC account; SOL pays the fee and ≈ 0.005 SOL of rent the order returns at its end.
        let changes = try await signAndSend(wire, lastValid: lastValid, owners: [myUSDC: wallet.solana, myCoin: wallet.solana],
                                            rules: [(myUSDC, "USDC", true, -Int64(units))], note: "Jupiter DCA \(Fmt.num(usdc)) USDC into \(asset.symbol)",
                                            lamportsAtLeast: -12_000_000, dryRun: dryRun)
        if dryRun { return "Dry run, nothing signed: " + changes.joined(separator: "; ") }
        let line = "Jupiter DCA set: \(Fmt.num(usdc)) USDC into \(asset.symbol), \(orders) swaps of \(Fmt.num(usdc / Decimal(orders))) USDC every "
            + "\(WalletCommand.everyText(every)); the first runs now, and \(asset.symbol) comes to the wallet after each."
        report(line)
        return line
    }

    /// Closes the running Jupiter DCA orders (`asset` nil = all of them): what they haven't spent comes back, with their rent.
    public func cancelJupiterDCA(_ asset: SpotAsset?, dryRun: Bool = false) async throws -> String {
        let open = try await jupiterDCAOrders().filter { asset == nil || $0.outputMint == asset?.mint }
        guard !open.isEmpty else { return "No Jupiter DCA\(asset.map { " into \($0.symbol)" } ?? "") is running." }
        var notes: [String] = []
        for order in open {
            let wire = try await JupiterRecurring.cancelOrder(user: wallet.solana, order: order.key).wire
            let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
            let myIn = try SPL.associatedAccount(owner: wallet.solana, mint: order.inputMint)
            let myOut = try SPL.associatedAccount(owner: wallet.solana, mint: order.outputMint)
            // Money only comes back: the input account can't go down, SOL only by the fee.
            let changes = try await signAndSend(wire, lastValid: lastValid, owners: [myIn: wallet.solana, myOut: wallet.solana],
                                                rules: [(myIn, "unspent input", true, 0)], note: "cancel Jupiter DCA \(order.key.prefix(4))…",
                                                lamportsAtLeast: -1_000_000, dryRun: dryRun)
            if dryRun { notes.append(changes.joined(separator: "; ")) }
        }
        if dryRun { return "Dry run, nothing signed: " + notes.joined(separator: " | ") }
        let back = open.reduce(Decimal(0)) { $0 + max($1.deposited - $1.used, 0) }
        let line = "Cancelled \(open.count == 1 ? "the Jupiter DCA" : "\(open.count) Jupiter DCAs"): about \(Fmt.num(back)) of what they hadn't spent and their rent are back in the wallet."
        report(line)
        return line
    }

    /// The wallet's running Jupiter DCA orders, newest first.
    public func jupiterDCAOrders() async throws -> [JupiterRecurring.Order] {
        try await JupiterRecurring.orders(user: wallet.solana)
    }

    /// Closes our token account when it holds nothing; returns the line saying how much rent came back.
    private func closeIfEmpty(_ account: String, mint: String, label: String) async throws -> String? {
        guard (try? await rpc.call("getAccountInfo", [account, ["encoding": "base64"]]) as? [String: Any])?["value"] is [String: Any],
              try await rpc.tokenBalance(owner: wallet.solana, mint: mint) == 0 else { return nil }
        return try await close(account, label: label, least: 1_000_000)
    }

    /// Closing the wSOL account turns the wrapped SOL and its rent back into ordinary SOL.
    private func closeWrappedSOL(_ account: String) async throws -> String {
        guard (try? await rpc.call("getAccountInfo", [account, ["encoding": "base64"]]) as? [String: Any])?["value"] is [String: Any] else { return "" }
        return try await close(account, label: "wSOL account (SOL unwrapped)", least: 0)
    }

    private func close(_ account: String, label: String, least: Int64) async throws -> String {
        let instruction = SolanaInstruction(program: SPL.tokenProgram, accounts: [
            .init(pubkey: account, isSigner: false, isWritable: true), .init(pubkey: wallet.solana, isSigner: false, isWritable: true),
            .init(pubkey: wallet.solana, isSigner: true, isWritable: false),
        ], data: Data([9])) // CloseAccount
        let (hash, lastValid) = try await rpc.latestBlockhash()
        let before = try await rpc.balance(wallet.solana)
        try await signAndSend(SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([instruction], payer: wallet.solana, blockhash: hash)),
                              lastValid: lastValid, owners: [account: wallet.solana], rules: [], note: "close \(label)", lamportsAtLeast: least)
        return " Closed \(label): +\(Fmt.num(Decimal(Int64(try await rpc.balance(wallet.solana)) - Int64(before)) / 1_000_000_000, maxFraction: 6)) SOL."
    }

    /// Spot on Solana through Jupiter: `buy` spends USDC on the coin, otherwise the coin is sold for USDC.
    /// The simulated balances must show at least Jupiter's minimum output arriving in our own account.
    /// Buy (from USDC) or sell (for USDC); kept for the chat commands.
    public func swap(_ asset: SpotAsset, buy: Bool, amount requested: Decimal?, equity: Decimal? = nil, shown: UInt64? = nil) async throws -> String {
        try await convert(from: buy ? .usdc : asset, to: buy ? asset : .usdc, amount: requested, equity: equity, shown: shown)
    }

    /// Any verified token into any other through Jupiter: buying and selling, and staking SOL into JitoSOL and back.
    /// The simulated balances must show at least Jupiter's minimum output arriving in our own account. `amount` in the
    /// input token; nil = all of it (SOL keeps 0.01 for fees). Spending USDC on one coin obeys the concentration rule.
    /// `shown`: the output the card showed, in the token's units — the swap doesn't go for more than 1% less.
    public func convert(from: SpotAsset, to: SpotAsset, amount requested: Decimal?, equity: Decimal? = nil, dryRun: Bool = false,
                        shown: UInt64? = nil) async throws -> String {
        guard from != to else { throw ExecutionError("That's the same token.") }
        let nativeIn = from.symbol == "SOL", nativeOut = to.symbol == "SOL"
        let scale = Decimal(sign: .plus, exponent: from.decimals, significand: 1)
        let outScale = Decimal(sign: .plus, exponent: to.decimals, significand: 1)
        // SOL bought where there's none for fees is gas, not a bet: the concentration rule doesn't count it.
        let lamports = try await rpc.balance(wallet.solana)
        let buyingGas = from == .usdc && to.symbol == "SOL" && lamports < TopUpOrder.solanaFeeFloor
        if from == .usdc, !buyingGas, let equity, equity > 0 {
            let share = (requested ?? 0) / equity * 100
            guard share <= 50 else {
                throw ExecutionError("That's \(Fmt.pct(share.rounded(0, .plain))) of your capital in one coin. The rule: no more than half at once — buy in parts, e.g. \(Fmt.usd((equity / 10).rounded(2, .down))) a week.")
            }
            if share > 25 { report("That's \(Fmt.pct(share.rounded(0, .plain))) of your capital in \(to.symbol): big buys are usually split over time (DCA).") }
        }
        let have: UInt64 = nativeIn ? try await rpc.balance(wallet.solana) : try await rpc.tokenBalance(owner: wallet.solana, mint: from.mint)
        let spendable = nativeIn ? (have > 10_000_000 ? have - 10_000_000 : 0) : have
        let units = requested.map { NSDecimalNumber(decimal: ($0 * scale).rounded(0, .down)).uint64Value } ?? spendable
        guard units > 0, units <= spendable else {
            throw ExecutionError("You have \(Fmt.num(Decimal(spendable) / scale, maxFraction: 6)) \(from.symbol)\(nativeIn ? " (0.01 SOL is kept for fees)" : ""); need \(Fmt.num(Decimal(units) / scale, maxFraction: 6)).")
        }
        let inAccount = try SPL.associatedAccount(owner: wallet.solana, mint: from.mint, program: from.token2022 ? SPL.token2022Program : SPL.tokenProgram)
        let outAccount = try SPL.associatedAccount(owner: wallet.solana, mint: to.mint, program: to.token2022 ? SPL.token2022Program : SPL.tokenProgram)
        let owners = [inAccount: wallet.solana, outAccount: wallet.solana]   // wSOL accounts too, for the native side
        let outBefore = nativeOut ? try await rpc.balance(wallet.solana) : try await rpc.tokenBalance(owner: wallet.solana, mint: to.mint)
        var changes: [String] = []
        // The price moved more than 1% (and Jupiter's own fee, when its wallet pays) under what the card showed: not signed.
        func holdsShown(_ out: UInt64, feeBps: Decimal = 0) throws {
            guard let shown, Decimal(out) < Decimal(shown) * (1 - (100 + feeBps) / 10_000) else { return }
            throw ExecutionError("Jupiter now gives \(Fmt.num(Decimal(out) / outScale, maxFraction: 8)) \(to.symbol), more than 1% under the "
                                 + "\(Fmt.num(Decimal(shown) / outScale, maxFraction: 8)) the card showed. Nothing was signed; Refresh shows the new price.")
        }
        // The swap moves the price past impactCap: asked, not signed. What goes in, in USD: the USDC itself, else what comes
        // out (Jupiter's own price for it) before the impact.
        func holdsImpact(_ impact: Decimal, usd: Decimal, out: UInt64) throws {
            let worth = to == .usdc ? Decimal(out) / 1_000_000 : usd
            let paid = from == .usdc ? Decimal(units) / 1_000_000 : impact < 1 ? worth / (1 - impact) : 0
            try Self.checkImpact(impact, paid: paid, amount: Decimal(units) / scale, symbol: from.symbol, venue: "Jupiter")
        }
        // Jupiter's /order, where someone else pays the fee and signs first, our wallet second: JupiterZ's market makers when
        // their price beats the pools' (/quote) — big swaps mostly (2M WIF: $362k against $321k, 2026-10-28; on small ones
        // its fee makes it the worse) — and, under the fee floor (USDC came without SOL), Jupiter's own gasless swap, which
        // Jupiter grants a wallet that can't pay (a few thousandths of SOL still gets the ordinary swap below).
        let lowSOL = !nativeIn && lamports < TopUpOrder.solanaFeeFloor
        // The Dev copy's JupiterZ run: Jupiter's other routers left out, so its market makers answer even a test size.
        let order = try? await Jupiter.order(input: from.mint, output: to.mint, amount: units, taker: wallet.solana,
                                             excludeRouters: Self.jupiterZFirst ? ["metis", "iris", "dflow", "okx"] : [])
        let payer = order.flatMap { $0.gasless ? try? SolanaMessage.parse(transaction: $0.wire).staticKeys.first : nil }.flatMap { $0 == wallet.solana ? nil : $0 }
        var sponsored: (order: Jupiter.Order, sponsor: String)? = nil
        if let order, let payer {
            if lowSOL { sponsored = (order, payer) }
            else if order.router == "jupiterz", Self.fillSwaps(order.wire, from: from.mint, to: to.mint),
                    let pools = try? await Jupiter.quote(input: from.mint, output: to.mint, amount: units), order.outAmount > pools.outAmount || Self.jupiterZFirst {
                sponsored = (order, payer)
            }
        }
        // Not even the rent of an account the swap opens (the temporary wSOL one, or the coin's own: ≈ 0.00204 SOL) and Jupiter
        // won't pay: said before a simulation fails on it.
        if sponsored == nil, !nativeIn, nativeOut || outBefore == 0, lamports < 2_100_000 {
            throw ExecutionError("There's \(Fmt.num(Decimal(lamports) / 1_000_000_000, maxFraction: 6)) SOL for the network fee, too little for this swap, and Jupiter pays the fee only for a bigger one ($5 and up). Swap more, or add ≈ 0.01 SOL first.")
        }
        if let (order, sponsor) = sponsored {
            let maker = order.router == "jupiterz"
            try holdsShown(order.outAmount, feeBps: Decimal(order.feeBps))
            try holdsImpact(order.impact, usd: order.usd, out: order.outAmount)
            report("\(from.symbol) → \(to.symbol) via \(maker ? "JupiterZ, a market maker filling it and paying the fee" : "Jupiter, its wallet paying the fee"): \(Fmt.num(Decimal(units) / scale, maxFraction: 8)) → ≈ \(Fmt.num(Decimal(order.outAmount) / outScale, maxFraction: 8)), at least \(Fmt.num(Decimal(order.minOut) / outScale, maxFraction: 8)) (\(Fmt.num(Decimal(order.feeBps) / 100, maxFraction: 2))% fee). Route: \(order.route.isEmpty ? order.router : order.route).")
            var rules: [(String, String, Bool, Int64)] = nativeIn ? [] : [(inAccount, from.symbol, true, -Int64(units))]
            if !nativeOut { rules.append((outAccount, "\(to.symbol) (quote minimum)", true, Int64(order.minOut))) }
            // Not a lamport of ours pays: SOL in leaves exactly, SOL bought arrives whole, at least the minimum. A maker leaves
            // the rent of the coin's new account to the wallet (≈ 0.00156 SOL for an Ondo stock's Token-2022 one, 2026-10-29).
            do {
                changes = try await signAndSend(order.wire, lastValid: order.lastValidBlockHeight, owners: owners, rules: rules,
                                                note: "Jupiter \(from.symbol)→\(to.symbol), fee paid by \(maker ? "JupiterZ's market maker" : "Jupiter")",
                                                lamportsAtLeast: nativeIn ? -Int64(units) : nativeOut ? Int64(order.minOut) : maker && outBefore == 0 ? -2_100_000 : 0,
                                                dryRun: dryRun, sponsor: (sponsor, order.requestId))
            } catch where maker && !lowSOL && ((error as? GuardStop)?.message.hasPrefix("The guard stopped") == true
                                                 || (error as? ExecutionError)?.message.hasPrefix("The simulation failed") == true) {
                // Refused by the app's own checks before the signer was asked (the guard, or a simulation off the quote): the
                // pools' route below instead. A «no» at the signer's prompt stops here, never asks again for another route.
                report("JupiterZ's fill didn't pass the checks, nothing was sent: \(error.localizedDescription) Taking the pools' route.")
                sponsored = nil
            }
        }
        if sponsored == nil {
            // Jupiter's route can fail before anything is signed: its transaction is over Solana's 1232 bytes (HumidiFi → Hadron
            // into a Token-2022 coin), or the simulation ends under its own minimum — 6001, SlippageToleranceExceeded, when a
            // pool quoted more than it gives (Flux → Hadron, TesseraV → Hadron). Up to three routes, each without the failed ones' pools.
            var excluded: [String] = [], firstFailure: Error?
            for attempt in 1...3 {
                let quote: Jupiter.Quote
                do { quote = try await Jupiter.quote(input: from.mint, output: to.mint, amount: units, excludeDexes: excluded) }
                catch { throw firstFailure ?? error }
                try holdsShown(quote.outAmount)
                try holdsImpact(quote.impact, usd: quote.usd, out: quote.outAmount)
                report("\(from.symbol) → \(to.symbol) via Jupiter: \(Fmt.num(Decimal(units) / scale, maxFraction: 8)) → ≈ \(Fmt.num(Decimal(quote.outAmount) / outScale, maxFraction: 8)), at least \(Fmt.num(Decimal(quote.minOut) / outScale, maxFraction: 8)). Route: \(quote.route).")
                let tx = try await Jupiter.swapTransaction(quote, user: wallet.solana)
                var rules: [(String, String, Bool, Int64)] = []
                if !nativeIn { rules.append((inAccount, from.symbol, true, -Int64(units))) }
                if !nativeOut { rules.append((outAccount, "\(to.symbol) (quote minimum)", true, Int64(quote.minOut))) }
                let fee: Int64 = 5_000_000
                // Up to ~0.0021 SOL more when the output token's account is created (rent, returned when it is closed).
                let lamportsAtLeast: Int64 = nativeIn ? -Int64(units) - fee : nativeOut ? Int64(quote.minOut) - fee : -fee
                let failure: Error, why: String
                if tx.wire.count > 1232 {
                    why = "is \(tx.wire.count) bytes, over Solana's limit of 1232"
                    failure = ExecutionError("Jupiter's route \(quote.route) \(why), so nothing was signed.")
                } else {
                    do {
                        changes = try await signAndSend(tx.wire, lastValid: tx.lastValidBlockHeight, owners: owners, rules: rules, note: "Jupiter \(from.symbol)→\(to.symbol)",
                                                        lamportsAtLeast: lamportsAtLeast, dryRun: dryRun)
                        break
                    } catch let e as ExecutionError where e.message.hasPrefix("The simulation failed") && e.message.contains(#"{"Custom":6001}"#) {
                        why = "would give less than its minimum (Jupiter's 6001)"
                        failure = e
                    }
                }
                guard attempt < 3 else { throw failure }
                firstFailure = firstFailure ?? failure
                excluded += quote.dexes.filter { !excluded.contains($0) }
                report("Route \(quote.route) \(why); nothing was signed. Asking Jupiter for a route without \(excluded.joined(separator: ", ")).")
            }
        }
        if dryRun { return "Simulation (nothing signed): " + changes.joined(separator: "; ") }
        let outAfter = nativeOut ? try await rpc.balance(wallet.solana) : try await rpc.tokenBalance(owner: wallet.solana, mint: to.mint)
        var line = "Swapped \(Fmt.num(Decimal(units) / scale, maxFraction: 8)) \(from.symbol) → \(to.symbol) +\(Fmt.num(Decimal(Int64(outAfter) - Int64(outBefore)) / outScale, maxFraction: 8))\(nativeOut ? " (net of fees)" : "")."
        swapped?(from, to, Decimal(units) / scale, Decimal(Int64(outAfter) - Int64(outBefore)) / outScale)
        // Out of a token entirely: its empty account still holds ~0.002 SOL of rent; closing returns it. A Token-2022
        // account would need that program, which the guard doesn't allow: its rent stays.
        if !nativeIn, !from.token2022, try await rpc.tokenBalance(owner: wallet.solana, mint: from.mint) == 0 {
            let close = SolanaInstruction(program: SPL.tokenProgram, accounts: [
                .init(pubkey: inAccount, isSigner: false, isWritable: true), .init(pubkey: wallet.solana, isSigner: false, isWritable: true),
                .init(pubkey: wallet.solana, isSigner: true, isWritable: false),
            ], data: Data([9])) // CloseAccount
            let (hash, lastValid) = try await rpc.latestBlockhash()
            let before = try await rpc.balance(wallet.solana)
            try await signAndSend(SolanaTransaction.unsigned(try SolanaTransaction.compileMessage([close], payer: wallet.solana, blockhash: hash)),
                                  lastValid: lastValid, owners: [inAccount: wallet.solana], rules: [], note: "close empty \(from.symbol) account",
                                  lamportsAtLeast: 1_000_000)
            line += " Empty \(from.symbol) account closed: +\(Fmt.num(Decimal(Int64(try await rpc.balance(wallet.solana)) - Int64(before)) / 1_000_000_000, maxFraction: 6)) SOL of rent returned."
        }
        report(line)
        return line
    }

    /// A JupiterZ order's one fill swaps exactly these mints: its input mint (account 6) and output mint (account 8), read
    /// from the transaction's own keys. What the guard can't know — what the user asked to swap — the app checks.
    /// A mint a lookup table names (USDC in the fills for Ondo's stocks, 2026-10-29) is proven by the taker's own account
    /// for it instead (input 2, output 4): a token account's address derives from its mint.
    static func fillSwaps(_ wire: Data, from: String, to: String) -> Bool {
        guard let m = try? SolanaMessage.parse(transaction: wire) else { return false }
        let fills = m.instructions.filter { $0.programIndex < m.staticKeys.count && m.staticKeys[$0.programIndex] == SolanaGuard.jupiterZ }
        guard fills.count == 1, let ix = fills.first, ix.accounts.count > 8 else { return false }
        func key(_ i: Int) -> String? { ix.accounts[i] < m.staticKeys.count ? m.staticKeys[ix.accounts[i]] : nil }
        func swaps(mint at: Int, account: Int, _ mint: String) -> Bool {
            if let named = key(at) { return named == mint }
            guard let taker = key(0), let own = key(account) else { return false }
            return [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: taker, mint: mint, program: $0)) == own }
        }
        return swaps(mint: 6, account: 2, from) && swaps(mint: 8, account: 4, to)
    }

    /// Guard and simulation here, the signer's own guard, then send and wait. `rules`: (account, label, token?, least change).
    /// Returns the simulated change per watched balance; `dryRun` stops before signing. `sponsor`: Jupiter's wallet pays the
    /// fee and signs first, and the transaction goes back through Jupiter's /execute with its order id.
    @discardableResult
    func signAndSend(_ unsigned: Data, lastValid: UInt64, owners: [String: String], rules: [(String, String, Bool, Int64)], note: String,
                             lamportsAtLeast: Int64 = -5_000_000, dryRun: Bool = false, sponsor: (key: String, requestId: String)? = nil) async throws -> [String] {
        let message = try SolanaMessage.parse(transaction: unsigned)
        let accounts = try await rpc.resolveAccounts(message)
        var verdict = SolanaGuard.check(message, accounts: accounts, policy: SolanaPolicy(owner: wallet.solana, sponsor: sponsor?.key), tokenOwners: owners)
        // A program or an account the guard refuses stops it before the simulation: that answer needs no balances.
        guard verdict.ok else { throw GuardStop("The guard stopped \(note): " + verdict.problems.joined(separator: " ")) }
        let watch = rules.map(\.0) + [wallet.solana]
        var before: [String: Int64] = [:]
        for (account, _, token, _) in rules {
            if token {
                let r = try? await rpc.call("getTokenAccountBalance", [account, ["commitment": "confirmed"]]) as? [String: Any]
                before[account] = ((r?["value"] as? [String: Any])?["amount"] as? String).flatMap(Int64.init) ?? 0
            }
        }
        before[wallet.solana] = Int64(try await rpc.balance(wallet.solana))
        let simulation = try await rpc.simulate(unsigned, watch: watch, sigVerify: false)
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let error = simulation.error { throw ExecutionError("The simulation failed, so nothing was signed: \(error)") }
        verdict.merge(SolanaGuard.balances(rules.map { account, label, token, least in
            .init(label: label, before: before[account] ?? 0, after: Int64(token ? simulation.after[account]?.token ?? 0 : simulation.after[account]?.lamports ?? 0), atLeast: least)
        } + [.init(label: "lamports", before: before[wallet.solana] ?? 0, after: Int64(simulation.after[wallet.solana]?.lamports ?? 0), atLeast: lamportsAtLeast)]))
        guard verdict.ok else { throw GuardStop("The guard stopped \(note): " + verdict.problems.joined(separator: " ")) }
        var changes = rules.map { account, label, token, _ in
            let after = Int64(token ? simulation.after[account]?.token ?? 0 : simulation.after[account]?.lamports ?? 0)
            return "\(label): \(after - (before[account] ?? 0)) (raw units)"
        }
        let solDelta = Decimal(Int64(simulation.after[wallet.solana]?.lamports ?? 0) - (before[wallet.solana] ?? 0)) / 1_000_000_000
        changes.append("SOL: \(Fmt.num(solDelta, maxFraction: 6)) (\(solDelta < 0 ? "fee and rent for new accounts" : "rent back from closed accounts"))")
        if dryRun { return changes }
        if let sponsor {
            // Only our slot, the second signature, may differ from what was checked.
            let signed = try await signer.signSolanaSponsored(unsigned, accounts: accounts, tokenOwners: owners, sponsor: sponsor.key)
            guard signed.count == unsigned.count, signed.prefix(65) == unsigned.prefix(65), signed.dropFirst(129) == unsigned.dropFirst(129) else {
                throw GuardStop("What was signed isn't what was checked.")
            }
            let ours = Base58.encode(signed.subdata(in: 65..<129))
            try journal.append(.init(chain: "solana", id: ours, kind: .signed, note: "\(note); Jupiter order \(sponsor.requestId), valid until block \(lastValid)"))
            do {
                let landed = try await Jupiter.execute(signed, requestId: sponsor.requestId)
                try journal.append(.init(chain: "solana", id: ours, kind: .confirmed, note: "transaction \(landed)"))
            } catch {
                try journal.append(.init(chain: "solana", id: ours, kind: .failed, note: "\(error.localizedDescription)"))
                throw ExecutionError("Jupiter didn't send the swap: \(error.localizedDescription)")
            }
            return changes
        }
        let signed = try await signer.signSolana(unsigned, accounts: accounts, recipients: [], tokenOwners: owners)
        guard signed.count == unsigned.count, signed.subdata(in: 65..<signed.count) == unsigned.subdata(in: 65..<unsigned.count) else {
            throw GuardStop("What was signed isn't what was checked.")
        }
        let signature = Base58.encode(signed.subdata(in: 1..<65))
        try journal.append(.init(chain: "solana", id: signature, kind: .signed, note: "\(note); valid until block \(lastValid)"))
        switch try await broadcastSolana(signed, signature: signature, lastValid: lastValid) {
        case .confirmed: try journal.append(.init(chain: "solana", id: signature, kind: .confirmed))
        case .failed(let e):
            try journal.append(.init(chain: "solana", id: signature, kind: .failed, note: e))
            throw ExecutionError("The transaction failed: \(e).")
        case .expired:
            try journal.append(.init(chain: "solana", id: signature, kind: .expired))
            throw ExecutionError("The transaction didn't make it into a block; you can retry.")
        }
        return changes
    }

    /// Where «buy NEAR» takes its USDC — the one rule the card shows and the executor signs: Solana when it holds the
    /// amount, else the first EVM network 1Click takes USDC from (OneClick.Asset.usdc: Base, Arbitrum, Ethereum — Ethereum
    /// last) that holds it, straight into 1Click: one hop, not a bridge to Solana and a second swap. nil: Solana.
    public static func nearSource(usdc: Decimal, onSolana: Decimal, held: (String) -> Decimal?) -> EVMNetwork? {
        guard onSolana < usdc else { return nil }
        return EVMNetwork.searched.first { OneClick.Asset.usdc(onEVM: $0.key) != nil && (held($0.key) ?? 0) >= usdc }
    }

    /// Native NEAR for USDC through NEAR Intents 1Click, delivered to `account` — the wallet's own NEAR implicit account,
    /// which only the signer derives. The USDC is Solana's, or an EVM network's when Solana is short (`nearSource`), sent
    /// to 1Click's deposit address there; a refund goes back to the wallet on the same network. The first quote is what
    /// the user approved; the live one must stay within 1% of it, and 1Click itself must have the deposit address bound
    /// to our recipient, refund address, asset and amount.
    public func buyNear(usdc: Decimal, account: String) async throws -> String {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard usdc >= 1 else { throw ExecutionError("Buy NEAR from 1 USDC: below that 1Click's fee eats too much.") }
        guard account.wholeMatch(of: /[0-9a-f]{64}/) != nil else { throw ExecutionError("That is not a NEAR implicit account.") }
        let onSolana = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        let idle = Dictionary(await idleUSDC().map { ($0.net.key, $0.usdc) }, uniquingKeysWith: +)
        let src = Self.nearSource(usdc: usdc, onSolana: onSolana, held: { idle[$0] })
        let origin = src.flatMap { OneClick.Asset.usdc(onEVM: $0.key) } ?? OneClick.Asset.solanaUSDC
        let refundTo = src == nil ? wallet.solana : wallet.evm
        let shown = try await OneClick.quote(dry: true, origin: origin, destination: OneClick.Asset.near,
                                             amount: "\(units)", refundTo: refundTo, recipient: account)
        var envelope = Envelope.approve(input: usdc, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
        let quote = try await OneClick.quote(dry: false, origin: origin, destination: OneClick.Asset.near,
                                             amount: "\(units)", refundTo: refundTo, recipient: account)
        guard let deposit = quote.depositAddress else { throw ExecutionError("1Click gave no deposit address.") }
        envelope.destination = deposit
        let problems = envelope.violations(input: Decimal(string: quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                           output: Decimal(string: quote.amountOutFormatted) ?? 0, destination: deposit)
        guard problems.isEmpty else { throw ExecutionError("The price moved past what was approved: " + problems.joined(separator: " ")) }
        let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: account, refundTo: refundTo,
                                         origin: origin, destination: OneClick.Asset.near, amount: "\(units)")
        guard binding.ok else { throw GuardStop("The guard stopped the deposit: " + binding.problems.joined(separator: " ")) }
        let from = src?.name ?? "Solana"
        report("NEAR Intents: \(Fmt.num(usdc)) USDC from \(from) → ≈ \(Fmt.num(Decimal(string: quote.amountOutFormatted) ?? 0, maxFraction: 4)) NEAR to your NEAR account.")
        let tx: String
        if let src {
            // Gas there first; the USDC goes to the deposit address, which the signer asks to allow in person.
            try await ensureEVM(src, usdc: units)
            tx = try await sendEVM(src.rpc, to: src.usdc, data: try ERC20.transfer(to: deposit, amount: units),
                                   note: "Send \(Fmt.num(usdc)) USDC to the 1Click deposit", recipients: [deposit])
        } else {
            tx = String(try await sendSolanaUSDC(units, to: deposit, label: "the 1Click deposit").split(separator: "/").last ?? "")
        }
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "USDC→NEAR \(Fmt.num(usdc)) from \(from) to \(account)"))
        if !tx.isEmpty { try? await OneClick.submitDeposit(txHash: tx, depositAddress: deposit) }
        let started = Date()
        let s = try await oneClickSettled(deposit, refund: "USDC went back to \(from)", estimate: quote.timeEstimate)
        let line = "Bought \(Fmt.num(Decimal(string: s.amountOutFormatted ?? "") ?? 0, maxFraction: 4)) NEAR for \(Fmt.num(usdc)) USDC from \(from) in \(Int(Date().timeIntervalSince(started))) s — native NEAR on your NEAR account."
        report(line)
        return line
    }

    /// What each of 1Click's states means, said on the running card as it changes.
    public static let oneClickWords = [
        "PENDING_DEPOSIT": "waiting for the deposit", "KNOWN_DEPOSIT_TX": "deposit sent, waiting for it to land",
        "INCOMPLETE_DEPOSIT": "the deposit came short: 1Click refunds it", "PROCESSING": "deposit received, swapping",
        "SUCCESS": "done", "REFUNDED": "refunded", "FAILED": "failed",
    ]

    /// 1Click's progress on `deposit` until it settles, each new state a line on the running card («1Click · deposit
    /// received, swapping · 12 s of ≈ 30»), written to the journal at the end. Throws on a refund or a failure, saying
    /// what went back (`refund`), and after 10 minutes without an end.
    func oneClickSettled(_ deposit: String, refund: String, estimate: Int?) async throws -> OneClick.Status {
        let started = Date()
        var last = ""
        while Date().timeIntervalSince(started) < 600 {
            let s = try await OneClick.status(depositAddress: deposit)
            if s.status != last {
                last = s.status
                let took = Int(Date().timeIntervalSince(started))
                report("1Click · \(Self.oneClickWords[s.status] ?? s.status.lowercased()) · \(took) s\(estimate.map { " of ≈ \($0)" } ?? "")")
            }
            if s.isFinal {
                try journal.append(.init(chain: "1click", id: deposit, kind: s.status == "SUCCESS" ? .settled : .failed, note: s.status))
                guard s.status == "SUCCESS" else { throw ExecutionError("1Click \(s.status.lowercased()): \(s.refundedFormatted.map { "\($0) \(refund)" } ?? "see the journal").") }
                return s
            }
            try await Task.sleep(for: .seconds(3))
        }
        throw ExecutionError("1Click hasn't finished within 10 minutes. The deposit \(deposit) is in the journal; it settles or refunds on its own.")
    }

    /// Native NEAR from the wallet's NEAR account → USDC through NEAR Intents 1Click (the reverse of `buyNear`), on our
    /// Solana address or, with `to`, at our own address on that EVM network (Base, Arbitrum, Ethereum). nil = all the
    /// account can give: its storage stays, and so does what the transaction may need (`NearTransaction.intentDepositOverhead`,
    /// mostly refunded). The live quote must stay within 1% of the one shown; 1Click must have the deposit address bound to
    /// our address there as recipient and our NEAR account for refunds; the signer builds the wrap.near transaction itself
    /// from the amount and the deposit address.
    public func sellNear(amount requested: Decimal?, account: String, to net: EVMNetwork? = nil) async throws -> String {
        guard account.wholeMatch(of: /[0-9a-f]{64}/) != nil else { throw ExecutionError("That is not a NEAR implicit account.") }
        let destination = try net.map { n in
            guard let asset = OneClick.Asset.usdc(onEVM: n.key) else { throw ExecutionError("1Click pays NEAR's USDC on Solana, Base, Arbitrum and Ethereum, not on \(n.name).") }
            return asset
        } ?? OneClick.Asset.solanaUSDC
        let recipient = net == nil ? wallet.solana : wallet.evm, place = net.map { "your address on \($0.name)" } ?? "your Solana address"
        let context = try await Near.txContext(account: account)
        let micro: UInt128 = 1_000_000_000_000_000_000   // 10⁻⁶ NEAR: amounts are whole micro-NEAR
        let storage = UInt128(context.storageBytes) * 10_000_000_000_000_000_000   // 10¹⁹ yocto a byte
        let balance = UInt128(context.balance) ?? 0
        let keep = storage + NearTransaction.intentDepositOverhead(gasPrice: UInt128(context.gasPrice) ?? 100_000_000)
        let free = balance > keep ? (balance - keep) / micro * micro : 0
        let yocto = requested.flatMap { UInt128(NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).stringValue) }.map { $0 * micro } ?? free
        guard yocto >= 50_000 * micro else { throw ExecutionError("Sell NEAR from 0.05: below that 1Click's fee eats too much.") }
        guard yocto <= free else {
            throw ExecutionError("\(NearTransaction.near(free)) NEAR can be sold: the account keeps \(NearTransaction.near(keep)) for its storage and the transaction.")
        }
        let near = Decimal(string: NearTransaction.near(yocto)) ?? 0, units = "\(yocto)"
        let shown = try await OneClick.quote(dry: true, origin: OneClick.Asset.near, destination: destination,
                                             amount: units, refundTo: account, recipient: recipient)
        var envelope = Envelope.approve(input: near, quotedOutput: Decimal(string: shown.amountOutFormatted) ?? 0)
        let quote = try await OneClick.quote(dry: false, origin: OneClick.Asset.near, destination: destination,
                                             amount: units, refundTo: account, recipient: recipient)
        guard let deposit = quote.depositAddress, deposit.wholeMatch(of: /[0-9a-f]{64}/) != nil else {
            throw ExecutionError("1Click gave no NEAR deposit account.")
        }
        envelope.destination = deposit
        var problems = envelope.violations(input: Decimal(string: quote.amountInFormatted) ?? .greatestFiniteMagnitude,
                                           output: Decimal(string: quote.amountOutFormatted) ?? 0, destination: deposit)
        if let d = quote.deadline.flatMap({ try? Date($0, strategy: .iso8601) }), d < Date().addingTimeInterval(300) {
            problems.append("The quote expires in less than 5 minutes.")
        }
        guard problems.isEmpty else { throw ExecutionError("The price moved past what was approved: " + problems.joined(separator: " ")) }
        let binding = DepositGuard.check(bound: try await OneClick.boundRequest(depositAddress: deposit), recipient: recipient, refundTo: account,
                                         origin: OneClick.Asset.near, destination: destination, amount: units)
        guard binding.ok else { throw GuardStop("The guard stopped the deposit: " + binding.problems.joined(separator: " ")) }
        report("NEAR Intents: \(Fmt.num(near, maxFraction: 6)) NEAR → ≈ \(Fmt.num(Decimal(string: quote.amountOutFormatted) ?? 0)) USDC to \(place).")
        let signed = try await signer.signNearIntentDeposit(amount: yocto, depositAddress: deposit, nonce: context.nonce, blockHash: context.blockHash)
        try journal.append(.init(chain: "1click", id: deposit, kind: .signed, note: "NEAR→USDC \(Fmt.num(near, maxFraction: 6)) to \(recipient)\(net.map { " on \($0.name)" } ?? "")"))
        let hash = try await Near.send(signed)
        try journal.append(.init(chain: "near", id: hash, kind: .confirmed, note: "wNEAR to 1Click \(deposit)"))
        try? await OneClick.submitDeposit(txHash: hash, depositAddress: deposit)
        let started = Date(), estimate = quote.timeEstimate
        let landing: @Sendable () async throws -> String = { [self] in
            let s = try await oneClickSettled(deposit, refund: "wNEAR went back to your NEAR account", estimate: estimate)
            let line = "Sold \(Fmt.num(near, maxFraction: 6)) NEAR for \(Fmt.num(Decimal(string: s.amountOutFormatted ?? "") ?? 0)) USDC in \(Int(Date().timeIntervalSince(started))) s — on \(place). https://nearblocks.io/txns/\(hash)"
            report(line)
            return line
        }
        // Inside a chain the next step is signed while 1Click swaps and pays (Landings).
        if let landings = Self.landings {
            landings.add(net?.key ?? "solana") { try await landing() }
            return "Sent \(Fmt.num(near, maxFraction: 6)) NEAR to NEAR Intents: ≈ \(Fmt.num(Decimal(string: quote.amountOutFormatted) ?? 0)) USDC lands on \(place) in about a minute, and what needs it waits for it."
        }
        return try await landing()
    }

    /// Hyperliquid's HLP vault from the main perp balance. Deposits are locked for 4 days; withdraw with nil takes
    /// everything that can be withdrawn now.
    public func hlp(deposit: Bool, amount requested: Decimal?) async throws -> String {
        var free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
        let equity = try await HyperliquidAccount.vaultEquity(user: wallet.evm, vault: HLGuard.hlpVault)
        let amount: Decimal
        if deposit {
            // Hyperliquid refuses vault deposits under $5 (checked live 2026-09-25); say so before signing.
            guard let requested, requested >= 5 else { throw ExecutionError("Hyperliquid accepts vault deposits from $5.") }
            // Margin left on a HIP-3 exchange (after an oil or stock trade) comes back to the main balance first.
            for dex in Hyperliquid.hip3Dexes where free < requested {
                let there = try await Hyperliquid.dexBalance(user: wallet.evm, dex: dex).withdrawable
                let pull = min(there, requested - free).rounded(2, .up)
                guard pull > 0, pull <= there else { continue }
                try await moveMargin(from: dex, to: "", usdc: pull)
                free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
            }
            guard requested <= free else { throw ExecutionError("\(Fmt.usd(free)) free on Hyperliquid; can't deposit \(Fmt.usd(requested)).") }
            amount = requested
        } else {
            guard equity.withdrawable > 0 else {
                throw ExecutionError(equity.equity > 0 ? "\(Fmt.usd(equity.equity)) in HLP, locked for 4 days after a deposit: withdrawable from \(equity.lockedUntil.map(Fmt.date) ?? "soon")." : "Nothing in HLP.")
            }
            amount = min(requested ?? equity.withdrawable, equity.withdrawable)
        }
        let units = NSDecimalNumber(decimal: (amount * 1_000_000).rounded(0, .down)).uint64Value
        let action = MsgPack.map([("type", .string("vaultTransfer")), ("vaultAddress", .string(HLGuard.hlpVault)),
                                  ("isDeposit", .bool(deposit)), ("usd", .uint(units))])
        let reply = try await post(action, label: deposit ? "HLP deposit" : "HLP withdraw")
        guard !reply.isError else { throw ExecutionError("Hyperliquid rejected it: \(reply).") }
        let line = deposit
            ? "Deposited \(Fmt.usd(amount)) into HLP (\(Fmt.num(((try? await Yields.hyperliquidVaultAPR(HLGuard.hlpVault)) ?? 0).rounded(2, .plain)))% APY historically). You can withdraw after 4 days; there are losing periods."
            : "Withdrew \(Fmt.usd(amount)) from HLP to the main Hyperliquid balance."
        report(line)
        return line
    }

    /// "Spread USDC across the best rates": plans with `Allocation.plan` over the live rates and what is already
    /// deposited, and with `execute` carries the slices out one by one through the same checked paths.
    /// USDC is counted in dollars, SOL in SOL; SOL keeps 0.02 for fees and account rent.
    /// Returns the text and whether there is anything to confirm.
    /// `from`: only that network's free money ("solana", "base"…), what a slice elsewhere needs bridged from it.
    public func allocate(_ symbol: String, maxShare: Decimal = 50, limit: Decimal? = nil, without: [String] = [], from: String? = nil,
                         execute: Bool) async throws -> (text: String, ready: Bool, preview: AllocationPreview) {
        let isSOL = symbol.uppercased() == "SOL"
        let asset = isSOL ? "SOL" : "USDC"
        // «закинь 5 долларов из бейз в лучшие волты» took 1.69 from Solana and 3.41 from Base: with `from` the rest stays put.
        let onSolana: Decimal = from != nil && from != "solana" && !isSOL ? 0 : isSOL
            ? max(0, Decimal(try await rpc.balance(wallet.solana)) / 1_000_000_000 - Decimal(string: "0.02")!)
            : Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
        // USDC idle on the EVM networks is free too: a slice there takes it where it is, and what Solana's slices lack
        // comes over from them (below). Before, «spread USDC» saw only Solana's and left 2,960 idle on Base out.
        let idle = isSOL || from == "solana" ? [] : await idleUSDC().filter { from == nil || $0.net.key == from }
        let held = onSolana + idle.reduce(0) { $0 + $1.usdc }
        // «spread 2 USDC»: at most that much of what is free.
        let free = limit.map { min(held, $0) } ?? held
        async let optionsCall = Yields.options(for: asset)
        async let vaultsCall = try? JupiterLend.vaults()
        async let jupCall = try? JupiterLend.positions(user: wallet.solana)
        async let kamCall = try? Kamino.positions(wallet: wallet.solana)
        let solPrice = try await Hyperliquid.markets()["SOL"]?.markPx ?? 0
        let options = await optionsCall
        let vault = (await vaultsCall ?? []).first { $0.assetSymbol.uppercased() == (isSOL ? "WSOL" : "USDC") }
        let jupPositions = await jupCall ?? []
        let jupiter = vault.flatMap { v in jupPositions.first { $0.vault == v.symbol }.map { Decimal($0.assets) / Decimal(sign: .plus, exponent: v.decimals, significand: 1) } } ?? 0
        let kaminoUSD = (await kamCall ?? []).filter { $0.symbol.uppercased() == asset }.reduce(0) { $0 + $1.valueUSD }
        let kamino = isSOL ? (solPrice > 0 ? kaminoUSD / solPrice : 0) : kaminoUSD
        var jito: Decimal = 0
        if isSOL, let held = try? await rpc.tokenBalance(owner: wallet.solana, mint: SpotAsset.jitoSOL.mint), held > 0,
           let q = try? await Jupiter.quote(input: SpotAsset.jitoSOL.mint, output: SpotAsset.all[0].mint, amount: held) {
            jito = Decimal(q.outAmount) / 1_000_000_000
        }
        // Aave and Morpho on the EVM networks, USDC only: rates and balances from the pools and vaults themselves.
        let evm = isSOL ? [] : await evmPlaces()
        let total = free + jupiter + kamino + jito + evm.reduce(0) { $0 + $1.supplied }
        let kaminoRentUSD = Decimal(string: "0.0298")! * solPrice
        // A vault switched off in Settings › Modules is skipped, with the reason in the notes; so is one they asked to leave out.
        let modules = Modules.current
        func leftOut(_ place: String) -> String? { without.contains { place.lowercased().contains($0) } ? "left out, as asked" : nil }
        let places: [Allocation.Place] = options.compactMap { o in
            switch o.place {
            case "Jupiter Lend": return .init(name: o.place, apy: o.apy, existing: jupiter, blocked: leftOut(o.place) ?? modules.refusal(vault: o.place))
            case "JitoSOL staking": return .init(name: o.place, apy: o.apy, existing: jito, blocked: leftOut(o.place) ?? modules.refusal(vault: o.place))
            case "Kamino Lend":
                // The first deposit's rent is a cost, not a refundable deposit: worth it only for a big enough slice.
                let slice = min(free, total * maxShare / 100 - kamino) * (isSOL ? solPrice : 1)
                let blocked = kamino == 0 && kaminoRentUSD > slice / 10
                    ? "the first deposit creates Kamino accounts for ≈ \(Fmt.usd(kaminoRentUSD)) of rent, more than 10% of the \(Fmt.usd(slice)) slice" : nil
                return .init(name: o.place, apy: o.apy, existing: kamino, blocked: leftOut(o.place) ?? modules.refusal(vault: o.place) ?? blocked)
            case "HLP (Hyperliquid)":
                return .init(name: o.place, apy: o.apy, blocked: "another network: bridge, $5 minimum and a 4-day lock — use a separate command, «deposit 5 USDC to HLP»")
            default: return nil
            }
        }
        var all = places
        // A known place whose rate, balance or gas didn't answer this time is named with why, not left out silently:
        // Morpho on Ethereum vanished from a spread between two messages, and nothing said so.
        if !isSOL {
            let read = Set(evm.map(\.name))
            for name in AaveV3.markets.map(\.place) + MorphoVault.vaults.map(\.place) where !read.contains(name) && modules.refusal(vault: name) == nil {
                all.append(.init(name: name, apy: options.first { $0.place == name }?.apy ?? 0, blocked: leftOut(name) ?? "its rate, balance or gas couldn't be read just now — ask again in a minute"))
            }
        }
        for p in evm {
            // Worth it only when the round trip costs less than half a year of the slice's interest: on Ethereum that
            // takes a large slice, on the L2s a few dollars.
            let slice = max(0, min(free, total * maxShare / 100 - p.supplied))
            all.append(.init(name: p.name, apy: p.apy, existing: p.supplied,
                             blocked: leftOut(p.name) ?? modules.refusal(vault: p.name) ?? p.tooCostly(yearly: slice * p.apy / 100), protocol: p.protocolName))
        }
        let plan = Allocation.plan(free: free, places: all, maxShare: maxShare, minSlice: isSOL ? Decimal(string: "0.003")! : Decimal(string: "0.5")!)
        func amount(_ d: Decimal) -> String { Fmt.num(d, maxFraction: isSOL ? 6 : 2) + " " + asset }
        var lines = ["Spread \(amount(free)) free (\(amount(total)) total in \(asset)), at most \(Fmt.pct(maxShare)) per protocol:"]
        let sources = (onSolana > 0 ? [("Solana", onSolana)] : []) + idle.map { ($0.net.name, $0.usdc) }
        if !idle.isEmpty { lines.append("· free now: " + sources.map { "\(amount($0.1)) on \($0.0)" }.joined(separator: ", ")) }
        lines += plan.slices.map { "· \($0.place): \(amount($0.amount)) at \(Fmt.num($0.apy.rounded(2, .plain)))%" }
        if plan.slices.isEmpty { lines.append("· nowhere to put it: everything is at its limit or unavailable.") }
        lines += plan.notes.map { "· " + $0 }
        if plan.leftover > 0 { lines.append("Left free: \(amount(plan.leftover)).") }
        if plan.yearly > 0 { lines.append("Income from the new deposits ≈ \(amount(plan.yearly)) a year\(isSOL && solPrice > 0 ? " (≈ \(Fmt.usd(plan.yearly * solPrice)))" : "").") }
        // Where each slice's money comes from, straight from the network that holds it (Self.spreadMoves).
        func network(_ place: String) -> String? { AaveV3.market(place: place)?.chain ?? MorphoVault.vault(place: place)?.chain }
        func name(_ key: String) -> String { key == "solana" ? "Solana" : EVMNetwork.named(key)?.name ?? key }
        let money = Self.spreadMoves(slices: plan.slices.map { (network($0.place), $0.amount) }, idle: idle.map { ($0.net.key, $0.usdc) }, onSolana: onSolana)
        var preview = AllocationPreview(asset: asset, free: free, maxShare: maxShare, slices: plan.slices,
                                        skipped: all.compactMap { p in p.blocked.map { (p.name, p.apy, $0) } },
                                        leftover: plan.leftover, yearly: plan.yearly, yearlyUSD: isSOL && solPrice > 0 ? plan.yearly * solPrice : nil,
                                        sources: sources.map { (place: $0.0, amount: $0.1) })
        preview.drawn = money.drawn.map { (place: name($0.network), amount: $0.usdc) }
        preview.moves = money.moves.map { (from: name($0.from), to: name($0.to), amount: $0.usdc) }
        // The preview goes back to the caller, which adds how to confirm.
        guard execute, !plan.slices.isEmpty else { return (lines.joined(separator: "\n"), !plan.slices.isEmpty, preview) }
        report(lines.joined(separator: "\n"))
        var done: [String] = []
        // The money goes first, each move straight from the network that holds it to the one a slice is on (Solana's
        // slices included); what Solana itself gives an EVM slice, the deposit brings on its own (ensureEVM).
        for m in money.moves where m.from != "solana" {
            guard let src = EVMNetwork.named(m.from) else { continue }
            try await ensureEVM(src, usdc: 0)
            done.append(try await (m.to == "solana" ? evmToSolana(src, usdc: m.usdc) : bridge(src, to: EVMNetwork.named(m.to), usdc: m.usdc)))
        }
        // EVM slices first: their shortfall leaves Solana before Solana's own slices spend it.
        for slice in plan.slices.sorted(by: { network($0.place) != nil && network($1.place) == nil }) {
            var piece = slice.amount.rounded(isSOL ? 6 : 2, .down)
            if !isSOL, network(slice.place) == nil {
                // Relay's fees on the way leave Solana a few cents under the plan: a slice takes at most what is there.
                let there = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) / 1_000_000
                piece = min(piece, there.rounded(2, .down))
            } else if let key = network(slice.place), money.moves.contains(where: { $0.to == key && $0.from != "solana" }), let net = EVMNetwork.named(key) {
                // The same on an EVM network a move just reached.
                let there = Decimal(try await net.usdcBalance(wallet.evm)) / 1_000_000
                piece = min(piece, (there - Decimal(net.feeReserve) / 1_000_000).rounded(2, .down))
            }
            switch slice.place {
            case "Jupiter Lend": done.append(try await lend(asset, amount: piece, deposit: true))
            case "Kamino Lend": done.append(try await self.kamino(asset, amount: piece, deposit: true))
            case "JitoSOL staking": done.append(try await convert(from: SpotAsset.all[0], to: .jitoSOL, amount: piece))
            default:
                if let m = AaveV3.market(place: slice.place) { done.append(try await self.aave(deposit: true, usdc: piece, market: m)) }
                else if let v = MorphoVault.vault(place: slice.place) { done.append(try await self.morpho(deposit: true, usdc: piece, vault: v)) }
            }
        }
        let line = "Spread across \(done.count) \(done.count == 1 ? "protocol" : "protocols")."
        report(line)
        return (line, false, preview)
    }

    /// Where a spread's money comes from. A slice on an EVM network (`network` set) takes the USDC idle there first;
    /// Solana's slices take Solana's own. What a place still lacks comes straight from the idle USDC of the other
    /// networks, in the order given (Ethereum last) — a dime over each for Relay's fee, nothing under a bridge's half
    /// dollar — and an EVM slice's last gap from what Solana has left («solana» → network, brought by the deposit).
    /// `drawn`: what each place gives in all, its own slices and its moves; network keys, «solana» for Solana.
    static func spreadMoves(slices: [(network: String?, amount: Decimal)], idle: [(network: String, usdc: Decimal)], onSolana: Decimal)
        -> (moves: [(from: String, to: String, usdc: Decimal)], drawn: [(network: String, usdc: Decimal)]) {
        var spare = Dictionary(idle.map { ($0.network, $0.usdc) }, uniquingKeysWith: +), drawn: [String: Decimal] = [:]
        var short: [(to: String, usdc: Decimal)] = [], onSolanaNeeds: Decimal = 0
        for slice in slices {
            guard let key = slice.network else { onSolanaNeeds += slice.amount; continue }
            let use = min(spare[key] ?? 0, slice.amount)
            spare[key, default: 0] -= use
            drawn[key, default: 0] += use
            if slice.amount > use { short.append((key, slice.amount - use)) }
        }
        let own = min(onSolana, onSolanaNeeds)
        var solanaLeft = onSolana - own
        drawn["solana", default: 0] += own
        if onSolanaNeeds > own { short.append(("solana", onSolanaNeeds - own)) }
        var moves: [(from: String, to: String, usdc: Decimal)] = []
        for var need in short {
            for (key, _) in idle where need.usdc > 0 && key != need.to {
                let send = min(spare[key] ?? 0, need.usdc + Decimal(string: "0.1")!).rounded(2, .down)
                guard send >= Decimal(string: "0.5")! else { continue }
                moves.append((key, need.to, send))
                spare[key, default: 0] -= send
                drawn[key, default: 0] += send
                need.usdc -= send
            }
            if need.usdc > 0, need.to != "solana", solanaLeft > 0 {
                let send = min(solanaLeft, need.usdc)
                moves.append(("solana", need.to, send))
                solanaLeft -= send
                drawn["solana", default: 0] += send
            }
        }
        let order = ["solana"] + idle.map(\.network)
        return (moves, order.compactMap { k in drawn[k].flatMap { $0 > 0 ? (k, $0) : nil } })
    }

    /// What comes over to Solana from each EVM network before a spread (spreadMoves' moves into Solana).
    static func pullToSolana(slices: [(network: String?, amount: Decimal)], idle: [(network: String, usdc: Decimal)],
                             onSolana: Decimal) -> [(network: String, usdc: Decimal)] {
        spreadMoves(slices: slices, idle: idle, onSolana: onSolana).moves.filter { $0.to == "solana" }.map { ($0.from, $0.usdc) }
    }

    /// SOL spent according to a simulation's change lines (the «SOL: −0.0298» line), as a positive number.
    static func solSpent(_ changes: [String]) -> Decimal {
        guard let line = changes.first(where: { $0.hasPrefix("SOL: ") }) else { return 0 }
        let number = line.dropFirst(5).prefix { !$0.isWhitespace }.replacingOccurrences(of: ",", with: "")   // en_US: «,» only groups thousands
            .replacingOccurrences(of: "−", with: "-")
        return max(0, -(Decimal(string: number) ?? 0))
    }

    /// What «withdraw all» asks Kamino for, in tokens: the position's dollars at the token's price, 1% over. In dollars it
    /// asked 202 «SOL» for 1.3 SOL, and the guard refused every time.
    static func kaminoAll(valueUSD: Decimal, priceUSD: Decimal) -> Decimal {
        (valueUSD / priceUSD * Decimal(string: "1.01")!).rounded(6, .up)
    }

    /// The first Kamino deposit's rent against what goes in: over a tenth of it, and over what the user took on with
    /// «Send anyway» (`accepted`, dollars), is a stop to ask about; nil lets the deposit go.
    static func kaminoRent(sol: Decimal, solPrice: Decimal, depositUSD: Decimal, accepted: Decimal) -> CostlyMove? {
        let rent = sol * solPrice
        guard rent > depositUSD / 10, rent > accepted else { return nil }
        return CostlyMove(lost: rent, paid: depositUSD, what: "Kamino", words: "The first Kamino deposit creates your accounts: rent ≈ \(Fmt.num(sol, maxFraction: 4)) SOL (≈ \(Fmt.usd(rent))) on a \(Fmt.usd(depositUSD)) deposit — over 10% of the amount. Jupiter Lend has no rent; Kamino makes sense from ≈ \(Fmt.usd((rent * 10).rounded(0, .up))). Nothing was signed.")
    }

    /// Kamino Lend: the main market, and the Backpack market for Backpack's stocks (Kamino.market(for:)) — there a
    /// deposit is collateral to borrow USDC against. `symbol`: USDC, SOL, USDT, SPCX… `amount` in tokens; withdraw with
    /// nil takes the whole position (its dollars at the token's price). `dryRun` only simulates: the first deposit's rent
    /// shows up here. `acceptRent`: the plan showed the first deposit's rent (a rebalance, the CLI's flag), so the 10%
    /// rule does not ask.
    public func kamino(_ symbol: String, amount requested: Decimal?, deposit: Bool, dryRun: Bool = false, acceptRent: Bool = false) async throws -> String {
        let market = Kamino.market(for: symbol), stock = market == Kamino.backpackMarket
        guard let reserve = try await Kamino.reserves(market: market).first(where: { $0.symbol.uppercased() == symbol.uppercased() }) else {
            throw ExecutionError("Kamino has no \(symbol) market.")
        }
        let native = reserve.mint == "So11111111111111111111111111111111111111112"
        let supply = try await rpc.call("getTokenSupply", [reserve.mint]) as? [String: Any]
        let decimals = ((supply?["value"] as? [String: Any])?["decimals"] as? NSNumber)?.intValue ?? 6
        let scale = Decimal(sign: .plus, exponent: decimals, significand: 1)
        // On the Backpack market a position is collateral, and a loan may stand on it.
        let loan = stock ? try await Kamino.loan(wallet: wallet.solana, market: market) : nil
        let positionUSD = stock
            ? loan?.collateral.first { $0.reserve == reserve.address }?.valueUSD
            : try await Kamino.positions(wallet: wallet.solana).first { $0.reserve == reserve.address }?.valueUSD
        if stock, !deposit, let loan, loan.debtUSD > 0 {
            // Kamino lets collateral out only while what's left still carries the debt at its borrow limit.
            let risk = try await Kamino.risk(reserve: reserve.address)
            let free = Self.kaminoFreeCollateral(loan, maxLTV: risk.maxLTV, priceUSD: reserve.priceUSD)
            if requested.map({ $0 > free }) ?? true {
                throw ExecutionError("A loan of \(Fmt.usd(loan.debtUSD)) stands on this \(symbol) on Kamino: "
                                     + (free > 0 ? "up to \(Fmt.num(free, maxFraction: 6)) \(symbol) can come out now. " : "")
                                     + "Pay it back first — «repay all USDC on kamino» — and the rest comes out. Nothing was signed.")
            }
        }
        // A deposit made on kamino.com sits in a Kamino Vault (Earn), not in Lend.
        // ponytail: Lend first when both hold the token; «all» then leaves the vault, say it again for that.
        if !deposit, !stock, positionUSD == nil,
           let v = try await Kamino.vaultPositions(wallet: wallet.solana).first(where: { $0.mint == reserve.mint && $0.shares > 0 }) {
            return try await kaminoVault(v, amount: requested, decimals: decimals, dryRun: dryRun)
        }
        // Everything: ask for 1% over the value — Kamino caps a withdrawal at what is there (checked live), so no dust stays.
        let all = !deposit && requested == nil
        if all, positionUSD != nil, reserve.priceUSD <= 0 { throw ExecutionError("Kamino gave no \(symbol) price just now; say how much to withdraw.") }
        guard let amount = requested ?? (deposit ? nil : positionUSD.map { Self.kaminoAll(valueUSD: $0, priceUSD: reserve.priceUSD) }), amount > 0 else {
            throw ExecutionError(deposit ? "How much should go into Kamino?" : "No \(symbol) position in Kamino.")
        }
        let units = NSDecimalNumber(decimal: (amount * scale).rounded(0, .down)).uint64Value
        /// What should come out: the position's value when taking everything, else what was asked.
        let expected = Int64(all ? units * 100 / 101 : units)
        // Backpack's stocks are Token-2022 tokens: their account derives from that program.
        let myToken = try SPL.associatedAccount(owner: wallet.solana, mint: reserve.mint, program: try await tokenProgram(reserve.mint))
        if deposit {
            let have = native ? try await rpc.balance(wallet.solana) : try await rpc.tokenBalance(owner: wallet.solana, mint: reserve.mint)
            guard have >= units else { throw ExecutionError("You have \(Fmt.num(Decimal(have) / scale, maxFraction: 6)) \(symbol); need \(Fmt.num(amount, maxFraction: 6)).") }
        }
        let wire = deposit
            ? try await Kamino.depositTransaction(wallet: wallet.solana, reserve: reserve.address, amount: amount, market: market)
            : try await Kamino.withdrawTransaction(wallet: wallet.solana, reserve: reserve.address, amount: amount, market: market)
        let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
        // The first deposit pays rent for the user's Kamino accounts and lookup table; later ones only the fee. On the
        // Backpack market the position is one for all its stocks.
        let firstTime = (stock ? loan == nil : positionUSD == nil) && deposit
        var rules: [(String, String, Bool, Int64)] = []
        if !native { rules.append((myToken, symbol, true, deposit ? -Int64(units) : expected * 99 / 100)) }
        let lamports: Int64 = native ? (deposit ? -Int64(units) - (firstTime ? 60_000_000 : 5_000_000) : expected * 99 / 100 - 5_000_000)
                                     : (firstTime ? -60_000_000 : -5_000_000)
        // A stock put up on the Backpack market isn't asked about: its position's rent (≈ 0.0176 SOL) comes back when
        // everything is taken out — Kamino closes the position then (measured live 2026-10-28).
        if firstTime, !dryRun, !acceptRent, !stock {
            // The first deposit pays ~0.03 SOL of rent for Kamino's user accounts and lookup table. Not worth it for
            // a small amount: simulate first, and when the rent is over a tenth of what goes in, stop and ask (CostlyMove:
            // «Send anyway», or Jupiter Lend, which has no rent). It was refused outright, which stopped a chain of moves
            // at its last step with no way on.
            let preview = try await signAndSend(wire, lastValid: lastValid, owners: [myToken: wallet.solana], rules: rules,
                                                note: "Kamino \(symbol)", lamportsAtLeast: lamports, dryRun: true)
            let solPrice = try await Hyperliquid.markets()["SOL"]?.markPx ?? 0
            let rentSOL = Self.solSpent(preview)
            let valueUSD = native ? amount * solPrice : amount
            if let stop = Self.kaminoRent(sol: rentSOL, solPrice: solPrice, depositUSD: valueUSD, accepted: Self.lossAccepted) { throw stop }
        }
        let changes = try await signAndSend(wire, lastValid: lastValid, owners: [myToken: wallet.solana], rules: rules,
                                            note: "Kamino \(deposit ? "deposit" : "withdraw") \(symbol)", lamportsAtLeast: lamports, dryRun: dryRun)
        if stock, !dryRun {
            // Read on: the portfolio right after must find the stock under Kamino, not lose it.
            let was = loan?.collateral.first { $0.reserve == reserve.address }?.valueUSD ?? 0
            _ = try? await kaminoLoan(market) { l in
                let v = l?.collateral.first { $0.reserve == reserve.address }?.valueUSD ?? 0
                return deposit ? v > was : v < was
            }
        }
        let did = deposit ? "deposited \(Fmt.num(amount, maxFraction: 6)) \(symbol)" : all ? "withdrew everything" : "withdrew \(Fmt.num(amount, maxFraction: 6)) \(symbol)"
        let line = (dryRun ? "Kamino simulation (nothing signed): "
                    : stock ? "Kamino's Backpack market: \(deposit ? "put up \(Fmt.num(amount, maxFraction: 6)) \(symbol) as collateral" : did). "
                    : "Kamino \(symbol) \(Fmt.num(reserve.supplyAPY.rounded(2, .plain)))% APY: \(did). ")
            + changes.joined(separator: "; ") + "."
        report(line)
        return line
    }

    /// Out of a Kamino Vault: `amount` of its token (nil = all), asked for in shares at Kamino's price per share, which
    /// runs a hair above what comes out (3 asked → 2.999 in simulation). Shares staked in the vault's farm are unstaked
    /// in the same transaction.
    func kaminoVault(_ v: Kamino.VaultPosition, amount requested: Decimal?, decimals: Int, dryRun: Bool) async throws -> String {
        // A cent short of the whole is the whole: «выведи 5» from 4.999 takes every share instead of failing.
        let all = requested.map { $0 >= v.tokens - Decimal(string: "0.01")! } ?? true
        if let requested, requested > v.tokens + Decimal(string: "0.01")! {
            throw ExecutionError("Only \(Fmt.num(v.tokens, maxFraction: 6)) \(v.symbol) in \(v.name) on Kamino; can't withdraw \(Fmt.num(requested, maxFraction: 6)).")
        }
        let amount = all ? v.tokens : requested ?? v.tokens
        guard amount > 0 else { throw ExecutionError("Say how much to withdraw from \(v.name).") }
        let shares = all ? v.shares : min(v.shares, (amount * v.shares / v.tokens).rounded(6, .up))
        let scale = Decimal(sign: .plus, exponent: decimals, significand: 1)
        let expected = Int64(NSDecimalNumber(decimal: (amount * scale).rounded(0, .down)).int64Value)
        let myToken = try SPL.associatedAccount(owner: wallet.solana, mint: v.mint)
        let wire = try await Kamino.vaultWithdrawTransaction(wallet: wallet.solana, vault: v.vault, shares: shares)
        let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
        // ponytail: a SOL vault pays out wSOL, left wrapped in the token account.
        let changes = try await signAndSend(wire, lastValid: lastValid, owners: [myToken: wallet.solana], rules: [(myToken, v.symbol, true, expected * 99 / 100)],
                                            note: "Kamino \(v.name) withdraw", dryRun: dryRun)
        let line = (dryRun ? "Kamino simulation (nothing signed): " : "Kamino \(v.name): withdrew \(all ? "everything, " : "")\(Fmt.num(amount, maxFraction: 6)) \(v.symbol). ")
            + changes.joined(separator: "; ") + "."
        report(line)
        return line
    }
}

/// «Spread USDC across the best rates» as data, for the studio canvas: where each slice goes and what was skipped.
public struct AllocationPreview: Sendable {
    public let asset: String
    public let free: Decimal
    public let maxShare: Decimal
    public let slices: [Allocation.Slice]
    public let skipped: [(place: String, apy: Decimal, reason: String)]
    public let leftover: Decimal
    public let yearly: Decimal
    public let yearlyUSD: Decimal?
    /// Where the free money is: Solana and each EVM network with idle USDC.
    public var sources: [(place: String, amount: Decimal)] = []
    /// What the plan takes from each place, and the moves between them before the deposits (Executor.spreadMoves).
    public var drawn: [(place: String, amount: Decimal)] = []
    public var moves: [(from: String, to: String, amount: Decimal)] = []
}
