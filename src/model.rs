use crate::protocol::setting_specs;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
fn is_false(v: &bool) -> bool {
    !*v
}
pub const VID: u16 = 0xcb3a;
pub const PID: u16 = 0xc10a;
pub const KEY_POSITIONS: [(u8, u8); 10] = [
    (0, 0),
    (0, 3),
    (1, 0),
    (1, 1),
    (1, 2),
    (1, 3),
    (2, 0),
    (2, 1),
    (2, 2),
    (2, 3),
];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: String,
    pub serial: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub value: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    #[serde(default, skip_serializing_if = "is_false")]
    pub simulated: bool,
    pub schema_version: u32,
    pub captured_at: String,
    pub device: Identity,
    pub protocol_version: u16,
    pub layers: u8,
    pub keymap: Vec<u16>,
    pub encoders: Vec<[u16; 4]>,
    pub macro_count: u8,
    pub macro_buffer: Vec<u8>,
    pub macro_names: Vec<String>,
    pub settings: Vec<Setting>,
    pub warnings: Vec<String>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Phiên bản backup chưa hỗ trợ");
        ensure!(
            self.device.vendor_id == VID && self.device.product_id == PID,
            "Backup không thuộc Coban Pad 10A"
        );
        ensure!((1..=32).contains(&self.layers), "Số layer không hợp lệ");
        ensure!(
            self.keymap.len() == self.layers as usize * 12,
            "Keymap sai kích thước"
        );
        ensure!(
            self.encoders.len() == self.layers as usize,
            "Encoder sai kích thước"
        );
        ensure!(
            self.macro_count <= 128 && self.macro_buffer.len() <= 65535,
            "Macro vượt giới hạn"
        );
        ensure!(
            self.macro_names.len() == self.macro_count as usize,
            "Sai số tên macro"
        );
        for n in &self.macro_names {
            ensure!(
                n.len() <= 6 && n.bytes().all(|b| b.is_ascii_graphic() || b == b' '),
                "Tên macro tối đa 6 ký tự ASCII"
            );
        }
        let specs = setting_specs();
        let mut seen = std::collections::HashSet::new();
        for s in &self.settings {
            ensure!(seen.insert(&s.key), "Trùng setting");
            let spec = specs
                .iter()
                .find(|x| x.key == s.key)
                .ok_or_else(|| anyhow::anyhow!("Setting chưa hỗ trợ: {}", s.key))?;
            ensure!(
                s.value.len() == spec.len,
                "Setting {} sai kích thước",
                s.key
            );
            spec.validate(&s.value)?;
        }
        Ok(())
    }
    pub fn compatible(&self, other: &Self) -> Result<()> {
        other.validate()?;
        ensure!(
            self.simulated == other.simulated,
            "Không thể áp dụng bản sao mô phỏng vào thiết bị thật"
        );
        ensure!(
            self.device.vendor_id == other.device.vendor_id
                && self.device.product_id == other.device.product_id,
            "Khác model"
        );
        ensure!(
            self.layers == other.layers
                && self.macro_count == other.macro_count
                && self.macro_buffer.len() == other.macro_buffer.len()
                && self.protocol_version == other.protocol_version,
            "Khác khả năng/phiên bản thiết bị"
        );
        ensure!(
            self.settings.len() == other.settings.len(),
            "Backup thiếu nhóm thiết lập; không áp dụng bản sao một phần"
        );
        for s in &other.settings {
            ensure!(
                self.settings.iter().any(|x| x.key == s.key),
                "Thiết bị chưa xác nhận hỗ trợ {}",
                s.key
            );
        }
        Ok(())
    }
    pub fn binding(&self, layer: usize, index: usize) -> u16 {
        if index < 10 {
            let (r, c) = KEY_POSITIONS[index];
            self.keymap[layer * 12 + r as usize * 4 + c as usize]
        } else {
            self.encoders[layer][index - 10]
        }
    }
    pub fn set_binding(&mut self, layer: usize, index: usize, value: u16) -> Result<()> {
        ensure!(
            layer < self.layers as usize && index < 14,
            "Vị trí phím không hợp lệ"
        );
        if index < 10 {
            let (r, c) = KEY_POSITIONS[index];
            self.keymap[layer * 12 + r as usize * 4 + c as usize] = value;
        } else {
            self.encoders[layer][index - 10] = value;
        }
        Ok(())
    }
    pub fn same_content(&self, other: &Self) -> bool {
        self.keymap == other.keymap
            && self.encoders == other.encoders
            && self.macro_buffer == other.macro_buffer
            && self.macro_names == other.macro_names
            && self.settings == other.settings
    }
    pub fn demo() -> Self {
        let mut keymap = vec![0; 48];
        for l in 0..4 {
            for (i, (r, c)) in KEY_POSITIONS.iter().enumerate() {
                keymap[l * 12 + *r as usize * 4 + *c as usize] = [
                    0x00ae, 0x5201, 0x7700, 0x29, 0x52, 0x28, 0x7701, 0x50, 0x51, 0x5223,
                ][i];
            }
        }
        Self {
            simulated: true,
            schema_version: 1,
            captured_at: chrono::Utc::now().to_rfc3339(),
            device: Identity {
                vendor_id: VID,
                product_id: PID,
                product: "Coban Pad 10A".into(),
                serial: None,
            },
            protocol_version: 12,
            layers: 4,
            keymap,
            encoders: vec![[0x00aa, 0x00a9, 0x0050, 0x004f]; 4],
            macro_count: 16,
            macro_buffer: vec![0; 1024],
            macro_names: vec![String::new(); 16],
            settings: setting_specs()
                .iter()
                .map(|s| Setting {
                    key: s.key.into(),
                    value: s.default.clone(),
                })
                .collect(),
            warnings: vec!["Dữ liệu minh họa — không gửi đến bàn phím".into()],
        }
    }
}
