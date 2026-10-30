//! Device input never executes actions on the input thread.
use super::{HostAction, HostConfig};
use crate::transport::{self, DeviceInfo};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostState {
    Stopped,
    Starting,
    WaitingForDevice,
    NeedsInputPermission,
    Ready,
    Degraded,
}
#[derive(Clone, Debug)]
pub enum HostEvent {
    State(HostState),
    Message(String),
}
type Notify = Arc<dyn Fn(HostEvent) + Send + Sync>;
type Executor = Box<dyn Fn(&HostAction, &AtomicBool) -> Result<()> + Send>;
struct Queued {
    action: HostAction,
    cancel: Arc<AtomicBool>,
    created: Instant,
    reply: Option<SyncSender<std::result::Result<(), String>>>,
}
#[derive(Clone)]
pub struct ActionEngine {
    tx: SyncSender<Option<Queued>>,
    running: Arc<AtomicBool>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
    notify: Notify,
    tokens: Arc<Mutex<Vec<std::sync::Weak<AtomicBool>>>>,
}
impl ActionEngine {
    pub fn new(notify: impl Fn(HostEvent) + Send + Sync + 'static) -> Self {
        Self::with_executor(Arc::new(notify), Box::new(super::execute))
    }
    fn with_executor(notify: Notify, execute: Executor) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Option<Queued>>(16);
        let messages = notify.clone();
        let worker = thread::spawn(move || {
            for job in rx {
                let Some(job) = job else {
                    break;
                };
                let result = if job.cancel.load(Ordering::Acquire)
                    || job.created.elapsed() > Duration::from_secs(5)
                {
                    Err("Hành động đã hủy hoặc quá hạn".into())
                } else {
                    execute(&job.action, &job.cancel).map_err(|e| format!("{e:#}"))
                };
                let result = if job.cancel.load(Ordering::Acquire) {
                    Err("Đã hủy hành động".into())
                } else {
                    result
                };
                if let Some(reply) = job.reply {
                    let _ = reply.send(result);
                } else {
                    messages(HostEvent::Message(match result {
                        Ok(()) => "Đã thực thi hành động từ bàn phím".into(),
                        Err(e) => format!("Hành động: {e}"),
                    }));
                }
            }
        });
        Self {
            tx,
            notify,
            running: Arc::new(AtomicBool::new(true)),
            worker: Arc::new(Mutex::new(Some(worker))),
            tokens: Arc::new(Mutex::new(Vec::new())),
        }
    }
    fn enqueue(&self, job: Queued) -> std::result::Result<(), &'static str> {
        // Serialize acceptance with cancellation so shutdown cannot miss a new token.
        let mut tokens = self.tokens.lock().expect("action tokens");
        if !self.running.load(Ordering::Acquire) {
            return Err("Bộ thực thi đã dừng");
        }
        let weak = Arc::downgrade(&job.cancel);
        self.tx.try_send(Some(job)).map_err(|error| match error {
            TrySendError::Full(_) => "Hàng đợi đầy; đã bỏ hành động mới",
            TrySendError::Disconnected(_) => "Bộ thực thi đã dừng",
        })?;
        tokens.retain(|token| token.strong_count() > 0);
        if !tokens.iter().any(|token| token.ptr_eq(&weak)) {
            tokens.push(weak);
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        if self.running.swap(false, Ordering::AcqRel) {
            self.cancel_all();
            let _ = self.tx.send(None);
            if let Some(worker) = self.worker.lock().expect("action worker").take() {
                let _ = worker.join();
            }
        }
    }
    pub fn cancel_all(&self) {
        for token in self
            .tokens
            .lock()
            .expect("action tokens")
            .iter()
            .filter_map(|token| token.upgrade())
        {
            token.store(true, Ordering::Release);
        }
    }
    pub fn trigger(&self, action: HostAction, cancel: Arc<AtomicBool>) {
        match self.enqueue(Queued {
            action,
            cancel,
            created: Instant::now(),
            reply: None,
        }) {
            Ok(()) => (self.notify)(HostEvent::Message(
                "Đã nhận phím · Đã xếp hàng hành động".into(),
            )),
            Err(message) => (self.notify)(HostEvent::Message(message.into())),
        }
    }
    pub fn test(&self, action: HostAction) -> Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        self.enqueue(Queued {
            action,
            cancel: cancel.clone(),
            created: Instant::now(),
            reply: Some(tx),
        })
        .map_err(|_| anyhow::anyhow!("Hàng đợi hành động đang đầy hoặc đã dừng"))?;
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(result) => result.map_err(anyhow::Error::msg),
            Err(_) => {
                cancel.store(true, Ordering::Release);
                anyhow::bail!("Thử hành động hết thời gian; đã yêu cầu hủy");
            }
        }
    }
}

pub struct HostControl {
    stop: Arc<AtomicBool>,
    config: Arc<Mutex<HostConfig>>,
    thread: Option<JoinHandle<()>>,
}
impl HostControl {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
    pub fn update(&self, config: HostConfig) {
        *self.config.lock().expect("host config") = config;
    }
}
impl Drop for HostControl {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
pub fn input_access() -> bool {
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(kind: u32) -> u32;
    }
    unsafe { IOHIDCheckAccess(1) == 0 }
}
#[cfg(not(target_os = "macos"))]
pub fn input_access() -> bool {
    true
}

pub fn start(
    config: HostConfig,
    selected: DeviceInfo,
    engine: ActionEngine,
    notify: impl Fn(HostEvent) + Send + Sync + 'static,
) -> Result<HostControl> {
    config.validate()?;
    ensure!(
        config.profiles.iter().any(|p| !p.bindings.is_empty()),
        "Tạo ít nhất một hành động trước khi bật dịch vụ"
    );
    ensure!(
        selected.physical_id.is_some() || selected.identity.serial.is_some(),
        "Chưa xác định được thiết bị vật lý; không bật nhận phím theo VID/PID"
    );
    let stop = Arc::new(AtomicBool::new(false));
    let current = Arc::new(Mutex::new(config));
    let flag = stop.clone();
    let cfg = current.clone();
    let notify: Notify = Arc::new(notify);
    let thread = thread::spawn(move || {
        let mut state = HostState::Starting;
        notify(HostEvent::State(state.clone()));
        let set_state = |next: HostState, current: &mut HostState| {
            if *current != next {
                *current = next.clone();
                notify(HostEvent::State(next));
            }
        };
        while !flag.load(Ordering::Acquire) {
            if !input_access() {
                set_state(HostState::NeedsInputPermission, &mut state);
                pause(&flag, Duration::from_secs(1));
                continue;
            }
            let devices = transport::enumerate().unwrap_or_default();
            let matches: Vec<_> = devices
                .iter()
                .filter(|d| transport::same_device(&selected, d))
                .collect();
            if matches.len() != 1 {
                set_state(HostState::WaitingForDevice, &mut state);
                pause(&flag, Duration::from_millis(500));
                continue;
            }
            let device = matches[0].clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let dispatch = |key| {
                // Resolve the profile now, before a previous action can change the foreground app.
                let app = super::foreground();
                let action = cfg.lock().ok().and_then(|c| c.action(&app, key).cloned());
                if let Some(action) = action {
                    engine.trigger(action, cancel.clone());
                } else {
                    notify(HostEvent::Message(format!(
                        "Đã nhận Hành động {}; chưa có nội dung cho ứng dụng hiện tại",
                        key - 103
                    )));
                }
            };
            let ready = || notify(HostEvent::State(HostState::Ready));
            let result = listen(&device, &flag, &cancel, dispatch, ready);
            cancel.store(true, Ordering::Release); // invalidates the entire connection epoch
            if flag.load(Ordering::Acquire) {
                break;
            }
            state = HostState::Degraded;
            notify(HostEvent::State(state.clone()));
            if let Err(error) = result {
                notify(HostEvent::Message(format!("Bộ nhận phím: {error:#}")));
            }
            pause(&flag, Duration::from_millis(500));
        }
        notify(HostEvent::State(HostState::Stopped));
    });
    Ok(HostControl {
        stop,
        config: current,
        thread: Some(thread),
    })
}
fn pause(stop: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while !stop.load(Ordering::Acquire) && Instant::now() < until {
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(target_os = "macos")]
fn listen(
    device: &DeviceInfo,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    dispatch: impl Fn(u16),
    ready: impl Fn(),
) -> Result<()> {
    let path = transport::keyboard_path(device)?;
    let api = hidapi::HidApi::new()?;
    let input = api.open_path(&path)?;
    ready();
    let mut edges = Edges::default();
    let mut buffer = [0u8; 64];
    let mut checked = Instant::now();
    let mut previous_tick = std::time::SystemTime::now();
    while !stop.load(Ordering::Acquire) {
        let tick = std::time::SystemTime::now();
        if tick.duration_since(previous_tick).unwrap_or(Duration::MAX) > Duration::from_secs(2) {
            cancel.store(true, Ordering::Release);
            anyhow::bail!("Đã thức máy/đổi thời gian; kết nối lại bộ nhận phím");
        }
        previous_tick = tick;
        let n = input.read_timeout(&mut buffer, 100)?;
        ensure!(
            std::time::SystemTime::now()
                .duration_since(previous_tick)
                .unwrap_or(Duration::MAX)
                <= Duration::from_secs(2),
            "Đã thức máy; bỏ report cũ và kết nối lại"
        );
        if n != 0 {
            let report = if n == 9 && buffer[0] == 0 {
                &buffer[1..9]
            } else {
                &buffer[..n]
            };
            for key in edges.report(report) {
                dispatch(key);
            }
        }
        if checked.elapsed() > Duration::from_secs(1) {
            ensure!(input_access(), "Quyền nhận phím không sẵn sàng");
            ensure!(
                transport::enumerate()?
                    .iter()
                    .any(|d| transport::same_device(device, d)),
                "Thiết bị đã ngắt kết nối"
            );
            checked = Instant::now();
        }
    }
    Ok(())
}
#[cfg(target_os = "windows")]
fn listen(
    device: &DeviceInfo,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    dispatch: impl Fn(u16),
    ready: impl Fn(),
) -> Result<()> {
    super::windows_input::listen(device, stop, cancel, dispatch, ready)
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn listen(
    _: &DeviceInfo,
    _: &AtomicBool,
    _: &AtomicBool,
    _: impl Fn(u16),
    _: impl Fn(),
) -> Result<()> {
    anyhow::bail!("Nhận phím chỉ hỗ trợ macOS và Windows")
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct Edges {
    previous: [bool; 12],
}
#[cfg(any(target_os = "macos", test))]
impl Edges {
    fn report(&mut self, report: &[u8]) -> Vec<u16> {
        if report.len() != 8 || report[2..].iter().any(|key| (1..=3).contains(key)) {
            return Vec::new();
        }
        let mut pressed = Vec::new();
        for (i, before) in self.previous.iter_mut().enumerate() {
            let down = report[2..].contains(&(104 + i as u8));
            if down && !*before {
                pressed.push(104 + i as u16);
            }
            *before = down;
        }
        pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_edges_ignore_holds_and_rollover_but_accept_a_new_press() {
        let mut edges = Edges::default();
        assert_eq!(edges.report(&[0, 0, 104, 0, 0, 0, 0, 0]), vec![104]);
        assert!(edges.report(&[0, 0, 104, 0, 0, 0, 0, 0]).is_empty());
        assert!(edges.report(&[0, 0, 1, 1, 1, 1, 1, 1]).is_empty());
        edges.report(&[0; 8]);
        assert_eq!(edges.report(&[0, 0, 104, 115, 0, 0, 0, 0]), vec![104, 115]);
    }
    #[test]
    fn input_queue_is_bounded_and_connection_cancel_skips_queued_work() {
        let (began_tx, began_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = count.clone();
        let engine = ActionEngine::with_executor(
            Arc::new(|_| {}),
            Box::new(move |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                began_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(())
            }),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        engine.trigger(HostAction::Web("example.com".into()), cancel.clone());
        began_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..100 {
            engine.trigger(HostAction::Web("example.com".into()), cancel.clone());
        }
        cancel.store(true, Ordering::Release);
        release_tx.send(()).unwrap();
        engine.shutdown();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
