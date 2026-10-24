@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Settings › AI model and Background (LocalModels.swift): the checks that decide what runs, and the ops the window sends.
/// No model, Ollama or installer is run: the owner forbids running Ollama on this Mac, and Check spends a real account.
@Test func onlyOllamasValidSignatureRunsItsInstaller() {
    #expect(OllamaSetup.signedByOllama("Valid|CN=Ollama Inc., O=Ollama Inc., L=Palo Alto, S=California, C=US"))
    #expect(OllamaSetup.signedByOllama("Valid|CN=Ollama, O=Ollama, C=US\r\n"))
    #expect(!OllamaSetup.signedByOllama("NotSigned|"))
    #expect(!OllamaSetup.signedByOllama("HashMismatch|CN=Ollama Inc., O=Ollama Inc."))
    #expect(!OllamaSetup.signedByOllama("Valid|CN=Ollama Inc., O=OllamaFake Inc."))
    #expect(!OllamaSetup.signedByOllama("Valid|CN=Ollama Inc., O=Someone Else"))
    #expect(!OllamaSetup.signedByOllama("Valid"))
}

@Test func versionsAndTheInstallersHost() {
    #expect(OllamaSetup.older("0.12.3", than: "0.13.0") && OllamaSetup.older("0.9.9", than: "0.10.0"))
    #expect(!OllamaSetup.older("0.13.0", than: "0.13.0") && !OllamaSetup.older("0.13.1", than: "0.13.0"))
    #expect(ClaudeCodeSetup.fromClaude("claude.ai") && ClaudeCodeSetup.fromClaude("downloads.claude.ai"))
    #expect(!ClaudeCodeSetup.fromClaude("evilclaude.ai") && !ClaudeCodeSetup.fromClaude("claude.ai.example.com") && !ClaudeCodeSetup.fromClaude(nil))
}

@Test func theChecksLineSaysWhatItMeans() throws {
    let good = try JSONDecoder().decode(ModelExam.Result.self, from: Data(#"{"passed":9,"total":10,"seconds":2.5,"misses":["a"]}"#.utf8))
    let bad = try JSONDecoder().decode(ModelExam.Result.self, from: Data(#"{"passed":5,"total":10,"seconds":4,"misses":["send 5 USDC to Mom","b","c","d"]}"#.utf8))
    #expect(Engine.examLine(good, model: "Qwen", say: { en, _ in en }) == "Qwen understood 9 of 10 requests, 2.5 s a reply — good for the chat.")
    let line = Engine.examLine(bad, model: "Qwen", say: { en, _ in en })
    #expect(line.contains("only 5 of 10") && line.hasSuffix("send 5 USDC to Mom; b; c"))
}

extension EngineTests {
    /// The window's switches: «Think before answering» is kept; Check without a connected account says what's missing;
    /// Keep always running shows what Windows said back, not the wish.
    @Test func theAIModelAndBackgroundSwitches() throws {
        let engine = try Engine()
        try engine.call("hello")
        func local(_ s: [String: Any]?) -> [String: Any] { s?["localSetup"] as? [String: Any] ?? [:] }
        #expect(local(try engine.call("thinking", ["test": true]).state)["thinking"] as? Bool == true)
        let checked = try engine.call("checkModel", ["provider": "anthropic"])
        #expect((local(checked.state)["status"] as? String)?.hasPrefix("Connect") == true)
        #expect(local(checked.state)["examProgress"] == nil || local(checked.state)["examProgress"] is NSNull)
        let wished = try engine.call("keepRunning", ["test": true])
        #expect(local(wished.state)["keepRunning"] == nil || local(wished.state)["keepRunning"] as? Bool != true)
        #expect(local(try engine.call("keepRunningState", ["test": true]).state)["keepRunning"] as? Bool == true)
        // Restarted on the same settings, the choices are still there.
        let saved = try Data(contentsOf: engine.dir.appendingPathComponent("engine/settings.json"))
        let again = try Engine(seed: ["settings.json": saved])
        #expect(local(try again.call("hello").state)["thinking"] as? Bool == true)
    }
}
