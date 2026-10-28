import Foundation
import Synchronization
import WalletCore

/// What the user can say about a position that is already open («стоп в безубыток», «закрой половину»).
/// Deterministic like the plan parser; only checked while a position or order exists.
public enum PositionCommand: Equatable, Sendable {
    /// Share of the position to close, 0 < f ≤ 1.
    case close(Decimal)
    case stopToBreakeven
    case stopTo(Decimal)
    case takeTo(Decimal)
    case cancelOrders
    /// Keep the stop N% behind the best price; ihatecrypto's guardian moves it, only ever tighter.
    case trail(Decimal)

    public static func parse(_ text: String) -> PositionCommand? {
        let q = IntentParser.plainNumbers(text.lowercased())
        // «should I close ETH?», «закрыть?», «не закрывай позицию», «don't close it»: a question or a «don't» is answered,
        // never carried out (without a model a close runs at once).
        if IntentParser.isQuestion(text) || q.contains(/(?:^|[^\p{L}'’])(?:не|нельзя|don['’]?t|dont|do\s+not|never|not)(?![\p{L}'’])|should\s+(?:i|we)\b|стоит\s+ли|(?:а|что)\s+если/) {
            return nil
        }
        if q.contains(/отмени\w*\s+(?:лимит|ордер|заявк)|сними\w*\s+(?:лимит|ордер|заявк)|cancel\s+(?:the\s+|my\s+)?(?:limit|order)|remove\s+(?:the\s+)?(?:limit|order)/) { return .cancelOrders }
        if q.contains(/безубыт|стоп\w*\s+(?:в|на)\s+(?:ноль|бу\b|вход)|break\s*-?\s*even|stop\s+(?:to|at)\s+entry/) { return .stopToBreakeven }
        if let m = q.firstMatch(of: /(?:(?:перенес\w*|передвин\w*|подвин\w*|постав\w*)?\s*стоп\w*\s+(?:на|в)|(?:move|set|put)?\s*(?:the\s+)?stop(?:[- ]?loss)?\s+(?:to|at))\s+\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)/),
           let v = Decimal(string: m.1.replacingOccurrences(of: ",", with: ".")) {
            return .stopTo(v)
        }
        if let m = q.firstMatch(of: /(?:(?:перенес\w*|передвин\w*|подвин\w*|постав\w*)?\s*тейк\w*\s+(?:на|в)|(?:move|set|put)?\s*(?:the\s+)?take(?:[- ]?profit)?\s+(?:to|at))\s+\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)/),
           let v = Decimal(string: m.1.replacingOccurrences(of: ",", with: ".")) {
            return .takeTo(v)
        }
        // «trail 2%» about the open position; a new plan with a trailing stop («long ETH trailing 2%») is not this.
        if !q.contains(/лонг|шорт|\blong\b|\bshort\b/), let m = q.firstMatch(of: /(?:трейл\w*|trail\w*)[^\d]{0,15}?(\d+(?:[.,]\d+)?)\s*%/),
           let v = Decimal(string: m.1.replacingOccurrences(of: ",", with: ".")), v > 0, v < 50 {
            return .trail(v)
        }
        guard q.contains(/закр\w*|выйди|выход\w*\s+из|\bclose\b|\bexit\b/) else { return nil }
        if q.contains(/половин|50\s*%|\bhalf\b/) { return .close(Decimal(string: "0.5")!) }
        if q.contains(/треть|\bthird\b/) { return .close(Decimal(1) / 3) }
        if q.contains(/четверть|\bquarter\b/) { return .close(Decimal(string: "0.25")!) }
        if let m = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*%/), let v = Decimal(string: m.1.replacingOccurrences(of: ",", with: ".")), v > 0, v <= 100 {
            return .close(v / 100)
        }
        return .close(1)
    }
}

/// Coins to buy and hold on Solana, by fixed verified mint: a search by ticker returns look-alikes. With Safe search
/// off, any token by its mint too: parsed with its mint alone, then looked up (decimals, symbol, token program).
public struct SpotAsset: Equatable, Sendable {
    public let symbol: String
    public let mint: String
    public let decimals: Int
    /// A Token-2022 mint: its accounts derive from that program, not the classic one.
    public var token2022 = false

    public init(symbol: String, mint: String, decimals: Int, token2022: Bool = false) {
        self.symbol = symbol; self.mint = mint; self.decimals = decimals; self.token2022 = token2022
    }

    /// A token named by its mint only, until the app looks it up.
    public static func byMint(_ mint: String) -> SpotAsset { SpotAsset(symbol: "\(mint.prefix(4))…\(mint.suffix(4))", mint: mint, decimals: 0) }
    /// A token named by its ticker only («BONK»): the app finds the Jupiter-verified token trading under it.
    public static func bySymbol(_ symbol: String) -> SpotAsset {
        let s = symbol.trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        return all.first { $0.symbol.uppercased() == s.uppercased() } ?? SpotAsset(symbol: s.uppercased(), mint: "", decimals: 0)
    }
    public var resolved: Bool { decimals > 0 }
    /// One of the fixed verified coins (or USDC): the only ones bought while Safe search is on.
    public var verified: Bool { self == .usdc || Self.all.contains { $0.mint == mint } }

    /// Checked 2026-09-25 against Jupiter's verified list (isVerified true).
    public static let all = [
        SpotAsset(symbol: "SOL", mint: "So11111111111111111111111111111111111111112", decimals: 9),
        SpotAsset(symbol: "BTC", mint: "cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij", decimals: 8),   // Coinbase Wrapped BTC
        SpotAsset(symbol: "ETH", mint: "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs", decimals: 8),   // Ether (Portal)
        SpotAsset(symbol: "JUP", mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", decimals: 6),
        // Liquid staking: SOL that earns the staking yield and can be sold back any time.
        SpotAsset(symbol: "JitoSOL", mint: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn", decimals: 9),
    ]
    public static let usdc = SpotAsset(symbol: "USDC", mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", decimals: 6)
    public static let jitoSOL = all[4]

    static func find(_ q: String) -> SpotAsset? {
        if q.contains(/jitosol|джито/) { return all[4] }
        if q.contains(/\bsol\b|\bсол\b|\bсолу\b|солан|solana/) { return all[0] }
        if q.contains(/\bbtc\b|биткоин|битк|bitcoin/) { return all[1] }
        if q.contains(/\beth\b|эфир|ether/) { return all[2] }
        if q.contains(/\bjup\b|юпитер|джуп|jupiter/) { return all[3] }
        return nil
    }
}

/// Moving the user's own money: back to Solana, into the USDC vault and out, spot buys and sells.
public enum WalletCommand: Equatable, Sendable {
    /// Spend `usdc` on the coin, on Solana through Jupiter; nil = ask how much.
    case buy(SpotAsset, usdc: Decimal?)
    /// Native NEAR on the NEAR chain for Solana USDC through NEAR Intents 1Click, to the wallet's own NEAR account;
    /// nil = ask how much.
    case buyNear(usdc: Decimal?)
    /// Native NEAR from the NEAR account for USDC, through 1Click; nil = all it can give. `to`: the EVM network the USDC
    /// arrives on at our own address (Base, Arbitrum, Ethereum: what 1Click pays there), nil = the Solana address.
    case sellNear(amount: Decimal?, to: String? = nil)
    /// Sell this much of the coin for USDC; nil = all of it (SOL keeps a fee reserve).
    case sell(SpotAsset, amount: Decimal?)
    /// Regular purchases (DCA): `usdc` of the coin every `days` days.
    case schedule(SpotAsset, usdc: Decimal, days: Int)
    /// Jupiter's on-chain DCA: `usdc` in all (nil = ask) into the coin, in `orders` swaps (nil = one per 50 USDC, 2 to 10)
    /// `every` seconds apart. The whole amount goes into an order account of the wallet's own at once; Jupiter's keepers
    /// buy, and the coin comes to the wallet after each swap — the app needn't run.
    case jupiterDCA(SpotAsset, usdc: Decimal?, orders: Int?, every: Int)
    /// Running Jupiter DCA orders closed, the USDC they haven't spent back; nil = all, else those buying this coin.
    case jupiterDCACancel(SpotAsset?)
    /// The running Jupiter DCA orders, on the canvas.
    case jupiterDCAOrders
    case stopSchedules
    /// nil = everything withdrawable.
    case withdrawToSolana(Decimal?)
    /// Jupiter Lend: symbol («USDC», «SOL», «USDT»), amount in that asset; nil = ask how much.
    case lendDeposit(String, Decimal?)
    /// nil = the whole vault position.
    case lendWithdraw(String, Decimal?)
    /// Kamino Lend: the main market, and the Backpack market for Backpack's stocks (`kaminoStocks`), where a deposit is
    /// collateral to borrow against.
    case kamino(deposit: Bool, symbol: String, amount: Decimal?)
    /// USDC borrowed on Kamino — on the Backpack market against Backpack's tokenized stocks, or on the main market against
    /// deposits there — and paid back (nil = all of it). `against`: the stock the loan stands on («SPCX»); `adding`: that
    /// many of its tokens put up as collateral first (nil = what's there already).
    case kaminoBorrow(Decimal?, against: String? = nil, adding: Decimal? = nil)
    case kaminoRepay(Decimal?)
    /// SOL into JitoSOL (liquid staking) and back; nil = all.
    case stake(Decimal?)
    case unstake(Decimal?)
    /// Where the asset earns most now; nil = stablecoins and SOL.
    case yields(String?)
    /// Hyperliquid's HLP vault: deposit or withdraw USDC (nil = all).
    case hlp(deposit: Bool, amount: Decimal?)
    /// «Разложи USDC по лучшим ставкам»: the asset and the most one protocol may hold, % (nil = 50).
    /// `amount`: spread at most this much of it (nil = all that is free).
    /// `without`: protocols left out, as asked («без kamino»): keys of `spreadProtocols`.
    /// `from`: only this network's free money («с Base»: "base", or "solana"); nil = free money anywhere.
    case allocate(String, maxShare: Decimal?, amount: Decimal? = nil, without: [String] = [], from: String? = nil)
    /// «Переложи в лучшие ставки»: USDC already deposited moves to better rates, at most this % per protocol (nil = 50),
    /// only where the extra interest pays the move back within 30 days.
    case rebalance(maxShare: Decimal?)
    /// USDC into Aave v3 on `network` («base», «arbitrum», «polygon», «ethereum»; bridged from Solana when short) or
    /// out of it and back to Solana; nil = ask (deposit) / everything (withdraw).
    case aave(deposit: Bool, amount: Decimal?, network: String = "base")
    /// USDC borrowed on Aave against what the wallet supplied there, paid back (nil = all of it), and ETH supplied there
    /// as collateral; the network as for `aave`.
    case aaveBorrow(Decimal?, network: String = "base")
    case aaveRepay(Decimal?, network: String = "base")
    case aaveCollateral(Decimal?, network: String = "base")
    /// The same with a Morpho vault («base», «arbitrum», «ethereum»).
    case morpho(deposit: Bool, amount: Decimal?, network: String = "base")
    /// USDC idle on Base back to Solana over Relay; nil = everything.
    case baseToSolana(Decimal?)
    /// The same from Arbitrum.
    case arbitrumToSolana(Decimal?)
    /// The same from «polygon» or «ethereum».
    case evmToSolana(String, Decimal?)
    /// «send 5 USDC to Bybit»: to an entry of the address book, by its name. An address in the text is never a recipient.
    /// `network`: the one the message names («… on ethereum», «… по эфир сети»); nil = where the entry is saved.
    case sendTo(name: String, amount: Decimal, network: String? = nil)
    /// Native NEAR from the wallet's NEAR account to an address-book entry on NEAR; nil = all but the account's own
    /// storage and the fee.
    case sendNear(name: String, amount: Decimal?)
    /// Deletes the wallet's NEAR account; NEAR sends everything on it to the address-book entry `name`.
    case closeNear(name: String)
    /// Native SOL to an address-book entry on Solana; the amount is always said.
    case sendSol(name: String, amount: Decimal)
    /// «send all RAY on solana to Bybit», «send 1973 AI on robinhood to New wallet»: any token the wallet holds — by
    /// ticker, contract or mint — to an address-book entry, as it is, on the same network: no swap, no bridge. Stock
    /// tokens, Morpho's, Aave's and Jupiter Lend's shares, a network's own coin (ETH, POL: with a number). nil amount =
    /// all of it; `network` nil = where the entry is saved.
    case sendToken(name: String, token: String, amount: Decimal?, network: String? = nil)
    /// «задепозить», «пополнить», «my address»: money coming in from outside — show where to send it.
    case receive
    /// A Robinhood Stock Token (SPCX, TSLA…) bought on Robinhood Chain for USDC from Solana; `symbol` as the user named
    /// it, resolved against Robinhood's registry by the app. nil amount = ask.
    case buyStock(symbol: String, usdc: Decimal?)
    /// A token on an EVM network, by ticker (Uniswap's list) or contract; nil usdc = ask how much. `from`: nil = where
    /// the money is (that network's own USDC when it holds enough — Uniswap there, no bridge — else Solana: Relay's swap
    /// or Relay's bridge + Uniswap, whichever leaves more); «solana»; that network itself; or another EVM network, whose
    /// USDC comes over first.
    case buyOn(String, token: String, usdc: Decimal?, from: String? = nil)
    /// Such a token back to USDC on Solana, the better way again; nil = all of it. `keep`: the USDC stays on that network.
    case sellOn(String, token: String, amount: Decimal?, keep: Bool = false)
    /// One token for another on that network, straight on Uniswap (v2, v3 or v4, the best), no USDC in the middle; either
    /// side may be the network's own coin (ETH, POL). nil amount = all of `from`.
    case swapOn(String, from: String, to: String, amount: Decimal?)
    /// A token on one network for a token on another through Relay, one signature: «swap 0.1 SOL on Solana for ETH on
    /// Arbitrum». Places are «solana» or an EVM network's key; nil amount = all of a token.
    case crossSwap(from: String, on: String, to: String, at: String, amount: Decimal?)
    /// Money coming in from a network ihatecrypto has no wallet on — «receive 50 USDC from Sui», «получи 0.01 BTC с
    /// биткоина»: 1Click's deposit address there, what arrives swapped into USDC on Solana. `network` is the address
    /// book's («sui», «bitcoin», «tron», «xrp», «dogecoin», «zcash»); nil amount = ask.
    case receiveAcross(network: String, token: String, amount: Decimal?)
    /// Sell a Stock Token for USDG on Robinhood Chain; nil = all of it.
    case sellStock(symbol: String, amount: Decimal?)
    /// USDG on Robinhood Chain back to Solana as USDC; nil = everything.
    case robinhoodToSolana(Decimal?)
    /// USDC from the Solana wallet to our own address on «base», «arbitrum» or «robinhood» (USDG there); nil = ask.
    case solanaTo(String, Decimal?)
    /// USDC from one EVM network straight to another over Relay, at our own address («base» → «robinhood», where it
    /// arrives as USDG), or to our Hyperliquid trading balance («hyperliquid»); nil = everything on `from`.
    case bridge(from: String, to: String, Decimal?)
    /// The same move through Circle's CCTP, asked for by name («… via CCTP»): Ethereum, Arbitrum, Base, Polygon.
    case cctp(from: String, to: String, Decimal?)
    /// USDT between two of USDT0's networks (Stablecoins.usdt0Networks) at our own address: Relay's swap of it, seconds —
    /// or, asked for by name («… via LayerZero»), Tether's own bridge: the same amount on the other side for cents of
    /// gas, in minutes to hours. nil = all the USDT on `from`.
    case usdt(from: String, to: String, Decimal?, layerZero: Bool)
    /// The network's own coin for fees («robinhood», «base», …), `usd` worth of it over Relay; nil = $1.
    case gas(String, usd: Decimal?)
    /// A paid API on Tempo (MPP): the request as the service documents it; the price comes from its 402 answer and
    /// is paid in USDC on Tempo only after Confirm. `method` «GET» or «POST»; `body` JSON or nil.
    case pay(method: String, url: String, body: String?)
    /// «claim rewards», «забери награды»: what Merkl owes this wallet (Morpho's, Aave's and other protocols' rewards)
    /// on Base, Arbitrum, Ethereum and Polygon, paid to it.
    case claimRewards
    /// «sweep rewards over $10 into USDC», «продавай награды в стейблы»: Merkl's rewards, when together they're worth
    /// `minUSD` or more (never under $5, or the gas eats them), claimed, and each sold for USDC on Solana, or `into` a token
    /// bought with that USDC. Under the floor nothing is signed: a schedule of it just waits.
    case sweepRewards(minUSD: Decimal?, into: String?)
    /// «sweep dust», «собери пыль»: the verified tokens worth under `under` dollars (nil = $5) on Base, Arbitrum and
    /// Polygon, each sold for USDC on its own network when the gas takes less than half of it.
    case sweepDust(under: Decimal?)
    /// «show approvals», «мои разрешения»: who may still spend which token, on every EVM network — a board, nothing signed.
    /// A token on Hyperliquid's spot book (HYPE…) for USDC on HyperCore's spot balance; nil = ask how much.
    case hlSpotBuy(symbol: String, usdc: Decimal?)
    /// Sells it there for USDC; nil = all that's free.
    case hlSpotSell(symbol: String, amount: Decimal?)
    case showApprovals
    /// Sets one approval to zero: the token's own approve(spender, 0), or Permit2's (`permit2`).
    case revokeApproval(String, token: String, spender: String, permit2: Bool)
    /// Any contract's function the chat wrote out (ContractCall): checked on the explorer, simulated, then a card.
    case contractCall(ContractCall)
    /// «make a multisig 2 of 3 with Phone on base and arbitrum»: a Safe multisig whose keys are this Mac's (and the connected
    /// Ledger's) plus the saved addresses named, `threshold` of them to move money (nil: a majority); nil `of` when not said,
    /// no network = Base. Its card adjusts all of it; Confirm makes it.
    case multisigCreate(threshold: Int?, of: Int?, keys: [String], networks: [String])
    /// «send 200 USDC from the multisig to Alex on base»: a payment out of it, signed by its keys here; nil = all of it.
    case multisigSend(token: String, amount: Decimal?, to: String, network: String?)
    /// «show the multisig»: its card on the canvas.
    case multisigShow

    /// Reading a text compiles up to ~150 regexes; the Schedule reads its tasks' steps on every redraw of the window,
    /// and with six tasks that was a third of the main thread (2026-10-16). The answer depends on the text alone: kept.
    public static func parse(_ text: String) -> WalletCommand? {
        if let known = parsed.withLock({ $0[text] }) { return known }
        let command = read(text)
        parsed.withLock { if $0.count >= 1000 { $0.removeAll() }; $0[text] = command }
        return command
    }

    private static let parsed = Mutex<[String: WalletCommand?]>([:])

    private static func read(_ text: String) -> WalletCommand? {
        if let call = ContractCall.parse(text) { return .contractCall(call) }
        if let multisig = multisig(text) { return multisig }
        // «pay POST https://… {"q": 1}»: the URL and the body kept as written.
        if let m = text.firstMatch(of: /(?is)^\s*pay\s+(?:(get|post|put|patch|delete)\s+)?(https:\/\/[^\s{}\[\]]+)(?:\s+(?:with\s+)?([\{\[].*[\}\]]))?\s*$/) {
            let body = m.3.map(String.init)
            return .pay(method: m.1.map { $0.uppercased() } ?? (body == nil ? "GET" : "POST"), url: String(m.2), body: body)
        }
        let text = IntentParser.plainNumbers(text)
        let q = text.lowercased()
        // Jupiter's on-chain DCA, before the app's own recurring buys below take «dca».
        if q.contains(Self.jupiterDCAWords) { return jupiterDCA(q) }
        // «add gas on robinhood for 3 USDC», «добавь газа на робингуд на 3 бакса», «докинь газ в полигон».
        if q.contains(/\bgas\b|\bгаз\w*/), q.contains(/\badd\b|top\s*up|\bbring\b|\bget\b|\bbuy\b|добав\w*|доки\w*|пополн\w*|закин\w*|подкин\w*|купи\w*|нуж\w*/),
           let word = q.firstMatch(of: /\b(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet)\b/),
           let net = Self.network(String(word.1)) {
            let usd = (q.firstMatch(of: /\$\s*(\d+(?:[.,]\d+)?)/)?.1 ?? q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)/)?.1)
                .flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
            return .gas(net, usd: usd)
        }
        // One approval to zero: «revoke the approval of 0x… to 0x… on base [through permit2]», «отзови разрешение 0x… для 0x… на base».
        if let m = text.firstMatch(of: /(?i)\b(?:revoke|cancel|отзов\w*|отмен\w*)\s+(?:the\s+)?(?:approval|разрешени\w*)\s+(?:of|на|для)?\s*(0x[0-9a-fA-F]{40})\s+(?:to|для|у)\s+(0x[0-9a-fA-F]{40})\s+(?:on|на|в)\s+(base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|robinhood|робинхуд\w*)\b/) {
            let permit2 = text.range(of: "permit2", options: .caseInsensitive) != nil
            return .revokeApproval(Self.network(String(m.3)) ?? "base", token: String(m.1).lowercased(), spender: String(m.2).lowercased(), permit2: permit2)
        }
        // The approvals board: «show approvals», «мои разрешения», «кому я дал разрешения», «revoke approvals».
        if q.contains(/\b(?:show\s+|my\s+|list\s+|check\s+|revoke\s+)?(?:token\s+)?approvals\b|\ballowances\b|(?:мои|покажи|проверь|какие)\s+(?:\w+\s+)?разрешени\w*|кому\s+(?:я\s+)?(?:дал|выдал)\w*\s+разрешени|апрув\w*/) {
            return .showApprovals
        }
        // Small leftovers into USDC: «sweep dust», «собери пыль до $3», «convert small balances to USDC».
        if q.contains(/\bdust\b|\bпыль\b|\bпыли\b|собер\w*\s+мелоч\w*|\bsmall\s+balances?\s+(?:in)?to\s+usdc|\bleftovers?\s+(?:in)?to\s+usdc|остатк\w*\s+(?:в|на)\s+usdc/) {
            let under = q.firstMatch(of: /(?:under|below|less\s+than|<|до|меньше|ниже)\s*\$?\s*(\d+(?:[.,]\d+)?)/)
                .flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            return .sweepDust(under: under)
        }
        // Rewards to claim AND sell: before the plain claim below.
        if q.contains(/\bsweep\s+(?:the\s+|my\s+)?rewards?|\bsell\s+(?:the\s+|my\s+|all\s+)?(?:claimed\s+)?rewards?|(?:прода|обменя|меня)\w*\s+(?:\w+\s+){0,2}?награ/) {
            let min = q.firstMatch(of: /(?:over|above|at\s+least|≥|>=|от|больше|свыше|когда\s+(?:наберётся|наберется|будет))\s*\$?\s*(\d+(?:[.,]\d+)?)/)
                .flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            let into = q.firstMatch(of: /\b(?:into|to|for)\s+\$?([a-z][a-z0-9]{1,9})\b|(?:\sв|\sна)\s+\$?([a-z][a-z0-9]{1,9})\b/).map { String($0.1 ?? $0.2 ?? "").uppercased() }
            return .sweepRewards(minUSD: min, into: into.flatMap { $0.isEmpty || ["USDC", "USD", "STABLES", "STABLECOINS"].contains($0) ? nil : $0 })
        }
        // Rewards to collect, before «claim … from Aave» reads as a withdrawal and «claim yield» as the rates.
        if q.contains(/\bclaim|клейм|ревард|реворд/) || q.contains(/(?:забер|забра|собер|собра|получ|вывед|выве)\w*\s+(?:\w+\s+){0,2}?(?:награ|бонус)/) {
            return .claimRewards
        }
        let amount = (q.firstMatch(of: /\$\s*(\d+(?:[.,]\d+)?)/)?.1 ?? q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)/)?.1)
            .flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
        // Hyperliquid's spot book: «buy HYPE on hyperliquid spot for 10 USDC», «sell all HYPE spot», «купи HYPE на споте за 15$».
        if q.contains(/\bspot\b|\bспот\w*/), !q.contains(/\b(?:long|short|perp\w*|лонг\w*|шорт\w*)\b/) {
            if let m = q.firstMatch(of: /(?:\bbuy|купи\w*)\s+\$?([a-z][a-z0-9]{1,9})\b/), !["spot", "on", "for"].contains(String(m.1)) {
                return .hlSpotBuy(symbol: String(m.1).uppercased(), usdc: amount)
            }
            if let m = q.firstMatch(of: /(?:\bsell|прода\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь)\s+)?\$?([a-z][a-z0-9]{1,9})\b/), !["spot", "on"].contains(String(m.2)) {
                let count = m.1.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
                if count == nil, amount != nil { return nil }   // a sale in dollars names no count of coins
                return .hlSpotSell(symbol: String(m.2).uppercased(), amount: count)
            }
        }
        // «положи 1 в джупитер»: a bare number is the amount of a deposit or a withdrawal when nothing else is
        // (not for buys: «buy 0.1 SOL» is coins, not dollars).
        let bare = q.firstMatch(of: /(?:^|[^\w.,])(\d+(?:[.,]\d+)?)(?![\d.,]*\s*(?:%|x\b|х\b|×|r\b))/)
            .flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
        let jupiter = q.contains(/jupiter|jupyter|джупитер|юпитер|\bjup\s*(?:lend|vault|earn)|jlusdc/)
        let vault = jupiter || q.contains(/хранилищ|вол[тл]|vault|\blend|депозит|лендинг/)
        let into = q.contains(Self.depositWords) || q.contains(Self.intoPlace)
        let outOf = q.contains(Self.withdrawWords) || q.contains(Self.outOfPlace)
        let coins = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:sol|сол|jitosol|usdt|jupusd|usdc)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
        // «USDC на Солане» is USDC; Solana alone is the network, SOL the coin.
        let symbol = q.contains(/usdc|юсдс|юсдц/) ? "USDC" : q.contains(/\bsol\b|\bсол\b/) ? "SOL" : q.contains(/usdt/) ? "USDT" : q.contains(/jupusd/) ? "JupUSD" : "USDC"
        // Money already deposited, moved to better rates: «переложи в лучшие ставки», «rebalance my USDC».
        if q.contains(/перебаланс|ребаланс|rebalanc/) || (q.contains(/перелож|move\s+my\s+deposits/) && q.contains(/ставк|доходн|лучш|rates?|yields?|best/)) {
            let share = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*%/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            return .rebalance(maxShare: share)
        }
        if q.contains(/разлож|распредел|раскид|размест|spread|allocate|distribute|split/), q.contains(/ставк|доходн|протокол|лучш|хранилищ|rates?|yields?|protocols?|best|vaults?/) {
            let share = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*%/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            // «spread 5 USDC from base», «разложи 1000 с базы»: only that network's money.
            let source = q.firstMatch(of: /(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(solana|солан\w*|base|бейс\w*|бейз\w*|баз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робинхуд\w*|tempo|темпо\w*)/)
            let from = source.map { m -> String in
                let w = String(m.1)
                return w.hasPrefix("sol") || w.hasPrefix("сол") ? "solana" : w.hasPrefix("баз") ? "base" : Self.network(w) ?? w
            }
            // «from solana» names the network, not the coin; «USDC» named wins.
            let rest = source.map { q.replacingCharacters(in: $0.range, with: " ") } ?? q
            let isSOL = !rest.contains(/usdc|юсдс|юсдц/) && rest.contains(/\bsol\b|солан|\bсол\b/)
            // «… without kamino, aave», «без камино»: the protocols named after the word are left out.
            let tail = q.firstMatch(of: /(?:\bwithout\b|\bexcept\b|\bбез\b|\bкроме\b)(.*)/).map { String($0.1) } ?? ""
            let without = Self.spreadProtocols.filter { p in (try? Regex(p.words)).map { tail.contains($0) } ?? false }.map(\.key)
            return .allocate(isSOL ? "SOL" : "USDC", maxShare: share, amount: isSOL ? coins : amount ?? coins, without: without, from: isSOL ? nil : from)
        }
        if q.contains(/доходност|где\s+(?:лучше\s+)?держать|куда\s+(?:лучше\s+)?(?:положить|вложить|деть)|\bapy\b|ставк[иа]\s+(?:по\s+)?депозит|where\s+(?:to|should\s+i|can\s+i)\s+(?:keep|put|park|hold)|best\s+(?:rates?|yields?)|\byields?\b/) {
            let sol = q.contains(/\bsol\b|солан|\bсол\b|solana/), usd = q.contains(/usdc|стейбл|доллар|stable/)
            return .yields(sol && usd ? nil : sol ? "SOL" : usd ? "USDC" : q.contains(/usdt/) ? "USDT" : nil)
        }
        // A token on one network for one on another, through Relay: «swap 0.1 SOL on Solana for ETH on Arbitrum», «swap all
        // AERO on base for SOL on solana», «обменяй 0.1 SOL на ETH в арбитруме» (SOL comes from Solana). USDC for USDC is
        // a bridge; a swap on one network is the rule below.
        if let m = text.firstMatch(of: /(?i)\b(?:swap|exchange|convert|поменя\w*|обменя\w*|свапн\w*|свопн\w*|конверт\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь|всю)\s+)?\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:(?:on|from|in|на|в|во|с|со)\s+(solana|солан\w*|near|неар\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)(?:\s+chain)?\s+)?(?:for|to|into|на|в)\s+\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:on|in|at|на|в|во)\s+(solana|солан\w*|near|неар\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b/) {
            func place(_ w: Substring) -> String {
                let w = w.lowercased()
                return w.hasPrefix("sol") || w.hasPrefix("сол") ? "solana" : w.hasPrefix("near") || w.hasPrefix("неар") ? "near" : Self.network(w) ?? "base"
            }
            let a = String(m.2), b = String(m.4), to = place(m.5)
            if let from = m.3.map(place) ?? (a.uppercased() == "SOL" ? "solana" : a.uppercased() == "NEAR" ? "near" : nil), from != to {
                let count = m.1.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
                if a.uppercased() == "USDC", b.uppercased() == "USDC" { return from == "solana" ? .solanaTo(to, count) : .bridge(from: from, to: to, count) }
                return .crossSwap(from: a, on: from, to: b, at: to, amount: count)
            }
        }
        // One token for another on an EVM network: «swap 0.01 ETH for AERO on base», «swap all DEGEN to AERO on base»,
        // «поменяй 0.01 ETH на AERO на бейсе». USDC on one side is a buy or a sale there, with the money on that network.
        if let m = text.firstMatch(of: /(?i)\b(?:swap|exchange|convert|поменя\w*|обменя\w*|свапн\w*|свопн\w*|конверт\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь|всю)\s+)?\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:for|to|into|на|в)\s+\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:on|на|в|во)\s+(?:the\s+)?(solana|солан\w*|near|неар\w*|robinhood|робин[гх]уд\w*|hyperliquid(?:\s+spot)?|хайперликвид\w*(?:\s+спот\w*)?|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|tempo|темпо\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b/) {
            let from = String(m.2), to = String(m.3), word = String(m.4).lowercased()
            let place = word.hasPrefix("sol") || word.hasPrefix("сол") ? "solana" : word.hasPrefix("near") || word.hasPrefix("неар") ? "near"
                : Self.network(word) ?? "base"
            let count = m.1.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
            // NEAR's account holds NEAR alone: for USDC or back it's 1Click's sale or buy.
            if place == "near", from.uppercased() == "NEAR", to.uppercased() == "USDC" { return .sellNear(amount: count) }
            if place == "near", from.uppercased() == "USDC", to.uppercased() == "NEAR" { return .buyNear(usdc: count) }
            // Anything else on NEAR, one token for another there: NEAR Intents.
            if place == "near" { return .swapOn(place, from: from.uppercased(), to: to.uppercased(), amount: count) }
            // Solana, through Jupiter: USDC on a side is a buy or a sale there; any other two tokens straight, by ticker or mint.
            if place == "solana" {
                func asset(_ t: String) -> SpotAsset { t.count >= 32 ? .byMint(t) : .bySymbol(t) }
                if from.uppercased() == "USDC" { return .buy(asset(to), usdc: count) }
                if to.uppercased() == "USDC" { return .sell(asset(from), amount: count) }
                return .swapOn(place, from: from.count >= 32 ? from : from.uppercased(), to: to.count >= 32 ? to : to.uppercased(), amount: count)
            }
            // Hyperliquid's spot book trades everything against USDC: one token for another is a sale and a buy.
            if place == "hyperliquid" {
                if from.uppercased() == "USDC" { return .hlSpotBuy(symbol: to.uppercased(), usdc: count) }
                if to.uppercased() == "USDC" { return .hlSpotSell(symbol: from.uppercased(), amount: count) }
                return .swapOn(place, from: from.uppercased(), to: to.uppercased(), amount: count)
            }
            // Robinhood Chain: Stock Tokens and tokens for one another, USDG too, straight on Uniswap there.
            if place == "robinhood" { return .swapOn(place, from: from, to: to, amount: count) }
            // Tempo's dollars for one another on its own exchange: USDC on a side is a swap there too.
            if place == "tempo" { return .swapOn(place, from: from, to: to, amount: count) }
            // HyperEVM's money isn't in Uniswap's pools: a swap there is Relay's own on the network (Executor.pairSwap),
            // with USDC on a side too, never a buy that would bring the USDC from Solana.
            if place == "hyperevm" { return .swapOn(place, from: from, to: to, amount: count) }
            // The network's own coin (ETH, POL) for its USDC or back stays a swap: Uniswap's ETH pools take it as it is,
            // while a buy or a sale looks the token up on Uniswap's list, where ETH isn't.
            let coin = Self.ownCoin(place)
            if from.lowercased() == "usdc", !coin.contains(to.uppercased()) { return .buyOn(place, token: to, usdc: count, from: place) }
            if to.lowercased() == "usdc", !coin.contains(from.uppercased()) { return .sellOn(place, token: from, amount: count, keep: true) }
            return .swapOn(place, from: from, to: to, amount: count)
        }
        // A token on a named network: «buy AERO on base for 5 USDC», «sell all DEGEN on base», «buy BONK on solana for 5
        // USDC», «купи ARB на арбитруме на 5$», «buy token 0x… on arbitrum for 3 USDC».
        if let m = text.firstMatch(of: /(?i)\b(buy|купи\w*|sell|прода\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь)\s+)?(?:(?:the\s+)?tokens?\s+|токен\w*\s+)?\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11})\s+(?:on|на|в)\s+(solana|солан\w*|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b/),
           !["usdc", "usdg", "tokens", "token"].contains(String(m.3).lowercased()) {
            let token = String(m.3), place = String(m.4).lowercased()
            let chain = place.hasPrefix("sol") || place.hasPrefix("сол") ? "solana" : Self.network(place) ?? "base"
            let count = m.2.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
            let sell = String(m.1).lowercased().hasPrefix("s") || String(m.1).lowercased().hasPrefix("прод")
            // «sell BONK on solana for 10 USDC» names no count of coins: never «all of it».
            if sell, m.2 == nil, amount != nil { return nil }
            if chain == "solana", !token.hasPrefix("0x") {
                return sell ? .sell(.bySymbol(token), amount: count) : .buy(.bySymbol(token), usdc: amount)
            }
            // «buy ETH on arbitrum for 10 USDC», «sell 0.01 ETH on base»: the network's own coin for USDC is the swap above.
            // With a number only: the swap's «no amount» is all of what it spends, and the gas coin needs one.
            if chain != "solana", Self.ownCoin(chain).contains(token.uppercased()), let n = sell ? count : amount {
                return sell ? .swapOn(chain, from: token.uppercased(), to: "USDC", amount: n) : .swapOn(chain, from: "USDC", to: token.uppercased(), amount: n)
            }
            if chain != "solana" {
                // «… from solana», «… с арбитрума»; «… and keep the USDC there», «… оставь USDC на base».
                let rest = String(text[m.range.upperBound...])
                let from = rest.firstMatch(of: /(?i)\b(?:from|с|со|из)\s+(solana|солан\w*|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|хайперевм\w*|гиперевм\w*)/)
                    .map { String($0.1).lowercased() }.map { $0.hasPrefix("sol") || $0.hasPrefix("сол") ? "solana" : Self.network($0) ?? $0 }
                let keep = rest.contains(/(?i)\bkeep\b|\bstays?\b|остав\w*|остан\w*|не\s+выводи/)
                return sell ? .sellOn(chain, token: token, amount: count, keep: keep) : .buyOn(chain, token: token, usdc: amount, from: from)
            }
        }
        // Any token by its contract («купи 0x… токен на $2»): bought on Robinhood Chain, where ihatecrypto trades by contract.
        if let contract = text.firstMatch(of: /0x[0-9a-fA-F]{40}(?![0-9a-fA-F])/).map({ String($0.0) }) {
            if q.contains(/\bsell\b|прода\w*|\bdump\b/) {
                let tokens = q.firstMatch(of: /(?:^|\s)(\d+(?:[.,]\d+)?)\s*(?:tokens?|токен\w*|шт)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
                // A sale in dollars has no count of tokens to sell: not «all of it».
                if tokens == nil, amount != nil { return nil }
                return .sellStock(symbol: contract, amount: tokens)
            }
            if q.contains(/\bbuy\b|куп\w*|purchase|приобрет\w*|\bget\s+me\b|вложи\w*\s+в/) { return .buyStock(symbol: contract, usdc: amount) }
        }
        // Any Solana token by its mint («buy token B5AN… for 2 USDC»): through Jupiter, only with Safe search off.
        if let mint = AddressBookCommand.address(in: text), !mint.hasPrefix("0x"), !mint.hasSuffix(".near"), mint.count >= 32,
           SpotAsset.all.allSatisfy({ $0.mint != mint }) {
            if q.contains(/\bsell\b|прода\w*|\bdump\b/) {
                let tokens = q.firstMatch(of: /(?:^|\s)(\d+(?:[.,]\d+)?)\s*(?:tokens?|токен\w*|шт)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
                if tokens == nil, amount != nil { return nil }
                return .sell(.byMint(mint), amount: tokens)
            }
            if q.contains(/\bbuy\b|куп\w*|purchase|приобрет\w*|\bget\s+me\b|вложи\w*\s+в/) { return .buy(.byMint(mint), usdc: amount) }
        }
        // Robinhood Stock Tokens: a stock word or a company ihatecrypto knows, with buy or sell. A loan against stocks
        // isn't one: «займи под акции SpaceX, не продавая» was read as their sale.
        if let symbol = StockNames.symbol(in: text), !q.contains(Self.borrowWords), q.contains(/\bstocks?\b|\bshares?\b|акци\w*|токен\w*|robinhood|робинхуд|спейс|spacex|\btesla\b|тесл|нвид|nvidia/) {
            if q.contains(/\bsell\b|прода\w*|\bdump\b/) {
                // «sell 2 TSLA stock for 10 USDC» sells 2; «sell TSLA stock for 10 USDC» names no count: not «all of it».
                let shares = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:shares?|акци\w*|tokens?|токен\w*|(?!usd|bucks)[a-z]{2,5}\b)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
                if shares == nil, amount != nil { return nil }
                return .sellStock(symbol: symbol, amount: shares)
            }
            if q.contains(/\bbuy\b|куп\w*|purchase|приобрет\w*|\bget\s+me\b|вложи\w*\s+в/) { return .buyStock(symbol: symbol, usdc: amount) }
        }
        // Money leaving Solana («move 5 USDC to Base from Solana»): never a move back to it, whatever network is named.
        let fromSolana = q.contains(/(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(?:solana|солан)/)
        // Out of the Solana wallet: «move 3 USDC from Solana to Robinhood», «переведи 3 USDC из соланы в робингуд», «move 11
        // USDC from Solana to Hyperliquid» (the trading account, which the first deposit opens).
        // «из соланы в aave на арбитрум» is a deposit into Aave there, not an idle transfer: it's read further down.
        if !q.contains(/\baave\b|ааве|morpho|морфо/),
           let m = q.firstMatch(of: /(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(?:solana|солан\w*)(?:[^.]|\.(?=\d))*?(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|hyperliquid|хайперликвид\w*|гиперликвид\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*)/) {
            return .solanaTo(Self.network(String(m.1)) ?? "robinhood", amount)
        }
        // Between two EVM networks, or from one to Hyperliquid, straight over Relay: «move 20 USDC from Base to Robinhood»,
        // «из бейза в робингуд», «с бейза на гипер».
        if !q.contains(/\baave\b|ааве|morpho|морфо/),
           let m = q.firstMatch(of: /(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*)(?:[^.]|\.(?=\d))*?(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*|hyperliquid|гипер\w*|хайпер\w*|hl\b)/),
           let from = Self.network(String(m.1)), let to = Self.network(String(m.2)), from != to {
            // USDT, not USDC: Tether's own bridge when LayerZero is named, else Relay's swap of it — with that bridge's
            // price beside it on the card where it runs. Optimism's «USDT» is the old bridged token, not USDT0
            // (Stablecoins): a plain move from there swaps the one they hold.
            if q.contains(/\busdt0?\b|usd₮|\btether\b|юсдт|тезер/), !q.contains(/usdc|юсдс|юсдц/), to != "hyperliquid" {
                let layerZero = q.contains(/layer\s*zero|л[еэ]й?ер\s*зеро|(?:via|through|over|by|через|по)\s+(?:the\s+)?usdt0|\boft\b/)
                if layerZero || (Stablecoins.usdt0Networks.contains(from) && Stablecoins.usdt0Networks.contains(to) && from != "optimism") {
                    return .usdt(from: from, to: to, coins, layerZero: layerZero)
                }
                return .crossSwap(from: "USDT", on: from, to: "USDT", at: to, amount: coins)
            }
            // «… via CCTP», «… через Circle»: Circle's burn and mint, whatever Relay would land.
            if q.contains(/\bcctp\b|\bcircle\b|циркл|сиркл/), !["hyperliquid", "robinhood", "tempo"].contains(from), !["hyperliquid", "robinhood", "tempo"].contains(to) {
                return .cctp(from: from, to: to, amount)
            }
            return .bridge(from: from, to: to, amount)
        }
        if !fromSolana, q.contains(/robinhood|робин[гх]уд/), q.contains(/(?:выв[её]д\w*|верн\w*|перевед\w*|перенес\w*|withdraw\w*|move|send|bring|bridge)(?:[^.]|\.(?=\d))*(?:solana|солан)/) {
            return .robinhoodToSolana(amount)
        }
        // From a network ihatecrypto has no wallet on, through 1Click: «receive 50 USDC from Sui», «получи 0.01 BTC с
        // биткоина», «top up 20 USDT from tron». The coin left unsaid is the network's own (USDT on TRON).
        if let m = text.firstMatch(of: /(?i)\b(?:receive|get|bring|deposit|top\s*up|получи\w*|прими\w*|пополн\w*|заведи|завести)\b(?:\s+(?:me|мне))?(?:\s+(\d+(?:[.,]\d+)?))?(?:\s+(btc|zec|trx|usdt|xrp|doge|sui|usdc))?\s+(?:from|с|со|из)\s+(?:the\s+)?(sui|суи|bitcoin|биткоин\w*|btc|zcash|zec|tron|трон\w*|xrp|ripple|рипл\w*|dogecoin|doge|додж\w*)\b/) {
            let w = String(m.3).lowercased()
            let network = w.hasPrefix("su") || w.hasPrefix("су") ? "sui" : w.hasPrefix("bit") || w.hasPrefix("бит") || w == "btc" ? "bitcoin"
                : w.hasPrefix("z") ? "zcash" : w.hasPrefix("tr") || w.hasPrefix("тр") ? "tron" : w.hasPrefix("do") || w.hasPrefix("до") ? "dogecoin" : "xrp"
            let token = m.2.map { String($0).uppercased() } ?? ["sui": "SUI", "bitcoin": "BTC", "zcash": "ZEC", "tron": "USDT", "xrp": "XRP", "dogecoin": "DOGE"][network]!
            return .receiveAcross(network: network, token: token, amount: m.1.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) })
        }
        // Topping ihatecrypto up from outside: a deposit with no place to put it, or asking for our address.
        let place = q.contains(/хранилищ|vault|jupiter|jupyter|джупитер|юпитер|kamino|камино|aave|ааве|morpho|морфо|hlp|хлп|\blend|лендинг|стейк|stake/)
        if !place, q.contains(/^\s*(?:(?:хочу|want\s+to|i\s+want\s+to|let'?s|давай)\s+)?(?:задепозит\w*|депозит\w*|пополн\w*|deposit|top\s*up|fund|receive|закинуть\s+деньги)\b|(?:мо[йи]|наш\w*|our|my|ihatecrypto'?s?)\s+(?:\w+\s+)?(?:адрес\w*|address(?:es)?)|deposit\s+address|адрес\w*\s+(?:для\s+)?(?:пополнен|депозит)|куда\s+(?:мне\s+)?(?:от)?(?:прав|слать|кин|закин|переве)|where\s+(?:do\s+i|to|can\s+i|should\s+i)\s+(?:send|deposit|transfer)|how\s+(?:do\s+i|to|can\s+i)\s+(?:top\s+up|deposit|fund|add\s+money)|как\s+(?:мне\s+)?пополн/) {
            return .receive
        }
        if q.contains(/анстейк|unstake|из\s+стейкинга|продай\w*\s+(?:весь\s+)?jitosol/) { return .unstake(coins) }
        if q.contains(/застейк|стейкни|\bstake\b|в\s+стейкинг/) { return .stake(coins ?? amount) }
        if q.contains(/\bhlp\b|хлп/) {
            if outOf { return .hlp(deposit: false, amount: coins ?? amount ?? bare) }
            if into { return .hlp(deposit: true, amount: coins ?? amount ?? bare) }
        }
        // The network the lending command names; Base when it names none.
        let lendingNetwork = Self.network(in: q) ?? "base"
        // Borrowing on Kamino and paying it back: «borrow 2 USDC on kamino against 0.05 SPCX», «займи 2 USDC под SPCX на
        // камино», «repay all USDC on kamino», «погаси долг на камино». A Backpack stock named is Kamino, not Aave.
        let stock = Self.kaminoStock(in: q)
        if q.contains(/kamino|камино/) || (stock != nil && !q.contains(/\baave\b|ааве|\bаав/)) {
            if q.contains(Self.borrowWords), !q.contains(Self.repayWords) {
                let adding = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:shares?\s+of\s+|акци\w*\s+)?(?:spcx|sndk|skhy|\bmu\b|space\s*x|спейс\w*|micron|микрон\w*|hynix|хайникс\w*|sandisk|сандиск\w*)/)
                    .flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
                return .kaminoBorrow(amount ?? (bare == adding ? nil : bare), against: stock, adding: adding)
            }
            if q.contains(Self.repayWords) {
                return .kaminoRepay(q.contains(/\ball\b|\bвесь\b|\bвсё\b|\bвсе\b|полност/) ? nil : amount ?? bare)
            }
        }
        // Borrowing on Aave and paying it back, and ETH as its collateral: «borrow 10 USDC on aave», «займи 10 USDC в aave»,
        // «repay all USDC on aave», «погаси долг на aave», «supply 0.01 ETH as collateral on aave», «положи 0.01 ETH в залог».
        if q.contains(Self.borrowWords), !q.contains(/\brepay\w*|погас\w*|верн\w*/) {
            return .aaveBorrow(amount ?? bare, network: lendingNetwork)
        }
        if q.contains(Self.repayWords) {
            return .aaveRepay(q.contains(/\ball\b|\bвесь\b|\bвсё\b|\bвсе\b|полност/) ? nil : amount ?? bare, network: lendingNetwork)
        }
        if q.contains(/collateral|залог/), let m = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:eth|эфир)/) {
            return .aaveCollateral(Decimal(string: String(m.1).replacingOccurrences(of: ",", with: ".")), network: lendingNetwork)
        }
        if q.contains(/\baave\b|ааве|\bаав/) {
            if outOf { return .aave(deposit: false, amount: coins ?? amount ?? bare, network: lendingNetwork) }
            // Only with a word for putting money in: «what do I have on Aave?» is a question.
            if into || q.contains(/\blend\b|\bsupply\b|\bbuy\b|\bкуп\w*|одолж/) { return .aave(deposit: true, amount: coins ?? amount ?? bare, network: lendingNetwork) }
        }
        if q.contains(/morpho|морфо|gauntlet/) {
            if outOf { return .morpho(deposit: false, amount: coins ?? amount ?? bare, network: lendingNetwork) }
            if into || q.contains(/\blend\b|\bsupply\b|\bdeposit\b|депозит|полож|одолж/) { return .morpho(deposit: true, amount: coins ?? amount ?? bare, network: lendingNetwork) }
        }
        if q.contains(/kamino|камино/) {
            // A Backpack stock goes in and out by its count of tokens: «deposit 0.05 SPCX to kamino».
            let tokens = stock == nil ? coins : bare
            if outOf { return .kamino(deposit: false, symbol: stock ?? symbol, amount: tokens ?? amount ?? bare) }
            if into { return .kamino(deposit: true, symbol: stock ?? symbol, amount: tokens ?? amount ?? bare) }
        }
        if q.contains(/(?:останов|отмен|прекрат|стоп)\w*\s+(?:регулярн\w*\s+)?(?:покупк|dca|расписан)|(?:stop|cancel|end)\s+(?:the\s+|all\s+)?(?:recurring|regular|scheduled)?\s*(?:buys|purchases|dca|schedules?)/) { return .stopSchedules }
        if q.contains(/покупай|каждый|каждую|каждые|раз\s+в\s+|регулярн|\bdca\b|\bevery\b|\beach\b|weekly|daily|monthly|recurring/), q.contains(/куп|dca|\bbuy/), let asset = SpotAsset.find(q), let amount {
            let days: Int
            if q.contains(/день|сутки|ежедневн|daily|every\s+day|each\s+day/) && !q.contains(/каждые\s+\d|every\s+\d/) { days = 1 }
            else if let m = q.firstMatch(of: /(?:каждые|every)\s+(\d+)\s*(?:дн|день|сут|days?)/), let n = Int(m.1) {
                // «every 0 days» never gets past now and buys without end; ScheduledTask.parse refuses it too.
                guard n > 0 else { return nil }
                days = n
            }
            else if q.contains(/месяц|monthly/) { days = 30 }
            else { days = 7 }
            return .schedule(asset, usdc: amount, days: days)
        }
        // «закрой NEAR-аккаунт и переведи всё на Bybit», «close the NEAR account into Bybit»: the last «на / to / into» names it.
        if q.contains(/(?:close|delete|закр\w*|удали\w*)\s+(?:(?:the|my|мой)\s+)?(?:near|неар\w*)[\s-]*(?:account|аккаунт\w*)/),
           let m = text.firstMatch(of: /(?i).*\s(?:into|to|на|в)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$/) {
            return .closeNear(name: String(m.1).trimmingCharacters(in: .whitespaces))
        }
        // «send all RAY on solana to Bybit», «отправь 1973 AI в робинхуде на New wallet»: any token, as it is, to a saved
        // address. SOL with a number, NEAR, and USDC with a number but no network keep their own forms below.
        if let m = text.firstMatch(of: /(?i)^\s*(?:send|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+(all|весь|всё|все|\d+(?:[.,]\d+)?)\s+(?:(?:the|my|мои|мой|моих)\s+)?\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9._-]{0,19})(?:\s+tokens?)?(?:\s+(?:on|in|в|на|по)\s+(?:the\s+)?(?:сети\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|solana|солан\w*|base|бейс\w*|бейз\w*|баз[еыау]|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робин[гх]уд\w*|tempo|темпо\w*|bitcoin|биткоин\w*|zcash|tron|трон\w*|xrp|ripple|dogecoin|sui)(?:\s+(?:network|chain|сети))?)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$/) {
            let amount = Decimal(string: String(m.1).replacingOccurrences(of: ",", with: "."))
            let token = String(m.2), t = token.uppercased(), name = String(m.4).trimmingCharacters(in: .whitespaces)
            let network = m.3.map { w -> String in
                let s = String(w).lowercased()
                let across = ["bitcoin": "bitcoin", "биткоин": "bitcoin", "zcash": "zcash", "tron": "tron", "трон": "tron", "xrp": "xrp", "ripple": "xrp", "dogecoin": "dogecoin", "sui": "sui"]
                if let a = across.first(where: { s.hasPrefix($0.key) })?.value { return a }
                return s.hasPrefix("sol") || s.hasPrefix("сол") ? "solana" : s.hasPrefix("баз") ? "base" : Self.network(s) ?? s
            }
            let stable = t == "USDC" || t == "USDG"
            let ownForm = (t == "SOL" && amount != nil) || t == "NEAR" || (stable && amount != nil && network == nil)
            // Our own places stay what they were; a pasted address is not a name; «send all the money» names no token.
            let place = name.lowercased().wholeMatch(of: /(?:the\s+)?(?:solana|солан\w*|hyperliquid|хайперликвид\w*|base|бейс\w*|aave|kamino|hlp|vault|jupiter\w*|morpho)/) != nil
            if !ownForm, !place, !["MONEY", "FUNDS", "EVERYTHING", "IT", "THEM", "TOKENS"].contains(t),
               !name.lowercased().contains(/0x[0-9a-f]{6}/), name.count <= 40 {
                // USDC on a network only NEAR Intents reaches (Sui) goes as a token through it.
                if stable, let amount, !["bitcoin", "zcash", "tron", "xrp", "dogecoin", "sui"].contains(network ?? "") { return .sendTo(name: name, amount: amount, network: network) }
                return .sendToken(name: name, token: token, amount: amount, network: network)
            }
        }
        // «отправь 0.001 SOL на Old wallet», «send 0.01 SOL to Bybit», «send 0.5 SOL on solana to Bybit»: native SOL to a
        // saved Solana address.
        if let m = text.firstMatch(of: /(?i)(?:send|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+(\d+(?:[.,]\d+)?)\s*(?:sol|сол\w*)(?:\s+(?:on|in|в|на|по)\s+(?:the\s+)?(?:сети\s+)?(?:solana|солан\w*)(?:\s+(?:network|chain|сети))?)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$/),
           let value = Decimal(string: String(m.1).replacingOccurrences(of: ",", with: ".")) {
            let name = String(m.2).trimmingCharacters(in: .whitespaces)
            // «send 1 SOL to Jupiter / the vault» is a deposit, not a transfer to someone.
            if name.lowercased().wholeMatch(of: /(?:the\s+)?(?:vault|jupiter\w*|kamino|jito\w*|хранилищ\w*|джупитер\w*|камино)/) == nil {
                return .sendSol(name: name, amount: value)
            }
        }
        // «отправь 0.05 NEAR на Bybit», «send all NEAR to Bybit»: a number or «all» is required — never guessed.
        if let m = text.firstMatch(of: /(?i)(?:send|transfer|move|отправь\w*|переведи|перевести|скинь|кинь)\s+((?:all|весь|всё|все)\s+(?:(?:the|my|мой|мои)\s+)?)?(?:(\d+(?:[.,]\d+)?)\s*)?(?:near|неар\w*|ниар\w*)\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$/),
           m.1 != nil || m.2 != nil {
            let amount = m.2.flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
            return .sendNear(name: String(m.3).trimmingCharacters(in: .whitespaces), amount: amount)
        }
        // «near» is also a word: «buy ETH near 2500» is a limit on ETH, not the NEAR token.
        if q.contains(/\bкуп\w*|\bbuy\b/), q.contains(/\bnear\b(?!\s*\$?\d)|\bнеар\w*|\bниар\w*|\bнир\b/), SpotAsset.find(q) == nil,
           !q.contains(/\b(?:long|short|лонг\w*|шорт\w*|perp\w*)\b/) {
            // «на 3» / «for 3» is dollars; «buy 2 NEAR» is coins and is not guessed.
            let forDollars = q.firstMatch(of: /(?:\bна|\bfor)\s+(\d+(?:[.,]\d+)?)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            return .buyNear(usdc: amount ?? forDollars)
        }
        // «продай неар», «sell 0.5 NEAR»: native NEAR to USDC on Solana; no number = all of it. Where the USDC lands («for
        // USDC on ethereum», «… on Solana») names no coin to sell: it's left out of the check for one.
        if q.contains(/продай\w*|продать|\bsell\b/), q.contains(/\bnear\b|\bнеар\w*|\bниар\w*|\bнир\b/),
           SpotAsset.find(q.replacing(/\b(?:for|в|за|into)\s+usdc\b.*$/, with: "")) == nil,
           !q.contains(/\b(?:long|short|лонг\w*|шорт\w*|perp\w*)\b/) {
            let coins = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:near|неар\w*|ниар\w*)/).flatMap { Decimal(string: String($0.1).replacingOccurrences(of: ",", with: ".")) }
            if coins == nil, amount != nil { return nil }
            // «… for USDC on base», «продай NEAR в USDC на арбитрум»: the network the USDC arrives on; Solana by default.
            return .sellNear(amount: coins, to: Self.network(in: q))
        }
        if q.contains(/\bкуп\w*|\bbuy\b/), let asset = SpotAsset.find(q) { return .buy(asset, usdc: amount) }
        if q.contains(/продай\w*|продать|\bsell\b/), let asset = SpotAsset.find(q) {
            let coins = q.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:sol|btc|eth|jup|сол|битк|эфир)/).flatMap { Decimal(string: $0.1.replacingOccurrences(of: ",", with: ".")) }
            // «продай сол на 10 баксов» names dollars, not coins: no amount to sell, never «all of it».
            if coins == nil, amount != nil { return nil }
            return .sell(asset, amount: coins)
        }
        if vault, outOf || q.contains(/достан\w*/) { return .lendWithdraw(symbol, coins ?? amount ?? bare) }
        if vault, into || q.contains(/^\s*lend\b/) { return .lendDeposit(symbol, coins ?? amount ?? bare) }
        // «jupiter vault 1 USDC» without a verb.
        if jupiter, let value = coins ?? amount { return .lendDeposit(symbol, value) }
        if let m = text.firstMatch(of: /(?i)(?:send|pay|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+\$?(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл\w*|dollars?|bucks)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$/),
           let value = Decimal(string: String(m.1).replacingOccurrences(of: ",", with: ".")) {
            var name = String(m.2).trimmingCharacters(in: .whitespaces)
            // «… to Mom on ethereum», «… на main wallet по эфир сети»: the network goes, the name stays.
            var network: String?
            if let tail = name.firstMatch(of: /(?i)\s+(?:on|in|via|по|в|на|через)\s+(?:the\s+)?(?:сети\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|ethereum|эфир\w*|mainnet|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|robinhood|робинхуд\w*|tempo|темпо|solana|солан\w*|hyperliquid|хайперликвид\w*)(?:\s+(?:network|chain|mainnet|сет[иья]))?\s*$/) {
                let word = String(tail.1).lowercased()
                network = ["hyperevm", "optimism", "bnb"].contains(Self.network(word) ?? "") ? Self.network(word) : word.hasPrefix("эфир") || word == "mainnet" ? "ethereum" : word.hasPrefix("бейс") ? "base" : word.hasPrefix("арбитрум") ? "arbitrum"
                    : word.hasPrefix("полигон") ? "polygon" : word.hasPrefix("робинхуд") ? "robinhood" : word == "темпо" ? "tempo"
                    : word.hasPrefix("солан") ? "solana" : word.hasPrefix("хайперликвид") ? "hyperliquid" : word
                name = String(name[..<tail.range.lowerBound])
            }
            // Our own places stay what they were; a pasted address is not a name.
            let place = name.lowercased().wholeMatch(of: /(?:the\s+)?(?:solana|солан\w*|hyperliquid|хайперликвид\w*|base|бейс|aave|kamino|hlp|vault)/) != nil
            if !place, !name.lowercased().contains(/0x[0-9a-f]{6}/), name.count <= 40 {
                return .sendTo(name: name, amount: value, network: network)
            }
        }
        if !fromSolana, q.contains(/arbitrum|арбитрум/), q.contains(/(?:выв[её]д\w*|верн\w*|перевед\w*|перенес\w*|withdraw\w*|move|send|bring|bridge)(?:[^.]|\.(?=\d))*(?:solana|солан)/) {
            return .arbitrumToSolana(amount)
        }
        if !fromSolana, let net = Self.network(in: q), ["polygon", "ethereum", "tempo", "hyperevm", "optimism", "bnb"].contains(net),
           q.contains(/(?:выв[её]д\w*|верн\w*|перевед\w*|перенес\w*|withdraw\w*|move|send|bring|bridge)(?:[^.]|\.(?=\d))*(?:solana|солан)/) {
            return .evmToSolana(net, amount)
        }
        if !fromSolana, q.contains(/\bbase\b|бейс|бейз|\bбаз[аеуы]\b/), q.contains(/(?:выв[её]д\w*|верн\w*|перевед\w*|перенес\w*|withdraw\w*|move|send|bring|bridge)(?:[^.]|\.(?=\d))*(?:solana|солан)/) {
            return .baseToSolana(amount)
        }
        // «…to Solana» from Hyperliquid, with a word for taking it out («withdraw», «выведи», «верни», «from Hyperliquid»)
        // or Solana as where it goes («send 3 USDC to Solana»). «send 0.5 SOL on solana to Bybit» names Solana as the
        // place, not as where it goes; «из соланы» is the other direction.
        if !vault, !fromSolana,
           q.contains(/(?:выв[её]д\w*|верн\w*|перевед\w*|withdraw\w*|move|send|bring)(?:[^.]|\.(?=\d))*(?:solana|солан)/),
           q.contains(/выв[её]д\w*|верн\w*|withdraw\w*|hyperliquid|хайпер\w*|гипер\w*|\bhl\b|(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+|my\s+)?(?:solana|солан[уа]?)\b(?!\s+(?:to|на|для)\b)/) {
            return .withdrawToSolana(amount)
        }
        return nil
    }

    nonisolated(unsafe) static let depositWords = try! Regex("полож\\w*|внес\\w*|закин\\w*|вкин\\w*|кинь|засун\\w*|влож\\w*|задепозит\\w*|депозитн\\w*|депни|пополн\\w*|инвест\\w*|\\bdeposit\\w*|\\bdepo\\b|\\bput\\b|\\badd\\b|\\bplace\\b|\\bsupply\\b|\\bstash\\b|\\bpark\\b|\\binvest\\w*|\\btop\\s+up\\b")
    nonisolated(unsafe) static let withdrawWords = try! Regex("сним\\w*|выв[её]д\\w*|забер\\w*|верн\\w*|\\bwithdraw\\w*|take\\s+out|\\bremove\\b|\\bpull\\b|\\bredeem\\w*")
    /// «в камино», «into the vault», «из хранилища»: compiled once, not on every message.
    /// «займёт» is time («сколько займёт покупка»), not a loan.
    nonisolated(unsafe) static let borrowWords = /\bborrow\w*|\bзайм(?![её]т)\w*|\bзайми\b|занять|\bв\s+долг\b|\bloan\b/
    nonisolated(unsafe) static let repayWords = /\brepay\w*|погас\w*|(?:верни|вернуть|закрой|закрыть)\s+(?:весь\s+|мой\s+)?долг/

    /// Backpack's tokenized stocks Kamino's Backpack market takes as collateral, by ticker: their Token-2022 mints
    /// (Kamino.backpackStocks has the same four).
    public static let kaminoStockMints = ["SPCX": "SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb", "MU": "MUxEsUKSMACyw5fZf68wxf5FLnZVhtU9CwH8uNNGay1",
                                          "SKHY": "SKHYhSjuRWHgikq8eRKbtBbpABgJSkd7ytQV14i9EQ3", "SNDK": "SNDKbwMUQvZhnLnxLduradgLHG5KrPuKwpnrkkGRhfH"]
    public static let kaminoStocks = ["SPCX", "MU", "SKHY", "SNDK"]

    /// The Backpack stock a request names, by ticker or company, in English or Russian.
    static func kaminoStock(in q: String) -> String? {
        if q.contains(/\bspcx\b|space\s*x|спейс\w*/) { return "SPCX" }
        if q.contains(/\bsndk\b|sandisk|сандиск/) { return "SNDK" }
        if q.contains(/\bskhy\b|hynix|хайникс|хиникс/) { return "SKHY" }
        if q.contains(/\bmu\b|micron|микрон/) { return "MU" }
        return nil
    }

    static let places = #"(?:the\s+|my\s+|мо[йеюи]\s+)?(?:vault|хранилищ|jupiter|jupyter|джупитер|юпитер|kamino|камино|aave|ааве|hlp|хлп|лендинг|lend)"#
    nonisolated(unsafe) static let intoPlace = try! Regex(#"(?:\bв\b|\bво\b|into|\bto\b|\bin\b)\s+"# + places)
    nonisolated(unsafe) static let outOfPlace = try! Regex(#"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b|out\s+of)\s+"# + places)
}

/// A regular purchase the app carries out while it is open.
public struct Schedule: Codable, Equatable, Sendable {
    public let symbol: String
    public let usdc: Decimal
    public let days: Int
    public var next: Date
    /// Switched off on the Schedule.
    public var paused: Bool?

    public init(symbol: String, usdc: Decimal, days: Int, next: Date) {
        self.symbol = symbol
        self.usdc = usdc
        self.days = days
        self.next = next
    }

    public var asset: SpotAsset? { SpotAsset.all.first { $0.symbol == symbol } }

    /// After a run: the next date strictly in the future, so a long pause does not buy several times at once.
    /// A stored «every 0 days» counts as daily: it would otherwise never get past now.
    public mutating func advance(now: Date = Date()) {
        repeat { next = next.addingTimeInterval(TimeInterval(max(days, 1)) * 86_400) } while next <= now
    }
}

extension WalletCommand {
    /// «jupiter dca …», «dca on jupiter», «дса через юпитер».
    nonisolated(unsafe) static let jupiterDCAWords = /(?:jupiter|jupyter|джупитер\w*|юпитер\w*)\s*(?:dca|дса|дца)|(?:dca|дса|дца)\s+(?:on|via|through|with|at|через|на|в|с)\s+(?:jupiter|jupyter|джупитер\w*|юпитер\w*)/

    /// «jupiter dca 100 USDC into SOL in 2 orders every day», «… over 4 weeks», «cancel jupiter dca SOL», «show jupiter dca».
    static func jupiterDCA(_ q: String) -> WalletCommand? {
        let rest = q.replacing(/jupiter|jupyter|джупитер\w*|юпитер\w*/, with: " ")
        // «into BONK»: a coin by ticker; «jupiter» itself must not read as JUP.
        let named = rest.firstMatch(of: /(?:into|in|for|of|в|на)\s+\$?([a-z][a-z0-9]{1,9})\b/).map { String($0.1) }
            .flatMap { ["usdc", "usd", "orders", "order", "parts", "swaps", "total", "all"].contains($0) ? nil : SpotAsset.find($0) ?? SpotAsset.bySymbol($0) }
        let coin = named ?? SpotAsset.find(rest)
        if rest.contains(/cancel|stop|close|\bend\b|отмен|останов|прекрат|закр|стоп/) { return .jupiterDCACancel(coin) }
        let usdc = (rest.firstMatch(of: /\$\s*(\d+(?:[.,]\d+)?)/)?.1 ?? rest.firstMatch(of: /(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл|dollars?|bucks|бакс)/)?.1)
            .flatMap { Decimal(string: String($0).replacingOccurrences(of: ",", with: ".")) }
        guard let coin, coin != .usdc, usdc != nil || !rest.contains(/show|list|status|\bmy\b|покаж|мои|список|статус|какие/) else {
            return usdc == nil ? .jupiterDCAOrders : nil
        }
        func unit(_ w: Substring) -> Int? {
            w.hasPrefix("min") || w.hasPrefix("мин") ? 60 : w.hasPrefix("hour") || w.hasPrefix("час") ? 3600
                : w.hasPrefix("day") || w.hasPrefix("дн") || w.hasPrefix("день") || w.hasPrefix("сут") ? 86400
                : w.hasPrefix("week") || w.hasPrefix("недел") ? 604_800 : w.hasPrefix("month") || w.hasPrefix("месяц") ? 2_592_000 : nil
        }
        var orders = rest.firstMatch(of: /(\d+)\s*(?:orders?|buys?|purchases?|swaps?|times|parts?|раз\b|раза|покуп\w*|част\w*|сдел\w*)/).flatMap { Int($0.1) }
        var every: Int?
        if let m = rest.firstMatch(of: /(?:every|each|кажд\w*|раз\s+в)\s+(\d+)?\s*(minutes?|hours?|days?|weeks?|months?|мин\w*|час\w*|дн\w*|день|сут\w*|недел\w*|месяц\w*)/), let u = unit(m.2) {
            every = (m.1.flatMap { Int($0) } ?? 1) * u
        } else if let m = rest.firstMatch(of: /\b(hourly|daily|weekly|monthly|ежечасн|ежедневн|еженедельн|ежемесячн)/) {
            every = ["hourly": 3600, "daily": 86400, "weekly": 604_800, "monthly": 2_592_000, "ежечасн": 3600, "ежедневн": 86400,
                     "еженедельн": 604_800, "ежемесячн": 2_592_000][String(m.1)]
        }
        // «over 4 weeks»: 4 swaps a week apart, or the given swaps spread over that time.
        if let m = rest.firstMatch(of: /(?:over|within|during|for|за|в\s+течени\w*|на)\s+(\d+)\s*(minutes?|hours?|days?|weeks?|months?|мин\w*|час\w*|дн\w*|день|сут\w*|недел\w*|месяц\w*)/), let n = Int(m.1), let u = unit(m.2), n > 0 {
            if let o = orders, o > 0, every == nil { every = n * u / o } else if orders == nil { orders = n; every = every ?? u }
        }
        return .jupiterDCA(coin, usdc: usdc, orders: orders, every: max(every ?? 86400, 60))
    }

    /// Swaps when none are named: one per 50 USDC (Jupiter's least swap), 2 to 10.
    public static func jupiterDCAOrders(for usdc: Decimal) -> Int {
        max(2, min(10, NSDecimalNumber(decimal: (usdc / 50).rounded(0, .down)).intValue))
    }

    /// Jupiter's least DCA, in USDC.
    public static let jupiterDCAMinimum: Decimal = 100

    /// Jupiter's own minimums for its DCA, said before a card: nil when the order can be placed.
    public static func jupiterDCAProblem(usdc: Decimal, orders: Int) -> String? {
        if usdc < jupiterDCAMinimum { return "Jupiter's DCA takes at least 100 USDC in all, 50 a swap: \(Fmt.num(usdc)) USDC is too little." }
        if orders < 2 { return "Jupiter's DCA needs at least 2 swaps." }
        if usdc / Decimal(orders) < 50 {
            return "Jupiter's DCA buys at least 50 USDC a swap: \(Fmt.num(usdc)) USDC makes \(NSDecimalNumber(decimal: (usdc / 50).rounded(0, .down)).intValue) swaps at most."
        }
        return nil
    }

    /// «day», «2 days», «week», «hour»: an interval in seconds as words.
    public static func everyText(_ seconds: Int) -> String {
        for (s, w) in [(2_592_000, "month"), (604_800, "week"), (86400, "day"), (3600, "hour"), (60, "minute")] where seconds % s == 0 {
            return seconds == s ? w : "\(seconds / s) \(w)s"
        }
        return "\(seconds) seconds"
    }

    /// A deposit, a buy or a stake that came without an amount: the chat asks, and the next answer fills it.
    public var amountQuestion: String? {
        switch self {
        case .lendDeposit(let s, nil): "How much \(s) into Jupiter Lend?"
        case .kamino(true, let s, nil): "How much \(s) into Kamino?"
        case .hlp(true, nil): "How much USDC into HLP? (at least $5)"
        case .aave(true, nil, let net): "How much USDC into Aave on \(Self.placeName(net))?"
        case .morpho(true, nil, let net): "How much USDC into Morpho on \(Self.placeName(net))?"
        case .buy(let a, nil): "For how many USDC should I buy \(a.symbol)?"
        case .jupiterDCA(let a, nil, _, _): "How many USDC in all for the Jupiter DCA into \(a.symbol)? (at least 100, 50 a swap)"
        case .buyNear(nil): "For how many USDC should I buy NEAR?"
        case .buyStock(let s, nil): "For how many USDC should I buy \(s)?"
        case .buyOn(let place, let t, nil, _): "For how many USDC should I buy \(t) on \(Self.placeName(place))?"
        case .stake(nil): "How much SOL to stake?"
        case .solanaTo(let place, nil): "How many USDC should I move from Solana to \(Self.placeName(place))?"
        case .receiveAcross(let net, let t, nil): "How much \(t) will you send from \(AddressCheck.Network(rawValue: net)?.title ?? net)?"
        case .aaveBorrow(nil, let net): "How many USDC to borrow on Aave on \(Self.placeName(net))?"
        case .kaminoBorrow(nil, let s, _): "How many USDC to borrow on Kamino" + (s.map { " against \($0)" } ?? "") + "?"
        case .aaveCollateral(nil, let net): "How much ETH as collateral on Aave on \(Self.placeName(net))?"
        default: nil
        }
    }

    /// The amount `with(amount:)` changes, as the command holds it: what a route step's details start from.
    public var editableAmount: Decimal? {
        switch self {
        case .lendDeposit(_, let x), .kamino(_, _, let x), .hlp(_, let x), .aave(_, let x, _), .morpho(_, let x, _), .buy(_, let x),
             .buyNear(let x), .buyStock(_, let x), .buyOn(_, _, let x, _), .stake(let x), .solanaTo(_, let x), .jupiterDCA(_, let x, _, _): x
        default: nil
        }
    }

    public func with(amount: Decimal) -> WalletCommand {
        switch self {
        case .lendDeposit(let s, _): .lendDeposit(s, amount)
        case .kamino(let d, let s, _): .kamino(deposit: d, symbol: s, amount: amount)
        case .hlp(let d, _): .hlp(deposit: d, amount: amount)
        case .aave(let d, _, let net): .aave(deposit: d, amount: amount, network: net)
        case .morpho(let d, _, let net): .morpho(deposit: d, amount: amount, network: net)
        case .buy(let a, _): .buy(a, usdc: amount)
        case .jupiterDCA(let a, _, let o, let e): .jupiterDCA(a, usdc: amount, orders: o, every: e)
        case .buyNear: .buyNear(usdc: amount)
        case .buyStock(let s, _): .buyStock(symbol: s, usdc: amount)
        case .buyOn(let place, let t, _, let from): .buyOn(place, token: t, usdc: amount, from: from)
        case .stake: .stake(amount)
        case .solanaTo(let place, _): .solanaTo(place, amount)
        case .receiveAcross(let net, let t, _): .receiveAcross(network: net, token: t, amount: amount)
        case .aaveBorrow(_, let net): .aaveBorrow(amount, network: net)
        case .kaminoBorrow(_, let s, let add): .kaminoBorrow(amount, against: s, adding: add)
        case .aaveCollateral(_, let net): .aaveCollateral(amount, network: net)
        case .sell(let a, _): .sell(a, amount: amount)
        case .sellOn(let place, let t, _, let keep): .sellOn(place, token: t, amount: amount, keep: keep)
        case .swapOn(let place, let from, let to, _): .swapOn(place, from: from, to: to, amount: amount)
        case .sendToken(let name, let token, _, let net): .sendToken(name: name, token: token, amount: amount, network: net)
        case .crossSwap(let from, let on, let to, let at, _): .crossSwap(from: from, on: on, to: to, at: at, amount: amount)
        default: self
        }
    }

    /// A network's own coin, the one its fees are paid in: on its network it is swapped for USDC, never bought from a token list.
    static func ownCoin(_ place: String) -> Set<String> {
        place == "polygon" ? ["POL", "MATIC"] : place == "hyperevm" ? ["HYPE"] : place == "bnb" ? ["BNB"] : ["ETH"]
    }

    /// The multisig's three forms, as the chat writes them in English.
    static func multisig(_ text: String) -> WalletCommand? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: ".!"))
        guard t.range(of: "multisig", options: .caseInsensitive) != nil else { return nil }
        if t.wholeMatch(of: /(?i)(?:show|open)\s+(?:the\s+|my\s+)?multisig/) != nil { return .multisigShow }
        func nets(_ s: Substring) -> [String]? {
            let list = s.split(separator: /\s*,\s*|\s+and\s+/).map { network(String($0)) }
            return list.allSatisfy { $0.map(ContractCall.networks.contains) == true } ? list.compactMap { $0 } : nil
        }
        if let m = t.wholeMatch(of: /(?i)send\s+(all|\d+(?:\.\d+)?)\s+(\S+)\s+from\s+(?:the\s+|my\s+)?multisig\s+to\s+(.+?)(?:\s+on\s+(\S+(?:\s+chain)?))?/) {
            guard let net = m.4.map({ nets($0)?.first }) ?? .some(nil) else { return nil }
            return .multisigSend(token: String(m.2), amount: m.1.lowercased() == "all" ? nil : Decimal(string: String(m.1)), to: String(m.3), network: net)
        }
        if let m = t.wholeMatch(of: /(?i)(?:make|create|set\s+up)\s+(?:a\s+|my\s+)?multisig(?:\s+vault)?(?:\s+(\d+)\s+of\s+(\d+))?(?:\s+with\s+(.+?))?(?:\s+on\s+(.+))?/) {
            guard let networks = m.4.map({ nets($0) }) ?? .some([]) else { return nil }
            let keys = m.3.map { $0.split(separator: /\s*,\s*|\s+and\s+/).map(String.init) } ?? []
            return .multisigCreate(threshold: m.1.flatMap { Int($0) }, of: m.2.flatMap { Int($0) }, keys: keys, networks: networks)
        }
        return nil
    }

    public static func placeName(_ place: String) -> String {
        ["base": "Base", "arbitrum": "Arbitrum", "robinhood": "Robinhood", "polygon": "Polygon", "ethereum": "Ethereum", "tempo": "Tempo", "hyperevm": "HyperEVM", "optimism": "Optimism", "bnb": "BNB Chain", "near": "NEAR"][place] ?? place.capitalized
    }

    /// The EVM network a word names: «арбитрум» → «arbitrum», «полигон» → «polygon», «эфириум» or «mainnet» → «ethereum».
    static func network(_ word: String) -> String? {
        let w = word.lowercased()
        // HyperEVM before Hyperliquid: both start with «hyper».
        if w.hasPrefix("hyperevm") || w.wholeMatch(of: /hyper\s+evm/) != nil || w.hasPrefix("хайперевм") || w.hasPrefix("гиперевм") { return "hyperevm" }
        if w.hasPrefix("optimism") || w.hasPrefix("оптимизм") { return "optimism" }
        // BNB Chain before Base: both start with «b».
        if w.hasPrefix("bnb") || w.hasPrefix("bsc") || w.hasPrefix("бнб") || w.hasPrefix("бсц") || w.hasPrefix("binance") { return "bnb" }
        return w.hasPrefix("b") || w.hasPrefix("бейс") || w.hasPrefix("бейз") ? "base" : w.hasPrefix("arb") || w.hasPrefix("арб") ? "arbitrum"
            : w.hasPrefix("rob") || w.hasPrefix("роб") ? "robinhood" : w.hasPrefix("pol") || w.hasPrefix("пол") ? "polygon"
            : w.hasPrefix("eth") || w.hasPrefix("эфир") || w.hasPrefix("main") || w.hasPrefix("мейн") ? "ethereum"
            : w.hasPrefix("temp") || w.hasPrefix("темп") ? "tempo"
            : w.hasPrefix("hyper") || w.hasPrefix("гипер") || w.hasPrefix("хайпер") || w == "hl" ? "hyperliquid" : nil
    }

    /// The first EVM network the text names, if any.
    static func network(in q: String) -> String? {
        q.firstMatch(of: /\b(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|tempo|темпо\w*)\b/).flatMap { network(String($0.1)) }
    }

    /// «1», «1 USDC», «$1.5», «да, 2», «ok 3», «1,000» — a short reply that is only an amount, with the unit it names:
    /// "$", "USDC", "USDT" or "SOL" (nil: none).
    static func amountReply(_ text: String) -> (value: Decimal, unit: String?)? {
        let q = IntentParser.plainNumbers(text.lowercased().trimmingCharacters(in: .whitespacesAndNewlines))
        guard let m = q.wholeMatch(of: /(?:(?:да|yes|ok|окей|давай|го|sure|yep|угу)[,!\s]*)?(?:на\s+|for\s+)?(\$)?\s*(\d+(?:[.,]\d+)?)\s*(usdc|юсдс|юсдц|usdt|usd|\$|долл\w*|dollars?|bucks|бакс\w*|sol|сол)?\s*[.!]?/),
              let value = Decimal(string: String(m.2).replacingOccurrences(of: ",", with: ".")), value > 0 else { return nil }
        let unit = m.3.map { w in w.hasPrefix("usdc") || w.hasPrefix("юсд") ? "USDC" : w == "usdt" ? "USDT" : w == "sol" || w == "сол" ? "SOL" : "$" }
        // «$5 sol» says both.
        if m.1 != nil, unit == "SOL" { return nil }
        return (value, unit ?? m.1.map { _ in "$" })
    }

    /// The amount alone, whatever its unit. Use `amountAnswer(_:for:)`, which checks the unit against the question.
    public static func amountAnswer(_ text: String) -> Decimal? { amountReply(text)?.value }

    /// The answer to `waiting`'s «How much?»: nil when its unit isn't what was asked for («$5» or «на 5 долларов» to a SOL
    /// stake, «1 sol» to a USDC deposit) or it's under the asset's smallest unit (0.000001 USDC, 0.000000001 SOL), which
    /// would build a «Deposit 0 USDC» card.
    public static func amountAnswer(_ text: String, for waiting: WalletCommand) -> Decimal? {
        guard let (value, unit) = amountReply(text) else { return nil }
        let asset = waiting.amountAsset
        let fits = unit == nil || unit == asset || (unit == "$" && asset != "SOL")
        return fits && value >= (asset == "SOL" ? Decimal(string: "0.000000001")! : Decimal(string: "0.000001")!) ? value : nil
    }

    /// What the amount of a command waiting for one counts: SOL for a stake, the deposited coin for Jupiter Lend and Kamino,
    /// else USDC (buys are paid in it).
    public var amountAsset: String {
        switch self {
        case .lendDeposit(let s, _), .kamino(_, let s, _): s
        case .stake: "SOL"
        default: "USDC"
        }
    }

    /// In words, for the confirm button and the reply: «deposit 1 USDC into Jupiter Lend».
    /// What a spread can leave out: the key a place's name contains, its name, and how it's said.
    public static let spreadProtocols: [(key: String, name: String, words: String)] = [
        ("jupiter", "Jupiter Lend", #"jupiter|jupyter|юпитер|джупитер|\bjup\b"#), ("kamino", "Kamino", "kamino|камино"),
        ("aave", "Aave", #"aave|ааве|\bаав"#), ("morpho", "Morpho", "morpho|морфо"), ("jitosol", "JitoSOL", "jito|джито|стейк|stak"),
    ]

    public var summary: String {
        func n(_ d: Decimal?) -> String { d.map { Fmt.num($0, maxFraction: 6) } ?? "all" }
        switch self {
        case .buy(let a, let u): return "buy \(a.symbol) for \(n(u)) USDC"
        case .buyNear(let u): return "buy native NEAR for \(n(u)) USDC (to your NEAR account)"
        case .sellNear(let x, let to): return "sell \(n(x)) NEAR from your NEAR account for USDC on \(to.map(Self.placeName) ?? "Solana")"
        case .sell(let a, let x): return "sell \(n(x)) \(a.symbol)"
        case .schedule(let a, let u, let d): return "buy \(a.symbol) for \(Fmt.num(u)) USDC every \(d == 1 ? "day" : d == 7 ? "week" : "\(d) days")"
        case .stopSchedules: return "stop recurring buys"
        // The canonical form: it parses back to the same order.
        case .jupiterDCA(let a, let u, let o, let e):
            return "Jupiter DCA \(n(u)) USDC into \(a.symbol) in \(o ?? u.map(Self.jupiterDCAOrders) ?? 2) orders every \(Self.everyText(e))"
        case .jupiterDCACancel(let a): return "cancel Jupiter DCA" + (a.map { " \($0.symbol)" } ?? "")
        case .jupiterDCAOrders: return "show Jupiter DCA"
        case .withdrawToSolana(let u): return "withdraw \(n(u)) USDC from Hyperliquid to Solana"
        case .lendDeposit(let s, let x): return "deposit \(n(x)) \(s) into Jupiter Lend"
        case .lendWithdraw(let s, let x): return "withdraw \(n(x)) \(s) from Jupiter Lend"
        case .kamino(let d, let s, let x): return d ? "deposit \(n(x)) \(s) into Kamino" : "withdraw \(n(x)) \(s) from Kamino"
        case .stake(let x): return "stake \(n(x)) SOL (→ JitoSOL)"
        case .unstake(let x): return "unstake \(n(x)) JitoSOL"
        case .yields(let a): return "rates for \(a ?? "USDC and SOL")"
        case .hlp(let d, let x): return d ? "deposit \(n(x)) USDC into HLP" : "withdraw \(n(x)) USDC from HLP"
        case .allocate(let s, let share, let x, let without, let from):
            return "spread \(x.map { "\(Fmt.num($0)) " } ?? "")\(s)\(from.map { " from \($0 == "solana" ? "Solana" : Self.placeName($0))" } ?? "") across the best rates"
                + (share.map { " (max \(Fmt.num($0))% per protocol)" } ?? "")
                + (without.isEmpty ? "" : " without " + without.compactMap { k in Self.spreadProtocols.first { $0.key == k }?.name }.joined(separator: ", "))
        case .rebalance(let share): return "move deposited USDC to the best rates" + (share.map { " (max \(Fmt.num($0))% per protocol)" } ?? "")
        case .aave(let d, let x, let net):
            return d ? "deposit \(n(x)) USDC into Aave on \(Self.placeName(net))" : "withdraw \(n(x)) USDC from Aave on \(Self.placeName(net)) to Solana"
        case .morpho(let d, let x, let net):
            return d ? "deposit \(n(x)) USDC into Morpho on \(Self.placeName(net))" : "withdraw \(n(x)) USDC from Morpho on \(Self.placeName(net)) to Solana"
        case .baseToSolana(let x): return "move \(n(x)) USDC from Base to Solana"
        case .arbitrumToSolana(let x): return "move \(n(x)) USDC from Arbitrum to Solana"
        case .evmToSolana(let net, let x): return "move \(n(x)) USDC from \(Self.placeName(net)) to Solana"
        case .sendTo(let name, let x, let net): return "send \(Fmt.num(x)) USDC to \(name)" + (net.map { " on \($0)" } ?? "")
        case .sendNear(let name, let x): return "send \(n(x)) NEAR to \(name)"
        case .closeNear(let name): return "close your NEAR account and send all its NEAR to \(name)"
        case .sendSol(let name, let x): return "send \(Fmt.num(x, maxFraction: 9)) SOL to \(name)"
        // The amount without grouping («1973.719299», not «1,973.72»): the text parses back to the same transfer.
        case .sendToken(let name, let token, let x, let net):
            return "send \(x.map { NSDecimalNumber(decimal: $0).stringValue } ?? "all") \(token)\(net.map { " on \($0)" } ?? "") to \(name)"
        case .receive: return "show where to top up"
        case .buyStock(let s, let u): return "buy \(s.hasPrefix("0x") ? "the token \(s)" : "\(s) stock tokens") for \(n(u)) USDC on Robinhood Chain"
        case .sellStock(let s, let x): return "sell \(n(x)) \(s.hasPrefix("0x") ? "of the token \(s)" : "\(s) stock tokens") on Robinhood Chain"
        case .robinhoodToSolana(let x): return "move \(n(x)) USDG from Robinhood Chain to Solana"
        case .buyOn(let place, let t, let u, let from):
            return "buy \(t.hasPrefix("0x") ? "the token \(t)" : Stablecoins.find(t, on: place)?.symbol ?? t.uppercased()) on \(Self.placeName(place)) for \(n(u)) USDC" + (from.map { " from \(Self.placeName($0))" } ?? "")
        case .sellOn(let place, let t, let x, let keep):
            return "sell \(n(x)) \(t.hasPrefix("0x") ? "of the token \(t)" : Stablecoins.find(t, on: place)?.symbol ?? t.uppercased()) on \(Self.placeName(place)) " + (keep ? "and keep the USDC there" : "for USDC on Solana")
        // The amount without grouping, the contracts whole: the text parses back to the same swap.
        case .swapOn(let place, let a, let b, let x):
            func name(_ t: String) -> String { t.hasPrefix("0x") || t.count >= 32 ? t : t.uppercased() }   // a mint keeps its case
            return "swap \(x.map { NSDecimalNumber(decimal: $0).stringValue } ?? "all") \(name(a)) for \(name(b)) on \(place == "hyperliquid" ? "Hyperliquid spot" : Self.placeName(place))"
        case .receiveAcross(let net, let t, let x):
            return "receive \(x.map { NSDecimalNumber(decimal: $0).stringValue + " " } ?? "")\(t) from \(AddressCheck.Network(rawValue: net)?.title ?? net) as USDC on Solana"
        case .crossSwap(let a, let from, let b, let to, let x):
            func name(_ t: String) -> String { t.hasPrefix("0x") || t.count >= 32 ? t : t.uppercased() }   // a mint keeps its case
            return "swap \(x.map { NSDecimalNumber(decimal: $0).stringValue } ?? "all") \(name(a)) on \(Self.placeName(from)) for \(name(b)) on \(Self.placeName(to))"
        case .solanaTo(let place, let x): return "move \(n(x)) USDC from Solana to \(Self.placeName(place))"
        case .pay(let method, let url, _): return "pay for \(method) \(url) in USDC on Tempo"
        case .contractCall(let c): return "call \(c.name) on \(c.contract.prefix(6))…\(c.contract.suffix(4)) on \(Self.placeName(c.network))"
        case .gas(let place, let usd): return "add ≈ \(Fmt.usd(usd ?? 1)) of gas on \(Self.placeName(place))"
        case .bridge(let from, let to, let x): return "move \(n(x)) USDC from \(Self.placeName(from)) to \(Self.placeName(to))" + (to == "robinhood" ? " (arrives as USDG)" : "")
        case .cctp(let from, let to, let x): return "move \(n(x)) USDC from \(Self.placeName(from)) to \(Self.placeName(to)) via CCTP"
        case .usdt(let from, let to, let x, let layerZero):
            return "move \(n(x)) USDT from \(Self.placeName(from)) to \(Self.placeName(to))" + (layerZero ? " via LayerZero" : "")
        case .aaveBorrow(let x, let net): return "borrow \(n(x)) USDC on Aave on \(Self.placeName(net))"
        case .kaminoBorrow(let x, let s, let add): return "borrow \(n(x)) USDC on kamino" + (s.map { " against " + (add.map { "\(n($0)) " } ?? "") + $0 } ?? "")
        case .kaminoRepay(let x): return x == nil ? "repay all USDC on kamino" : "repay \(n(x)) USDC on kamino"
        case .aaveRepay(let x, let net): return x == nil ? "repay all USDC on Aave on \(Self.placeName(net))" : "repay \(n(x)) USDC on Aave on \(Self.placeName(net))"
        case .aaveCollateral(let x, let net): return "supply \(n(x)) ETH as collateral on Aave on \(Self.placeName(net))"
        case .claimRewards: return "claim rewards on Merkl"
        case .hlSpotBuy(let s, let u): return "buy \(s) on Hyperliquid spot for \(n(u)) USDC"
        case .hlSpotSell(let s, let x): return "sell \(x.map { NSDecimalNumber(decimal: $0).stringValue } ?? "all") \(s) on Hyperliquid spot"
        case .showApprovals: return "show my token approvals"
        case .revokeApproval(let place, let token, let spender, let permit2):
            return "revoke the approval of \(token) to \(spender) on \(Self.placeName(place))" + (permit2 ? " through Permit2" : "")
        case .sweepDust(let under): return "sweep dust\(under.map { " under $\(Fmt.num($0))" } ?? "") into USDC on Base, Arbitrum and Polygon"
        case .sweepRewards(let min, let into): return "sweep Merkl rewards once worth \(Fmt.usd(max(min ?? 5, 5))) or more into \(into ?? "USDC")"
        case .multisigCreate(let t, let of, let keys, let nets):
            return "make a multisig" + (t.map { m in " \(m) of \(of ?? m)" } ?? "") + (keys.isEmpty ? "" : " with " + keys.joined(separator: ", "))
                + (nets.isEmpty ? "" : " on " + nets.joined(separator: ", "))
        case .multisigSend(let token, let x, let to, let net):
            return "send \(x.map { NSDecimalNumber(decimal: $0).stringValue } ?? "all") \(token) from the multisig to \(to)" + (net.map { " on \($0)" } ?? "")
        case .multisigShow: return "show the multisig"
        }
    }
}

extension IntentParser {
    /// Whether the text can start a new trade plan: it names a market or a direction. «1 USDC» alone does not —
    /// it would otherwise turn into the default «long ETH».
    public static func startsPlan(_ text: String, markets: Set<String> = []) -> Bool {
        text.lowercased().contains(/лонг|шорт|\blong\b|\bshort\b|\bbuy\b|\bsell\b/) || namedAsset(text, markets: markets) != nil
    }
}

/// «call arbitrum 0x… deposit(uint256,address) 10e6 me approving 10 USDC»: any contract's function on an EVM network
/// ihatecrypto signs on, from its signature, the arguments as written («me» is this wallet), in the token's smallest units.
/// The app encodes it, checks the contract on the network's explorer, simulates it with its exact approve and shows the
/// card; the signer reads the calldata again and refuses what drains (unlimited approvals, paying someone else).
public struct ContractCall: Equatable, Sendable {
    public struct Approve: Equatable, Sendable {
        public let amount: Decimal
        /// The token's ticker on the network (USDC, a stablecoin) or its contract.
        public let token: String
    }
    /// The EVM network's key (base, arbitrum, …).
    public let network: String
    public let contract: String
    /// «deposit(uint256,address)»
    public let function: String
    public let args: [String]
    /// The network's own coin sent with the call, in whole coins.
    public let value: Decimal?
    /// An exact approve of this much of the token to the contract, made first.
    public let approve: Approve?

    public var name: String { String(function.prefix { $0 != "(" }) }
    public static let networks: Set<String> = ["base", "arbitrum", "robinhood", "polygon", "ethereum", "tempo", "hyperevm", "optimism", "bnb"]

    public static func parse(_ text: String) -> ContractCall? {
        guard let m = text.wholeMatch(of: /(?i)\s*call\s+(\S+)\s+(0x[0-9a-f]{40})\s+(\w+\([a-z0-9,]*\))(.*?)(?:\s+with\s+(\d+(?:\.\d+)?)\s+[a-z]+)?(?:\s+approving\s+(\d+(?:\.\d+)?)\s+(\S+))?\s*[.]?\s*/),
              let net = WalletCommand.network(String(m.1)), networks.contains(net) else { return nil }
        let args = m.4.split(whereSeparator: \.isWhitespace).map(String.init)
        return ContractCall(network: net, contract: String(m.2), function: String(m.3), args: args,
                            value: m.5.flatMap { Decimal(string: String($0)) },
                            approve: m.6.flatMap { Decimal(string: String($0)) }.map { Approve(amount: $0, token: String(m.7 ?? "")) })
    }

    /// The command as the chat writes it.
    public var text: String {
        "call \(network) \(contract) \(function)" + args.map { " " + $0 }.joined()
            + (value.map { " with \(NSDecimalNumber(decimal: $0).stringValue) coins" } ?? "")
            + (approve.map { " approving \(NSDecimalNumber(decimal: $0.amount).stringValue) \($0.token)" } ?? "")
    }
}
