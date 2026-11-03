@testable import IhatecryptoApp
import Foundation
import Testing

/// A send card to another Hyperliquid account says before Confirm the dollar a new account keeps, and that with no answer
/// from Hyperliquid it's asked again at Confirm; an open account (or one still being asked) gets the plain card.
@Test func hyperliquidSendCardSaysTheOpeningDollar() {
    let new = Workspace.hlOpening(5, to: "Alex", .new)
    #expect(new.extra == 1 && new.line == "5 arrive · 1 opens Alex’s account, once" && new.to == " · new on Hyperliquid")
    #expect(new.warning == "Alex’s Hyperliquid account is new: Hyperliquid keeps 1 USDC of the first transfer to it, so 1 more goes and the 5 asked arrive.")
    #expect(new.footer == " The signature is for 6 USDC: 5 for Alex and 1 that Hyperliquid keeps once.")
    let unknown = Workspace.hlOpening(5, to: "Alex", .unknown)
    #expect(unknown.extra == 0 && unknown.line == nil && unknown.to.isEmpty && unknown.footer.isEmpty)
    #expect(unknown.warning == "Couldn’t ask Hyperliquid whether Alex’s account is new; a new one keeps 1 USDC of the first transfer. It’s asked again at Confirm, and with no answer then, nothing is sent.")
    for state in [HLRecipient.open, .checking, nil] {
        let plain = Workspace.hlOpening(5, to: "Alex", state)
        #expect(plain.extra == 0 && plain.line == nil && plain.to.isEmpty && plain.warning == nil && plain.footer.isEmpty, "\(String(describing: state))")
    }
}
