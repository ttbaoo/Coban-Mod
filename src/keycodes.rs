use anyhow::{Result, bail};
use std::{collections::BTreeMap, sync::OnceLock};

pub fn codes() -> &'static BTreeMap<String, u16> {
    static CODES: OnceLock<BTreeMap<String, u16>> = OnceLock::new();
    CODES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/keycodes.json")).expect("keycode definitions")
    })
}
pub fn parse(input: &str) -> Result<u16> {
    let s = input.trim().to_uppercase();
    if let Some(n) = s.strip_prefix("0X") {
        return Ok(u16::from_str_radix(n, 16)?);
    }
    if let Some(v) = codes().get(&s) {
        return Ok(*v);
    }
    if let Some(v) = codes().get(&format!("KC_{s}")) {
        return Ok(*v);
    }
    for (prefix, base, max) in [
        ("TO", 0x5200, 31),
        ("MO", 0x5220, 31),
        ("DF", 0x5240, 31),
        ("TG", 0x5260, 31),
        ("OSL", 0x5280, 31),
        ("TT", 0x52c0, 31),
        ("M", 0x7700, 127),
        ("CUSTOM", 0x7e00, 255),
    ] {
        if let Some(n) = s
            .strip_prefix(&format!("{prefix}("))
            .and_then(|x| x.strip_suffix(')'))
        {
            let n: u16 = n.parse()?;
            if n <= max {
                return Ok(base + n);
            }
            bail!("Chỉ số {prefix} vượt giới hạn");
        }
    }
    // Modifier combinations use QMK's 5-bit modifier encoding.
    if s.contains('+') {
        let mut mods = 0u16;
        let mut key = None;
        for part in s.split('+') {
            match part.trim() {
                "CTRL" => mods |= 1,
                "SHIFT" => mods |= 2,
                "ALT" | "OPTION" => mods |= 4,
                "CMD" | "WIN" | "META" => mods |= 8,
                other => {
                    if key.is_some() {
                        bail!("Tổ hợp chỉ có một phím chính");
                    }
                    key = Some(parse(other)?);
                }
            }
        }
        let k = key.ok_or_else(|| anyhow::anyhow!("Thiếu phím chính"))?;
        if k > 255 || mods == 0 {
            bail!("Tổ hợp không hợp lệ");
        }
        return Ok(mods << 8 | k);
    }
    bail!("Không nhận diện mã phím: {input}. Dùng KC_A, CTRL+C, M(0), TO(1) hoặc 0x1234")
}
pub fn label(code: u16) -> String {
    if (0x7700..=0x777f).contains(&code) {
        return format!("M({})", code - 0x7700);
    }
    for (name, base) in [
        ("TO", 0x5200),
        ("MO", 0x5220),
        ("DF", 0x5240),
        ("TG", 0x5260),
        ("OSL", 0x5280),
        ("TT", 0x52c0),
    ] {
        if (base..base + 32).contains(&code) {
            return format!("{name}({})", code - base);
        }
    }
    if (0x7e00..=0x7eff).contains(&code) {
        return format!("CUSTOM({})", code - 0x7e00);
    }
    if (0x100..=0x1fff).contains(&code) {
        let m = code >> 8;
        let mut p = Vec::new();
        if m & 16 != 0 {
            return format!("0x{code:04X}");
        }
        for (bit, name) in [(1, "CTRL"), (2, "SHIFT"), (4, "ALT"), (8, "CMD")] {
            if m & bit != 0 {
                p.push(name.to_string());
            }
        }
        p.push(label(code & 255));
        return p.join("+");
    }
    codes()
        .iter()
        .find(|(k, v)| **v == code && !k.starts_with('_'))
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| format!("0x{code:04X}"))
}
pub fn catalog(query: &str) -> Vec<String> {
    let q = query.to_uppercase();
    let mut all: Vec<_> = codes()
        .keys()
        .filter(|k| !k.starts_with('_'))
        .cloned()
        .collect();
    for n in 0..16 {
        all.push(format!("M({n})"));
    }
    for n in 0..4 {
        for p in ["TO", "MO", "DF", "TG", "OSL", "TT"] {
            all.push(format!("{p}({n})"));
        }
    }
    for n in 0..8 {
        all.push(format!("CUSTOM({n})"));
    }
    for p in ["CTRL+C", "CTRL+V", "CMD+C", "CMD+V", "CTRL+SHIFT+ESC"] {
        all.push(p.into());
    }
    all.retain(|x| x.contains(&q));
    all.sort();
    all
}

/// Catalog entries keep the wire code separate from the human-readable label.
#[derive(Clone, Debug)]
pub struct KeyChoice {
    pub code: String,
    pub title: String,
}

pub fn choices(query: &str, category: i32, layers: u8, macro_count: u8) -> Vec<KeyChoice> {
    let q = query.trim().to_lowercase();
    let mut all = catalog("");
    for n in 16..macro_count.min(128) {
        all.push(format!("M({n})"));
    }
    for n in 4..layers.min(32) {
        for prefix in ["TO", "MO", "DF", "TG", "OSL", "TT"] {
            all.push(format!("{prefix}({n})"));
        }
    }
    let mut entries: Vec<_> = all
        .into_iter()
        .filter(|code| {
            let Ok(value) = parse(code) else { return false };
            if validate_for_device(value, layers, macro_count).is_err() {
                return false;
            }
            let function = code
                .strip_prefix("KC_F")
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=24).contains(&n));
            match category {
                1 => function,
                2 => (value <= 0xa4 || (0xe0..=0xe7).contains(&value)) && !function,
                3 => (0xa5..=0xdf).contains(&value) || code.starts_with("AU_"),
                4 => code.contains('+'),
                5 => code.starts_with("M("),
                6 => ["TO(", "MO(", "DF(", "TG(", "OSL(", "TT("]
                    .iter()
                    .any(|p| code.starts_with(p)),
                7 => code.starts_with("CUSTOM("),
                _ => true,
            }
        })
        .map(|code| {
            let name = match code.as_str() {
                "KC_VOLU" => "Tăng âm lượng",
                "KC_VOLD" => "Giảm âm lượng",
                "KC_MUTE" => "Tắt / bật tiếng",
                "KC_MPLY" => "Phát / tạm dừng",
                "KC_MNXT" => "Bài tiếp theo",
                "KC_MPRV" => "Bài trước",
                "KC_MSTP" => "Dừng phát",
                "KC_ENT" => "Enter",
                "KC_ESC" => "Escape",
                "KC_BSPC" => "Backspace",
                "KC_DEL" => "Delete",
                "KC_SPC" => "Dấu cách",
                "KC_UP" => "Mũi tên lên",
                "KC_DOWN" => "Mũi tên xuống",
                "KC_LEFT" => "Mũi tên trái",
                "KC_RGHT" => "Mũi tên phải",
                "KC_NO" => "Không gán chức năng",
                "KC_TRNS" => "Dùng chức năng lớp dưới",
                "CMD+C" => "Sao chép · macOS",
                "CMD+V" => "Dán · macOS",
                "CTRL+C" => "Sao chép · Windows",
                "CTRL+V" => "Dán · Windows",
                _ => "",
            };
            let title = if let Some(n) = code.strip_prefix("KC_F")
                && n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
            {
                format!("F{n} · phím chức năng")
            } else if code.starts_with("M(") {
                format!("Macro {} · {code}", &code[2..code.len() - 1])
            } else if name.is_empty() {
                code.clone()
            } else {
                format!("{name} · {code}")
            };
            KeyChoice { code, title }
        })
        .filter(|entry| {
            entry.code.to_lowercase().contains(&q) || entry.title.to_lowercase().contains(&q)
        })
        .collect();
    if category == 1 {
        entries.sort_by_key(|entry| parse(&entry.code).unwrap_or(0));
    }
    entries
}

pub fn validate_for_device(code: u16, layers: u8, macro_count: u8) -> Result<()> {
    for base in [0x5200, 0x5220, 0x5240, 0x5260, 0x5280, 0x52c0] {
        if (base..base + 32).contains(&code) {
            anyhow::ensure!(
                code - base < layers as u16,
                "Thiết bị chỉ có {layers} lớp phím"
            );
        }
    }
    if (0x7700..=0x777f).contains(&code) {
        anyhow::ensure!(
            code - 0x7700 < macro_count as u16,
            "Thiết bị chỉ có {macro_count} macro"
        );
    }
    Ok(())
}
