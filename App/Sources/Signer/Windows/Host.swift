import CWinSec
import Foundation
import SignerAPI
import WalletCore

/// The signer's door on Windows: a request's JSON in, its reply's JSON out (SignerRequest, SignerReply — the same as
/// the Mac's XPC), one at a time, over the pipes of the engine that started it.
public enum SignerHost {
    /// What `winsec_harden` couldn't do, said in every status reply, so the app can show it.
    nonisolated(unsafe) public static var notes: [String] = []

    public static func handle(_ body: Data) -> Data {
        var answer: SignerReply
        do {
            let request = try JSONDecoder().decode(SignerRequest.self, from: body)
            answer = try Vault.$scope.withValue(Vault.Scope()) { try Signer.handle(request) }
            if case .status = request { answer.checks += notes + presence() }
        } catch {
            answer = SignerReply(ok: false, error: error.localizedDescription)
        }
        return (try? JSONEncoder().encode(answer)) ?? Data()
    }

    /// Whether Windows Hello can guard a wallet here, and whether its key is in the TPM.
    static func presence() -> [String] {
        guard winsec_hello_available() == WINSEC_OK else { return ["Windows Hello isn't set up: only a test wallet can be made."] }
        guard Vault.master() != nil else { return ["Windows Hello is ready."] }
        let name = Array(Vault.helloKey.utf8)
        return winsec_hello_in_tpm(name, name.count) == WINSEC_OK
            ? ["Windows Hello is ready; its key is in the TPM."]
            : ["Windows Hello is ready, but Windows can't show its key is in a TPM: it may be kept in software."]
    }

    /// The hardening the process got, in words.
    public static func describe(hardening failed: Int32) -> [String] {
        var notes: [String] = []
        if failed & 1 != 0 { notes.append("Other programs may be able to read the signer's memory: its process rule wasn't set.") }
        if failed & (2 | 4) != 0 { notes.append("Other programs may be able to steer the signer's threads: their rule wasn't set.") }
        if failed & 8 != 0 { notes.append("Some of Windows' protections for the signer (no extension DLLs, no child processes) weren't turned on.") }
        if winsec_folder_protected() != WINSEC_OK, winsec_is_stub() == 0 {
            notes.append("ihatecrypto is installed in a folder your Windows user can write to: install it for all users so no program can replace it.")
        }
        return notes
    }
}
