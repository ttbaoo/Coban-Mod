//! Lossless VIA macro buffer editor. Unknown macros remain untouched as raw bytes.
use crate::keycodes;
use anyhow::{Result, bail, ensure};

pub fn split(buffer: &[u8], count: usize) -> Result<Vec<Vec<u8>>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < buffer.len() && result.len() < count {
        match buffer[i] {
            0 => {
                result.push(buffer[start..i].to_vec());
                start = i + 1;
            }
            1 => {
                ensure!(i + 1 < buffer.len(), "Macro bị cắt ở prefix");
                match buffer[i + 1] {
                    1..=3 => {
                        ensure!(
                            i + 2 < buffer.len() && buffer[i + 2] != 0,
                            "Macro phím bị cắt"
                        );
                        i += 2;
                    }
                    4 => {
                        i += 2;
                        while i < buffer.len() && buffer[i] != b'|' {
                            ensure!(buffer[i].is_ascii_digit(), "Delay macro không hợp lệ");
                            i += 1;
                        }
                        ensure!(i < buffer.len(), "Delay thiếu dấu kết thúc");
                    }
                    _ => bail!("Macro có opcode chưa hỗ trợ; giữ nguyên buffer khi sửa mục khác"),
                }
            }
            _ => {}
        }
        i += 1;
    }
    ensure!(result.len() == count, "Macro buffer thiếu dấu kết thúc");
    Ok(result)
}
pub fn decode(bytes: &[u8]) -> Result<String> {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            1 => {
                ensure!(i + 1 < bytes.len(), "Macro prefix thiếu dữ liệu");
                let action = bytes[i + 1];
                i += 2;
                match action {
                    1..=3 => {
                        ensure!(i < bytes.len(), "Thiếu keycode");
                        let prefix = match action {
                            2 => "+",
                            3 => "-",
                            _ => "",
                        };
                        out.push_str(&format!("{{{prefix}{}}}", keycodes::label(bytes[i] as u16)));
                    }
                    4 => {
                        let start = i;
                        while i < bytes.len() && bytes[i] != b'|' {
                            i += 1;
                        }
                        ensure!(i < bytes.len(), "Delay bị cắt");
                        out.push('{');
                        out.push_str(std::str::from_utf8(&bytes[start..i])?);
                        out.push('}');
                    }
                    _ => bail!("Opcode chưa hỗ trợ"),
                }
            }
            b'{' => out.push_str("\\{"),
            b'\\' => out.push_str("\\\\"),
            n if (2..=127).contains(&n) => out.push(n as char),
            _ => bail!("Macro không phải ASCII; giữ nguyên raw bytes"),
        }
        i += 1;
    }
    Ok(out)
}
pub fn encode(text: &str) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();
    let mut held = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let c = chars
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Escape chưa kết thúc"))?;
                ensure!(c == '{' || c == '\\', "Chỉ hỗ trợ \\{{ hoặc \\\\");
                result.push(c as u8);
            }
            '{' => {
                let mut block = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    block.push(c);
                }
                ensure!(
                    closed && !block.is_empty(),
                    "Khối macro thiếu dấu }} hoặc rỗng"
                );
                if block.chars().all(|c| c.is_ascii_digit()) {
                    let ms: u32 = block.parse()?;
                    ensure!(ms <= 9999, "Delay tối đa 9999 ms");
                    result.extend([1, 4]);
                    result.extend(ms.to_string().bytes());
                    result.push(b'|');
                } else {
                    let (kind, value) = if let Some(x) = block.strip_prefix('+') {
                        (2, x)
                    } else if let Some(x) = block.strip_prefix('-') {
                        (3, x)
                    } else {
                        (1, block.as_str())
                    };
                    let keys: Vec<u8> = value
                        .split(',')
                        .map(|x| {
                            let n = keycodes::parse(x)?;
                            ensure!((2..=255).contains(&n), "Macro chỉ dùng phím cơ bản HID");
                            Ok(n as u8)
                        })
                        .collect::<Result<_>>()?;
                    ensure!(kind == 1 || keys.len() == 1, "Nhấn/nhả chỉ nhận một phím");
                    if kind == 1 && keys.len() > 1 {
                        for k in &keys {
                            result.extend([1, 2, *k]);
                        }
                        for k in keys.iter().rev() {
                            result.extend([1, 3, *k]);
                        }
                    } else {
                        let k = keys[0];
                        if kind == 2 {
                            ensure!(!held.contains(&k), "Phím đang giữ");
                            held.push(k);
                        }
                        if kind == 3 {
                            ensure!(held.contains(&k), "Nhả phím chưa được giữ");
                            held.retain(|x| *x != k);
                        }
                        result.extend([1, kind, k]);
                    }
                }
            }
            c => {
                ensure!(
                    c.is_ascii() && c != '\0' && c != '\u{1}',
                    "Macro trên bàn phím chỉ hỗ trợ ASCII. Dùng macro máy tính cho tiếng Việt."
                );
                result.push(c as u8);
            }
        }
    }
    ensure!(held.is_empty(), "Macro còn phím chưa nhả; thêm {{-KC_...}}");
    Ok(result)
}
pub fn replace(buffer: &[u8], count: usize, index: usize, text: &str) -> Result<Vec<u8>> {
    ensure!(index < count, "Macro index vượt giới hạn");
    let mut items = split(buffer, count)?;
    items[index] = encode(text)?;
    let required: usize = items.iter().map(|x| x.len() + 1).sum();
    ensure!(
        required <= buffer.len(),
        "Macro cần {required} byte; thiết bị chỉ có {} byte",
        buffer.len()
    );
    let mut out = Vec::with_capacity(buffer.len());
    for item in items {
        out.extend(item);
        out.push(0);
    }
    out.resize(buffer.len(), 0);
    Ok(out)
}
