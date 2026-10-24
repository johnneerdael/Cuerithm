//! How long a finished rekordbox sync keeps what restoring it needs.
//!
//! Applying a sync keeps a copy of the DJ database as it was (tens of megabytes), the staged and
//! replaced analysis resources, and a frozen copy of the sync records. All of that exists only so
//! the sync can be restored. Like every Undo, it is kept for the newest ten syncs of a target;
//! an older one keeps a one-line summary in the history and says that its backup is gone. A sync
//! that was restored or rolled back can never be restored again and gives up its copies at once.
use super::*;

/// How many applied syncs of one target stay restorable.
pub const RESTORE_LIMIT: usize = crate::REMOVAL_UNDO_LIMIT;
/// Syncs handled per pass, so that opening a library with a long history stays quick.
const PER_PASS: usize = 50;

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    restore_expired: bool,
}
/// Whether a saved journal is only the summary an expired sync leaves behind.
pub(super) fn is_expired(journal: &str) -> bool {
    serde_json::from_str::<Probe>(journal).is_ok_and(|probe| probe.restore_expired)
}

/// Gives up the copies of one finished sync and replaces its journal by a summary.
fn expire(library: &Library, id: &str) -> Result<u64> {
    let op = load(library, id)?;
    ensure!(
        matches!(op.status.as_str(), "applied" | "restored" | "rolled_back"),
        "Only a finished sync gives up its backup"
    );
    let mut freed = 0;
    let workspace = root(library).join("operations").join(id);
    if let Ok(metadata) = fs::symlink_metadata(&workspace)
        && metadata.is_dir()
        && !metadata.file_type().is_symlink()
    {
        freed += size_of(&workspace);
        fs::remove_dir_all(&workspace)?;
    }
    freed += FileTransaction::expire_payload(&root(library).join("assets"), &op.asset_journal_id)?;
    let summary = json!({
        "version": 1,
        "id": op.id,
        "target": {"id": op.target.id},
        "native_target_id": op.native_target_id,
        "status": op.status,
        "created": op.created,
        "updated": op.updated,
        "outcome": op.outcome,
        "restore_expired": true,
    });
    library.conn()?.execute(
        "UPDATE rekordbox_sync_operations SET journal_json=?1 WHERE id=?2 AND status=?3",
        params![serde_json::to_string(&summary)?, id, op.status],
    )?;
    Ok(freed)
}
fn size_of(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        0
    } else if metadata.is_dir() {
        fs::read_dir(path)
            .map(|rows| rows.flatten().map(|row| size_of(&row.path())).sum())
            .unwrap_or(0)
    } else {
        metadata.len()
    }
}

/// Applies the bound with the service lock already held (every sync operation holds it).
pub(super) fn prune_locked(library: &Library, limit: usize) -> Result<u64> {
    let rows: Vec<(String, String, String, String)> = {
        let connection = library.conn()?;
        let mut statement = connection.prepare(
            "SELECT id,target_id,status,journal_json FROM rekordbox_sync_operations
              WHERE status IN ('applied','restored','rolled_back')
              ORDER BY created DESC,id DESC",
        )?;
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut newest: BTreeMap<String, usize> = BTreeMap::new();
    let (mut freed, mut handled) = (0, 0);
    for (id, target, status, journal) in rows {
        let expired = is_expired(&journal);
        drop(journal);
        let beyond = if status == "applied" {
            let rank = newest.entry(target).or_default();
            *rank += 1;
            *rank > limit
        } else {
            true
        };
        if !beyond || expired || handled >= PER_PASS {
            continue;
        }
        handled += 1;
        // One that cannot be read or cleaned now is tried again at the next pass.
        if let Ok(bytes) = expire(library, &id) {
            freed += bytes;
        }
    }
    Ok(freed)
}

impl Library {
    /// Frees the backups of finished rekordbox syncs beyond the newest ten of each target.
    /// Run when the library opens; every sync does the same when it finishes.
    pub(crate) fn prune_rekordbox_sync_history(&self) -> Result<u64> {
        if !self.data_dir().join("sync/rekordbox").exists() {
            return Ok(0);
        }
        let _lock = lock(self)?;
        prune_locked(self, RESTORE_LIMIT)
    }
}
