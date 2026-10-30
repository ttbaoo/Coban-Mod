//! Restore a verified configuration backup and original GIF after a separately
//! authorized firmware deployment. All writes use existing diff/readback guards.
use anyhow::{Context, Result, ensure};
use mk::{media, protocol::Protocol, service::Client, storage, transport::Transport};

fn verify_image<T: Transport>(protocol: &mut Protocol<T>, original: &[u8]) -> Result<()> {
    // This firmware exposes only its bounded GIF region, in 25-byte blocks.
    let mut actual = Vec::with_capacity(original.len());
    for offset in (0..original.len()).step_by(25) {
        let mut q = vec![8, 0, 0x58];
        q.extend_from_slice(&(offset as u32).to_be_bytes());
        let reply = protocol.io.exchange(&q, 7)?;
        let n = (original.len() - offset).min(25);
        actual.extend_from_slice(&reply[7..7 + n]);
    }
    ensure!(actual == original, "Ảnh GIF trong flash không khớp bản gốc");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 4 && matches!(args[1].as_str(), "--restore" | "--verify"),
        "Dùng --restore hoặc --verify BACKUP_FILE ORIGINAL_GIF"
    );
    ensure!(
        Client::connect().is_err(),
        "Đóng Coban-mod và agent trước khi khôi phục"
    );
    let path = std::path::Path::new(&args[2]);
    let saved = storage::load(path)?;
    ensure!(!saved.simulated, "Không dùng backup mô phỏng");
    let gif = std::fs::read(&args[3])?;
    ensure!(
        gif.len() <= media::MAX_BYTES && media::preview(&gif)?.dimensions() == (160, 80),
        "GIF gốc không hợp lệ"
    );
    let devices = mk::transport::enumerate()?;
    ensure!(
        devices.len() == 1 && devices[0].identity == saved.device,
        "Thiết bị không khớp backup"
    );
    let mut protocol = Protocol::new(mk::transport::HidTransport::open(&devices[0])?);
    let layer = protocol
        .read_active_layer()?
        .context("Thiếu getter CBLR sau nạp firmware")?;
    ensure!(layer.count == saved.layers, "Số lớp không khớp");
    let live = protocol.read_snapshot(saved.device.clone())?;
    ensure!(
        live.macro_count == saved.macro_count
            && live.macro_buffer.len() == saved.macro_buffer.len(),
        "Dung lượng macro khác bản gốc"
    );
    if args[1] == "--verify" {
        ensure!(saved.same_content(&live), "Cấu hình không khớp bản gốc");
        verify_image(&mut protocol, &gif)?;
        println!(
            "Đã đọc lại GIF từ flash: {} byte khớp bản gốc; keymap/encoder/macro/settings cũng khớp.",
            gif.len()
        );
        return Ok(());
    }
    let dir = path.parent().context("Thiếu thư mục backup")?;
    storage::save(
        &dir.join("configuration-after-flash-before-restore.mk.json"),
        &live,
    )?;
    let result = protocol.apply(&live, &saved)?;
    ensure!(
        saved.same_content(&result.snapshot),
        "Khôi phục cấu hình chưa khớp"
    );
    println!(
        "Đã khôi phục/đọc lại cấu hình: {} thay đổi; mọi keymap/encoder/macro/settings khớp backup.",
        result.changes
    );
    protocol.upload_gif(&gif)?;
    // The pinned firmware handles GIF size on SET 0x57, not SAVE 0x57.
    let mut size = vec![7, 0, 0x57];
    size.extend_from_slice(&(gif.len() as u32).to_be_bytes());
    protocol.io.exchange(&size, 3)?;
    let screen = saved
        .settings
        .iter()
        .find(|s| s.key == "screen")
        .context("Thiếu thiết lập màn hình")?;
    let mut select = vec![7, 0, 0x51];
    select.extend_from_slice(&screen.value);
    protocol.io.exchange(&select, 3)?;
    protocol.io.exchange(&[9, 0, 0x96], 3)?;
    let size_reply = protocol.io.exchange(&[8, 0, 0x57], 3)?;
    ensure!(
        u32::from_be_bytes(size_reply[3..7].try_into()?) as usize == gif.len(),
        "Kích thước GIF đọc lại không khớp"
    );
    verify_image(&mut protocol, &gif)?;
    let readback = protocol.read_snapshot(saved.device.clone())?;
    ensure!(
        saved.same_content(&readback),
        "Cấu hình thay đổi khi gửi lại ảnh"
    );
    storage::save(&dir.join("configuration-restored.mk.json"), &readback)?;
    let active = protocol.read_active_layer()?.context("Mất getter CBLR")?;
    println!(
        "Đã gửi lại GIF gốc, {} byte; giữ chế độ màn hình {}; lớp hiện tại {} / {}.",
        gif.len(),
        screen.value[0],
        active.active + 1,
        active.count
    );
    Ok(())
}
