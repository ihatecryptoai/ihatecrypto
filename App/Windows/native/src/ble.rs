//! A Ledger Nano X, Stax, Flex or Apex over Bluetooth LE for the engine (the Mac's LedgerBLE): found by its model's
//! GATT service, linked, its notify channel subscribed. Replies land in an inbox the engine reads; writes go to the
//! write-command channel (or the write channel). The handshake and Ledger's framing (without the HID channel) are
//! Swift's. The first link may need Windows' pairing prompt, which Windows shows by itself.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use windows::Devices::Bluetooth::Advertisement::{BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher, BluetoothLEScanningMode};
use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Bluetooth::GenericAttributeProfile::*;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter};
use windows::core::GUID;

/// Each model's service (LedgerHQ device-sdk-ts): notify is …-0001-…, write …-0002-…, write command …-0003-….
const MODELS: [(u128, &str); 4] = [
    (0x13d63400_2c97_0004_0000_4c6564676572, "Nano X"),
    (0x13d63400_2c97_6004_0000_4c6564676572, "Stax"),
    (0x13d63400_2c97_3004_0000_4c6564676572, "Flex"),
    (0x13d63400_2c97_8004_0000_4c6564676572, "Apex"),
];

fn channel(service: u128, n: u128) -> GUID { GUID::from_u128(service | (n << 48)) }

#[derive(Default)]
struct Inbox {
    packets: VecDeque<Vec<u8>>,
    gone: bool,
}

struct Link {
    device: BluetoothLEDevice,
    writer: GattCharacteristic,
    without_response: bool,
    inbox: Arc<(Mutex<Inbox>, Condvar)>,
    mtu: usize,
    model: &'static str,
}

/// The first Ledger advertising within `timeout`: its address and model.
fn find(timeout: Duration) -> Option<(u64, u128, &'static str)> {
    let found: Arc<(Mutex<Option<(u64, u128, &'static str)>>, Condvar)> = Arc::default();
    let watcher = BluetoothLEAdvertisementWatcher::new().ok()?;
    let _ = watcher.SetScanningMode(BluetoothLEScanningMode::Active);
    let sink = found.clone();
    watcher.Received(&TypedEventHandler::<BluetoothLEAdvertisementWatcher, BluetoothLEAdvertisementReceivedEventArgs>::new(move |_, args| {
        let Ok(args) = args.ok() else { return Ok(()) };
        let uuids = args.Advertisement()?.ServiceUuids()?;
        for i in 0..uuids.Size()? {
            let uuid = uuids.GetAt(i)?.to_u128();
            if let Some((service, model)) = MODELS.iter().find(|(s, _)| *s == uuid) {
                let (lock, ready) = &*sink;
                *lock.lock().unwrap() = Some((args.BluetoothAddress()?, *service, model));
                ready.notify_all();
            }
        }
        Ok(())
    })).ok()?;
    watcher.Start().ok()?;
    let (lock, ready) = &*found;
    let deadline = Instant::now() + timeout;
    let mut got = lock.lock().unwrap();
    while got.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() { break }
        got = ready.wait_timeout(got, left).unwrap().0;
    }
    let _ = watcher.Stop();
    *got
}

fn link(timeout: Duration) -> Result<Link, String> {
    let (address, service, model) = find(timeout).ok_or("No Ledger answered over Bluetooth: unlock it, turn its Bluetooth on and keep it close.")?;
    let device = BluetoothLEDevice::FromBluetoothAddressAsync(address).and_then(|op| op.join())
        .map_err(|e| format!("The Ledger didn't connect over Bluetooth: {}", e.message()))?;
    let services = device.GetGattServicesForUuidAsync(GUID::from_u128(service)).and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    if services.Status().map_err(|e| e.message().to_string())? != GattCommunicationStatus::Success {
        return Err("The Ledger didn't open its Bluetooth service: pair it in Settings › Bluetooth & devices, then try again.".into());
    }
    let service_ = services.Services().and_then(|s| s.GetAt(0)).map_err(|_| "That Bluetooth device isn't a Ledger.".to_string())?;
    let characteristic = |n: u128| -> Option<GattCharacteristic> {
        let r = service_.GetCharacteristicsForUuidAsync(channel(service, n)).and_then(|op| op.join()).ok()?;
        if r.Status().ok()? != GattCommunicationStatus::Success { return None }
        r.Characteristics().and_then(|c| c.GetAt(0)).ok()
    };
    let notify = characteristic(1).ok_or("The Ledger's Bluetooth service is missing its channels: update its firmware.")?;
    let (writer, without_response) = match characteristic(3) {
        Some(c) => (c, true),
        None => (characteristic(2).ok_or("The Ledger's Bluetooth service is missing its channels: update its firmware.")?, false),
    };
    let inbox: Arc<(Mutex<Inbox>, Condvar)> = Arc::default();
    let sink = inbox.clone();
    notify.ValueChanged(&TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(move |_, args| {
        let Ok(args) = args.ok() else { return Ok(()) };
        let buffer = args.CharacteristicValue()?;
        let reader = DataReader::FromBuffer(&buffer)?;
        let mut bytes = vec![0u8; buffer.Length()? as usize];
        reader.ReadBytes(&mut bytes)?;
        let (lock, ready) = &*sink;
        lock.lock().unwrap().packets.push_back(bytes);
        ready.notify_all();
        Ok(())
    })).map_err(|e| e.message().to_string())?;
    let status = notify.WriteClientCharacteristicConfigurationDescriptorAsync(GattClientCharacteristicConfigurationDescriptorValue::Notify)
        .and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    if status != GattCommunicationStatus::Success {
        return Err("The Ledger refused the Bluetooth link: pair it again in Settings › Bluetooth & devices.".into());
    }
    // What one write may carry: the session's PDU less the 3-byte ATT header.
    let mtu = device.BluetoothDeviceId().and_then(|id| GattSession::FromDeviceIdAsync(&id)).and_then(|op| op.join())
        .and_then(|s| s.MaxPduSize()).map(|p| (p as usize).saturating_sub(3)).unwrap_or(20).max(20);
    Ok(Link { device, writer, without_response, inbox, mtu, model })
}

/// A Ledger linked over Bluetooth within `timeout_ms`: an opaque handle, NULL with the reason in `why` (UTF-8, its
/// length in `why_len`) when none answered.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_open(timeout_ms: i32, why: *mut u8, why_cap: i32, why_len: *mut i32) -> *mut c_void {
    match link(Duration::from_millis(timeout_ms.max(0) as u64)) {
        Ok(l) => Box::into_raw(Box::new(l)) as *mut c_void,
        Err(text) => {
            let n = text.len().min(why_cap.max(0) as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr(), why, n);
                *why_len = n as i32;
            }
            std::ptr::null_mut()
        }
    }
}

/// «Nano X», «Flex»… of the linked Ledger, and the most one write carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_info(link: *mut c_void, model: *mut u8, cap: i32, mtu: *mut i32) -> i32 {
    if link.is_null() { return -1 }
    let l = unsafe { &*(link as *const Link) };
    let n = l.model.len().min(cap.max(0) as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(l.model.as_ptr(), model, n);
        *mtu = l.mtu as i32;
    }
    n as i32
}

/// One frame to the Ledger. 0 when it took it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_write(link: *mut c_void, frame: *const u8, len: i32) -> i32 {
    if link.is_null() || len <= 0 { return -1 }
    let l = unsafe { &*(link as *const Link) };
    let bytes = unsafe { std::slice::from_raw_parts(frame, len as usize) };
    let sent = (|| -> windows::core::Result<GattCommunicationStatus> {
        let w = DataWriter::new()?;
        w.WriteBytes(bytes)?;
        let buffer = w.DetachBuffer()?;
        let option = if l.without_response { GattWriteOption::WriteWithoutResponse } else { GattWriteOption::WriteWithResponse };
        l.writer.WriteValueWithOptionAsync(&buffer, option)?.join()
    })();
    match sent {
        Ok(GattCommunicationStatus::Success) => 0,
        _ => {
            let (lock, _) = &*l.inbox;
            lock.lock().unwrap().gone = true;
            -1
        }
    }
}

/// One notification from the Ledger into `out`, waiting at most `timeout_ms`: its length, 0 when none came, -1 once
/// the link is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_read(link: *mut c_void, out: *mut u8, cap: i32, timeout_ms: i32) -> i32 {
    if link.is_null() { return -1 }
    let l = unsafe { &*(link as *const Link) };
    let (lock, ready) = &*l.inbox;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(0) as u64);
    let mut inbox = lock.lock().unwrap();
    loop {
        if let Some(p) = inbox.packets.pop_front() {
            let n = p.len().min(cap.max(0) as usize);
            unsafe { std::ptr::copy_nonoverlapping(p.as_ptr(), out, n) };
            return n as i32;
        }
        if inbox.gone { return -1 }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() { return 0 }
        inbox = ready.wait_timeout(inbox, left).unwrap().0;
    }
}

/// Empties the inbox before a new exchange.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_drain(link: *mut c_void) {
    if link.is_null() { return }
    let l = unsafe { &*(link as *const Link) };
    l.inbox.0.lock().unwrap().packets.clear();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ble_close(link: *mut c_void) {
    if link.is_null() { return }
    let l = unsafe { Box::from_raw(link as *mut Link) };
    let _ = l.device.Close();
}
