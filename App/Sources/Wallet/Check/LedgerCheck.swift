import EVMChain
import Foundation
import HyperliquidSigning
import Ledger
import SolanaChain
import TradeExecution
import WalletCore

/// `ledger [--speculos|--ble]`: the app open on the Ledger, its address, blind signing (USB unless told otherwise).
/// `ledger sign [--speculos] [--shots=<dir>]`: the same requests a move makes, signed on the device through
/// `LedgerSigner` (guard first, the signature checked against the device's address after). With --speculos the
/// emulator's buttons are pressed through its API and each screen is printed (and saved as PNG with --shots).
/// Nothing is sent to any network. Run the emulator with Tools/speculos.sh eth|sol.
func ledgerCheck(_ args: [String]) async throws {
    let speculos = args.contains("--speculos")
    let sign = args.contains("sign")
    let hand = speculos ? SpeculosHand(shots: args.first { $0.hasPrefix("--shots=") }.map { URL(fileURLWithPath: String($0.dropFirst(8))) }) : nil
    let ble = args.contains("--ble")
    let transport: any LedgerTransport = if speculos { LedgerTCPTransport() } else if ble { LedgerBLE.shared } else { LedgerHID() }
    let device = LedgerDevice(transport)
    if ble { print("Bluetooth: \(await LedgerBLE.shared.connect(timeout: 30) ?? "no Ledger answered")") }
    else if !speculos { print("USB: \(LedgerHID.connected() ?? "no Ledger found")") }
    let app = try await device.openApp()
    print("Open app: \(app.name) \(app.version)")
    // While the device waits for the person, the emulator's hand reads the screen and presses its buttons.
    let pressing = PressingTask()
    let prompt: @Sendable (String?) -> Void = { text in
        print("  ▸ \(text ?? "done")")
        if let hand, text?.hasPrefix("Review") == true { pressing.start { await hand.approve() } } else if text == nil { pressing.stop() }
    }
    switch app.name {
    case "Ethereum":
        let address = try await device.evmAddress()
        let blind = try await device.evmBlindSigning()
        print("EVM address \(LedgerDevice.evmPath): \(address)\nBlind signing: \(blind.on ? "on" : "off")")
        guard sign else { return }
        let signer = LedgerSigner(device: device, wallet: TradeWallet(solana: "", evm: address), prompt: prompt)
        let plain = try await signer.signEVM(chainId: 8453, nonce: 7, maxPriorityFee: 1_000_000, maxFee: 20_000_000, gasLimit: 21_000,
                                             to: address, data: Data(), recipients: [])
        print("✓ Base transaction signed and recovered to \(address): 0x\(plain.hex.prefix(24))… (\(plain.count) bytes)")
        let usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", aave = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
        do {
            let main = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
            let send = try await signer.signEVM(chainId: 8453, nonce: 10, maxPriorityFee: 1_000_000, maxFee: 20_000_000, gasLimit: 65_000,
                                                to: usdc, data: try ERC20.transfer(to: main, amount: 25_000_000), recipients: [main])
            print("✓ 25 USDC to Main on Base signed: 0x\(send.hex.prefix(24))…")
        } catch { print("✗ USDC transfer: \(error.localizedDescription)") }
        do {
            let approve = try await signer.signEVM(chainId: 8453, nonce: 8, maxPriorityFee: 1_000_000, maxFee: 20_000_000, gasLimit: 60_000,
                                                   to: usdc, data: try ERC20.approve(spender: aave, amount: 2_000_000), recipients: [])
            print("✓ Exact approve of 2 USDC to Aave signed: 0x\(approve.hex.prefix(24))…")
        } catch { print("✗ Exact approve: \(error.localizedDescription)") }
        do {
            let order = try await signer.signHyperliquid(HLAction.cancel([(asset: 4, oid: 1)]), nonce: 1_760_000_000_000)
            print("✓ Hyperliquid action signed as typed data, recovered to \(address): r=0x\(order.r.hex.prefix(16))… v=\(order.v)")
        } catch { print("✗ Hyperliquid action: \(error.localizedDescription)") }
        // The trading key's one approval: the device shows the key's address.
        do {
            _ = try await signer.approveAgent("0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf", name: "ihatecrypto", nonce: 1_760_000_000_001)
            print("✓ approveAgent signed on the device, recovered to \(address)")
        } catch { print("✗ approveAgent: \(error.localizedDescription)") }
        // A withdrawal is always the device's: destination and amount on its screen.
        do {
            let main = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
            _ = try await signer.signHyperliquidUser(HLSigner.usdSendAction(destination: main, amount: "25", time: 1_760_000_000_002, mainnet: true), recipients: [main])
            print("✓ usdSend of 25 USDC to Main signed on the device")
        } catch { print("✗ usdSend: \(error.localizedDescription)") }
        // ihatecrypto's fee on Hyperliquid: approved and taken back on the device; the referral code as typed data.
        for (rate, n) in [(HLBuilder.rate, UInt64(1_760_000_000_004)), (HLBuilder.revoke, 1_760_000_000_005)] {
            do {
                let action = HLBuilder.approveAction(maxFeeRate: rate, nonce: n, mainnet: true)
                let sig = try await signer.signHyperliquidUser(action, recipients: [])
                let (type, fields) = try HLSigner.userAction(action)
                let who = try EVMSigner.recover(digest: try HLSigner.userParts(type, fields: fields, mainnet: true).digest, signature: sig)
                print("✓ approveBuilderFee \(rate) signed on the device, recovered to \(who)")
            } catch { print("✗ approveBuilderFee \(rate): \(error.localizedDescription)") }
        }
        do {
            _ = try await signer.signHyperliquid(HLBuilder.referrerAction, nonce: 1_760_000_000_006)
            print("✓ setReferrer \(HLBuilder.referralCode) signed on the device")
        } catch { print("✗ setReferrer: \(error.localizedDescription)") }
        do {
            let foreign = MsgPack.map([("type", .string("approveBuilderFee")), ("maxFeeRate", .string(HLBuilder.rate)),
                                       ("builder", .string("0x000000000000000000000000000000000000dead")), ("nonce", .uint(1_760_000_000_007))])
            _ = try await signer.signHyperliquidUser(foreign, recipients: [])
            print("✗ A fee approval for a stranger was signed!")
        } catch { print("✓ Fee approval for a stranger refused before the device: \(error.localizedDescription)") }
        // With a trading key, orders don't reach the device at all.
        let withAgent = LedgerSigner(device: device, wallet: TradeWallet(solana: "", evm: address),
                                     agent: { action, nonce in try HLSigner.signL1(action, privateKey: Data(repeating: 1, count: 32), nonce: nonce, mainnet: true) },
                                     prompt: prompt)
        let byAgent = try await withAgent.signHyperliquid(HLAction.cancel([(asset: 4, oid: 1)]), nonce: 1_760_000_000_003)
        print("✓ With a trading key the order is signed without the device: \(try EVMSigner.recover(digest: try HLSigner.l1Parts(HLAction.cancel([(asset: 4, oid: 1)]), nonce: 1_760_000_000_003, mainnet: true).digest, signature: byAgent))")
        // The guard stops a transfer to a stranger before the device is even asked.
        do {
            _ = try await signer.signEVM(chainId: 8453, nonce: 9, maxPriorityFee: 1, maxFee: 1, gasLimit: 60_000, to: usdc,
                                         data: try ERC20.transfer(to: "0x000000000000000000000000000000000000dEaD", amount: 1), recipients: [])
            print("✗ A transfer to a stranger was signed!")
        } catch { print("✓ Refused before the device: \(error.localizedDescription)") }
    case "Solana":
        let address = try await device.solanaAddress()
        let blind = try await device.solanaBlindSigning()
        print("Solana address \(LedgerDevice.solanaPath): \(address)\nBlind signing: \(blind.on ? "on" : "off")")
        guard sign else { return }
        let signer = LedgerSigner(device: device, wallet: TradeWallet(solana: address, evm: ""), prompt: prompt)
        let to = "3YqEpfo3c818GhvbQ1UmVY1nJxw16vtu4JB9peJXT94k"
        let accounts = [address, to, "11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111"]
        let wire = try unsignedTransfer(accounts: accounts, lamports: 1_000_000)
        let signed = try await signer.signSolana(wire, accounts: accounts, recipients: [to], tokenOwners: [:])
        print("✓ 0.001 SOL transfer signed, ed25519 checked against \(address): \(Base58.encode(signed.dropFirst().prefix(64)).prefix(20))…")
        // «send 25 USDC to Main» from the Ledger: the same message the app builds (its account created if missing).
        let mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        let message = try SPL.transferMessage(payer: address, to: to, mint: mint, amount: 25_000_000, decimals: 6,
                                              blockhash: Base58.encode(Data(repeating: 7, count: 32)))
        let usdcWire = Data([1]) + Data(count: 64) + message
        let keys = try SolanaMessage.parse(transaction: usdcWire).staticKeys
        let owners = [try SPL.associatedAccount(owner: address, mint: mint): address, try SPL.associatedAccount(owner: to, mint: mint): to]
        do {
            _ = try await signer.signSolana(usdcWire, accounts: keys, recipients: [to], tokenOwners: owners)
            print("✓ 25 USDC transfer signed")
        } catch { print("✗ USDC transfer: \(error.localizedDescription)") }
        do {
            _ = try await signer.signSolana(wire, accounts: accounts, recipients: [], tokenOwners: [:])
            print("✗ A transfer to an unapproved address was signed!")
        } catch { print("✓ Refused before the device: \(error.localizedDescription)") }
    default:
        print("Open the Ethereum or Solana app on the Ledger.")
    }
}

/// A legacy SOL transfer with a priority fee, unsigned: one empty signature slot, then the message.
func unsignedTransfer(accounts: [String], lamports: UInt64) throws -> Data {
    let keys = try accounts.map { try Base58.decode($0) }
    func le<T: FixedWidthInteger>(_ n: T) -> Data { withUnsafeBytes(of: n.littleEndian) { Data($0) } }
    var message = Data([1, 0, 2, 4]) + keys.reduce(Data(), +) + Data(repeating: 7, count: 32) + Data([2])
    message += Data([3, 0, 9, 3]) + le(UInt64(20_000))                       // ComputeBudget.SetComputeUnitPrice
    message += Data([2, 2, 0, 1, 12]) + le(UInt32(2)) + le(lamports)         // System.Transfer
    return Data([1]) + Data(count: 64) + message
}

/// One running press loop at a time.
final class PressingTask: @unchecked Sendable {
    private var task: Task<Void, Never>?
    private let lock = NSLock()
    func start(_ body: @escaping @Sendable () async -> Void) { lock.withLock { task?.cancel(); task = Task { await body() } } }
    func stop() { lock.withLock { task?.cancel(); task = nil } }
}

/// Speculos's REST API as a person's hand: reads the screen, presses right through a review and both buttons on the
/// page that approves it.
actor SpeculosHand {
    let api = URL(string: "http://127.0.0.1:5000")!
    let shots: URL?
    var count = 0
    var last = ""
    init(shots: URL?) {
        self.shots = shots
        if let shots { try? FileManager.default.createDirectory(at: shots, withIntermediateDirectories: true) }
    }

    func screen() async -> [String] {
        guard let (data, _) = try? await URLSession.shared.data(from: api.appending(path: "events").appending(queryItems: [.init(name: "currentscreenonly", value: "true")])),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let events = json["events"] as? [[String: Any]] else { return [] }
        return events.compactMap { $0["text"] as? String }
    }

    func press(_ button: String) async {
        var request = URLRequest(url: api.appending(path: "button/\(button)"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data(#"{"action":"press-and-release"}"#.utf8)
        _ = try? await URLSession.shared.data(for: request)
    }

    func approve() async {
        try? await Task.sleep(for: .milliseconds(600))
        var pages = 0
        while !Task.isCancelled, pages < 40 {
            let texts = await screen()
            let joined = texts.joined(separator: " | ")
            if joined != last {
                last = joined
                count += 1
                print("    screen \(count): \(joined)")
                if let shots, let (png, _) = try? await URLSession.shared.data(from: api.appending(path: "screenshot")) {
                    try? png.write(to: shots.appending(path: String(format: "%02d.png", count)))
                }
            }
            // «Sign transaction», «Approve»…, and a warning page that says to press both buttons to go on.
            // Not a bare «Approve»: that labels an ERC-20 approval's amount page.
            let approves = texts.contains { $0.wholeMatch(of: /(?i)\s*(sign\s+(transaction|typed message|message|operation)\??|confirm)\s*/) != nil }
                || texts.joined(separator: " ").lowercased().split(separator: " ").joined(separator: " ").contains("press both buttons")
            await press(approves ? "both" : "right")
            pages += 1
            try? await Task.sleep(for: .milliseconds(approves ? 800 : 250))
        }
    }
}
