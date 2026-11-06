use super::set;
use crate::site_check::{Verdict, check, listed};

fn lookalike(k: &str) -> Verdict { Verdict::Lookalike(k.into()) }

#[test]
fn lookalike_sites_are_flagged() {
    assert_eq!(check("jup.ag"), Verdict::Known);
    assert_eq!(check("app.kamino.finance"), Verdict::Known);
    assert_eq!(check("jupp.ag"), lookalike("jup.ag"));
    assert_eq!(check("raydlum.io"), lookalike("raydium.io"));
    assert_eq!(check("example.com"), Verdict::Unknown);
    assert_eq!(check(""), Verdict::Local);
    // A known name inside another domain, a dash for its dot, or letters from another alphabet (punycode).
    assert_eq!(check("jup.ag.claim-airdrop.com"), lookalike("jup.ag"));
    assert_eq!(check("jup-ag.com"), lookalike("jup.ag"));
    assert_eq!(check("hyperliquid-app.xyz"), lookalike("app.hyperliquid.xyz"));
    assert_eq!(check("hyperliquid.exchange"), lookalike("app.hyperliquid.xyz"));
    assert_eq!(check("hyperliquid.xyz.evil.io"), lookalike("app.hyperliquid.xyz"));
    assert_eq!(check("kamino-finance.app"), lookalike("kamino.finance"));
    assert_ne!(check("xn--jp-zka.ag"), Verdict::Unknown);
    assert_eq!(check("station.jup.ag"), Verdict::Known);
    assert_eq!(check("app.hyperliquid.xyz"), Verdict::Known);
}

#[test]
fn phishing_list_blocks_a_domain_and_everything_under_it() {
    let (blocked, allowed) = (set(&["polymaraket.com", "evil.github.io"]), set(&["relay.walletconnect.org"]));
    assert!(listed("polymaraket.com", &blocked, &allowed));
    assert!(listed("app.Polymaraket.com", &blocked, &allowed));
    assert!(listed("evil.github.io", &blocked, &allowed));
    assert!(!listed("github.io", &blocked, &allowed));   // not the domain above an entry
    assert!(!listed("jup.ag", &blocked, &allowed));
    let mut more = blocked.clone();
    more.insert("walletconnect.org".into());
    assert!(!listed("relay.walletconnect.org", &more, &allowed));
    assert!(!listed("", &blocked, &allowed));
}
