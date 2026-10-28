// Presses a button in the development copy's window through Accessibility, the way a person would: what a live run on
// the Dev test wallet needs to press Confirm without keystrokes. Only «ihatecrypto Dev» (app.ihatecrypto.mac.dev):
// it never reaches the installed app. The terminal needs the Accessibility right (System Settings › Privacy & Security).
//   swiftc -O App/Tools/axpress.swift -o App/.build/axpress   (Tools/live.sh does it)
//   axpress list [part of a title]   the buttons on screen, their titles and descriptions
//   axpress press <exact title>      presses the first button with that title («Confirm», «Send anyway · ≈ $3.10 lost»)
import AppKit
import ApplicationServices

let args = CommandLine.arguments
guard args.count >= 2, ["list", "press"].contains(args[1]), args[1] == "list" || args.count >= 3 else {
    print("usage: axpress list [part of a title] | press <exact title>"); exit(2)
}
guard let dev = NSRunningApplication.runningApplications(withBundleIdentifier: "app.ihatecrypto.mac.dev").first else {
    print("ihatecrypto Dev isn't running"); exit(1)
}
guard AXIsProcessTrusted() else { print("this terminal has no Accessibility right: System Settings › Privacy & Security › Accessibility"); exit(1) }
let app = AXUIElementCreateApplication(dev.processIdentifier)
// SwiftUI builds its accessibility tree only for a client that asks for it.
AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}
func names(_ e: AXUIElement) -> [String] {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].compactMap { attr(e, $0) as? String }.filter { !$0.isEmpty }
}
var roots = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
if let main = attr(app, kAXMainWindowAttribute) { roots.append(main as! AXUIElement) }
var found: [AXUIElement] = []
func walk(_ e: AXUIElement, _ depth: Int) {
    guard depth < 60 else { return }
    let n = names(e)
    if attr(e, kAXRoleAttribute) as? String == "AXButton" {
        if args[1] == "list", args.count < 3 || n.contains(where: { $0.localizedCaseInsensitiveContains(args[2]) }) { print(n.joined(separator: " · ")) }
        if args[1] == "press", n.contains(args[2]) { found.append(e) }
    }
    for c in (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] { walk(c, depth + 1) }
}
for r in roots { walk(r, 0) }
if args[1] == "press" {
    // The card waits twice, under the chat's reply and on the canvas: both are the same Confirm.
    guard let e = found.first else { print("no button titled «\(args[2])»; axpress list shows what there is"); exit(1) }
    let r = AXUIElementPerformAction(e, kAXPressAction as CFString)
    print(r == .success ? "pressed «\(args[2])»" : "press failed: \(r.rawValue)")
    exit(r == .success ? 0 : 1)
}
