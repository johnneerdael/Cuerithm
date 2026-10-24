//! The copies of removed files that earlier versions kept in the data folder.
//!
//! Earlier versions deleted an unused file and kept a copy of it in the data folder, for a
//! Restore button that never expired. The library no longer keeps audio. For a removal that
//! can still be restored this copy is the only one left, so it is not deleted: it goes to the
//! Trash under the file's own name, where the user can still put it back. The copies of a
//! removal that was restored or rolled back duplicate a file that is in its place, so they are
//! simply deleted.
//!
//! Only files the application created are touched: a copy must be inside the removal folder of
//! the data folder, a regular file and not a link. A bounded number of removals is handled per
//! start; the rest follows at the next one.
//!
//! The copy is the only one left, so nothing here may declare it gone on a guess. A removal whose
//! folder cannot be listed, or one of whose copies cannot be read, is left untouched for the next
//! start. A copy is moved into a holder folder of its own before the Trash call; if the app
//! exits during the call the copy is still in that holder, and the next start looks there too.
//! A file that matches nothing is sent to the Trash like the rest, never deleted.
use super::*;
use crate::legacy_originals::owned_regular_file;
use std::collections::BTreeMap;

/// Removals handled per start.
const OPERATIONS_PER_START: usize = 50;
const AREA: &str = "maintenance/removal";

/// The folder a copy waits in while the Trash call runs.
const HOLDER: &str = "trash-";
/// Entries looked at in one removal folder. More than this is not a folder this application
/// made, and is left alone rather than half read.
const MAX_ENTRIES: usize = 100_000;

fn is_holder(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(HOLDER))
}

fn add(found: &mut BTreeMap<String, Vec<PathBuf>>, data: &Path, path: PathBuf) -> Result<()> {
    if owned_regular_file(data, AREA, &path).is_some() {
        let hash = cuerithm_media::file_sha256(&path)
            .with_context(|| format!("Cannot read the kept copy {}", path.display()))?;
        found.entry(hash).or_default().push(path);
    }
    Ok(())
}

/// The copies of one removal found on disk, by the hash of their content, directly in its folder
/// and in the holders an interrupted start left behind. A copy is told apart by its bytes, not
/// its name: an interrupted disposal from an earlier version renamed some. A folder that is gone
/// holds nothing; one that cannot be listed, or a copy that cannot be read, is an error, because
/// "not found" would be a guess about the only copy there is.
fn copies(data: &Path, workspace: &Path) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let rows = match std::fs::read_dir(workspace) {
        Ok(rows) => rows,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(error).context("Cannot list the kept copies"),
    };
    let mut seen = 0usize;
    for row in rows {
        let path = row?.path();
        seen += 1;
        ensure!(seen <= MAX_ENTRIES, "Too many entries in a removal folder");
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() && !meta.file_type().is_symlink() && is_holder(&path) {
            for inner in std::fs::read_dir(&path).context("Cannot list a Trash holder")? {
                seen += 1;
                ensure!(seen <= MAX_ENTRIES, "Too many entries in a removal folder");
                add(&mut found, data, inner?.path())?;
            }
        } else {
            add(&mut found, data, path)?;
        }
    }
    Ok(found)
}

/// Removes the holders that nothing is waiting in.
fn remove_empty_holders(workspace: &Path) {
    if let Ok(rows) = std::fs::read_dir(workspace) {
        for row in rows.flatten() {
            let path = row.path();
            let plain_folder = std::fs::symlink_metadata(&path)
                .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink());
            if plain_folder && is_holder(&path) {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

/// Moves one copy to the Trash under the file's own name. On a refusal the copy is back where
/// it was, or still in its holder.
fn to_trash(
    copy: &Path,
    name: &std::ffi::OsStr,
    hash: &str,
    trash: &dyn FileTrash,
) -> Result<(), String> {
    let folder = copy.parent().ok_or("Missing folder")?;
    if is_holder(folder) {
        // An earlier start exited after it renamed the copy into its holder, which gave it its
        // own name. The Trash call is all that is left.
        let moved = trash.move_to_trash(copy);
        if moved.is_ok() {
            let _ = std::fs::remove_dir(folder);
        }
        return moved;
    }
    let holder = folder.join(format!("{HOLDER}{}", &hash[..16.min(hash.len())]));
    std::fs::create_dir_all(&holder).map_err(|e| e.to_string())?;
    let named = holder.join(name);
    crate::publish_files::create(copy, &named).map_err(|e| format!("{e:#}"))?;
    let moved = trash.move_to_trash(&named);
    if moved.is_err() {
        let _ = crate::publish_files::create(&named, copy);
    }
    let _ = std::fs::remove_dir(&holder);
    moved
}

/// Moves what earlier versions kept out of the data folder. Runs after the interrupted removals
/// have been finished.
pub(super) fn retire(lib: &Library, trash: &dyn FileTrash) -> Result<()> {
    retire_with(lib, trash, OPERATIONS_PER_START)
}
pub(super) fn retire_with(lib: &Library, trash: &dyn FileTrash, limit: usize) -> Result<()> {
    // Nothing creates or reads these any more; one left half-done would only block a library
    // backup for ever.
    lib.conn()?.execute(
        "DELETE FROM maintenance_scans WHERE kind IN ('unused_backup_disposal','unused_backup_disposal_plan')",
        [],
    )?;
    let ids: Vec<String> = {
        let conn = lib.conn()?;
        let mut q = conn.prepare(
            "SELECT id FROM maintenance_scans WHERE kind='unused_removal_operation'
               AND json_extract(payload_json,'$.schema') IN ('unused-removal-v1','unused-removal-v2')
               AND COALESCE(json_extract(payload_json,'$.backups_disposed'),0)=0
               AND json_extract(payload_json,'$.cleanup_complete')=1
               AND json_extract(payload_json,'$.state') IN ('committed','undone','rolled_back')
             ORDER BY created,rowid LIMIT ?1",
        )?;
        q.query_map([limit as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let data = lib.data_dir().canonicalize()?;
    let (mut files, mut bytes) = (0u64, 0u64);
    for id in ids {
        let Ok(mut op) = load_operation(lib, &id) else {
            continue;
        };
        let Some(workspace) = bytes_path(&op.workspace)
            .ok()
            .filter(|w| !w.as_os_str().is_empty())
        else {
            // Nothing was kept, or the folder was removed: there is nothing left to move.
            op.backups_disposed = true;
            save(&*lib.conn()?, &op)?;
            continue;
        };
        // The copy is the only one left when the removal still stands.
        let stands = op.state == "committed" && op.undone_by.is_none();
        // A folder that cannot be listed or a copy that cannot be read: try again next start.
        let Ok(mut found) = copies(&data, &workspace) else {
            continue;
        };
        let mut complete = true;
        for f in op.files.iter_mut() {
            if stands && f.disposal.is_some() {
                // Handled by an earlier start that could not finish the whole removal.
                continue;
            }
            let hash = f.entry.public.source_sha256.clone();
            let Some(copy) = found.get_mut(&hash).and_then(|paths| paths.pop()) else {
                // Already gone: a disposal from an earlier version, or a damaged folder.
                if stands && f.disposal.is_none() {
                    f.disposal = Some(UnusedRemovalDisposal::Gone);
                }
                continue;
            };
            let size = std::fs::metadata(&copy).map(|m| m.len()).unwrap_or(0);
            if stands && f.disposal.is_none() {
                let name = bytes_path(&f.entry.path)?
                    .file_name()
                    .map(|n| n.to_owned())
                    .unwrap_or_else(|| "removed-file".into());
                match to_trash(&copy, &name, &hash, trash) {
                    Ok(()) => f.disposal = Some(UnusedRemovalDisposal::Trashed),
                    Err(reason) => {
                        f.reason = Some(format!("Kept in the data folder: {reason}"));
                        complete = false;
                        continue;
                    }
                }
            } else if std::fs::remove_file(&copy).is_err() {
                complete = false;
                continue;
            }
            files += 1;
            bytes += size;
            f.backup = vec![];
        }
        if complete {
            // Whatever else is in the folder (a copy that no file of the removal matches) was
            // made by this application. It may be the last trace of a file, so it goes to the
            // Trash like the rest and is never deleted.
            for (hash, paths) in std::mem::take(&mut found) {
                for rest in paths {
                    let size = std::fs::metadata(&rest).map(|m| m.len()).unwrap_or(0);
                    let name = rest
                        .file_name()
                        .map(|n| n.to_owned())
                        .unwrap_or_else(|| "kept-copy".into());
                    if to_trash(&rest, &name, &hash, trash).is_ok() {
                        files += 1;
                        bytes += size;
                    } else {
                        complete = false;
                    }
                }
            }
        }
        if complete {
            remove_empty_holders(&workspace);
            let _ = std::fs::remove_dir(&workspace);
            op.backups_disposed = true;
            op.workspace = vec![];
            for f in &mut op.files {
                f.backup = vec![];
            }
        }
        if stands {
            op.result.outcomes = outcomes(&op);
        }
        save(&*lib.conn()?, &op)?;
    }
    lib.record_originals_removed(files, bytes)
}
