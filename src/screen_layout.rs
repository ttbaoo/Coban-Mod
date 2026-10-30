//! Local artwork rendering/storage. The UI sequences completed edits into agent HID uploads.
use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use anyhow::{Context, Result, ensure};
use image::{ImageReader, Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, path::Path, sync::OnceLock};

pub use crate::screen_symbols::Weight as IconWeight;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub png: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    Text,
    Image,
    Symbol,
    Empty,
    Clock,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IconSize {
    #[default]
    Large,
    Medium,
    Small,
}
impl IconSize {
    fn percent(self) -> u32 {
        match self {
            Self::Large => 100,
            Self::Medium => 75,
            Self::Small => 50,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockFace {
    #[default]
    Regular,
    Bold,
    Segment,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockLayout {
    #[default]
    TimeDate,
    Time,
    WeekdayDate,
    Inline,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateFormat {
    #[default]
    DayMonth,
    DayMonthYear,
    MonthDayYear,
    YearMonthDay,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clock {
    pub face: ClockFace,
    pub layout: ClockLayout,
    pub hour24: bool,
    pub ampm: bool,
    pub date: DateFormat,
}
impl Default for Clock {
    fn default() -> Self {
        Self {
            face: ClockFace::Regular,
            layout: ClockLayout::TimeDate,
            hour24: true,
            ampm: true,
            date: DateFormat::DayMonth,
        }
    }
}
const WEEKDAYS: [&str; 7] = [
    "Chủ nhật",
    "Thứ Hai",
    "Thứ Ba",
    "Thứ Tư",
    "Thứ Năm",
    "Thứ Sáu",
    "Thứ Bảy",
];
impl Clock {
    /// Time, optional AM/PM and the second line drawn for `now`.
    pub fn lines(
        &self,
        now: chrono::NaiveDateTime,
    ) -> (String, Option<&'static str>, Option<String>) {
        use chrono::{Datelike, Timelike};
        let time = if self.hour24 {
            format!("{:02}:{:02}", now.hour(), now.minute())
        } else {
            format!("{}:{:02}", now.hour12().1, now.minute())
        };
        let ampm = (!self.hour24 && self.ampm).then(|| if now.hour12().0 { "PM" } else { "AM" });
        let (d, m, y) = (now.day(), now.month(), now.year());
        let date = match self.date {
            DateFormat::DayMonth => format!("{d:02}/{m:02}"),
            DateFormat::DayMonthYear => format!("{d:02}/{m:02}/{y}"),
            DateFormat::MonthDayYear => format!("{m:02}/{d:02}/{y}"),
            DateFormat::YearMonthDay => format!("{y}-{m:02}-{d:02}"),
        };
        let second = match self.layout {
            ClockLayout::Time => None,
            ClockLayout::TimeDate | ClockLayout::Inline => Some(date),
            ClockLayout::WeekdayDate => Some(format!(
                "{} {date}",
                WEEKDAYS[now.weekday().num_days_from_sunday() as usize]
            )),
        };
        (time, ampm, second)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub kind: Content,
    pub text: String,
    pub image: Option<Asset>,
    /// Phosphor icon name; SF Symbol names from older layouts are upgraded on load.
    pub symbol: String,
    #[serde(default)]
    pub symbol_weight: IconWeight,
    pub text_size: u8,
    #[serde(default)]
    pub ink: Option<[u8; 3]>,
    #[serde(default)]
    pub icon_size: IconSize,
    #[serde(default)]
    pub tint_image: bool,
    #[serde(default)]
    pub clock: Clock,
}
impl Tile {
    fn with_text(text: impl Into<String>) -> Self {
        Self {
            kind: Content::Text,
            text: text.into(),
            image: None,
            symbol: "star".into(),
            symbol_weight: IconWeight::Fill,
            text_size: 12,
            ink: None,
            icon_size: IconSize::Large,
            tint_image: false,
            clock: Clock::default(),
        }
    }
}
fn default_knobs() -> Vec<Tile> {
    vec![Tile::with_text("L"), Tile::with_text("R")]
}
/// Layouts saved before the centre strip was editable keep an empty centre.
fn default_center() -> Tile {
    Tile {
        kind: Content::Empty,
        ..Tile::with_text("")
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub round: bool,
}
pub fn geometry(index: usize) -> Option<Geometry> {
    match index {
        0..=7 => Some(Geometry {
            x: (index as u32 % 4) * 40,
            y: 28 + (index as u32 / 4) * 26,
            width: 40,
            height: 26,
            round: false,
        }),
        8..=9 => Some(Geometry {
            x: if index == 8 { 8 } else { 128 },
            y: 1,
            width: 24,
            height: 24,
            round: true,
        }),
        10 => Some(Geometry {
            x: 36,
            y: 1,
            width: 88,
            height: 26,
            round: false,
        }),
        _ => None,
    }
}
pub fn label(index: usize) -> String {
    match index {
        8 => "Núm trái".into(),
        9 => "Núm phải".into(),
        10 => "Ô giữa".into(),
        _ => format!("Phím {}", index + 1),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub schema: u32,
    pub background: [u8; 3],
    pub ink: [u8; 3],
    pub use_background_image: bool,
    pub background_image: Option<Asset>,
    pub tile_background: bool,
    pub tiles: Vec<Tile>,
    #[serde(default = "default_knobs")]
    pub knobs: Vec<Tile>,
    #[serde(default = "default_center")]
    pub center: Tile,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            schema: 1,
            background: [16, 24, 39],
            ink: [255, 255, 255],
            use_background_image: false,
            background_image: None,
            tile_background: true,
            tiles: (1..=8).map(|n| Tile::with_text(n.to_string())).collect(),
            knobs: default_knobs(),
            center: default_center(),
        }
    }
}
pub fn parse_color(text: &str) -> Result<[u8; 3]> {
    let s = text.trim().trim_start_matches('#');
    ensure!(
        s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "Màu cần có dạng #RRGGBB, ví dụ #101827"
    );
    let bytes = hex::decode(s)?;
    Ok([bytes[0], bytes[1], bytes[2]])
}
pub fn color_text(color: [u8; 3]) -> String {
    format!("#{}", hex::encode_upper(color))
}
fn decode(bytes: &[u8], max_width: u32, max_height: u32) -> Result<RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_width);
    limits.max_image_height = Some(max_height);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?.to_rgba8())
}
pub fn png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}
pub fn import_asset(path: &Path, background: bool) -> Result<Asset> {
    ensure!(
        std::fs::metadata(path)?.len() <= 20_000_000,
        "Ảnh gốc tối đa 20 MB"
    );
    let image = decode(&std::fs::read(path)?, 4096, 4096)?;
    let mut image = if background {
        image::DynamicImage::ImageRgba8(image)
            .resize_to_fill(160, 80, imageops::FilterType::Lanczos3)
            .to_rgba8()
    } else {
        image::DynamicImage::ImageRgba8(image)
            .resize(128, 128, imageops::FilterType::Lanczos3)
            .to_rgba8()
    };
    // Keep enough detail for the Retina preview while respecting the stored-asset bound.
    let mut encoded = png(&image)?;
    while !background && encoded.len() > 24_000 {
        image = imageops::resize(
            &image,
            (image.width() * 3 / 4).max(1),
            (image.height() * 3 / 4).max(1),
            imageops::FilterType::Lanczos3,
        );
        encoded = png(&image)?;
    }
    Ok(Asset {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(100)
            .collect(),
        png: encoded,
    })
}
fn check_asset(asset: &Asset, background: bool) -> Result<()> {
    ensure!(
        asset.name.chars().count() <= 100
            && asset.png.len() <= if background { 60_000 } else { 24_000 },
        "Ảnh trong bố cục vượt giới hạn"
    );
    ensure!(
        asset.png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Ảnh bố cục phải là PNG"
    );
    let img = decode(
        &asset.png,
        if background { 160 } else { 128 },
        if background { 80 } else { 128 },
    )?;
    ensure!(img.width() > 0 && img.height() > 0, "Ảnh trống");
    Ok(())
}
impl Layout {
    pub fn tile(&self, index: usize) -> Result<&Tile> {
        match index {
            0..8 => self.tiles.get(index),
            10 => Some(&self.center),
            _ => self.knobs.get(index - 8),
        }
        .context("Chọn phím, núm xoay hoặc ô giữa")
    }
    pub fn tile_mut(&mut self, index: usize) -> Result<&mut Tile> {
        match index {
            0..8 => self.tiles.get_mut(index),
            10 => Some(&mut self.center),
            _ => self.knobs.get_mut(index - 8),
        }
        .context("Chọn phím, núm xoay hoặc ô giữa")
    }
    /// Keys, knobs, then the centre strip, in tile-index order (0–10).
    pub fn all_tiles(&self) -> impl Iterator<Item = &Tile> {
        self.tiles
            .iter()
            .chain(&self.knobs)
            .chain(std::iter::once(&self.center))
    }
    pub fn has_clock(&self) -> bool {
        self.all_tiles().any(|t| t.kind == Content::Clock)
    }
    /// Replaces SF Symbol names saved by older versions with Phosphor icons.
    pub fn upgrade_symbols(&mut self) -> bool {
        let mut changed = false;
        for tile in self
            .tiles
            .iter_mut()
            .chain(&mut self.knobs)
            .chain(std::iter::once(&mut self.center))
        {
            if let Some((name, weight)) = crate::screen_symbols::upgrade(&tile.symbol) {
                tile.symbol = name;
                tile.symbol_weight = weight;
                changed = true;
            }
        }
        changed
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.tiles.len() == 8 && self.knobs.len() == 2,
            "Bố cục phải có 8 ô phím và 2 núm xoay"
        );
        if let Some(asset) = &self.background_image {
            check_asset(asset, true)?;
        }
        ensure!(
            !self.use_background_image || self.background_image.is_some(),
            "Chọn ảnh nền trước"
        );
        for (index, tile) in self.all_tiles().enumerate() {
            ensure!(
                (6..=16).contains(&tile.text_size)
                    && tile.text.chars().count() <= 24
                    && !tile.text.chars().any(|c| c.is_control() && c != '\n'),
                "{}: văn bản tối đa 24 ký tự, cỡ chữ 6–16",
                label(index)
            );
            ensure!(
                tile.symbol.len() <= 100
                    && tile
                        .symbol
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "Tên biểu tượng không hợp lệ"
            );
            if let Some(asset) = &tile.image {
                check_asset(asset, false)?;
            }
            ensure!(
                tile.kind != Content::Symbol || crate::screen_symbols::exists(&tile.symbol),
                "Chọn biểu tượng cho {}",
                label(index).to_lowercase()
            );
        }
        Ok(())
    }
}
fn system_font(bold: bool) -> Result<&'static FontArc> {
    static FONTS: [OnceLock<Option<FontArc>>; 2] = [OnceLock::new(), OnceLock::new()];
    FONTS[bold as usize]
        .get_or_init(|| {
            let mut paths: Vec<std::path::PathBuf> = if bold {
                vec![
                    "/System/Library/Fonts/Supplemental/Arial Bold.ttf".into(),
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into(),
                ]
            } else {
                vec![
                    "/System/Library/Fonts/Supplemental/Arial.ttf".into(),
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
                ]
            };
            if let Some(windir) = std::env::var_os("WINDIR") {
                let file = if bold { "segoeuib.ttf" } else { "segoeui.ttf" };
                paths.insert(0, Path::new(&windir).join("Fonts").join(file));
            }
            paths
                .into_iter()
                .find_map(|p| FontArc::try_from_vec(std::fs::read(p).ok()?).ok())
        })
        .as_ref()
        .context("Không tìm được font hệ thống để vẽ văn bản")
}
fn font() -> Result<&'static FontArc> {
    system_font(false)
}
fn lines(text: &str, scaled: &ab_glyph::PxScaleFont<&FontArc>, width: f32) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut used = 0.;
        for c in paragraph.chars() {
            let advance = scaled.h_advance(scaled.glyph_id(c));
            if used + advance > width && !line.is_empty() {
                result.push(line);
                line = String::new();
                used = 0.;
            }
            line.push(c);
            used += advance;
        }
        result.push(line);
    }
    result
}
fn text(
    canvas: &mut RgbaImage,
    value: &str,
    size: u8,
    ink: [u8; 3],
    round: bool,
    scale: u32,
) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let font = font()?;
    ensure!(
        value.chars().all(|c| c == '\n' || font.glyph_id(c).0 != 0),
        "Văn bản có ký tự font hệ thống chưa hỗ trợ; dùng ảnh cho ký tự này"
    );
    let (w, h) = (canvas.width() / scale, canvas.height() / scale);
    let available_w = if round { 14. } else { (w - 6) as f32 };
    let available_h = if round { 14. } else { (h - 4) as f32 };
    let mut chosen = 4;
    for n in (4..=size).rev() {
        let scaled = font.as_scaled(PxScale::from(n as f32));
        if lines(value, &scaled, available_w).len() as f32 * (scaled.height() + 1.) <= available_h {
            chosen = n;
            break;
        }
    }
    let logical_font = font.as_scaled(PxScale::from(chosen as f32));
    let lines = lines(value, &logical_font, available_w);
    ensure!(
        lines.len() as f32 * (logical_font.height() + 1.) <= available_h,
        "Văn bản không vừa ô; rút gọn nội dung"
    );
    let factor = scale as f32;
    let scaled = font.as_scaled(PxScale::from(chosen as f32 * factor));
    let top = (h as f32 * factor - lines.len() as f32 * (scaled.height() + factor)) / 2.;
    for (row, line) in lines.iter().enumerate() {
        let width: f32 = line
            .chars()
            .map(|c| scaled.h_advance(scaled.glyph_id(c)))
            .sum();
        let mut cursor = (w as f32 * factor - width) / 2.;
        for c in line.chars() {
            let id = scaled.glyph_id(c);
            let glyph = id.with_scale_and_position(
                scaled.scale(),
                point(
                    cursor,
                    top + row as f32 * (scaled.height() + factor) + scaled.ascent(),
                ),
            );
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px >= (2 * scale) as i32
                        && px < ((w - 2) * scale) as i32
                        && py >= (2 * scale) as i32
                        && py < ((h - 2) * scale) as i32
                    {
                        image::Pixel::blend(
                            canvas.get_pixel_mut(px as u32, py as u32),
                            &Rgba([ink[0], ink[1], ink[2], (coverage * 255.) as u8]),
                        );
                    }
                });
            }
            cursor += scaled.h_advance(id);
        }
    }
    Ok(())
}
/// Width and vertical ink extent (relative to the baseline) of one line at `px`.
fn measure(font: &FontArc, value: &str, px: f32) -> (f32, f32, f32) {
    let scaled = font.as_scaled(PxScale::from(px));
    let (mut width, mut top, mut bottom) = (0., 0f32, 0f32);
    for c in value.chars() {
        let id = scaled.glyph_id(c);
        if let Some(outline) = font.outline_glyph(id.with_scale_and_position(px, point(width, 0.)))
        {
            let b = outline.px_bounds();
            top = top.min(b.min.y);
            bottom = bottom.max(b.max.y);
        }
        width += scaled.h_advance(id);
    }
    (width, top, bottom)
}
fn draw_line(
    canvas: &mut RgbaImage,
    font: &FontArc,
    value: &str,
    px: f32,
    left: f32,
    baseline: f32,
    ink: [u8; 3],
) {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut cursor = left;
    for c in value.chars() {
        let id = scaled.glyph_id(c);
        if let Some(outline) =
            font.outline_glyph(id.with_scale_and_position(px, point(cursor, baseline)))
        {
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
        }
        cursor += scaled.h_advance(id);
    }
}
const SEGMENTS: [u8; 10] = [
    0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
    0b1111111, 0b1101111,
];
/// Width of a seven-segment line `h` px tall; digits are h/2 wide, colons 3 strokes.
fn segment_width(value: &str, h: f32) -> f32 {
    let t = (h * 0.14).max(1.);
    value
        .chars()
        .map(|c| if c == ':' { 4. * t } else { h * 0.5 + t })
        .sum::<f32>()
        - t
}
fn draw_segments(canvas: &mut RgbaImage, value: &str, h: f32, left: f32, top: f32, ink: [u8; 3]) {
    let t = (h * 0.14).max(1.);
    let mut fill = |x: f32, y: f32, w: f32, hh: f32| {
        let (x0, y0) = (x.round().max(0.) as u32, y.round().max(0.) as u32);
        let (x1, y1) = ((x + w).round() as u32, (y + hh).round() as u32);
        for py in y0..y1.min(canvas.height()) {
            for px in x0..x1.min(canvas.width()) {
                canvas.put_pixel(px, py, Rgba([ink[0], ink[1], ink[2], 255]));
            }
        }
    };
    let mut x = left;
    for c in value.chars() {
        if c == ':' {
            fill(x + t, top + h * 0.3 - t / 2., t, t);
            fill(x + t, top + h * 0.7 - t / 2., t, t);
            x += 4. * t;
            continue;
        }
        let w = h * 0.5;
        let m = top + (h - t) / 2.;
        let mask = c.to_digit(10).map_or(0, |d| SEGMENTS[d as usize]);
        // Joined strokes stay legible at the LCD's 160×80 pixels.
        let parts = [
            (x, top, w, t),
            (x + w - t, top, t, m - top + t),
            (x + w - t, m, t, top + h - m),
            (x, top + h - t, w, t),
            (x, m, t, top + h - m),
            (x, top, t, m - top + t),
            (x, m, w, t),
        ];
        for (bit, (px, py, pw, ph)) in parts.into_iter().enumerate() {
            if mask & (1 << bit) != 0 {
                fill(px, py, pw, ph);
            }
        }
        x += w + t;
    }
}
/// Time (with optional AM/PM) fitted into the box whose centre is `center`.
fn draw_time(
    canvas: &mut RgbaImage,
    clock: &Clock,
    time: &str,
    ampm: Option<&str>,
    (max_w, max_h): (f32, f32),
    center: (f32, f32),
    ink: [u8; 3],
) -> Result<()> {
    let small = font()?;
    let mut h = max_h;
    loop {
        let suffix_px = h * 0.55;
        let suffix = ampm.map_or(0., |s| measure(small, s, suffix_px).0 + h * 0.12);
        let (width, top, bottom) = match clock.face {
            ClockFace::Segment => (segment_width(time, h), -h, 0.),
            face => {
                let f = system_font(face == ClockFace::Bold)?;
                // Size the font so digit height (not line height) fills the box.
                let px = h / 0.72;
                let (w, top, bottom) = measure(f, time, px);
                (w, top, bottom)
            }
        };
        if (width + suffix <= max_w && bottom - top <= max_h + 0.5) || h <= 4. {
            let left = center.0 - (width + suffix) / 2.;
            let baseline = center.1 - (top + bottom) / 2.;
            match clock.face {
                ClockFace::Segment => draw_segments(canvas, time, h, left, baseline - h, ink),
                face => draw_line(
                    canvas,
                    system_font(face == ClockFace::Bold)?,
                    time,
                    h / 0.72,
                    left,
                    baseline,
                    ink,
                ),
            }
            if let Some(s) = ampm {
                let (_, s_top, _) = measure(small, s, suffix_px);
                draw_line(
                    canvas,
                    small,
                    s,
                    suffix_px,
                    left + width + h * 0.12,
                    baseline + top - s_top,
                    ink,
                );
            }
            return Ok(());
        }
        h -= 0.5;
    }
}
fn draw_fitted(
    canvas: &mut RgbaImage,
    value: &str,
    (max_w, max_h): (f32, f32),
    center: (f32, f32),
    ink: [u8; 3],
) -> Result<()> {
    let f = font()?;
    let mut px = max_h / 0.72;
    loop {
        let (width, top, bottom) = measure(f, value, px);
        if (width <= max_w && bottom - top <= max_h + 0.5) || px <= 4. {
            draw_line(
                canvas,
                f,
                value,
                px,
                center.0 - width / 2.,
                center.1 - (top + bottom) / 2.,
                ink,
            );
            return Ok(());
        }
        px -= 0.5;
    }
}
fn render_clock(
    cell: &mut RgbaImage,
    clock: &Clock,
    now: chrono::NaiveDateTime,
    round: bool,
    scale: u32,
    ink: [u8; 3],
) -> Result<()> {
    let s = scale as f32;
    let (w, h) = (cell.width() as f32, cell.height() as f32);
    let (bw, bh) = if round {
        (14. * s, 14. * s)
    } else {
        (w - 6. * s, h - 6. * s)
    };
    let (time, ampm, second) = clock.lines(now);
    match (clock.layout, second) {
        (ClockLayout::Inline, Some(date)) => {
            // Time takes the space the date does not need on one line.
            let date_w = bw * 0.48;
            draw_time(
                cell,
                clock,
                &time,
                ampm,
                (bw - date_w - 2. * s, bh * 0.8),
                (w / 2. - date_w / 2. - s, h / 2.),
                ink,
            )?;
            draw_fitted(
                cell,
                &date,
                (date_w, bh * 0.45),
                (w / 2. + bw / 2. - date_w / 2., h / 2.),
                ink,
            )?;
        }
        (_, Some(date)) => {
            let top = (h - bh) / 2.;
            draw_time(
                cell,
                clock,
                &time,
                ampm,
                (bw, bh * 0.58),
                (w / 2., top + bh * 0.29),
                ink,
            )?;
            draw_fitted(cell, &date, (bw, bh * 0.3), (w / 2., top + bh * 0.82), ink)?;
        }
        (_, None) => draw_time(
            cell,
            clock,
            &time,
            ampm,
            (bw, bh * 0.8),
            (w / 2., h / 2.),
            ink,
        )?,
    }
    Ok(())
}
#[cfg(test)]
fn render_tile(tile: &Tile, shape: Geometry, layout: &Layout, scale: u32) -> Result<RgbaImage> {
    render_tile_at(
        tile,
        shape,
        layout,
        scale,
        chrono::Local::now().naive_local(),
    )
}
fn render_tile_at(
    tile: &Tile,
    shape: Geometry,
    layout: &Layout,
    scale: u32,
    now: chrono::NaiveDateTime,
) -> Result<RgbaImage> {
    let (w, h) = (shape.width * scale, shape.height * scale);
    let ink = tile.ink.unwrap_or(layout.ink);
    let mut cell = RgbaImage::new(w, h);
    if layout.tile_background {
        for py in 2 * scale..h - 2 * scale {
            for px in 2 * scale..w - 2 * scale {
                cell.put_pixel(
                    px,
                    py,
                    Rgba([
                        layout.background[0],
                        layout.background[1],
                        layout.background[2],
                        180,
                    ]),
                );
            }
        }
    }
    match tile.kind {
        Content::Text => text(
            &mut cell,
            &tile.text,
            tile.text_size,
            ink,
            shape.round,
            scale,
        )?,
        Content::Symbol => {
            let side = ((shape.width - 6).min(shape.height - 6) * tile.icon_size.percent()) as f32
                / 100.
                * scale as f32;
            crate::screen_symbols::draw(
                &mut cell,
                &tile.symbol,
                tile.symbol_weight,
                side,
                (w as f32 / 2., h as f32 / 2.),
                ink,
            )?;
        }
        Content::Clock => render_clock(&mut cell, &tile.clock, now, shape.round, scale, ink)?,
        Content::Image => {
            if let Some(asset) = &tile.image {
                let img = decode(&asset.png, 128, 128)?;
                let mut img = image::DynamicImage::ImageRgba8(img)
                    .resize(
                        ((shape.width - 6) * tile.icon_size.percent() / 100).max(1) * scale,
                        ((shape.height - 6) * tile.icon_size.percent() / 100).max(1) * scale,
                        imageops::FilterType::Lanczos3,
                    )
                    .to_rgba8();
                if tile.tint_image {
                    for p in img.pixels_mut() {
                        p[0] = ink[0];
                        p[1] = ink[1];
                        p[2] = ink[2];
                    }
                }
                let (iw, ih) = img.dimensions();
                imageops::overlay(
                    &mut cell,
                    &img,
                    ((w - iw) / 2) as i64,
                    ((h - ih) / 2) as i64,
                );
            }
        }
        Content::Empty => (),
    }
    if shape.round {
        let center = w as f32 / 2.;
        let radius = center - 0.75 * scale as f32;
        // Keep the knob outline visible when the user's text and solid background match.
        let edge_ink = if !layout.use_background_image
            && ink
                .iter()
                .zip(layout.background)
                .map(|(a, b)| (*a as i16 - b as i16).unsigned_abs() as u32)
                .sum::<u32>()
                < 96
        {
            if layout.background.iter().map(|c| *c as u32).sum::<u32>() > 384 {
                [32, 32, 32]
            } else {
                [224, 224, 224]
            }
        } else {
            ink
        };
        for (x, y, p) in cell.enumerate_pixels_mut() {
            let distance =
                ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0., 1.);
            p[3] = (p[3] as f32 * coverage).round() as u8;
            let edge = (1. - (distance - (radius - 0.5 * scale as f32)).abs() / scale as f32)
                .clamp(0., 1.)
                * coverage;
            image::Pixel::blend(
                p,
                &Rgba([edge_ink[0], edge_ink[1], edge_ink[2], (edge * 190.) as u8]),
            );
        }
    }
    Ok(cell)
}
pub struct Rendered {
    pub png: Vec<u8>,
    pub gif: Vec<u8>,
    pub preview_png: Vec<u8>,
}
fn canvas(layout: &Layout, scale: u32, now: chrono::NaiveDateTime) -> Result<RgbaImage> {
    let [r, g, b] = layout.background;
    let mut canvas = RgbaImage::from_pixel(160 * scale, 80 * scale, Rgba([r, g, b, 255]));
    if layout.use_background_image {
        let img = decode(
            &layout
                .background_image
                .as_ref()
                .context("Chọn ảnh nền")?
                .png,
            160,
            80,
        )?;
        imageops::overlay(
            &mut canvas,
            &image::DynamicImage::ImageRgba8(img)
                .resize_to_fill(160 * scale, 80 * scale, imageops::FilterType::Lanczos3)
                .to_rgba8(),
            0,
            0,
        );
    }
    for (i, tile) in layout.all_tiles().enumerate() {
        // An empty centre strip (the default) leaves the background untouched.
        if i == 10 && tile.kind == Content::Empty {
            continue;
        }
        let shape = geometry(i).context("Vị trí phím không hợp lệ")?;
        let cell = render_tile_at(tile, shape, layout, scale, now)?;
        imageops::overlay(
            &mut canvas,
            &cell,
            (shape.x * scale) as i64,
            (shape.y * scale) as i64,
        );
    }
    Ok(canvas)
}
pub fn render(layout: &Layout) -> Result<Rendered> {
    render_at(layout, chrono::Local::now().naive_local())
}
/// Clock tiles show `now` (local time, to the minute).
pub fn render_at(layout: &Layout, now: chrono::NaiveDateTime) -> Result<Rendered> {
    layout.validate()?;
    let gif = crate::media::encode_frames(&[(canvas(layout, 1, now)?, 100)])?;
    // Retain exact device pixels alongside a separately rendered Retina editing preview.
    Ok(Rendered {
        png: png(&crate::media::preview(&gif)?)?,
        gif,
        preview_png: png(&canvas(layout, 4, now)?)?,
    })
}
#[derive(Serialize, Deserialize)]
pub struct Store {
    pub revision: u64,
    pub layout: Layout,
}
impl Store {
    /// Each layer starts with the legacy artwork, without overwriting or migrating
    /// that file. The first edit creates only that layer's private copy.
    pub fn load_layer(layer: u8) -> Result<Self> {
        Self::load_layer_at(&crate::storage::data_dir(), layer)
    }
    pub fn save_layer(&self, layer: u8) -> Result<()> {
        self.save_layer_at(&crate::storage::data_dir(), layer)
    }
    pub(crate) fn load_layer_at(dir: &Path, layer: u8) -> Result<Self> {
        ensure!(layer < 32, "Lớp ảnh không hợp lệ");
        let own = dir.join(format!("screen-layout-layer-{}.json", layer + 1));
        let path = if own.exists() {
            own
        } else {
            dir.join("screen-layout.json")
        };
        if !path.exists() {
            return Ok(Self {
                revision: 0,
                layout: Layout::default(),
            });
        }
        ensure!(
            std::fs::metadata(&path)?.len() <= 1_500_000,
            "Bố cục lưu vượt giới hạn"
        );
        let mut stored: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        stored.layout.upgrade_symbols();
        stored.layout.validate()?;
        Ok(stored)
    }
    pub(crate) fn save_layer_at(&self, dir: &Path, layer: u8) -> Result<()> {
        ensure!(layer < 32, "Lớp ảnh không hợp lệ");
        self.layout.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= 1_500_000, "Bố cục lưu vượt giới hạn");
        crate::storage::write_private(
            &dir.join(format!("screen-layout-layer-{}.json", layer + 1)),
            &bytes,
        )
    }
    pub fn load() -> Result<Self> {
        let path = crate::storage::data_dir().join("screen-layout.json");
        if !path.exists() {
            return Ok(Self {
                revision: 0,
                layout: Layout::default(),
            });
        }
        ensure!(
            std::fs::metadata(&path)?.len() <= 1_500_000,
            "Bố cục lưu vượt giới hạn"
        );
        let mut stored: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        stored.layout.upgrade_symbols();
        stored.layout.validate()?;
        Ok(stored)
    }
    pub fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= 1_500_000, "Bố cục lưu vượt giới hạn");
        let path = crate::storage::data_dir().join("screen-layout.json");
        let backup = path.with_file_name("screen-layout.pre-knobs.json");
        if path.exists() && !backup.exists() {
            let previous = std::fs::read(&path)?;
            let value: serde_json::Value = serde_json::from_slice(&previous)?;
            if value.get("layout").and_then(|l| l.get("knobs")).is_none() {
                crate::storage::write_private(&backup, &previous)?;
            }
        }
        crate::storage::write_private(&path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layer_artwork_keeps_legacy_bytes_and_other_layers_and_revisions() {
        let dir = std::env::temp_dir().join(format!(
            "coban-layer-art-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let original = Store {
            revision: 7,
            layout: Layout::default(),
        };
        let legacy = serde_json::to_vec(&original).unwrap();
        std::fs::write(dir.join("screen-layout.json"), &legacy).unwrap();
        let mut second = Store::load_layer_at(&dir, 1).unwrap();
        second.layout.tiles[0].text = "Lớp hai".into();
        second.revision += 1;
        second.save_layer_at(&dir, 1).unwrap();
        assert_eq!(
            Store::load_layer_at(&dir, 1).unwrap().layout.tiles[0].text,
            "Lớp hai"
        );
        for layer in [0, 2, 3] {
            let untouched = Store::load_layer_at(&dir, layer).unwrap();
            assert_eq!(untouched.revision, 7);
            assert_eq!(untouched.layout, original.layout);
        }
        assert_eq!(
            std::fs::read(dir.join("screen-layout.json")).unwrap(),
            legacy
        );
        assert!(Store::load_layer_at(&dir, 32).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn old_layouts_keep_their_colour_and_icon_size_and_sharp_preview_keeps_device_pixels() {
        let mut json = serde_json::to_value(Layout::default()).unwrap();
        for name in ["tiles", "knobs"] {
            for tile in json[name].as_array_mut().unwrap() {
                tile.as_object_mut().unwrap().remove("ink");
                tile.as_object_mut().unwrap().remove("icon_size");
                tile.as_object_mut().unwrap().remove("tint_image");
            }
        }
        let old: Layout = serde_json::from_value(json).unwrap();
        assert!(
            old.tiles
                .iter()
                .chain(&old.knobs)
                .all(|t| t.ink.is_none() && t.icon_size == IconSize::Large && !t.tint_image)
        );
        let rendered = render(&old).unwrap();
        assert_eq!(
            decode(&rendered.preview_png, 640, 320)
                .unwrap()
                .dimensions(),
            (640, 320)
        );
        assert_eq!(
            decode(&rendered.png, 160, 80).unwrap(),
            crate::media::preview(&rendered.gif).unwrap()
        );
    }
    #[test]
    fn each_tile_has_independent_ink_and_three_centred_icon_sizes() {
        let mut cfg = Layout {
            background: [0, 0, 0],
            tile_background: false,
            ..Layout::default()
        };
        cfg.tiles[0].ink = Some([255, 0, 0]);
        cfg.knobs[0].ink = Some([0, 255, 0]);
        cfg.tiles[1].kind = Content::Symbol;
        cfg.tiles[1].symbol = "stop".into();
        cfg.tiles[1].ink = Some([0, 0, 255]);
        let img = canvas(&cfg, 1, chrono::NaiveDateTime::default()).unwrap();
        assert!(img.enumerate_pixels().any(|(x, y, p)| x < 40
            && y >= 28
            && p[0] > 50
            && p[1] == 0
            && p[2] == 0));
        assert_eq!(img.get_pixel(60, 41).0, [0, 0, 255, 255]);
        assert!(img.enumerate_pixels().any(|(x, y, p)| x < 40
            && y < 25
            && p[1] > 50
            && p[0] == 0
            && p[2] == 0));
        for index in [1, 8] {
            let shape = geometry(index).unwrap();
            let tile = cfg.tile_mut(index).unwrap();
            tile.kind = Content::Symbol;
            tile.symbol = "stop".into();
            tile.ink = Some([0, 0, 255]);
            let mut areas = Vec::new();
            for size in [IconSize::Large, IconSize::Medium, IconSize::Small] {
                cfg.tile_mut(index).unwrap().icon_size = size;
                let cell = render_tile(cfg.tile(index).unwrap(), shape, &cfg, 1).unwrap();
                areas.push(
                    cell.pixels()
                        .filter(|p| p[2] > 240 && p[0] == 0 && p[1] == 0)
                        .count(),
                );
                assert_eq!(
                    cell.get_pixel(shape.width / 2, shape.height / 2).0,
                    [0, 0, 255, 255]
                );
            }
            assert!(areas[0] > areas[1] && areas[1] > areas[2]);
        }
        let reloaded: Layout = serde_json::from_slice(&serde_json::to_vec(&cfg).unwrap()).unwrap();
        assert_eq!(reloaded, cfg);
    }
    #[test]
    fn imported_icon_tint_is_optional_and_preserves_transparency() {
        let mut cfg = Layout {
            tile_background: false,
            ..Layout::default()
        };
        let tile = &mut cfg.tiles[0];
        tile.kind = Content::Image;
        tile.ink = Some([255, 0, 0]);
        tile.image = Some(Asset {
            name: "transparent.png".into(),
            png: png(&RgbaImage::from_pixel(20, 20, Rgba([0, 255, 0, 128]))).unwrap(),
        });
        let shape = geometry(0).unwrap();
        let original = render_tile(&cfg.tiles[0], shape, &cfg, 1).unwrap();
        cfg.tiles[0].tint_image = true;
        let tinted = render_tile(&cfg.tiles[0], shape, &cfg, 1).unwrap();
        assert_eq!(original.get_pixel(20, 13).0, [0, 255, 0, 128]);
        assert_eq!(tinted.get_pixel(20, 13).0, [255, 0, 0, 128]);
        assert_eq!(tinted.get_pixel(0, 0)[3], 0);
    }
    #[test]
    fn each_tile_has_its_own_image_and_preview_matches_transmitted_gif() {
        let mut cfg = Layout {
            tile_background: false,
            ..Layout::default()
        };
        let colors = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [0, 255, 255],
            [255, 0, 255],
            [200, 100, 50],
            [250, 250, 250],
        ];
        for (i, tile) in cfg.tiles.iter_mut().enumerate() {
            let [r, g, b] = colors[i];
            tile.kind = Content::Image;
            tile.image = Some(Asset {
                name: format!("{i}.png"),
                png: png(&RgbaImage::from_pixel(20, 30, Rgba([r, g, b, 255]))).unwrap(),
            });
        }
        let rendered = render(&cfg).unwrap();
        let preview = decode(&rendered.png, 160, 80).unwrap();
        assert_eq!(preview, crate::media::preview(&rendered.gif).unwrap());
        assert!(rendered.gif.len() < crate::media::MAX_BYTES);
        for (i, color) in colors.iter().enumerate() {
            let center = preview.get_pixel((i as u32 % 4) * 40 + 20, 41 + (i as u32 / 4) * 26);
            for channel in 0..3 {
                assert!((center[channel] as i16 - color[channel] as i16).abs() < 20);
            }
        }
    }
    #[test]
    fn vietnamese_text_renders_and_malformed_assets_are_rejected() {
        let mut cfg = Layout::default();
        cfg.tiles[0].text = "Lời\ngiải".into();
        assert_eq!(
            crate::media::preview(&render(&cfg).unwrap().gif)
                .unwrap()
                .dimensions(),
            (160, 80)
        );
        cfg.tiles[0].text = "too long text to fit this key".into();
        assert!(render(&cfg).is_err());
        cfg.tiles[0].text = "Text".into();
        cfg.tiles[0].image = Some(Asset {
            name: "bad".into(),
            png: vec![0; 30],
        });
        assert!(render(&cfg).is_err());
        assert!(parse_color("#FFF").is_err());
        assert_eq!(parse_color(" #12abEF ").unwrap(), [18, 171, 239]);
    }
    #[test]
    fn legacy_layout_keeps_all_eight_keys_and_adds_independent_knobs() {
        let mut old = Layout::default();
        old.tiles[0].text = "Lời giải".into();
        let mut json = serde_json::to_value(&old).unwrap();
        json.as_object_mut().unwrap().remove("knobs");
        let mut migrated: Layout = serde_json::from_value(json).unwrap();
        assert_eq!(migrated.tiles, old.tiles);
        assert_eq!(migrated.knobs.len(), 2);
        migrated.tile_mut(8).unwrap().text = "VOL".into();
        assert_eq!(migrated.tile(9).unwrap().text, "R");
        assert_eq!(migrated.tiles, old.tiles);
        assert!(render(&migrated).is_ok());
        assert!(migrated.tile(11).is_err());
    }
    #[test]
    fn knob_artwork_is_round_and_does_not_cover_the_keys() {
        let mut cfg = Layout {
            background: [0, 0, 0],
            tile_background: false,
            ..Layout::default()
        };
        cfg.tiles.iter_mut().for_each(|t| t.kind = Content::Empty);
        for (i, color) in [[255, 0, 0, 255], [0, 0, 255, 255]].into_iter().enumerate() {
            cfg.knobs[i].kind = Content::Image;
            cfg.knobs[i].image = Some(Asset {
                name: "knob.png".into(),
                png: png(&RgbaImage::from_pixel(24, 24, Rgba(color))).unwrap(),
            });
        }
        let image = crate::media::preview(&render(&cfg).unwrap().gif).unwrap();
        assert!(image.get_pixel(20, 13)[0] > 230);
        assert!(image.get_pixel(140, 13)[2] > 230);
        // Circle corners and the first key's centre keep the background colour.
        for (x, y) in [(8, 1), (31, 1), (128, 24), (151, 24), (20, 41)] {
            assert!(image.get_pixel(x, y).0[..3].iter().all(|c| *c < 20));
        }
        assert!(image.get_pixel(20, 2)[0] > 80); // visible circular outline
        cfg.background = [255, 255, 255];
        cfg.ink = [255, 255, 255];
        let white = crate::media::preview(&render(&cfg).unwrap().gif).unwrap();
        assert!(white.get_pixel(20, 2)[0] < 200); // outline also visible with white-on-white user settings
    }
    fn at(h: u32, m: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 28)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }
    #[test]
    fn clock_text_follows_hour_cycle_and_date_format() {
        let mut clock = Clock::default();
        assert_eq!(
            clock.lines(at(21, 5)),
            ("21:05".into(), None, Some("28/10".into()))
        );
        clock.hour24 = false;
        clock.layout = ClockLayout::WeekdayDate;
        clock.date = DateFormat::DayMonthYear;
        assert_eq!(
            clock.lines(at(21, 5)),
            ("9:05".into(), Some("PM"), Some("Thứ Tư 28/10/2026".into()))
        );
        clock.ampm = false;
        clock.layout = ClockLayout::Time;
        assert_eq!(clock.lines(at(0, 30)), ("12:30".into(), None, None));
        clock.layout = ClockLayout::Inline;
        clock.date = DateFormat::YearMonthDay;
        assert_eq!(clock.lines(at(8, 0)).2.unwrap(), "2026-10-28");
        clock.date = DateFormat::MonthDayYear;
        assert_eq!(clock.lines(at(8, 0)).2.unwrap(), "10/28/2026");
    }
    #[test]
    fn centre_strip_is_a_tile_and_clock_redraws_each_minute() {
        let mut cfg = Layout {
            background: [0, 0, 0],
            tile_background: false,
            ..Layout::default()
        };
        let blank = canvas(&cfg, 1, at(10, 41)).unwrap();
        cfg.tiles.iter_mut().for_each(|t| t.kind = Content::Empty);
        cfg.knobs.iter_mut().for_each(|t| t.kind = Content::Empty);
        // Old layouts have no centre and render the same as an empty centre.
        let mut json = serde_json::to_value(&cfg).unwrap();
        json.as_object_mut().unwrap().remove("center");
        let old: Layout = serde_json::from_value(json).unwrap();
        assert_eq!(old.center.kind, Content::Empty);
        assert!(!old.has_clock());
        let empty = canvas(&cfg, 1, at(10, 41)).unwrap();
        assert!(
            empty
                .enumerate_pixels()
                .all(|(x, y, p)| !(36..124).contains(&x) || y > 26 || p.0 == [0, 0, 0, 255])
        );
        assert_ne!(blank, empty);
        for face in [ClockFace::Regular, ClockFace::Bold, ClockFace::Segment] {
            for layout in [
                ClockLayout::TimeDate,
                ClockLayout::Time,
                ClockLayout::WeekdayDate,
                ClockLayout::Inline,
            ] {
                let tile = cfg.tile_mut(10).unwrap();
                tile.kind = Content::Clock;
                tile.clock.face = face;
                tile.clock.layout = layout;
                tile.clock.hour24 = false;
                assert!(cfg.has_clock());
                let a = canvas(&cfg, 1, at(10, 41)).unwrap();
                let b = canvas(&cfg, 1, at(10, 42)).unwrap();
                assert_ne!(a, b, "{face:?} {layout:?}");
                // Drawn only inside the centre strip, between the knobs and above the keys.
                for (x, y, p) in a.enumerate_pixels() {
                    // Knob outlines (x < 32, x ≥ 128) stay as they were.
                    if p[0] > 0 && (32..128).contains(&x) {
                        assert!(
                            (38..122).contains(&x) && (3..25).contains(&y),
                            "{face:?} {layout:?} {x},{y}"
                        );
                    }
                }
                assert!(render_at(&cfg, at(23, 59)).is_ok());
            }
        }
        cfg.center.kind = Content::Text;
        cfg.center.text = "Bao-keyboard".into();
        assert!(
            canvas(&cfg, 1, at(0, 0))
                .unwrap()
                .pixels()
                .any(|p| p[0] > 200)
        );
        assert_eq!(label(10), "Ô giữa");
        assert!(cfg.tile(11).is_err());
    }
    #[test]
    fn sf_symbols_from_older_layouts_become_phosphor_icons() {
        let mut json = serde_json::to_value(Layout::default()).unwrap();
        json["tiles"][0]["kind"] = "Symbol".into();
        json["tiles"][0]["symbol"] = "stop.fill".into();
        json["tiles"][0]["symbol_image"] =
            serde_json::json!({"name": "stop.fill", "png": [137, 80]});
        json["tiles"][1]["symbol"] = "checkmark".into();
        json["tiles"][0]
            .as_object_mut()
            .unwrap()
            .remove("symbol_weight");
        let mut old: Layout = serde_json::from_value(json).unwrap();
        assert!(old.validate().is_err());
        assert!(old.upgrade_symbols());
        assert_eq!(old.tiles[0].symbol, "stop");
        assert_eq!(old.tiles[0].symbol_weight, IconWeight::Fill);
        assert_eq!(old.tiles[1].symbol, "check");
        assert_eq!(old.tiles[1].symbol_weight, IconWeight::Bold);
        assert!(!old.upgrade_symbols(), "upgrade runs once");
        assert!(render(&old).is_ok());
        old.tiles[0].symbol = "not-an-icon".into();
        assert!(old.validate().is_err());
    }
    #[test]
    fn background_and_alpha_are_composited_without_affecting_tile_content() {
        let mut cfg = Layout {
            background: [255, 0, 0],
            tile_background: false,
            ..Layout::default()
        };
        cfg.tiles.iter_mut().for_each(|t| t.kind = Content::Empty);
        cfg.use_background_image = true;
        cfg.background_image = Some(Asset {
            name: "background".into(),
            png: png(&RgbaImage::from_pixel(160, 80, Rgba([0, 0, 255, 128]))).unwrap(),
        });
        let rendered = render(&cfg).unwrap();
        let image = crate::media::preview(&rendered.gif).unwrap();
        let p = image.get_pixel(80, 40);
        assert!(p[0] > 110 && p[2] > 110 && p[1] < 20 && p[3] == 255);
    }
}
