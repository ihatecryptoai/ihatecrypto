//! SiteOpen.swift: «open jup.ag» in the chat opens the site in the window's browser, with the wallet.

use std::collections::HashMap;
use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::engine::Engine;

/// «open jup.ag», «открой сайт raydium.io», «go to https://app.kamino.finance/…»: the whole message, https only.
static OPEN_SITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:open|go\s+to|открой|зайди\s+на|перейди\s+на)\s+(?:the\s+site\s+|site\s+|сайт\s+)?((?:https://)?[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:/\S*)?)\s*$")
        .expect("a regex")
});

impl Engine {
    /// «open jup.ag», «открой сайт raydium.io»: the site in ihatecrypto's browser with the wallet, as the Mac's chat opens
    /// it, before any model reads the message. The window opens it (main.rs, «openSite»), https only.
    pub fn open_site_command(&self, text: &str) -> bool {
        let Ok(Some(m)) = OPEN_SITE.captures(text) else { return false };
        let site = m[1].to_string();
        self.out.event("openSite", &HashMap::from([("url", site.as_str())]));
        self.reply(&self.say(&format!("{site} is open in ihatecrypto's browser with your wallet: the site asks to connect first, and every signature shows here before it's made."),
                             &format!("{site} открыт в браузере ihatecrypto, с кошельком: сайт сначала попросит подключиться, и каждую подпись ты увидишь здесь до того, как она сделана.")),
                   None);
        true
    }
}
