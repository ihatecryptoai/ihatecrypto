import CryptoKit
import EVMChain
import Foundation
import HyperliquidSigning
import SignerAPI
import SolanaChain
import TxGuard
import WalletCore

/// Everything that needs the seed happens here, after the guard. The app is not trusted with more than it
/// must supply: approved recipients (the user approved them in the app) and lookup-table contents (the signer
/// has no network). Token-account owners it claims are re-derived here, and with presence on, the system
/// prompt shows what the signer itself parsed, so a lying app is visible to the user.
enum Signer {
    /// Public addresses and the signer's settings, out of the seed's way so status never touches it: in the Keychain
    /// (SignerPrefs), not the sandbox's preferences file, which anything on the Mac could rewrite.
    static var defaults: SignerPrefs { .shared }

    static func handle(_ request: SignerRequest) throws -> SignerReply {
        switch request {
        case .status:
            var r = SignerReply(ok: true)
            let wallets = Wallets.all
            r.hasWallet = !wallets.isEmpty
            r.wallets = wallets
            r.activeWallet = Wallets.active?.id
            r.solanaAddress = defaults.string(forKey: "solana")
            r.evmAddress = defaults.string(forKey: "evm")
            r.nearAddress = defaults.string(forKey: "near")
            r.agentAddress = AgentStore.load().flatMap { try? EVMAccount.address(privateKey: $0) }
            r.agentActive = defaults.bool(forKey: agentActiveKey) && r.agentAddress != nil
            r.sessionLength = SeedStore.sessionLength
            r.codeSet = CodeStore.isSet
            return r

        case .setCode(let code):
            if let problem = Passcode.problem(code) { throw WalletError(problem) }
            // In person every time, even with no wallet yet: a code set silently would sign for every wallet sealed under
            // it later. A Debug signer that holds only the public test wallet (its self-test) is the one exception.
            #if DEBUG
            let onlyPublicTest = !Wallets.all.isEmpty && Wallets.all.allSatisfy { $0.solana == publicTestSolana }
            #else
            let onlyPublicTest = false
            #endif
            if !onlyPublicTest {
                try SeedStore.confirm("save your new 6-digit passcode: it opens the app" + (Presence.touchID ? "" : " and signs your moves"))
            }
            defer { SeedStore.endSession() } // a confirmation, not a move: it leaves no session for an unrelated signature
            try CodeStore.set(code, presence: SeedStore.confirmed)
            var r = SignerReply(ok: true)
            r.codeSet = true
            return r

        case .clearCode:
            if CodeStore.isSet {
                try SeedStore.confirm("turn off your passcode")
                SeedStore.endSession()
                CodeStore.clear()
            }
            var r = SignerReply(ok: true)
            r.codeSet = false
            return r

        case .enterCode(let code, let sign):
            guard CodeStore.isSet else {
                var r = SignerReply(ok: false, error: "There is no passcode: your Mac password opens ihatecrypto.")
                (r.codeSet, r.codeOff) = (false, true)
                return r
            }
            switch CodeStore.enter(code) {
            case .right(let context):
                if sign, CodeStore.signs { SeedStore.startCodeSession(context) }
                var r = SignerReply(ok: true)
                r.codeSet = true
                return r
            case .wrong(let left):
                var r = SignerReply(ok: false, error: "Wrong code · \(left) \(left == 1 ? "try" : "tries") left")
                r.codeSet = true
                return r
            case .uncounted:
                var r = SignerReply(ok: false, error: "The signer couldn't count this try, so it didn't check the code: try again.")
                r.codeSet = true
                return r
            case .off:
                var r = SignerReply(ok: false, error: "\(CodeStore.tries) wrong codes: the code is off. Only your Mac password opens ihatecrypto and signs now.")
                (r.codeSet, r.codeOff) = (false, true)
                return r
            }

        case .enableAgent:
            let owner = try address("evm")
            var agentKey = AgentStore.load()
            let made = agentKey == nil
            if made { agentKey = try AgentStore.make() }
            let agent = try EVMAccount.address(privateKey: agentKey!)
            guard agent.lowercased() != owner.lowercased() else { throw WalletError("The agent key matches the main key.") }
            let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
            // In person every time: an open Touch ID session from another move doesn't turn the key on (the 2026-10-24 review).
            try confirmAfresh("approve a Hyperliquid trading key: orders without \(Presence.name) for 7 days; it can't withdraw money")
            let (action, signature) = try withKey("evm", reason: "approve a trading key: orders without \(Presence.name); it can't withdraw money") {
                try HLSigner.approveAgent(agent: agent, name: "ihatecrypto", nonce: nonce, privateKey: $0, mainnet: true)
            }
            // Kept only once its approval is signed: a Touch ID turned down leaves no key that Settings would call paused.
            if made { try AgentStore.save(agentKey!) }
            renewAgent(agentRenewedKey)
            SeedStore.endSession() // a confirmation, not a move: it leaves no session for an unrelated signature
            var r = SignerReply(ok: true)
            r.agentAddress = agent
            r.action = action
            r.signature = signature
            r.nonce = nonce
            r.checks = ["Approving an agent key created inside the signer: it can trade but not withdraw."]
            return r

        case .setAgentActive(let on):
            // Resuming a paused trading key is the user's decision: it asks for Touch ID like a signature.
            // So does one whose week is over (agentWeek): the app can't keep it going by turning it on again.
            if on, !defaults.bool(forKey: agentActiveKey) || !agentFresh(agentRenewedKey), AgentStore.load() != nil {
                // A fresh prompt, not an open session's: SeedStore.confirm always asks.
                try confirmAfresh("turn the Hyperliquid trading key on for 7 days: orders without \(Presence.name); it can't withdraw money")
                SeedStore.endSession() // a confirmation, not a move: it leaves no session for an unrelated signature
                renewAgent(agentRenewedKey)
            }
            defaults.set(on && AgentStore.load() != nil, forKey: agentActiveKey)
            var r = SignerReply(ok: true)
            r.agentActive = defaults.bool(forKey: agentActiveKey)
            return r

        case .createWallet(let requirePresence):
            // Without Touch ID it can only be a test wallet: a phrase made here and now, never one imported with money on it.
            let phrase = Mnemonic.generate()
            try confirmInPerson(requirePresence ? "add a new wallet to ihatecrypto and sign with it"
                                                : "add a TEST wallet that signs WITHOUT \(Presence.name) — keep only a few dollars on it")
            var r = try store(phrase: phrase, passphrase: "", requirePresence: requirePresence, test: !requirePresence)
            r.phrase = phrase
            return r

        case .createAutomationWallet:
            // Next to a Touch ID wallet only, and one: it's a budget beside the real money, never the only key.
            guard !Wallets.all.isEmpty, !Wallets.all.contains(where: \.isAutomation) else {
                throw WalletError(Wallets.all.isEmpty ? "Add your main wallet first." : "There is an automation wallet already.")
            }
            try confirmInPerson("add an AUTOMATION wallet: scheduled tasks sign with it WITHOUT \(Presence.name) — keep only their budget on it")
            return try store(phrase: Mnemonic.generate(), passphrase: "", requirePresence: false, test: true, automation: true)

        case .asWallet(let id, let inner):
            guard let wallet = Wallets.all.first(where: { $0.id == id }), wallet.isAutomation else {
                throw WalletError("Only the automation wallet signs for a scheduled task.")
            }
            switch inner {
            case .signSolana, .signEVMTransaction, .signEVMBatch, .signHyperliquidL1, .signHyperliquidUser, .signRelayNonceMapping,
                 .signReceiveAuthorization, .signRelayMulticallPermit, .signSponsoredTransfer, .signNearIntentDeposit, .signNearTransfer,
                 .relayOrder: break
            default: throw WalletError("The automation wallet signs only a task's moves.")
            }
            return try Wallets.$signing.withValue(wallet) { try handle(inner) }

        case .importMnemonic(let phrase, let passphrase, let requirePresence):
            _ = try Mnemonic.validate(phrase) // a mistyped phrase fails before any prompt
            try confirmInPerson("add a wallet from a recovery phrase to ihatecrypto and sign with it")
            return try store(phrase: phrase, passphrase: passphrase, requirePresence: requirePresence)

        case .addressBook:
            var r = SignerReply(ok: true)
            r.addresses = AddressBookStore.all()
            return r

        case .saveAddresses(let entries):
            guard !entries.isEmpty else { throw WalletError("Nothing to save.") }
            for e in entries {
                guard !e.name.trimmingCharacters(in: .whitespaces).isEmpty, let net = AddressCheck.Network(rawValue: e.network) else {
                    throw WalletError("An address needs a name and a network.")
                }
                if let problem = AddressCheck.problem(e.address, network: net) { throw WalletError(problem) }
                if let problem = AddressCheck.labelProblem(e.name) ?? AddressCheck.labelProblem(e.memo) { throw WalletError("\(e.address): \(problem)") }
            }
            // The whole address on the prompt, not its ends: a look-alike differs in the middle. It starts the line, before
            // the name, so no name can stand where an address should be.
            let lines = entries.map { e in
                "\(e.address) on \(AddressCheck.Network(rawValue: e.network)?.title ?? e.network) as «\(e.name)»" + (e.memo.isEmpty ? "" : ", memo \(e.memo)")
            }
            try SeedStore.confirm("save to your address book (check every character):\n" + lines.joined(separator: "\n"))
            defer { SeedStore.endSession() } // a confirmation, not a move: it leaves no session for an unrelated signature
            var book = AddressBookStore.all()
            for e in entries {
                book.removeAll { $0.name.lowercased() == e.name.lowercased() && $0.network == e.network }
                book.append(e)
            }
            try AddressBookStore.save(book)
            var r = SignerReply(ok: true)
            r.addresses = book
            return r

        case .removeAddress(let name, let network):
            var book = AddressBookStore.all()
            book.removeAll { $0.name.lowercased() == name.lowercased() && $0.network == network }
            try AddressBookStore.save(book)
            var r = SignerReply(ok: true)
            r.addresses = book
            return r

        case .importPrivateKey(let text):
            let imported = try ImportedKey.parse(text) // a malformed key fails before any prompt
            let address = try imported.address()
            try confirmInPerson("add the \(imported.chain) key of \(address) to ihatecrypto and sign with it")
            return switch imported {
            case .solana(let secret): try add(secret: secret, kind: "solana", solana: address, evm: nil, near: nil, requirePresence: true)
            case .evm(let secret): try add(secret: secret, kind: "evm", solana: nil, evm: address, near: nil, requirePresence: true)
            }

        case .setActiveWallet(let id):
            guard let wallet = Wallets.all.first(where: { $0.id == id }) else { throw WalletError("No such wallet.") }
            // The automation wallet signs only a task's moves, through asWallet: on, it would sign any request directly.
            guard !wallet.isAutomation else { throw WalletError("The automation wallet signs only scheduled tasks' moves; it can't be the wallet that's on.") }
            // Which wallet signs, and whose addresses ihatecrypto shows for deposits, is the user's call in person.
            if wallet.id != Wallets.active?.id {
                try confirmInPerson("sign with the wallet «\(wallet.label)» from now on:\n\(addresses(wallet))")
                SeedStore.endSession() // a confirmation, not a move: it leaves no session for an unrelated signature
            }
            Wallets.activate(wallet)
            var r = SignerReply(ok: true)
            r.wallets = Wallets.all
            r.activeWallet = wallet.id
            return r

        case .removeWallet(let id):
            guard let wallet = Wallets.all.first(where: { $0.id == id }) else { throw WalletError("No such wallet.") }
            try confirmInPerson("REMOVE the wallet «\(wallet.label)» from ihatecrypto: without its phrase or key its money can't be reached from here\n\(addresses(wallet))")
            remove(wallet)
            var r = SignerReply(ok: true)
            r.wallets = Wallets.all
            r.activeWallet = Wallets.active?.id
            return r

        case .renameWallet(let id, let label):
            let name = label.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !name.isEmpty, name.count <= 32 else { throw WalletError("A wallet's name is 1 to 32 characters.") }
            var list = Wallets.all
            guard let i = list.firstIndex(where: { $0.id == id }) else { throw WalletError("No such wallet.") }
            list[i].label = name
            Wallets.save(list)
            var r = SignerReply(ok: true)
            r.wallets = list
            r.activeWallet = Wallets.active?.id
            return r

        case .nearAddress:
            var r = SignerReply(ok: true)
            if let known = defaults.string(forKey: "near") {
                r.nearAddress = known
            } else {
                // A phrase wallet imported before NEAR: derive once (one unlock), then keep it with the wallet.
                let near = try withKey("near", reason: "show your NEAR address, derived from your phrase") { try NearAccount.implicit(privateKey: $0) }
                defaults.set(near, forKey: "near")
                var list = Wallets.all
                if let i = list.firstIndex(where: { $0.id == Wallets.activeID }) { list[i].near = near; Wallets.save(list) }
                r.nearAddress = near
            }
            return r

        case .ledgerAgent(let owner, let create):
            let id = try ledgerAgentID(owner)
            var key = AgentStore.load(id: id)
            if key == nil, create {
                key = try AgentStore.make()
                try AgentStore.save(key!, id: id)
            }
            var r = SignerReply(ok: true)
            r.agentAddress = key.flatMap { try? EVMAccount.address(privateKey: $0) }
            r.agentActive = r.agentAddress != nil && defaults.string(forKey: "agentActive-" + id) == "on"
            return r

        case .setLedgerAgentActive(let owner, let on):
            let id = try ledgerAgentID(owner)
            guard AgentStore.load(id: id) != nil else { throw WalletError("This Ledger has no trading key yet: approve one on the Ledger first.") }
            // Back on after a lock is the user's decision, as for a wallet's own key.
            let state = defaults.string(forKey: "agentActive-" + id)
            if on, state == nil { renewAgent("agentRenewed-" + id) }   // just approved on the Ledger itself, in person
            if on, state == "paused" || state == "on" && !agentFresh("agentRenewed-" + id) {
                try SeedStore.confirm("turn the Hyperliquid trading key of your Ledger \(owner) on for 7 days")
                renewAgent("agentRenewed-" + id)
                SeedStore.endSession() // a confirmation, not a move: it leaves no session for an unrelated signature
            }
            defaults.set(on ? "on" : "paused", forKey: "agentActive-" + id)
            var r = SignerReply(ok: true)
            r.agentActive = on
            return r

        case .signLedgerAgentL1(let owner, let action, let nonce):
            let id = try ledgerAgentID(owner)
            guard HLSigner.isTrading(action) else {
                var v = Verdict()
                v.problems.append("The Ledger's trading key signs orders, cancels and leverage only; anything else is signed on the Ledger.")
                return refused(v)
            }
            guard defaults.string(forKey: "agentActive-" + id) == "on", var agent = AgentStore.load(id: id) else {
                throw WalletError("The Ledger's trading key is off: approve it on the Ledger, or turn it back on.")
            }
            guard agentFresh("agentRenewed-" + id) || HLAction.onlyReduces(action) else {
                SeedStore.wipe(&agent)
                throw WalletError("The Ledger's trading key opens positions for 7 days after \(Presence.name) and those are over; stops and closes still go. "
                                  + "Turn it off and on in Settings › Wallet › Hyperliquid to go on.")
            }
            defer { SeedStore.wipe(&agent) }
            let verdict = HLGuard.check(action, owner: owner, recipients: [])
            guard verdict.ok else { return refused(verdict) }
            var r = SignerReply(ok: true)
            r.signature = try HLSigner.signL1(action, privateKey: agent, nonce: nonce, mainnet: true)
            r.checks = verdict.checks
            return r

        case .lock:
            SeedStore.endSession()
            defaults.set(false, forKey: agentActiveKey)
            // A Ledger's trading keys pause too: turning one back on asks for Touch ID.
            for (key, value) in defaults.dictionaryRepresentation() where key.hasPrefix("agentActive-ledger-") && value as? String == "on" {
                defaults.set("paused", forKey: key)
            }
            var r = SignerReply(ok: true)
            r.agentActive = false
            r.checks = ["The \(Presence.short) session ended; the trading key is paused."]
            return r

        case .setSessionLength(let seconds):
            guard SignerSession.choices.contains(seconds) else { throw WalletError("A confirmation can stay valid for 1, 5, 15 or 30 minutes.") }
            if seconds > SeedStore.sessionLength {
                try SeedStore.confirm("skip \(Presence.name) for \(seconds / 60) min after each confirmation")
            }
            defaults.set(seconds, forKey: "sessionLength")
            SeedStore.endSession()
            var r = SignerReply(ok: true)
            r.sessionLength = seconds
            return r

        case .endSession:
            SeedStore.endSession()
            return SignerReply(ok: true)

        case .signNearIntentDeposit(let amount, let depositAddress, let nonce, let blockHash):
            guard let yocto = UInt128(amount), yocto > 0 else { throw WalletError("Not a NEAR amount: \(amount).") }
            guard depositAddress.wholeMatch(of: /[0-9a-f]{64}/) != nil else { throw WalletError("Not a 1Click deposit account: \(depositAddress).") }
            guard blockHash.count == 32 else { throw WalletError("A NEAR block hash is 32 bytes.") }
            let doing = "sell \(NearTransaction.near(yocto)) NEAR: wrapped and sent to the 1Click deposit \(depositAddress.prefix(8))…\(depositAddress.suffix(6))"
            try approve([depositAddress], doing: doing)
            var r = SignerReply(ok: true)
            r.signedRaw = try withNearKey(reason: doing) { nearSecret, account in
                try NearTransaction.sign(receiver: NearTransaction.wrapContract, nonce: nonce, blockHash: blockHash,
                                         actions: NearTransaction.intentDeposit(account: account, depositAddress: depositAddress, amount: yocto),
                                         privateKey: nearSecret).signed
            }
            r.checks = ["\(NearTransaction.near(yocto)) NEAR → wNEAR → \(depositAddress) (1Click)"]
            return r

        case .signNearTransfer(let amount, let receiver, let recipients, let nonce, let blockHash):
            guard let yocto = UInt128(amount), yocto > 0 else { throw WalletError("Not a NEAR amount: \(amount).") }
            try checkNearRecipient(receiver, recipients, blockHash)
            try approve([receiver], doing: "send \(NearTransaction.near(yocto)) NEAR to \(receiver)")
            var r = SignerReply(ok: true)
            r.signedRaw = try withNearKey(reason: "send \(NearTransaction.near(yocto)) NEAR to \(receiver)") { nearSecret, account in
                guard receiver != account else { throw WalletError("That is this wallet's own NEAR account.") }
                return try NearTransaction.sign(receiver: receiver, nonce: nonce, blockHash: blockHash, actions: [.transfer(deposit: yocto)], privateKey: nearSecret).signed
            }
            r.checks = ["\(NearTransaction.near(yocto)) NEAR → \(receiver)"]
            return r

        case .signNearDeleteAccount(let beneficiary, let recipients, let nonce, let blockHash):
            try checkNearRecipient(beneficiary, recipients, blockHash)
            try approve([beneficiary], doing: "delete your NEAR account into \(beneficiary)")
            // Always a fresh prompt, whatever Touch ID session is open: this empties and removes the account.
            try SeedStore.confirm("DELETE your NEAR account and send all the NEAR on it to \(beneficiary)")
            let (raw, account) = try withNearKey(reason: "delete your NEAR account into \(beneficiary)") { nearSecret, account in
                guard beneficiary != account else { throw WalletError("An account can't be deleted into itself.") }
                return (try NearTransaction.sign(receiver: account, nonce: nonce, blockHash: blockHash,
                                                 actions: [.deleteAccount(beneficiary: beneficiary)], privateKey: nearSecret).signed, account)
            }
            var r = SignerReply(ok: true)
            r.signedRaw = raw
            r.checks = ["NEAR account \(account.prefix(8))… deleted into \(beneficiary)"]
            return r

        case .forget:
            guard let wallet = Wallets.active else { return SignerReply(ok: true) }
            try confirmInPerson("remove the wallet «\(wallet.label)» from ihatecrypto")
            remove(wallet)
            return SignerReply(ok: true)

        case .signSolanaMessage(let message):
            // A «message» that parses as a transaction would be a signed transaction once returned.
            guard (try? SolanaMessage.parse(message: message)) == nil, let text = String(data: message, encoding: .utf8),
                  !text.unicodeScalars.contains(where: { $0.value < 0x20 && $0 != "\n" && $0 != "\r" && $0 != "\t" }) else {
                var v = Verdict()
                v.problems.append("This isn't text but data that looks like a transaction: message signing refused.")
                return refused(v)
            }
            var r = SignerReply(ok: true)
            r.signatureBytes = try withKey("solana", reason: "sign text for a site:\n" + String(text.prefix(160))) {
                try Curve25519.Signing.PrivateKey(rawRepresentation: $0).signature(for: message)
            }
            r.checks = ["Only text was signed, not a transaction."]
            return r

        case .signPersonal(let message):
            guard !PersonalMessage.isBlindHash(message), message.count != 32 else {
                var v = Verdict()
                v.problems.append("The site asks to sign a hash, not text: such a signature could authorize anything.")
                return refused(v)
            }
            let text = String(data: message, encoding: .utf8) ?? "\(message.count) bytes"
            var r = SignerReply(ok: true)
            r.signature = try withKey("evm", reason: "sign text for a site:\n" + String(text.prefix(160))) {
                try EVMSigner.sign(digest: PersonalMessage.digest(message), privateKey: $0)
            }
            r.checks = ["Text was signed, not a transaction or an approval."]
            return r

        case .signTypedData(let json):
            let typed = try TypedData.parse(json)
            if let refusal = typed.refusal(owner: try? address("evm")) {
                var v = Verdict()
                v.problems.append(refusal)
                return refused(v)
            }
            var r = SignerReply(ok: true)
            r.signature = try withKey("evm", reason: typed.prompt) { try EVMSigner.sign(digest: typed.digest, privateKey: $0) }
            r.checks = ["Signed \(typed.primaryType) data for «\(typed.domainName)»; the hash was computed by the signer."]
            return r

        case .signEVMTransaction(let chainId, let nonce, let maxPriorityFee, let maxFee, let gasLimit, let to, let value, let data, let recipients):
            let owner = try address("evm")
            var v = Verdict()
            let call = EVMCall.decode(data)
            let approved = Set(recipients.map { $0.lowercased() } + [owner.lowercased()])
            let tx = EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                    gasLimit: gasLimit, to: to, value: value, data: data)
            if !EVMRPC.signable.contains(chainId) { v.problems.append("ihatecrypto doesn't sign for network \(chainId).") }
            if let refusal = call.refusal(sender: owner, chainId: chainId) { v.problems.append(refusal) }
            if let refusal = EVMCall.targetRefusal(to: to, data: data, sender: owner) { v.problems.append(refusal) }
            if let refusal = Batch7702.selfCall(to: to, data: data, owner: owner) { v.problems.append(refusal) }
            let relayPayees = EVMCall.relayDepositID(to: to, data: data).map { RelayOrders.order($0).map { [$0.recipient] } } ?? []
            if relayPayees == nil { v.problems.append(RelayOrders.unchecked) }
            if let fee = tx.feeProblem { v.problems.append(fee) }
            // Coins (calldata or not), tokens, NFTs and spending rights go only to this wallet or an address approved in person.
            for payee in call.payees(to: to, value: value, chainId: chainId) where !approved.contains(payee.lowercased()) {
                v.problems.append("Money or the right to spend it goes to an unapproved address \(payee).")
            }
            guard v.ok else { return refused(v) }
            // A contract whose call can't be read is asked about in person once, whether or not the app named it: it's
            // allowed as a call, apart from money recipients (a site's contract comes as a recipient only to be asked about).
            let doing = call.summary(chainId: chainId, to: to, value: value) + "\nnetwork fee " + tx.feeText
            let target = call.unreadTarget(to: to, value: value, chainId: chainId)
            try approve(recipients.filter { RecipientStore.key($0) != target } + (relayPayees ?? []), doing: doing)
            if let target { try approve([target], calls: true, doing: doing) }
            var r = SignerReply(ok: true)
            r.signedRaw = try withKey("evm", reason: doing) {
                try tx.signed(privateKey: $0).raw
            }
            r.checks = ["Calldata parsed by the signer: no unlimited approvals, no spending by others, money only to approved addresses."]
            return r

        case .signSafeMove(let chainId, let safe, let to, let value, let data, let nonce, let recipients):
            let owner = try address("evm")
            let move = Safe.Move(chainId: chainId, safe: safe, to: to, value: value, data: data, nonce: nonce)
            var v = Verdict()
            if let why = Safe.refusal(move, approved: Set((recipients + [owner, safe]).map { $0.lowercased() })) { v.problems.append(why) }
            guard v.ok else { return refused(v) }
            try approve(recipients, doing: "approve the multisig's move: " + move.summary)
            var r = SignerReply(ok: true)
            r.signature = try withKey("evm", reason: "approve the multisig's move: " + move.summary) {
                try EVMSigner.sign(digest: try move.hash(), privateKey: $0)
            }
            r.checks = ["Multisig move hashed by the signer: a plain payment, money only to approved addresses."]
            return r

        case .signEVMBatch(let chainId, let nonce, let maxPriorityFee, let maxFee, let gasLimit, let signerCalls, let recipients):
            let owner = try address("evm")
            let calls = signerCalls.map { Batch7702.Call(to: $0.to, data: $0.data) }
            var v = Verdict()
            v.problems = Batch7702.problems(calls, owner: owner, chainId: chainId)
            if !EVMRPC.signable.contains(chainId) { v.problems.append("ihatecrypto doesn't sign for network \(chainId).") }
            let fee = EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                     gasLimit: gasLimit, to: owner, value: 0, data: Data())
            if let problem = fee.feeProblem { v.problems.append(problem) }
            let approved = Set(recipients.map { $0.lowercased() } + [owner.lowercased()])
            var relayPayees: [String] = []
            for c in calls {
                for payee in EVMCall.decode(c.data).payees(to: c.to, value: 0, chainId: chainId) where !approved.contains(payee.lowercased()) {
                    v.problems.append("Money or the right to spend it goes to an unapproved address \(payee).")
                }
                if let id = EVMCall.relayDepositID(to: c.to, data: c.data) {
                    if let order = RelayOrders.order(id) { relayPayees.append(order.recipient) } else { v.problems.append(RelayOrders.unchecked) }
                }
            }
            guard v.ok else { return refused(v) }
            // The prompt names every call, read from its calldata here, then that they go as one — and that the wallet
            // keeps running the batch account's code on that network afterwards (it accepts coins and tokens as before).
            let lines = calls.map { EVMCall.decode($0.data).summary(chainId: chainId, to: $0.to, value: 0) }
            let net = EVMRPC.names[chainId] ?? "network \(chainId)"
            let doing = lines.joined(separator: "\nthen ") + "\nas one transaction: this wallet runs the audited batch account on \(net) (EIP-7702)\nnetwork fee " + fee.feeText
            // Each unread contract is asked about once, as in a single call; one another call lets spend (an exact approve
            // for the chat's `call`) is a money recipient, and that approval covers its call too.
            let targets = calls.compactMap { EVMCall.decode($0.data).unreadTarget(to: $0.to, value: 0, chainId: chainId) }
            let payees = Set(calls.flatMap { EVMCall.decode($0.data).payees(to: $0.to, value: 0, chainId: chainId) }.map(RecipientStore.key))
            try approve(recipients.filter { !targets.contains(RecipientStore.key($0)) || payees.contains(RecipientStore.key($0)) } + relayPayees, doing: doing)
            if !targets.isEmpty { try approve(targets, calls: true, doing: doing) }
            var r = SignerReply(ok: true)
            r.signedRaw = try withKey("evm", reason: doing) {
                try Batch7702.signed(chainId: chainId, nonce: nonce, maxPriorityFee: maxPriorityFee, maxFee: maxFee, gasLimit: gasLimit,
                                     owner: owner, calls: calls, privateKey: $0).raw
            }
            r.checks = ["Each call parsed by the signer and held to the one-call rules; the wallet runs only the audited batch account's code."]
            return r

        case .signSolana(let wire, let accounts, let recipients, let tokenOwners, let extraPrograms, let sponsor):
            let owner = try address("solana")
            let sponsorKey = try sponsor.map { s -> Data in
                guard let key = try? Base58.decode(s), key.count == 32 else { throw WalletError("The fee payer isn't a Solana address.") }
                return key
            }
            let message = try SolanaMessage.parse(transaction: wire)
            // Lookup tables are resolved by the app; the static part must be exactly what is in the transaction,
            // and money may not go to an account named only by a lookup table (no network here to check one).
            guard accounts.count >= message.staticKeys.count, Array(accounts.prefix(message.staticKeys.count)) == message.staticKeys else {
                throw WalletError("The account list doesn't match the transaction.")
            }
            var programs = SolanaGuard.knownPrograms
            for p in extraPrograms where programs[p] == nil { programs[p] = "program \(p.prefix(6))… (approved by you)" }
            let verdict = SolanaGuard.check(message, accounts: accounts,
                                            policy: SolanaPolicy(owner: owner, recipients: Set(recipients), programs: programs, trustLookupTables: false,
                                                                 sponsor: sponsor),
                                            tokenOwners: verifiedOwners(tokenOwners, mints: accounts))
            guard verdict.ok else { return refused(verdict) }
            let relayPayees = try SolanaGuard.relayDepositIDs(message).map { try RelayOrders.require($0).recipient }
            // The programs the user allowed for this one transaction are named in full: they run with the wallet's authority.
            let extra = extraPrograms.filter { SolanaGuard.knownPrograms[$0] == nil && (try? Base58.decode($0))?.count == 32 }
                .map { "runs \($0), not on ihatecrypto's list" }
            let payer = sponsor.map { ["the network fee is paid by \($0.prefix(4))…\($0.suffix(4)), not by you"] } ?? []
            let doing = ([prompt("sign a Solana transaction", verdict)] + extra + payer).joined(separator: "\n")
            // The fee payer may get rent back from this wallet: then it's a payee like any other, allowed in person once. One that
            // gets nothing of the wallet's (a JupiterZ maker, which only swaps) isn't kept as an address money may go to.
            let payee = sponsor.flatMap { SolanaGuard.refundsSponsor(message, accounts: accounts, owner: owner, sponsor: $0) ? [$0] : nil } ?? []
            try approve(recipients + payee + relayPayees, doing: doing)
            // A program not on the list runs with this wallet's authority and the signer can't read what it does: every
            // such transaction asks in person afresh, whatever session is open (the 2026-10-24 review).
            if !extra.isEmpty { try confirmAfresh(doing + "\nit can move anything in this wallet: sign only if you trust this site") }
            var r = SignerReply(ok: true)
            r.signedWire = try withKey("solana", reason: doing) {
                try SolanaTransaction.sign(wire, privateKey: $0, sponsor: sponsorKey).wire
            }
            r.checks = verdict.checks
            return r

        case .signReceiveAuthorization:
            // Relay's v3 proxy alone: nothing this signature covers names the order, so whom Relay pays would be only its
            // quote's word. Off (the owner, 2026-10-24); v3.2's pair (signRelayMulticallPermit) binds the order and signs.
            var v = Verdict()
            v.problems.append("A payment by signature that names no Relay order: whom it pays can't be checked, so ihatecrypto doesn't sign it.")
            return refused(v)

        case .signSponsoredTransfer(let chainId, let nonce, let gasLimit, let token, let to, let amount, let recipients):
            let owner = try address("evm")
            let t = SponsoredTransfer(chainId: chainId, nonce: nonce, gasLimit: gasLimit, token: token, to: to, amount: amount)
            var v = Verdict()
            if let refusal = t.refusal { v.problems.append(refusal) }
            let approved = Set(recipients.map(RecipientStore.key) + [RecipientStore.key(owner)])
            if !approved.contains(RecipientStore.key(to)) { v.problems.append("Money goes to an unapproved address \(to).") }
            guard v.ok else { return refused(v) }
            try approve(recipients.filter { RecipientStore.key($0) == RecipientStore.key(to) && RecipientStore.key($0) != RecipientStore.key(owner) },
                        doing: t.summary)
            var r = SignerReply(ok: true)
            r.signedRaw = try withKey("evm", reason: t.summary) { try t.signed(privateKey: $0).raw }
            r.checks = ["The signer built the transfer itself: a dollar it knows, to an approved address, gas price zero."]
            return r

        case .signRelayMulticallPermit(let chainId, let token, let value, let validBefore, let relayer, let refundTo, let nftRecipient, let metadata, let calls):
            let owner = try address("evm")
            let m = RelayMulticallPermit(chainId: chainId, token: token, from: owner, value: value, validBefore: validBefore, relayer: relayer,
                                         refundTo: refundTo, nftRecipient: nftRecipient, metadata: metadata,
                                         calls: calls.map { .init(target: $0.target, allowFailure: $0.allowFailure, value: $0.value, callData: $0.data) })
            guard let id = m.depositID else { throw WalletError(RelayOrders.unchecked) }
            let order = try RelayOrders.require(id)
            try approve([order.recipient], doing: order.summary)
            if let refusal = m.refusal(owner: owner, now: UInt64(Date().timeIntervalSince1970)) {
                var v = Verdict()
                v.problems.append(refusal)
                return refused(v)
            }
            let permit = try m.permit()
            let digests = [try m.digest(), try permit.typed().parts().digest]
            var r = SignerReply(ok: true)
            r.signatures = try withKey("evm", reason: permit.summary + "\ninto Relay's depository for this wallet") { key in
                try digests.map { try EVMSigner.sign(digest: $0, privateKey: key) }
            }
            r.checks = ["The signer read Relay's calls itself (one deposit into its depository for this wallet) and computed the nonce they bind."]
            return r

        case .signHyperliquidL1(let action, let nonce, let mainnet):
            let verdict = HLGuard.check(action, owner: try address("evm"), recipients: [])
            guard verdict.ok else { return refused(verdict) }
            // Orders, cancels and leverage go through the agent when it is active: it cannot move money out,
            // so no Touch ID. Otherwise the main key signs.
            // The agent signs trading only; anything that moves money (a vault transfer) takes the main key.
            // Past its week the agent still cancels and places reduce-only orders (stops, takes, closes), so a position
            // stays protected; what opens or adds to one takes the main key in person, and that renews the week.
            var r = SignerReply(ok: true)
            let agentOn = HLSigner.isTrading(action) && defaults.bool(forKey: agentActiveKey)
            if agentOn, agentFresh(agentRenewedKey) || HLAction.onlyReduces(action), var agent = AgentStore.load() {
                defer { SeedStore.wipe(&agent) }
                r.signature = try HLSigner.signL1(action, privateKey: agent, nonce: nonce, mainnet: mainnet)
            } else {
                let stale = agentOn && AgentStore.load() != nil
                let type = HLAction.type(action)
                let what = type == "setReferrer" ? "use ihatecrypto's referral code on Hyperliquid: 4% off its fees"
                    : type == "vaultTransfer" ? "move money between your Hyperliquid account and a vault"
                    : stale ? "sign a Hyperliquid order: the trading key's 7 days are over, and this renews them" : "sign a Hyperliquid order"
                // Renewing the trading key's week, and a vault deposit (money locked for days), ask in person afresh.
                let deposit = type == "vaultTransfer" && { if case .map(let p) = action { p.contains { $0.0 == "isDeposit" && $0.1 == .bool(true) } } else { false } }()
                if stale || deposit { try confirmAfresh(prompt(what, verdict)) }
                r.signature = try withKey("evm", reason: prompt(what, verdict)) {
                    try HLSigner.signL1(action, privateKey: $0, nonce: nonce, mainnet: mainnet)
                }
                if stale { renewAgent(agentRenewedKey) }
            }
            r.checks = verdict.checks
            return r

        case .relayOrder(let json):
            let order = try RelayOrder.read(json, evm: try? address("evm"), solana: try? address("solana"), now: UInt64(Date().timeIntervalSince1970))
            // Held, not yet approved: its payee is asked about (when it isn't this wallet's or already allowed) with the
            // deposit that names it, after Confirm — never while a card is being prepared.
            RelayOrders.add(order)
            var r = SignerReply(ok: true)
            r.checks = ["Relay order checked by the signer: it pays \(order.recipient) on \(order.outputChain); refunds come back to this wallet."]
            return r

        case .signRelayNonceMapping(let wallet, let depositor, let id, let nonce):
            let order = try RelayOrders.require(id)
            try approve([order.recipient], doing: order.summary)
            let owner = try address("evm").lowercased()
            guard wallet.lowercased() == owner, depositor.lowercased() == owner else {
                var v = Verdict()
                v.problems.append("The deposit mapping isn't for your wallet: \(wallet), \(depositor).")
                return refused(v)
            }
            let digest = try HLSigner.relayNonceMappingDigest(wallet: wallet, depositor: depositor, id: id, nonce: nonce)
            var r = SignerReply(ok: true)
            r.signature = try withKey("evm", reason: "link a Hyperliquid withdrawal to a Relay order") { try EVMSigner.sign(digest: digest, privateKey: $0) }
            r.checks = ["Deposit mapping: the wallet and depositor are yours."]
            return r

        case .signHyperliquidUser(let action, let mainnet, let recipients):
            let verdict = HLGuard.check(action, owner: try address("evm"), recipients: Set(recipients))
            guard verdict.ok else { return refused(verdict) }
            let (type, fields) = try HLSigner.userAction(action)
            let what = type != "ApproveBuilderFee" ? "sign a Hyperliquid transfer"
                : HLAction.text(action, "maxFeeRate") == HLBuilder.revoke ? "take back ihatecrypto's fee approval" : "approve ihatecrypto's fee on your Hyperliquid trades"
            try approve(recipients, doing: prompt(what, verdict))
            // Approving ihatecrypto's fee is the user's own opt-in: a fresh prompt, never an open session (a revoke needs none).
            if type == "ApproveBuilderFee", HLAction.text(action, "maxFeeRate") != HLBuilder.revoke { try confirmAfresh(prompt(what, verdict)) }
            var r = SignerReply(ok: true)
            r.signature = try withKey("evm", reason: prompt(what, verdict)) {
                try HLSigner.signUserAction(type, fields: fields, privateKey: $0, mainnet: mainnet)
            }
            r.checks = verdict.checks
            return r
        }
    }

    /// A phrase wallet: its seed and the public addresses derived from it.
    static func store(phrase: String, passphrase: String, requirePresence: Bool, test: Bool = false, automation: Bool = false) throws -> SignerReply {
        var seed = try Mnemonic.seed(phrase, passphrase: passphrase)
        defer { SeedStore.wipe(&seed) }
        return try add(secret: seed, kind: "phrase",
                       solana: try SolanaAccount.address(privateKey: Derivation.ed25519(seed: seed, path: WalletPath.solana(0))),
                       evm: try EVMAccount.address(privateKey: Derivation.secp256k1(seed: seed, path: WalletPath.evm(0))),
                       near: try NearAccount.implicit(privateKey: Derivation.ed25519(seed: seed, path: WalletPath.near(0))),
                       requirePresence: requirePresence, test: test, automation: automation)
    }

    /// Adds a wallet (or re-seals the same one: same kind and addresses keep its id, trading key and approved
    /// addresses) and signs with it from now on — except the automation wallet, which signs only a task's moves.
    /// `test`: sealed without Touch ID (a test or the automation wallet).
    static func add(secret: Data, kind: String, solana: String?, evm: String?, near: String?, requirePresence: Bool,
                    test: Bool = false, automation: Bool = false) throws -> SignerReply {
        var list = Wallets.all
        let same = list.first { $0.kind == kind && $0.solana == solana && $0.evm?.lowercased() == evm?.lowercased() }
        let id = same?.id ?? (list.isEmpty ? SignerWallet.firstID : String(UUID().uuidString.prefix(8)).lowercased())
        try SeedStore.save(secret, requirePresence: requirePresence, id: id, test: test)
        if solana != publicTestSolana { OwnStore.add([solana, evm].compactMap { $0 }) }
        let tests = list.filter(\.isTest).count
        let wallet = SignerWallet(id: id, label: same?.label ?? (automation ? "Automation" : test ? (tests == 0 ? "Test" : "Test \(tests + 1)") : list.isEmpty ? "Main" : "Wallet \(list.count + 1)"),
                                  kind: kind, solana: solana, evm: evm, near: near, test: test && !automation ? true : nil, automation: automation ? true : nil)
        if let i = list.firstIndex(where: { $0.id == id }) { list[i] = wallet } else { list.append(wallet) }
        Wallets.save(list)
        if automation {
            var r = SignerReply(ok: true)
            r.hasWallet = true
            r.wallets = list
            r.activeWallet = Wallets.active?.id
            return r
        }
        Wallets.activate(wallet)
        var r = SignerReply(ok: true)
        r.hasWallet = true
        r.solanaAddress = solana
        r.evmAddress = evm
        r.nearAddress = near
        r.wallets = list
        r.activeWallet = id
        return r
    }

    /// Removes a wallet with everything kept for it; signing moves to the first one left.
    static func remove(_ wallet: SignerWallet) {
        let wasActive = Wallets.active?.id == wallet.id
        SeedStore.delete(id: wallet.id)
        AgentStore.delete(id: wallet.id)
        RecipientStore.delete(id: wallet.id)
        OwnStore.remove([wallet.solana, wallet.evm].compactMap { $0 })
        defaults.removeObject(forKey: Wallets.scoped("agentActive", wallet.id))
        let rest = Wallets.all.filter { $0.id != wallet.id }
        Wallets.save(rest)
        if wasActive { Wallets.activate(rest.first) }
    }

    /// Relay orders this signer checked (relayOrder), by id, until their deadline: a deposit naming any other id isn't
    /// signed, so whom a bridge pays is never only the app's word (the 2026-10-24 review).
    enum RelayOrders {
        static let lock = NSLock()
        nonisolated(unsafe) static var held: [Data: RelayOrder] = [:]
        static let unchecked = "A Relay deposit for an order the signer hasn't checked: nothing was signed."
        static func add(_ order: RelayOrder) {
            let now = UInt64(Date().timeIntervalSince1970)
            lock.withLock { held = held.filter { $0.value.deadline > now }; held[order.id] = order }
        }
        static func order(_ id: Data) -> RelayOrder? {
            lock.withLock { held[id].flatMap { $0.deadline > UInt64(Date().timeIntervalSince1970) ? $0 : nil } }
        }
        static func require(_ id: Data) throws -> RelayOrder {
            guard let o = order(id) else { throw WalletError(unchecked) }
            return o
        }
    }

    static var agentActiveKey: String { Wallets.scoped("agentActive") }
    static var agentRenewedKey: String { Wallets.scoped("agentRenewed") }

    /// The Hyperliquid trading key opens or adds to positions for 7 days after the user's last Touch ID for it (turning it
    /// on, an order the main key signed in person); then cancels and reduce-only orders still go without one. The signer
    /// sees no prices, so it can't cap what an order risks: this caps how long a program that took over the app could
    /// trade the account down unseen. Only an in-person act renews it, never the app's word.
    static let agentWeek: TimeInterval = 7 * 86_400
    static func agentFresh(_ key: String) -> Bool { Date().timeIntervalSince1970 - ((defaults.object(forKey: key) as? Double) ?? 0) < agentWeek }
    static func renewAgent(_ key: String) { defaults.set(Date().timeIntervalSince1970, forKey: key) }

    /// Where a Ledger's trading key is kept: by its account, which must be an EVM address.
    static func ledgerAgentID(_ owner: String) throws -> String {
        guard owner.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else { throw WalletError("Not an EVM address: \(owner).") }
        return "ledger-" + owner.lowercased()
    }

    /// A wallet's addresses for a Touch ID prompt: its name is the app's to set, its addresses are not.
    static func addresses(_ w: SignerWallet) -> String {
        [w.solana.map { "Solana \($0)" }, w.evm.map { "EVM \($0)" }].compactMap { $0 }.joined(separator: "\n")
    }

    /// The active wallet's key for one chain («solana», «evm», «near»), after Touch ID or within its session: derived
    /// from its phrase, or its one imported key. A key wallet asked for another chain says so plainly. The key lives
    /// only while `body` signs with it: then it and the seed are overwritten.
    static func withKey<T>(_ chain: String, reason: String, _ body: (Data) throws -> T) throws -> T {
        guard let wallet = Wallets.active else { throw WalletError("No wallet imported.") }
        if let missing = wallet.missing(chain) { throw WalletError(missing) }
        var secret = try SeedStore.load(reason: reason, id: wallet.id, session: Wallets.signing == nil)
        defer { SeedStore.wipe(&secret) }
        guard wallet.kind == "phrase" else { // a key wallet holds that chain's key itself
            try vouch(chain, secret, of: wallet)
            return try body(secret)
        }
        try vouch(seed: secret, of: wallet)
        var key = switch chain {
        case "solana": try Derivation.ed25519(seed: secret, path: WalletPath.solana(0))
        case "evm": try Derivation.secp256k1(seed: secret, path: WalletPath.evm(0))
        default: try Derivation.ed25519(seed: secret, path: WalletPath.near(0))
        }
        defer { SeedStore.wipe(&key) }
        return try body(key)
    }

    /// The address `key` signs for must be the one the checks took as «ours» (`address`): the settings name it, the key
    /// proves it, so a made-up «our address» can't pass money to itself. Then it's kept among ours (OwnStore), so a
    /// wallet here before OwnStore joins it with its next signature. The public test wallet stays out: anyone holds its key.
    private static func vouch(_ chain: String, _ key: Data, of wallet: SignerWallet) throws {
        guard let a = chain == "solana" ? try? SolanaAccount.address(privateKey: key) : chain == "evm" ? try? EVMAccount.address(privateKey: key) : nil
        else { return }
        if let claimed = try? address(chain), RecipientStore.key(claimed) != RecipientStore.key(a) {
            throw WalletError("This wallet's saved \(chain) address isn't the one its key signs for: nothing was signed.")
        }
        if wallet.solana != publicTestSolana { OwnStore.add([a]) }
    }

    /// A phrase wallet's every saved address, not only the one this signature is for: the app pays to the others too (a
    /// NEAR account or a Solana address in a bridge quote, Receive), so one changed in the settings is caught at the first
    /// signature of any kind. Its seed derives them all: the mirrors and the wallet list must name those.
    private static func vouch(seed: Data, of wallet: SignerWallet) throws {
        for chain in ["solana", "evm", "near"] {
            var key = try chain == "evm" ? Derivation.secp256k1(seed: seed, path: WalletPath.evm(0))
                : Derivation.ed25519(seed: seed, path: chain == "solana" ? WalletPath.solana(0) : WalletPath.near(0))
            defer { SeedStore.wipe(&key) }
            let own = try chain == "solana" ? SolanaAccount.address(privateKey: key)
                : chain == "evm" ? EVMAccount.address(privateKey: key) : NearAccount.implicit(privateKey: key)
            let saved = [try? address(chain), chain == "solana" ? wallet.solana : chain == "evm" ? wallet.evm : wallet.near].compactMap { $0 }
            if saved.contains(where: { RecipientStore.key($0) != RecipientStore.key(own) }) {
                throw WalletError("This wallet's saved \(chain) address isn't the one its key signs for: nothing was signed.")
            }
            if chain != "near", wallet.solana != publicTestSolana { OwnStore.add([own]) }
        }
    }

    static let publicTestSolana = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"

    /// The public BIP-39 test phrase («abandon … about»): anyone can sign for it, so the self-test runs without prompts.
    /// Its address in the settings isn't enough: the wallet's seed must open without a prompt and derive it.
    static var holdsPublicTestWallet: Bool {
        guard (try? address("solana")) == publicTestSolana, let wallet = Wallets.active else { return false }
        return signsWithoutPrompt(wallet, solana: publicTestSolana)
    }

    /// The wallet was sealed without Touch ID — its seed opens with no prompt — and, given `solana`, its key is that one.
    /// A test flag in the settings counts only with this.
    static func signsWithoutPrompt(_ wallet: SignerWallet, solana: String? = nil) -> Bool {
        guard var secret = SeedStore.withoutPrompt(id: wallet.id) else { return false }
        defer { SeedStore.wipe(&secret) }
        guard let solana else { return true }
        if wallet.kind != "phrase" { return (try? SolanaAccount.address(privateKey: secret)) == solana }
        guard var key = try? Derivation.ed25519(seed: secret, path: WalletPath.solana(0)) else { return false }
        defer { SeedStore.wipe(&key) }
        return (try? SolanaAccount.address(privateKey: key)) == solana
    }

    /// Adding, switching or removing a wallet once there is one is the user's act, in person: Touch ID or the password.
    /// A program that talks to the signer as ihatecrypto can't slip in its own wallet and wait for the user's deposits.
    static func confirmInPerson(_ reason: String) throws {
        guard !Wallets.all.isEmpty, !holdsPublicTestWallet else { return }
        try SeedStore.confirm(reason)
    }

    /// An act that asks in person afresh, whatever Touch ID session is open (a site's program, the trading key's week, a
    /// vault deposit, a fee opt-in, Relay's v3 payment by signature). A test wallet asks nothing, as with new addresses;
    /// the automation wallet's prompt closes by itself after a minute, as its others do.
    static func confirmAfresh(_ reason: String) throws {
        if holdsPublicTestWallet { return }
        if let wallet = Wallets.active, wallet.isTest, !wallet.isAutomation, Wallets.signing == nil, signsWithoutPrompt(wallet) { return }
        try SeedStore.confirm(reason, within: Wallets.signing == nil ? nil : 60)
    }

    /// Money to an address outside the wallet: each new address is confirmed in person once, with the address on
    /// the prompt, and remembered in the signer's own Keychain. The app saying «the user approved it» isn't enough.
    /// `calls`: contracts whose calls the signer can't read, allowed in person as calls and kept in their own list, so a
    /// later payment or approval to one asks again; an address allowed for money may be called too.
    /// `doing`: what the signature that follows does, read by the signer itself — the prompt that allows the address opens
    /// the session that signature then uses, so this prompt is the one place the amount is shown.
    static func approve(_ recipients: [String], calls: Bool = false, doing: String) throws {
        // The Test wallet signs without Touch ID by design (a few dollars for checks): asking it about a new address
        // only stops the check. Its approvals are its own (RecipientStore is per wallet), so no other wallet skips one.
        if holdsPublicTestWallet { return }
        // Never the automation wallet: its seed opens without a prompt too, so a test flag on it would pass any address.
        if let wallet = Wallets.active, wallet.isTest, !wallet.isAutomation, Wallets.signing == nil, signsWithoutPrompt(wallet) { return }
        let own = Set([try? address("solana"), (try? address("evm"))?.lowercased()].compactMap { $0 })
        // An address saved in the book was confirmed in person, whole, when it was saved; another wallet of this Mac's
        // is the user's own.
        let money = RecipientStore.all().union(AddressBookStore.all().map { RecipientStore.key($0.address) }).union(OwnStore.all())
        let known = calls ? RecipientStore.all(RecipientStore.calls).union(money) : money
        let new = Array(Set(recipients.map(RecipientStore.key).filter { !known.contains($0) && !own.contains($0) })).sorted()
        guard !new.isEmpty else { return }
        // The automation wallet signs with no one at the Mac: its prompt closes by itself after a minute.
        let what = calls ? "allow ihatecrypto to call a contract whose call it can't read:\n" : "allow ihatecrypto to send money to a new address:\n"
        try SeedStore.confirm(what + new.joined(separator: "\n") + "\nfor this move: " + doing, within: Wallets.signing == nil ? nil : 60)
        try RecipientStore.add(new, to: calls ? RecipientStore.calls : RecipientStore.money)
    }

    /// A NEAR receiver ihatecrypto may pay: a valid account, among the saved addresses the app passed, with a real block hash.
    static func checkNearRecipient(_ receiver: String, _ recipients: [String], _ blockHash: Data) throws {
        guard AddressCheck.problem(receiver, network: .near) == nil else { throw WalletError("Not a NEAR account: \(receiver).") }
        guard recipients.contains(receiver) else { throw WalletError("\(receiver) isn't a saved address: NEAR goes only to one.") }
        guard blockHash.count == 32 else { throw WalletError("A NEAR block hash is 32 bytes.") }
    }

    /// The wallet's NEAR key (m/44'/397'/0') after Touch ID, and its implicit account, which must be the one stored.
    static func withNearKey<T>(reason: String, _ body: (Data, String) throws -> T) throws -> T {
        try withKey("near", reason: reason) { secret in
            let account = try NearAccount.implicit(privateKey: secret)
            if let known = try? address("near"), known != account { throw WalletError("The NEAR key doesn't match this wallet's NEAR account.") }
            return try body(secret, account)
        }
    }

    /// The signing wallet's own address on `chain`: the active wallet's (mirrored in defaults), or the automation
    /// wallet's while it signs a task's move.
    static func address(_ chain: String) throws -> String {
        if let w = Wallets.signing {
            if let a = chain == "solana" ? w.solana : chain == "evm" ? w.evm : w.near { return a }
            throw WalletError(w.missing(chain) ?? "No \(chain) address.")
        }
        if let a = defaults.string(forKey: chain) { return a }
        if let missing = Wallets.active?.missing(chain) { throw WalletError(missing) }
        throw WalletError("No wallet imported.")
    }

    static func refused(_ verdict: Verdict) -> SignerReply {
        var r = SignerReply(ok: false, error: "The guard refused to sign.")
        r.checks = verdict.checks
        r.problems = verdict.problems
        return r
    }

    /// The Touch ID / password prompt text: the money lines the signer parsed itself.
    static func prompt(_ what: String, _ verdict: Verdict) -> String {
        ([what] + verdict.checks.filter { $0.contains("→") }).joined(separator: "\n")
    }

    /// Keeps only claims the signer can prove: the account is the associated token account of that owner for
    /// one of the transaction's own accounts as the mint. A made-up "this is your account" is dropped.
    static func verifiedOwners(_ claimed: [String: String], mints: [String]) -> [String: String] {
        claimed.filter { account, owner in
            mints.contains { mint in
                [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: owner, mint: mint, program: $0)) == account }
            }
        }
    }
}
