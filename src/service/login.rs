//! Login registration is an explicit UI preference. It starts the app (tray/menubar),
//! which in turn starts the agent and its key service.
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginStatus {
    pub available: bool,
    pub enabled: bool,
    pub message: String,
}
#[cfg(target_os = "macos")]
fn service() -> Option<objc2::rc::Retained<objc2::runtime::AnyObject>> {
    // Resolve dynamically: macOS 12 remains supported without this optional feature.
    use objc2::{msg_send, runtime::AnyClass};
    let class = AnyClass::get(c"SMAppService")?;
    let identifier = objc2_foundation::NSString::from_str("vn.baotruong.mk.agent");
    unsafe { msg_send![class,loginItemServiceWithIdentifier:&*identifier] }
}
#[cfg(target_os = "macos")]
#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}
#[cfg(target_os = "macos")]
pub fn status() -> Result<LoginStatus> {
    use objc2::msg_send;
    objc2::rc::autoreleasepool(|_| {
        let Some(service) = service() else {
            return Ok(LoginStatus {
                available: false,
                enabled: false,
                message: "Tự chạy khi đăng nhập có trên macOS 13 trở lên".into(),
            });
        };
        let value: isize = unsafe { msg_send![&*service, status] };
        Ok(LoginStatus {
            available: true,
            enabled: matches!(value, 1 | 2),
            message: match value {
                1 => "Đã bật khởi động cùng macOS",
                2 => "Đang chờ chấp thuận trong Login Items",
                3 => "Mục chạy khi đăng nhập chưa được hệ thống nhận diện",
                _ => "Khởi động cùng macOS đang tắt",
            }
            .into(),
        })
    })
}
#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> Result<LoginStatus> {
    use objc2::msg_send;
    objc2::rc::autoreleasepool(|_| {
        let service =
            service().ok_or_else(|| anyhow::anyhow!("Chức năng này cần macOS 13 trở lên"))?;
        let value: isize = unsafe { msg_send![&*service, status] };
        if (enabled && matches!(value, 1 | 2)) || (!enabled && matches!(value, 0 | 3)) {
            return status();
        }
        let mut error: *mut objc2_foundation::NSError = std::ptr::null_mut();
        let ok: bool = unsafe {
            if enabled {
                msg_send![&*service,registerAndReturnError:&mut error]
            } else {
                msg_send![&*service,unregisterAndReturnError:&mut error]
            }
        };
        if !ok {
            let reason = if error.is_null() {
                "Không thay đổi được Login Item".into()
            } else {
                unsafe { (*error).localizedDescription().to_string() }
            };
            anyhow::bail!(reason);
        }
        status()
    })
}
#[cfg(windows)]
fn run_key() -> Result<windows_sys::Win32::System::Registry::HKEY> {
    use windows_sys::Win32::System::Registry::*;
    let path: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();
    let mut key = std::ptr::null_mut();
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    anyhow::ensure!(code == 0, "Không mở được thiết lập đăng nhập của user");
    Ok(key)
}
#[cfg(windows)]
pub fn status() -> Result<LoginStatus> {
    use windows_sys::Win32::System::Registry::*;
    let key = run_key()?;
    let name: Vec<u16> = "MK Agent\0".encode_utf16().collect();
    let mut bytes = 0;
    let found = unsafe {
        let code = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        );
        RegCloseKey(key);
        code == 0
    };
    Ok(LoginStatus {
        available: true,
        enabled: found,
        message: if found {
            "Đã bật chạy khi đăng nhập"
        } else {
            "Tự chạy khi đăng nhập đang tắt"
        }
        .into(),
    })
}
#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<LoginStatus> {
    use windows_sys::Win32::System::Registry::*;
    let key = run_key()?;
    let name: Vec<u16> = "MK Agent\0".encode_utf16().collect();
    let code = unsafe {
        if enabled {
            let value: Vec<u16> =
                format!("\"{}\" --background\0", std::env::current_exe()?.display())
                    .encode_utf16()
                    .collect();
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        }
    };
    unsafe {
        RegCloseKey(key);
    }
    anyhow::ensure!(
        code == 0 || (!enabled && code == 2),
        "Không cập nhật được thiết lập đăng nhập"
    );
    status()
}
#[cfg(not(any(target_os = "macos", windows)))]
pub fn status() -> Result<LoginStatus> {
    Ok(LoginStatus {
        available: false,
        enabled: false,
        message: "Không hỗ trợ tự đăng nhập trên OS này".into(),
    })
}
#[cfg(not(any(target_os = "macos", windows)))]
pub fn set_enabled(_: bool) -> Result<LoginStatus> {
    status()
}
