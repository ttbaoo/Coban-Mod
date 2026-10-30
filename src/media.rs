//! Convert locally to a bounded 160x80, <=128-color standard GIF for Coban 10A.
use anyhow::{Result, ensure};
use image::{AnimationDecoder, ImageDecoder, ImageReader, imageops::FilterType};
use std::{
    io::{BufReader, Cursor},
    path::Path,
};
pub const MAX_BYTES: usize = 102400;
pub fn prepare(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        std::fs::metadata(path)?.len() <= 20_000_000,
        "Ảnh gốc tối đa 20 MB"
    );
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits.clone());
    let format = reader
        .format()
        .ok_or_else(|| anyhow::anyhow!("Không nhận diện ảnh"))?;
    let mut frames = Vec::new();
    if format == image::ImageFormat::Gif {
        let mut decoder =
            image::codecs::gif::GifDecoder::new(BufReader::new(std::fs::File::open(path)?))?;
        decoder.set_limits(limits)?;
        for frame in decoder.into_frames() {
            ensure!(frames.len() < 300, "Tối đa 300 khung GIF");
            let frame = frame?;
            let (ms, den) = frame.delay().numer_denom_ms();
            let delay = ((ms as u64 / den.max(1) as u64) / 10).clamp(2, 65535) as u16;
            frames.push((resize(frame.buffer())?, delay));
        }
    } else {
        let img = reader.decode()?.to_rgba8();
        frames.push((resize(&img)?, 100));
    }
    ensure!(!frames.is_empty(), "GIF không có khung hình");
    encode_frames(&frames)
}
fn resize(img: &image::RgbaImage) -> Result<image::RgbaImage> {
    ensure!(
        img.width() > 0
            && img.height() > 0
            && img.width() as u64 * img.height() as u64 <= 16_000_000,
        "Kích thước ảnh quá lớn"
    );
    let mut image = image::imageops::resize(img, 160, 80, FilterType::Triangle);
    for p in image.pixels_mut() {
        let a = p[3] as u16;
        for c in 0..3 {
            p[c] = ((p[c] as u16 * a) / 255) as u8;
        }
        p[3] = 255;
    }
    Ok(image)
}
pub fn encode_frames(frames: &[(image::RgbaImage, u16)]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut bytes, 160, 80, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        for (img, delay) in frames {
            ensure!(
                img.width() == 160 && img.height() == 80,
                "Khung hình phải có kích thước 160×80"
            );
            let quant = color_quant::NeuQuant::new(10, 128, img.as_raw());
            let pixels: Vec<u8> = img.pixels().map(|p| quant.index_of(&p.0) as u8).collect();
            let mut frame =
                gif::Frame::from_palette_pixels(160, 80, pixels, quant.color_map_rgb(), None);
            frame.delay = *delay;
            frame.dispose = gif::DisposalMethod::Keep;
            encoder.write_frame(&frame)?;
        }
    }
    ensure!(
        bytes.len() <= MAX_BYTES,
        "GIF sau chuyển đổi là {} KB, vượt 100 KB; giảm số khung hoặc độ phức tạp",
        bytes.len().div_ceil(1024)
    );
    Ok(bytes)
}
pub fn preview(bytes: &[u8]) -> Result<image::RgbaImage> {
    Ok(image::load(Cursor::new(bytes), image::ImageFormat::Gif)?.to_rgba8())
}
