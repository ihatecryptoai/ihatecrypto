//! Favorites (Live13Tests' star, LiveTests' «+» box, EngineTests' unfavorite): a star toggles in place or brings the
//! block up, the «+» tells a token from a wallet before anything is looked up, and «remove token X» says what it did.
//! Offline: only the answers that come before DexScreener is asked.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use signer_api::SignerAddress;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

/// An engine on the test's own folder, in English; locked, so a state push draws the lock alone. Its settings set the
/// process's switches (`trade::modules`), so the tests that make one take turns.
async fn engine() -> (Rc<Engine>, tokio::sync::MutexGuard<'static, ()>) {
    let turn = crate::modules::SWITCHES.lock().await;
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
    e.st_mut().lock.locked = true;
    e.set_settings(|s| s.language = Some("en".into()));
    (e, turn)
}

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> { pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }

// Public test data only: Hardhat's account #1.
const MOM: &str = "0x70997970C51812dc3A65C64090c6Cd9F57a2c4Fc";

#[tokio::test]
async fn a_star_leaves_the_favorites_block_where_it_is() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        e.pin(Board::new("favorites", "favorites"));
        e.pin(Board::new("result", "Live13 above"));
        let mom = SignerAddress { name: "Live13 Star".into(), network: "base".into(), address: MOM.into(), memo: String::new() };
        assert_eq!(Engine::star_key(&mom), "base:0x70997970c51812dc3a65c64090c6cd9f57a2c4fc");
        e.star(&mom, true, false);
        assert_eq!(e.st().boards().iter().map(|b| b.kind.as_str()).collect::<Vec<_>>(), ["result", "favorites"]);
        assert!(e.st().starred.contains(&Engine::star_key(&mom)));
        assert_eq!(store::load::<Vec<String>>("starred.json").unwrap(), [Engine::star_key(&mom)]);
        // From the chat a star still brings the block up.
        e.star(&mom, false, true);
        assert_eq!(e.st().boards()[0].kind, "favorites");
        assert!(!e.st().starred.contains(&Engine::star_key(&mom)));
    }).await;
}

#[tokio::test]
async fn the_plus_box_tells_what_was_pasted() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        assert_eq!(e.look_token("hello there").await, map(&[("kind", "invalid")]));
        // A Bitcoin address is never a DexScreener token: no lookup, a wallet to save on Bitcoin.
        assert_eq!(e.look_token(" bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq ").await,
                   map(&[("kind", "address"), ("address", "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"), ("network", "bitcoin")]));
    }).await;
}

#[tokio::test]
async fn a_favorite_is_removed_by_its_symbol() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let aero = FavoriteToken {
            chain: "base".into(), address: "0x940181a94A35A4569E4529A3CDfB74e38FD98631".into(), name: "Aerodrome".into(), symbol: "AERO".into(), image: None,
            price: None, change24h: None, liquidity: None,
        };
        e.set_favorites(|f| f.push(aero));
        e.token_command(TokenCommand::parse("remove token AERO from favorites").unwrap(), None).await;
        assert!(e.st().favorites.is_empty() && store::load::<Vec<FavoriteToken>>("favorites.json").unwrap().is_empty());
        assert_eq!(e.st().messages().last().unwrap().text, "AERO is no longer in your favorites.");
        e.set_settings(|s| s.language = Some("ru".into()));
        e.token_command(TokenCommand::Unfavorite { symbol: "AERO".into() }, Some("note")).await;
        let said = e.st().messages().last().cloned().unwrap();
        assert_eq!((said.text.as_str(), said.note.as_deref()), ("AERO нет в избранном.", Some("note")));
        // Nothing changes without favorites: the prices aren't asked for.
        e.refresh_favorites().await;
    }).await;
}
