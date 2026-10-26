@testable import IhatecryptoApp
import GuardJev
import Testing
import TxGuard

/// Why the guard or the second check stopped something is said in the chat's language, not in English inside a Russian reply.
@Test func secondCheckSaysWhyInTheChatsLanguage() {
    let asset = JevReview(label: "mismatch", confidence: 0.9, concern: true, text: "the command does something other than the message asks: the asset isn't the one named.", about: "asset")
    let ru = Workspace.doubtWords(asset, in: "Russian")
    #expect(ru.contains("монета") && !ru.contains("the command"))
    #expect(Workspace.doubtWords(asset, in: "English") == asset.text)
    #expect(Workspace.doubtWords(asset, in: "Spanish").contains("activo"))
    let unclear = JevReview(label: "unclear", confidence: 0.6, concern: true, text: "the message is too vague to tell.")
    #expect(Workspace.doubtWords(unclear, in: "Russian").contains("не понять"))
    #expect(Workspace.stopWords(.steers(82), in: "Spanish") == "La segunda verificación ve en el texto órdenes ajenas para el asistente (82%).")
    #expect(Workspace.stopWords(.commands(["ignore all previous instructions"]), in: "Russian") == "В тексте есть команды ассистенту: «ignore all previous instructions».")
    #expect(Workspace.stopWords(.redirects(91), in: "English") == "The second check: the text tries to redirect funds to its own address (91%).")
}
