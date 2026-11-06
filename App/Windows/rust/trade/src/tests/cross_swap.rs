//! ExecutorTests' and OneClickTests' offline cases of CrossSwap.swift: what NEAR Intents may take over from Relay, and
//! a swap across networks with no dollar price for a side.

use super::d;
use crate::{CostlyMove, Error, Executor};

// A stop by the guard or the signer on Relay's quote is never taken to NEAR Intents, nor is a loss «Send anyway» asks
// about; a missing route is.
#[test]
fn relays_guard_stop_is_never_taken_to_near_intents() {
    assert!(!Executor::intents_may_take(&Error::Guard("Relay quote: recipient is 0xdead…, expected 0x9858….".into())));
    assert!(!Executor::intents_may_take(&Error::Guard("The Relay order refunds someone else. Nothing was signed.".into())));
    assert!(!Executor::intents_may_take(&Error::Costly(CostlyMove::new(d("1"), d("10"), "Relay's swap"))));
    assert!(Executor::intents_may_take(&Error::Execution("Relay doesn't quote BONK for ETH now. Nothing was signed.".into())));
    assert!(Executor::intents_may_take(&Error::Wallet("The request timed out.".into())));
}

/// No price for what goes in or what comes out stops a swap across networks, capped or not (read as $0, the loss check
/// let any loss through); with both priced, past 3% asks «Send anyway». 1Click pricing only what goes in: refused, never
/// asked about as a 100% loss.
#[test]
fn a_cross_swap_without_a_price_stops() {
    for capped in [true, false] {
        let Err(Error::Execution(said)) = Executor::holds_cross_loss(d("0"), d("8"), "Relay doesn't price USDC on Solana", "Relay's swap", capped) else { panic!() };
        assert_eq!(said, "Relay doesn't price USDC on Solana now, so what Relay's swap keeps can't be checked. Nothing was signed.");
    }
    Executor::holds_cross_loss(d("8.2"), d("8"), "x", "Relay's swap", true).unwrap(); // $0.20 of $8.20, under 3%: passes
    assert!(matches!(Executor::holds_cross_loss(d("10"), d("8"), "x", "Relay's swap", true), Err(Error::Costly(_)))); // $2 of $10: asks
    Executor::holds_cross_loss(d("10"), d("8"), "x", "Relay's swap", false).unwrap(); // the card shows the cost instead
    assert!(matches!(Executor::holds_cross_loss(d("10"), d("0"), "1Click doesn't price X for Y", "NEAR Intents", true), Err(Error::Execution(_))));
}
