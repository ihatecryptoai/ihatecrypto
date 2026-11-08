//! A Ledger over USB for the engine: found by Ledger's vendor id (0x2c97) and the HID usage page its APDUs travel on
//! (0xffa0; the device's other interfaces are U2F and WebUSB). A report is the 64-byte packet after report id 0. The
//! framing is Swift's (LedgerFraming); here are only the device's writes and reads.

use std::ffi::c_void;
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::Threading::*;
use windows::core::PCWSTR;

const VENDOR: u16 = 0x2c97;
const USAGE_PAGE: u16 = 0xffa0;

struct Device {
    handle: HANDLE,
    event: HANDLE,
    input: usize,
    output: usize,
}

/// Every HID interface present, as its device path (NUL-terminated UTF-16).
unsafe fn paths() -> Vec<Vec<u16>> {
    let guid = unsafe { HidD_GetHidGuid() };
    let Ok(set) = (unsafe { SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) }) else { return vec![] };
    let mut out = vec![];
    let mut i = 0;
    loop {
        let mut data = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
        if unsafe { SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut data) }.is_err() { break }
        i += 1;
        let mut needed = 0u32;
        let _ = unsafe { SetupDiGetDeviceInterfaceDetailW(set, &data, None, 0, Some(&mut needed), None) };
        if needed < 8 { continue }
        // Four-byte aligned, as the structure needs.
        let mut buf = vec![0u32; (needed as usize).div_ceil(4)];
        let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe { (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
        if unsafe { SetupDiGetDeviceInterfaceDetailW(set, &data, Some(detail), needed, None, None) }.is_err() { continue }
        let p = unsafe { (*detail).DevicePath.as_ptr() };
        let mut n = 0;
        while unsafe { *p.add(n) } != 0 { n += 1 }
        out.push(unsafe { std::slice::from_raw_parts(p, n + 1) }.to_vec());
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
    out
}

/// The path's device when it is a Ledger's APDU interface: its handle, report sizes and product id. `access` 0 only
/// reads its attributes (another program may hold it open).
unsafe fn ledger(path: &[u16], access: u32) -> Option<(HANDLE, HIDP_CAPS, u16)> {
    let h = unsafe {
        CreateFileW(PCWSTR(path.as_ptr()), access, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, None)
    }.ok()?;
    let mut attrs = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
    let mut caps = HIDP_CAPS::default();
    let mut ok = unsafe { HidD_GetAttributes(h, &mut attrs) } && attrs.VendorID == VENDOR;
    if ok {
        let mut pre = PHIDP_PREPARSED_DATA::default();
        ok = unsafe { HidD_GetPreparsedData(h, &mut pre) };
        if ok {
            ok = unsafe { HidP_GetCaps(pre, &mut caps) } == HIDP_STATUS_SUCCESS && caps.UsagePage == USAGE_PAGE;
            let _ = unsafe { HidD_FreePreparsedData(pre) };
        }
    }
    if !ok {
        let _ = unsafe { CloseHandle(h) };
        return None;
    }
    Some((h, caps, attrs.ProductID))
}

/// The first Ledger plugged in, opened for one exchange; null when none is (or another program holds it).
#[unsafe(no_mangle)]
pub extern "C" fn winsec_ledger_open() -> *mut c_void {
    for p in unsafe { paths() } {
        let Some((handle, caps, _)) = (unsafe { ledger(&p, (GENERIC_READ | GENERIC_WRITE).0) }) else { continue };
        if let Some(device) = unsafe { device(handle, &caps) } { return device }
    }
    std::ptr::null_mut()
}

unsafe fn device(handle: HANDLE, caps: &HIDP_CAPS) -> Option<*mut c_void> {
    let Ok(event) = (unsafe { CreateEventW(None, true, false, PCWSTR::null()) }) else {
        let _ = unsafe { CloseHandle(handle) };
        return None;
    };
    let device = Device { handle, event, input: caps.InputReportByteLength as usize, output: caps.OutputReportByteLength as usize };
    Some(Box::into_raw(Box::new(device)) as *mut c_void)
}

/// The first Ledger's APDU interface opened for reading and writing, for the window to hand to the contained engine,
/// which can't open a device itself (winsec_ledger_adopt there).
pub fn open_handle() -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::FromRawHandle;
    for p in unsafe { paths() } {
        if let Some((handle, _, _)) = unsafe { ledger(&p, (GENERIC_READ | GENERIC_WRITE).0) } {
            return Some(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.0) });
        }
    }
    None
}

/// A Ledger the window opened (`open_handle`), as winsec_ledger_open's device; null when the handle isn't a Ledger's
/// APDU interface. The device owns the handle from here.
#[unsafe(no_mangle)]
pub extern "C" fn winsec_ledger_adopt(handle: isize) -> *mut c_void {
    let handle = HANDLE(handle as *mut c_void);
    let mut caps = HIDP_CAPS::default();
    let mut pre = PHIDP_PREPARSED_DATA::default();
    let mut attrs = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
    let ok = unsafe { HidD_GetAttributes(handle, &mut attrs) } && attrs.VendorID == VENDOR && unsafe { HidD_GetPreparsedData(handle, &mut pre) } && {
        let read = unsafe { HidP_GetCaps(pre, &mut caps) } == HIDP_STATUS_SUCCESS && caps.UsagePage == USAGE_PAGE;
        let _ = unsafe { HidD_FreePreparsedData(pre) };
        read
    };
    if !ok {
        let _ = unsafe { CloseHandle(handle) };
        return std::ptr::null_mut();
    }
    unsafe { device(handle, &caps) }.unwrap_or(std::ptr::null_mut())
}

/// «Nano S Plus», «Flex»… of the first Ledger plugged in.
pub fn product() -> Option<String> {
    let mut name = [0u8; 64];
    let n = unsafe { winsec_ledger_product(name.as_mut_ptr(), name.len() as i32) };
    (n > 0).then(|| String::from_utf8_lossy(&name[..n as usize]).into_owned())
}

/// «Nano S Plus», «Flex»… of the first Ledger plugged in, as UTF-8 into `out`: its length, 0 when none is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ledger_product(out: *mut u8, cap: i32) -> i32 {
    for p in unsafe { paths() } {
        let Some((h, _, product)) = (unsafe { ledger(&p, 0) }) else { continue };
        let _ = unsafe { CloseHandle(h) };
        let name = match product >> 8 { 0x10 => "Nano S", 0x40 => "Nano X", 0x50 => "Nano S Plus", 0x60 => "Stax", 0x70 => "Flex", _ => "Ledger" };
        let n = name.len().min(cap.max(0) as usize);
        unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), out, n) };
        return n as i32;
    }
    0
}

/// One packet to the device, behind report id 0. 0 when written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ledger_write(device: *mut c_void, packet: *const u8, len: i32) -> i32 {
    if device.is_null() || len <= 0 { return -1 }
    let d = unsafe { &*(device as *const Device) };
    let len = len as usize;
    let mut report = vec![0u8; d.output.max(len + 1)];
    report[1..1 + len].copy_from_slice(unsafe { std::slice::from_raw_parts(packet, len) });
    let mut overlapped = OVERLAPPED { hEvent: d.event, ..Default::default() };
    let _ = unsafe { ResetEvent(d.event) };
    if let Err(e) = unsafe { WriteFile(d.handle, Some(&report), None, Some(&mut overlapped)) } {
        if e.code() != ERROR_IO_PENDING.to_hresult() { return -1 }
    }
    let mut written = 0u32;
    if unsafe { GetOverlappedResult(d.handle, &overlapped, &mut written, true) }.is_err() { return -1 }
    0
}

/// One packet from the device into `out`, waiting at most `timeout_ms`: its length, 0 when none came, -1 on an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ledger_read(device: *mut c_void, out: *mut u8, cap: i32, timeout_ms: i32) -> i32 {
    if device.is_null() { return -1 }
    let d = unsafe { &*(device as *const Device) };
    let mut report = vec![0u8; d.input.max(65)];
    let mut overlapped = OVERLAPPED { hEvent: d.event, ..Default::default() };
    let _ = unsafe { ResetEvent(d.event) };
    if let Err(e) = unsafe { ReadFile(d.handle, Some(&mut report), None, Some(&mut overlapped)) } {
        if e.code() != ERROR_IO_PENDING.to_hresult() { return -1 }
    }
    let mut read = 0u32;
    if unsafe { WaitForSingleObject(d.event, timeout_ms.max(0) as u32) } != WAIT_OBJECT_0 {
        // The buffer stays until the cancelled read has finished.
        let _ = unsafe { CancelIoEx(d.handle, Some(&overlapped)) };
        let _ = unsafe { GetOverlappedResult(d.handle, &overlapped, &mut read, true) };
        return 0;
    }
    if unsafe { GetOverlappedResult(d.handle, &overlapped, &mut read, false) }.is_err() { return -1 }
    // Without the report id.
    let body = &report[1.min(read as usize)..read as usize];
    let n = body.len().min(cap.max(0) as usize);
    unsafe { std::ptr::copy_nonoverlapping(body.as_ptr(), out, n) };
    n as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_ledger_close(device: *mut c_void) {
    if device.is_null() { return }
    let d = unsafe { Box::from_raw(device as *mut Device) };
    let _ = unsafe { CloseHandle(d.handle) };
    let _ = unsafe { CloseHandle(d.event) };
}
