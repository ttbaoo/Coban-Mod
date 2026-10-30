//! Menubar lifecycle; the separate Coban-mod Agent continues to own input and HID.
use crate::MainWindow;
use anyhow::Result;
use mk::storage;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::AnyObject, sel,
};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSAppleEventManager, NSObject, NSObjectProtocol};
use slint::{ComponentHandle, winit_030::WinitWindowAccessor};
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

pub struct Instance {
    _lock: File,
    request: PathBuf,
}
impl Instance {
    pub fn acquire(background: bool) -> Result<Option<Self>> {
        Self::acquire_at(&storage::data_dir(), background)
    }
    fn acquire_at(data: &std::path::Path, background: bool) -> Result<Option<Self>> {
        use std::os::unix::fs::OpenOptionsExt;
        fs::create_dir_all(data)?;
        let request = data.join("ui-open.request");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(data.join("ui.lock"))?;
        match lock.try_lock() {
            Ok(()) => {
                let _ = fs::remove_file(&request);
                Ok(Some(Self {
                    _lock: lock,
                    request,
                }))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                if !background {
                    storage::write_private(&request, b"open")?;
                }
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
    pub fn take_open_request(&self) -> bool {
        fs::remove_file(&self.request).is_ok()
    }
}

pub fn select_backend(background: bool) -> Result<()> {
    use slint::winit_030::{
        SlintEvent,
        winit::{
            event_loop::EventLoop,
            platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
        },
    };
    let mut builder = EventLoop::<SlintEvent>::with_user_event();
    builder.with_activation_policy(if background {
        ActivationPolicy::Accessory
    } else {
        ActivationPolicy::Regular
    });
    builder.with_activate_ignoring_other_apps(!background);
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_event_loop_builder(builder)
        .select()?;
    Ok(())
}

pub fn show(ui: &MainWindow) -> Result<()> {
    let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("UI main thread"));
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    ui.show()?;
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    ui.window().with_winit_window(|window| {
        window.set_minimized(false);
        window.focus_window();
    });
    Ok(())
}
pub fn hide(ui: &MainWindow) -> Result<()> {
    ui.hide()?;
    NSApplication::sharedApplication(MainThreadMarker::new().expect("UI main thread"))
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    Ok(())
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = slint::Weak<MainWindow>]
    pub struct ReopenHandler;
    unsafe impl NSObjectProtocol for ReopenHandler {}
    impl ReopenHandler {
        #[unsafe(method(handleReopen:withReplyEvent:))]
        fn reopen(&self, _event: &AnyObject, _reply: &AnyObject) {
            if let Some(ui) = self.ivars().upgrade() {
                let _ = show(&ui);
            }
        }
    }
);
pub fn install_reopen_handler(ui: &MainWindow) -> Retained<ReopenHandler> {
    let mtm = MainThreadMarker::new().expect("UI main thread");
    let handler: Retained<ReopenHandler> = unsafe {
        msg_send![
            super(ReopenHandler::alloc(mtm).set_ivars(ui.as_weak())),
            init
        ]
    };
    // Handle only the standard reopen AppleEvent. Winit owns NSApp's delegate.
    let event_handler = handler.clone();
    // AppKit installs its default AppleEvent handlers while finishing launch.
    // Register once the event loop has started so it cannot overwrite ours.
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let _: () = unsafe {
            msg_send![&*manager,
                setEventHandler: &*event_handler,
                andSelector: sel!(handleReopen:withReplyEvent:),
                forEventClass: u32::from_be_bytes(*b"aevt"),
                andEventID: u32::from_be_bytes(*b"rapp")
            ]
        };
    });
    handler
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_ui_and_background_launch_does_not_open_window() {
        let data = std::env::temp_dir().join(format!("mk-ui-instance-{}", std::process::id()));
        let first = Instance::acquire_at(&data, true).unwrap().unwrap();
        assert!(Instance::acquire_at(&data, true).unwrap().is_none());
        assert!(!first.take_open_request());
        assert!(Instance::acquire_at(&data, false).unwrap().is_none());
        assert!(first.take_open_request());
        assert!(!first.take_open_request());
        drop(first);
        assert!(Instance::acquire_at(&data, true).unwrap().is_some());
        fs::remove_dir_all(data).unwrap();
    }
}
