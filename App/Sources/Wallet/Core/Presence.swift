import Foundation
import LocalAuthentication

/// How this Mac asks for the person before a signature: Touch ID where it has the sensor, its login password
/// otherwise (`.deviceOwnerAuthentication` falls back to it by itself). Texts name the one this Mac uses.
public enum Presence {
    /// ponytail: read once per launch — a MacBook whose lid closes after launch still says Touch ID while macOS asks
    /// the password; read it on each use if that matters.
    public static let touchID: Bool = {
        #if DEBUG
        // `-noTouchID YES`: a Mac without the sensor, to read its texts on one that has it.
        if UserDefaults.standard.bool(forKey: "noTouchID") { return false }
        #endif
        var error: NSError?
        return LAContext().canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
    }()

    /// In a sentence: «asks for Touch ID» / «asks for your Mac password».
    public static var name: String { touchID ? "Touch ID" : "your Mac password" }
    /// On a label, or after «one»: «Sign · Touch ID» / «Sign · Mac password».
    public static var short: String { touchID ? "Touch ID" : "Mac password" }
    public static var icon: String { touchID ? "touchid" : "lock.fill" }
    /// Russian, by case: «спросит Touch ID / пароль от Mac».
    public static var ru: String { touchID ? "Touch ID" : "пароль от Mac" }
    /// «без Touch ID / пароля от Mac».
    public static var ruGen: String { touchID ? "Touch ID" : "пароля от Mac" }
    /// «подтверждено Touch ID / паролем от Mac».
    public static var ruIns: String { touchID ? "Touch ID" : "паролем от Mac" }
}
