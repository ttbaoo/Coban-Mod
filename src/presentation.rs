//! User-facing names; protocol identifiers stay inside the codec.
use crate::{keycodes, model::Snapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub code: u16,
    pub title: String,
    pub detail: String,
    pub icon: i32,
}
fn item(code: u16, title: impl Into<String>, detail: impl Into<String>, icon: i32) -> Function {
    Function {
        code,
        title: title.into(),
        detail: detail.into(),
        icon,
    }
}
pub fn key_name(code: u16) -> String {
    match code {
        4..=29 => ((b'A' + (code - 4) as u8) as char).to_string(),
        30..=38 => (code - 29).to_string(),
        39 => "0".into(),
        40 => "Enter".into(),
        41 => "Esc".into(),
        42 => "Backspace".into(),
        43 => "Tab".into(),
        44 => "Dấu cách".into(),
        45 => "−".into(),
        46 => "=".into(),
        47 => "[".into(),
        48 => "]".into(),
        49 => "\\".into(),
        51 => ";".into(),
        52 => "'".into(),
        53 => "`".into(),
        54 => ",".into(),
        55 => ".".into(),
        56 => "/".into(),
        57 => "Caps Lock".into(),
        58..=69 => format!("F{}", code - 57),
        70 => "Print Screen".into(),
        71 => "Scroll Lock".into(),
        72 => "Pause".into(),
        73 => "Insert".into(),
        74 => "Home".into(),
        75 => "Page Up".into(),
        76 => "Delete".into(),
        77 => "End".into(),
        78 => "Page Down".into(),
        79 => "Phải".into(),
        80 => "Trái".into(),
        81 => "Xuống".into(),
        82 => "Lên".into(),
        104..=115 => format!("F{}", code - 91),
        224 => "Ctrl".into(),
        225 => "Shift".into(),
        226 => "Alt".into(),
        227 => if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Win"
        }
        .into(),
        228 => "Ctrl phải".into(),
        229 => "Shift phải".into(),
        230 => "Alt phải".into(),
        231 => "Win phải".into(),
        _ => keycodes::label(code)
            .trim_start_matches("KC_")
            .replace('_', " "),
    }
}
pub fn describe(code: u16, snapshot: Option<&Snapshot>) -> Function {
    if (0x100..=0x1fff).contains(&code) {
        let mut parts = Vec::new();
        let mods = code >> 8;
        for (mask, label) in [
            (1, "Ctrl"),
            (2, "Shift"),
            (4, "Alt"),
            (
                8,
                if cfg!(target_os = "macos") {
                    "⌘"
                } else {
                    "Win"
                },
            ),
        ] {
            if mods & mask != 0 {
                parts.push(label.to_string());
            }
        }
        if mods & 16 != 0 {
            parts.push("Phải".into());
        }
        let base = code & 255;
        parts.push(key_name(base));
        let detail = parts.join(" + ");
        let standard_mods = mods == 1 || mods == 8;
        let (title, icon) = match (base, standard_mods, mods) {
            (6, true, _) => ("Sao chép", 1),
            (25, true, _) => ("Dán", 2),
            (27, true, _) => ("Cắt", 3),
            (29, true, _) => ("Hoàn tác", 4),
            (22, true, _) => ("Lưu tài liệu", 6),
            (9, true, _) => ("Tìm kiếm", 7),
            (4, true, _) => ("Chọn tất cả", 0),
            (28, true, _) | (29, _, 10) => ("Làm lại", 5),
            (33, _, 10) | (22, _, 10) => ("Chụp màn hình", 8),
            _ => ("Tổ hợp phím", 34),
        };
        return item(code, title, detail, icon);
    }
    for (base, prefix) in [
        (0x5200, "Chuyển lớp"),
        (0x5220, "Giữ để đổi lớp"),
        (0x5240, "Lớp mặc định"),
        (0x5260, "Bật / tắt lớp"),
        (0x5280, "Đổi lớp một lần"),
        (0x52c0, "Chạm để đổi lớp"),
    ] {
        if (base..base + 32).contains(&code) {
            return item(
                code,
                format!("{prefix} {}", code - base + 1),
                "Lớp phím",
                20,
            );
        }
    }
    if (0x7700..=0x777f).contains(&code) {
        let i = (code - 0x7700) as usize;
        let name = snapshot
            .and_then(|s| s.macro_names.get(i))
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Macro {}", i + 1));
        return item(code, name, "Chuỗi thao tác", 21);
    }
    if (0x7e00..=0x7e07).contains(&code) {
        let names = [
            "Màn hình kế tiếp",
            "Màn hình trước",
            "Mở đồng hồ",
            "Mở ảnh / GIF",
            "Hiện lớp phím",
            "Mở Pomodoro",
            "Bắt đầu tập trung",
            "Dừng Pomodoro",
        ];
        return item(
            code,
            names[(code - 0x7e00) as usize],
            "Màn hình bàn phím",
            22,
        );
    }
    let (title, detail, icon) = match code {
        0 => ("Không gán", "Để trống", 30),
        1 => ("Dùng lớp dưới", "Kế thừa chức năng", 20),
        40 => ("Enter", "Xuống dòng", 9),
        41 => ("Esc", "Thoát", 33),
        42 => ("Xóa lùi", "Backspace", 3),
        44 => ("Dấu cách", "Space", 0),
        76 => ("Xóa", "Delete", 3),
        79 => ("Phải", "Mũi tên →", 19),
        80 => ("Trái", "Mũi tên ←", 18),
        81 => ("Xuống", "Mũi tên ↓", 17),
        82 => ("Lên", "Mũi tên ↑", 16),
        0xa8 => ("Tắt / bật tiếng", "Âm thanh", 12),
        0xa9 => ("Tăng âm lượng", "Âm lượng +", 10),
        0xaa => ("Giảm âm lượng", "Âm lượng −", 11),
        0xab => ("Bài tiếp theo", "Nhạc & video", 14),
        0xac => ("Bài trước", "Nhạc & video", 15),
        0xad => ("Dừng phát", "Nhạc & video", 30),
        0xae => ("Phát / tạm dừng", "Nhạc & video", 13),
        0x7803 => ("Giảm đèn nền", "Độ sáng đèn nền", 11),
        0x7804 => ("Tăng đèn nền", "Độ sáng đèn nền", 10),
        0x7c00 => (
            "Vào chế độ nạp firmware",
            "Khởi động lại vào bootloader",
            20,
        ),
        0x7c77 | 0x7c78 => ("Lớp chức năng", "Chức năng gốc Coban", 20),
        _ => ("", "", 0),
    };
    if !title.is_empty() {
        return item(code, title, detail, icon);
    }
    let raw = keycodes::label(code);
    if raw.starts_with("KC_MS_WH_") {
        return item(
            code,
            if raw.ends_with("UP") {
                "Cuộn lên"
            } else if raw.ends_with("DOWN") {
                "Cuộn xuống"
            } else {
                "Cuộn ngang"
            },
            "Con lăn chuột",
            if raw.ends_with("UP") { 16 } else { 17 },
        );
    }
    if raw.starts_with("KC_") {
        return item(
            code,
            key_name(code),
            if matches!(code,58..=69|104..=115) {
                "Phím chức năng"
            } else {
                "Phím bàn phím"
            },
            0,
        );
    }
    let named = match raw.as_str() {
        "AU_ON" => "Bật loa",
        "AU_OFF" => "Tắt loa",
        "AU_TOG" => "Bật / tắt loa",
        "RGB_TOG" => "Bật / tắt đèn",
        "RM_TOGG" => "Bật / tắt đèn",
        "RGB_MOD" | "RM_NEXT" => "Hiệu ứng tiếp theo",
        _ => "Chức năng đặc biệt",
    };
    item(
        code,
        named,
        "Thiết lập nâng cao",
        if raw.starts_with("AU_") { 10 } else { 23 },
    )
}
/// 0 popular, 1 keyboard, 2 media, 3 user's macros/layers/Coban. Search spans all groups.
pub fn catalog(
    query: &str,
    category: i32,
    key_group: i32,
    snapshot: Option<&Snapshot>,
) -> Vec<Function> {
    catalog_named(query, category, key_group, snapshot, |f| f)
}
/// Apply local names before searching so both aliases and underlying codes are searchable.
pub fn catalog_named(
    query: &str,
    category: i32,
    key_group: i32,
    snapshot: Option<&Snapshot>,
    name: impl Fn(Function) -> Function,
) -> Vec<Function> {
    let modifier = if cfg!(target_os = "macos") { 8 } else { 1 };
    let popular = [
        modifier << 8 | 6,
        modifier << 8 | 25,
        modifier << 8 | 27,
        modifier << 8 | 29,
        if cfg!(target_os = "macos") {
            0x0a1d
        } else {
            0x011c
        },
        modifier << 8 | 22,
        modifier << 8 | 9,
        if cfg!(target_os = "macos") {
            0x0a21
        } else {
            0x0a16
        },
    ];
    let mut values: Vec<u16> = if !query.trim().is_empty() {
        keycodes::codes()
            .values()
            .copied()
            .filter(|n| *n < 256)
            .chain(popular)
            .collect()
    } else {
        match category {
            1 => match key_group {
                0 => (58..=69).chain(104..=115).collect(),
                1 => (4..=39).collect(),
                _ => [
                    40, 41, 42, 43, 44, 57, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 224, 225, 226,
                    227,
                ]
                .to_vec(),
            },
            2 => (0xa8..=0xae)
                .chain(
                    keycodes::codes()
                        .iter()
                        .filter(|(k, _)| k.starts_with("KC_MS_WH_"))
                        .map(|(_, v)| *v),
                )
                .collect(),
            3 => Vec::new(),
            _ => popular.to_vec(),
        }
    };
    if category == 3 || !query.trim().is_empty() {
        let (layers, macros) = snapshot.map_or((0, 0), |s| (s.layers, s.macro_count));
        values.extend((0..macros).map(|n| 0x7700 + n as u16));
        for base in [0x5200, 0x5220, 0x5260] {
            values.extend((0..layers).map(|n| base + n as u16));
        }
        values.extend(0x7e00..=0x7e07);
        values.extend([0, 1]);
    }
    let mut seen = std::collections::HashSet::new();
    values.retain(|n| seen.insert(*n));
    let q = query.to_lowercase();
    values
        .into_iter()
        .map(|v| describe(v, snapshot))
        .map(name)
        .filter(|f| {
            q.is_empty()
                || format!("{} {} {}", f.title, f.detail, keycodes::label(f.code))
                    .to_lowercase()
                    .contains(&q)
        })
        .collect()
}
