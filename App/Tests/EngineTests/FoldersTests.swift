import Foundation
import Testing

/// The canvas list's own order, as the Mac's CanvasList: a new canvas is kept only once something is on it, folders are
/// made, merged on a rename and deleted with their canvases kept, at most five canvases pin to the top, a picture is
/// checked before it's kept, and all of it survives a restart.
@Suite(.serialized) struct FoldersTests {
    static func rows(_ state: [String: Any]?) -> [[String: Any]] { state?["canvases"] as? [[String: Any]] ?? [] }
    static func row(_ state: [String: Any]?, _ id: String) -> [String: Any]? { rows(state).first { $0["id"] as? String == id } }
    static func folders(_ state: [String: Any]?) -> [String] { (state?["folders"] as? [[String: Any]] ?? []).compactMap { $0["name"] as? String } }

    /// A canvas with a block on it, kept.
    static func kept(_ engine: EngineTests.Engine) throws -> String {
        let made = try engine.call("newCanvas")
        let id = try #require(made.state?["canvas"] as? String)
        try engine.call("addBoard", ["name": "receive"])
        return id
    }

    @Test func foldersPinsAndPicturesAsOnTheMac() throws {
        let engine = try EngineTests.Engine()
        let hello = try engine.call("hello")
        let first = try #require(hello.state?["canvas"] as? String)
        try engine.call("addBoard", ["name": "receive"])

        // An empty canvas left for another one goes; it's shown as the new canvas while it's open.
        let fresh = try engine.call("newCanvas")
        let empty = try #require(fresh.state?["canvas"] as? String)
        #expect(Self.row(fresh.state, empty)?["empty"] as? Bool == true)
        let next = try engine.call("newCanvasIn", ["name": "  Ideas  "])
        let inIdeas = try #require(next.state?["canvas"] as? String)
        #expect(Self.row(next.state, empty) == nil, "an empty canvas stayed")
        #expect(Self.row(next.state, inIdeas)?["folder"] as? String == "Ideas" && Self.folders(next.state) == ["Ideas"])
        try engine.call("addBoard", ["name": "receive"])

        // A folder made by a move; a rename onto another folder merges them, pictures kept.
        try engine.call("moveCanvas", ["wallet": first, "name": "Trading"])
        let emoji = String(data: try JSONSerialization.data(withJSONObject: ["kind": "emoji", "value": "📈"]), encoding: .utf8)!
        try engine.call("setIcon", ["name": "Trading", "text": emoji])
        let merged = try engine.call("renameFolder", ["label": "Trading", "name": "Ideas"])
        #expect(Self.folders(merged.state) == ["Ideas"] && Self.row(merged.state, first)?["folder"] as? String == "Ideas")
        #expect(((merged.state?["folders"] as? [[String: Any]])?.first?["icon"] as? [String: Any])?["value"] as? String == "📈")

        // A picture is one of the library's or a small PNG of the window's own; anything else is refused.
        for bad in [["kind": "file", "value": "x"], ["kind": "image", "value": "https://example.com/a.png"], ["kind": "logo", "value": "../../x"]] {
            let text = String(data: try JSONSerialization.data(withJSONObject: bad), encoding: .utf8)!
            #expect(try engine.call("setIcon", ["wallet": first, "text": text]).answer["ok"] as? Bool == false, "\(bad) was kept")
        }
        let sol = String(data: try JSONSerialization.data(withJSONObject: ["kind": "logo", "value": "sol"]), encoding: .utf8)!
        let pictured = try engine.call("setIcon", ["wallet": first, "text": sol])
        #expect((Self.row(pictured.state, first)?["icon"] as? [String: Any])?["value"] as? String == "sol")

        // Five pin; the sixth is refused with what to do.
        var ids = [first, inIdeas]
        for _ in 0..<4 { ids.append(try Self.kept(engine)) }
        for id in ids.prefix(5) { #expect(try engine.call("pinCanvas", ["wallet": id, "test": true]).answer["ok"] as? Bool == true) }
        let sixth = try engine.call("pinCanvas", ["wallet": ids[5], "test": true])
        #expect(sixth.answer["ok"] as? Bool == false && (sixth.answer["error"] as? String)?.contains("unpin one first") == true)

        // Deleting a folder keeps its canvases, out of any folder.
        let gone = try engine.call("deleteFolder", ["name": "Ideas"])
        #expect(Self.folders(gone.state).isEmpty && Self.row(gone.state, first)?["folder"] == nil && Self.row(gone.state, inIdeas) != nil)
        try engine.call("createFolder", ["name": "Taxes"])

        // A restart reads it all back: the pins in their order, the picture, the emptied folder.
        let folder = engine.dir.appendingPathComponent("engine")
        let again = try EngineTests.Engine(seed: ["canvases.json": try Data(contentsOf: folder.appendingPathComponent("canvases.json")),
                                                  "folders.json": try Data(contentsOf: folder.appendingPathComponent("folders.json"))])
        let back = try again.call("hello")
        #expect(Self.rows(back.state).filter { $0["pinnedAt"] != nil }.count == 5)
        #expect((Self.row(back.state, first)?["icon"] as? [String: Any])?["kind"] as? String == "logo")
        #expect(Self.folders(back.state) == ["Taxes"])
    }
}
