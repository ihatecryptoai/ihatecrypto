//! Landings: a chain's bridge legs land side by side, and a step waits only for those where it spends.

use crate::Error;
use crate::transit::Landings;

#[tokio::test]
async fn a_step_waits_for_the_landings_where_it_spends() {
    let landings = Landings::new();
    landings.add("base", async { Ok("Moved 2 USDC to Base.".to_string()) });
    landings.add("solana", async { Err(Error::Execution("Relay returned refund.".into())) });
    landings.add("base", async { Ok("Moved 3 USDC to Base.".to_string()) });
    assert_eq!((landings.count(None), landings.count(Some("base")), landings.count(Some("hyperliquid"))), (3, 2, 0));
    assert_eq!(landings.wait(Some("base")).await.unwrap(), ["Moved 2 USDC to Base.", "Moved 3 USDC to Base."]);
    assert_eq!(landings.count(None), 1);
    assert_eq!(landings.wait(None).await, Err(Error::Execution("Relay returned refund.".into())));
    assert_eq!(landings.count(None), 0);
}
