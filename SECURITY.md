# Security

ihatecrypto holds keys to real money. It has not been audited, and there is no bug bounty.

## Reporting a vulnerability

Use **Report a vulnerability** on this repository's Security tab. It opens a private advisory that only the
maintainers see. Don't open a public issue, pull request or discussion for a vulnerability.

Include what an attacker needs (a web page, a token, a message in the chat, a program running on the Mac), the steps,
and what they get. If money can move, say from which wallet and to whom. Test with a throwaway wallet and small
amounts, never with someone else's funds. We reply in the advisory and credit you when the fix ships, unless you
ask us not to.

## In scope

- The signing process (`App/Sources/Signer`) and the Guard (`App/Sources/Guard`): anything that gets a signature the
  user didn't approve, sends money to an address they didn't approve, or reads a seed or key.
- Prompt injection: text from a web page, a token's listing or an API reply that makes the app prepare a move the
  user didn't ask for.
- The local debug API, the dApp browser, the address book and the update path.

Out of scope: bugs in the services a move goes through (Relay, Jupiter, Hyperliquid and others) or in your model
provider, attacks that need root or a disabled SIP, and losses from trades the user confirmed.

## How it is built

**Separate signing process.** Keys live in `Signer.xpc`, an XPC service inside the app. It is sandboxed and has no
network entitlement. It accepts connections only from the app it is built into, signed by the same team. The app
sends a JSON request and gets back a signature; it never sees the seed or a private key. Release builds have no
`get-task-allow`, so no debugger can attach to either process, and launch flags that drive the app are Debug-only.

**Seed storage.** The seed is encrypted to a Secure Enclave key and the ciphertext sits in the data-protection
Keychain, this device only, never synced, in the signer's own access group. Opening it takes Touch ID or the Mac
password. On a Mac without Touch ID the app's 6-digit passcode can sign instead; it is mixed into its own Secure
Enclave key, and five wrong codes in a row delete it. After each signature the seed and the derived key are
overwritten in memory.

**What asks in person.** One Touch ID covers one move, for a session of one minute by default; locking the screen or
sleep ends it. Adding, switching or removing a wallet, saving an address book entry and paying a new address each ask
in person, with the address on the prompt (a test wallet skips the last one). A site's call to a contract whose calldata
the signer can't read is allowed in person once per contract, and kept apart from payment recipients: it doesn't make
that contract a place money or a spending right may go without asking. The prompt shows the amounts and addresses the
signer parsed itself, so an app that lies about a transaction is visible there.

**What signs without Touch ID.** Only what the user turns on: a test wallet made fresh in the app, an automation
wallet that signs only scheduled tasks' moves, and the Hyperliquid trading key. That key can place and cancel orders,
set leverage up to 20× and bind ihatecrypto's referral code; it can't withdraw or transfer. Turning each one on asks in
person.

**The Guard.** Plain code checks every transaction before it is signed, and the signer runs the checks again itself:

- Solana: only programs on a fixed list run (System, SPL Token, Jupiter, Relay Depository, Kamino and a few more).
  Money goes only to the user's own accounts or to an address bound to a quote they approved. Token approvals, owner
  changes and other fee payers are refused, and the priority fee has a cap.
- Simulation: the transaction runs on the network first, and the balances after must match what the user approved.
- Bridges: a 1Click deposit address must be bound by 1Click to the user's recipient, refund address, assets and
  amount. A Relay deposit must come from the user's wallet and name them as sender and recipient.
- Hyperliquid: orders stay inside the account; transfers out go only to an approved address. An order may pay
  ihatecrypto's fee (a 0.05% builder fee), only to ihatecrypto's own address and at no more than that rate, and only after
  the user approved that fee once in person; a fee approval for any other address or rate, and any referral code but
  ihatecrypto's, are refused.
- EVM: the signer decodes calldata itself and refuses unlimited approvals, `setApprovalForAll`, permits, Permit2 for
  longer than a day and `transferFrom` out of the user's wallet. Typed data with Permit or nested types, and a bare
  hash to sign, are refused.

**Untrusted text.** Pasted messages and what the model's lookups bring back (token listings, pages, API replies) pass
local rules before any model reads them. A second check (Settings › Second check) has the chat's model read each
command and plan again, and a separate model on OpenRouter too when the user turns that on; it can stop a text but can
never approve one. An address found by a lookup never becomes a recipient. A typed «yes» never
signs: only the Confirm button does.

**Local debug API.** Not in a release: only development builds (`build-app.sh`, `swift run`) have it. There it is off
by default (Settings › Debugging), listens on 127.0.0.1 only, needs a bearer token that changes on every start and
sits in an owner-only file, and refuses requests a browser sends. It never signs: while it drives the app, only status
requests reach the signer.

**Updates.** Releases are signed with the team's Developer ID and notarized. Sparkle checks each update against the
EdDSA public key in the app. The app only says a new version is out: nothing downloads or installs until you press
Install.

**Reproducible releases.** From the version after 0.6.9, a release can be rebuilt from its tag byte for byte, apart
from the code signatures that only the project's keys can make. `App/verify-release.sh <version>` downloads the DMG,
builds the tag the way `release.sh` did (with the Xcode build the release names, in the same folder) and compares every
file with the signatures taken off both copies. The **Verify release** workflow runs it on GitHub's Mac for each
published release, so a green run there says the download is this code.

**No telemetry.** The app sends no analytics. It copies the crash reports macOS writes into its own folder and sends
them nowhere.

## Known limits

- The app tells the signer the approved recipients and Solana lookup-table contents, since the signer has no network.
  New recipients still need Touch ID.
- A Solana or EVM key is in the signer's memory while it signs. Copies inside the derivation and secp256k1 code can't
  be wiped from Swift.
- Chats, canvases and settings are owner-only files in `~/Library/Application Support`, not encrypted.
