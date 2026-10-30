//! Serialized, acknowledged changes. A pending change is never a saved configuration.
use crate::model::Snapshot;
use anyhow::{Context, Result, ensure};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Change {
    pub before: Snapshot,
    pub after: Snapshot,
    pub description: String,
    undo: bool,
}
#[derive(Default)]
pub struct Editor {
    current: Option<Snapshot>,
    pending: Option<Change>,
    undo: Vec<Snapshot>,
    pub history: Vec<String>,
    needs_read: bool,
}
impl Editor {
    pub fn current(&self) -> Result<&Snapshot> {
        self.current.as_ref().context("Chưa đọc cấu hình bàn phím")
    }
    pub fn replace(&mut self, snapshot: Snapshot) -> Result<()> {
        snapshot.validate()?;
        ensure!(self.pending.is_none(), "Đang lưu, vui lòng chờ hoàn tất");
        self.current = Some(snapshot);
        self.undo.clear();
        self.needs_read = false;
        Ok(())
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() && self.pending.is_none() && !self.needs_read
    }
    pub fn begin(&mut self, after: Snapshot, description: String) -> Result<Option<Change>> {
        self.begin_inner(after, description, false)
    }
    fn begin_inner(
        &mut self,
        after: Snapshot,
        description: String,
        undo: bool,
    ) -> Result<Option<Change>> {
        ensure!(!self.needs_read, "Đọc lại bàn phím trước khi chỉnh tiếp");
        ensure!(self.pending.is_none(), "Đang lưu thay đổi trước");
        let before = self.current()?.clone();
        before.compatible(&after)?;
        if before.same_content(&after) {
            return Ok(None);
        }
        let change = Change {
            before,
            after,
            description,
            undo,
        };
        self.pending = Some(change.clone());
        Ok(Some(change))
    }
    pub fn begin_undo(&mut self) -> Result<Option<Change>> {
        let target = self
            .undo
            .last()
            .context("Chưa có thay đổi để hoàn tác")?
            .clone();
        self.begin_inner(target, "Đã hoàn tác thay đổi trước".into(), true)
    }
    pub fn confirm(&mut self, snapshot: Snapshot) -> Result<String> {
        let change = self
            .pending
            .as_ref()
            .context("Không có thao tác đang lưu")?;
        change.after.compatible(&snapshot)?;
        ensure!(
            change.after.same_content(&snapshot),
            "Đọc lại không khớp; chưa xác nhận lưu"
        );
        let change = self.pending.take().expect("checked pending");
        if change.undo {
            self.undo.pop();
        } else {
            self.undo.push(change.before);
            if self.undo.len() > 20 {
                self.undo.remove(0);
            }
        }
        self.current = Some(snapshot);
        self.history.insert(
            0,
            format!(
                "{} · {}",
                chrono::Local::now().format("%H:%M:%S"),
                change.description
            ),
        );
        self.history.truncate(30);
        Ok(change.description)
    }
    pub fn fail(&mut self) {
        self.pending = None;
        self.needs_read = true;
    }
}
