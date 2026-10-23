import Foundation
import WalletCore

/// A picture from the icon library (the Mac's AddressIcon): a token or network logo, one of the library's glyphs, an
/// emoji, or the person's own picture as a small PNG («data:image/png;base64,…», 128 px, made by the window).
nonisolated struct CanvasIcon: Codable, Equatable {
    var kind: String
    var value: String

    /// What the window may send: a known kind, a short name, an emoji, or a small picture of its own making.
    static func checked(_ icon: CanvasIcon) -> CanvasIcon? {
        switch icon.kind {
        case "logo", "symbol": icon.value.count <= 60 && icon.value.allSatisfy { $0.isLetter || $0.isNumber || ".-_".contains($0) } && !icon.value.isEmpty ? icon : nil
        case "emoji": (1...16).contains(icon.value.count) ? icon : nil
        // ponytail: kept inline in canvases.json and sent with every state; a file beside it if many pictures slow it.
        case "image": icon.value.hasPrefix("data:image/png;base64,") && icon.value.count <= 80_000 ? icon : nil
        default: nil
        }
    }
}

/// The folders' names and pictures: a folder lasts until it's deleted, even emptied (the Mac's `folders`).
nonisolated struct FolderStore: Codable, Equatable {
    var names: [String] = []
    var icons: [String: CanvasIcon] = [:]
}

/// A folder as the canvas list draws it.
nonisolated struct FolderView: Encodable, Equatable {
    var name: String
    var icon: CanvasIcon?
}

/// The canvas list's own order and folders, as the Mac's CanvasList: up to five canvases pinned on top, folders of
/// canvases, a picture for a canvas or a folder, the model each was talked through, and a new canvas kept only once
/// something is on it.
extension Engine {
    /// At most this many canvases stay pinned to the top of the list.
    static let maxPinned = 5

    var canvasFolders: [String] {
        Array(Set(folderStore.names + canvases.compactMap(\.folder))).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    var folderViews: [FolderView] { canvasFolders.map { FolderView(name: $0, icon: folderStore.icons[$0]) } }

    var canvasRows: [Snapshot.CanvasRow] {
        canvases.map { c in
            .init(id: c.id, title: c.title, at: c.messages.last?.at ?? c.at, pinnedAt: c.pinnedAt, folder: c.folder, icon: c.icon,
                  provider: c.provider, model: c.model, empty: c.schedule == nil && c.messages.isEmpty && c.boards.isEmpty, blocks: !c.boards.isEmpty)
        }
    }

    /// The window's folder and pin requests, one op each.
    func folderOp(_ r: Request) async throws -> [String: String] {
        switch r.op {
        case "siteIcon": return try await siteIcon(r.address ?? "")
        case "pinCanvas": try pinCanvas(r.wallet ?? "", on: r.test == true)
        case "moveCanvas": moveCanvas(r.wallet ?? "", to: r.name)
        case "createFolder": createFolder(r.name ?? "")
        case "renameFolder": renameFolder(r.label ?? "", to: r.name ?? "")
        case "deleteFolder": deleteFolder(r.name ?? "")
        case "newCanvasIn":
            guard let folder = Self.folderName(r.name ?? "") else { return [:] }
            try newCanvas()
            canvases[canvasIndex].folder = folder
            createFolder(folder)
            saveCanvases()
        case "setIcon":
            let icon = try r.text.flatMap { $0.isEmpty ? nil : try JSONDecoder().decode(CanvasIcon.self, from: Data($0.utf8)) }
            if let icon, CanvasIcon.checked(icon) == nil { throw WalletError(say("That picture can't be used.", "Эту картинку не получится поставить.")) }
            if let id = r.wallet, let i = canvases.firstIndex(where: { $0.id == id }) {
                canvases[i].icon = icon
                saveCanvases()
            } else if let name = r.name, canvasFolders.contains(name) {
                folderStore.icons[name] = icon
            }
        default: break
        }
        changed()
        return [:]
    }

    /// A site's own icon for the picker's «Logo from a website», asked of the site itself as the Mac does (a favicon
    /// service would learn the names): its touch icon, then its favicon. The window scales it to 128 px.
    func siteIcon(_ domain: String) async throws -> [String: String] {
        var host = domain.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        if let r = host.range(of: "://") { host = String(host[r.upperBound...]) }
        host = host.split(separator: "/").first.map(String.init) ?? ""
        guard host.contains("."), host.count <= 253, host.allSatisfy({ $0.isLetter || $0.isNumber || ".-".contains($0) }) else {
            throw WalletError(say("Type a site's address, like bybit.com.", "Напиши адрес сайта, например bybit.com."))
        }
        for path in ["apple-touch-icon.png", "favicon.ico"] {
            guard let url = URL(string: "https://\(host)/\(path)") else { continue }
            guard let (data, response) = try? await URLSession.shared.data(for: URLRequest(url: url, timeoutInterval: 8)),
                  (response as? HTTPURLResponse)?.statusCode == 200, (100..<512_000).contains(data.count) else { continue }
            // A picture by its first bytes, not a page answering for one.
            let type = data.starts(with: [0x89, 0x50, 0x4E, 0x47]) ? "image/png" : data.starts(with: [0, 0, 1, 0]) ? "image/x-icon" : nil
            guard let type else { continue }
            return ["image": "data:\(type);base64," + data.base64EncodedString()]
        }
        throw WalletError(say("No icon found for \(host).", "У \(host) не нашлось иконки."))
    }

    /// Pinning past the limit is refused with what to do instead.
    func pinCanvas(_ id: String, on: Bool) throws {
        guard let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        if on, canvases[i].pinnedAt == nil, canvases.filter({ $0.pinnedAt != nil }).count >= Self.maxPinned {
            throw WalletError(say("\(Self.maxPinned) canvases are pinned already: unpin one first.", "Уже закреплено \(Self.maxPinned) холстов: сначала открепи один."))
        }
        canvases[i].pinnedAt = on ? (canvases[i].pinnedAt ?? Date()) : nil
        saveCanvases()
    }

    /// nil or an empty name takes it out of its folder; a new name makes the folder. Both folders stay in the list.
    func moveCanvas(_ id: String, to folder: String?) {
        guard let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        let to = folder.flatMap(Self.folderName)
        for f in [canvases[i].folder, to].compactMap({ $0 }) { createFolder(f) }
        canvases[i].folder = to
        saveCanvases()
    }

    func createFolder(_ name: String) {
        if let f = Self.folderName(name), !folderStore.names.contains(f) { folderStore.names.append(f) }
    }

    /// Its canvases go with it, merging into a folder of that name if one is there; its picture too, unless that one has its own.
    func renameFolder(_ old: String, to name: String) {
        guard let new = Self.folderName(name), new != old else { return }
        for i in canvases.indices where canvases[i].folder == old { canvases[i].folder = new }
        var store = folderStore
        store.names = store.names.filter { $0 != old && $0 != new } + [new]
        if let icon = store.icons.removeValue(forKey: old), store.icons[new] == nil { store.icons[new] = icon }
        folderStore = store
        saveCanvases()
    }

    /// The folder goes; its canvases stay, out of any folder.
    func deleteFolder(_ name: String) {
        for i in canvases.indices where canvases[i].folder == name { canvases[i].folder = nil }
        var store = folderStore
        store.names.removeAll { $0 == name }
        store.icons[name] = nil
        folderStore = store
        saveCanvases()
    }

    /// The canvas being left had nothing on it: it goes, as the Mac keeps a new canvas only once it's written on. A
    /// scheduled item's own canvas stays: it leaves the list when its item view closes (the live part's).
    func forgetUnkept() {
        let c = canvases[canvasIndex]
        guard c.schedule == nil, c.messages.isEmpty, c.boards.isEmpty, canvases.count > 1 else { return }
        canvases.remove(at: canvasIndex)
    }

    /// The model that answered on the open canvas: its company's logo on the row, its name in the row's tooltip.
    func canvasTalked(provider: String) {
        let i = canvasIndex
        guard canvases[i].provider != provider || canvases[i].model != model else { return }
        canvases[i].provider = provider
        canvases[i].model = model.isEmpty ? nil : model
        saveCanvases()
    }

    nonisolated static func folderName(_ raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : String(trimmed.prefix(40))
    }
}
