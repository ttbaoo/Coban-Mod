use crate::{
    model::{Identity, Setting, Snapshot},
    storage,
    transport::Transport,
};
use anyhow::{Context, Result, bail, ensure};

#[derive(Clone, Debug)]
pub struct SettingSpec {
    pub key: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub channel: u8,
    pub id: u8,
    pub len: usize,
    pub save_channel: u8,
    pub save_id: u8,
    pub fields: Vec<(&'static str, u8)>,
    pub default: Vec<u8>,
}
impl SettingSpec {
    pub fn validate(&self, value: &[u8]) -> Result<()> {
        ensure!(value.len() == self.len, "Sai kích thước {}", self.key);
        Ok(())
    }
}
pub fn setting_specs() -> Vec<SettingSpec> {
    type Definition = (
        &'static str,
        &'static str,
        &'static str,
        u8,
        u8,
        u8,
        u8,
        Vec<(&'static str, u8)>,
        Vec<u8>,
    );
    let definitions: Vec<Definition> = vec![
        (
            "screen",
            "Màn hình",
            "Điều khiển màn hình",
            0,
            81,
            0,
            150,
            vec![
                ("Màn hình", 5),
                ("Tự hiện lớp khi nhấn (0/1)", 1),
                ("Hiện lớp (giây)", 255),
                ("Tắt màn hình sau (phút; 0 luôn bật)", 255),
            ],
            vec![0, 1, 5, 5],
        ),
        (
            "brightness",
            "Màn hình",
            "Độ sáng màn hình",
            1,
            1,
            1,
            0,
            vec![("Độ sáng", 255)],
            vec![153],
        ),
        (
            "layer",
            "Màn hình",
            "Tự về lớp mặc định",
            0,
            96,
            0,
            150,
            vec![("Bật (0/1)", 1), ("Thời gian (giây)", 255)],
            vec![1, 60],
        ),
        (
            "theme",
            "Màn hình",
            "Màu giao diện · RGB",
            0,
            100,
            0,
            150,
            vec![
                ("Nền R", 255),
                ("Nền G", 255),
                ("Nền B", 255),
                ("Chữ chính R", 255),
                ("Chữ chính G", 255),
                ("Chữ chính B", 255),
                ("Chữ phụ R", 255),
                ("Chữ phụ G", 255),
                ("Chữ phụ B", 255),
                ("Đồ họa R", 255),
                ("Đồ họa G", 255),
                ("Đồ họa B", 255),
                ("Nền đồ họa R", 255),
                ("Nền đồ họa G", 255),
                ("Nền đồ họa B", 255),
            ],
            vec![
                0, 0, 0, 255, 255, 255, 136, 136, 136, 0, 255, 0, 255, 255, 255,
            ],
        ),
        (
            "pomodoro",
            "Màn hình",
            "Pomodoro",
            0,
            89,
            0,
            150,
            vec![
                ("Số hiệp", 255),
                ("Tập trung (phút)", 255),
                ("Nghỉ (phút)", 255),
                ("Thông báo (0 tắt · 1 âm · 2 RGB · 3 cả hai)", 3),
            ],
            vec![4, 25, 5, 1],
        ),
        (
            "clock",
            "Màn hình",
            "Đồng hồ",
            0,
            85,
            0,
            150,
            vec![
                ("Kiểu (0 thường · 1 vuông · 2 nghệ thuật)", 2),
                ("Định dạng (0 12 giờ · 1 24 giờ)", 1),
                ("Hiện AM/PM (0 ẩn · 1 hiện)", 1),
                ("Ngày (0 dd/mm/yyyy · 1 mm/dd/yyyy · 2 yyyy/mm/dd)", 2),
                ("Ngày (0 hiện · 1 ẩn)", 1),
            ],
            vec![0, 1, 0, 0, 0],
        ),
        (
            "stats",
            "Màn hình",
            "Bố cục thông tin hệ thống",
            0,
            83,
            0,
            150,
            vec![
                ("Bố cục", 2),
                ("Metric ô 1", 16),
                ("Metric ô 2", 16),
                ("Metric ô 3", 16),
                ("Metric ô 4", 16),
            ],
            vec![0, 1, 2, 3, 0],
        ),
        (
            "gif_background",
            "Màn hình",
            "GIF làm nền",
            0,
            99,
            0,
            150,
            vec![("Bật (0/1)", 1)],
            vec![1],
        ),
        (
            "rgb_brightness",
            "Đèn phím",
            "Độ sáng RGB",
            3,
            1,
            3,
            1,
            vec![("Độ sáng", 255)],
            vec![255],
        ),
        (
            "rgb_effect",
            "Đèn phím",
            "Hiệu ứng RGB",
            3,
            2,
            3,
            2,
            vec![("Hiệu ứng (0–12; xem danh sách bên dưới)", 12)],
            vec![7],
        ),
        (
            "rgb_speed",
            "Đèn phím",
            "Tốc độ hiệu ứng",
            3,
            3,
            3,
            3,
            vec![("Tốc độ", 255)],
            vec![127],
        ),
        (
            "rgb_color",
            "Đèn phím",
            "Màu RGB",
            3,
            4,
            3,
            4,
            vec![("Hue", 255), ("Saturation", 255)],
            vec![128, 255],
        ),
        (
            "audio",
            "Âm thanh",
            "Loa",
            4,
            1,
            4,
            1,
            vec![("Bật (0/1)", 1)],
            vec![1],
        ),
        (
            "clicky",
            "Âm thanh",
            "Âm khi nhấn phím",
            4,
            2,
            4,
            2,
            vec![("Bật (0/1)", 1)],
            vec![0],
        ),
        (
            "audio_level",
            "Âm thanh",
            "Mức âm thanh",
            0,
            97,
            0,
            97,
            vec![("Mức âm thanh", 4)],
            vec![0],
        ),
    ];
    definitions
        .into_iter()
        .map(
            |(key, group, title, channel, id, save_channel, save_id, fields, default)| {
                SettingSpec {
                    key,
                    group,
                    title,
                    channel,
                    id,
                    len: fields.len(),
                    save_channel,
                    save_id,
                    fields,
                    default,
                }
            },
        )
        .collect()
}
pub struct Protocol<T: Transport> {
    pub io: T,
}
impl<T: Transport> Protocol<T> {
    pub fn new(io: T) -> Self {
        Self { io }
    }
    /// Optional firmware extension; a stock echo is unsupported, never layer 0.
    pub fn read_active_layer(&mut self) -> Result<Option<crate::device_layer::DeviceLayer>> {
        let reply = self.request(&crate::device_layer::request(), 3)?;
        crate::device_layer::DeviceLayer::from_reply(&reply)
    }
    fn request(&mut self, data: &[u8], echo: usize) -> Result<[u8; 32]> {
        self.io.exchange(data, echo)
    }
    pub fn read_snapshot(&mut self, identity: Identity) -> Result<Snapshot> {
        let p = self.request(&[1], 1)?;
        let version = u16::from_be_bytes([p[1], p[2]]);
        ensure!(
            (1..=64).contains(&version),
            "VIA version không hợp lệ: {version}"
        );
        let layers = self.request(&[17], 1)?[1];
        ensure!((1..=32).contains(&layers), "Số layer không hợp lệ");
        let size = layers as usize * 24;
        let mut bytes = Vec::with_capacity(size);
        for offset in (0..size).step_by(28) {
            let len = (size - offset).min(28);
            let r = self.request(&[18, (offset >> 8) as u8, offset as u8, len as u8], 4)?;
            bytes.extend_from_slice(&r[4..4 + len]);
        }
        let keymap = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        let mut encoders = Vec::new();
        for layer in 0..layers {
            let mut e = [0; 4];
            for (i, v) in e.iter_mut().enumerate() {
                let r = self.request(&[20, layer, (i / 2) as u8, (i % 2) as u8], 4)?;
                *v = u16::from_be_bytes([r[4], r[5]]);
            }
            encoders.push(e);
        }
        let macro_count = self.request(&[12], 1)?[1];
        ensure!(macro_count <= 128, "Quá nhiều macro");
        let r = self.request(&[13], 1)?;
        let macro_size = u16::from_be_bytes([r[1], r[2]]) as usize;
        let mut macro_buffer = Vec::new();
        for offset in (0..macro_size).step_by(28) {
            let len = (macro_size - offset).min(28);
            let r = self.request(&[14, (offset >> 8) as u8, offset as u8, len as u8], 4)?;
            macro_buffer.extend_from_slice(&r[4..4 + len]);
        }
        let mut macro_names = Vec::new();
        for i in 0..macro_count {
            let r = self.request(&[8, 0, 98, i], 4)?;
            let name = r[4..10]
                .iter()
                .take_while(|b| **b != 0)
                .filter(|b| b.is_ascii_graphic() || **b == b' ')
                .map(|b| *b as char)
                .collect::<String>();
            macro_names.push(name.trim_end().into());
        }
        let mut settings = Vec::new();
        let mut warnings = Vec::new();
        for spec in setting_specs() {
            match self.request(&[8, spec.channel, spec.id], 3) {
                Ok(r) => settings.push(Setting {
                    key: spec.key.into(),
                    value: r[3..3 + spec.len].to_vec(),
                }),
                Err(e) => warnings.push(format!("{}: {e:#}", spec.title)),
            }
        }
        let snapshot = Snapshot {
            simulated: self.io.simulated(),
            schema_version: 1,
            captured_at: chrono::Utc::now().to_rfc3339(),
            device: identity,
            protocol_version: version,
            layers,
            keymap,
            encoders,
            macro_count,
            macro_buffer,
            macro_names,
            settings,
            warnings,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn upload_gif(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            "Dữ liệu không phải GIF"
        );
        ensure!(
            bytes.len() <= crate::media::MAX_BYTES,
            "GIF vượt dung lượng"
        );
        let image = crate::media::preview(bytes)?;
        ensure!(
            image.width() == 160 && image.height() == 80,
            "GIF phải có kích thước 160×80"
        );
        // The upstream SET-size handler creates a second GIF decoder in GIF
        // mode. Use it only when the firmware advertises the v1.1 fix; stock
        // firmware keeps the legacy client sequence and parses the GIF trailer.
        let size_command = if self.read_active_layer()?.is_some_and(|s| s.image_safe) {
            7
        } else {
            9
        };
        for (i, chunk) in bytes.chunks(25).enumerate() {
            let off = (i * 25) as u32;
            let mut req = vec![7, 0, 86];
            req.extend_from_slice(&off.to_be_bytes());
            req.extend_from_slice(chunk);
            self.request(&req, 7)
                .with_context(|| format!("Gửi GIF dừng tại byte {off}; chưa xác nhận lưu"))?;
        }
        let mut size = vec![size_command, 0, 87];
        size.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        self.request(&size, 3)?;
        self.request(&[9, 0, 151], 3)?;
        self.request(&[9, 0, 150], 3)?;
        Ok(())
    }
    pub fn sync_clock(&mut self) -> Result<()> {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        self.request(
            &[
                7,
                0,
                82,
                t.hour() as u8,
                t.minute() as u8,
                t.second() as u8,
                t.day() as u8,
                t.month() as u8,
                (t.year() % 100) as u8,
            ],
            3,
        )?;
        Ok(())
    }
    /// Clock tick: replaces the GIF already shown in Ảnh/GIF mode. Settings are unchanged,
    /// so there is no backup or readback; `screen` is the device's current screen setting.
    pub fn refresh_screen_gif(&mut self, screen: &[u8], bytes: &[u8]) -> Result<()> {
        ensure!(
            screen.first() == Some(&2),
            "Màn hình bàn phím không ở chế độ Ảnh/GIF"
        );
        self.upload_gif(bytes)?;
        let mut request = vec![7, 0, 81];
        request.extend_from_slice(screen);
        self.request(&request, 3)
            .context("Ảnh đồng hồ đã gửi; chưa kích hoạt lại màn hình Ảnh/GIF")?;
        self.request(&[9, 0, 150], 3)?;
        Ok(())
    }
    /// Explicit artwork replacement. GIF bytes cannot be read back from stock firmware.
    pub fn apply_screen_gif(&mut self, base: &Snapshot, bytes: &[u8]) -> Result<ApplyResult> {
        base.validate()?;
        let mut after = base.clone();
        let screen = after
            .settings
            .iter_mut()
            .find(|s| s.key == "screen")
            .context("Thiết bị không có chế độ màn hình")?;
        screen.value[0] = 2;
        screen.value[1] = 0; // keep the composed image visible while keys are pressed
        after.validate()?;
        let live = self.read_snapshot(base.device.clone())?;
        ensure!(
            base.same_content(&live),
            "Cấu hình đã đổi bên ngoài Coban-mod; đọc lại trước khi gửi ảnh"
        );
        // Validate the complete payload before backing up or sending a first chunk.
        ensure!(
            bytes.len() <= crate::media::MAX_BYTES
                && (bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a")),
            "Ảnh màn hình không hợp lệ"
        );
        let image = crate::media::preview(bytes)?;
        ensure!(
            image.dimensions() == (160, 80),
            "Ảnh phải có kích thước 160×80"
        );
        let backup = storage::automatic_backup(&live)?;
        self.upload_gif(bytes)?;
        // The stock client always reselects animation after saving the GIF. A diff-only
        // settings apply skipped this when already in GIF mode, leaving the old decoder open.
        let screen = after.settings.iter().find(|s| s.key == "screen").unwrap();
        let mut request = vec![7, 0, 81];
        request.extend_from_slice(&screen.value);
        self.request(&request, 3)
            .context("Ảnh đã gửi; chưa kích hoạt lại màn hình Ảnh/GIF")?;
        self.request(&[9, 0, 150], 3)
            .context("Chưa xác nhận lưu chế độ màn hình sau khi gửi ảnh")?;
        let snapshot = self.read_snapshot(base.device.clone())?;
        ensure!(
            after.same_content(&snapshot),
            "Chế độ hiển thị đọc lại không khớp"
        );
        Ok(ApplyResult {
            snapshot,
            backup: Some(backup),
            changes: 1,
        })
    }
    pub fn apply(&mut self, base: &Snapshot, draft: &Snapshot) -> Result<ApplyResult> {
        base.compatible(draft)?;
        for (before, after) in base.keymap.iter().zip(&draft.keymap) {
            if before != after {
                crate::keycodes::validate_for_device(*after, base.layers, base.macro_count)?;
            }
        }
        for (before, after) in base
            .encoders
            .iter()
            .flatten()
            .zip(draft.encoders.iter().flatten())
        {
            if before != after {
                crate::keycodes::validate_for_device(*after, base.layers, base.macro_count)?;
            }
        }

        if base.macro_buffer != draft.macro_buffer {
            crate::macros::split(&draft.macro_buffer, draft.macro_count as usize)?;
        }
        for spec in setting_specs() {
            if let (Some(b), Some(a)) = (
                base.settings.iter().find(|x| x.key == spec.key),
                draft.settings.iter().find(|x| x.key == spec.key),
            ) && b != a
            {
                for (i, v) in a.value.iter().enumerate() {
                    if *v != b.value[i] {
                        ensure!(*v <= spec.fields[i].1, "{} vượt giới hạn", spec.fields[i].0);
                        let options = field_options(spec.key, i);
                        ensure!(
                            options.is_empty() || options.iter().any(|(n, _)| n == v),
                            "{} không thuộc lựa chọn firmware hỗ trợ",
                            spec.fields[i].0
                        );
                        if spec.key == "pomodoro" && i < 3 {
                            ensure!(*v > 0, "Số hiệp và thời gian Pomodoro phải lớn hơn 0");
                        }
                    }
                }
            }
        }

        if base.same_content(draft) {
            return Ok(ApplyResult {
                snapshot: base.clone(),
                backup: None,
                changes: 0,
            });
        }
        let live = self.read_snapshot(base.device.clone())?;
        ensure!(
            base.same_content(&live),
            "Cấu hình bàn phím đã thay đổi bên ngoài Coban-mod. Đọc lại trước khi Áp dụng."
        );
        let backup = storage::automatic_backup(&live)?;
        let mut changes = 0;
        let result = (|| -> Result<Snapshot> {
            for layer in 0..base.layers as usize {
                let mut key_changed = false;
                for r in 0..3 {
                    for c in 0..4 {
                        let i = layer * 12 + r * 4 + c;
                        if base.keymap[i] != draft.keymap[i] {
                            let v = draft.keymap[i];
                            self.request(
                                &[5, layer as u8, r as u8, c as u8, (v >> 8) as u8, v as u8],
                                4,
                            )?;
                            changes += 1;
                            key_changed = true;
                        }
                    }
                }
                if key_changed {
                    self.request(&[7, 0, 149, layer as u8], 3)?;
                }
                for i in 0..4 {
                    if base.encoders[layer][i] != draft.encoders[layer][i] {
                        let v = draft.encoders[layer][i];
                        self.request(
                            &[
                                21,
                                layer as u8,
                                (i / 2) as u8,
                                (i % 2) as u8,
                                (v >> 8) as u8,
                                v as u8,
                            ],
                            4,
                        )?;
                        changes += 1;
                    }
                }
            }
            if base.macro_buffer != draft.macro_buffer {
                crate::macros::split(&draft.macro_buffer, draft.macro_count as usize)?;
                for (offset, data) in draft.macro_buffer.chunks(28).enumerate() {
                    let offset = offset * 28;
                    if data == &base.macro_buffer[offset..offset + data.len()] {
                        continue;
                    }
                    let mut req = vec![15, (offset >> 8) as u8, offset as u8, data.len() as u8];
                    req.extend_from_slice(data);
                    self.request(&req, 4)?;
                }
                changes += 1;
            }
            for i in 0..base.macro_count as usize {
                if base.macro_names[i] != draft.macro_names[i] {
                    let mut req = vec![7, 0, 98, i as u8];
                    let mut name = [0; 6];
                    name[..draft.macro_names[i].len()]
                        .copy_from_slice(draft.macro_names[i].as_bytes());
                    req.extend_from_slice(&name);
                    self.request(&req, 4)?;
                    self.request(&[9, 0, 150], 3)?;
                    changes += 1;
                }
            }
            for spec in setting_specs() {
                let before = base.settings.iter().find(|x| x.key == spec.key);
                let after = draft.settings.iter().find(|x| x.key == spec.key);
                if let (Some(b), Some(a)) = (before, after)
                    && b != a
                {
                    for (i, v) in a.value.iter().enumerate() {
                        if *v != b.value[i] {
                            ensure!(*v <= spec.fields[i].1, "{} vượt giới hạn", spec.fields[i].0);
                            let options = field_options(spec.key, i);
                            ensure!(
                                options.is_empty() || options.iter().any(|(n, _)| n == v),
                                "{} không thuộc lựa chọn firmware hỗ trợ",
                                spec.fields[i].0
                            );
                            if spec.key == "pomodoro" && i < 3 {
                                ensure!(*v > 0, "Số hiệp và thời gian Pomodoro phải lớn hơn 0");
                            }
                        }
                    }
                    let mut req = vec![7, spec.channel, spec.id];
                    req.extend_from_slice(&a.value);
                    self.request(&req, 3)?;
                    self.request(&[9, spec.save_channel, spec.save_id], 3)?;
                    changes += 1;
                }
            }
            let readback = self.read_snapshot(base.device.clone())?;
            ensure!(
                draft.same_content(&readback),
                "Đọc lại không khớp bản nháp; không xác nhận lưu thành công"
            );
            Ok(readback)
        })();
        match result {
            Ok(snapshot) => Ok(ApplyResult {
                snapshot,
                backup: Some(backup),
                changes,
            }),
            Err(e) => bail!(
                "Ghi chưa hoàn tất hoặc chưa xác minh. Có thể đã ghi một phần. Bản sao: {}. Chi tiết: {e:#}",
                backup.display()
            ),
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApplyResult {
    pub snapshot: Snapshot,
    pub backup: Option<std::path::PathBuf>,
    pub changes: usize,
}

/// Deterministic virtual device for UI preview and error-injection tests.
pub struct MockTransport {
    pub snapshot: Snapshot,
    pub writes: usize,
    pub fail_after: Option<usize>,
    pub gif_bytes: Vec<u8>,
}
impl MockTransport {
    pub fn new() -> Self {
        Self {
            snapshot: Snapshot::demo(),
            writes: 0,
            fail_after: None,
            gif_bytes: Vec::new(),
        }
    }
}
impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}
impl Transport for MockTransport {
    fn simulated(&self) -> bool {
        true
    }

    fn exchange(&mut self, q: &[u8], _: usize) -> Result<[u8; 32]> {
        let mut r = [0; 32];
        r[..q.len()].copy_from_slice(q);
        if [5, 7, 9, 15, 21].contains(&q[0]) {
            if self.fail_after == Some(self.writes) {
                bail!("Mô phỏng mất kết nối");
            }
            self.writes += 1;
        }
        match q[0] {
            8 if q[1] == 0 && q[2] == crate::device_layer::VALUE_ID => {
                r.fill(0);
                r[..3].copy_from_slice(&[8, 0, crate::device_layer::VALUE_ID]);
                r[3..7].copy_from_slice(b"CBLR");
                r[7] = 1;
                r[8] = 3;
                r[10] = self.snapshot.layers;
                r[15..19].copy_from_slice(&1u32.to_be_bytes());
            }
            1 => r[1..3].copy_from_slice(&self.snapshot.protocol_version.to_be_bytes()),
            17 => r[1] = self.snapshot.layers,
            18 => {
                let bytes: Vec<_> = self
                    .snapshot
                    .keymap
                    .iter()
                    .flat_map(|k| k.to_be_bytes())
                    .collect();
                let off = u16::from_be_bytes([q[1], q[2]]) as usize;
                let n = q[3] as usize;
                r[4..4 + n].copy_from_slice(&bytes[off..off + n]);
            }
            4 | 5 => {
                let i = q[1] as usize * 12 + q[2] as usize * 4 + q[3] as usize;
                if q[0] == 5 {
                    self.snapshot.keymap[i] = u16::from_be_bytes([q[4], q[5]]);
                }
                r[4..6].copy_from_slice(&self.snapshot.keymap[i].to_be_bytes());
            }
            20 | 21 => {
                let i = q[2] as usize * 2 + q[3] as usize;
                if q[0] == 21 {
                    self.snapshot.encoders[q[1] as usize][i] = u16::from_be_bytes([q[4], q[5]]);
                }
                r[4..6].copy_from_slice(&self.snapshot.encoders[q[1] as usize][i].to_be_bytes());
            }
            12 => r[1] = self.snapshot.macro_count,
            13 => r[1..3].copy_from_slice(&(self.snapshot.macro_buffer.len() as u16).to_be_bytes()),
            14 | 15 => {
                let off = u16::from_be_bytes([q[1], q[2]]) as usize;
                let n = q[3] as usize;
                if q[0] == 15 {
                    self.snapshot.macro_buffer[off..off + n].copy_from_slice(&q[4..4 + n]);
                }
                r[4..4 + n].copy_from_slice(&self.snapshot.macro_buffer[off..off + n]);
            }
            7 | 8 if q[1] == 0 && q[2] == 98 => {
                let i = q[3] as usize;
                if q[0] == 7 {
                    self.snapshot.macro_names[i] = q[4..10]
                        .iter()
                        .take_while(|x| **x != 0)
                        .map(|b| *b as char)
                        .collect();
                }
                let name = &self.snapshot.macro_names[i];
                r[4..4 + name.len()].copy_from_slice(name.as_bytes());
            }
            7 if q[1] == 0 && q[2] == 86 => {
                let offset = u32::from_be_bytes(q[3..7].try_into().unwrap()) as usize;
                let end = offset + q.len() - 7;
                ensure!(end <= crate::media::MAX_BYTES, "GIF quá lớn");
                self.gif_bytes.resize(self.gif_bytes.len().max(end), 0);
                self.gif_bytes[offset..end].copy_from_slice(&q[7..]);
            }
            7 if q[1] == 0 && q[2] == 87 => {
                let size = u32::from_be_bytes(q[3..7].try_into().unwrap()) as usize;
                self.gif_bytes.truncate(size);
            }
            7 | 8 => {
                if q[2] != 82 && q[2] != 149 {
                    let specs = setting_specs();
                    let spec = specs
                        .iter()
                        .find(|s| s.channel == q[1] && s.id == q[2])
                        .context("Setting không có")?;
                    let v = self
                        .snapshot
                        .settings
                        .iter_mut()
                        .find(|s| s.key == spec.key)
                        .context("Setting không có")?;
                    if q[0] == 7 {
                        v.value.copy_from_slice(&q[3..3 + spec.len]);
                    }
                    r[3..3 + spec.len].copy_from_slice(&v.value);
                }
            }
            9 => {}
            _ => bail!("Lệnh mô phỏng chưa hỗ trợ"),
        }
        Ok(r)
    }
}

pub fn field_options(key: &str, index: usize) -> Vec<(u8, String)> {
    let list: Vec<(u8, &str)> = match (key, index) {
        ("screen", 0) => vec![
            (0, "Đồng hồ"),
            (1, "Thông tin hệ thống"),
            (2, "Ảnh / GIF"),
            (3, "Lớp phím"),
            (5, "Pomodoro"),
        ],
        ("screen", 1) | ("layer", 0) | ("audio", 0) | ("clicky", 0) | ("gif_background", 0) => {
            vec![(0, "Tắt"), (1, "Bật")]
        }
        ("layer", 1) => vec![
            (5, "5 giây"),
            (15, "15 giây"),
            (30, "30 giây"),
            (60, "1 phút"),
            (120, "2 phút"),
        ],
        ("screen", 3) => vec![
            (0, "Luôn bật"),
            (1, "1 phút"),
            (5, "5 phút"),
            (15, "15 phút"),
            (30, "30 phút"),
            (45, "45 phút"),
            (60, "1 giờ"),
        ],
        ("pomodoro", 3) => vec![
            (0, "Tắt"),
            (1, "Âm thanh"),
            (2, "Nháy RGB"),
            (3, "Âm thanh + RGB"),
        ],
        ("clock", 0) => vec![(0, "Thường"), (1, "Vuông"), (2, "Nghệ thuật")],
        ("clock", 1) => vec![(0, "12 giờ"), (1, "24 giờ")],
        ("clock", 2) => vec![(0, "Ẩn AM/PM"), (1, "Hiện AM/PM")],
        ("clock", 3) => vec![(0, "dd/mm/yyyy"), (1, "mm/dd/yyyy"), (2, "yyyy/mm/dd")],
        ("clock", 4) => vec![(0, "Hiện ngày"), (1, "Ẩn ngày")],
        ("stats", 0) => vec![
            (0, "2×1 · hai biểu đồ cung"),
            (1, "2×2 · bốn biểu đồ"),
            (2, "Hỗn hợp"),
        ],
        ("stats", 1..=4) => vec![
            (0, "Trống"),
            (1, "CPU sử dụng"),
            (2, "Nhiệt độ CPU"),
            (3, "RAM"),
            (4, "GPU sử dụng"),
            (5, "Nhiệt độ GPU"),
            (6, "Dung lượng đĩa dùng"),
            (7, "Tổng dung lượng đĩa"),
            (8, "Mạng gửi"),
            (9, "Mạng nhận"),
            (10, "Swap"),
            (11, "Xung CPU"),
            (12, "Nhiệt SoC"),
            (13, "Đọc đĩa"),
            (14, "Ghi đĩa"),
            (15, "Công suất CPU"),
            (16, "Công suất GPU"),
        ],
        ("rgb_effect", 0) => vec![
            (0, "Tắt"),
            (1, "Solid Color"),
            (2, "Breathing"),
            (3, "Cycle All"),
            (4, "Rainbow Beacon"),
            (5, "Raindrops"),
            (6, "Pixel Rain"),
            (7, "Typing Heatmap"),
            (8, "Digital Rain"),
            (9, "Reactive Simple"),
            (10, "Reactive Wide"),
            (11, "Reactive Nexus"),
            (12, "Splash"),
        ],
        ("audio_level", 0) => vec![(0, "Yên lặng"), (4, "Lớn")],
        _ => Vec::new(),
    };
    list.into_iter().map(|(v, s)| (v, s.into())).collect()
}
