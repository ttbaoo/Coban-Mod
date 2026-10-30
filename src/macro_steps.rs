//! Friendly macro steps, compiled to the existing lossless VIA codec.
use crate::{keycodes, macros, presentation};
use anyhow::{Context, Result, ensure};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: i32,
    pub text: String,
}
/// Keep existing press/release pairs together when a user edits their key.
pub fn set_text(steps: &mut [Step], index: usize, text: String) -> Result<()> {
    let old = steps.get(index).context("Chọn bước macro")?.clone();
    if old.kind == 3 || old.kind == 4 {
        let code = parse_shortcut(&old.text)?;
        let new_code = parse_shortcut(&text)?;
        ensure!((2..=255).contains(&new_code), "Giữ / nhả chỉ dùng một phím");
        let other_kind = if old.kind == 3 { 4 } else { 3 };
        for (i, step) in steps.iter_mut().enumerate() {
            if i != index
                && step.kind == other_kind
                && parse_shortcut(&step.text).ok() == Some(code)
            {
                step.text = text.clone();
            }
        }
    }
    steps[index].text = text;
    Ok(())
}
pub fn remove(steps: &mut Vec<Step>, index: usize) -> Result<()> {
    let old = steps.get(index).context("Chọn bước macro")?.clone();
    if old.kind == 3 || old.kind == 4 {
        let code = parse_shortcut(&old.text)?;
        let other_kind = if old.kind == 3 { 4 } else { 3 };
        steps.retain(|s| !(s.kind == other_kind && parse_shortcut(&s.text).ok() == Some(code)));
        if let Some(i) = steps
            .iter()
            .position(|s| s.kind == old.kind && parse_shortcut(&s.text).ok() == Some(code))
        {
            steps.remove(i);
        }
    } else {
        steps.remove(index);
    }
    Ok(())
}
pub fn parse_shortcut(text: &str) -> Result<u16> {
    let normalized = text
        .replace('⌘', "CMD")
        .replace('⇧', "SHIFT")
        .replace('⌥', "ALT")
        .replace('⌃', "CTRL");
    let parts: Vec<_> = normalized
        .split('+')
        .map(|p| match p.trim().to_lowercase().as_str() {
            "enter" => "KC_ENT".into(),
            "esc" | "escape" => "KC_ESC".into(),
            "dấu cách" | "space" => "KC_SPC".into(),
            "lên" => "KC_UP".into(),
            "xuống" => "KC_DOWN".into(),
            "trái" => "KC_LEFT".into(),
            "phải" => "KC_RGHT".into(),
            "backspace" | "xóa lùi" => "KC_BSPC".into(),
            "delete" | "xóa" => "KC_DEL".into(),
            "print screen" => "KC_PSCR".into(),
            "scroll lock" => "KC_SLCK".into(),
            "pause" => "KC_PAUS".into(),
            "insert" => "KC_INS".into(),
            "caps lock" => "KC_CAPS".into(),
            "page up" => "KC_PGUP".into(),
            "page down" => "KC_PGDN".into(),
            "ctrl phải" => "KC_RCTL".into(),
            "shift phải" => "KC_RSFT".into(),
            "alt phải" => "KC_RALT".into(),
            "win phải" => "KC_RGUI".into(),
            "−" | "-" => "KC_MINS".into(),
            "=" => "KC_EQL".into(),
            "[" => "KC_LBRC".into(),
            "]" => "KC_RBRC".into(),
            "\\" => "KC_BSLS".into(),
            ";" => "KC_SCLN".into(),
            "'" => "KC_QUOT".into(),
            "`" => "KC_GRV".into(),
            "," => "KC_COMM".into(),
            "." => "KC_DOT".into(),
            "/" => "KC_SLSH".into(),
            _ => p.trim().to_uppercase(),
        })
        .collect();
    if parts.len() == 1 {
        let basic = match parts[0].as_str() {
            "CTRL" => "KC_LCTL",
            "SHIFT" => "KC_LSFT",
            "ALT" | "OPTION" => "KC_LALT",
            "CMD" | "WIN" | "META" => "KC_LGUI",
            _ => &parts[0],
        };
        return keycodes::parse(basic).or_else(|_| keycodes::parse(&basic.replace(' ', "_")));
    }
    keycodes::parse(&parts.join("+"))
        .or_else(|_| keycodes::parse(&parts.join("+").replace(' ', "_")))
}
pub fn decode(bytes: &[u8]) -> Result<Vec<Step>> {
    // Reject unsupported bytes before presenting an editable representation.
    macros::decode(bytes)?;
    let mut result = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 1 {
            literal.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if !literal.is_empty() {
            result.push(Step {
                kind: 0,
                text: std::mem::take(&mut literal),
            });
        }
        let op = bytes[i + 1];
        i += 2;
        if op == 4 {
            let start = i;
            while bytes[i] != b'|' {
                i += 1;
            }
            result.push(Step {
                kind: 2,
                text: String::from_utf8(bytes[start..i].to_vec())?,
            });
            i += 1;
        } else {
            result.push(Step {
                kind: match op {
                    2 => 3,
                    3 => 4,
                    _ => 1,
                },
                text: presentation::key_name(bytes[i] as u16),
            });
            i += 1;
        }
    }
    if !literal.is_empty() {
        result.push(Step {
            kind: 0,
            text: literal,
        });
    }
    Ok(result)
}
pub fn encode(steps: &[Step]) -> Result<String> {
    let mut text = String::new();
    for s in steps {
        match s.kind {
            0 => {
                for c in s.text.chars() {
                    if c == '{' || c == '\\' {
                        text.push('\\');
                    }
                    text.push(c);
                }
            }
            2 => {
                let delay: u32 = s.text.trim().parse()?;
                ensure!(delay <= 9999, "Khoảng chờ tối đa 9999 mili giây");
                text.push_str(&format!("{{{delay}}}"));
            }
            kind => {
                ensure!([1, 3, 4].contains(&kind), "Loại bước chưa hỗ trợ");
                let code = parse_shortcut(&s.text)?;
                if kind == 1 && (0x100..=0x0fff).contains(&code) {
                    let mut keys = Vec::new();
                    for (mask, k) in [
                        (1, "KC_LCTL"),
                        (2, "KC_LSFT"),
                        (4, "KC_LALT"),
                        (8, "KC_LGUI"),
                    ] {
                        if (code >> 8) & mask != 0 {
                            keys.push(k.to_string());
                        }
                    }
                    keys.push(keycodes::label(code & 255));
                    text.push_str(&format!("{{{}}}", keys.join(",")));
                } else {
                    ensure!((2..=255).contains(&code), "Chọn phím cơ bản cho bước macro");
                    text.push_str(&format!(
                        "{{{}{}}}",
                        if kind == 3 {
                            "+"
                        } else if kind == 4 {
                            "-"
                        } else {
                            ""
                        },
                        keycodes::label(code)
                    ));
                }
            }
        }
    }
    macros::encode(&text)?;
    Ok(text)
}
