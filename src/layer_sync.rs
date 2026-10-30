//! Live CBLR layer tracking and serialized image synchronization.
use crate::model::Snapshot;
use anyhow::{Result, ensure};

/// Builds an explicit configuration edit. This function performs no USB writes
/// and does not claim that the device has switched its active layer.
pub fn right_knob_cycle(before: &Snapshot) -> Result<Snapshot> {
    before.validate()?;
    ensure!(before.layers > 1, "Bàn phím cần ít nhất hai lớp");
    let mut after = before.clone();
    for layer in 0..before.layers as usize {
        let next = (layer + 1) % before.layers as usize;
        after.set_binding(layer, 1, 0x5200 + next as u16)?;
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_changes_only_the_right_press_and_preserves_rotations_and_macros() {
        let before = Snapshot::demo();
        let after = right_knob_cycle(&before).unwrap();
        assert_eq!(after.binding(0, 1), 0x5201);
        assert_eq!(after.binding(1, 1), 0x5202);
        assert_eq!(after.binding(2, 1), 0x5203);
        assert_eq!(after.binding(3, 1), 0x5200);
        let mut restored = after;
        for layer in 0..before.layers as usize {
            restored
                .set_binding(layer, 1, before.binding(layer, 1))
                .unwrap();
        }
        assert_eq!(restored, before);
    }
}

use crate::{
    device_layer::{self, DeviceLayer},
    transport::Transport,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub active: Option<u8>,
    pub displayed: Option<u8>,
    pub message: String,
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub image_safe: bool,
}

#[derive(Default)]
pub struct Session {
    pub status: Status,
    snapshot: Option<Snapshot>,
    pub paused: bool,
    pub displayed_revision: Option<u64>,
    pub displayed_minute: Option<i64>,
}
impl Session {
    pub fn reset(&mut self, snapshot: Snapshot) {
        let enabled = self.status.enabled;
        *self = Self::default();
        self.status.enabled = enabled;
        self.snapshot = Some(snapshot);
        self.status.message = "Đang đọc lớp thực tế từ bàn phím…".into();
    }
    pub fn invalidate(&mut self, reason: impl Into<String>) {
        self.status.active = None;
        self.status.message = reason.into();
    }
    pub fn update_snapshot(&mut self, snapshot: Snapshot) {
        if self.status.enabled
            && snapshot
                .settings
                .iter()
                .find(|s| s.key == "screen")
                .is_none_or(|s| s.value[0] != 2)
        {
            self.status.enabled = false;
            self.status.message = "Đã dừng tự đổi ảnh vì đổi chế độ màn hình".into();
        }
        self.snapshot = Some(snapshot);
    }
    pub fn observe_device(&mut self, state: Option<DeviceLayer>) -> Result<()> {
        let Some(state) = state else {
            self.status.supported = false;
            self.status.image_safe = false;
            self.invalidate("Firmware chưa hỗ trợ đọc lớp CBLR; không thể tự đổi ảnh");
            return Ok(());
        };
        self.status.supported = true;
        self.status.image_safe = state.image_safe;
        if !state.image_safe {
            self.status.enabled = false;
        }
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| state.count != snapshot.layers)
        {
            self.invalidate("Số lớp CBLR không khớp cấu hình");
            return Err(anyhow::anyhow!("Số lớp CBLR không khớp cấu hình"));
        }
        if self.status.active != Some(state.active) {
            self.status.active = Some(state.active);
            self.status.message = if self.status.enabled {
                format!("Đang gửi ảnh lớp {}…", state.active + 1)
            } else {
                format!("Đã đọc lớp {} từ bàn phím", state.active + 1)
            };
        }
        if !state.image_safe {
            self.status.message =
                "Firmware đọc được lớp nhưng cần bản vá GIF v1.1 để tự đổi ảnh".into();
        }
        Ok(())
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.status.enabled = enabled;
        self.paused = false;
        self.status.displayed = None;
        self.displayed_revision = None;
        self.displayed_minute = None;
        self.status.message = if enabled {
            "Đang đồng bộ ảnh theo lớp bàn phím…"
        } else {
            "Đã tắt tự đổi ảnh; vẫn đọc lớp thực tế"
        }
        .into();
    }
}

/// Interleave live layer GETs on the existing USB owner, even during GIF upload.
/// No matrix inference, fabricated startup anchor or unsolicited input reader.
pub struct ObservedTransport {
    inner: Box<dyn Transport>,
    pub session: Arc<Mutex<Session>>,
    last_poll: Instant,
}
impl ObservedTransport {
    pub fn new(inner: Box<dyn Transport>, session: Arc<Mutex<Session>>) -> Self {
        Self {
            inner,
            session,
            last_poll: Instant::now(),
        }
    }
}
impl Transport for ObservedTransport {
    fn simulated(&self) -> bool {
        self.inner.simulated()
    }
    fn exchange(&mut self, request: &[u8], echo: usize) -> Result<[u8; 32]> {
        let result = self.inner.exchange(request, echo);
        if let Ok(reply) = &result {
            let report = if request.starts_with(&[8, 0, device_layer::VALUE_ID]) {
                Some(DeviceLayer::from_reply(reply))
            } else if self.last_poll.elapsed() >= Duration::from_millis(50) {
                Some(
                    self.inner
                        .exchange(&device_layer::request(), 3)
                        .and_then(|r| DeviceLayer::from_reply(&r)),
                )
            } else {
                None
            };
            if let Some(report) = report {
                self.last_poll = Instant::now();
                let mut session = self.session.lock().expect("layer session");
                if let Err(error) = report.and_then(|state| session.observe_device(state)) {
                    session.invalidate(format!("Không đọc được lớp thực tế: {error:#}"));
                }
            }
        } else {
            self.session
                .lock()
                .expect("layer session")
                .invalidate("USB gián đoạn; cần kết nối/đọc lại");
        }
        result
    }
}

#[cfg(test)]
mod tracking_tests {
    use super::*;
    fn native(active: u8) -> DeviceLayer {
        DeviceLayer {
            active,
            count: 4,
            layer_mask: 1 << active,
            default_mask: 1,
            uptime_ms: 0,
            image_safe: true,
        }
    }
    #[test]
    fn live_state_recovers_startup_timeout_and_missed_transitions_without_anchor() {
        let mut tracker = Session::default();
        tracker.reset(Snapshot::demo());
        assert_eq!(tracker.status.active, None);
        tracker.observe_device(Some(native(2))).unwrap();
        assert_eq!(tracker.status.active, Some(2));
        tracker.set_enabled(true);
        tracker.observe_device(Some(native(2))).unwrap();
        assert_eq!(
            tracker.status.active,
            Some(2),
            "repeated/held state does not advance"
        );
        tracker.observe_device(Some(native(0))).unwrap();
        assert_eq!(
            tracker.status.active,
            Some(0),
            "auto-return is read directly"
        );
        tracker.invalidate("sleep");
        tracker.observe_device(Some(native(3))).unwrap();
        assert_eq!(
            tracker.status.active,
            Some(3),
            "does not count missed transitions"
        );
        tracker.reset(Snapshot::demo());
        assert_eq!(tracker.status.active, None);
        assert!(tracker.status.enabled);
        tracker.observe_device(Some(native(1))).unwrap();
        assert_eq!(
            tracker.status.active,
            Some(1),
            "reconnect reads actual state"
        );
    }
    #[test]
    fn stock_echo_and_count_mismatch_never_supply_an_active_layer() {
        let mut tracker = Session::default();
        tracker.reset(Snapshot::demo());
        tracker.observe_device(None).unwrap();
        assert!(!tracker.status.supported);
        assert_eq!(tracker.status.active, None);
        let mut old = native(2);
        old.image_safe = false;
        tracker.set_enabled(true);
        tracker.observe_device(Some(old)).unwrap();
        assert_eq!(tracker.status.active, Some(2));
        assert!(
            !tracker.status.enabled,
            "unpatched firmware must never auto-send images"
        );
        let mut wrong = native(0);
        wrong.count = 5;
        assert!(tracker.observe_device(Some(wrong)).is_err());
        assert_eq!(tracker.status.active, None);
    }
}
