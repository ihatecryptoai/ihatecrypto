import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// Settings › Modules, as on the Mac (ModulesUI.swift): the switches per network, kind of move and protocol. The shared
/// code reads them from UserDefaults (`Modules.current`, the batch switch, the contract cap), so the engine keeps them in
/// settings.json and puts them there at launch and after every change; «off» is then refused everywhere the Mac refuses
/// it — the chat's forms (`c.off`), every wallet command, trades, the schedule, the bridges and money searches.
extension Settings {
    /// What the shared code reads, from what the person chose here. Into this process's registration domain only: it's
    /// never written to disk, so another engine of the same program (a second copy, a test run beside it) can't change
    /// what this one refuses; whatever an earlier build left on disk under these keys is taken away first.
    func mirror() {
        let d = UserDefaults.standard
        for key in [Modules.key, Modules.knownKey, Executor.batchKey, ContractCallGuard.capKey] { d.removeObject(forKey: key) }
        var values: [String: Any] = [Modules.key: modules ?? "", Executor.batchKey: batches == true, ContractCallGuard.capKey: contractCap ?? 50]
        if let modulesKnown { values[Modules.knownKey] = modulesKnown }
        d.register(defaults: values)
    }
}

/// What Settings › Modules draws: every switch with the words the Mac shows beside it.
nonisolated struct ModulesView: Encodable, Equatable {
    struct Network: Encodable, Equatable { var id, name, uses, logo, users: String; var on: Bool }
    struct Item: Encodable, Equatable {
        var id, name, line, logo: String
        var on: Bool
        /// It works: its own switch, its kind's and its networks' are on.
        var allowed: Bool
        /// One of its networks is off (the line says which).
        var networksOff: Bool
    }
    struct Kind: Encodable, Equatable { var id, key, name, line: String; var on: Bool; var protocols: [Item] }
    var networks: [Network]
    var kinds: [Kind]
}

extension Engine {
    func modulesView() -> ModulesView {
        let m = Modules.current
        return ModulesView(
            networks: ModuleNetwork.allCases.map { n in
                let users = ModuleProtocol.allCases.filter { $0.runs(on: n) }.map(\.name)
                return .init(id: "network.\(n.rawValue)", name: n.name, uses: n.uses, logo: n.logo,
                             users: users.isEmpty ? say("Balances only", "Только балансы") : say("Used by ", "Нужна для: ") + users.joined(separator: ", "),
                             on: m.isOn(n))
            },
            kinds: ModuleKind.allCases.map { k in
                .init(id: "kind.\(k.rawValue)", key: k.rawValue, name: k.name, line: k.line, on: m.isOn(k),
                      protocols: ModuleProtocol.allCases.filter { $0.kind == k }.map { p in
                          let line = Self.moduleLine(p, m)
                          return .init(id: "protocol.\(p.rawValue)", name: p.name, line: line.text, logo: p.logo, on: m.isOn(p),
                                       allowed: m.allows(p), networksOff: line.networksOff)
                      })
            })
    }

    /// The line under a protocol: where it works, or which of its networks are off (ModulesSettings.line).
    static func moduleLine(_ p: ModuleProtocol, _ m: Modules) -> (text: String, networksOff: Bool) {
        let used = ModuleNetwork.allCases.filter { p.runs(on: $0) }
        let off = used.filter { !m.isOn($0) }
        let names = off.map(\.name).joined(separator: ", "), verb = off.count == 1 ? "is" : "are"
        let text = off.isEmpty ? p.place
            : off.contains(where: p.networks.contains) || off.count == used.count ? "\(names) \(verb) off"
            : "\(p.place) · \(names) \(verb) off, the rest work"
        return (text, !off.isEmpty)
    }

    /// One switch: «network.base», «kind.perps», «protocol.kamino».
    func setModule(_ id: String?, on: Bool) throws {
        guard let id, Modules.allIDs.contains(id) else { throw WalletError("Unknown module «\(id ?? "")».") }
        var m = Modules.current
        m.choices[id] = on
        settings.modules = m.stored
    }

    // MARK: New modules (Workspace.offerNewModules)

    static let moduleOn = "turn on module ", moduleKeepOff = "keep the new modules off"

    /// A module a later version brings starts off and is asked about once, in the chat. The first run records every
    /// module there is, so nothing the person already has changes.
    func offerNewModules() {
        guard settings.modulesKnown != nil else { settings.modulesKnown = Modules.allIDs; return }
        let new = Modules.current.newIDs
        // Not during the first run (the Mac's ModulesUI.swift:334): the question waits for the next launch.
        guard !new.isEmpty, card == nil, !onboarding else { return }
        reply(say("New in ihatecrypto: \(new.map(Modules.name).joined(separator: ", ")). It stays off until you say — turn it on?",
                  "Новое в ihatecrypto: \(new.map(Modules.name).joined(separator: ", ")). Пока выключено — включить?"))
        options = new.prefix(4).map { id in
            ChatRouter.Option(title: say("Turn on \(Modules.name(id))", "Включить \(Modules.name(id))"), comment: say("Settings › Modules", "Настройки › Модули"), command: Self.moduleOn + id)
        } + [ChatRouter.Option(title: say("Keep it off", "Оставить выключенным"),
                               comment: say("Settings › Modules turns it on any time", "Включить можно в Настройках › Модули"), command: Self.moduleKeepOff)]
    }

    /// The answer to that question; false for any other card.
    func answerNewModules(_ command: String) -> Bool {
        guard command.hasPrefix(Self.moduleOn) || command == Self.moduleKeepOff else { return false }
        var m = Modules.current
        if command.hasPrefix(Self.moduleOn) {
            let id = String(command.dropFirst(Self.moduleOn.count))
            // Only a module asked about here: a card the model wrote can't turn on what the person switched off.
            guard m.newIDs.contains(id) else {
                reply(say("That one is switched in Settings › Modules.", "Это включается в Настройках › Модули."))
                return true
            }
            m.choices[id] = true
            reply(say("\(Modules.name(id)) is on.", "\(Modules.name(id)) включено."))
        } else {
            for id in m.newIDs { m.choices[id] = false }
            reply(say("Left off. Settings › Modules turns it on any time.", "Оставил выключенным. Включить можно в Настройках › Модули."))
        }
        settings.modules = m.stored
        return true
    }
}

extension ModuleProtocol {
    /// Whether it works on `n`: Uniswap, Aave and Morpho check the network per move, so their place names count too.
    func runs(on n: ModuleNetwork) -> Bool { networks.contains(n) || place.contains(n.name) }
}
