use anyhow::{Result, ensure};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::*, WindowsAndMessaging::*},
};
thread_local! {static TARGET:RefCell<usize>=const{RefCell::new(0)};static PRESSED:RefCell<[bool;12]>=const{RefCell::new([false;12])};static EVENTS:RefCell<Vec<u16>>=const{RefCell::new(Vec::new())};}
unsafe fn device_name(device: HANDLE) -> String {
    unsafe {
        let mut len = 0;
        GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, std::ptr::null_mut(), &mut len);
        if len == 0 || len > 32768 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, buf.as_mut_ptr().cast(), &mut len);
        if n == u32::MAX {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize]).to_lowercase()
    }
}
unsafe extern "system" fn callback(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_INPUT_DEVICE_CHANGE && w == GIDC_REMOVAL as usize {
            TARGET.with(|target| {
                if *target.borrow() == l as usize {
                    *target.borrow_mut() = 0;
                }
            });
        }
        if msg == WM_INPUT {
            let mut len = 0;
            let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
            GetRawInputData(
                l as HRAWINPUT,
                RID_INPUT,
                std::ptr::null_mut(),
                &mut len,
                header,
            );
            if len as usize >= std::mem::size_of::<RAWINPUTHEADER>() && len < 65536 {
                let mut buf = vec![
                    0u64;
                    (len as usize)
                        .max(std::mem::size_of::<RAWINPUT>())
                        .div_ceil(8)
                ];
                let n = GetRawInputData(
                    l as HRAWINPUT,
                    RID_INPUT,
                    buf.as_mut_ptr().cast(),
                    &mut len,
                    header,
                );
                if n != u32::MAX
                    && n as usize
                        >= std::mem::size_of::<RAWINPUTHEADER>()
                            + std::mem::size_of::<RAWKEYBOARD>()
                {
                    let raw = &*(buf.as_ptr().cast::<RAWINPUT>());
                    if raw.header.dwType == RIM_TYPEKEYBOARD
                        && TARGET.with(|target| *target.borrow() == raw.header.hDevice as usize)
                    {
                        let key = raw.data.keyboard;
                        if (0x7c..=0x87).contains(&key.VKey) {
                            let i = (key.VKey - 0x7c) as usize;
                            let down = key.Flags & 1 == 0;
                            PRESSED.with(|p| {
                                let mut p = p.borrow_mut();
                                if down && !p[i] {
                                    EVENTS.with(|e| {
                                        let mut events = e.borrow_mut();
                                        events.push(104 + i as u16);
                                    });
                                }
                                p[i] = down;
                            });
                        }
                    }
                }
            }
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}
pub fn listen(
    selected: &crate::transport::DeviceInfo,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    dispatch: impl Fn(u16),
    ready: impl Fn(),
) -> Result<()> {
    unsafe {
        let mut count = 0;
        let size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        ensure!(
            GetRawInputDeviceList(std::ptr::null_mut(), &mut count, size) != u32::MAX,
            "Không liệt kê được Raw Input"
        );
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![std::mem::zeroed(); count as usize];
        ensure!(
            GetRawInputDeviceList(list.as_mut_ptr(), &mut count, size) != u32::MAX,
            "Không đọc được Raw Input"
        );
        let keyboard = crate::transport::keyboard_path(selected)?
            .to_string_lossy()
            .to_lowercase();
        let matching: Vec<_> = list
            .iter()
            .filter(|d| {
                d.dwType == RIM_TYPEKEYBOARD
                    && device_name(d.hDevice).trim_end_matches('\0') == keyboard
            })
            .collect();
        ensure!(
            matching.len() == 1,
            "Không tìm được Raw Input handle của bàn phím đã chọn"
        );
        TARGET.with(|target| *target.borrow_mut() = matching[0].hDevice as usize);
        PRESSED.with(|p| *p.borrow_mut() = [false; 12]);
        EVENTS.with(|e| e.borrow_mut().clear());
        let class: Vec<u16> = "MKHostInput\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(callback),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        ensure!(!hwnd.is_null(), "Không tạo được cửa sổ nhận input");
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
            hwndTarget: hwnd,
        };
        if RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            DestroyWindow(hwnd);
            anyhow::bail!("Không đăng ký được Raw Input");
        }
        ready();
        let mut previous_tick = std::time::SystemTime::now();
        let mut interrupted = false;
        while !stop.load(Ordering::Relaxed) {
            let now = std::time::SystemTime::now();
            if now.duration_since(previous_tick).unwrap_or(Duration::MAX) > Duration::from_secs(2) {
                interrupted = true;
                break;
            }
            previous_tick = now;
            let mut msg: MSG = std::mem::zeroed();
            for _ in 0..64 {
                if PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if TARGET.with(|target| *target.borrow() == 0) {
                interrupted = true;
                break;
            }
            let keys = EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
            for key in keys {
                dispatch(key);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        cancel.store(true, Ordering::Release);
        let remove = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: std::ptr::null_mut(),
        };
        RegisterRawInputDevices(&remove, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
        DestroyWindow(hwnd);
        UnregisterClassW(class.as_ptr(), instance);
        ensure!(
            !interrupted,
            "Thiết bị đã rút hoặc máy vừa thức; đang phục hồi bộ nhận"
        );
        Ok(())
    }
}
