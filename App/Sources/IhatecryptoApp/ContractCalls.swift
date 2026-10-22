import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// A contract call the chat wrote out («call arbitrum 0x… deposit(uint256,address) 10e6 me approving 10 USDC»), checked
/// and simulated, waiting for Confirm. The model only writes the command: ihatecrypto encodes it from the function's
/// signature, checks the contract on the network's explorer, simulates the exact approve and the call together and draws
/// the card; anything it can't decode, verify or simulate isn't offered (it fails closed). The signer reads the calldata
/// again: no unlimited approvals, nothing paid to someone else, a new spender confirmed in person.
struct CallReview {
    struct Approve { let token: String; let symbol: String; let amount: Decimal; let units: Decimal }
    let command: WalletCommand
    let call: ContractCall
    let net: EVMNetwork
    let data: Data
    /// Wei of the network's coin sent with the call.
    let value: UInt64
    let approve: Approve?
    /// What it does, read from the calldata.
    let lines: [String]
    /// The simulated change of each of this wallet's balances.
    let changes: [String]
    let trust: ContractTrust
    let warnings: [String]
    /// The simulated calls and what they did: run again just before signing (`runContractCall`).
    let calls: [(to: String, value: UInt64, data: Data)]
    let simulation: EVMSimulation
    /// «First time with this contract», «proxy: its owner can change the code»: on the card, above everything.
    let badges: [String]
    /// «FluidLending» or «0x1a99…6096».
    var contractName: String { trust.implementation ?? trust.name ?? "\(call.contract.prefix(6))…\(call.contract.suffix(4))" }
}

extension Workspace {
    func prepareContractCall(_ command: WalletCommand, note: String? = nil) async {
        guard case .contractCall(let c) = command, let net = EVMNetwork.named(c.network) else { return }
        guard let me = signerWallet?.evm, isEVMAddress(me) else {
            reply(say("Contract calls need this wallet's EVM key in secure storage.", "Для вызова контракта нужен EVM-ключ этого кошелька в защищённом хранилище."), note: note)
            return
        }
        func refuse(_ en: String, _ ru: String) {
            reply(say("I won't make this call: " + en + " Nothing was signed.", "Этот вызов делать не буду: " + ru + " Ничего не подписано."), note: note)
        }
        // Addresses the user gave: in their own last message (never a chain's or a task's command text, which the model
        // wrote), their favorites or their address book.
        let theirs = Set(([me] + favoriteTokens.map(\.address) + addressBook.map(\.address)).map { $0.lowercased() })
        let said = (messages.last(where: \.fromUser)?.text ?? "").lowercased()
        func usersOwn(_ a: String) -> Bool { theirs.contains(a.lowercased()) || said.contains(a.lowercased()) }

        agentStep = say("Encoding the call", "Собираю вызов")
        let data: Data
        do { data = try AgentReads.calldata(c.function, c.args, me: me) } catch {
            refuse(error.localizedDescription, error.localizedDescription)
            return
        }
        // The contract, and every other address among the arguments, is the user's own or verified code on the explorer.
        agentStep = say("Checking the contract on the explorer", "Проверяю контракт в обозревателе")
        let trust = await ContractTrust.check(c.contract, on: net.key)
        if !trust.unknown, !trust.isContract { refuse("\(c.contract) isn't a contract on \(net.name).", "\(c.contract) — не контракт в сети \(net.name)."); return }
        // A protocol outside ihatecrypto's own list (Aave, Morpho, Uniswap, Relay…) must look like one: verified code, a
        // month old at least, money in it as DefiLlama prices it, on no scam list. Each of these blocks.
        let listed = EVMCall.knownSpenders(chainId: net.rpc.chainId).contains(c.contract.lowercased())
        var reputation = ContractReputation()
        if !listed {
            guard trust.verified else {
                refuse("\(c.contract) on \(net.name) has no verified source on the explorer (\(trust.line)).",
                       "у \(c.contract) в \(net.name) нет проверенного исходника в обозревателе (\(trust.line)).")
                return
            }
            guard let days = trust.days, days >= ContractReputation.minDays else {
                refuse("the contract is \(trust.days.map { "\($0) days" } ?? "of unknown age") old; ihatecrypto calls only contracts a month old or more.",
                       "контракту \(trust.days.map { "\($0) дн." } ?? "неизвестно сколько дней"); ihatecrypto вызывает только контракты старше месяца.")
                return
            }
            agentStep = say("Checking its reputation", "Проверяю репутацию")
            reputation = await ContractReputation.check(c.contract, on: net, me: me)
            if let why = reputation.scam { refuse("\(why).", "\(why) — это скам-адрес."); return }
            guard let tvl = reputation.tvlUSD, tvl >= ContractReputation.minTVL else {
                refuse("DefiLlama \(reputation.tvlUSD.map { "prices only \(Fmt.usd($0)) in it" } ?? "doesn't price it") — ihatecrypto calls only protocols with \(Fmt.usd(ContractReputation.minTVL)) or more in them.",
                       "DefiLlama \(reputation.tvlUSD.map { "оценивает в нём только \(Fmt.usd($0))" } ?? "его не оценивает") — ihatecrypto вызывает только протоколы, где лежит от \(Fmt.usd(ContractReputation.minTVL)).")
                return
            }
        }
        // The zero address («no referrer», «none») gives nobody anything; the network's USDC, the big stablecoins there, the
        // token approved and ihatecrypto's own protocols are known; anything else is checked.
        let known = Set(([net.usdc] + Stablecoins.top.compactMap { Stablecoins.find($0, on: net.key)?.address }
                         + [c.approve?.token].compactMap { $0 }.filter { $0.hasPrefix("0x") }).map { $0.lowercased() })
            .union(EVMCall.knownSpenders(chainId: net.rpc.chainId))
        for a in c.args where a.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil && !usersOwn(a) && a.lowercased() != c.contract.lowercased()
            && a.dropFirst(2).contains(where: { $0 != "0" }) && !known.contains(a.lowercased()) {
            let t = await ContractTrust.check(a, on: net.key)
            if let why = await ScamList.shared.names(a) { refuse("the argument \(a): \(why).", "аргумент \(a): \(why)."); return }
            guard t.isContract, t.verified else {
                refuse("the argument \(a) is neither yours nor a verified contract — money could go there.",
                       "аргумент \(a) — не твой адрес и не проверенный контракт: туда могут уйти деньги.")
                return
            }
        }
        // Coins sent with it, in wei.
        var value: UInt64 = 0
        if let v = c.value, v > 0 {
            let wei = (v * 1_000_000_000_000_000_000).rounded(0, .down)
            guard wei <= Decimal(UInt64.max) else { refuse("that's more \(net.gasCoin) than one call takes.", "это больше \(net.gasCoin), чем берёт один вызов."); return }
            value = NSDecimalNumber(decimal: wei).uint64Value
        }
        // The exact approve: the network's USDC or a big stablecoin by ticker, else a token contract.
        var approve: CallReview.Approve?
        if let a = c.approve {
            let up = a.token.uppercased()
            guard let token = a.token.hasPrefix("0x") ? a.token : up == "USDC" ? net.usdc : Stablecoins.find(a.token, on: net.key)?.address,
                  token.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else {
                refuse("I don't know the token «\(a.token)» on \(net.name): write its contract.", "не знаю токен «\(a.token)» в \(net.name): напиши его контракт.")
                return
            }
            let meta = await tokenMeta(token, net)
            approve = .init(token: token, symbol: meta.symbol, amount: a.amount, units: (a.amount * pow(10, meta.decimals)).rounded(0, .down))
        }
        // What the calldata does, and what the signer would refuse anyway.
        let decoded = EVMCall.decode(data)
        if let why = decoded.refusal(sender: me, chainId: net.rpc.chainId) { refuse(why, why); return }
        let foreign = decoded.payees(to: c.contract, value: value, chainId: net.rpc.chainId).filter { ![me.lowercased(), c.contract.lowercased()].contains($0.lowercased()) }
        if let other = foreign.first { refuse("money or the right to spend it would go to \(other).", "деньги или право их тратить ушли бы на \(other)."); return }
        // Both simulated together: the approve, then the call that spends it. No simulation, no card.
        agentStep = say("Simulating it on \(net.name)", "Симулирую в \(net.name)")
        var calls: [(to: String, value: UInt64, data: Data)] = []
        if let a = approve { calls.append((a.token, 0, (try? ERC20.approve(spender: c.contract, units: a.units)) ?? Data())) }
        calls.append((c.contract, value, data))
        let sim: EVMSimulation
        do { sim = try await Self.simulator(net).simulate(from: me, calls: calls) } catch {
            refuse("\(net.name)'s node couldn't simulate it (\(error.localizedDescription)), and ihatecrypto signs nothing it hasn't simulated.",
                   "нода \(net.name) не смогла это просимулировать (\(error.localizedDescription)), а без симуляции ihatecrypto ничего не подписывает.")
            return
        }
        if let revert = sim.revert { refuse("the simulation says it would fail — \(revert).", "симуляция говорит, что он не пройдёт — \(revert)."); return }
        // A call that moves nothing failed quietly: Compound-style markets (Moonwell…) return an error code instead of
        // reverting — no cash to withdraw, a paused market. Signed, it would only cost gas.
        if sim.changes.isEmpty {
            refuse("the simulation shows it changes nothing in your wallet: the protocol refused it without failing (a market with no cash left to withdraw, or paused). Signing it would only pay gas.",
                   "симуляция показывает, что он ничего не меняет в кошельке: протокол отказал, не откатив вызов (в рынке нет свободных денег для вывода или он на паузе). Подпись лишь потратила бы газ.")
            return
        }
        // Rights the balances can't show: an approve, an operator, ownership — only a limited approve to the contract passes.
        if let why = ContractCallGuard.grantProblem(sim.grants, contract: c.contract) { refuse(why + ".", why + "."); return }
        // A protocol outside the list may put at most the cap at risk: what leaves, its approve and the coins sent.
        if !listed {
            let outflows = sim.changes.filter { $0.value < 0 && $0.key != EVMSimulation.native }
            let priced = await ContractReputation.prices(Array(outflows.keys) + (approve.map { [$0.token] } ?? []), on: net)
            var atRisk: Decimal = 0
            for (asset, delta) in outflows {
                let t = await tokenMeta(asset, net)
                guard let p = priced[asset.lowercased()] ?? (Stablecoins.dollars.contains(t.symbol.uppercased()) ? 1 : nil) else {
                    refuse("ihatecrypto can't price the \(t.symbol) that would leave, so it can't keep the call under its cap.",
                           "ihatecrypto не может оценить уходящий \(t.symbol), а значит не может удержать вызов в лимите.")
                    return
                }
                atRisk += -delta / pow(10, t.decimals) * p
            }
            if let a = approve {
                let p = priced[a.token.lowercased()] ?? (Stablecoins.dollars.contains(a.symbol.uppercased()) ? 1 : 0)
                atRisk = max(atRisk, a.amount * p)
            }
            let coins = Decimal(value) / 1_000_000_000_000_000_000 + max(0, -(sim.changes[EVMSimulation.native] ?? 0) / 1_000_000_000_000_000_000 - Decimal(value) / 1_000_000_000_000_000_000)
            if coins > 0 {
                let price = await ContractReputation.coinUSD(net)
                guard price > 0 else { refuse("ihatecrypto can't price \(net.gasCoin) right now, so it can't keep the call under its cap.", "ihatecrypto сейчас не может оценить \(net.gasCoin), а значит не может удержать вызов в лимите."); return }
                atRisk += coins * price
            }
            guard atRisk <= ContractCallGuard.capUSD else {
                refuse("it puts ≈ \(Fmt.usd(atRisk)) at risk in a protocol outside ihatecrypto's list, over the \(Fmt.usd(ContractCallGuard.capUSD)) cap (Settings › Wallet).",
                       "он ставит под риск ≈ \(Fmt.usd(atRisk)) в протоколе вне списка ihatecrypto — больше лимита \(Fmt.usd(ContractCallGuard.capUSD)) (Settings › Wallet).")
                return
            }
        }
        var changes: [String] = []
        for (asset, delta) in sim.changes.sorted(by: { $0.key < $1.key }) {
            if asset == EVMSimulation.native { changes.insert("\(net.gasCoin): \(Self.delta(delta / 1_000_000_000_000_000_000))", at: 0); continue }
            let t = await tokenMeta(asset, net)
            changes.append("\(t.symbol): \(Self.delta(sim.nfts.contains(asset) ? delta : delta / pow(10, t.decimals)))")
        }
        var warnings: [String] = []
        if !sim.changes.isEmpty, sim.changes.values.allSatisfy({ $0 < 0 }) {
            warnings.append(say("Nothing comes back to this wallet in this transaction.", "В этой транзакции на кошелёк ничего не возвращается."))
        }
        if reputation.unchecked { warnings.append(say("The scam list didn't load this time; the other checks passed.", "Список скама в этот раз не загрузился; остальные проверки пройдены.")) }
        var badges: [String] = []
        if reputation.firstTime { badges.append(say("First time with this contract", "Первый раз с этим контрактом")) }
        if trust.implementation != nil { badges.append(say("Proxy: its owner can change the code", "Прокси: владелец может поменять код")) }
        if !listed, let tvl = reputation.tvlUSD { badges.append(say("Outside ihatecrypto's list · \(Fmt.usd(tvl)) in it · cap \(Fmt.usd(ContractCallGuard.capUSD))",
                                                                   "Вне списка ihatecrypto · в нём \(Fmt.usd(tvl)) · лимит \(Fmt.usd(ContractCallGuard.capUSD))")) }
        let review = CallReview(command: command, call: c, net: net, data: data, value: value, approve: approve,
                                lines: [decoded.summary(chainId: net.rpc.chainId, to: c.contract, value: value)], changes: changes.isEmpty ? [say("None of your balances change.", "Балансы не меняются.")] : changes,
                                trust: trust, warnings: warnings, calls: calls, simulation: sim, badges: badges)
        callReview = review
        pendingAction = .wallet(command)
        pendingText = c.text
        reply(say("\(c.name) on \(review.contractName) (\(net.name)) is checked and simulated: \(review.changes.joined(separator: ", ")). The card is on the canvas — press Confirm.",
                  "\(c.name) на \(review.contractName) (\(net.name)) проверен и просимулирован: \(review.changes.joined(separator: ", ")). Карточка на холсте — нажми Confirm.")
              + (badges.isEmpty ? "" : " ⚑ " + badges.joined(separator: " · ")) + (warnings.isEmpty ? "" : " ⚠ " + warnings.joined(separator: " ")), note: note)
    }

    /// What the call did to the wallet's tokens, in its cost basis: the shares it got for the dollars it spent, so the
    /// portfolio shows them (and their P/L) before the explorer lists them; shares it gave back, sold.
    func recordCall(_ r: CallReview) async {
        var spent: Decimal = 0, got: [(key: String, amount: Decimal)] = []
        for (asset, delta) in r.simulation.changes where asset != EVMSimulation.native {
            let t = await tokenMeta(asset, r.net), amount = abs(delta) / pow(10, t.decimals), key = "\(r.net.key):\(asset.lowercased())"
            if delta < 0 {
                if Stablecoins.dollars.contains(t.symbol.uppercased()) { spent += amount } else { costBasis[key]?.sold(amount) }
            } else if !Stablecoins.dollars.contains(t.symbol.uppercased()) {
                got.append((key, amount))
            }
        }
        // One kind of share for the dollars: its cost. Several, or none paid in dollars: kept at no cost, so they still show.
        for g in got { costBasis[g.key, default: CostBasis()].bought(g.amount, for: got.count == 1 && spent > 0 ? spent : Decimal(string: "0.000001")!) }
    }

    /// Shares a contract call brought (kept in the cost basis) that the network's explorer hasn't listed yet: read from the
    /// token itself and priced by DefiLlama. Blockscout indexes a new holding minutes late.
    func missingDeposits(_ have: [Holding], owner: String) async -> [Holding] {
        var out: [Holding] = []
        for key in costBasis.keys {
            let parts = key.split(separator: ":").map(String.init)
            guard parts.count == 2, Holdings.blockscout[parts[0]] != nil, let net = EVMNetwork.named(parts[0]),
                  !have.contains(where: { $0.chain == net.key && $0.contract.lowercased() == parts[1] }),
                  let raw = try? await net.rpc.read(parts[1], ERC20.balanceOf(owner)), let units = ABI.decimal(in: raw, word: 0), units > 0 else { continue }
            let t = await tokenMeta(parts[1], net)
            out.append(Holding(chain: net.key, contract: parts[1], amount: units / pow(10, t.decimals), symbol: t.symbol))
        }
        var priced: [Holding] = []
        for chain in Set(out.map(\.chain)) { priced += await Holdings.llamaPriced(chain, out.filter { $0.chain == chain }) }
        return priced
    }

    /// After Confirm: the approve and the call exactly as simulated (the executor dry-runs the call again before signing it).
    func runContractCall(_ command: WalletCommand, _ e: Executor) async throws -> String {
        guard let r = callReview, r.command == command else { throw ExecutionError("The call's check is gone; ask for it again.") }
        callReview = nil
        // Simulated again now: what's signed must still do what the card showed (a proxy upgraded, a pool moved).
        guard let me = signerWallet?.evm else { throw ExecutionError("No EVM key in secure storage.") }
        let now = try await Self.simulator(r.net).simulate(from: me, calls: r.calls)
        if let why = ContractCallGuard.drift(card: r.simulation, now: now) ?? ContractCallGuard.grantProblem(now.grants, contract: r.call.contract) {
            throw GuardStop("Nothing was signed: since the card, \(why). Ask for it again to see the new card.")
        }
        let done = try await e.contractCall(r.net, to: r.call.contract, data: r.data, value: r.value, approve: r.approve.map { ($0.token, $0.units) },
                                            approveNote: "Approve exactly \(Fmt.num(r.approve?.amount ?? 0, maxFraction: 6)) \(r.approve?.symbol ?? "") to \(r.contractName)",
                                            note: "\(r.call.name) on \(r.contractName) on \(r.net.name)")
        await recordCall(r)
        return done
    }

    /// The card: the exact approve, the call with what the explorer says of the contract, and the simulated balances.
    func callRoute(_ command: WalletCommand) -> MoveRoute? {
        guard let r = callReview, r.command == command else { return nil }
        let logo = ModuleNetwork(place: r.net.key)?.logo, fill = RouteFill(rawValue: r.net.key) ?? .check
        var steps: [ActivityBubble] = []
        if let a = r.approve {
            steps.append(ActivityBubble(kind: "step", label: "APPROVE · EXACT", title: "\(Fmt.num(a.amount, maxFraction: 6)) \(a.symbol)",
                                        lines: [say("exactly this · only \(r.contractName) may take it", "ровно столько · взять может только \(r.contractName)")], logo: logo, fill: fill))
        }
        steps.append(ActivityBubble(kind: "step", label: "CALL · \(r.net.name.uppercased())", title: r.call.name, lines: r.lines + [r.trust.line], logo: logo, fill: fill))
        steps.append(ActivityBubble(kind: "step", label: "SIMULATED", title: r.changes[0], lines: Array(r.changes.dropFirst()), logo: logo, fill: .check))
        return MoveRoute(title: "\(r.call.name) · \(r.contractName)",
                         footer: "Encoded by ihatecrypto from \(r.call.function), simulated on \(r.net.name)"
                            + (r.approve == nil ? "" : " with its exact approve") + "; the signer reads it again and asks for a new spender in person. Press Confirm.",
                         steps: steps, warning: (r.badges.map { "⚑ " + $0 } + r.warnings.map { "⚠ " + $0 }).joined(separator: "   ").nilIfEmpty)
    }

    /// The node that runs eth_simulateV1 for a network: Arbitrum's own crashes on it (Browser.evmSimulationRPC).
    static func simulator(_ net: EVMNetwork) -> EVMRPC {
        Browser.evmSimulationRPC[net.rpc.chainId].flatMap(URL.init(string:)).map { EVMRPC(url: $0, chainId: net.rpc.chainId) } ?? net.rpc
    }

    nonisolated static func delta(_ d: Decimal) -> String { (d > 0 ? "+" : d < 0 ? "−" : "") + Fmt.num(abs(d), maxFraction: 8) }

    /// A token's symbol and decimals, read from it.
    func tokenMeta(_ token: String, _ net: EVMNetwork) async -> (symbol: String, decimals: Int) {
        let symbol = (try? await net.rpc.read(token, Data([0x95, 0xd8, 0x9b, 0x41]))).flatMap(ABI.string) ?? "\(token.prefix(6))…"
        let decimals = (try? await net.rpc.read(token, Data([0x31, 0x3c, 0xe5, 0x67]))).flatMap { ABI.uint(in: $0, word: 0) } ?? 18
        return (symbol, Int(decimals))
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
