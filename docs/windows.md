# ihatecrypto for Windows

The Windows app is the Mac's core ported to Rust (docs/windows-rust.md): the wallet, the chains, the protocols, the
Guard, the plans and the execution, each crate checked against the Swift it came from. What differs from the Mac is the
window, the engine that runs the chat in place of the Mac's `Workspace`, and where the signer keeps its keys.

```
ihatecrypto.exe (Tauri)          App/Windows/app        the window: the page in ui/, no key, no money logic; the
  │                                                     engine's broker (src/broker.rs)
  ├─ ihatecrypto-engine.exe      Windows/rust/engine    the chat, the cards, Confirm, balances — in an AppContainer
  └─ ihatecrypto-signer.exe      Windows/rust/signer    what the signer decides, as the Mac's; keys under Windows
                                                        Hello, the trusted settings, the pipe — its pipes the engine's
     (all link winsec)           App/Windows/native     Windows Hello, DPAPI, hardening, the container (windows-rs)
```

The window starts the engine with pipes, in its AppContainer, and starts the signer for it, handing the engine the
signer's pipes (below). Each line between the window and the engine is one JSON request (`Engine.Request`) or one
answer or event (`Outbox`); the engine sends the whole state after every change. Between the engine and the signer go
the Mac's own `SignerRequest` and `SignerReply`, framed by a 4-byte length. Outside the window (tests, a developer's
Mac) the engine starts the signer itself, as before.

## The engine's AppContainer

The Windows counterpart of the Mac's App Sandbox (`winsec::sandbox`, the window's `broker.rs`). The engine reads what
services, sites and models answer, so it runs where a flaw in it reaches little: an AppContainer named after the app's
identifier (`app.ihatecrypto.windows.engine`), made once per user, with two capabilities — connections out to the
internet and to the home network (a person's own node) — and a rule that it may start no process at all
(`PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY`). From there, checked on Windows 11 (`native/examples/acprobe.rs`), it
can't read the person's files (Documents), read or write the signer's folder (so it can't delete `pending.json`),
write the registry's Run key or the Startup folder, reach Task Scheduler, start a program, open a USB device, or take
a connection from the browser; DPAPI, Windows Hello's checks, the internet and the session's idle time work as before.

Its files are in the container's own folder (`%LOCALAPPDATA%\Packages\app.ihatecrypto.windows.engine\AC`, which
Windows gives it as LOCALAPPDATA and TEMP, and the window names them too): `ihatecrypto\engine` there. On the first start
in the container the window copies the old `%LOCALAPPDATA%\ihatecrypto\engine` in, file by file so the copies take the
container's rules, and renames the old one `engine-before-container`; a copy that fails leaves it as it was and the
engine unstarted, so the next start tries again. DPAPI-sealed keys and sign-ins read as before. A development build keeps IHATECRYPTO_ENGINE_DIR, which the
window lets the container into.

What the container doesn't allow, the window does — the Mac's Helper.xpc. The engine asks on two pipes of their own
that only the two hold (no other program can write to them, so no caller check is needed), and each request is one
fixed thing (`broker.rs`), with ceilings — 32 requests, 3 Claude Code runs and 4 pages at once, 4 MB a line:

| Request | What the window does |
| --- | --- |
| `signer` | starts the signer beside the engine, hardened as before, and hands the engine its pipes and a handle that can only wait for it; the signer accepts the window (`ihatecrypto.exe`, the folder above `bin`) as its parent too |
| `claude` | runs the installed `claude.exe` with arguments built in the window: `-p --restricted --strict-mcp-config --tools "" --no-session-persistence …`, a model name that must be `claude-` and `[a-z0-9.-]`, the system prompt (in a file of the window's, `--system-prompt-file`: the chat's is longer than Windows' 32,767-character command line), the effort; the prompt goes through the stdin the engine gets |
| `find` | says whether Claude Code, Codex CLI's login or Ollama is on this PC — yes or no |
| `codex` | reads the Codex CLI login the person picked once in the window's Open dialog (kept in the window's folder) and hands over the access token, account and id token, never the refresh token |
| `serve`, `pageReply`, `unserve` | listens on 127.0.0.1 (any free port, or 1455 for ChatGPT's sign-in) for the Top up page and the sign-in redirects, hands each request over whole (64 KB, 5 s) and gives the browser the engine's answer |
| `claudeInstall`, `claudeSignIn` | Anthropic's installer (claude.ai/install.ps1, only if the download stayed on claude.ai); `claude auth login` in a terminal |
| `ollamaStart` | Ollama's app, or its server |
| `reveal` | the journal, copied into the window's folder and shown selected there, or the study notes folder, opened only while it holds nothing but .md and .txt files and no link: Explorer reads a folder's desktop.ini and a shortcut's icon as the person, outside the container |
| `locked` | whether the session is locked: the container can't open the input desktop |
| `hello` | the app lock's Windows Hello, with the unlock key the window names (the container has no package for Windows Hello to keep a key under); only signed, cancelled or failed goes back |
| `ledger` | the first Ledger's USB handle, opened by the window and handed over (the container can't open a device) |

Windows keeps a container off 127.0.0.1 both ways. Out: the per-machine installer, which runs as an administrator, lets
the engine's container reach this PC's own servers (`windows/hooks.nsh`, CheckNetIsolation's loopback exemption by the
container's SID), so Ollama and LM Studio answer; the uninstaller takes it back. The exemption is for every port on
127.0.0.1, not only theirs: an engine taken over could still reach this PC's own servers (a debugging port, a database,
Ollama's whole API), as it could before the container. Narrowing it means the window carrying Ollama's requests itself.
In: no exemption allows it outside debugging, hence the window's listener. Getting Ollama opens its download page
instead of running its installer, and an Ollama older than the newest is used as it is.

Left as it was: the window itself isn't contained. WebView2 in a custom AppContainer isn't something Microsoft
supports for an unpackaged app, and the window's tray, notifications, Run key, Keep running's scheduled task, updates
and microphone would each need a broker of their own; the pages it shows run in WebView2's own sandbox, and a site
reaches the window only through `dapp_call`. A Ledger over Bluetooth (WinRT in the container) isn't checked yet, nor is
the terminal tool's journal in `~\.ihatecrypto` read any more (out of the container's reach).

## Keys and Windows Hello

- **The seed** is sealed with AES-GCM under a key derived (HKDF) from a Windows Hello signature: Windows Hello signs a
  random challenge with an RSA key in the TPM after the face, the fingerprint or the PIN, and the signature is the same
  for the same challenge, so the key comes back only with the person (KeePassXC's approach). The sealed seed is then
  encrypted to the Windows user with DPAPI. Files: `%LOCALAPPDATA%\ihatecrypto\signer`.
- **One prompt per move**: every key comes from one signature (`Vault.master`), held for the session the user picked
  (one move by default), then dropped — as Touch ID on the Mac.
- **The signer's window** shows what it parsed itself while Windows Hello asks: amounts, addresses, the network fee
  (`winsec_hello_sign`). The engine and the window never see a key.
- **Trusted settings** — the wallets and their addresses, which one signs, the address book, the approved and own
  addresses, the session length, the trading keys' switches — sit in `state.json` sealed with an HMAC under the same
  key, and the seal is checked at every prompt. A program that edits the file (adds its address to the approved ones,
  swaps a saved name's address) stops every signature (`Trusted.tampered`). Changes that only take trust away (a
  rename, a removal, a pause, a shorter session) may be made with no prompt and wait in `pending.json`, which counts
  only as far as it takes trust away; anything else asks Windows Hello first.
- **The Hyperliquid trading key** places orders with no prompt; it can't withdraw. A Mac keeps it in the signer's
  Keychain group, which no other program reads; on Windows it is sealed as a seed is, under the Windows Hello key, and
  opened once per signer run — by the first Windows Hello the signer asks for anything, or by a prompt at the first
  order — then kept only in the signer's memory. Its address is kept beside it and recorded in the trusted settings, so
  Settings shows it without opening it. A DPAPI-only key file beside a wallet whose keys are sealed is never read: one
  another program puts there is neither shown, used for an order nor approved — turning the key on makes a new one
  inside the signer and removes that file. A Ledger's trading key is sealed the same way, even when nothing has asked
  Windows Hello yet (a Ledger brought in at the first run skips the steps that do): turning it on makes the Windows
  Hello key then, one prompt, so a file put there first is never the key the Ledger is asked to approve, and the key
  stays after the Windows Hello prompts that come later. (Keys kept DPAPI-only by builds before 2026-10-24 were on test
  machines; they are left, and the key is approved again once.)
- **After a restart, until that first Windows Hello**, the guardian can't move a stop by itself, as it can on a Mac. Its
  first try asks for the trading key, a prompt that closes after a minute; unanswered, that order is refused with why —
  never handed to the main key — and for ten minutes the key doesn't ask again for orders that only reduce (stops,
  takes, closes), so a stop retried every 10 s doesn't keep a prompt up or hold the signer. The stop and take on the
  exchange protect the position meanwhile. An order that opens or adds asks at once (someone just confirmed its card),
  and any Windows Hello answered for anything opens the key. With nobody at the computer that is one prompt every ten
  minutes, each closing after a minute (the signer answers nothing else for that minute), and the guardian says its
  reason in the chat once, not once a spell.
- **Test and automation wallets** sign with no prompt, so they are under DPAPI only, with their trading keys: any program
  of the same user could read them. They are for a few dollars, as on the Mac.
- **A scheduled move's Windows Hello** (the automation wallet asking for a new address) closes by itself after a minute
  unanswered, as a Mac's Touch ID prompt does (`winsec_hello_sign_within`); so does the trading key's own prompt.
- **The passcode** only opens the app (PBKDF2 under DPAPI, kept by the signer); Windows Hello signs every move.

### Other programs of the same user

Windows lets a user's programs read each other's memory and files; a Mac's sandbox and Keychain groups don't. The
signer narrows that (`winsec_harden`, `winsec_spawn_signer`):

- its process and threads are created with an access rule that lets the user's other programs only see, wait for and
  end them — no memory reads, no new threads, no rule changes (an administrator still can, as root on a Mac);
- no extension DLLs (global hooks, AppInit), no remote images, no child processes;
- it serves only the engine that started it, from its own folder, or the window from the folder above, signed with the
  same certificate when it is signed (`winsec_parent_check`, `winsec_parent_check_above`); in an install for all users
  (Program Files, the installer's mode) the user can't put a file there.

What remains: a program of the same user can open the same Windows Hello key by name and ask for a prompt itself, and
read the engine's memory. It still needs the person to answer a prompt, and the Guard and the signer's checks hold for
whatever reaches them: money goes only to the wallet's own addresses and to addresses confirmed in person.

Files such a program can put back, which a Mac's Keychain would keep from it; nothing on Windows short of the TPM's own
counters can tell an older copy from the newest, so these stay:

- **An older `state.json`**, sealed then, passes the seal: it can bring back an address removed since — a saved name,
  an address approved for money — until the next change seals a newer one. Every move still asks Windows Hello with
  the address shown; only the automation wallet's moves, which ask nothing, could then pay such an address.
- **`pending.json` deleted** undoes a change that only took trust away and wasn't sealed yet (it is at the next
  Windows Hello). The engine, in its container, can't touch the signer's folder; another program of the user can. While the signer runs it notices and stops every signature; after a restart it doesn't. The trading
  key's pause is the one that matters, and the paused key stays sealed under Windows Hello, so the program still can't
  use it.
- **The scheduled tasks' key** (`task-seal.key`, DPAPI): a list with no key beside it loads paused, but a program that
  reads the key can tag a list of its own. It can read the automation wallet's seed too, which is the larger matter.

## Building

On a Mac, to work on the window (the engine and the signer are Rust, docs/windows-rust.md; off Windows winsec's
stand-in guards nothing and fakes Windows Hello only with `WINSEC_FAKE_HELLO=1`):

```sh
cd App
cargo build --manifest-path Windows/rust/Cargo.toml -p engine -p signer
cargo test --manifest-path Windows/rust/Cargo.toml -p signer --test vault
mkdir -p Windows/app/src-tauri/bin && cp Windows/rust/target/debug/ihatecrypto-{engine,signer} Windows/app/src-tauri/bin/
cd Windows/app/src-tauri && cargo build
WINSEC_FAKE_HELLO=1 IHATECRYPTO_SIGNER_DIR=/tmp/s IHATECRYPTO_ENGINE_DIR=/tmp/e \
  IHATECRYPTO_ENGINE=$PWD/bin/ihatecrypto-engine target/debug/ihatecrypto
```

To look at the window without the engine, `App/Windows/app/preview/shot.sh out.png "fixture=main&theme=dark"` draws
a state from `preview/fixtures` (demo balances on the public test addresses; `make-fixtures.py` writes the main ones) in
headless Chrome, which is what WebView2 is; `&view=settings` opens a page, `&store={"railCollapsed":true}` folds the rail.

A debug build also runs lines of JS appended to `IHATECRYPTO_UI_EVAL` in the page, so a test can press buttons with
the window in the background.

On Windows, from `App/`: `pwsh Windows/build.ps1` builds the engine and the signer in release and makes the installer
(`Windows/app/src-tauri/target/release/bundle/nsis/ihatecrypto_<version>_<x64|arm64>-setup.exe`). It needs Rust,
Node (for the Tauri CLI) and Visual Studio's C++ tools. The GitHub workflow `windows.yml` does the same on every push
to `windows` and keeps the installer as an artifact for two weeks.

## Signing and updates

- **Authenticode**: with `WINDOWS_PFX` (a code-signing certificate) and `WINDOWS_PFX_PASSWORD`, `build.ps1` signs both
  programs, and Tauri signs the window and the installer. Unsigned, Windows SmartScreen warns on install. The
  certificate is bought (an OV/EV certificate, or Azure Trusted Signing), then given to the workflow as secrets.
- **Updates**: the Tauri updater checks
  `https://github.com/ihatecryptoai/ihatecrypto/releases/download/windows-latest/latest.json` and installs only an
  archive signed with the key whose public half is in `tauri.conf.json`. The private key and its password are in the
  release Mac's Keychain (`ihatecrypto-windows-updater`, `ihatecrypto-windows-updater-password`), and the key stays
  there: the installers are signed on that Mac.
- **A release** (0.9.0, 2026-11-07, the first one): the version in `tauri.conf.json` and the window's `Cargo.toml`, the
  PC's own What's New in `whatsnew.py`, a push to `windows` (CI builds the x64 installer as an artifact) and
  `build.ps1` on a Windows on ARM machine for the ARM64 one. On the Mac, `npx @tauri-apps/cli@2.12.4 signer sign <installer>`
  with the key from the Keychain in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` writes each
  `.sig`; `latest.json` names both (`windows-x86_64`, `windows-aarch64`) with their `.sig` text and their URLs on the
  versioned release. Both releases are pre-releases, so the repository's «Latest» stays the Mac's: `windows-v<version>`
  holds the installers, their signatures, `latest.json` and SHA256SUMS; the rolling `windows-latest` holds `latest.json`
  and the installers again under fixed names (`ihatecrypto-windows-x64-setup.exe`, `-arm64-`), which the landing links.
  The installer doesn't remove files an older version left in `bin\`: a PC that had a Swift build keeps its DLLs there,
  unused.

## What the Windows app has, and what it doesn't yet

The window is the Mac's Studio, with the Mac's colors, fonts and logos in light and dark: the wallet rail floating
over the dotted canvas as a glass panel (balance, Top up, Portfolio, History, Favorites, Schedule, the canvases, the model
accounts) that folds to a strip by its button or on its own in a narrow window; the canvas with the portfolio's frame,
pan, zoom and «Add block»; the chat floating over it. Windows has no traffic lights, so the window draws its own
minimize, maximize and close at the top right and is dragged by its empty top strip; Windows Hello stands where the Mac
says Touch ID.

Has: canvases as on the Mac (each with its own chat and blocks: the portfolio framed on the left, a move as its route's
cards with the balance it spends now → after, results the chat makes — a chart, the rates, approvals, P/L, a token, a
service's answer — and blocks added by hand, the newest on top; nothing leaves a canvas unless the person closes it;
right-click a block to ask about it in the chat), the chat floating over the canvas with Confirm in its field and a mic (Windows' own speech recognizer in the
keyboard layout's language; the words go into the field, never sent by themselves), a Ledger over USB or Bluetooth
(winsec's HID and WinRT GATT calls under the `ledger` crate: «connect my Ledger» reads one address from its Ethereum app and one from its Solana
app, «use my Ledger» makes it sign every move on its screen with the same guard first, «show on the Ledger» to compare
an address, and it can be a key of a multisig), the dApp browser
(https sites beside the chat in the page panel, as the Mac's WebPanel — the panel moves by its header, widens by its
edge, docks on a double click and keeps its place; «Move to its own window» takes the same page out — the Mac's toolbar across its top — back, forward, reload, the address with the
site's check (known, a lookalike of which, unknown), «Connected · Disconnect», the wallet the site would get — as our own
page in a webview of its own above the site's, with ihatecrypto as a Solana Wallet Standard wallet and an
EIP-1193/EIP-6963 provider in the site; each connect, message, typed data or transaction is a card on the canvas — the Solana guard and simulation,
the EVM call decoded and simulated, the site checked against MetaMask's phishing list and for lookalikes — and nothing
is signed before Confirm; a site's page may call only the wallet command, never the engine, the toolbar's commands,
links or events, its origin is read from its webview, not from the page, and only the page itself gets the wallet —
a frame inside it, an ad or a widget, gets none and its call is refused), a Safe
multisig (made from a card where its keys, how many sign and its networks change before Confirm; paid out of when this
computer's key is enough), wallets (create with the phrase shown once, import a phrase or a key, switch, rename, remove, a test wallet; or, with none, addresses only watched for their balances, as the Mac's «Addresses for balances»: nothing signs or connects for them),
Receive, the address book, the portfolio on Solana, Hyperliquid, every EVM network and NEAR, the chat with a model
(an API key, a browser sign-in to ChatGPT or OpenRouter caught on 127.0.0.1, a local model, or a login another app
has) or plain commands without one, every money command the Mac's
chat runs through the Executor (transfers, swaps, bridges, lending, Hyperliquid spot), Hyperliquid trades by the risk
rules with every signature on the card, managing open positions, the best rates and spreading or rebalancing money
across them, Merkl's rewards (claim, or sweep them into USDC), token approvals with a revoke on each, Jupiter DCA,
coins from networks without a wallet here (NEAR Intents' deposit address), several moves in one message under one Confirm,
price alerts and scheduled tasks with recurring buys (checked every 30 seconds while the app runs, beside the balances so an unanswered Windows Hello holds only that run, with a Windows
notification), the automation wallet (a task the person switched on in the Schedule block spends from it without
Windows Hello, only to their own wallets and confirmed addresses, and what it buys goes on to the main wallet; a sale
and a recurring buy wait for Confirm on the wallet that's on; without it a task's moves come as cards), the tray (closing the window keeps it running, as the Mac's menu bar
does), price charts and the day's movers in the chat, P/L (Hyperliquid's trading after fees and
funding, open positions, the portfolio's change without money moved in or out, and each coin's P/L against what it
cost: the chain's history priced on the day, the app's own buys, or when first seen), favorites in two tabs (tokens with their prices; your wallets and the saved addresses, starred first, with Send),
ENS and SNS names saved as the address they point at, contract calls («call base 0x… f(…) … approving N USDC»: the
contract checked on the explorer and for its reputation, the exact approve and the call simulated together before the
card, simulated again before signing, never inside a chain), favorite tokens and token cards (DexScreener), paid requests to services (MPP: the price on a card, paid in USDC
on Tempo after Confirm, the services the model can look up), skills (the built-in Trading study with its journal and notes, and the user's own .md or .txt), «remember: …», requests of several steps (the model hears how each step went, after its Confirm when it moves money,
and takes the next, five steps at most; «Next, once this is through: …» under it), «check: …» tasks on the schedule
(the model looks itself and speaks, with a notification, only when the condition is met),
the second check (on by default, as on the
Mac: the model reads every command and trade plan again, Jev through OpenRouter when allowed, and code checks that a
plan holds no more money or risk than named), history from the journal, lock and the session
length, signed updates.

Since 2026-10-24 also, as on the Mac: a route step's menu («Why this way?», «Edit step…» with the amount and, for a
Base/Arbitrum trade, the way with both ways quoted, «Send to chat», «Copy details»); a Solana swap's card quoted by
Jupiter and held to it; Favorites' «+» that tells a token from a wallet; the Schedule's History tab, its pause for all
tasks and the automation wallet's budget; a multisig's money per network; tokens hidden and shown in the portfolio;
pinch zoom on a touchpad or a touch screen; «In transit» and a place's details board; Receive's QR codes; the dApp
browser's Back and Forward enabled only when there is somewhere to go (WebView2's own history), a page that fails to
load said under the toolbar, phishing hosts never loaded (the check holds while the app is locked), a site's
new-window link opening nothing (a phishing host gets the banner), no alerts, file pickers (an input outside the page
too), print preview, `window.close()`, `Notification` or element fullscreen for sites, and none of WebView2's own prompts (permissions,
HTTP sign-in, client certificates, «open this app?» from a frame, autofill), a certificate error refused with the red
«certificate» line and no way past, a server's own error page (a 404) shown with no «didn't load» line, a
page that asks to close closing the browser as its × does, right-click
in a field or on selected text giving Cut/Copy/Paste, «open jup.ag» from the chat, and links opening beside the
chat in a page panel (the Mac's WebPanel); Settings › Trading agent (styles, limits, rules, safe search), Modules (every
network and protocol, «off» refused before a card), the Hyperliquid trading key (offered after the first money there,
paused by lock, a key of its own for a Ledger), own nodes and the NEAR Intents key (DPAPI); dollars without gas moving
by signature or with Jupiter paying, offered once a network.

Then the rest of the Mac: the lock screen with a 6-digit code (kept and checked by the signer, PBKDF2 under DPAPI;
locks at launch, after 1/5/15/60 minutes away, on Win+L, after sleep, on Ctrl+L; five wrong codes leave Windows Hello
only); the staged first run (Model › Wallet › Secure › Networks › Top up › Trading), Welcome's demo and languages, and
importing with a BIP-39 passphrase with the first addresses compared; the chat's Stop (Esc), its thinking step, a
priced swap's 15-second countdown and Refresh, the amount slider, «/», «@» and «$» lookups, several attachments,
offer cards kept with their reply, «Executed · N steps», checklist and service cards, the live voice transcript, the
model menu's and the context meter's states; canvas pins, folders and icons; the History board's day and the History
page's Agent, Positions and All tabs; Top up's browser-wallet way (a one-time page on 127.0.0.1), amounts, QR codes
with the amount and «Landed on …»; price alerts every 10 s with their THEN steps; a Schedule row's own canvas; a trade
plan drawn on its chart; Robinhood's stock routes; the Hyperliquid details (HIP-3 exchanges, HLP and its lock) and the
position guardian (the half at 1R, the stop to breakeven after the first take, trailing stops).

And the rest of a second audit: each move drawn as its route with a balance box per step and checked before its card
(the shortfall and its choices, a first send, an address saved for another network, a saved address by its name);
the trade plan's whole route with what stops it and its fix; the chat's own commands, folders and notes, and answers
when no model does; the token card and the rich boards (P/L, rates, movers, approvals, DCA, a paid answer); scheduled
runs on their own canvas while the person is away; «Think before answering», the model check, Claude Code and Ollama
installed from the app, «Keep always running». Main's second security review is ported too: Relay deposits bound to
the order the signer checked, scheduled tasks sealed (HMAC under DPAPI: a list changed outside the app loads paused),
paid services only from Tempo's directory, a site transaction's target checked, the signing session ended per move and
on a locked screen or sleep.

And a third audit's: chains of every step kind the Mac chains (a chart, a token, P/L, an alert, the address book, a
folder, lock, a position or a trade plan beside the moves, «How much?» in the middle holding the rest, bridge legs
landing side by side with an «ARRIVES · SOLANA» step, the next step spending what arrived, a send no balance covers said
before its card); every move drawn step by step with its quote on its step (stocks, EVM tokens, swaps off Solana and
across networks, revoke, NEAR, pay, multisig, CCTP, USDT0, Aave, Merkl, a saved address, closing a position) and the
15-second countdown on every priced swap, a chain's first move too (it's quoted, counted down, refreshed and held to
its price as a card of its own; its amount or way can be changed with the rest kept behind it); a Hyperliquid spot trade shows its quote on
the card without holding Confirm, as on the Mac; a quote that doesn't come shows Refresh, and a swap Jupiter can't price
keeps a plain Confirm; the chat's quote line warns when fees take more than a tenth, and a coin sent through NEAR
Intents is quoted in its own words; the cards' warnings (an XRP exchange address without its tag, a token by its
contract, Robinhood's stock tokens, Aave's liquidation price, Jupiter Lend instead of Kamino's rent); a price alert
added to the plan as its first step; the position board's ruler, guardian and «Close X at market?»; Jupiter's staked
JUP, perps and predictions in the portfolio; the token card's held P/L; the plan templates' preview; Skills' Study notes
and Journal.

A fourth and fifth audit's, all as on the Mac: a site's request in a slot of its own, never taking the person's card,
with a tick for each risk (a lookalike, an unknown program, a payee or contract not yours, the second check's warning)
and gone with its page; a trade the second check doubts needs its tick too; Confirm and Cancel name their card; a wallet
switch drops what was built for the other one; wallets imported from a key move money; automation tasks run as one
confirmed chain, and a step that names no amount stops the run instead of spending everything; the builder fee asked on
the first Confirm; a trade confirmed late gets fresh numbers; «Sent, status unknown»; a waiting limit order; the guardian
every 10 s; recurring buys at their time, per wallet, kept by mint; a model per canvas and per account, and an account
connected never switching the chat; the plan, cards and skills kept per canvas; History in a store of its own; what the
app does by itself recorded; the chart board, the move board's countdown, what gets signed under the route; nothing
leaving the canvas; the «$» chip; Robinhood stock tokens at Robinhood's bid; Aave and Morpho with their rate and last
reading; the dApp browser beside the chat in the page panel.

Everything the Mac's window does is in the Windows app; what only a Windows machine can check is listed below.

## Checking on a Windows machine

A runner has no Windows Hello, microphone, Ledger or person at it, so these are checked by hand on Windows 11 with a PIN set:

1. Install from the workflow's installer; Windows asks for an administrator (Program Files).
2. Settings › Signing says «Windows Hello is ready; its key is in the TPM» and no hardening notes.
3. Create a wallet: Windows Hello asks twice (making the key, then signing), the phrase shows, the window can't be
   captured meanwhile (Win+Shift+S shows it black), «Copy all 12 words» leaves nothing in Win+V history.
4. Save an address: the signer's window shows the whole address while Windows Hello asks.
5. A move on a test amount: one prompt, the signer's window shows the amounts it parsed.
6. Edit `%LOCALAPPDATA%\ihatecrypto\signer\state.json` by hand: the next signature refuses with the «changed outside
   it» message.
7. Process Explorer as the same user can't read the signer's memory (access denied).
8. The mic in the chat field: Windows asks for the microphone once; Russian words with the Russian layout on, English
   with the English one; with Settings › Privacy & security › Speech › Online speech recognition off it says so.
9. The browser: open jup.ag (it opens beside the chat; «Move to its own window» keeps the page and its connection), connect, and a swap's card comes to the canvas with its simulated balances; on a site's
   page, `__TAURI_INTERNALS__.invoke('engine_send', …)` (or `browser_nav`, `browser_state`) in DevTools is refused; the
   toolbar's Disconnect takes the address away and reloads the page. Back and Forward turn on after a link and after a
   single-page app's route change; an unreachable site (jup.ag times out on some networks) shows the red line; a host on
   MetaMask's phishing list doesn't load; a block's link opens beside the chat, and that panel's page is refused our
   commands too. A frame inside a site gets no wallet: in a development build, a sandboxed `srcdoc` frame's
   `invoke('dapp_call', …)` is refused while the page's own connect comes to the canvas.
   A site's `window.open(…)` opens nothing; `alert(1)` shows no dialog and an `<input type=file>` opens no picker; a
   listed host reached while the app is locked is still stopped and named after unlocking.
   A file input made by script and never put in the page, or a label of one, opens no picker on a real click; a
   frame sent to `ms-settings:` opens nothing; `window.print()` and `window.close()` do nothing; `typeof Notification`
   is "undefined"; expired.badssl.com is blank under the «certificate» line, with no «Continue».
   With a page that first replaces `preventDefault`, `composedPath`, `Function.prototype.call` and `Reflect.apply`, a
   real click on a file input still opens no picker; client.badssl.com's 400 page shows with no red line;
   `document.fullscreenEnabled` is false.
10. A Ledger over USB: «connect my Ledger» reads both addresses; a 1 USDC send shows on its screen. Then a Nano X, Stax
    or Flex over Bluetooth the same way (Windows may ask to pair it first).
11. A scheduled buy switched on in the Schedule block runs from the automation wallet with no Windows Hello, and what it
    bought arrives on the main wallet.
12. The engine's container: `acprobe token <engine pid>` (`cargo build --example acprobe` in App/Windows/native) says
    AppContainer, the container's SID, low integrity, its two capabilities and no child processes; `acprobe run` started
    from the user's session (a scheduled task with /IT: a container started from an SSH session has no desktop and
    fails with 0xC0000142) runs the same checks from inside the same container. The chat answers on the Claude Code
    subscription, Ollama's models list (after the installer's loopback rule), and a Windows Hello signature still works.

### What a Windows 11 machine has shown so far

On Windows 11 ARM64 (build 26300) in a virtual machine with a software TPM, 2026-10-23: `build.ps1` built and the
installer installed; the Swift package's tests pass there (485; the network ones and the stand-in Hello's skipped).
Steps 1, 3, 4, 6 and 7 hold: the phrase window is left out of a screenshot, the copied words carry the formats that
keep them out of Win+V history and the cloud and are cleared after a minute, a process of the same user is refused the
signer's memory, its threads and their context. In the browser app.hyperliquid.xyz's Connect lists ihatecrypto, its ask
comes to the canvas as a card with a Windows notification, and after Confirm the site shows the address. Step 2 says
the key can't be shown to be in the TPM: a software TPM can't attest it, a real one should. What that run fixed: Windows
Hello's dialog opening behind the window, the signer's details covering the dialog's title or cut short, a white browser
window, a window taller than a small screen, a false «threads' rule» note. Still to check on a real PC: a move (5), the
mic (8), a site's page refused `engine_send` (9), a Ledger (10) and the automation wallet (11).

The Studio window on the same machine, 2026-10-23: WebView2 draws it as the preview does (the glass, the dots, Onest,
the logos); the caption buttons minimize, maximize and restore, a double click on the top strip maximizes; on its
800×600 screen the rail folds by itself and the window and the browser open maximized; the browser's toolbar shows the
site's check and the wallet over app.hyperliquid.xyz. What that run fixed: the folded rail's Profile below a short
window's edge, the toolbar never hearing a state (it looked for «event» at the start of the line, where Windows'
encoder needn't put it), and the toolbar's address empty while a page is still loading.

Wave two on the same machine, 2026-10-24: the passcode made and turned off through the real Windows Hello prompt, the
lock screen (a wrong code counted, the right one opens), the tray panel, a test notification, «Open at sign-in» writing
and removing its Run key, one instance; the browser's Back/Forward, the red line for a site that doesn't answer, and
typing in its address field — which needed two fixes found there: the toolbar redrew six times a second (each state
redrew it and asked siteState, and every request, even a read, sent a state), and a redraw moved the field out of the
page, dropping its focus. A programmatic focus on a click made it worse and is gone: WebView2 focuses what's clicked.

The frame check on the same machine, 2026-10-24, in a development build: WebView2 gives a frame inside a site Tauri's
IPC too, so the wallet's script now stays out of frames and `dapp_call` refuses a call whose Origin isn't the page's. A
sandboxed frame got no wallet and its call was refused; a frame of the page's own origin got no wallet object either
(its direct call reaches the card, as the page's would); the page's own connect came to the canvas. With `fetch`
broken, as a strict Content-Security-Policy does (app.drift.trade has one), Tauri falls back to `chrome.webview.postMessage`:
the page's call still came to the canvas, and neither frame's ever arrived — WebView2 hands only the page's messages to
the webview, so a call without an Origin header is the page's own.

Later on 2026-10-24, the same machine at 00a8388 and after: the whole package's tests pass there (a full run took about
two and a half minutes once engines inside the test process stopped writing their states — before, `swift test`
stalled on that output in two runs of three). By hand, in a development build: a typed message sends and is answered
(it hadn't since 3543cc1: the page's send() threw with nothing attached); «send 10 USDC on arbitrum to Multisig» with
no Safe there is refused; app.hyperliquid.xyz opens beside the chat with the wallet's row, and its connect stepping the
panel aside for the request; inside the panel a sandboxed frame is refused and neither frame gets a wallet object;
Quit Completely with Keep always running on leaves no process after 90 s, the task disabled and a RunOnce entry,
the next launch turns the task back on and removes the entry, and switching Keep running off removes both. The signer now runs in its engine's job object (kill on close): with the engine killed, the signer was gone
too — before, a signer could outlive its engine, holding the pipes it was given and any Windows Hello prompt it had open.
A full `swift test` there can still stall in its own output at about 74 KB, mid-way through the core's tests, with
`swift-test` spinning; each engine suite run on its own passes, so check with `swift test --filter EngineTests.<Suite>`.

At 0.7.7 (8f9e9f9, wave 13) on the same machine: the installer built and installed; every test target, every engine
suite and the tests outside a suite pass one by one. In the release build a right-click in the chat field gives only
editing items (Cut, Copy, Paste, Select all, Undo, emoji), on a block only the app's own menu, on the rail none. In a
development build, from a site's page: `window.open` opens nothing and Edge never starts, a host on the phishing list
gets the banner while the page stays; `alert`, `confirm` and `prompt` give undefined, false and null with no dialog; a
file input opens no picker, from a script or a real click; a `beforeunload` handler can't hold the page; an http link
leaves the red «https only» line and the open page's address.

At 00f72cf (wave 14): every target, engine suite and suite-less test passes again. With the saved phishing list aged two
days and the app restarted, a listed host opened as the very first site never loads and gets the banner; a file input
made in another frame and moved into the page opens no picker on a real click; a site asking for the location is
refused with no prompt.

At 6ba6913 (wave 15): 59 test runs pass. In a site: `typeof Notification` is "undefined"; a script's file input never
put in the page, and a label of one, open no picker on a real click; a frame sent to `ms-settings:` starts nothing and
asks nothing; `print()` and `close()` do nothing; client.badssl.com gets no certificate picker; expired.badssl.com is
refused with no way past.

With the engine and the signer in Rust (docs/windows-rust.md), 7684108c and 58eb7f17, 2026-11-06: the development
window started both, the public test phrase went in through the real Windows Hello PIN prompt, the portfolio loaded and
«chart SOL» drew its card; `build.ps1` made the arm64 installer with only the two programs in `bin\` (11.7 MB). The
Swift Windows code is gone since: App/Sources, App/Tests and the package are the Mac's as of the last merge of main.
