import Foundation
@testable import IhatecryptoCore
import Testing

/// The chat's safety rules a review found too loose (C1, C10).

@Test func lookupAddressesOnlyInStepsThatShow() {   // C1
    let a = "0xF00dF00dF00dF00dF00dF00dF00dF00dF00dF00d"
    let shows: (String) -> Bool = { $0.hasPrefix("chart") }
    func allowed(_ c: String, _ user: String = "покажи PEPE на базе и купи на 5 баксов", found: [String] = [a]) -> Bool {
        ChatRouter.addressesAllowed(c, userText: user, known: [], found: found, displays: shows, markets: ["ETH"])
    }
    #expect(allowed("chart token \(a) on base"))
    #expect(!allowed("buy token \(a) on base for 5 USDC"))
    // A display step first doesn't open the door for the buy after it, in a chain, a task or an alert.
    #expect(!allowed("chart token \(a) on base then buy token \(a) on base for 5 USDC"))
    #expect(!allowed("buy token \(a) on base for 5 USDC then chart ETH"))
    #expect(!allowed("every day: buy token \(a) on base for 5 USDC then chart ETH"))
    #expect(!allowed("when ETH is below 2000: buy token \(a) on base for 5 USDC"))
    #expect(allowed("every day: chart token \(a) on base"))
    // The address in the user's own words, or a favorite's, goes anywhere.
    #expect(allowed("chart token \(a) on base then buy token \(a) on base for 5 USDC", "купи \(a) на 5 баксов"))
    #expect(ChatRouter.addressesAllowed("buy token \(a) on base for 5 USDC", userText: "", known: [a.lowercased()], found: [], displays: shows, markets: []))
    // A favorite is known from then on, so a found address may not become one by a command that runs by itself (the
    // 2026-11-13 review): the rule for those leaves it out, in English and Russian; the user's own address still goes.
    let runs: (String) -> Bool = { ($0.hasPrefix("chart") || TokenCommand.addsFavorite($0)) && !TokenCommand.addsFavorite($0) }
    for favorite in ["favorite token \(a) on base", "watch token \(a)", "добавь в избранное токен \(a) на бейсе"] {
        #expect(TokenCommand.addsFavorite(favorite), "\(favorite)")
        #expect(!ChatRouter.addressesAllowed(favorite, userText: "добавь его в избранное", known: [], found: [a], displays: runs, markets: []), "\(favorite)")
        #expect(ChatRouter.addressesAllowed(favorite, userText: "добавь \(a) в избранное", known: [], found: [a], displays: runs, markets: []), "\(favorite)")
    }
    #expect(!TokenCommand.addsFavorite("chart token \(a) on base") && !TokenCommand.addsFavorite("buy token \(a) on base for 5 USDC"))
}

/// The chat asks «How much SOL?» again (naming `amountAsset`) when the reply is an amount, but not one for this question.
@Test func anAmountInTheWrongUnitIsAskedAgain() {   // C8, R1, R3
    let deposit = WalletCommand.lendDeposit("USDC", nil)
    for (reply, waiting) in [("$5", WalletCommand.stake(nil)), ("на 5 долларов", .stake(nil)), ("1 sol", deposit), ("0.0000001", deposit)] {
        #expect(WalletCommand.amountAnswer(reply) != nil && WalletCommand.amountAnswer(reply, for: waiting) == nil, "\(reply)")
    }
    #expect(WalletCommand.stake(nil).amountAsset == "SOL" && deposit.amountAsset == "USDC")
    #expect(WalletCommand.amountAnswer("0.5", for: .stake(nil)) == Decimal(string: "0.5"))
    #expect(WalletCommand.amountAnswer("long ETH") == nil)   // not an amount: the message goes on as before
}

@Test func aChangeInWordsNeverSignsTheCard() {   // C10
    for change in ["добавь ещё триста", "make it five hundred", "давай половину", "удвой", "double it", "add another hundred",
                   "сделай 800", "да, но на тысячу", "yes but two thousand", "давай поменьше", "давай сто"] {
        #expect(ChatWords.mayChange(change), "\(change)")
    }
    for yes in ["да", "давай", "подтверждаю, всё верно", "yes, go ahead", "да, сделай это", "make it so", "окей, подписывай",
                "верно, адрес правильный", "the address is correct, go", "посмотрел, всё ок", "стоит, делай", "sounds good"] {
        #expect(!ChatWords.mayChange(yes), "\(yes)")
    }
}
