import Foundation

extension Engine {
    /// «open jup.ag», «открой сайт raydium.io»: the site in ihatecrypto's browser with the wallet, as the Mac's chat opens
    /// it, before any model reads the message. The window opens it (main.rs, «openSite»), https only.
    func openSiteCommand(_ text: String) -> Bool {
        guard let m = text.wholeMatch(of: /\s*(?:open|go\s+to|открой|зайди\s+на|перейди\s+на)\s+(?:the\s+site\s+|site\s+|сайт\s+)?((?:https:\/\/)?[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:\/\S*)?)\s*/.ignoresCase()) else { return false }
        let site = String(m.1)
        out.event("openSite", ["url": site])
        reply(say("\(site) is open in ihatecrypto's browser with your wallet: the site asks to connect first, and every signature shows here before it's made.",
                  "\(site) открыт в браузере ihatecrypto, с кошельком: сайт сначала попросит подключиться, и каждую подпись ты увидишь здесь до того, как она сделана."))
        return true
    }
}
