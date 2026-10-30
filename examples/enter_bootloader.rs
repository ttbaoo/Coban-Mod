//! Explicit flash preparation: validate a fresh configuration backup, then send
//! the vendor's documented bootloader command once. Never erases/writes flash.
use anyhow::{Result, ensure};
use hidapi::HidApi;
use mk::{protocol::Protocol, service::Client, storage, transport};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 3 && args[1] == "--enter",
        "Dùng --enter BACKUP_FILE"
    );
    ensure!(
        Client::connect().is_err(),
        "Đóng Coban-mod và agent trước khi vào bootloader"
    );
    let backup = storage::load(std::path::Path::new(&args[2]))?;
    ensure!(!backup.simulated, "Không dùng backup mô phỏng");
    let devices = transport::enumerate()?;
    ensure!(devices.len() == 1, "Cần đúng một Coban Pad 10A");
    let device = &devices[0];
    ensure!(
        device.identity == backup.device,
        "Thiết bị không khớp backup"
    );
    {
        let mut protocol = Protocol::new(transport::HidTransport::open(device)?);
        let live = protocol.read_snapshot(device.identity.clone())?;
        ensure!(
            backup.same_content(&live),
            "Cấu hình đã đổi sau backup; sao lưu lại trước"
        );
    }
    let api = HidApi::new()?;
    let hid = api.open_path(&device.path)?;
    let mut request = [0u8; 33];
    request[1..4].copy_from_slice(&[7, 0, 0x99]);
    // pad10a/hid/hid.c: SET custom 0x99 calls reset_keyboard(); QMK RP2040
    // bootloader_jump() calls ROM reset_usb_boot(). No reply is required.
    ensure!(hid.write(&request)? == 33, "Không gửi đủ lệnh bootloader");
    println!("Đã gửi đúng một lệnh vào bootloader; chưa ghi flash.");
    Ok(())
}
