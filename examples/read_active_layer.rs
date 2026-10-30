//! Read-only acceptance tool for the optional layer-read firmware.
use anyhow::{Result, ensure};
use mk::{protocol::Protocol, service::Client, transport};
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    ensure!(
        Client::connect().is_err(),
        "Đóng Coban-mod và agent trước khi đọc"
    );
    let seconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "120".into())
        .parse::<u64>()?;
    ensure!((1..=600).contains(&seconds), "Thời gian từ 1–600 giây");
    let devices = transport::enumerate()?;
    ensure!(devices.len() == 1, "Cần đúng một Coban Pad 10A");
    let mut protocol = Protocol::new(transport::HidTransport::open(&devices[0])?);
    let started = Instant::now();
    let mut previous = None;
    while started.elapsed() < Duration::from_secs(seconds) {
        let state = protocol.read_active_layer()?.ok_or_else(|| {
            anyhow::anyhow!("Firmware chưa hỗ trợ CBLR v1; không thể xác định lớp thực tế")
        })?;
        let signature = (state.active, state.layer_mask, state.default_mask);
        if previous != Some(signature) {
            println!(
                "{} ms · Lớp {} / {} · QMK={} · mask={:#010x} · default={:#010x} · uptime={} ms",
                started.elapsed().as_millis(),
                state.active + 1,
                state.count,
                state.active,
                state.layer_mask,
                state.default_mask,
                state.uptime_ms
            );
            previous = Some(signature);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
