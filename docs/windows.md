# ihatecrypto for Windows

The Windows app shares the Mac's core: the wallet, the chains, the protocols, the Guard, the plans and the execution
(everything under `App/Sources` except `IhatecryptoApp` and `Signer/Service`). What differs is the window, the engine
that runs the chat in place of the Mac's `Workspace`, and where the signer keeps its keys.

```
ihatecrypto.exe (Tauri)          App/Windows/app        the window: the page in ui/, no key, no money logic
  └─ ihatecrypto-engine.exe      Sources/Desktop/Engine the chat, the cards, Confirm, balances (Swift, the Mac's core)
       └─ ihatecrypto-signer.exe Sources/Signer/Core    what the signer decides, the same code as the Mac's
                                 Sources/Signer/Windows keys under Windows Hello, the trusted settings, the pipe
            └─ winsec.dll        App/Windows/native     Windows Hello, DPAPI, hardening (Rust, windows-rs)
```

The window starts the engine with pipes; the engine starts the signer with pipes. Each line between the window and the
engine is one JSON request (`Engine.Request`) or one answer or event (`Outbox`); the engine sends the whole state after
every change. Between the engine and the signer go the Mac's own `SignerRequest` and `SignerReply`, framed by a
4-byte length.

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
- **Test and automation wallets** sign with no prompt, so they are under DPAPI only: any program of the same user could
  read them. They are for a few dollars, as on the Mac.
- **No passcode** on Windows: Windows Hello itself asks for the PIN.

### Other programs of the same user

Windows lets a user's programs read each other's memory and files; a Mac's sandbox and Keychain groups don't. The
signer narrows that (`winsec_harden`, `winsec_spawn_signer`):

- its process and threads are created with an access rule that lets the user's other programs only see, wait for and
  end them — no memory reads, no new threads, no rule changes (an administrator still can, as root on a Mac);
- no extension DLLs (global hooks, AppInit), no remote images, no child processes;
- it serves only the engine that started it, from its own folder, signed with the same certificate when it is signed
  (`winsec_parent_check`); in an install for all users (Program Files, the installer's mode) the user can't put a file
  there.

What remains: a program of the same user can open the same Windows Hello key by name and ask for a prompt itself, and
read the engine's memory. It still needs the person to answer a prompt, and the Guard and the signer's checks hold for
whatever reaches them: money goes only to the wallet's own addresses and to addresses confirmed in person.

## Building

On a Mac, to work on the window (the stand-in `Sources/Native/WinSec/winsec_stub.c` takes winsec.dll's place; it
guards nothing and fakes Windows Hello only with `WINSEC_FAKE_HELLO=1`):

```sh
cd App
IHATECRYPTO_DESKTOP=1 swift build --scratch-path .build/desktop --product ihatecrypto-engine
IHATECRYPTO_DESKTOP=1 swift build --scratch-path .build/desktop --product ihatecrypto-signer
IHATECRYPTO_DESKTOP=1 swift test --scratch-path .build/desktop --filter VaultTests
mkdir -p Windows/app/src-tauri/bin && cp .build/desktop/debug/ihatecrypto-{engine,signer} Windows/app/src-tauri/bin/
cd Windows/app/src-tauri && cargo build
WINSEC_FAKE_HELLO=1 IHATECRYPTO_SIGNER_DIR=/tmp/s IHATECRYPTO_ENGINE_DIR=/tmp/e \
  IHATECRYPTO_ENGINE=$PWD/bin/ihatecrypto-engine target/debug/ihatecrypto
```

To look at the window without the engine, `App/Windows/app/preview/shot.sh out.png "fixture=main&theme=dark"` draws
a state from `preview/fixtures` (demo balances on the public test addresses; `make-fixtures.py` writes the main ones) in
headless Chrome, which is what WebView2 is; `&view=settings` opens a page, `&store={"railCollapsed":true}` folds the rail.

A debug build also runs lines of JS appended to `IHATECRYPTO_UI_EVAL` in the page, so a test can press buttons with
the window in the background. On Linux the core, the signer and the engine build in the `swift:6.2` container.

On Windows, from `App/`: `pwsh Windows/build.ps1` builds winsec.dll, the engine and the signer in release, copies the
Swift runtime beside them and makes the installer
(`Windows/app/src-tauri/target/release/bundle/nsis/ihatecrypto_<version>_<x64|arm64>-setup.exe`). It needs Rust, Swift 6.4.1,
Node (for the Tauri CLI) and Visual Studio's C++ tools. The GitHub workflow `windows.yml` does the same on every push
to `windows` and keeps the installer as an artifact for two weeks.

## Signing and updates

- **Authenticode**: with `WINDOWS_PFX` (a code-signing certificate) and `WINDOWS_PFX_PASSWORD`, `build.ps1` signs every
  program and DLL, and Tauri signs the window and the installer. Unsigned, Windows SmartScreen warns on install. The
  certificate is bought (an OV/EV certificate, or Azure Trusted Signing), then given to the workflow as secrets.
- **Updates**: the Tauri updater checks
  `https://github.com/ihatecryptoai/ihatecrypto/releases/download/windows-latest/latest.json` and installs only an
  archive signed with the key whose public half is in `tauri.conf.json`. The private key and its password are in the
  release Mac's Keychain (`ihatecrypto-windows-updater`, `ihatecrypto-windows-updater-password`); given as
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `build.ps1` makes the signed update archive.
  A release puts the installer, the archive and `latest.json` on a `windows-v<version>` release and `latest.json` on
  the rolling `windows-latest` one.

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
(winsec.dll's HID and WinRT GATT calls under the Mac's Ledger module: «connect my Ledger» reads one address from its Ethereum app and one from its Solana
app, «use my Ledger» makes it sign every move on its screen with the same guard first, «show on the Ledger» to compare
an address, and it can be a key of a multisig), the dApp browser
(a window of its own for https sites, the Mac's toolbar across its top — back, forward, reload, the address with the
site's check (known, a lookalike of which, unknown), «Connected · Disconnect», the wallet the site would get — as our own
page in a webview of its own above the site's, with ihatecrypto as a Solana Wallet Standard wallet and an
EIP-1193/EIP-6963 provider in the site; each connect, message, typed data or transaction is a card on the canvas — the Solana guard and simulation,
the EVM call decoded and simulated, the site checked against MetaMask's phishing list and for lookalikes — and nothing
is signed before Confirm; a site's page may call only the wallet command, never the engine, the toolbar's commands,
links or events, and its origin is read from its webview, not from the page), a Safe
multisig (made from a card where its keys, how many sign and its networks change before Confirm; paid out of when this
computer's key is enough), wallets (create with the phrase shown once, import a phrase or a key, switch, rename, remove, a test wallet; or, with none, addresses only watched for their balances, as the Mac's «Addresses for balances»: nothing signs or connects for them),
Receive, the address book, the portfolio on Solana, Hyperliquid, every EVM network and NEAR, the chat with a model
(an API key, a browser sign-in to ChatGPT or OpenRouter caught on 127.0.0.1, a local model, or a login another app
has) or plain commands without one, every money command the Mac's
chat runs through the Executor (transfers, swaps, bridges, lending, Hyperliquid spot), Hyperliquid trades by the risk
rules with every signature on the card, managing open positions, the best rates and spreading or rebalancing money
across them, Merkl's rewards (claim, or sweep them into USDC), token approvals with a revoke on each, Jupiter DCA,
coins from networks without a wallet here (NEAR Intents' deposit address), several moves in one message under one Confirm,
price alerts and scheduled tasks with recurring buys (checked every minute while the app runs, with a Windows
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
9. The browser: open jup.ag, connect, and a swap's card comes to the canvas with its simulated balances; on a site's
   page, `__TAURI_INTERNALS__.invoke('engine_send', …)` (or `browser_nav`, `browser_state`) in DevTools is refused; the
   toolbar's Disconnect takes the address away and reloads the page.
10. A Ledger over USB: «connect my Ledger» reads both addresses; a 1 USDC send shows on its screen. Then a Nano X, Stax
    or Flex over Bluetooth the same way (Windows may ask to pair it first).
11. A scheduled buy switched on in the Schedule block runs from the automation wallet with no Windows Hello, and what it
    bought arrives on the main wallet.

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
