use crate::model::{Identity, PID, VID};
use anyhow::{Context, Result, bail, ensure};
use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceInfo {
    pub path: std::ffi::CString,
    pub identity: Identity,
    pub display: String,
    pub physical_id: Option<String>,
}
pub fn enumerate() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    Ok(api
        .device_list()
        .filter(|d| {
            d.vendor_id() == VID
                && d.product_id() == PID
                && d.usage_page() == 0xff60
                && d.usage() == 0x61
        })
        .map(|d| DeviceInfo {
            path: d.path().to_owned(),
            identity: Identity {
                vendor_id: d.vendor_id(),
                product_id: d.product_id(),
                product: d.product_string().unwrap_or("Coban Pad 10A").into(),
                serial: d
                    .serial_number()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            },
            display: format!(
                "{} · {}",
                d.product_string().unwrap_or("Coban Pad 10A"),
                d.path().to_string_lossy()
            ),
            physical_id: physical_id(d.path()),
        })
        .collect())
}

/// Match a physical unit, never just a model. A serial is preferred; without one
/// the selection is tied to the physical port/container for this agent session.
pub fn same_device(a: &DeviceInfo, b: &DeviceInfo) -> bool {
    if a.identity.vendor_id != b.identity.vendor_id
        || a.identity.product_id != b.identity.product_id
    {
        return false;
    }
    match (&a.identity.serial, &b.identity.serial) {
        (Some(a), Some(b)) => a == b,
        (None, None) => a.physical_id.is_some() && a.physical_id == b.physical_id,
        _ => false,
    }
}

pub fn keyboard_path(selected: &DeviceInfo) -> Result<std::ffi::CString> {
    let api = HidApi::new()?;
    let matches: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == VID
                && d.product_id() == PID
                && d.usage_page() == 1
                && d.usage() == 6
                && match &selected.identity.serial {
                    Some(serial) => d.serial_number() == Some(serial.as_str()),
                    None => {
                        selected.physical_id.is_some()
                            && physical_id(d.path()) == selected.physical_id
                    }
                }
        })
        .collect();
    ensure!(
        matches.len() == 1,
        "Không xác định được keyboard collection của bàn phím đã chọn ({} kết quả)",
        matches.len()
    );
    Ok(matches[0].path().to_owned())
}

#[cfg(target_os = "macos")]
pub fn physical_id(path: &std::ffi::CStr) -> Option<String> {
    use std::{ffi::c_void, ptr};
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegistryEntryIDMatching(id: u64) -> *const c_void;
        fn IOServiceGetMatchingService(port: u32, matching: *const c_void) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: *const c_void,
            alloc: *const c_void,
            options: u32,
        ) -> *const c_void;
        fn IOObjectRelease(object: u32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            text: *const i8,
            encoding: u32,
        ) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, kind: i32, value: *mut c_void) -> bool;
        fn CFGetTypeID(object: *const c_void) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFRelease(object: *const c_void);
    }
    let id = path
        .to_str()
        .ok()?
        .strip_prefix("DevSrvsID:")?
        .parse::<u64>()
        .ok()?;
    unsafe {
        let entry = IOServiceGetMatchingService(0, IORegistryEntryIDMatching(id));
        if entry == 0 {
            return None;
        }
        let key = CFStringCreateWithCString(ptr::null(), c"LocationID".as_ptr(), 0x08000100);
        let value = IORegistryEntryCreateCFProperty(entry, key, ptr::null(), 0);
        CFRelease(key);
        IOObjectRelease(entry);
        if value.is_null() {
            return None;
        }
        let mut location: i64 = 0;
        let valid = CFGetTypeID(value) == CFNumberGetTypeID()
            && CFNumberGetValue(value, 4, (&mut location as *mut i64).cast());
        CFRelease(value);
        valid.then(|| format!("usb-port:{location:08x}"))
    }
}

#[cfg(target_os = "windows")]
pub fn physical_id(path: &std::ffi::CStr) -> Option<String> {
    use windows_sys::Win32::Devices::{DeviceAndDriverInstallation::*, Properties::*};
    let name: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut kind = 0;
        let mut instance = [0u16; 1024];
        let mut len = (instance.len() * 2) as u32;
        if CM_Get_Device_Interface_PropertyW(
            name.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            instance.as_mut_ptr().cast(),
            &mut len,
            0,
        ) != CR_SUCCESS
        {
            return None;
        }
        let mut node = 0;
        if CM_Locate_DevNodeW(&mut node, instance.as_ptr(), 0) != CR_SUCCESS {
            return None;
        }
        let mut container = [0u8; 16];
        len = 16;
        if CM_Get_DevNode_PropertyW(
            node,
            &DEVPKEY_Device_ContainerId,
            &mut kind,
            container.as_mut_ptr(),
            &mut len,
            0,
        ) != CR_SUCCESS
            || kind != DEVPROP_TYPE_GUID
            || len != 16
            || container == [0; 16]
        {
            return None;
        }
        Some(format!("container:{}", hex::encode(container)))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn physical_id(_path: &std::ffi::CStr) -> Option<String> {
    None
}
pub trait Transport {
    fn simulated(&self) -> bool {
        false
    }
    fn exchange(&mut self, request: &[u8], echo: usize) -> Result<[u8; 32]>;
}
pub struct HidTransport {
    device: HidDevice,
}
impl HidTransport {
    pub fn open(info: &DeviceInfo) -> Result<Self> {
        let api = HidApi::new()?;
        let device = api.open_path(&info.path).context(
            "Không mở được HID; ngắt kết nối Coban Connect/Stats trước khi dùng Coban-mod",
        )?;
        Ok(Self { device })
    }
}
impl Transport for HidTransport {
    fn exchange(&mut self, request: &[u8], echo: usize) -> Result<[u8; 32]> {
        ensure!(
            !request.is_empty() && request.len() <= 32 && echo <= request.len(),
            "HID request không hợp lệ"
        );
        let mut stale = [0u8; 64];
        for _ in 0..16 {
            if self.device.read_timeout(&mut stale, 0)? == 0 {
                break;
            }
        }
        let mut out = [0u8; 33];
        out[1..1 + request.len()].copy_from_slice(request);
        ensure!(self.device.write(&out)? == 33, "Không gửi đủ HID report");
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "HID hết thời gian chờ (0x{:02X}). Ngắt kết nối Chrome/Coban Stats rồi đọc lại.",
                    request[0]
                );
            }
            let mut data = [0u8; 64];
            let n = self
                .device
                .read_timeout(&mut data, remaining.as_millis().min(1500) as i32)?;
            let payload = if n == 33 && data[0] == 0 {
                &data[1..33]
            } else {
                &data[..n]
            };
            if payload.first() == Some(&0xff) {
                bail!("Firmware không hỗ trợ lệnh 0x{:02X}", request[0]);
            }
            if payload.len() != 32 || payload[..echo] != request[..echo] {
                continue;
            }
            let mut response = [0; 32];
            response.copy_from_slice(payload);
            return Ok(response);
        }
    }
}
impl<T: Transport + ?Sized> Transport for Box<T> {
    fn simulated(&self) -> bool {
        (**self).simulated()
    }
    fn exchange(&mut self, q: &[u8], echo: usize) -> Result<[u8; 32]> {
        (**self).exchange(q, echo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unit(port: Option<&str>, serial: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            path: std::ffi::CString::new("test-path").unwrap(),
            identity: Identity {
                vendor_id: VID,
                product_id: PID,
                product: "Coban".into(),
                serial: serial.map(str::to_owned),
            },
            display: String::new(),
            physical_id: port.map(str::to_owned),
        }
    }
    #[test]
    fn never_groups_two_units_only_by_model() {
        assert!(!same_device(&unit(None, None), &unit(None, None)));
        assert!(!same_device(
            &unit(Some("port1"), None),
            &unit(Some("port2"), None)
        ));
        assert!(same_device(
            &unit(Some("port1"), None),
            &unit(Some("port1"), None)
        ));
        assert!(!same_device(
            &unit(Some("port1"), Some("one")),
            &unit(Some("port1"), Some("two"))
        ));
        assert!(same_device(
            &unit(Some("port1"), Some("one")),
            &unit(Some("port2"), Some("one"))
        ));
    }
}
