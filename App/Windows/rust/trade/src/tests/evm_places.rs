//! A place's round trip against what it earns (EVMTests' aRoundTripMustPayForItselfWithinHalfAYear).

use super::d;
use crate::evm_places::EvmPlace;

fn place(name: &str, protocol: &str, apy: &str, trip: &str) -> EvmPlace {
    EvmPlace { name: name.into(), protocol_name: protocol.into(), apy: d(apy), supplied: d("0"), round_trip_usd: d(trip) }
}

#[test]
fn a_round_trip_must_pay_for_itself_within_half_a_year() {
    // Ethereum: ≈ $1.90 there and back is more than half a year of 4.3% on 15 USDC (≈ $0.65 a year).
    assert!(place("Morpho · Ethereum", "Morpho", "4.3", "1.9").too_costly(d("15") * d("4.3") / d("100")).is_some());
    // Arbitrum: ≈ $0.08 is well under it.
    assert!(place("Aave · Arbitrum", "Aave", "3", "0.08").too_costly(d("15") * d("3") / d("100")).is_none());
}
