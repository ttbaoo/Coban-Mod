#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use anyhow::{Context, Result};
use mk::{
    service::{self, Client, Event, Job},
    storage,
};
#[cfg(feature = "desktop")]
mod app;
#[cfg(feature = "desktop")]
slint::include_modules!();
fn main() {
    if let Err(e) = run() {
        eprintln!("Coban-mod: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--agent") => service::serve(),
        Some("--agent-status") => {
            let client = Client::connect()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&client.call(Job::Status)?)?
            );
            Ok(())
        }
        Some("--agent-stop") => {
            let event = Client::connect()?.call(Job::StopAgent)?;
            println!("{}", serde_json::to_string_pretty(&event)?);
            Ok(())
        }
        Some("--list") => {
            let client = Client::ensure_running()?;
            let Event::Devices(devices, _) = client.call(Job::Discover(false))? else {
                anyhow::bail!("Không đọc được danh sách thiết bị từ agent");
            };
            for (i, d) in devices.iter().enumerate() {
                println!("{i}: {}", d.display);
            }
            Ok(())
        }
        Some("--check-backup") => {
            let path = std::path::PathBuf::from(args.get(2).context("Thiếu đường dẫn backup")?);
            let s = storage::load(&path)?;
            println!(
                "Backup hợp lệ: {} layers, {} macros, {} settings",
                s.layers,
                s.macro_count,
                s.settings.len()
            );
            Ok(())
        }
        Some("--inspect") | Some("--backup") => {
            let client = Client::ensure_running()?;
            let Event::Devices(devices, _) = client.call(Job::Discover(false))? else {
                anyhow::bail!("Agent không liệt kê được thiết bị");
            };
            anyhow::ensure!(
                devices.len() == 1,
                "Cần đúng một Coban Pad 10A; chọn thiết bị trong giao diện"
            );
            let event = client.call(Job::Connect(devices[0].clone()))?;
            let s = match event {
                Event::Snapshot(snapshot) => snapshot,
                Event::Error(text, _) => anyhow::bail!(text),
                _ => anyhow::bail!("Agent không trả snapshot"),
            };
            if args[1] == "--backup" {
                let path = std::path::PathBuf::from(args.get(2).context("Thiếu đường dẫn backup")?);
                let path = std::path::absolute(path)?;
                match client.call(Job::Backup(path))? {
                    Event::Backup(path) => println!("Backup: {}", path.display()),
                    Event::Error(text, _) => anyhow::bail!(text),
                    _ => anyhow::bail!("Agent chưa xác nhận backup"),
                }
            }
            println!(
                "{} · {:04X}:{:04X}\nVIA {} · {} layers · {} macros · {} macro bytes · {} settings\nWarnings: {}",
                s.device.product,
                s.device.vendor_id,
                s.device.product_id,
                s.protocol_version,
                s.layers,
                s.macro_count,
                s.macro_buffer.len(),
                s.settings.len(),
                s.warnings.join("; ")
            );
            Ok(())
        }
        #[cfg(feature = "desktop")]
        None => app::run(false),
        #[cfg(feature = "desktop")]
        Some("--background") => app::run(true),
        #[cfg(feature = "desktop")]
        Some(_) => anyhow::bail!(
            "Tham số không được hỗ trợ. Mở Coban-mod không kèm tham số để điều khiển bàn phím thật."
        ),
        #[cfg(not(feature = "desktop"))]
        _ => {
            println!(
                "Coban-mod CLI: --list | --inspect | --backup FILE. Build default features for the desktop UI."
            );
            Ok(())
        }
    }
}
