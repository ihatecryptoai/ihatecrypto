//! A Ledger on Windows through winsec: over USB (HID, a fresh handle for each exchange — the device reconnects whenever
//! an app opens or quits on it) and over Bluetooth LE (one link for the app's life, since linking takes seconds, with
//! Ledger's framing without the HID channel, in pieces as large as the link allows). As the Swift engine's
//! WinLedgerHID and WinLedgerBLE.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use winsec as w;

use crate::framing::{self, HID_PACKET_SIZE};
use crate::{Exchanged, LedgerError, Result, Transport};

/// One exchange at a time over USB.
static HID: Mutex<()> = Mutex::new(());

fn late() -> LedgerError { LedgerError::new("The Ledger didn't answer in time: check its screen and try again.") }

/// «Nano S Plus», «Flex»… of the first Ledger plugged in, or None when none is.
pub fn connected() -> Option<String> {
    let mut name = [0u8; 64];
    let n = unsafe { w::ledger::winsec_ledger_product(name.as_mut_ptr(), name.len() as i32) };
    (n > 0).then(|| String::from_utf8_lossy(&name[..n as usize]).into_owned())
}

pub struct Usb;

impl Transport for Usb {
    fn exchange(&self, apdu: Vec<u8>, timeout: Duration) -> Exchanged<'_> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || usb_exchange(&apdu, timeout)).await.map_err(|e| LedgerError::new(e.to_string()))?
        })
    }
}

fn usb_exchange(apdu: &[u8], timeout: Duration) -> Result<Vec<u8>> {
    let _one = HID.lock().unwrap_or_else(|e| e.into_inner());
    // Right after an app switch the device is briefly gone: wait for it to come back.
    let mut device = w::ledger::winsec_ledger_open();
    for _ in 0..20 {
        if !device.is_null() { break }
        std::thread::sleep(Duration::from_millis(250));
        device = w::ledger::winsec_ledger_open();
    }
    if device.is_null() {
        return Err(LedgerError::new(if connected().is_none() { "No Ledger found: plug it in over USB and unlock it." }
                                    else { "The Ledger is busy: close Ledger Live or any other wallet using it, then try again." }));
    }
    struct Close(*mut std::ffi::c_void);
    impl Drop for Close { fn drop(&mut self) { unsafe { w::ledger::winsec_ledger_close(self.0) } } }
    let device = Close(device);
    for p in framing::packets(apdu, HID_PACKET_SIZE, true) {
        if unsafe { w::ledger::winsec_ledger_write(device.0, p.as_ptr(), p.len() as i32) } != 0 {
            return Err(LedgerError::new("Couldn't write to the Ledger: plug it in again."));
        }
    }
    let deadline = Instant::now() + timeout;
    let mut packets = Vec::new();
    loop {
        if let Some(reply) = framing::reply(&packets, true)? { return Ok(reply) }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() { return Err(late()) }
        let mut buffer = [0u8; HID_PACKET_SIZE];
        let n = unsafe { w::ledger::winsec_ledger_read(device.0, buffer.as_mut_ptr(), buffer.len() as i32, left.min(Duration::from_secs(1)).as_millis() as i32) };
        if n < 0 { return Err(LedgerError::new("Couldn't read from the Ledger: plug it in again.")) }
        if n > 0 { packets.push(buffer[..n as usize].to_vec()) }
    }
}

struct Link {
    /// winsec's link, as an address so the state is Send; null: none yet.
    handle: usize,
    mtu: usize,
    model: Option<String>,
}

/// The Bluetooth Ledger: one for the app.
pub struct Bluetooth {
    link: Mutex<Link>,
}

pub static BLUETOOTH: Bluetooth = Bluetooth { link: Mutex::new(Link { handle: 0, mtu: 20, model: None }) };

fn gone() -> LedgerError { LedgerError::new("The Ledger went away over Bluetooth: unlock it and keep it close.") }

impl Bluetooth {
    /// Links a Ledger within `timeout`: its model, or the reason it didn't.
    pub async fn connect(&'static self, timeout: Duration) -> std::result::Result<String, String> {
        tokio::task::spawn_blocking(move || {
            let mut link = self.link.lock().unwrap_or_else(|e| e.into_inner());
            link_up(&mut link, timeout).map_err(|e| e.message)?;
            Ok(link.model.clone().unwrap_or_else(|| "Ledger".into()))
        }).await.map_err(|e| e.to_string())?
    }

    pub fn model(&self) -> Option<String> { self.link.lock().unwrap_or_else(|e| e.into_inner()).model.clone() }
}

impl Transport for &'static Bluetooth {
    fn exchange(&self, apdu: Vec<u8>, timeout: Duration) -> Exchanged<'_> {
        let this: &'static Bluetooth = *self;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut link = this.link.lock().unwrap_or_else(|e| e.into_inner());
                // The first link may wait for Windows' pairing prompt.
                link_up(&mut link, Duration::from_secs(45))?;
                ble_exchange(&link, &apdu, timeout).inspect_err(|_| {
                    // A link that went away is made again next time.
                    unsafe { w::ble::winsec_ble_close(link.handle as *mut _) };
                    link.handle = 0;
                })
            }).await.map_err(|e| LedgerError::new(e.to_string()))?
        })
    }
}

fn link_up(link: &mut Link, timeout: Duration) -> Result<()> {
    if link.handle != 0 { return Ok(()) }
    let (mut why, mut why_len) = ([0u8; 256], 0i32);
    let l = unsafe { w::ble::winsec_ble_open(timeout.as_millis() as i32, why.as_mut_ptr(), why.len() as i32, &mut why_len) };
    if l.is_null() { return Err(LedgerError::new(String::from_utf8_lossy(&why[..why_len.max(0) as usize]).into_owned())) }
    let (mut name, mut size) = ([0u8; 32], 20i32);
    let n = unsafe { w::ble::winsec_ble_info(l, name.as_mut_ptr(), name.len() as i32, &mut size) };
    link.model = Some(if n > 0 { String::from_utf8_lossy(&name[..n as usize]).into_owned() } else { "Ledger".into() });
    link.handle = l as usize;
    // Ledger's own client confirms notifications (0x0001), then asks the device's MTU (0x08).
    let _ = request(link, &[0x00, 0x01], Duration::from_secs(2));
    link.mtu = match request(link, &[0x08, 0, 0, 0, 0], Duration::from_secs(3)) {
        Ok(r) if r.len() > 5 && r[0] == 0x08 => (r[5] as usize).min(size.max(0) as usize).max(20),
        _ => (size.max(0) as usize).max(20),
    };
    Ok(())
}

fn write(link: &Link, frame: &[u8]) -> Result<()> {
    if unsafe { w::ble::winsec_ble_write(link.handle as *mut _, frame.as_ptr(), frame.len() as i32) } != 0 { return Err(gone()) }
    Ok(())
}

fn read(link: &Link, timeout: Duration) -> Result<Option<Vec<u8>>> {
    let mut buffer = [0u8; 512];
    let n = unsafe { w::ble::winsec_ble_read(link.handle as *mut _, buffer.as_mut_ptr(), buffer.len() as i32, timeout.as_millis() as i32) };
    if n < 0 { return Err(gone()) }
    Ok((n > 0).then(|| buffer[..n as usize].to_vec()))
}

/// One handshake frame and the first notification back.
fn request(link: &Link, frame: &[u8], timeout: Duration) -> Result<Vec<u8>> {
    unsafe { w::ble::winsec_ble_drain(link.handle as *mut _) };
    write(link, frame)?;
    read(link, timeout)?.ok_or_else(|| LedgerError::new("No answer over Bluetooth."))
}

fn ble_exchange(link: &Link, apdu: &[u8], timeout: Duration) -> Result<Vec<u8>> {
    unsafe { w::ble::winsec_ble_drain(link.handle as *mut _) };
    for frame in framing::packets(apdu, link.mtu, false) { write(link, &frame)? }
    let deadline = Instant::now() + timeout;
    let mut packets = Vec::new();
    loop {
        if let Some(reply) = framing::reply(&packets, false)? { return Ok(reply) }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() { return Err(late()) }
        if let Some(p) = read(link, left.min(Duration::from_secs(1)))? { packets.push(p) }
    }
}
