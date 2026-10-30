//! The agent is the only owner of HID transactions and persisted host profiles.
use crate::{
    editor::Change,
    host::{self, ActionEngine, HostConfig, HostControl, HostEvent, HostState},
    model::Snapshot,
    protocol::{ApplyResult, Protocol},
    storage,
    transport::{self, DeviceInfo, Transport},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
mod ipc;
pub mod login;
const WIRE_VERSION: u32 = 4;
/// The agent lives with the Coban-mod app: the UI polls every second, so a long
/// silence means the app was closed (or crashed) and the agent ends as well.
const UI_IDLE_EXIT: Duration = Duration::from_secs(30);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Job {
    Hello,
    Status,
    Profiles,
    LoginStatus,
    SetLogin(bool),
    SetupInput,
    Discover(bool),
    Connect(DeviceInfo),
    Read,
    Write(Box<Change>),
    Backup(PathBuf),
    Clock,
    Image {
        path: PathBuf,
        before: Box<Snapshot>,
    },
    LoadScreen {
        layer: u8,
    },
    ConfirmLayer {
        layer: u8,
        before: Box<Snapshot>,
    },
    RenderScreen {
        layer: u8,
        expected_revision: u64,
        layout: crate::screen_layout::Layout,
    },
    ImportScreenAsset {
        path: PathBuf,
        background: bool,
    },
    ApplyScreen {
        layer: u8,
        revision: u64,
        before: Box<Snapshot>,
    },
    /// Redraw clock tiles for `at` and resend the GIF; never changes the stored layout.
    RefreshClock {
        layer: u8,
        revision: u64,
        at: chrono::NaiveDateTime,
        screen: Vec<u8>,
    },
    UpdateProfiles {
        expected_revision: u64,
        config: HostConfig,
    },
    SetActions(bool),
    TestAction {
        profile: usize,
        trigger: u16,
    },
    StopAgent,
}
impl Job {
    pub fn fatal(&self) -> bool {
        matches!(
            self,
            Self::Connect(_)
                | Self::Read
                | Self::Write(_)
                | Self::Clock
                | Self::Image { .. }
                | Self::ApplyScreen { .. }
                | Self::ConfirmLayer { .. }
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStatus {
    pub instance: String,
    pub version: String,
    pub enabled: bool,
    pub state: HostState,
    pub message: String,
    pub selected: Option<DeviceInfo>,
    pub profile_revision: u64,
    #[serde(default)]
    pub layer_sync: crate::layer_sync::Status,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Devices(Vec<DeviceInfo>, bool),
    Snapshot(Snapshot),
    Written(ApplyResult),
    Backup(PathBuf),
    Done(String),
    Error(String, bool),
    LoginStatus(login::LoginStatus),
    LoginChanged(login::LoginStatus),
    HostStatus(AgentStatus),
    HostChanged(AgentStatus),
    Profiles {
        config: HostConfig,
        revision: u64,
    },
    ProfilesSaved {
        config: HostConfig,
        revision: u64,
    },
    TestResult(String),
    ScreenDraft {
        layer: u8,
        revision: u64,
        layout: crate::screen_layout::Layout,
        png: Vec<u8>,
        #[serde(default)]
        preview_png: Vec<u8>,
    },
    ScreenPreview {
        layer: u8,
        revision: u64,
        layout: crate::screen_layout::Layout,
        png: Vec<u8>,
        #[serde(default)]
        preview_png: Vec<u8>,
    },
    ScreenAsset(crate::screen_layout::Asset),
    ScreenStored {
        layer: u8,
    },
    LayerConfirmed(ApplyResult),
    ScreenApplied {
        result: ApplyResult,
        output: PathBuf,
    },
    /// Always delivered (errors included) so a clock tick never ends another operation.
    ClockRefreshed {
        layer: u8,
        revision: u64,
        png: Vec<u8>,
        preview_png: Vec<u8>,
        millis: u64,
        error: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
struct Request {
    version: u32,
    app_version: String,
    id: u64,
    instance: Option<String>,
    #[serde(default)]
    target: Option<std::ffi::CString>,
    job: Job,
}
#[derive(Serialize, Deserialize)]
struct Reply {
    version: u32,
    id: u64,
    instance: String,
    code: Option<ErrorCode>,
    event: Event,
}
#[derive(Serialize, Deserialize)]
enum ErrorCode {
    Incompatible,
    Conflict,
    Busy,
    Uncertain,
    Failed,
}

#[derive(Clone)]
pub struct Client {
    instance: String,
    data_dir: PathBuf,
    target: Arc<Mutex<Option<std::ffi::CString>>>,
}
impl Client {
    pub fn connect() -> Result<Self> {
        Self::connect_at(&storage::data_dir())
    }
    pub fn connect_at(data_dir: &Path) -> Result<Self> {
        let reply = ipc::exchange_at(
            &Request {
                version: WIRE_VERSION,
                app_version: env!("CARGO_PKG_VERSION").into(),
                id: 0,
                instance: None,
                target: None,
                job: Job::Hello,
            },
            data_dir,
        )?;
        ensure!(
            reply.version == WIRE_VERSION && reply.id == 0,
            "Giao thức agent không tương thích"
        );
        if let Event::Error(text, _) = reply.event {
            anyhow::bail!(text);
        }
        Ok(Self {
            instance: reply.instance,
            data_dir: data_dir.into(),
            target: Arc::new(Mutex::new(None)),
        })
    }
    pub fn ensure_running() -> Result<Self> {
        if let Ok(client) = Self::connect() {
            return Ok(client);
        }
        // If a server exists but is incompatible, do not start a second owner.
        if ipc::connect_at(&storage::data_dir()).is_ok() {
            anyhow::bail!(
                "Agent đang chạy không tương thích; dừng dịch vụ cũ trước khi mở bản mới"
            );
        }
        let current = std::env::current_exe()?;
        #[cfg(target_os = "macos")]
        let helper = current
            .parent()
            .context("Không tìm được thư mục Coban-mod")?
            .parent()
            .context("Không tìm được bundle Coban-mod")?
            .join("Library/LoginItems/MK Agent.app/Contents/MacOS/mk-agent");
        #[cfg(not(target_os = "macos"))]
        let helper = current.with_file_name(if cfg!(windows) {
            "mk-agent.exe"
        } else {
            "mk-agent"
        });
        let mut command = if helper.exists() {
            let mut cmd = std::process::Command::new(helper);
            // Interactive UI startup already owns the menubar process.
            cmd.arg("--session-agent");
            cmd
        } else {
            #[cfg(target_os = "macos")]
            ensure!(
                !current
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().ends_with(".app")),
                "Gói Coban-mod thiếu MK Agent.app; đóng gói lại ứng dụng"
            );
            let mut cmd = std::process::Command::new(current);
            cmd.arg("--agent");
            cmd
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Không khởi động được Coban-mod Agent")?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(client) = Self::connect() {
                return Ok(client);
            }
            ensure!(
                Instant::now() < deadline,
                "Coban-mod Agent chưa sẵn sàng; kiểm tra lại gói ứng dụng"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn call(&self, job: Job) -> Result<Event> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let selected = if let Job::Connect(device) = &job {
            Some(device.path.clone())
        } else {
            None
        };
        let reply = ipc::exchange_at(
            &Request {
                version: WIRE_VERSION,
                app_version: env!("CARGO_PKG_VERSION").into(),
                id,
                instance: Some(self.instance.clone()),
                target: self.target.lock().expect("client selection").clone(),
                job,
            },
            &self.data_dir,
        )?;
        ensure!(
            reply.version == WIRE_VERSION && reply.id == id && reply.instance == self.instance,
            "Agent đã khởi động lại hoặc khác phiên bản. Đọc lại cấu hình trước khi chỉnh tiếp"
        );
        if matches!(reply.event, Event::Snapshot(_))
            && let Some(path) = selected
        {
            *self.target.lock().expect("client selection") = Some(path);
        }
        Ok(reply.event)
    }
}

#[derive(Serialize, Deserialize)]
struct ProfileEnvelope {
    schema: u32,
    revision: u64,
    config: HostConfig,
}
struct ProfileStore {
    path: PathBuf,
    config: HostConfig,
    revision: u64,
}
impl ProfileStore {
    fn load(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                path,
                config: HostConfig::default(),
                revision: 0,
            });
        }
        ensure!(
            std::fs::metadata(&path)?.len() <= 1_000_000,
            "Kho hành động quá lớn"
        );
        let bytes = std::fs::read(&path)?;
        let (mut config, revision) =
            if let Ok(envelope) = serde_json::from_slice::<ProfileEnvelope>(&bytes) {
                ensure!(envelope.schema == 1, "Schema hành động chưa được hỗ trợ");
                (envelope.config, envelope.revision)
            } else {
                (
                    serde_json::from_slice::<HostConfig>(&bytes)
                        .context("Không đọc được cấu hình hành động cũ; giữ nguyên file")?,
                    0,
                )
            };
        config.upgrade_legacy_actions();
        config.validate()?;
        Ok(Self {
            path,
            config,
            revision,
        })
    }
    fn update(&mut self, expected: u64, config: HostConfig) -> Result<()> {
        ensure!(
            expected == self.revision,
            "Profile đã thay đổi ở cửa sổ khác. Đọc lại trước khi sửa"
        );
        config.validate()?;
        let next = self
            .revision
            .checked_add(1)
            .context("Revision vượt giới hạn")?;
        let bytes = serde_json::to_vec_pretty(&ProfileEnvelope {
            schema: 1,
            revision: next,
            config: config.clone(),
        })?;
        ensure!(bytes.len() <= 1_000_000, "Kho hành động tối đa 1 MB");
        // Preserve the legacy representation before the first schema migration.
        let backup = self.path.with_file_name("host-profiles.pre-agent.json");
        if self.path.exists() && !backup.exists() {
            storage::write_private(&backup, &std::fs::read(&self.path)?)?;
        }
        storage::write_private(&self.path, &bytes)?;
        self.config = config;
        self.revision = next;
        Ok(())
    }
}
struct Shared {
    layer_session: Arc<Mutex<crate::layer_sync::Session>>,
    status: AgentStatus,
    config: HostConfig,
}
fn update_host(shared: &Arc<Mutex<Shared>>, event: HostEvent) {
    if let Ok(mut shared) = shared.lock() {
        match event {
            HostEvent::State(state) => shared.status.state = state,
            HostEvent::Message(message) => shared.status.message = message,
        }
    }
}
struct Broker {
    protocol: Option<Protocol<Box<dyn Transport>>>,
    selected: Option<DeviceInfo>,
    profiles: ProfileStore,
    host: Option<HostControl>,
    shared: Arc<Mutex<Shared>>,
    engine: ActionEngine,
    last_snapshot: Option<Snapshot>,
    data_dir: PathBuf,
}
impl Broker {
    fn layer_preference_path(&self, device: &DeviceInfo) -> Result<PathBuf> {
        use sha2::{Digest, Sha256};
        let port = if device.identity.serial.is_some() {
            None
        } else {
            device.physical_id.clone()
        };
        let identity = serde_json::to_vec(&(&device.identity, port))?;
        Ok(self.data_dir.join(format!(
            "layer-image-sync-{}.json",
            &hex::encode(Sha256::digest(identity))[..16]
        )))
    }
    fn layer_session(&self) -> Arc<Mutex<crate::layer_sync::Session>> {
        self.shared
            .lock()
            .expect("agent status")
            .layer_session
            .clone()
    }
    /// Runs only on the existing USB owner thread, including its idle intervals.
    fn update_layer_screen(&mut self) -> Result<()> {
        if self.protocol.is_none() {
            return Ok(());
        }
        if self.layer_session().lock().expect("layer session").paused {
            return Ok(());
        }
        let protocol = self.protocol.as_mut().context("Chưa kết nối")?;
        protocol.read_active_layer()?;
        let session = self.layer_session();
        let target = {
            let state = session.lock().expect("layer session");
            if state.paused || !state.status.enabled {
                return Ok(());
            }
            let Some(layer) = state.status.active else {
                return Ok(());
            };
            layer
        };
        let now = chrono::Local::now();
        let minute = now.timestamp() / 60;
        {
            let state = session.lock().expect("layer session");
            if state.status.displayed == Some(target) && state.displayed_minute == Some(minute) {
                return Ok(());
            }
        }
        let stored = crate::screen_layout::Store::load_layer_at(&self.data_dir, target)?;
        {
            let mut state = session.lock().expect("layer session");
            if state.status.displayed == Some(target)
                && (!stored.layout.has_clock() || state.displayed_minute == Some(minute))
            {
                state.displayed_minute = Some(minute);
                return Ok(());
            }
        }
        let screen = self
            .last_snapshot
            .as_ref()
            .context("Chưa đọc cấu hình")?
            .settings
            .iter()
            .find(|s| s.key == "screen")
            .context("Thiếu thiết lập màn hình")?
            .value
            .clone();
        let rendered = crate::screen_layout::render_at(&stored.layout, now.naive_local())?;
        self.protocol
            .as_mut()
            .context("Chưa kết nối")?
            .read_active_layer()?;
        if session.lock().expect("layer session").status.active != Some(target) {
            return Ok(());
        }
        self.protocol
            .as_mut()
            .context("Chưa kết nối")?
            .refresh_screen_gif(&screen, &rendered.gif)?;
        let mut state = session.lock().expect("layer session");
        state.status.displayed = Some(target);
        state.displayed_revision = Some(stored.revision);
        state.displayed_minute = Some(minute);
        if state.status.active == Some(target) {
            state.status.message = format!("Đã gửi ảnh lớp {}", target + 1);
        }
        Ok(())
    }
    fn publish_profiles(&self) {
        let mut shared = self.shared.lock().expect("agent status");
        shared.config = self.profiles.config.clone();
        shared.status.profile_revision = self.profiles.revision;
    }
    fn stop_actions(&mut self) {
        if let Some(mut host) = self.host.take() {
            host.stop();
        }
        let mut shared = self.shared.lock().expect("agent status");
        shared.status.enabled = false;
        shared.status.state = HostState::Stopped;
    }
    fn enable_actions(&mut self) -> Result<()> {
        let selected = self
            .selected
            .clone()
            .context("Chọn và đọc bàn phím trước khi bật dịch vụ")?;
        let shared = self.shared.clone();
        let control = host::start(
            self.profiles.config.clone(),
            selected,
            self.engine.clone(),
            move |event| update_host(&shared, event),
        )?;
        self.host = Some(control);
        let mut shared = self.shared.lock().expect("agent status");
        shared.status.enabled = true;
        // Drop the "service off" note; host events report what happens next.
        shared.status.message.clear();
        Ok(())
    }
    fn process(&mut self, job: Job) -> Result<Event> {
        match job {
            Job::ConfirmLayer { layer: _, before } => {
                let session = self.layer_session();
                let enabled = !session.lock().expect("layer session").status.enabled;
                let protocol = self.protocol.as_mut().context("Chưa kết nối")?;
                let state = protocol.read_active_layer()?;
                if enabled {
                    let state = state.context("Firmware chưa hỗ trợ CBLR; không thể tự đổi ảnh")?;
                    ensure!(
                        state.image_safe,
                        "Cần firmware có bản vá GIF v1.1 trước khi bật tự đổi ảnh"
                    );
                    ensure!(
                        state.count == before.layers,
                        "Số lớp firmware không khớp cấu hình"
                    );
                }
                let mut after = (*before).clone();
                if enabled {
                    let screen = after
                        .settings
                        .iter_mut()
                        .find(|s| s.key == "screen")
                        .context("Thiếu thiết lập màn hình")?;
                    screen.value[0] = 2;
                    screen.value[1] = 0;
                }
                let result = protocol.apply(&before, &after)?;
                {
                    let mut state = session.lock().expect("layer session");
                    state.update_snapshot(result.snapshot.clone());
                    state.set_enabled(enabled);
                }
                protocol.read_active_layer()?;
                if let Some(device) = &self.selected {
                    storage::write_private(
                        &self.layer_preference_path(device)?,
                        &serde_json::to_vec(&enabled)?,
                    )?;
                }
                self.last_snapshot = Some(result.snapshot.clone());
                Ok(Event::LayerConfirmed(result))
            }
            Job::Discover(interactive) => Ok(Event::Devices(transport::enumerate()?, interactive)),
            Job::Connect(requested) => {
                let devices = transport::enumerate()?;
                let device = devices
                    .into_iter()
                    .find(|d| d.path == requested.path)
                    .context("Thiết bị đã rút; tìm và chọn lại")?;
                let changed = self
                    .selected
                    .as_ref()
                    .is_none_or(|d| !transport::same_device(d, &device));
                let actions_enabled = self.shared.lock().expect("agent status").status.enabled;
                if changed {
                    self.stop_actions();
                }
                self.protocol = None;
                let session = self
                    .shared
                    .lock()
                    .expect("agent status")
                    .layer_session
                    .clone();
                let observed = crate::layer_sync::ObservedTransport::new(
                    Box::new(transport::HidTransport::open(&device)?),
                    session,
                );
                let mut protocol = Protocol::new(Box::new(observed) as Box<dyn Transport>);
                let snapshot = protocol.read_snapshot(device.identity.clone())?;
                self.shared
                    .lock()
                    .expect("agent status")
                    .layer_session
                    .lock()
                    .expect("layer session")
                    .reset(snapshot.clone());
                let preference = self.layer_preference_path(&device)?;
                let enabled = if preference.exists() {
                    serde_json::from_slice::<bool>(&std::fs::read(preference)?)?
                } else {
                    false
                };
                let gif_mode = snapshot
                    .settings
                    .iter()
                    .find(|s| s.key == "screen")
                    .is_some_and(|s| s.value[0] == 2);
                self.layer_session()
                    .lock()
                    .expect("layer session")
                    .set_enabled(enabled && gif_mode);
                self.last_snapshot = Some(snapshot.clone());
                self.selected = Some(device.clone());
                self.protocol = Some(protocol);
                self.shared.lock().expect("agent status").status.selected = Some(device);
                if changed && actions_enabled {
                    self.enable_actions()?;
                }
                Ok(Event::Snapshot(snapshot))
            }
            Job::Read => {
                let identity = self
                    .selected
                    .as_ref()
                    .context("Chưa chọn thiết bị")?
                    .identity
                    .clone();
                Ok(Event::Snapshot(
                    self.protocol
                        .as_mut()
                        .context("Kết nối và đọc lại bàn phím")?
                        .read_snapshot(identity)?,
                ))
            }
            Job::Write(change) => Ok(Event::Written(
                self.protocol
                    .as_mut()
                    .context("Kết nối và đọc lại bàn phím")?
                    .apply(&change.before, &change.after)?,
            )),
            Job::Backup(path) => {
                let identity = self
                    .selected
                    .as_ref()
                    .context("Chưa chọn thiết bị")?
                    .identity
                    .clone();
                let snapshot = self
                    .protocol
                    .as_mut()
                    .context("Kết nối và đọc lại bàn phím")?
                    .read_snapshot(identity)?;
                storage::save(&path, &snapshot)?;
                Ok(Event::Backup(path))
            }
            Job::Clock => {
                self.protocol
                    .as_mut()
                    .context("Chưa kết nối")?
                    .sync_clock()?;
                Ok(Event::Done("Đã đồng bộ giờ máy tính với bàn phím".into()))
            }
            Job::Image { path, before } => {
                let bytes = crate::media::prepare(&path)?;
                let output = screen_output(&bytes)?;
                let result = self
                    .protocol
                    .as_mut()
                    .context("Chưa kết nối")?
                    .apply_screen_gif(&before, &bytes)?;
                let session = self.layer_session();
                let mut state = session.lock().expect("layer session");
                state.paused = true;
                state.status.message =
                    "Đang dùng ảnh/GIF từ tệp; bấm Đồng bộ lớp này để dùng lại ảnh theo lớp".into();
                Ok(Event::ScreenApplied { result, output })
            }
            Job::LoadScreen { layer } => {
                let stored = crate::screen_layout::Store::load_layer_at(&self.data_dir, layer)?;
                let rendered = crate::screen_layout::render(&stored.layout)?;
                Ok(Event::ScreenDraft {
                    layer,
                    revision: stored.revision,
                    layout: stored.layout,
                    png: rendered.png,
                    preview_png: rendered.preview_png,
                })
            }
            Job::RenderScreen {
                layer,
                expected_revision,
                layout,
            } => {
                let stored = crate::screen_layout::Store::load_layer_at(&self.data_dir, layer)?;
                ensure!(
                    stored.revision == expected_revision,
                    "Bố cục đã được sửa ở cửa sổ khác. Mở lại trình tùy chỉnh trước khi sửa tiếp"
                );
                let rendered = crate::screen_layout::render(&layout)?;
                let revision = expected_revision
                    .checked_add(1)
                    .context("Revision bố cục vượt giới hạn")?;
                crate::screen_layout::Store {
                    revision,
                    layout: layout.clone(),
                }
                .save_layer_at(&self.data_dir, layer)?;
                Ok(Event::ScreenPreview {
                    layer,
                    revision,
                    layout,
                    png: rendered.png,
                    preview_png: rendered.preview_png,
                })
            }
            Job::ImportScreenAsset { path, background } => Ok(Event::ScreenAsset(
                crate::screen_layout::import_asset(&path, background)?,
            )),
            Job::ApplyScreen {
                layer,
                revision,
                before,
            } => {
                let stored = crate::screen_layout::Store::load_layer_at(&self.data_dir, layer)?;
                ensure!(
                    revision == stored.revision,
                    "Bố cục đã đổi; xem lại ảnh trước khi áp dụng"
                );
                if self
                    .layer_session()
                    .lock()
                    .expect("layer session")
                    .status
                    .active
                    != Some(layer)
                {
                    return Ok(Event::ScreenStored { layer });
                }
                let rendered = crate::screen_layout::render(&stored.layout)?;
                // Persist the generated artwork before starting a hardware operation.
                let output = screen_output(&rendered.gif)?;
                storage::write_private(&output.with_extension("png"), &rendered.png)?;
                let result = self
                    .protocol
                    .as_mut()
                    .context("Chưa kết nối")?
                    .apply_screen_gif(&before, &rendered.gif)?;
                let session = self.layer_session();
                let mut state = session.lock().expect("layer session");
                state.status.displayed = Some(layer);
                state.displayed_revision = Some(revision);
                state.displayed_minute = Some(chrono::Local::now().timestamp() / 60);
                state.paused = false;
                Ok(Event::ScreenApplied { result, output })
            }
            Job::RefreshClock {
                layer,
                revision,
                at,
                screen,
            } => {
                let started = Instant::now();
                let result = (|| -> Result<crate::screen_layout::Rendered> {
                    ensure!(
                        self.layer_session()
                            .lock()
                            .expect("layer session")
                            .status
                            .active
                            == Some(layer),
                        "Lớp đang sửa không phải lớp đang hoạt động"
                    );
                    let stored = crate::screen_layout::Store::load_layer_at(&self.data_dir, layer)?;
                    ensure!(
                        stored.revision == revision && stored.layout.has_clock(),
                        "Bố cục đã đổi trước khi cập nhật đồng hồ"
                    );
                    let rendered = crate::screen_layout::render_at(&stored.layout, at)?;
                    self.protocol
                        .as_mut()
                        .context("Chưa kết nối")?
                        .refresh_screen_gif(&screen, &rendered.gif)?;
                    Ok(rendered)
                })();
                let millis = started.elapsed().as_millis() as u64;
                Ok(match result {
                    Ok(rendered) => Event::ClockRefreshed {
                        layer,
                        revision,
                        png: rendered.png,
                        preview_png: rendered.preview_png,
                        millis,
                        error: None,
                    },
                    Err(error) => Event::ClockRefreshed {
                        layer,
                        revision,
                        png: Vec::new(),
                        preview_png: Vec::new(),
                        millis,
                        error: Some(format!("{error:#}")),
                    },
                })
            }
            Job::UpdateProfiles {
                expected_revision,
                config,
            } => {
                self.profiles.update(expected_revision, config)?;
                if let Some(host) = &self.host {
                    host.update(self.profiles.config.clone());
                }
                self.publish_profiles();
                Ok(Event::ProfilesSaved {
                    config: self.profiles.config.clone(),
                    revision: self.profiles.revision,
                })
            }
            Job::SetActions(enabled) => {
                self.stop_actions();
                if enabled {
                    self.enable_actions()?;
                }
                Ok(Event::HostChanged(
                    self.shared.lock().expect("agent status").status.clone(),
                ))
            }
            Job::StopAgent => {
                self.stop_actions();
                self.engine.shutdown();
                Ok(Event::Done("Đã dừng Coban-mod Agent".into()))
            }
            _ => anyhow::bail!("Yêu cầu không thuộc worker thiết bị"),
        }
    }
}
struct Work {
    job: Job,
    target: Option<std::ffi::CString>,
    expires: Instant,
    reply: mpsc::SyncSender<(Event, Option<ErrorCode>)>,
}
fn screen_output(bytes: &[u8]) -> Result<PathBuf> {
    let path = storage::data_dir().join("screen-artwork").join(format!(
        "screen-{}-{}.gif",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    ));
    storage::write_private(&path, bytes)?;
    Ok(path)
}

pub fn serve() -> Result<()> {
    let Some((listener, _lock)) = ipc::listener()? else {
        return Ok(());
    };
    let profiles = ProfileStore::load(storage::data_dir().join("host-profiles.json"))?;
    let instance = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let shared = Arc::new(Mutex::new(Shared {
        layer_session: Arc::new(Mutex::new(Default::default())),
        status: AgentStatus {
            instance: instance.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            enabled: false,
            state: HostState::Stopped,
            message: "Dịch vụ sẵn sàng; nhận phím đang tắt".into(),
            selected: None,
            profile_revision: profiles.revision,
            layer_sync: Default::default(),
        },
        config: profiles.config.clone(),
    }));
    let messages = shared.clone();
    let engine = ActionEngine::new(move |event| update_host(&messages, event));
    let (tx, rx) = mpsc::sync_channel::<Work>(16);
    let stopping = Arc::new(AtomicBool::new(false));
    let flag = stopping.clone();
    let state = shared.clone();
    let executor = engine.clone();
    let worker = std::thread::spawn(move || {
        let mut broker = Broker {
            protocol: None,
            selected: None,
            profiles,
            host: None,
            shared: state,
            engine: executor,
            last_snapshot: None,
            data_dir: storage::data_dir(),
        };
        loop {
            let work = match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(work) => work,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(error) = broker.update_layer_screen() {
                        let session = broker.layer_session();
                        let mut state = session.lock().expect("layer session");
                        state.paused = true;
                        state.status.message = format!(
                            "Chưa đồng bộ màn hình: {error:#}. Tắt rồi bật tự đổi ảnh để thử lại"
                        );
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let shutdown = matches!(work.job, Job::StopAgent);
            let fatal = work.job.fatal();
            let conflict = matches!(&work.job,Job::UpdateProfiles{expected_revision,..} if *expected_revision!=broker.profiles.revision);
            let bound = matches!(
                work.job,
                Job::Read
                    | Job::Write(_)
                    | Job::Backup(_)
                    | Job::Clock
                    | Job::Image { .. }
                    | Job::ApplyScreen { .. }
                    | Job::RefreshClock { .. }
                    | Job::ConfirmLayer { .. }
                    | Job::SetActions(true)
            );
            let wrong_device = bound
                && (work.target.is_none()
                    || work.target.as_ref() != broker.selected.as_ref().map(|d| &d.path));
            let attempted = !wrong_device && Instant::now() <= work.expires;
            let result = if wrong_device {
                Err(anyhow::anyhow!(
                    "Thiết bị đã đổi hoặc chưa chọn trong cửa sổ này. Kết nối/đọc lại trước khi tiếp tục"
                ))
            } else if Instant::now() > work.expires {
                Err(anyhow::anyhow!(
                    "Yêu cầu hết hạn trước khi thực thi; không gửi lại tự động"
                ))
            } else {
                broker.process(work.job)
            };
            let (event, code) = match result {
                Ok(event) => (event, None),
                Err(error) => {
                    if fatal && attempted {
                        broker.protocol = None;
                    }
                    (
                        Event::Error(format!("{error:#}"), fatal),
                        Some(if conflict {
                            ErrorCode::Conflict
                        } else if fatal {
                            ErrorCode::Uncertain
                        } else {
                            ErrorCode::Failed
                        }),
                    )
                }
            };
            match &event {
                Event::Written(r)
                | Event::ScreenApplied { result: r, .. }
                | Event::LayerConfirmed(r) => {
                    broker.last_snapshot = Some(r.snapshot.clone());
                    broker
                        .shared
                        .lock()
                        .expect("agent status")
                        .layer_session
                        .lock()
                        .expect("layer session")
                        .update_snapshot(r.snapshot.clone());
                }
                Event::Snapshot(snapshot) => {
                    broker.last_snapshot = Some(snapshot.clone());
                    broker
                        .shared
                        .lock()
                        .expect("agent status")
                        .layer_session
                        .lock()
                        .expect("layer session")
                        .update_snapshot(snapshot.clone());
                }
                Event::Error(_, true) => {
                    broker
                        .shared
                        .lock()
                        .expect("agent status")
                        .layer_session
                        .lock()
                        .expect("layer session")
                        .invalidate("Cần kết nối và đồng bộ lại lớp");
                }
                _ => {}
            }
            let shutdown = shutdown && matches!(event, Event::Done(_));
            let _ = work.reply.send((event, code));
            if shutdown {
                flag.store(true, Ordering::Release);
                break;
            }
        }
    });
    let active = Arc::new(AtomicUsize::new(0));
    let mut last_request = Instant::now();
    while !stopping.load(Ordering::Acquire) {
        use interprocess::local_socket::traits::Listener as _;
        match listener.accept() {
            Ok(mut stream) => {
                last_request = Instant::now();
                if active.load(Ordering::Acquire) >= 8 {
                    drop(stream);
                    continue;
                }
                active.fetch_add(1, Ordering::AcqRel);
                let active = active.clone();
                let shared = shared.clone();
                let tx = tx.clone();
                let engine = engine.clone();
                let instance = instance.clone();
                std::thread::spawn(move || {
                    let _guard = ConnectionGuard(active);
                    if let Ok(request) = ipc::receive_request(&mut stream) {
                        let (event, code) =
                            handle_request(&shared, &tx, &engine, &instance, &request);
                        let _ = ipc::send_reply(
                            &mut stream,
                            &Reply {
                                version: WIRE_VERSION,
                                id: request.id,
                                instance,
                                code,
                                event,
                            },
                        );
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if active.load(Ordering::Acquire) > 0 {
                    last_request = Instant::now();
                } else if last_request.elapsed() > UI_IDLE_EXIT {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error.into()),
        }
    }
    drop(tx);
    let _ = worker.join();
    let deadline = Instant::now() + Duration::from_secs(4);
    while active.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
struct ConnectionGuard(Arc<AtomicUsize>);
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
fn handle_request(
    shared: &Arc<Mutex<Shared>>,
    tx: &mpsc::SyncSender<Work>,
    engine: &ActionEngine,
    instance: &str,
    request: &Request,
) -> (Event, Option<ErrorCode>) {
    if request.version != WIRE_VERSION
        || request.app_version != env!("CARGO_PKG_VERSION")
        || (!matches!(request.job, Job::Hello) && request.instance.as_deref() != Some(instance))
    {
        return (
            Event::Error(
                "UI/Agent khác phiên bản hoặc agent đã khởi động lại. Đọc lại trước khi chỉnh"
                    .into(),
                false,
            ),
            Some(ErrorCode::Incompatible),
        );
    }
    match &request.job {
        Job::Hello | Job::Status => {
            let shared = shared.lock().expect("agent status");
            let mut status = shared.status.clone();
            status.layer_sync = shared
                .layer_session
                .lock()
                .expect("layer session")
                .status
                .clone();
            (Event::HostStatus(status), None)
        }
        Job::SetupInput => {
            #[cfg(target_os = "macos")]
            {
                #[link(name = "IOKit", kind = "framework")]
                unsafe extern "C" {
                    fn IOHIDRequestAccess(kind: u32) -> bool;
                }
                unsafe {
                    IOHIDRequestAccess(1);
                }
            }
            (
                Event::Done(
                    "Đã gửi yêu cầu thiết lập nhận phím cho Coban-mod Agent qua macOS".into(),
                ),
                None,
            )
        }
        Job::Profiles => {
            let s = shared.lock().expect("agent status");
            (
                Event::Profiles {
                    config: s.config.clone(),
                    revision: s.status.profile_revision,
                },
                None,
            )
        }
        Job::TestAction { profile, trigger } => {
            let action = shared
                .lock()
                .expect("agent status")
                .config
                .profiles
                .get(*profile)
                .and_then(|p| p.bindings.iter().find(|b| b.trigger == *trigger))
                .map(|b| b.action.clone());
            let result = action
                .context("Chưa có nội dung hành động")
                .and_then(|action| engine.test(action));
            match result{Ok(())=>(Event::TestResult("Agent đã thực thi thử hành động. Phím thật được kiểm tra riêng qua bộ nhận thiết bị".into()),None),Err(e)=>(Event::Error(format!("Thử hành động: {e:#}"),false),Some(ErrorCode::Failed))}
        }
        job => {
            let (reply, rx) = mpsc::sync_channel(1);
            if tx
                .try_send(Work {
                    job: job.clone(),
                    target: request.target.clone(),
                    expires: Instant::now() + Duration::from_secs(5),
                    reply,
                })
                .is_err()
            {
                return (
                    Event::Error("Dịch vụ đang bận; chưa nhận yêu cầu".into(), false),
                    Some(ErrorCode::Busy),
                );
            }
            rx.recv_timeout(Duration::from_secs(65)).unwrap_or((
                Event::Error(
                    "Chưa nhận xác nhận từ worker; đọc lại trước khi chỉnh tiếp".into(),
                    job.fatal(),
                ),
                Some(ErrorCode::Uncertain),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_layer_press_during_upload_sends_the_latest_layer_next_without_rewriting_keys() {
        use crate::{
            layer_sync::{ObservedTransport, Session},
            protocol::MockTransport,
            screen_layout::{Layout, Store},
        };
        struct Display {
            inner: MockTransport,
            packets: Arc<Mutex<Vec<Vec<u8>>>>,
            chunks: usize,
            supported: Arc<AtomicBool>,
            image_safe: Arc<AtomicBool>,
        }
        impl Transport for Display {
            fn simulated(&self) -> bool {
                true
            }
            fn exchange(&mut self, q: &[u8], echo: usize) -> Result<[u8; 32]> {
                self.packets.lock().unwrap().push(q.to_vec());
                if q.starts_with(&[8, 0, crate::device_layer::VALUE_ID]) {
                    let mut r = [0; 32];
                    if !self.supported.load(Ordering::Relaxed) {
                        r[..q.len()].copy_from_slice(q);
                        return Ok(r);
                    }
                    r[..3].copy_from_slice(&[8, 0, crate::device_layer::VALUE_ID]);
                    r[3..7].copy_from_slice(b"CBLR");
                    r[7] = 1;
                    r[8] = if self.image_safe.load(Ordering::Relaxed) {
                        3
                    } else {
                        1
                    };
                    r[9] = u8::from(self.chunks >= 5);
                    r[10] = 4;
                    let mask = 1u32 << r[9];
                    r[11..15].copy_from_slice(&mask.to_be_bytes());
                    r[15..19].copy_from_slice(&1u32.to_be_bytes());
                    return Ok(r);
                }
                if q.starts_with(&[7, 0, 86]) {
                    self.chunks += 1;
                    std::thread::sleep(Duration::from_millis(6));
                }
                self.inner.exchange(q, echo)
            }
        }
        let dir = std::env::temp_dir().join(format!(
            "coban-layer-broker-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let profiles = ProfileStore::load(dir.join("host-profiles.json")).unwrap();
        let session = Arc::new(Mutex::new(Session::default()));
        let shared = Arc::new(Mutex::new(Shared {
            layer_session: session.clone(),
            config: HostConfig::default(),
            status: AgentStatus {
                instance: "test".into(),
                version: "test".into(),
                enabled: false,
                state: HostState::Stopped,
                message: String::new(),
                selected: None,
                profile_revision: 0,
                layer_sync: Default::default(),
            },
        }));
        let packets = Arc::new(Mutex::new(Vec::new()));
        let supported = Arc::new(AtomicBool::new(false));
        let image_safe = Arc::new(AtomicBool::new(false));
        let mut inner = MockTransport::new();
        inner.snapshot = crate::layer_sync::right_knob_cycle(&inner.snapshot).unwrap();
        let before = inner.snapshot.clone();
        let io = Display {
            inner,
            packets: packets.clone(),
            chunks: 0,
            supported: supported.clone(),
            image_safe: image_safe.clone(),
        };
        let protocol = Protocol::new(
            Box::new(ObservedTransport::new(Box::new(io), session.clone())) as Box<dyn Transport>,
        );
        let engine = ActionEngine::new(|_| {});
        let mut broker = Broker {
            protocol: Some(protocol),
            selected: None,
            profiles,
            host: None,
            shared,
            engine,
            last_snapshot: None,
            data_dir: dir.clone(),
        };
        for layer in 0..2 {
            let mut layout = Layout::default();
            layout.tiles[0].text = format!("Lớp {}", layer + 1);
            Store {
                revision: 1,
                layout,
            }
            .save_layer_at(&dir, layer)
            .unwrap();
        }
        assert!(
            broker
                .process(Job::ConfirmLayer {
                    layer: 3,
                    before: Box::new(before.clone())
                })
                .is_err()
        );
        assert!(
            !packets
                .lock()
                .unwrap()
                .iter()
                .any(|q| [5, 7, 9, 15, 21].contains(&q[0])),
            "stock echo must not trigger writes"
        );
        supported.store(true, Ordering::Relaxed);
        assert!(
            broker
                .process(Job::ConfirmLayer {
                    layer: 3,
                    before: Box::new(before.clone())
                })
                .is_err(),
            "old CBLR without the GIF fix must not enable image writes"
        );
        assert!(
            !packets
                .lock()
                .unwrap()
                .iter()
                .any(|q| [5, 7, 9, 15, 21].contains(&q[0]))
        );
        image_safe.store(true, Ordering::Relaxed);
        let event = broker
            .process(Job::ConfirmLayer {
                layer: 3,
                before: Box::new(before.clone()),
            })
            .unwrap();
        assert!(matches!(event, Event::LayerConfirmed(_)));
        assert_eq!(
            session.lock().unwrap().status.active,
            Some(0),
            "editing layer is not a firmware anchor"
        );
        let applied = broker.last_snapshot.as_ref().unwrap();
        assert_eq!(applied.keymap, before.keymap);
        assert_eq!(applied.encoders, before.encoders);
        assert_eq!(
            applied.settings.iter().find(|s| s.key == "layer"),
            before.settings.iter().find(|s| s.key == "layer")
        );
        packets.lock().unwrap().clear();
        broker.update_layer_screen().unwrap();
        assert_eq!(
            session.lock().unwrap().status.active,
            Some(1),
            "press is observed between GIF chunks"
        );
        assert_eq!(
            session.lock().unwrap().status.displayed,
            Some(0),
            "old transfer is reported accurately"
        );
        broker.update_layer_screen().unwrap();
        assert_eq!(session.lock().unwrap().status.displayed, Some(1));
        let packets = packets.lock().unwrap();
        assert!(!packets.iter().any(|q| [5, 15, 21].contains(&q[0])));
        assert!(!packets.iter().any(|q| q.starts_with(&[7, 0, 96])));
        assert!(
            packets
                .iter()
                .filter(|q| q.starts_with(&[7, 0, 81, 2]))
                .count()
                >= 2
        );
        let mut uploaded = Vec::new();
        for q in packets.iter() {
            if q.starts_with(&[7, 0, 86]) {
                let offset = u32::from_be_bytes(q[3..7].try_into().unwrap()) as usize;
                let end = offset + q.len() - 7;
                uploaded.resize(uploaded.len().max(end), 0);
                uploaded[offset..end].copy_from_slice(&q[7..]);
            } else if q.starts_with(&[7, 0, 87]) {
                uploaded.truncate(u32::from_be_bytes(q[3..7].try_into().unwrap()) as usize);
            }
        }
        let second = Store::load_layer_at(&dir, 1).unwrap();
        assert_eq!(
            uploaded,
            crate::screen_layout::render(&second.layout).unwrap().gif
        );
        drop(packets);
        session.lock().unwrap().invalidate("connection lost");
        broker.engine.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn profile_migration_keeps_legacy_file_and_rejects_stale_revision() {
        let dir = std::env::temp_dir().join(format!(
            "mk-profile-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("host-profiles.json");
        let legacy=br#"{"profiles":[{"name":"web","application":"","bindings":[{"trigger":104,"action":{"type":"Open","value":"example.com"}}]}]}"#;
        std::fs::write(&path, legacy).unwrap();
        let mut store = ProfileStore::load(path.clone()).unwrap();
        assert!(matches!(
            store.config.action("", 104),
            Some(host::HostAction::Web(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        let mut next = store.config.clone();
        next.profiles[0].name = "changed".into();
        store.update(0, next).unwrap();
        assert_eq!(store.revision, 1);
        assert_eq!(
            std::fs::read(dir.join("host-profiles.pre-agent.json")).unwrap(),
            legacy
        );
        let before = std::fs::read(&path).unwrap();
        assert!(store.update(0, HostConfig::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(ProfileStore::load(path).unwrap().revision, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
