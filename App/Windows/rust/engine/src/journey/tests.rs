use super::{Journey, Stage};

fn journey() -> Journey { Journey::new("2 USDC → Mom".into(), "solana", "base", true, Some(30), 1_000) }

#[test]
fn the_journey_follows_one_clicks_words() {
    let mut j = journey();
    j.read("Signed on Solana", 2_000);
    assert_eq!(j.stage, Stage::Paying);
    j.read("1Click · deposit sent, waiting for it to land · 2 s of ≈ 45", 3_000);
    assert!(j.stage == Stage::Paid && j.paid_at == Some(3_000) && j.estimate == Some(45));
    j.read("1Click · deposit received, swapping · 9 s of ≈ 45", 9_000);
    assert_eq!(j.stage, Stage::Filling);
    j.read("1Click · done · 20 s of ≈ 45", 20_000);
    assert!(j.stage == Stage::Landed && j.ended_at == Some(20_000) && j.paid_at == Some(3_000));
    // A word it doesn't know, or a later lower stage, changes nothing.
    j.read("1Click · deposit sent, waiting for it to land · 21 s", 21_000);
    assert_eq!(j.stage, Stage::Landed);
}

#[test]
fn a_refund_and_an_end_without_the_last_word() {
    let mut j = journey();
    j.read("1Click · the deposit came short: 1Click refunds it · 5 s", 5_000);
    assert_eq!(j.stage, Stage::Paid);
    j.read("1Click · refunded · 60 s", 60_000);
    assert!(j.stage == Stage::Refunded && j.refunded && j.ended_at == Some(60_000));

    // Failed before anything was paid: no journey. Paid and no outcome: still on its way, settling on its own.
    assert_eq!(journey().ended(false, false, 9_000), None);
    let sent = journey().ended(false, true, 9_000).unwrap();
    assert!(sent.stage == Stage::Paid && sent.stuck && sent.ended_at == Some(9_000));
    let landed = journey().ended(true, false, 9_000).unwrap();
    assert!(landed.stage == Stage::Landed && !landed.stuck);
}

#[test]
fn a_quotes_time_reads_in_both_languages() {
    assert_eq!(Journey::seconds_in("≈ 1.02 USDC for $1.00 of it · ≈ 30 s"), Some(30));
    assert_eq!(Journey::seconds_in("≈ 1.02 USDC за $1.00 · ≈ 3 мин"), Some(180));
    assert_eq!(Journey::seconds_in("Confidential Intents · ≈ 45 s"), Some(45));
    assert_eq!(Journey::seconds_in("≈ 2.41 AERO"), None);
}

mod privacy_word {
    use std::rc::Rc;
    use std::sync::Arc;

    use crate::engine::Engine;
    use crate::outbox::Outbox;
    use crate::signer_link::SignerLink;
    use crate::store;

    fn engine() -> Rc<Engine> {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
        let mut st = e.st_mut();
        st.lock.locked = true;
        st.settings.language = Some("en".into());
        drop(st);
        e
    }

    fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

    /// A private request is never a public send: a form that left «privately» out goes private, and a schedule that
    /// would send in the open later, without the user's words, is refused.
    #[tokio::test]
    async fn a_private_request_never_sends_in_the_open() {
        let _dir = store::test_folder();
        tokio::task::LocalSet::new().run_until(async {
            let e = engine();
            assert!(e.run_canonical("send 20 USDC to Mom", "переведи маме приватно 20 USDC").await);
            // The private send's own reading (private_recipient), not the plain transfer's «Add it there first».
            assert_eq!(last(&e), "There’s no “Mom” in Addresses. Save it there first, then send by its name.");
            assert!(e.run_canonical("every friday at 09:00: send 20 USDC to Mom", "каждую пятницу переведи маме приватно 20 USDC").await);
            assert!(last(&e).starts_with("You asked for privacy, but a step here would send in the open."), "{}", last(&e));
            assert!(e.st().tasks.is_empty());
            // Asked privately, a move that can't be private prepares nothing.
            assert!(e.run_canonical("withdraw 5 USDC to solana", "выведи приватно 5 USDC").await);
            assert!(last(&e).starts_with("You asked for privacy: only a send with an amount goes privately"), "{}", last(&e));
        }).await;
    }
}
