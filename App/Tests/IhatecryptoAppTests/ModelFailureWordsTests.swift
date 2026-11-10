@testable import IhatecryptoApp
import Testing

/// The service's name as the chat's sentence takes it: «Ollama isn’t running on this Mac», never «Ollama on this Mac
/// isn’t running on this Mac».
@Test func aServiceIsNamedShortInASentence() {
    let names = ["Claude (subscription via Claude Code)": "Claude", "ChatGPT (subscription)": "ChatGPT", "Ollama on this Mac": "Ollama",
                 "LM Studio on this Mac": "LM Studio", "OpenAI API": "OpenAI", "xAI Grok API": "xAI Grok", "NEAR AI Cloud": "NEAR AI Cloud",
                 "Grok (SuperGrok / X Premium+)": "Grok", "OpenRouter": "OpenRouter", "Apple Intelligence": "Apple Intelligence"]
    for (long, short) in names { #expect(Workspace.serviceName(long) == short) }
}
