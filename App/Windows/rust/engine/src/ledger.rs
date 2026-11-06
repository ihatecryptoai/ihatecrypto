//! The Ledger as a wallet (Ledger.swift): its addresses kept in ledger.json, its connect card, whether it signs now, the
//! signer a move uses while it's on, connecting it over USB or Bluetooth (the ledger crate's winsec transports, as the
//! Swift engine's WinLedgerHID and WinLedgerBLE), showing an address on its screen and forgetting it.

use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use execution::trade_signer::{Signing, TradeError, TradeWallet};
use fancy_regex::Regex;
use hyperliquid::MsgPack;
use ledger::signer::{Agent, LedgerSigner};
use ledger::{Device, LedgerError, Transport};
use serde::{Deserialize, Serialize};
use signer_api::{SignerRequest, SignerWallet};
use tokio::sync::mpsc;
use wallet::evm::EvmSignature;

use crate::canvas::Board;
use crate::engine::Engine;
use crate::link_signer;

/// The Ledger's addresses as a wallet in the list: nothing of it is on this computer but what it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerAccount {
    #[serde(default = "ledger_label")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solana: Option<String>,
    #[serde(default = "evm_path")]
    pub evm_path: String,
    #[serde(default = "solana_path")]
    pub solana_path: String,
    /// It signs now instead of the signer's wallet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Found over Bluetooth rather than USB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluetooth: Option<bool>,
}

fn ledger_label() -> String { "Ledger".into() }
fn evm_path() -> String { ledger::EVM_PATH.into() }
fn solana_path() -> String { ledger::SOLANA_PATH.into() }

impl Default for LedgerAccount {
    fn default() -> Self {
        LedgerAccount { label: ledger_label(), model: None, evm: None, solana: None, evm_path: evm_path(), solana_path: solana_path(), active: None, bluetooth: None }
    }
}

impl LedgerAccount {
    pub const ID: &str = "ledger";

    pub fn wallet(&self) -> SignerWallet {
        SignerWallet { id: Self::ID.into(), label: self.label.clone(), kind: "ledger".into(), solana: self.solana.clone(), evm: self.evm.clone(), near: None,
                       test: None, automation: None }
    }
}

/// The connect card's progress on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LedgerConnect {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Settings on the device to change (blind signing off, an app not installed).
    pub notes: Vec<String>,
    pub busy: bool,
}

impl Default for LedgerConnect {
    fn default() -> Self {
        LedgerConnect { model: None, evm: None, solana: None, status: "Looking for a Ledger on USB…".into(), problem: None, notes: vec![], busy: true }
    }
}

// MARK: The transports

/// The device as an account reaches it: over Bluetooth when it was found there, else USB (the ledger crate's winsec
/// transports, as the Swift engine's WinLedgerBLE.shared and WinLedgerHID).
impl LedgerAccount {
    pub fn transport(&self) -> Arc<dyn Transport> {
        #[cfg(windows)]
        { if self.bluetooth == Some(true) { Arc::new(&ledger::windows::BLUETOOTH) } else { Arc::new(ledger::windows::Usb) } }
        #[cfg(not(windows))]
        { Arc::new(NoDevice) }
    }
}

/// «Nano S Plus», «Flex»… of the first Ledger plugged in over USB, or None when none is.
fn usb_connected() -> Option<String> {
    #[cfg(windows)]
    { ledger::windows::connected() }
    #[cfg(not(windows))]
    { None }
}

/// Links a Ledger over Bluetooth within `timeout`: its model, or the reason it didn't.
async fn bluetooth_connect(timeout: Duration) -> Result<String, String> {
    #[cfg(windows)]
    { ledger::windows::BLUETOOTH.connect(timeout).await }
    #[cfg(not(windows))]
    { let _ = timeout; Err(NO_DEVICE.into()) }
}

/// Off Windows (the tests on a Mac) there is no USB or Bluetooth Ledger: every exchange says so.
#[cfg(not(windows))]
const NO_DEVICE: &str = "No Ledger found: plug it in over USB and unlock it.";

#[cfg(not(windows))]
struct NoDevice;

#[cfg(not(windows))]
impl Transport for NoDevice {
    fn exchange(&self, _: Vec<u8>, _: Duration) -> ledger::Exchanged<'_> { Box::pin(async { Err(LedgerError::new(NO_DEVICE)) }) }
}

// MARK: The Ledger as the wallet

/// Its words, in English and Russian (Swift's `/ledger|л[еэ]дж[еэ]р/` and the three whole-message forms).
static MENTIONS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ledger|л[еэ]дж[еэ]р").expect("a regex"));
static USE: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"^(?:\s*(?:use|switch\s+to|sign\s+with|используй|переключи\w*\s+на|подписывай\s+(?:с|на))\s+(?:my\s+|мой\s+|моём\s+|мо[йеё]м?\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*)$",
).expect("a regex"));
static FORGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"^(?:\s*(?:forget|remove|disconnect|забудь|удали|отключи)\s+(?:my\s+|мой\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*)$",
).expect("a regex"));
static CONNECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"^(?:\s*(?:(?:connect|add|pair|set\s*up|link|подключи\w*|добавь|привяжи|подсоедини)\s+(?:my\s+|a\s+|мой\s+)?)?(?:ledger|л[еэ]дж[еэ]р\w*)(?:\s+(?:nano\s*\w*|flex|stax))?\s*[.!?]?\s*)$",
).expect("a regex"));

fn matches(re: &Regex, text: &str) -> bool { re.is_match(text).unwrap_or(false) }

impl Engine {
    /// The Ledger signs now (State's, for the methods here).
    pub fn ledger_active(&self) -> bool { self.st().ledger_active() }

    /// The connect card, changed when it's on the canvas (`ledgerConnect?.… = …`).
    pub(crate) fn connect_card(&self, change: impl FnOnce(&mut LedgerConnect)) {
        if let Some(c) = self.st_mut().ledger_connect.as_mut() { change(c) }
    }

    /// The signer a move uses while the Ledger is on: the same guard the signer runs checks each request, then the
    /// device shows it and waits for its buttons. What the device waits for shows on the running move.
    pub fn ledger_signer(&self, wallet: TradeWallet) -> LedgerSigner {
        let account = self.st().ledger.clone().unwrap_or_default();
        // Orders, cancels and leverage through the trading key the Ledger approved (trading_key.rs): no device buttons.
        let agent: Option<Agent> = self.agent_active().then(|| {
            let (link, owner) = (self.signer.clone(), wallet.evm.clone());
            Arc::new(move |action: MsgPack, nonce: u64| -> Signing<'static, EvmSignature> {
                let (link, owner) = (link.clone(), owner.clone());
                Box::pin(async move {
                    let r = link.send(&SignerRequest::SignLedgerAgentL1 { owner, action, nonce }).await.map_err(|e| TradeError::Failed(e.0))?;
                    match (r.ok, r.signature.clone()) {
                        (true, Some(signature)) => Ok(signature),
                        _ => Err(TradeError::Guard(link_signer::reason(&r))),
                    }
                })
            }) as Agent
        });
        // The device's prompts come from wherever the signer runs; the engine takes them on its own thread.
        let (said, mut heard) = mpsc::unbounded_channel::<Option<String>>();
        let me = Rc::downgrade(&self.rc());
        tokio::task::spawn_local(async move {
            while let Some(text) = heard.recv().await {
                let Some(me) = me.upgrade() else { return };
                me.st_mut().ledger_prompt = text.clone();
                if let Some(text) = text {
                    let board = me.st().running_board.clone();
                    me.update_board(board.as_deref(), |b| b.lines.push(format!("Ledger: {text}")));
                }
                me.changed();
            }
        });
        let mut signer = LedgerSigner::new(Device::new(account.transport()), wallet);
        signer.evm_path = account.evm_path;
        signer.solana_path = account.solana_path;
        signer.model = account.model;
        signer.agent = agent;
        signer.prompt = Arc::new(move |text| { let _ = said.send(text); });
        signer
    }

    /// Opens the Ledger's Ethereum and Solana apps in turn and reads one address from each. Nothing is signed; the
    /// addresses become the «Ledger» wallet in the list.
    pub async fn connect_ledger(&self) {
        let looking = self.say("Looking for a Ledger on USB…", "Ищу Ledger на USB…");
        self.st_mut().ledger_connect = Some(LedgerConnect { status: looking, ..LedgerConnect::default() });
        self.pin(Board::new("ledger", "ledger"));
        self.reply(&self.say("Connecting your Ledger — follow its block on the canvas.", "Подключаю Ledger — следи за блоком на холсте."), None);
        // USB when it's plugged in; otherwise Bluetooth (Nano X, Stax, Flex, Apex), looked for in short rounds.
        let mut found = usb_connected();
        let mut bluetooth = false;
        let mut why: Option<String> = None;
        for _ in 0..12 {
            if found.is_some() { break }
            let plug = self.say("Plug your Ledger in over USB and unlock it — or, on a Nano X, Stax or Flex, unlock it with Bluetooth on and keep it close.",
                                "Подключи Ledger по USB и разблокируй — или, если это Nano X, Stax или Flex, разблокируй с включённым Bluetooth и держи рядом.");
            self.connect_card(|c| c.status = plug);
            self.changed();
            match bluetooth_connect(Duration::from_secs(4)).await {
                Ok(model) => { found = Some(model); bluetooth = true; break }
                Err(e) => why = Some(e),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            if self.st().ledger_connect.is_none() { return }
            found = usb_connected();
        }
        // Its block closed meanwhile: the search ends there and nothing connects (the Mac's loop checks ledgerConnect).
        if self.st().ledger_connect.is_none() { return }
        let Some(model) = found else {
            let problem = self.say("No Ledger answered over USB or Bluetooth in a minute. Plug it in or turn its Bluetooth on, unlock it and press Try again.",
                                   "За минуту Ledger не ответил ни по USB, ни по Bluetooth. Подключи его или включи Bluetooth, разблокируй и нажми «Ещё раз».")
                + &why.map(|w| format!(" ({w})")).unwrap_or_default();
            self.connect_card(|c| { c.problem = Some(problem); c.busy = false });
            self.changed();
            return;
        };
        let shown = format!("{model}{}", if bluetooth { " · Bluetooth" } else { " · USB" });
        self.connect_card(|c| c.model = Some(shown));
        let mut account = self.st().ledger.clone().unwrap_or_default();
        account.model = Some(model);
        account.bluetooth = Some(bluetooth);
        let device = Device::new(account.transport());
        match self.read_ledger_addresses(&device, &mut account).await {
            Ok(()) => {
                self.set_ledger(|l| *l = Some(account));
                let status = if self.ledger_active() { self.say("Connected. It signs every move now.", "Подключён. Теперь подписывает каждое движение.") }
                    else { self.say("Connected. Switch to it to sign with it.", "Подключён. Переключись на него, чтобы подписывать им.") };
                self.connect_card(|c| c.status = status);
            }
            Err(e) => {
                if account.evm.is_some() { self.set_ledger(|l| *l = Some(account)) }
                self.connect_card(|c| c.problem = Some(e.message));
            }
        }
        self.connect_card(|c| c.busy = false);
        self.changed();
    }

    /// connectLedger's `do` block: the Ethereum address, then the Solana one (an app not installed is a note, not a
    /// failure), with what the apps' settings lack.
    async fn read_ledger_addresses(&self, device: &Device, account: &mut LedgerAccount) -> ledger::Result<()> {
        let status = self.say("Open the Ethereum app on your Ledger — or allow it when the Ledger asks.", "Открой приложение Ethereum на Ledger — или разреши, когда он спросит.");
        self.connect_card(|c| c.status = status);
        self.changed();
        device.open("Ethereum").await?;
        account.evm = Some(device.evm_address(&account.evm_path, false).await?);
        let evm = account.evm.clone();
        self.connect_card(|c| c.evm = evm);
        if let Ok((on, version)) = device.evm_blind_signing().await {
            let note = format!("Ethereum {version}: turn on «Raw messages» in the app's settings — then the Ledger shows Hyperliquid approvals and withdrawals field by field.{}",
                               if on { "" } else { " «Blind signing» is needed too for swaps, Aave, Morpho and bridges." });
            self.connect_card(|c| c.notes.push(note));
        }
        let status = self.say("Now the Solana app: allow it when the Ledger asks.", "Теперь приложение Solana: разреши, когда Ledger спросит.");
        self.connect_card(|c| c.status = status);
        self.changed();
        let solana = async {
            device.open("Solana").await?;
            account.solana = Some(device.solana_address(&account.solana_path, false).await?);
            let solana = account.solana.clone();
            self.connect_card(|c| c.solana = solana);
            if let Ok((false, version)) = device.solana_blind_signing().await {
                let note = format!("Solana {version}: turn on «Blind signing» in the app's settings for USDC transfers, Jupiter swaps and lending. SOL sends work without it.");
                self.connect_card(|c| c.notes.push(note));
            }
            Ok::<(), LedgerError>(())
        };
        match solana.await {
            Err(e) if e.status == Some(0x6807) => {
                let note = "No Solana app on this Ledger: add it in Ledger Live › My Ledger, then connect again. Until then it signs EVM moves only.".to_string();
                self.connect_card(|c| c.notes.push(note));
                Ok(())
            }
            other => other,
        }
    }

    /// One of its addresses on its own screen, to compare with the one here: what the device says is the truth.
    pub async fn show_on_ledger(&self, solana: bool) {
        let Some(account) = self.st().ledger.clone() else { return };
        let device = Device::new(account.transport());
        if self.st().ledger_connect.is_none() {
            self.st_mut().ledger_connect = Some(LedgerConnect {
                model: account.model.clone(), evm: account.evm.clone(), solana: account.solana.clone(), busy: false, ..LedgerConnect::default()
            });
        }
        self.pin(Board::new("ledger", "ledger"));
        let compare = self.say("Compare the address on your Ledger with the one here, then approve it there.", "Сравни адрес на Ledger с адресом здесь и подтверди на устройстве.");
        self.connect_card(|c| { c.problem = None; c.status = compare });
        self.changed();
        let shown = async {
            if solana {
                device.open("Solana").await?;
                device.solana_address(&account.solana_path, true).await?;
            } else {
                device.open("Ethereum").await?;
                device.evm_address(&account.evm_path, true).await?;
            }
            Ok::<(), LedgerError>(())
        };
        match shown.await {
            Ok(()) => {
                let same = self.say("The Ledger showed the same address.", "Ledger показал тот же адрес.");
                self.connect_card(|c| c.status = same);
            }
            Err(e) => self.connect_card(|c| c.problem = Some(e.message)),
        }
        self.changed();
    }

    /// The Ledger signs from now on (true), or the signer's wallet again.
    pub fn use_ledger(&self, on: bool) {
        let Some(mut account) = self.st().ledger.clone() else { return };
        if account.active == Some(on) { return }
        account.active = Some(on);
        self.set_ledger(|l| *l = Some(account));
        self.wallet_changed();
        let me = self.rc();
        tokio::task::spawn_local(async move { me.refresh().await });
    }

    /// The Ledger in reach now (the Mac's ledgerReachable): plugged in over USB, or answering over Bluetooth.
    pub async fn ledger_reachable(&self) -> bool {
        let Some(account) = self.st().ledger.clone() else { return false };
        // Swift compared the Bluetooth answer's tuple with nil, which is never nil: an answer with a model counts here.
        if account.bluetooth == Some(true) { bluetooth_connect(Duration::from_secs(3)).await.is_ok() } else { usb_connected().is_some() }
    }

    /// Takes the Ledger off the list: only its addresses were here.
    pub async fn forget_ledger(&self) {
        let Some(account) = self.st().ledger.clone() else {
            self.reply(&self.say("No Ledger is connected.", "Ledger не подключён."), None);
            return;
        };
        // Its trading key stops signing first (the Mac's forgetLedger); connecting the same Ledger again brings it back
        // with Windows Hello.
        if let Some(evm) = account.evm { let _ = self.signer_call(SignerRequest::SetLedgerAgentActive { owner: evm, active: false }).await; }
        self.use_ledger(false);
        self.set_ledger(|l| *l = None);
        self.st_mut().ledger_connect = None;
        self.reply(&self.say("Ledger removed from the wallet list. The money on it stays on it; connect it again any time.",
                             "Ledger убран из списка кошельков. Деньги на нём остаются на нём; подключить снова можно в любой момент."), None);
    }

    /// «connect ledger», «подключи леджер», «use my Ledger», «forget my Ledger»: true when the message was about it.
    pub async fn ledger_command(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        if !matches(&MENTIONS, &lower) { return false }
        if matches(&USE, &lower) && self.st().ledger.is_some() {
            self.use_ledger(true);
            self.reply(&self.say("Your Ledger signs now: every move asks for its buttons.", "Теперь подписывает Ledger: каждое движение просит его кнопки."), None);
            return true;
        }
        if matches(&FORGET, &lower) {
            self.forget_ledger().await;
            return true;
        }
        if matches(&CONNECT, &lower) {
            let me = self.rc();
            tokio::task::spawn_local(async move { me.connect_ledger().await });
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "ledger/tests.rs"]
mod tests;
