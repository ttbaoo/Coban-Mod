//! Host profiles never rewrite device EEPROM when the foreground application changes.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HostConfig {
    pub profiles: Vec<HostProfile>,
    /// Local display aliases follow a macro slot or F13–F24 code across profiles.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub display_names: std::collections::BTreeMap<u16, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostProfile {
    pub name: String,
    pub application: String,
    pub bindings: Vec<HostBinding>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostBinding {
    pub trigger: u16,
    pub action: HostAction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum HostAction {
    Text(String),
    Web(String),
    Application(String),
    File(String),
    // Read compatibility for versions <= 0.2.1. New edits use the distinct types above.
    Open(String),
    Macro(String),
}
impl HostAction {
    pub fn upgraded(self) -> Self {
        let Self::Open(target) = self else {
            return self;
        };
        let normalized = normalize_open_target(&target).unwrap_or_else(|_| target.clone());
        if normalized.starts_with("https://") || normalized.starts_with("http://") {
            Self::Web(target)
        } else if normalized.to_ascii_lowercase().ends_with(".app")
            || normalized.to_ascii_lowercase().ends_with(".exe")
            || (!normalized.contains(['/', '\\', ':', '.']))
        {
            Self::Application(target)
        } else {
            Self::File(target)
        }
    }
}
impl HostConfig {
    pub fn display_name(&self, code: u16) -> Option<&str> {
        self.display_names.get(&code).map(String::as_str)
    }
    pub fn set_display_name(&mut self, code: u16, name: &str) -> Result<()> {
        ensure!(
            matches!(code, 104..=115 | 0x7700..=0x777f),
            "Chỉ đặt tên riêng cho F13–F24 và macro"
        );
        let name = name.trim();
        ensure!(
            name.chars().count() <= 80 && !name.chars().any(char::is_control),
            "Tên riêng tối đa 80 ký tự, không xuống dòng"
        );
        if name.is_empty() {
            self.display_names.remove(&code);
        } else {
            self.display_names.insert(code, name.into());
        }
        Ok(())
    }
    pub fn upgrade_legacy_actions(&mut self) {
        for profile in &mut self.profiles {
            for binding in &mut profile.bindings {
                binding.action = binding.action.clone().upgraded();
            }
        }
    }
    pub fn validate(&self) -> Result<()> {
        for (&code, name) in &self.display_names {
            ensure!(
                matches!(code, 104..=115 | 0x7700..=0x777f)
                    && !name.trim().is_empty()
                    && name.chars().count() <= 80
                    && !name.chars().any(char::is_control),
                "Tên riêng của phím không hợp lệ"
            );
        }
        ensure!(self.profiles.len() <= 100, "Quá nhiều profile");
        for p in &self.profiles {
            ensure!(p.bindings.len() <= 12, "Tối đa 12 trigger/profile");
            let mut seen = std::collections::HashSet::new();
            for b in &p.bindings {
                ensure!((104..=115).contains(&b.trigger), "Trigger chỉ nhận F13–F24");
                ensure!(seen.insert(b.trigger), "Trùng trigger");
                match &b.action {
                    HostAction::Web(target) => {
                        normalize_web_target(target)?;
                    }
                    HostAction::Application(target) => {
                        validate_application_target(target)?;
                    }
                    HostAction::File(target) | HostAction::Open(target) => {
                        nonempty_target(target)?;
                    }
                    HostAction::Text(text) => ensure!(text.len() <= 100_000, "Văn bản quá dài"),
                    HostAction::Macro(text) => validate_macro(text)?,
                }
            }
        }
        Ok(())
    }
    pub fn action(&self, app: &str, trigger: u16) -> Option<&HostAction> {
        self.profiles
            .iter()
            .filter(|p| !p.application.is_empty() && p.application.eq_ignore_ascii_case(app))
            .find_map(|p| p.bindings.iter().find(|b| b.trigger == trigger))
            .or_else(|| {
                self.profiles
                    .iter()
                    .filter(|p| p.application.is_empty())
                    .find_map(|p| p.bindings.iter().find(|b| b.trigger == trigger))
            })
            .map(|b| &b.action)
    }
}

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Keep file paths intact; a bare web address must not be passed to the OS as a file.
pub fn normalize_open_target(target: &str) -> Result<String> {
    let target = target.trim();
    ensure!(
        !target.is_empty() && target.len() <= 8192 && !target.chars().any(char::is_control),
        "Đích mở không hợp lệ"
    );
    if let Some(rest) = target.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("Không tìm được thư mục cá nhân")?
            .join(rest)
            .to_string_lossy()
            .into_owned());
    }
    // Explicit paths and existing relative files take precedence over domain detection.
    if Path::new(target).exists()
        || target.starts_with(['/', '\\'])
        || target.starts_with("./")
        || target.starts_with("../")
        || target.as_bytes().get(1) == Some(&b':')
    {
        return Ok(target.into());
    }
    // Application filenames must not be mistaken for domains.
    let lower = target.to_ascii_lowercase();
    if ![".app", ".exe", ".lnk", ".cmd", ".bat"]
        .iter()
        .any(|ext| lower.ends_with(ext))
        && !target.chars().any(char::is_whitespace)
        && let Ok(parsed) = url::Url::parse(&format!("https://{target}"))
        && let Some(host) = parsed.host_str()
        && (host.contains('.') || host == "localhost" || host.contains(':'))
        && parsed.username().is_empty()
        && parsed.password().is_none()
    {
        return Ok(format!("https://{target}"));
    }
    // Preserve explicit URL schemes, including application links and mailto:.
    if let Some((scheme, _)) = target.split_once(':')
        && !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    {
        url::Url::parse(target).context("URL không hợp lệ")?;
    }
    Ok(target.into())
}
fn nonempty_target(target: &str) -> Result<&str> {
    let target = target.trim();
    ensure!(
        !target.is_empty() && target.len() <= 8192 && !target.chars().any(char::is_control),
        "Nhập đích cần mở"
    );
    Ok(target)
}
pub fn normalize_web_target(target: &str) -> Result<String> {
    let target = nonempty_target(target)?;
    ensure!(
        !target.starts_with(['/', '\\', '.']) && !target.chars().any(char::is_whitespace),
        "Nhập địa chỉ website, ví dụ youtube.com"
    );
    let explicit = target
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        || target
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("http://"));
    let address = if explicit {
        target.into()
    } else {
        format!("https://{target}")
    };
    let parsed = url::Url::parse(&address).context("Địa chỉ website không hợp lệ")?;
    let host = parsed
        .host_str()
        .context("Địa chỉ website thiếu tên miền")?;
    ensure!(
        (explicit || host.contains('.') || host == "localhost" || host.contains(':'))
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && matches!(parsed.scheme(), "http" | "https"),
        "Chỉ nhận website HTTP/HTTPS, ví dụ youtube.com"
    );
    Ok(address)
}
pub fn validate_application_target(target: &str) -> Result<()> {
    let target = nonempty_target(target)?;
    ensure!(
        !target.contains("://"),
        "Chọn ứng dụng hoặc nhập tên ứng dụng; website dùng mục Mở website"
    );
    let lower = target.to_ascii_lowercase();
    ensure!(
        target.starts_with(['/', '\\'])
            || target.starts_with("~/")
            || target.starts_with("./")
            || target.starts_with("../")
            || target.as_bytes().get(1) == Some(&b':')
            || [".app", ".exe", ".lnk", ".cmd", ".bat"]
                .iter()
                .any(|ext| lower.ends_with(ext))
            || !target.contains(['.', '/', '\\', ':']),
        "Chọn ứng dụng cần mở; địa chỉ web dùng mục Mở website"
    );
    Ok(())
}
fn expanded_path(target: &str) -> Result<String> {
    let target = nonempty_target(target)?;
    Ok(if let Some(rest) = target.strip_prefix("~/") {
        dirs::home_dir()
            .context("Không tìm được thư mục cá nhân")?
            .join(rest)
            .to_string_lossy()
            .into_owned()
    } else {
        target.into()
    })
}
#[cfg(target_os = "macos")]
fn macos_open(args: &[&str]) -> Result<()> {
    let mut process = std::process::Command::new("/usr/bin/open")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Không chạy được trình mở của macOS")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while process.try_wait()?.is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            anyhow::bail!("Trình mở của macOS hết thời gian");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = process.wait_with_output()?;
    ensure!(
        output.status.success(),
        "Không mở được đích đã chọn: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
pub fn open_website(target: &str) -> Result<()> {
    let address = normalize_web_target(target)?;
    #[cfg(target_os = "macos")]
    {
        macos_open(&["--", &address])
    }
    #[cfg(not(target_os = "macos"))]
    {
        open_system_target(&address)
    }
}
pub fn open_application(target: &str) -> Result<()> {
    validate_application_target(target)?;
    let target = expanded_path(target)?;
    #[cfg(target_os = "macos")]
    {
        macos_open(&["-a", &target])
    }
    #[cfg(not(target_os = "macos"))]
    {
        open_system_target(&target)
    }
}
pub fn open_file(target: &str) -> Result<()> {
    let target = expanded_path(target)?;
    #[cfg(target_os = "macos")]
    {
        macos_open(&["--", &target])
    }
    #[cfg(not(target_os = "macos"))]
    {
        open_system_target(&target)
    }
}
#[cfg(target_os = "macos")]
fn macos_open_arguments(target: &str) -> Vec<String> {
    if !target.contains(['/', ':'])
        && !Path::new(target).exists()
        && (!target.contains('.') || target.to_ascii_lowercase().ends_with(".app"))
    {
        vec!["-a".into(), target.into()]
    } else {
        vec!["--".into(), target.into()]
    }
}
pub fn open_target(target: &str) -> Result<()> {
    let target = normalize_open_target(target)?;
    open_system_target(&target)
}
fn open_system_target(target: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/open")
            .args(macos_open_arguments(target))
            .output()
            .context("Không chạy được trình mở của macOS")?;
        ensure!(
            output.status.success(),
            "Không mở được {target}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let value: Vec<_> = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let n = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                value.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        ensure!(n as isize > 32, "Không mở được ứng dụng/tệp/URL");
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    anyhow::bail!("Mở ứng dụng/tệp/URL chỉ hỗ trợ macOS và Windows: {target}");
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Ok(())
}
pub fn application_identity(path: &Path) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSBundle, NSString};
        let s = NSString::from_str(&path.to_string_lossy());
        let b = NSBundle::bundleWithPath(&s).and_then(|b| b.bundleIdentifier());
        Ok(b.map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(path.to_string_lossy().to_string())
    }
}
pub fn foreground() -> String {
    #[cfg(target_os = "macos")]
    {
        objc2::rc::autoreleasepool(|_| {
            objc2_app_kit::NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .and_then(|a| a.bundleIdentifier())
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
    }
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows_sys::Win32::{
                Foundation::CloseHandle,
                System::Threading::{
                    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
                },
                UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
            };
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return String::new();
            }
            let mut path = [0u16; 32768];
            let mut n = path.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut n);
            CloseHandle(handle);
            if ok != 0 {
                String::from_utf16_lossy(&path[..n as usize])
            } else {
                String::new()
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        String::new()
    }
}
fn key_for_hid(code: u8) -> Result<Key> {
    Ok(match code {
        4..=29 => Key::Unicode((b'a' + code - 4) as char),
        30..=38 => Key::Unicode((b'1' + code - 30) as char),
        39 => Key::Unicode('0'),
        40 => Key::Return,
        41 => Key::Escape,
        42 => Key::Backspace,
        43 => Key::Tab,
        44 => Key::Space,
        45 => Key::Unicode('-'),
        46 => Key::Unicode('='),
        47 => Key::Unicode('['),
        48 => Key::Unicode(']'),
        49 => Key::Unicode('\\'),
        51 => Key::Unicode(';'),
        52 => Key::Unicode('\''),
        53 => Key::Unicode('`'),
        54 => Key::Unicode(','),
        55 => Key::Unicode('.'),
        56 => Key::Unicode('/'),
        58 => Key::F1,
        59 => Key::F2,
        60 => Key::F3,
        61 => Key::F4,
        62 => Key::F5,
        63 => Key::F6,
        64 => Key::F7,
        65 => Key::F8,
        66 => Key::F9,
        67 => Key::F10,
        68 => Key::F11,
        69 => Key::F12,
        104 => Key::F13,
        105 => Key::F14,
        106 => Key::F15,
        107 => Key::F16,
        108 => Key::F17,
        109 => Key::F18,
        110 => Key::F19,
        111 => Key::F20,
        #[cfg(not(target_os = "macos"))]
        112 => Key::F21,
        #[cfg(not(target_os = "macos"))]
        113 => Key::F22,
        #[cfg(not(target_os = "macos"))]
        114 => Key::F23,
        #[cfg(not(target_os = "macos"))]
        115 => Key::F24,
        74 => Key::Home,
        75 => Key::PageUp,
        76 => Key::Delete,
        77 => Key::End,
        78 => Key::PageDown,
        79 => Key::RightArrow,
        80 => Key::LeftArrow,
        81 => Key::DownArrow,
        82 => Key::UpArrow,
        224 | 228 => Key::Control,
        225 | 229 => Key::Shift,
        226 | 230 => Key::Alt,
        227 | 231 => Key::Meta,
        _ => anyhow::bail!("Host macro chưa hỗ trợ HID 0x{code:02X}"),
    })
}
pub fn validate_macro(text: &str) -> Result<()> {
    let b = crate::macros::encode(text)?;
    let mut i = 0;
    while i < b.len() {
        if b[i] == 1 {
            let kind = b[i + 1];
            i += 2;
            if kind == 4 {
                while b[i] != b'|' {
                    i += 1;
                }
            } else {
                key_for_hid(b[i])?;
            }
        }
        i += 1;
    }
    Ok(())
}
pub fn execute(action: &HostAction, stop: &AtomicBool) -> Result<()> {
    if let HostAction::Macro(t) = action {
        validate_macro(t)?;
    }
    match action {
        HostAction::Web(target) => return open_website(target),
        HostAction::Application(target) => return open_application(target),
        HostAction::File(target) => return open_file(target),
        HostAction::Open(_) => return execute(&action.clone().upgraded(), stop),
        _ => {}
    }
    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Default::default()
    };
    let mut e=Enigo::new(&settings).map_err(|err|anyhow::anyhow!("Không phát được phím: {err}. Trên macOS, cấp Accessibility cho Coban-mod trong System Settings."))?;
    match action {
        HostAction::Text(t) => {
            ensure!(t.len() <= 100_000, "Văn bản quá dài");
            for part in t.chars().collect::<Vec<_>>().chunks(128) {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                e.text(&part.iter().collect::<String>())
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            }
        }
        HostAction::Macro(t) => {
            let bytes = crate::macros::encode(t)?;
            let mut i = 0;
            while i < bytes.len() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if bytes[i] == 1 {
                    let kind = bytes[i + 1];
                    i += 2;
                    if kind == 4 {
                        let start = i;
                        while bytes[i] != b'|' {
                            i += 1;
                        }
                        let ms = std::str::from_utf8(&bytes[start..i])?.parse::<u64>()?;
                        for _ in 0..ms.div_ceil(10) {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    } else {
                        let key = key_for_hid(bytes[i])?;
                        e.key(
                            key,
                            match kind {
                                2 => Direction::Press,
                                3 => Direction::Release,
                                _ => Direction::Click,
                            },
                        )
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    }
                } else {
                    e.text(&(bytes[i] as char).to_string())
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                }
                i += 1;
            }
        }
        HostAction::Open(_)
        | HostAction::Web(_)
        | HostAction::Application(_)
        | HostAction::File(_) => {}
    }
    Ok(())
}
mod runtime;
pub use runtime::{ActionEngine, HostControl, HostEvent, HostState, start};
#[cfg(target_os = "windows")]
mod windows_input;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_names_accept_unicode_reset_and_load_old_profiles() {
        let mut config: HostConfig = serde_json::from_str(r#"{"profiles":[]}"#).unwrap();
        config.set_display_name(104, "  Mở bài giảng  ").unwrap();
        config.set_display_name(0x7700, "Chèn lời giải").unwrap();
        config.validate().unwrap();
        let loaded: HostConfig =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(loaded.display_name(104), Some("Mở bài giảng"));
        assert_eq!(loaded.display_name(0x7700), Some("Chèn lời giải"));
        for (code, name) in [(103, "F12"), (116, "F25"), (104, "bad\nname"), (104, "\0")] {
            assert!(config.set_display_name(code, name).is_err());
        }
        assert!(config.set_display_name(104, &"a".repeat(81)).is_err());
        config.set_display_name(104, " ").unwrap();
        assert!(config.display_name(104).is_none());
        assert_eq!(config.display_name(0x7700), Some("Chèn lời giải"));
    }

    #[test]
    fn website_and_application_inputs_are_validated_separately() {
        assert_eq!(
            normalize_web_target(" youtube.com ").unwrap(),
            "https://youtube.com"
        );
        assert_eq!(
            normalize_web_target("example.app").unwrap(),
            "https://example.app"
        );
        for target in [
            "Safari",
            "/Applications/Safari.app",
            "file:///tmp/test",
            "shortcuts://run-shortcut?name=Test",
        ] {
            assert!(normalize_web_target(target).is_err(), "{target}");
        }
        for target in [
            "Safari",
            "Google Chrome.app",
            "/Applications/Safari.app",
            "C:\\Program Files\\Example\\app.exe",
        ] {
            validate_application_target(target).unwrap();
        }
        for target in [
            "youtube.com",
            "youtube.com/watch?v=test",
            "https://youtube.com",
            "",
        ] {
            assert!(validate_application_target(target).is_err(), "{target}");
        }
    }

    #[test]
    fn legacy_open_bindings_migrate_without_changing_triggers_profiles_or_targets() {
        let data = r#"{"profiles":[{"name":"Existing","application":"","bindings":[{"trigger":104,"action":{"type":"Open","value":"youtube.com"}},{"trigger":105,"action":{"type":"Open","value":"/Applications/Safari.app"}},{"trigger":106,"action":{"type":"Open","value":"/tmp/lesson.pdf"}}]}]}"#;
        let mut config: HostConfig = serde_json::from_str(data).unwrap();
        config.upgrade_legacy_actions();
        config.validate().unwrap();
        assert!(
            matches!(config.action("anything",104),Some(HostAction::Web(s)) if s=="youtube.com")
        );
        assert!(
            matches!(config.action("anything",105),Some(HostAction::Application(s)) if s=="/Applications/Safari.app")
        );
        assert!(
            matches!(config.action("anything",106),Some(HostAction::File(s)) if s=="/tmp/lesson.pdf")
        );
        assert_eq!(config.profiles[0].name, "Existing");
        let saved = serde_json::to_string(&config).unwrap();
        assert!(!saved.contains("\"Open\""));
        let loaded: HostConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(serde_json::to_string(&loaded).unwrap(), saved);
    }

    #[test]
    fn bare_web_addresses_saved_by_existing_profiles_open_as_https() {
        for address in [
            "facebook.com",
            "youtube.com",
            "www.youtube.com/watch?v=test",
            "example.com:8443?q=test",
            "localhost:3000",
        ] {
            assert_eq!(
                normalize_open_target(address).unwrap(),
                format!("https://{address}")
            );
        }
        assert_eq!(
            normalize_open_target("  youtube.com  ").unwrap(),
            "https://youtube.com"
        );
    }

    #[test]
    fn explicit_links_and_file_paths_keep_their_meaning() {
        for target in [
            "https://youtube.com/watch?v=test",
            "mailto:someone@example.com",
            "shortcuts://run-shortcut?name=Test",
            "/Applications/Google Chrome.app",
            "./missing.pdf",
            "../missing.pdf",
            "C:\\Program Files\\Example\\app.exe",
            "Cargo.toml",
            "notepad.exe",
            "Safari.app",
            "Safari",
        ] {
            assert_eq!(normalize_open_target(target).unwrap(), target);
        }
        assert_eq!(
            normalize_open_target("~/Documents/test.pdf").unwrap(),
            dirs::home_dir()
                .unwrap()
                .join("Documents/test.pdf")
                .to_string_lossy()
        );
    }

    #[test]
    fn empty_or_invalid_open_targets_are_rejected_before_launch() {
        for target in ["", "  ", "https://", "youtube.com\0"] {
            assert!(normalize_open_target(target).is_err(), "{target:?}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_lookup_for_names_and_default_opener_for_urls_and_files() {
        assert_eq!(macos_open_arguments("Safari"), ["-a", "Safari"]);
        assert_eq!(
            macos_open_arguments("Google Chrome.app"),
            ["-a", "Google Chrome.app"]
        );
        for target in [
            "https://youtube.com",
            "/Applications/Safari.app",
            "Cargo.toml",
        ] {
            assert_eq!(macos_open_arguments(target), ["--", target]);
        }
    }
}
