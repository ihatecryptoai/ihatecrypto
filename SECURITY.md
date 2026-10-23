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
sends a JSON request and gets back a signature; it never sees a stored seed or a derived key. A secret passes through
it only twice: a recovery phrase or private key being imported, on its way in through the app's window, and a new
wallet's phrase, once, for the user to write down (secure fields, a window hidden from screen capture, a copy wiped
from the clipboard after 60 s, never logged).
Release builds have no
`get-task-allow`, so no debugger can attach to either process, and launch flags that drive the app are Debug-only.

**Seed storage.** The seed is encrypted to a Secure Enclave key and the ciphertext sits in the data-protection
Keychain, this device only, never synced, in the signer's own access group. Opening it takes Touch ID or the Mac
password; a seed kept from before sealing is sealed the first time it opens. On a Mac without Touch ID the app's 6-digit
passcode can sign instead; it is mixed into its own Secure
Enclave key, and five wrong codes in a row delete it. After each signature the seed and the derived key are
overwritten in memory.

**What asks in person.** One Touch ID covers one move, for a session of one minute by default (up to 30 minutes if the
user picks it: within it, a move that passes the Guard and pays only approved addresses signs with no new prompt, so a
short session is safer); locking the screen or sleep ends it. Adding, switching or removing a wallet, saving an address
book entry and paying a new address each ask in person, with the address on the prompt and what the move does, amount
included (a test wallet skips the last one). A call to a contract whose calldata the signer can't read — a site's, or
the chat's `call` — is allowed in person once per contract, whether or not the app asks for that, and kept apart from
payment recipients: it doesn't make that contract a place money or a spending right may go without asking. Only the
contracts ihatecrypto's own moves call that way go without it: Relay's depository and approval proxies, Tempo's
exchange and fee manager, Safe's factory. A call read by its selector alone (a swap, a vault, a claim) is that protocol's
only at that protocol's own contract; elsewhere it is asked about like an unread call. Some acts ask in person afresh
whatever session is open: running a Solana program not on the signer's list (a site's), turning the Hyperliquid
trading key on or renewing its 7 days, approving ihatecrypto's Hyperliquid fee, a deposit into a Hyperliquid vault,
and Relay's v3 payment by signature. The prompt shows the amounts and addresses the signer parsed itself (token
transfers with their amount, Hyperliquid orders and transfers with theirs), so an app that lies about a transaction is
visible there.

**What signs without Touch ID.** Only what the user turns on: a test wallet made fresh in the app (it pays new
addresses without asking too: keep a few dollars on it), an automation wallet that signs only scheduled tasks' moves,
and the Hyperliquid trading key. The automation wallet can spend what is on it and no more; a task that would spend from
it, one made before the wallet included, waits until the user switches it on in the Schedule, and a new recipient still
asks in person. Its tasks are kept with a tag only this app can make: a list changed outside ihatecrypto loads paused. The
trading key can place and cancel orders, set leverage up to 20× and bind ihatecrypto's referral
code; it can't withdraw or transfer. The signer has no network, so it can't see market prices and doesn't limit an
order's market, price or size: a program that took over the app could trade the account down. So the key opens or adds
to positions only for 7 days after the user's last Touch ID for it (turning it on, or an order the main key signed in
person); after that it still cancels and places reduce-only orders, so stops and closes keep working, and anything
that opens a position asks in person again and renews the 7 days. A Ledger's trading key works the same way. Keep on
Hyperliquid what you trade with. Turning each one on asks in person.

**The Guard.** Plain code checks every transaction before it is signed, and the signer runs the checks again itself:

- Solana: only programs on a fixed list run (System, SPL Token, Jupiter, Relay Depository, Kamino and a few more).
  Money goes only to the user's own accounts or to an address approved in person, named in the transaction itself: an
  entry a lookup table names is never taken for anyone's address, since the signer can't read tables and the app could
  name this wallet there. Token approvals and owner changes are refused, and the priority fee has a cap. The one other
  fee payer is a gasless swap's (Jupiter's wallet, for a wallet with no SOL): it signs first, the user's wallet is the
  only other signer, every instruction is checked the same, and since it may get its rent back, it is approved in person
  once like any payee. Every Jupiter swap form must pay the user's own account; any other Jupiter instruction is refused.
- Solana protocols, read by the signer itself: Kamino runs only the instructions ihatecrypto's deposits and withdrawals
  use, each one its program pays out only to the signer (read in Kamino's verified source), and only on the user's own
  position on Kamino's main market; anything else (a deposit minting to another account, repaying someone else's
  debt, a flash loan, handing a stake over) is refused. Jupiter Lend's deposits and withdrawals and Jupiter DCA's
  orders must pay the signer's own token accounts.
- Simulation: the app runs the transaction on the network first, and the balances after must match what the user
  approved. The signer has no network, so this check is the app's alone: for how much a swap or a protocol pays out,
  and for what a program allowed for one site does.
- Bridges: a Relay deposit (Solana, EVM, a v3.2 payment by signature, a Hyperliquid withdrawal's mapping) names its order
  by a 32-byte id, which is the hash of the order. The app hands the signer the order from Relay's quote; the signer
  recomputes the id, checks the order refunds only the user, runs no calls and takes no fees, and signs a deposit only
  for an order it checked, with the user's wallet as the depositor; the order's payee is the user's own address or one
  approved in person with the deposit. A 1Click deposit address must be bound by 1Click to the user's recipient, refund
  address, assets and amount (checked in the app).
- Hyperliquid: orders stay inside the account; transfers out go only to an approved address. An order may pay
  ihatecrypto's fee (a 0.05% builder fee), only to ihatecrypto's own address and at no more than that rate, and only after
  the user approved that fee once in person; a fee approval for any other address or rate, and any referral code but
  ihatecrypto's, are refused.
- EVM: the signer decodes calldata itself and refuses unlimited approvals, `setApprovalForAll`, permits, Permit2 for
  longer than a day and `transferFrom` out of the user's wallet; whoever a `transferFrom` or an `approve` names (an
  approve of zero may hand over an NFT) is a payee. A multicall paying two different addresses, a LayerZero send with
  options, a call to Relay's depository that isn't a deposit for the user, and a multisig's `execTransaction` that calls
  into the multisig, calls a contract the signer can't read or pays a gas refund are refused. A site's typed data with
  Permit or nested types, with bytes to run as the wallet, or with any address but the wallet's (the zero address
  included: an open order) beside a number that isn't a nonce or a time, is refused; so is a 32-byte message or a bare
  hash to sign. The one payment by signature it makes is its own: an EIP-3009 authorization it builds
  itself, for USDC (or Robinhood Chain's USDG) by the token's own domain, from the user's wallet, to Relay's approval
  proxy only, valid an hour at most. Relay's solver pays the gas, so dollars on a network with no gas coin can still
  move out. On Relay's v3.2 proxy the payment comes with a second signature over the calls the proxy runs with the
  money: the signer reads them itself (one deposit through Relay's router into its depository, for the user's wallet),
  computes the nonce that binds them and signs both; a changed call breaks the pair on-chain. On BNB Chain, whose dollar
  has no payment by signature, it signs one more zero-fee transaction it builds itself: a plain transfer of that dollar
  (to Relay's deposit address, approved like any new address) that BNB Chain's paymaster, MegaFuel, pays the gas for.

**Untrusted text.** Pasted messages and what the model's lookups bring back (token listings, pages, API replies) pass
local rules before any model reads them. A second check (Settings › Second check) has the chat's model read each
command and plan again, and a separate model on OpenRouter too when the user turns that on; it can stop a text but can
never approve one. An address found by a lookup never becomes a recipient. A pasted recovery phrase or private key
never reaches the model. A typed «yes» never signs: only the Confirm button does. A swap's price is held to its card: a
Jupiter swap whose fresh quote gives more than 1% less than the card showed isn't signed.

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

**What leaves the Mac.** The model provider the user picks (local models keep it on the Mac) gets the recent chat, saved
notes, the trade plan, open positions with P/L, the balances on every network, favorite tokens and the address book's
names and networks, with every message. Blockchain nodes and price, portfolio, swap and bridge services get the
wallet's addresses, and the services a move goes through get its amounts and recipients. A paid web service (MPP) is asked
its price without the request's content, which goes only after Confirm (a service that can't quote without it gets it
unpaid, as the protocol has it). Address book icons come from each entry's own site.

## Known limits

- The app tells the signer the approved recipients and Solana lookup-table contents, since the signer has no network.
  New recipients still need Touch ID, and no money goes to an account only a lookup table names.
- Relay's approval proxy runs Relay's own calls with the exact amount approved to it (a same-network swap, a token sold
  through Relay): the signer can't read those calls, so what such a swap pays out rests on Relay and the app's
  simulation. Relay's v3 payment by signature names no order either (v3.2's does); it asks in person every time.
- A transfer to a Relay or 1Click deposit address is a plain transfer to a new address: it asks in person with that
  address, and what the deposit address does after is the bridge's.
- A Solana or EVM key is in the signer's memory while it signs. Copies inside the derivation and secp256k1 code can't
  be wiped from Swift.
- Chats, canvases and settings are owner-only files in `~/Library/Application Support`, not encrypted.
- A site's typed data is refused by kind and by its values (see the Guard). What passes (a login, a vote, a delegation
  with no amount) is shown field by field, and the prompt says ihatecrypto can't tell what it allows.
- On a Mac without Touch ID the passcode is typed into ihatecrypto's own window, not a macOS dialog; it signs moves to
  addresses already approved, and a new address still asks for the Mac's password.
- Nobody outside the project has audited this code.
