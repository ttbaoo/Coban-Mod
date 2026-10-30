//! Observe agent state only; the agent remains the sole USB owner.
use anyhow::{Result, ensure};
use mk::service::{Client, Event, Job};
use std::time::{Duration, Instant};
fn main() -> Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "300".into())
        .parse::<u64>()?;
    ensure!((1..=600).contains(&seconds), "Thời gian từ 1–600 giây");
    let client = Client::connect()?;
    let started = Instant::now();
    let mut previous = None;
    while started.elapsed() < Duration::from_secs(seconds) {
        let Event::HostStatus(state) = client.call(Job::Status)? else {
            anyhow::bail!("Không đọc được status")
        };
        let layer = state.layer_sync;
        let signature = (
            layer.active,
            layer.displayed,
            layer.supported,
            layer.enabled,
            layer.message.clone(),
        );
        if previous.as_ref() != Some(&signature) {
            println!(
                "{} ms · active={:?} displayed={:?} supported={} enabled={} · {}",
                started.elapsed().as_millis(),
                layer.active.map(|n| n + 1),
                layer.displayed.map(|n| n + 1),
                layer.supported,
                layer.enabled,
                layer.message
            );
            previous = Some(signature);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
