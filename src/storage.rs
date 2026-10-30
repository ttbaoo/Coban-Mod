use crate::model::Snapshot;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
fn unique_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    sha256: String,
    snapshot: Snapshot,
}
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("MK_DATA_DIR") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("MK")
}
pub fn digest(snapshot: &Snapshot) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(snapshot)?)))
}
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".mk-{}-{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        unique_id()
    ));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let outcome = (|| -> Result<()> {
        let mut f = opts.open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temp, path).with_context(|| format!("Không lưu được {}", path.display()))?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(temp);
    }
    outcome
}
pub fn save(path: &Path, snapshot: &Snapshot) -> Result<()> {
    snapshot.validate()?;
    let envelope = Envelope {
        format: "MK-Coban-configuration".into(),
        sha256: digest(snapshot)?,
        snapshot: snapshot.clone(),
    };
    write_private(path, &serde_json::to_vec_pretty(&envelope)?)
}
pub fn load(path: &Path) -> Result<Snapshot> {
    ensure!(fs::metadata(path)?.len() <= 2_000_000, "Backup quá lớn");
    let e: Envelope = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        e.format == "MK-Coban-configuration",
        "Đây không phải backup Coban-mod"
    );
    e.snapshot.validate()?;
    ensure!(
        digest(&e.snapshot)? == e.sha256,
        "Checksum backup sai; không nhập dữ liệu"
    );
    Ok(e.snapshot)
}
pub fn automatic_backup(snapshot: &Snapshot) -> Result<PathBuf> {
    let path = data_dir().join("backups").join(format!(
        "{}coban-10a-{}-{}-{}.mk.json",
        if snapshot.simulated { "demo-" } else { "" },
        chrono::Local::now().format("%Y%m%d-%H%M%S-%9f"),
        std::process::id(),
        unique_id()
    ));
    save(&path, snapshot)?;
    Ok(path)
}
