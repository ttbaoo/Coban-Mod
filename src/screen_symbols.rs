//! Phosphor icons (MIT), the set EZP uses on Windows, bundled so macOS and Windows match.
use ab_glyph::{Font, FontRef, GlyphId, PxScale, point};
use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weight {
    Regular,
    Bold,
    #[default]
    Fill,
}
pub const CURATED: &[(&str, &str)] = &[
    ("Ngôi sao", "star"),
    ("Nhà", "house"),
    ("Phát", "play"),
    ("Tạm dừng", "pause"),
    ("Dừng", "stop"),
    ("Tua tới", "fast-forward"),
    ("Tua lùi", "rewind"),
    ("Bài tiếp", "skip-forward"),
    ("Bài trước", "skip-back"),
    ("Lặp lại", "repeat"),
    ("Trộn bài", "shuffle"),
    ("Âm lượng", "speaker-high"),
    ("Giảm âm lượng", "speaker-low"),
    ("Tắt tiếng", "speaker-slash"),
    ("Micro", "microphone"),
    ("Tắt micro", "microphone-slash"),
    ("Tai nghe", "headphones"),
    ("Nhạc", "music-note"),
    ("Phim", "film-strip"),
    ("Máy ảnh", "camera"),
    ("Ảnh", "image"),
    ("Ghi hình", "record"),
    ("Tệp", "file"),
    ("Thư mục", "folder"),
    ("Lưu", "floppy-disk"),
    ("Tải xuống", "download-simple"),
    ("Chia sẻ", "export"),
    ("Sao chép", "copy"),
    ("Dán", "clipboard-text"),
    ("Cắt", "scissors"),
    ("Hoàn tác", "arrow-u-up-left"),
    ("Làm lại", "arrow-u-up-right"),
    ("Tìm kiếm", "magnifying-glass"),
    ("Phóng to", "magnifying-glass-plus"),
    ("Thu nhỏ", "magnifying-glass-minus"),
    ("Trình duyệt", "globe"),
    ("Liên kết", "link"),
    ("Thư", "envelope"),
    ("Tin nhắn", "chat-circle"),
    ("Điện thoại", "phone"),
    ("Lịch", "calendar"),
    ("Đồng hồ", "clock"),
    ("Hẹn giờ", "timer"),
    ("Chuông", "bell"),
    ("Cài đặt", "gear"),
    ("Tia chớp", "lightning"),
    ("Trái tim", "heart"),
    ("Đánh dấu", "check"),
    ("Thêm", "plus"),
    ("Xóa", "trash"),
    ("Lên", "arrow-up"),
    ("Xuống", "arrow-down"),
    ("Trái", "arrow-left"),
    ("Phải", "arrow-right"),
    ("Bàn phím", "keyboard"),
    ("Màn hình", "monitor"),
    ("Máy tính", "laptop"),
    ("Trình chiếu", "presentation"),
    ("Chụp màn hình", "selection"),
    ("Mã nguồn", "code"),
    ("Dòng lệnh", "terminal-window"),
    ("Trò chơi", "game-controller"),
    ("Wi-Fi", "wifi-high"),
    ("Bluetooth", "bluetooth"),
    ("Pin", "battery-full"),
    ("Khóa", "lock"),
    ("Mở khóa", "lock-open"),
    ("Người dùng", "user"),
    ("Nguồn", "power"),
    ("Sáng", "sun"),
    ("Tối", "moon"),
    ("Bóng đèn", "lightbulb"),
    ("Ghim", "push-pin"),
    ("Cờ", "flag"),
    ("Bút", "pencil-simple"),
    ("Ghi chú", "note"),
    ("Dịch", "translate"),
    ("Đám mây", "cloud"),
    ("In", "printer"),
    ("Cà phê", "coffee"),
    ("Lửa", "fire"),
    ("YouTube", "youtube-logo"),
    ("Facebook", "facebook-logo"),
    ("Messenger", "messenger-logo"),
    ("Instagram", "instagram-logo"),
    ("TikTok", "tiktok-logo"),
    ("X", "x-logo"),
    ("Telegram", "telegram-logo"),
    ("WhatsApp", "whatsapp-logo"),
    ("Discord", "discord-logo"),
    ("Slack", "slack-logo"),
    ("Spotify", "spotify-logo"),
    ("Google", "google-logo"),
    ("Chrome", "google-chrome-logo"),
    ("ChatGPT", "open-ai-logo"),
    ("GitHub", "github-logo"),
    ("Figma", "figma-logo"),
    ("Word", "microsoft-word-logo"),
    ("Excel", "microsoft-excel-logo"),
    ("PowerPoint", "microsoft-powerpoint-logo"),
    ("Teams", "microsoft-teams-logo"),
    ("Apple", "apple-logo"),
    ("Windows", "windows-logo"),
];
// SF Symbols chosen before 0.6.2 whose name is not also a Phosphor name.
const LEGACY: &[(&str, &str)] = &[
    ("star.fill", "star"),
    ("house.fill", "house"),
    ("play.fill", "play"),
    ("pause.fill", "pause"),
    ("stop.fill", "stop"),
    ("forward.fill", "fast-forward"),
    ("backward.fill", "rewind"),
    ("speaker.wave.2.fill", "speaker-high"),
    ("speaker.slash.fill", "speaker-slash"),
    ("music.note", "music-note"),
    ("film.fill", "film-strip"),
    ("camera.fill", "camera"),
    ("photo.fill", "image"),
    ("doc.fill", "file"),
    ("folder.fill", "folder"),
    ("square.and.arrow.down", "download-simple"),
    ("square.and.arrow.up", "export"),
    ("doc.on.doc", "copy"),
    ("doc.on.clipboard", "clipboard-text"),
    ("arrow.uturn.backward", "arrow-u-up-left"),
    ("arrow.uturn.forward", "arrow-u-up-right"),
    ("magnifyingglass", "magnifying-glass"),
    ("envelope.fill", "envelope"),
    ("message.fill", "chat-circle"),
    ("phone.fill", "phone"),
    ("clock.fill", "clock"),
    ("gearshape.fill", "gear"),
    ("bolt.fill", "lightning"),
    ("heart.fill", "heart"),
    ("checkmark", "check"),
    ("trash.fill", "trash"),
    ("arrow.up", "arrow-up"),
    ("arrow.down", "arrow-down"),
    ("arrow.left", "arrow-left"),
    ("arrow.right", "arrow-right"),
];
pub fn codepoint(name: &str) -> Option<char> {
    let table = crate::phosphor_table::ICONS;
    let index = table.binary_search_by(|(n, _)| (*n).cmp(name)).ok()?;
    char::from_u32(table[index].1)
}
pub fn exists(name: &str) -> bool {
    codepoint(name).is_some()
}
/// Phosphor name and weight for a name saved by an older version (SF Symbols).
pub fn upgrade(name: &str) -> Option<(String, Weight)> {
    if exists(name) {
        return None;
    }
    let weight = if name.ends_with(".fill") {
        Weight::Fill
    } else {
        Weight::Bold
    };
    let mapped = LEGACY
        .iter()
        .find(|(sf, _)| *sf == name)
        .map(|(_, p)| (*p).to_string())
        .or_else(|| {
            let guess = name.trim_end_matches(".fill").replace('.', "-");
            exists(&guess).then_some(guess)
        })
        .unwrap_or_else(|| "star".into());
    Some((mapped, weight))
}
fn fold(text: &str) -> String {
    const FROM: &str = "àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ";
    const TO: &str = "aaaaaaaaaaaaaaaaaeeeeeeeeeeeiiiiiooooooooooooooooouuuuuuuuuuuyyyyyd";
    text.to_lowercase()
        .chars()
        .map(|c| {
            FROM.chars()
                .position(|f| f == c)
                .map_or(c, |i| TO.chars().nth(i).unwrap())
        })
        .collect()
}
/// Curated Vietnamese names first, then every Phosphor name containing the query.
pub fn search(query: &str, limit: usize) -> Vec<(String, &'static str)> {
    let q = fold(query.trim());
    if q.is_empty() {
        return CURATED.iter().map(|(l, n)| ((*l).into(), *n)).collect();
    }
    let slug = q.split_whitespace().collect::<Vec<_>>().join("-");
    let mut found: Vec<(String, &'static str)> = CURATED
        .iter()
        .filter(|(l, n)| fold(l).contains(&q) || n.contains(&slug))
        .map(|(l, n)| ((*l).into(), *n))
        .collect();
    for (name, _) in crate::phosphor_table::ICONS {
        if found.len() >= limit {
            break;
        }
        if name.contains(&slug) && !found.iter().any(|(_, n)| n == name) {
            found.push(((*name).into(), *name));
        }
    }
    found.truncate(limit);
    found
}
fn font(weight: Weight) -> &'static FontRef<'static> {
    static FONTS: OnceLock<[FontRef<'static>; 3]> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let load = |bytes: &'static [u8]| FontRef::try_from_slice(bytes).expect("Phosphor font");
        [
            load(include_bytes!("../assets/phosphor/Phosphor.ttf")),
            load(include_bytes!("../assets/phosphor/Phosphor-Bold.ttf")),
            load(include_bytes!("../assets/phosphor/Phosphor-Fill.ttf")),
        ]
    });
    &fonts[weight as usize]
}
fn glyph(name: &str, weight: Weight) -> Result<(&'static FontRef<'static>, GlyphId)> {
    let c = codepoint(name).with_context(|| format!("Không có biểu tượng Phosphor \"{name}\""))?;
    let font = font(weight);
    let id = font.glyph_id(c);
    anyhow::ensure!(id.0 != 0, "Font Phosphor thiếu biểu tượng \"{name}\"");
    Ok((font, id))
}
/// Draws the icon so its em square is `side` px, centred on (cx, cy).
pub fn draw(
    canvas: &mut RgbaImage,
    name: &str,
    weight: Weight,
    side: f32,
    (cx, cy): (f32, f32),
    ink: [u8; 3],
) -> Result<()> {
    let (font, id) = glyph(name, weight)?;
    let scale = PxScale::from(side * font.height_unscaled() / font.units_per_em().unwrap_or(1.));
    let Some(probe) = font.outline_glyph(id.with_scale_and_position(scale, point(0., 0.))) else {
        return Ok(());
    };
    let b = probe.px_bounds();
    let at = point(cx - (b.min.x + b.max.x) / 2., cy - (b.min.y + b.max.y) / 2.);
    let outline = font
        .outline_glyph(id.with_scale_and_position(scale, at))
        .context("Không dựng được biểu tượng")?;
    let bounds = outline.px_bounds();
    outline.draw(|gx, gy, coverage| {
        let x = bounds.min.x as i32 + gx as i32;
        let y = bounds.min.y as i32 + gy as i32;
        if x >= 0 && y >= 0 && (x as u32) < canvas.width() && (y as u32) < canvas.height() {
            image::Pixel::blend(
                canvas.get_pixel_mut(x as u32, y as u32),
                &Rgba([
                    ink[0],
                    ink[1],
                    ink[2],
                    (coverage.min(1.) * 255.).round() as u8,
                ]),
            );
        }
    });
    Ok(())
}
/// White icon on transparent for picker previews (the UI colourises it).
pub fn mask(name: &str, weight: Weight, size: u32) -> Result<RgbaImage> {
    let mut image = RgbaImage::new(size, size);
    let half = size as f32 / 2.;
    draw(
        &mut image,
        name,
        weight,
        size as f32 * 0.8,
        (half, half),
        [255, 255, 255],
    )?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curated_and_legacy_names_exist_in_every_weight() {
        for (_, name) in CURATED.iter().chain(LEGACY) {
            for weight in [Weight::Regular, Weight::Bold, Weight::Fill] {
                assert!(glyph(name, weight).is_ok(), "{name}");
            }
        }
        assert!(crate::phosphor_table::ICONS.is_sorted_by_key(|(n, _)| *n));
    }
    #[test]
    fn sf_names_upgrade_once_and_search_folds_vietnamese() {
        assert_eq!(upgrade("stop.fill"), Some(("stop".into(), Weight::Fill)));
        assert_eq!(upgrade("checkmark"), Some(("check".into(), Weight::Bold)));
        assert_eq!(
            upgrade("custom.symbol.fill"),
            Some(("star".into(), Weight::Fill))
        );
        assert_eq!(upgrade("plus"), None);
        assert_eq!(upgrade("stop"), None);
        assert_eq!(fold("ĐẶNG Ữ Ỵ"), "dang u y");
        assert_eq!(search("nhac", 10)[0].1, "music-note");
        assert!(
            search("youtube", 10)
                .iter()
                .any(|(_, n)| *n == "youtube-logo")
        );
        assert!(search("arrow", 60).len() == 60);
        let icon = mask("stop", Weight::Fill, 40).unwrap();
        assert_eq!(icon.get_pixel(20, 20)[3], 255);
        assert_eq!(icon.get_pixel(0, 0)[3], 0);
    }
}
