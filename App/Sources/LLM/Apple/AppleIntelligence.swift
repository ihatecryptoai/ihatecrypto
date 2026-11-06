import Foundation
import FoundationModels
import LLMCore
import OSLog
import Security

/// Apple Intelligence through the Foundation Models framework: the model on this Mac, and on macOS 27 the bigger one on
/// Apple's Private Cloud Compute once Apple grants the app its managed entitlement. No key, no sign-in, no bill.
public struct AppleIntelligence: LLMProvider {
    static let onDevice = "apple-on-device", privateCloud = "apple-private-cloud"
    /// Granted by Apple on request (apps under 2M downloads); without it PCC says «available» but every call fails with
    /// ModelManagerError 1046 (checked 2026-09-30 on macOS 27.0).
    static let entitlement = "com.apple.developer.private-cloud-compute"

    public init() {}

    public var info: ProviderInfo { Self.info }

    // ponytail: read once at launch; turning Apple Intelligence on or finishing its download shows after a restart.
    static let info: ProviderInfo = {
        var models: [String] = []
        if hasPrivateCloud { models.append(privateCloud) }
        let local = SystemLanguageModel.default.availability
        if local == .available { models.append(onDevice) }
        return ProviderInfo(
            id: "apple", name: "Apple Intelligence", kind: .local, defaultModel: models.first ?? onDevice,
            note: "Apple's Foundation Models: free, no key. On this Mac the model holds only 4,096 tokens, too small for the chat's "
                + "brief. Private Cloud Compute (32k, on Apple's servers): " + (privateCloudBlocker ?? "available") + ".",
            unavailable: models.isEmpty ? reason(local) : nil,
            models: models
        )
    }()

    /// Why a model can't be the chat's: the one on this Mac holds 4,096 tokens and reads no Russian, and the chat's brief
    /// alone is 5,042. Settings › Test still asks it.
    public static func chatRefusal(model: String) -> String? {
        model == onDevice ? "Apple's model on this Mac holds 4,096 tokens and doesn't read Russian, while the chat's brief alone is 5,042: "
            + "it can't be the chat's model. Test it in Settings; the bigger Apple model comes once Apple grants the app its entitlement." : nil
    }

    static var hasPrivateCloud: Bool { privateCloudBlocker == nil }

    /// What keeps Private Cloud Compute away, said plainly; nil when it's there. Logged once (subsystem app.ihatecrypto.mac,
    /// category apple-intelligence), since the menu only shows the model or not.
    static let privateCloudBlocker: String? = {
        let why: String? = {
            guard #available(macOS 27, *) else { return "needs macOS 27" }
            guard let task = SecTaskCreateFromSelf(nil), SecTaskCopyValueForEntitlement(task, entitlement as CFString, nil) as? Bool == true else {
                return "this build isn't signed with Apple's entitlement"
            }
            switch PrivateCloudComputeLanguageModel().availability {
            case .available: return nil
            case .unavailable(.deviceNotEligible): return "Apple says this Mac or Apple Account isn't eligible"
            case .unavailable(.systemNotReady): return "Apple's service isn't ready on this Mac yet (Apple Intelligence still setting up, or signed out of the Apple Account)"
            case .unavailable(let other): return "unavailable (\(other))"
            }
        }()
        Logger(subsystem: "app.ihatecrypto.mac", category: "apple-intelligence").notice("Private Cloud Compute: \(why ?? "available", privacy: .public)")
        return why
    }()

    static func reason(_ availability: SystemLanguageModel.Availability) -> String {
        switch availability {
        case .unavailable(.deviceNotEligible): "This Mac can't run Apple Intelligence."
        case .unavailable(.appleIntelligenceNotEnabled): "Turn on Apple Intelligence in System Settings, then restart ihatecrypto."
        case .unavailable(.modelNotReady): "Apple Intelligence is still downloading its model; restart ihatecrypto when it's done."
        default: "Apple Intelligence isn't available on this Mac."
        }
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        AppleIntelligenceClient()
    }
}

struct AppleIntelligenceClient: LLMClient {
    /// A fresh session per call: the app sends the whole conversation every time, like to any other model.
    func complete(_ r: LLMRequest) async throws -> String {
        let instructions = r.system.isEmpty ? nil : r.system
        let prompt = r.dialogue.map(\.text).joined(separator: "\n\n")
        let session: LanguageModelSession
        if r.model == AppleIntelligence.privateCloud, #available(macOS 27, *) {
            session = LanguageModelSession(model: PrivateCloudComputeLanguageModel(), instructions: instructions)
        } else {
            session = LanguageModelSession(model: SystemLanguageModel.default, instructions: instructions)
        }
        do {
            return try await session.respond(to: prompt).content
        } catch LanguageModelSession.GenerationError.exceededContextWindowSize {
            throw LLMError("Apple Intelligence: the request is longer than this model holds. Pick a bigger model for the chat.")
        } catch LanguageModelSession.GenerationError.guardrailViolation {
            throw LLMError("Apple Intelligence declined this request (its safety guardrails).")
        } catch {
            // macOS 27's LanguageModelError already reads well: «Content contains 8058 tokens, which exceeds … 4096.»
            // Its «transcript exceeded the model's context size» is the same too-long case.
            if error.localizedDescription.contains("context size") { throw LLMError(AppleIntelligence.chatRefusal(model: r.model) ?? "Apple Intelligence: the request is longer than this model holds.") }
            throw LLMError("Apple Intelligence: \(error.localizedDescription)")
        }
    }
}
