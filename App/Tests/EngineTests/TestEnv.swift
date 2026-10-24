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
