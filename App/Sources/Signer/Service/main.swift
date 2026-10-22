import Foundation
import SignerAPI

/// Signer: XPC service inside ihatecrypto.app. Sandboxed with no network entitlement, it holds the seed
/// in the data-protection Keychain under its own access group, checks every request with the guard and
/// only then signs. The app never sees the seed or a private key.
final class ServiceDelegate: NSObject, NSXPCListenerDelegate {
    static let app = Bundle.main.bundleIdentifier.flatMap { $0.hasSuffix(".signer") ? String($0.dropLast(7)) : nil } ?? "app.ihatecrypto.mac"

    /// The Apple team that signed this signer, read from its own signature; nil when it's unsigned or signed ad hoc.
    static let team: String? = {
        var me: SecCode?, onDisk: SecStaticCode?, info: CFDictionary?
        guard SecCodeCopySelf([], &me) == errSecSuccess, let me, SecCodeCopyStaticCode(me, [], &onDisk) == errSecSuccess, let onDisk,
              SecCodeCopySigningInformation(onDisk, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess
        else { return nil }
        return (info as? [String: Any])?[kSecCodeInfoTeamIdentifier as String] as? String
    }()

    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        // Only the app this signer is built into, signed by the same team as the signer, may talk to it:
        // app.ihatecrypto.mac for app.ihatecrypto.mac.signer; the development copy's signer (….dev.signer) takes only
        // the development copy. A signer without a team (unsigned, ad hoc) takes nobody.
        guard let team = Self.team else { return false }
        connection.setCodeSigningRequirement("identifier \"\(Self.app)\" and anchor apple generic and certificate leaf[subject.OU] = \"\(team)\"")
        connection.exportedInterface = NSXPCInterface(with: SignerXPC.self)
        connection.exportedObject = SignerService()
        connection.resume()
        return true
    }
}

final class SignerService: NSObject, SignerXPC {
    func handle(_ request: Data, reply: @escaping @Sendable (Data) -> Void) {
        var answer: SignerReply
        do { answer = try Signer.handle(try JSONDecoder().decode(SignerRequest.self, from: request)) }
        catch let wait as CodeNeeded {
            // The app asks for the passcode, then sends the same request again (SignerClient.send).
            answer = SignerReply(ok: false, error: wait.reason)
            answer.needsCode = true
        }
        catch { answer = SignerReply(ok: false, error: error.localizedDescription) }
        reply((try? JSONEncoder().encode(answer)) ?? Data())
    }
}

let delegate = ServiceDelegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()
