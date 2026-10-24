@testable import IhatecryptoCore
import Testing
import WalletCore

/// G24: the platform the app runs on, in the words the person and the model read. Off Windows (the Mac) every word is
/// the Mac's own, unchanged; on Windows it's Windows Hello and this PC.
struct Core10Tests {
    @Test func thePlatformIsNamedAsItIs() {
        #if os(Windows)
        #expect(Presence.name == "Windows Hello" && Presence.short == "Windows Hello" && Presence.ru == "Windows Hello"
                && Presence.ruGen == "Windows Hello" && Presence.ruIns == "Windows Hello")
        #expect(ChatRouter.forms.contains("this PC's key is always one") && !ChatRouter.forms.contains("this Mac's key"))
        #expect(ChatRouter.system.contains("a Windows app — this chat") && ChatRouter.system.contains("signer on this PC — neither"))
        #expect(!ChatRouter.system.contains("Mac password") && !ChatRouter.forms.contains("Mac password"))
        #else
        #expect(Presence.name == (Presence.touchID ? "Touch ID" : "your Mac password"))
        #expect(Presence.short == (Presence.touchID ? "Touch ID" : "Mac password"))
        #expect(Presence.ru == (Presence.touchID ? "Touch ID" : "пароль от Mac"))
        #expect(Presence.ruGen == (Presence.touchID ? "Touch ID" : "пароля от Mac"))
        #expect(Presence.ruIns == (Presence.touchID ? "Touch ID" : "паролем от Mac"))
        #expect(ChatRouter.forms.contains("this Mac's key is always one"))
        #expect(ChatRouter.system.contains("a Mac app — this chat") && ChatRouter.system.contains("signer on this Mac — neither"))
        #endif
    }
}
