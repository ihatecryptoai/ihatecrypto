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

A debug build also runs lines of JS appended to `IHATECRYPTO_UI_EVAL` in the page, so a test can press buttons with
the window in the background. On Linux the core, the signer and the engine build in the `swift:6.2` container.

On Windows, from `App/`: `pwsh Windows/build.ps1` builds winsec.dll, the engine and the signer in release, copies the
Swift runtime beside them and makes the installer
(`Windows/app/src-tauri/target/release/bundle/nsis/ihatecrypto_<version>_x64-setup.exe`). It needs Rust, Swift 6.4.1,
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

Has: wallets (create with the phrase shown once, import a phrase or a key, switch, rename, remove, a test wallet),
Receive, the address book, the portfolio on Solana, Hyperliquid, every EVM network and NEAR, the chat with a model
(an API key, a browser sign-in to ChatGPT or OpenRouter caught on 127.0.0.1, a local model, or a login another app
has) or plain commands without one, every money command the Mac's
chat runs through the Executor (transfers, swaps, bridges, lending, Hyperliquid spot), Hyperliquid trades by the risk
rules with every signature on the card, managing open positions, several moves in one message under one Confirm,
price alerts and scheduled tasks with recurring buys (checked every minute while the app runs; their moves come as
cards for Confirm, with a Windows notification), the tray (closing the window keeps it running, as the Mac's menu bar
does), price charts and the day's movers in the chat, P/L (Hyperliquid's trading after fees and
funding, open positions, the portfolio's change without money moved in or out, and each coin's P/L against what it
cost: the chain's history priced on the day, the app's own buys, or when first seen), the second check (on by default, as on the
Mac: the model reads every command and trade plan again, Jev through OpenRouter when allowed, and code checks that a
plan holds no more money or risk than named), history from the journal, lock and the session
length, signed updates.

Not yet: the canvas and its boards, the automation wallet (a task's moves sign only after Confirm), the dApp browser,
multisig, Ledger, favorites, skills, paid requests (MPP), voice, notifications.

## Checking on a Windows machine

A runner has no Windows Hello, so the prompt itself is checked by hand on Windows 11 with a PIN set:

1. Install from the workflow's installer; Windows asks for an administrator (Program Files).
2. Settings › Signing says «Windows Hello is ready; its key is in the TPM» and no hardening notes.
3. Create a wallet: Windows Hello asks twice (making the key, then signing), the phrase shows, the window can't be
   captured meanwhile (Win+Shift+S shows it black), «Copy all 12 words» leaves nothing in Win+V history.
4. Save an address: the signer's window shows the whole address while Windows Hello asks.
5. A move on a test amount: one prompt, the signer's window shows the amounts it parsed.
6. Edit `%LOCALAPPDATA%\ihatecrypto\signer\state.json` by hand: the next signature refuses with the «changed outside
   it» message.
7. Process Explorer as the same user can't read the signer's memory (access denied).
