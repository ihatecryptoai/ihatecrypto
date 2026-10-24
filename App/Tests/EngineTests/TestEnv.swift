#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
@testable import DesktopEngine
import Foundation
#if os(Windows)
import WinSDK
#endif

/// Sets a variable of this test process: Windows has no `setenv` (VaultTests.setEnv does the same for the signer).
func setEnv(_ name: String, _ value: String) {
    #if os(Windows)
    _ = name.withCString(encodedAs: UTF16.self) { n in value.withCString(encodedAs: UTF16.self) { SetEnvironmentVariableW(n, $0) } }
    #else
    setenv(name, value, 1)
    #endif
}

/// A task list as the engine's own save leaves it — tasks.json, its key under this user's DPAPI (the stand-in's off
/// Windows) and the tag (TaskSeal) — for a test to seed: a list seeded bare loads paused, as one changed outside the app.
func sealedTasks(_ json: Data) -> [String: Data] {
    let key = SymmetricKey(size: .bits256)
    return ["tasks.json": json, "task-seal.key": Store.protect(key.withUnsafeBytes { Data($0) }) ?? Data(), "tasks.seal": TaskSeal.tag(json, key)]
}
