//! Reviewed removal of unused files. All input paths originate in a scan.
//!
//! The files go to the operating system Trash or Recycle Bin, where the user can put them
//! back. The library keeps no copy of the audio: it holds a journal of what was removed, which
//! is only paths and hashes. Earlier versions deleted the files and kept a copy in the data
//! folder for a Restore button; [`legacy`] moves what those versions left behind to the Trash.
//!
//! The work is journaled so that an interruption never loses a file. First each file is moved
//! to a hidden temporary name beside it and the journal is committed. Only then is each file
//! given back its own name and handed to the Trash, one at a time, with the journal recording
//! the result of every file. A file the Trash refuses stays where it was unless the preview
//! announced that it would be deleted permanently.
mod folders;
mod legacy;
use super::*;
use crate::{
    Library,
    model::{bytes_path, path_bytes},
};
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256_cancellable};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    device: u64,
    file: u64,
}
fn identity(path: &Path) -> Result<Identity> {
    identity_kind(path, false)
}
fn identity_kind(path: &Path, directory: bool) -> Result<Identity> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "Removal source is not a regular file"
    );
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(
            windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        );
    }
    let file = options.open(path)?;
    ensure!(
        if directory {
            file.metadata()?.is_dir()
        } else {
            file.metadata()?.is_file()
        },
        "Removal source is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok(Identity {
            device: meta.dev(),
            file: meta.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0,
            "Cannot identify removal source: {}",
            std::io::Error::last_os_error()
        );
        Ok(Identity {
            device: u64::from(info.dwVolumeSerialNumber),
            file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        })
    }
}
fn guard(path: &Path, root: &Path) -> Result<()> {
    ensure!(
        root.canonicalize()? == root && std::fs::symlink_metadata(root)?.is_dir(),
        "Removal root changed"
    );
    ensure!(
        path.is_absolute() && path.starts_with(root) && path != root,
        "Removal path leaves reviewed root"
    );
    let parent = path.parent().context("Missing removal parent")?;
    ensure!(
        parent.canonicalize()? == parent,
        "Removal parent changed or became a symlink"
    );
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    path: Vec<u8>,
    identity: Identity,
    public: UnusedRemovalFile,
}
#[derive(Serialize, Deserialize)]
struct Preview {
    public: UnusedRemovalPreview,
    root: Vec<u8>,
    entries: Vec<Entry>,
    logic: String,
    consumed_by: Option<String>,
    #[serde(default)]
    directories: Vec<folders::Folder>,
}
fn load<T: serde::de::DeserializeOwned>(conn: &Connection, id: &str, kind: &str) -> Result<T> {
    let text: String = conn.query_row(
        "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind=?2",
        params![id, kind],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&text)?)
}
fn entry(
    path: &Path,
    root: &Path,
    role: UnusedRemovalRole,
    cancel: &CancellationToken,
) -> Result<Entry> {
    guard(path, root)?;
    let before = identity(path)?;
    let hash = file_sha256_cancellable(path, cancel)?;
    let size = std::fs::symlink_metadata(path)?.len();
    ensure!(
        identity(path)? == before,
        "Removal source changed while reading"
    );
    use sha2::{Digest, Sha256};
    let id = format!(
        "{:x}",
        Sha256::digest([path_bytes(path), hash.as_bytes().to_vec()].concat())
    );
    Ok(Entry {
        path: path_bytes(path),
        identity: before,
        public: UnusedRemovalFile {
            id,
            display_path: path.to_string_lossy().into(),
            size_bytes: size,
            source_sha256: hash,
            role,
            permanent: false,
        },
    })
}
// Source helper prefers Serato, then VirtualDJ, and removes only the first present sibling.
fn sibling(path: &Path) -> Result<Option<PathBuf>> {
    let serato = path.with_file_name({
        let mut stem = path.file_stem().context("Missing stem")?.to_os_string();
        stem.push(".1.2.serato-stems");
        stem
    });
    let mut vdj = path.as_os_str().to_os_string();
    vdj.push(".vdjstems");
    for path in [serato, PathBuf::from(vdj)] {
        match std::fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some(path)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(None)
}
#[derive(Clone, Serialize, Deserialize)]
struct MovedFile {
    entry: Entry,
    /// A copy in the data folder, kept by earlier versions for Restore. Empty for every removal
    /// made since: the library never keeps the audio.
    backup: Vec<u8>,
    displaced: Vec<u8>,
    restored_identity: Option<Identity>,
    /// The name the file has while the Trash takes it: its own, or the nearest free one.
    #[serde(default)]
    staged: Option<Vec<u8>>,
    #[serde(default)]
    disposal: Option<UnusedRemovalDisposal>,
    #[serde(default)]
    reason: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Operation {
    schema: String,
    kind: String,
    state: String,
    root: Vec<u8>,
    /// The folder of the copies kept by earlier versions. Empty since they are no longer made.
    #[serde(default)]
    workspace: Vec<u8>,
    files: Vec<MovedFile>,
    result: UnusedRemovalResult,
    error: Option<String>,
    undone_by: Option<String>,
    cleanup_complete: bool,
    #[serde(default)]
    directories: Vec<folders::Folder>,
    #[serde(default)]
    backups_disposed: bool,
}
pub(crate) fn ensure_unused_unreserved(conn: &Connection, paths: &[Vec<u8>]) -> Result<()> {
    let paths = paths.iter().collect::<BTreeSet<_>>();
    let mut q=conn.prepare("SELECT payload_json FROM maintenance_scans WHERE kind='unused_removal_operation' AND json_extract(payload_json,'$.state') IN ('prepared','conflict')")?;
    for row in q.query_map([], |r| r.get::<_, String>(0))? {
        let op: Operation = serde_json::from_str(&row?)?;
        ensure!(
            !op.files.iter().any(|f| paths.contains(&f.entry.path)),
            "Destination is reserved by pending unused-file recovery"
        );
    }
    Ok(())
}
fn ensure_metadata_unreserved(conn: &Connection, files: &[MovedFile]) -> Result<()> {
    for f in files {
        let busy:i64=conn.query_row("SELECT (SELECT COUNT(*) FROM tag_write_files f JOIN tag_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM cleanup_write_files f JOIN cleanup_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM maintenance_write_files f JOIN maintenance_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','conflict'))",[&f.entry.path],|r|r.get(0))?;
        ensure!(
            busy == 0,
            "Destination is reserved by pending metadata publication"
        );
    }
    Ok(())
}
fn entry_paths(entries: &[Entry]) -> Result<Vec<PathBuf>> {
    entries.iter().map(|e| bytes_path(&e.path)).collect()
}
fn validate_entries(entries: &[Entry], root: &Path, cancel: &CancellationToken) -> Result<()> {
    for e in entries {
        cancel.check()?;
        let path = bytes_path(&e.path)?;
        guard(&path, root)?;
        ensure!(
            identity(&path)? == e.identity
                && file_sha256_cancellable(&path, cancel)? == e.public.source_sha256,
            "Removal input changed; scan again"
        );
    }
    Ok(())
}
fn missing(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.into()),
    }
}
fn save(conn: &Connection, op: &Operation) -> Result<()> {
    ensure!(conn.execute("UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2 AND kind='unused_removal_operation'",params![serde_json::to_string(op)?,op.result.id])?==1,"Recovery operation disappeared");
    Ok(())
}
fn load_operation(lib: &Library, id: &str) -> Result<Operation> {
    let op: Operation = load(&*lib.conn()?, id, "unused_removal_operation")?;
    ensure!(op.result.id == id, "Recovery operation identity mismatch");
    ensure!(
        matches!(
            op.schema.as_str(),
            "unused-removal-v1" | "unused-removal-v2" | "unused-removal-v3"
        ) && matches!(op.kind.as_str(), "remove" | "restore"),
        "Unsupported unused-file journal"
    );
    // Copies kept by earlier versions live in a folder of the data folder; a journal never
    // names a place outside it. A folder that is gone took its copies with it.
    let workspace = bytes_path(&op.workspace)
        .ok()
        .filter(|w| !w.as_os_str().is_empty());
    if let Some(workspace) = &workspace
        && let Ok(resolved) = workspace.canonicalize()
    {
        let root = lib.data_dir().canonicalize()?.join("maintenance/removal");
        ensure!(
            resolved.starts_with(&root),
            "Recovery workspace leaves application data"
        );
    }
    let targets = op
        .result
        .files
        .iter()
        .map(|f| (&f.id, &f.source_sha256))
        .collect::<BTreeSet<_>>();
    for f in &op.files {
        let path = bytes_path(&f.entry.path)?;
        let displaced = bytes_path(&f.displaced)?;
        ensure!(
            targets.contains(&(&f.entry.public.id, &f.entry.public.source_sha256))
                && f.entry.public.display_path == path.to_string_lossy(),
            "Recovery target differs from reviewed file"
        );
        if let Some(workspace) = &workspace
            && !f.backup.is_empty()
        {
            ensure!(
                super::files::resolved_prefix(&bytes_path(&f.backup)?)?.starts_with(workspace),
                "Recovery backup leaves its workspace"
            );
        }
        ensure!(
            path.starts_with(bytes_path(&op.root)?)
                && path.parent() == displaced.parent()
                && path != displaced,
            "Recovery path leaves reviewed source folder"
        );
        if let Some(staged) = &f.staged {
            let staged = bytes_path(staged)?;
            ensure!(
                staged.parent() == path.parent(),
                "Recovery name leaves reviewed source folder"
            );
        }
        let name = displaced
            .file_name()
            .context("Missing recovery filename")?
            .to_string_lossy();
        let prefix = if op.kind == "remove" {
            ".cuerithm-unused-"
        } else {
            ".cuerithm-restore-"
        };
        ensure!(
            name.starts_with(&format!("{prefix}{}-", op.result.id)) && name.ends_with(".tmp"),
            "Recovery temporary is not owned by this operation"
        );
    }
    Ok(op)
}
fn rollback_remove(f: &MovedFile, root: &Path) -> Result<()> {
    let path = bytes_path(&f.entry.path)?;
    let displaced = bytes_path(&f.displaced)?;
    guard(&path, root)?;
    if !missing(&displaced)? {
        // Preserve the actual captured source, including a user edit that raced the move.
        let _ = identity(&displaced)?;
        ensure!(
            missing(&path)?,
            "New user file preserved; captured source retained"
        );
        crate::publish_files::create(&displaced, &path)?;
    } else {
        ensure!(
            !missing(&path)?
                && identity(&path)? == f.entry.identity
                && cuerithm_media::file_sha256(&path)? == f.entry.public.source_sha256,
            "Later user edit or missing source preserved; recovery needs review"
        );
    }
    Ok(())
}
fn rollback_restore(f: &MovedFile, root: &Path) -> Result<()> {
    let path = bytes_path(&f.entry.path)?;
    folders::guard_missing(&path, root)?;
    if !missing(&path)? {
        ensure!(
            f.restored_identity.as_ref() == Some(&identity(&path)?)
                && cuerithm_media::file_sha256(&path)? == f.entry.public.source_sha256,
            "New user file preserved; interrupted restore needs review"
        );
        #[cfg(test)]
        tests::after_restore_check(&path);
        let claimed = bytes_path(&f.displaced)?;
        crate::publish_files::create(&path, &claimed)?;
        if f.restored_identity.as_ref() != Some(&identity(&claimed)?)
            || cuerithm_media::file_sha256(&claimed)? != f.entry.public.source_sha256
        {
            // A replacement arrived after the proof. Return the captured newer file
            // without replacing anything else that appeared at the original path.
            crate::publish_files::create(&claimed, &path)?;
            anyhow::bail!("Racing user replacement preserved during restore rollback");
        }
    }
    Ok(())
}
/// Stands in where a rollback needs no Trash. It refuses every file.
struct NoTrash;
impl FileTrash for NoTrash {
    fn move_to_trash(&self, _: &Path) -> std::result::Result<(), String> {
        Err("The Trash is not available here".into())
    }
}
/// A free name beside the file, for the rare case that something else took its own meanwhile.
fn spare_name(path: &Path) -> Result<PathBuf> {
    let stem = path
        .file_stem()
        .context("Missing filename")?
        .to_string_lossy()
        .into_owned();
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..100 {
        let candidate = path.with_file_name(format!("{stem} (removed {n}){extension}"));
        if missing(&candidate)? {
            return Ok(candidate);
        }
    }
    anyhow::bail!("No free name for the removed file")
}
/// Whether the file at `path` is the very file the removal was approved for: the same file
/// on disk, with the same bytes.
fn is_the_approved_file(f: &MovedFile, path: &Path) -> bool {
    identity(path).is_ok_and(|id| id == f.entry.identity)
        && cuerithm_media::file_sha256(path).is_ok_and(|h| h == f.entry.public.source_sha256)
}
/// Gives one removed file back its own name and hands it to the Trash. The journal records the
/// name before the file is renamed and the result after the Trash has answered, so an
/// interruption at any point is picked up from what is on disk: the hidden temporary, the file
/// under its name, or nothing.
fn dispose(lib: &Library, op: &mut Operation, index: usize, trash: &dyn FileTrash) -> Result<()> {
    if op.files[index].disposal.is_some() {
        return Ok(());
    }
    let root = bytes_path(&op.root)?;
    let path = bytes_path(&op.files[index].entry.path)?;
    let temp = bytes_path(&op.files[index].displaced)?;
    folders::guard_missing(&temp, &root)?;
    folders::guard_missing(&path, &root)?;
    let staged = match &op.files[index].staged {
        Some(staged) => bytes_path(staged)?,
        None => {
            let staged = if missing(&path)? {
                path.clone()
            } else {
                spare_name(&path)?
            };
            op.files[index].staged = Some(path_bytes(&staged));
            save(&*lib.conn()?, op)?;
            staged
        }
    };
    folders::guard_missing(&staged, &root)?;
    if !missing(&temp)? {
        let f = &op.files[index];
        ensure!(
            identity(&temp)? == f.entry.identity
                && cuerithm_media::file_sha256(&temp)? == f.entry.public.source_sha256,
            "Unknown recovery temporary preserved"
        );
        #[cfg(test)]
        tests::checkpoint("staged");
        crate::publish_files::create(&temp, &staged)?;
        crate::publish_files::sync_parent(&staged)?;
    }
    #[cfg(test)]
    tests::checkpoint("named");
    let f = op.files[index].clone();
    let keep = |reason: String| {
        // Back under its own name when that is free, so the file is where it always was.
        if staged != path && missing(&path).unwrap_or(false) {
            let _ = crate::publish_files::create(&staged, &path);
        }
        (UnusedRemovalDisposal::LeftInPlace, Some(reason))
    };
    let (disposal, reason) = if missing(&staged)? {
        (UnusedRemovalDisposal::Gone, None)
    } else if identity(&staged).is_ok_and(|id| id != f.entry.identity) {
        // A different file has the name now: the removed one was taken out of the way before.
        (UnusedRemovalDisposal::Gone, None)
    } else if !is_the_approved_file(&f, &staged) {
        keep("The file changed after it was approved, so it was left where it is.".into())
    } else {
        match trash.move_to_trash(&staged) {
            Ok(()) => (UnusedRemovalDisposal::Trashed, None),
            Err(reason) if f.entry.public.permanent => {
                // The Trash took its time and refused; the file may have changed meanwhile.
                if !is_the_approved_file(&f, &staged) {
                    keep(
                        "The file changed after it was approved, so it was left where it is."
                            .into(),
                    )
                } else {
                    match trash.delete_permanently(&staged) {
                        Ok(()) => (UnusedRemovalDisposal::Deleted, None),
                        Err(error) => {
                            keep(format!("{reason}; permanent deletion also failed: {error}"))
                        }
                    }
                }
            }
            Err(reason) => keep(reason),
        }
    };
    #[cfg(test)]
    tests::checkpoint("trashed");
    op.files[index].disposal = Some(disposal);
    op.files[index].reason = reason;
    save(&*lib.conn()?, op)
}
fn outcomes(op: &Operation) -> Vec<UnusedRemovalOutcome> {
    op.files
        .iter()
        .filter_map(|f| {
            Some(UnusedRemovalOutcome {
                id: f.entry.public.id.clone(),
                display_path: f.entry.public.display_path.clone(),
                disposal: f.disposal.clone()?,
                reason: f.reason.clone(),
            })
        })
        .collect()
}
fn cleanup(lib: &Library, op: &mut Operation, trash: &dyn FileTrash) -> Result<()> {
    if op.kind == "remove" && op.state == "committed" {
        let mut first = None;
        for index in 0..op.files.len() {
            if let Err(error) = dispose(lib, op, index, trash) {
                first.get_or_insert(error);
            }
        }
        op.result.outcomes = outcomes(op);
        if let Some(error) = first {
            return Err(error);
        }
    } else {
        // A rolled-back removal has its files back; a restore from an earlier version has
        // its temporaries to remove.
        let root = bytes_path(&op.root)?;
        for f in &op.files {
            let temp = bytes_path(&f.displaced)?;
            folders::guard_missing(&temp, &root)?;
            if missing(&temp)? {
                continue;
            }
            let expected = if op.kind == "remove" {
                Some(&f.entry.identity)
            } else {
                f.restored_identity.as_ref()
            };
            ensure!(
                expected == Some(&identity(&temp)?)
                    && cuerithm_media::file_sha256(&temp)? == f.entry.public.source_sha256,
                "Unknown recovery temporary preserved"
            );
            std::fs::remove_file(&temp)?;
            crate::publish_files::sync_parent(&temp)?;
        }
    }
    folders::cleanup(op)
}
fn finish(
    lib: &Library,
    op: &mut Operation,
    error: Option<&anyhow::Error>,
    trash: &dyn FileTrash,
) -> Result<()> {
    if let Some(error) = error {
        let root = bytes_path(&op.root)?;
        let mut errors = vec![];
        for f in op.files.iter().rev() {
            let restored = if op.kind == "remove" {
                rollback_remove(f, &root)
            } else {
                rollback_restore(f, &root)
            };
            if let Err(e) = restored {
                errors.push(format!("{e:#}"));
            }
        }
        op.state = if errors.is_empty() {
            "rolled_back"
        } else {
            "conflict"
        }
        .into();
        op.error = Some(if errors.is_empty() {
            format!("{error:#}")
        } else {
            format!("{error:#}; recovery needs attention: {}", errors.join("; "))
        });
        save(&*lib.conn()?, op)?;
        if op.state == "conflict" {
            return Ok(());
        }
    }
    match cleanup(lib, op, trash) {
        Ok(()) => {
            op.cleanup_complete = true;
            // A rollback keeps its reason; a removal that settled has nothing to report.
            if op.state == "committed" {
                op.error = None;
            }
        }
        Err(e) => {
            op.cleanup_complete = false;
            op.error = Some(format!("Recovery cleanup pending: {e:#}"));
        }
    }
    save(&*lib.conn()?, op)?;
    if op.cleanup_complete {
        let _ = super::saved::prune(&*lib.conn()?);
    }
    Ok(())
}
impl Library {
    pub fn preview_unused_removal(
        &self,
        scan_id: &str,
        selected: &[String],
        cancel: &CancellationToken,
    ) -> Result<UnusedRemovalPreview> {
        self.preview_unused_removal_with(scan_id, selected, &SystemTrash, cancel)
    }
    /// As [`Library::preview_unused_removal`], with the Trash that decides which files would
    /// have to be deleted permanently (tests supply their own).
    pub fn preview_unused_removal_with(
        &self,
        scan_id: &str,
        selected: &[String],
        trash: &dyn FileTrash,
        cancel: &CancellationToken,
    ) -> Result<UnusedRemovalPreview> {
        cancel.check()?;
        ensure!(
            !selected.is_empty() && selected.len() <= 10000,
            "Select between 1 and 10000 reviewed files"
        );
        ensure!(
            selected.iter().collect::<BTreeSet<_>>().len() == selected.len(),
            "Repeated selected file"
        );
        let saved = self.removal_scan(scan_id, cancel)?;
        let root = bytes_path(&saved.root)?;
        let mut entries = vec![];
        let mut seen = BTreeSet::new();
        for id in selected {
            let index = saved
                .report
                .files
                .iter()
                .position(|f| &f.id == id)
                .context("Selected file is not in the scan")?;
            let path = bytes_path(&saved.files[index].path)?;
            if seen.insert(path_bytes(&path)) {
                let e = entry(&path, &root, UnusedRemovalRole::Selected, cancel)?;
                ensure!(
                    e.public.source_sha256 == saved.report.files[index].source_sha256,
                    "Selected file changed; scan again"
                );
                entries.push(e);
            }
            if let Some(sibling) = sibling(&path)?
                && seen.insert(path_bytes(&sibling))
            {
                entries.push(entry(&sibling, &root, UnusedRemovalRole::Sibling, cancel)?);
            }
        }
        let directories = folders::plan(&mut entries, &root, cancel)?;
        for e in &mut entries {
            e.public.permanent = !trash.can_trash(&bytes_path(&e.path)?);
        }
        let paths = entries
            .iter()
            .map(|e| bytes_path(&e.path))
            .collect::<Result<Vec<_>>>()?;
        let conn = self.conn()?;
        ensure!(
            super::scan::stamp(&conn)? == saved.report.stamp,
            "Library changed; scan again"
        );
        super::unused::ensure_unreferenced(&conn, &paths)?;
        let public = UnusedRemovalPreview {
            directories: folders::display(&directories)?,
            id: uuid::Uuid::new_v4().to_string(),
            scan_id: scan_id.into(),
            stamp: saved.report.stamp,
            size_bytes: entries.iter().try_fold(0u64, |sum, e| {
                sum.checked_add(e.public.size_bytes)
                    .context("Removal size overflow")
            })?,
            files: entries.iter().map(|e| e.public.clone()).collect(),
        };
        let preview = Preview {
            public: public.clone(),
            root: saved.root,
            entries,
            logic: super::scan::logic_identity(),
            consumed_by: None,
            directories,
        };
        cancel.check()?;
        conn.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'unused_removal_plan',?2,?3)",
            params![public.id, serde_json::to_string(&preview)?, crate::now()],
        )?;
        super::saved::prune(&conn)?;
        Ok(public)
    }
    pub fn apply_unused_removal(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<UnusedRemovalResult> {
        self.apply_unused_removal_with(id, &SystemTrash, cancel)
    }
    /// Moves the previewed files to the Trash. A file is deleted permanently only if the
    /// preview listed it as one the Trash cannot take, and only after the Trash refused it.
    pub fn apply_unused_removal_with(
        &self,
        id: &str,
        trash: &dyn FileTrash,
        cancel: &CancellationToken,
    ) -> Result<UnusedRemovalResult> {
        cancel.check()?;
        let mut preview: Preview = load(&*self.conn()?, id, "unused_removal_plan")?;
        ensure!(
            preview.consumed_by.is_none() && preview.logic == super::scan::logic_identity(),
            "Removal preview was consumed or its implementation changed"
        );
        self.removal_scan(&preview.public.scan_id, cancel)?;
        let root = bytes_path(&preview.root)?;
        validate_entries(&preview.entries, &root, cancel)?;
        folders::validate(&preview.directories, &root)?;
        let op_id = uuid::Uuid::new_v4().to_string();
        let mut files = vec![];
        for (i, e) in preview.entries.iter().enumerate() {
            let path = bytes_path(&e.path)?;
            files.push(MovedFile {
                entry: e.clone(),
                backup: vec![],
                displaced: path_bytes(
                    &path
                        .parent()
                        .unwrap()
                        .join(format!(".cuerithm-unused-{op_id}-{i}.tmp")),
                ),
                restored_identity: None,
                staged: None,
                disposal: None,
                reason: None,
            });
        }
        validate_entries(&preview.entries, &root, cancel)?;
        // Full scope proofs are rechecked outside the SQLite publication transaction.
        self.removal_scan(&preview.public.scan_id, cancel)?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut op = Operation {
            schema: "unused-removal-v3".into(),
            kind: "remove".into(),
            state: "prepared".into(),
            root: preview.root.clone(),
            workspace: vec![],
            files,
            result: UnusedRemovalResult {
                id: op_id.clone(),
                preview_id: id.into(),
                stamp: preview.public.stamp.clone(),
                files: preview.public.files.clone(),
                directories: preview.public.directories.clone(),
                size_bytes: preview.public.size_bytes,
                restored: false,
                undo_of: None,
                outcomes: vec![],
            },
            error: None,
            undone_by: None,
            cleanup_complete: false,
            directories: preview.directories.clone(),
            backups_disposed: false,
        };
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            ensure!(
                super::scan::stamp(&tx)? == preview.public.stamp,
                "Library changed; scan again"
            );
            super::unused::ensure_unreferenced(&tx, &entry_paths(&preview.entries)?)?;
            let current: Preview = load(&tx, id, "unused_removal_plan")?;
            ensure!(
                current.consumed_by.is_none(),
                "Removal preview already applied"
            );
            ensure_unused_unreserved(
                &tx,
                &op.files
                    .iter()
                    .map(|f| f.entry.path.clone())
                    .collect::<Vec<_>>(),
            )?;
            ensure_metadata_unreserved(&tx, &op.files)?;
            cancel.check()?;
            tx.execute(
                "INSERT INTO maintenance_scans VALUES(?1,'unused_removal_operation',?2,?3)",
                params![op_id, serde_json::to_string(&op)?, crate::now()],
            )?;
            tx.commit()?;
        }
        #[cfg(test)]
        tests::checkpoint("prepared");
        let outcome = (|| -> Result<()> {
            let mut conn = self.conn()?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            ensure!(
                super::scan::stamp(&tx)? == preview.public.stamp,
                "Library changed before removal"
            );
            let current: Preview = load(&tx, id, "unused_removal_plan")?;
            ensure!(
                current.consumed_by.is_none(),
                "Removal preview already applied"
            );
            validate_entries(&preview.entries, &root, cancel)?;
            folders::validate(&preview.directories, &root)?;
            super::unused::ensure_unreferenced(&tx, &entry_paths(&preview.entries)?)?;
            for file in &op.files {
                cancel.check()?;
                let path = bytes_path(&file.entry.path)?;
                guard(&path, &root)?;
                ensure!(
                    identity(&path)? == file.entry.identity
                        && file_sha256_cancellable(&path, cancel)?
                            == file.entry.public.source_sha256,
                    "Removal source changed before exchange"
                );
                let displaced = bytes_path(&file.displaced)?;
                crate::publish_files::create(&path, &displaced)?;
                ensure!(
                    identity(&displaced)? == file.entry.identity
                        && file_sha256_cancellable(&displaced, cancel)?
                            == file.entry.public.source_sha256,
                    "Removal source changed during exchange"
                );
            }
            #[cfg(test)]
            tests::checkpoint("files");
            preview.consumed_by = Some(op_id.clone());
            op.state = "committed".into();
            op.result.stamp = super::merge::advance(&tx)?;
            save(&tx, &op)?;
            tx.execute("UPDATE maintenance_scans SET payload_json=?1 WHERE id=?2 AND kind='unused_removal_plan'",params![serde_json::to_string(&preview)?,id])?;
            cancel.check()?;
            tx.commit()?;
            #[cfg(test)]
            tests::checkpoint("committed");
            Ok(())
        })();
        finish(self, &mut op, outcome.as_ref().err(), trash)?;
        outcome?;
        Ok(op.result)
    }
    pub fn unused_removal_history(&self) -> Result<Vec<UnusedRemovalHistory>> {
        let conn = self.conn()?;
        let mut q = conn.prepare("SELECT payload_json FROM maintenance_scans WHERE kind='unused_removal_operation' ORDER BY created DESC,rowid DESC")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .map(|row| {
                let op: Operation = serde_json::from_str(&row?)?;
                Ok(UnusedRemovalHistory {
                    result: op.result,
                    state: op.state,
                    error: op.error,
                    undone_by: op.undone_by,
                    cleanup_pending: !op.cleanup_complete,
                })
            })
            .collect()
    }
    pub fn retry_unused_removal_recovery(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<UnusedRemovalHistory>> {
        self.retry_unused_removal_recovery_with(id, &SystemTrash, cancel)
    }
    pub fn retry_unused_removal_recovery_with(
        &self,
        id: &str,
        trash: &dyn FileTrash,
        cancel: &CancellationToken,
    ) -> Result<Vec<UnusedRemovalHistory>> {
        cancel.check()?;
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut op = load_operation(self, id)?;
        let error = anyhow::anyhow!("Explicit unused-file recovery retry");
        let pending = matches!(op.state.as_str(), "prepared" | "conflict");
        finish(self, &mut op, pending.then_some(&error), trash)?;
        self.unused_removal_history()
    }
    /// Run when the library opens: puts back what an interrupted removal had moved aside. A
    /// removal that had already committed is finished by [`Library::resume_unused_removals`],
    /// which needs the Trash.
    pub(crate) fn recover_unused_removals(&self) -> Result<()> {
        let ids: Vec<String> = {
            let conn = self.conn()?;
            let mut q = conn.prepare("SELECT id FROM maintenance_scans WHERE kind='unused_removal_operation' AND (json_extract(payload_json,'$.state')='prepared' OR (json_extract(payload_json,'$.cleanup_complete')=0 AND json_extract(payload_json,'$.state') IN ('undone','rolled_back'))) ORDER BY created,rowid")?;
            q.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in ids {
            let mut op = match load_operation(self, &id) {
                Ok(op) => op,
                Err(error) => {
                    let mut raw: Operation = load(&*self.conn()?, &id, "unused_removal_operation")?;
                    if raw.state == "prepared" {
                        raw.state = "conflict".into();
                    }
                    raw.cleanup_complete = false;
                    raw.error = Some(format!("Recovery assets need attention: {error:#}"));
                    save(&*self.conn()?, &raw)?;
                    continue;
                }
            };
            let error = anyhow::anyhow!("Interrupted unused-file operation rolled back");
            let pending = op.state == "prepared";
            finish(self, &mut op, pending.then_some(&error), &NoTrash)?;
        }
        Ok(())
    }
    /// Finishes the removals that committed but were interrupted before every file reached the
    /// Trash, then moves out of the data folder the copies that earlier versions kept. The host
    /// runs it after the library has opened. What it could not finish is tried again next time.
    pub fn resume_unused_removals(&self, trash: &dyn FileTrash) -> Result<()> {
        let ids: Vec<String> = {
            let conn = self.conn()?;
            let mut q = conn.prepare("SELECT id FROM maintenance_scans WHERE kind='unused_removal_operation' AND json_extract(payload_json,'$.cleanup_complete')=0 AND json_extract(payload_json,'$.state')='committed' ORDER BY created,rowid")?;
            q.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        {
            #[cfg(feature = "autotag")]
            let _gate = self
                .inner
                .tag_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
            for id in ids {
                if let Ok(mut op) = load_operation(self, &id) {
                    finish(self, &mut op, None, trash)?;
                }
            }
        }
        legacy::retire(self, trash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    thread_local! { static CRASH: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
    pub(super) fn checkpoint(phase: &str) {
        CRASH.with(|slot| {
            if *slot.borrow() == Some(phase) {
                slot.take();
                panic!("simulated unused interruption: {phase}");
            }
        });
    }
    fn crash_at(phase: &'static str) {
        CRASH.with(|slot| *slot.borrow_mut() = Some(phase));
    }
    thread_local! { static REPLACE_AFTER_CHECK: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
    pub(super) fn after_restore_check(path: &Path) {
        REPLACE_AFTER_CHECK.with(|slot| {
            if let Some(witness) = slot.take() {
                std::fs::rename(path, witness).unwrap();
                std::fs::write(path, b"new user file during rollback").unwrap();
            }
        });
    }
    thread_local! { pub(super) static NOTE_BEFORE_FOLDER_CLEANUP: std::cell::RefCell<bool> = const {std::cell::RefCell::new(false)}; }
    pub(super) fn before_folder_cleanup(op: &Operation) {
        NOTE_BEFORE_FOLDER_CLEANUP.with(|flag| {
            if flag.replace(false) {
                let path = PathBuf::from(&op.result.directories[0]).join("new-user-note.txt");
                std::fs::write(path, b"new user contents").unwrap();
            }
        });
    }

    /// A Trash that is a folder in the test's own temporary directory.
    pub(super) struct Bin {
        pub folder: PathBuf,
        refuse: Mutex<BTreeSet<String>>,
        unavailable: Mutex<BTreeSet<String>>,
        taken: Mutex<Vec<String>>,
        deleted: Mutex<Vec<String>>,
    }
    impl Bin {
        pub fn new(folder: PathBuf) -> Self {
            std::fs::create_dir_all(&folder).unwrap();
            Self {
                folder,
                refuse: Mutex::default(),
                unavailable: Mutex::default(),
                taken: Mutex::default(),
                deleted: Mutex::default(),
            }
        }
        pub fn refuse(&self, name: &str) {
            self.refuse.lock().unwrap().insert(name.into());
        }
        pub fn accept_again(&self, name: &str) {
            self.refuse.lock().unwrap().remove(name);
        }
        pub fn unavailable(&self, name: &str) {
            self.unavailable.lock().unwrap().insert(name.into());
        }
        pub fn taken(&self) -> Vec<String> {
            self.taken.lock().unwrap().clone()
        }
        pub fn deleted(&self) -> Vec<String> {
            self.deleted.lock().unwrap().clone()
        }
        pub fn content(&self, name: &str) -> Vec<u8> {
            std::fs::read(self.folder.join(name)).unwrap()
        }
    }
    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }
    impl FileTrash for Bin {
        fn can_trash(&self, path: &Path) -> bool {
            !self.unavailable.lock().unwrap().contains(&name_of(path))
        }
        fn move_to_trash(&self, path: &Path) -> std::result::Result<(), String> {
            let name = name_of(path);
            if self.refuse.lock().unwrap().contains(&name) {
                return Err("The Trash is not available".into());
            }
            std::fs::rename(path, self.folder.join(&name)).map_err(|e| e.to_string())?;
            self.taken.lock().unwrap().push(name);
            Ok(())
        }
        fn delete_permanently(&self, path: &Path) -> std::result::Result<(), String> {
            let name = name_of(path);
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
            self.deleted.lock().unwrap().push(name);
            Ok(())
        }
    }

    struct Setup {
        dir: tempfile::TempDir,
        lib: Library,
        file: PathBuf,
        plan: UnusedRemovalPreview,
        bin: Bin,
    }
    fn scan(lib: &Library, root: PathBuf) -> UnusedScanReport {
        lib.scan_unused_files(
            UnusedScanRequest {
                root,
                extension_mode: ExtensionMode::All,
                extensions: vec![],
            },
            &CancellationToken::default(),
        )
        .unwrap()
    }
    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("music");
        std::fs::create_dir(&root).unwrap();
        let file = root.join("authored.flac");
        std::fs::write(&file, b"authored source").unwrap();
        let lib = Library::open(&dir.path().join("app/db")).unwrap();
        let bin = Bin::new(dir.path().join("bin"));
        let found = scan(&lib, root);
        let plan = lib
            .preview_unused_removal_with(
                &found.id,
                &[found.files[0].id.clone()],
                &bin,
                &CancellationToken::default(),
            )
            .unwrap();
        Setup {
            dir,
            lib,
            file,
            plan,
            bin,
        }
    }
    fn nested_setup() -> Setup {
        let mut s = setup();
        let nested = s.file.parent().unwrap().join("album");
        std::fs::create_dir(&nested).unwrap();
        let path = nested.join("authored.flac");
        std::fs::rename(&s.file, &path).unwrap();
        let found = scan(&s.lib, nested.parent().unwrap().to_owned());
        s.plan = s
            .lib
            .preview_unused_removal_with(
                &found.id,
                &[found.files[0].id.clone()],
                &s.bin,
                &CancellationToken::default(),
            )
            .unwrap();
        s.file = path;
        s
    }
    fn apply(s: &Setup) -> Result<UnusedRemovalResult> {
        s.lib
            .apply_unused_removal_with(&s.plan.id, &s.bin, &CancellationToken::default())
    }
    fn crashed_apply(s: &Setup, phase: &'static str) {
        crash_at(phase);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply(s))).is_err(),
            "the interruption at {phase} must happen"
        );
    }
    fn reopen(s: Setup) -> Setup {
        let db = s.dir.path().join("app/db");
        let Setup {
            dir,
            lib,
            file,
            plan,
            bin,
        } = s;
        drop(lib);
        let lib = Library::open(&db).unwrap();
        Setup {
            dir,
            lib,
            file,
            plan,
            bin,
        }
    }
    fn history(lib: &Library, id: &str) -> UnusedRemovalHistory {
        lib.unused_removal_history()
            .unwrap()
            .into_iter()
            .find(|h| h.result.id == id)
            .unwrap()
    }
    fn operation(lib: &Library) -> Operation {
        let id = lib.unused_removal_history().unwrap()[0].result.id.clone();
        load_operation(lib, &id).unwrap()
    }

    #[test]
    fn the_files_go_to_the_trash_under_their_own_names_and_the_library_keeps_no_copy() {
        let s = setup();
        let applied = apply(&s).unwrap();
        assert!(!s.file.exists());
        assert_eq!(s.bin.taken(), ["authored.flac"]);
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert_eq!(applied.outcomes.len(), 1);
        assert_eq!(applied.outcomes[0].disposal, UnusedRemovalDisposal::Trashed);
        // Nothing of the audio is in the data folder, and the journal names no copy.
        assert!(!s.lib.data_dir().join("maintenance/removal").exists());
        let op = operation(&s.lib);
        assert!(op.workspace.is_empty() && op.files[0].backup.is_empty());
        assert!(op.cleanup_complete && op.state == "committed");
        // The hidden temporary is gone too.
        let leftovers = std::fs::read_dir(s.file.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 0);
        assert_eq!(history(&s.lib, &applied.id).result.outcomes.len(), 1);
    }

    #[test]
    fn a_file_the_trash_refuses_stays_where_it_was_and_is_reported() {
        let s = setup();
        s.bin.refuse("authored.flac");
        let applied = apply(&s).unwrap();
        assert_eq!(std::fs::read(&s.file).unwrap(), b"authored source");
        assert!(s.bin.taken().is_empty() && s.bin.deleted().is_empty());
        assert_eq!(
            applied.outcomes[0].disposal,
            UnusedRemovalDisposal::LeftInPlace
        );
        assert!(
            applied.outcomes[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("Trash")
        );
        assert!(operation(&s.lib).cleanup_complete);
    }

    #[test]
    fn a_file_is_deleted_permanently_only_when_the_preview_said_so_and_the_trash_refused() {
        // The preview lists a file the Trash cannot take. A Trash that takes it anyway wins.
        let s = setup();
        s.bin.unavailable("authored.flac");
        let again = s
            .lib
            .preview_unused_removal_with(
                &s.plan.scan_id,
                &[scan_file_id(&s)],
                &s.bin,
                &CancellationToken::default(),
            )
            .unwrap();
        assert!(again.files[0].permanent);
        let applied = s
            .lib
            .apply_unused_removal_with(&again.id, &s.bin, &CancellationToken::default())
            .unwrap();
        assert_eq!(applied.outcomes[0].disposal, UnusedRemovalDisposal::Trashed);
        assert!(s.bin.deleted().is_empty());

        // The same file, refused: deleted, because it was announced.
        let s = setup();
        s.bin.unavailable("authored.flac");
        s.bin.refuse("authored.flac");
        let again = s
            .lib
            .preview_unused_removal_with(
                &s.plan.scan_id,
                &[scan_file_id(&s)],
                &s.bin,
                &CancellationToken::default(),
            )
            .unwrap();
        let applied = s
            .lib
            .apply_unused_removal_with(&again.id, &s.bin, &CancellationToken::default())
            .unwrap();
        assert_eq!(applied.outcomes[0].disposal, UnusedRemovalDisposal::Deleted);
        assert_eq!(s.bin.deleted(), ["authored.flac"]);
        assert!(!s.file.exists());

        // A file the preview did not announce is never deleted, whatever the Trash does.
        let s = setup();
        assert!(!s.plan.files[0].permanent);
        s.bin.refuse("authored.flac");
        apply(&s).unwrap();
        assert!(s.bin.deleted().is_empty() && s.file.exists());
    }
    fn scan_file_id(s: &Setup) -> String {
        let found = scan(&s.lib, s.file.parent().unwrap().to_owned());
        found.files[0].id.clone()
    }

    #[test]
    fn a_removal_interrupted_after_it_committed_is_finished_when_the_host_resumes() {
        for phase in ["committed", "staged", "named", "trashed"] {
            let s = setup();
            crashed_apply(&s, phase);
            let before = s.bin.taken().len();
            let s = reopen(s);
            // Opening the library alone never touches the Trash.
            assert_eq!(s.bin.taken().len(), before, "{phase}");
            s.lib.resume_unused_removals(&s.bin).unwrap();
            assert!(!s.file.exists(), "{phase}");
            let op = operation(&s.lib);
            assert!(op.cleanup_complete, "{phase}");
            // The file is in the Trash exactly once, whichever step stopped.
            let in_bin = std::fs::read_dir(&s.bin.folder).unwrap().count();
            assert_eq!(in_bin, 1, "{phase}");
            assert_eq!(s.bin.content("authored.flac"), b"authored source");
            // A second resume has nothing to do.
            s.lib.resume_unused_removals(&s.bin).unwrap();
            assert_eq!(std::fs::read_dir(&s.bin.folder).unwrap().count(), 1);
        }
    }

    #[test]
    fn a_name_taken_while_the_removal_waited_gives_the_removed_file_a_spare_name() {
        let s = setup();
        crashed_apply(&s, "committed");
        let s = reopen(s);
        std::fs::write(&s.file, b"a new file with the same name").unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(
            std::fs::read(&s.file).unwrap(),
            b"a new file with the same name"
        );
        assert_eq!(s.bin.taken(), ["authored (removed 1).flac"]);
        assert_eq!(
            s.bin.content("authored (removed 1).flac"),
            b"authored source"
        );
    }

    #[test]
    fn a_file_edited_between_its_return_and_the_trash_is_left_alone() {
        let s = setup();
        crashed_apply(&s, "named");
        let s = reopen(s);
        // It is back under its own name; the user edits it in place before the Trash is asked.
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&s.file)
            .unwrap()
            .write_all(b" edited")
            .unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert!(s.bin.taken().is_empty());
        assert_eq!(std::fs::read(&s.file).unwrap(), b"authored source edited");
        let op = operation(&s.lib);
        assert_eq!(
            op.files[0].disposal,
            Some(UnusedRemovalDisposal::LeftInPlace)
        );
        assert!(op.cleanup_complete);
    }

    #[test]
    fn a_new_file_under_the_old_name_after_the_trash_took_the_removed_one_is_left_alone() {
        let s = setup();
        crashed_apply(&s, "trashed");
        let s = reopen(s);
        std::fs::write(&s.file, b"a new file with the same name").unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(
            std::fs::read(&s.file).unwrap(),
            b"a new file with the same name"
        );
        assert_eq!(
            s.bin.taken().len(),
            1,
            "the new file never reached the Trash"
        );
        let op = operation(&s.lib);
        assert_eq!(op.files[0].disposal, Some(UnusedRemovalDisposal::Gone));
        assert!(op.cleanup_complete);
    }

    #[test]
    fn a_retry_after_a_refusal_tries_the_trash_again_only_for_what_is_still_pending() {
        let s = setup();
        crashed_apply(&s, "named");
        let s = reopen(s);
        s.bin.refuse("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(std::fs::read(&s.file).unwrap(), b"authored source");
        // A refusal is an answer: the file stays, and the removal is settled.
        assert!(operation(&s.lib).cleanup_complete);
        s.bin.accept_again("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert!(s.bin.taken().is_empty() && s.file.exists());
    }

    #[test]
    fn rollback_claims_its_restored_file_before_deleting_and_preserves_a_racing_user_replacement() {
        let s = setup();
        crashed_apply(&s, "files");
        let s = reopen(s);
        let op = operation(&s.lib);
        let path = s.file.clone();
        let mut f = op.files[0].clone();
        // A restore from an earlier version had put its copy back under the original name.
        std::fs::write(&path, b"authored source").unwrap();
        f.restored_identity = Some(identity(&path).unwrap());
        let witness = s.dir.path().join("restored-version");
        REPLACE_AFTER_CHECK.with(|slot| *slot.borrow_mut() = Some(witness.clone()));
        assert!(rollback_restore(&f, &bytes_path(&op.root).unwrap()).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"new user file during rollback"
        );
        assert_eq!(std::fs::read(&witness).unwrap(), b"authored source");
    }

    #[test]
    fn unresolved_recovery_reserves_its_source_against_a_second_reviewed_removal() {
        let s = setup();
        crashed_apply(&s, "files");
        let file = s.file.clone();
        // The user saves a new file under the name while the interrupted removal waits.
        std::fs::write(&file, b"new user source").unwrap();
        let s = reopen(s);
        let found = scan(&s.lib, file.parent().unwrap().to_owned());
        let token = CancellationToken::default();
        let next = s
            .lib
            .preview_unused_removal_with(&found.id, &[found.files[0].id.clone()], &s.bin, &token)
            .unwrap();
        assert!(
            s.lib
                .apply_unused_removal_with(&next.id, &s.bin, &token)
                .is_err()
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"new user source");
    }

    #[test]
    fn new_directory_contents_end_cleanup_without_scheduling_their_future_deletion() {
        let s = nested_setup();
        NOTE_BEFORE_FOLDER_CLEANUP.with(|flag| *flag.borrow_mut() = true);
        let applied = apply(&s).unwrap();
        assert!(!s.file.exists());
        assert_eq!(
            std::fs::read(s.file.parent().unwrap().join("new-user-note.txt")).unwrap(),
            b"new user contents"
        );
        assert!(!history(&s.lib, &applied.id).cleanup_pending);
    }

    #[test]
    fn the_folder_goes_with_its_files_only_once_they_are_all_gone() {
        let s = nested_setup();
        apply(&s).unwrap();
        assert!(!s.file.parent().unwrap().exists());
        // A file the Trash refused keeps its folder.
        let s = nested_setup();
        s.bin.refuse("authored.flac");
        apply(&s).unwrap();
        assert!(s.file.exists());
    }

    #[test]
    fn interrupted_unused_removal_restores_prepared_and_moved_files_on_restart() {
        for phase in ["prepared", "files"] {
            let s = setup();
            crashed_apply(&s, phase);
            let s = reopen(s);
            assert_eq!(std::fs::read(&s.file).unwrap(), b"authored source");
            assert!(
                s.lib
                    .unused_removal_history()
                    .unwrap()
                    .iter()
                    .any(|h| h.state == "rolled_back")
            );
            assert!(s.bin.taken().is_empty());
        }
    }

    #[test]
    fn interrupted_unused_removal_preserves_new_user_files_as_visible_conflicts() {
        let s = setup();
        crashed_apply(&s, "files");
        let file = s.file.clone();
        std::fs::write(&file, b"new user version").unwrap();
        let s = reopen(s);
        assert_eq!(std::fs::read(&file).unwrap(), b"new user version");
        let conflict = s
            .lib
            .unused_removal_history()
            .unwrap()
            .into_iter()
            .find(|h| h.state == "conflict")
            .expect("the conflict is visible");
        assert!(conflict.error.is_some());
        std::fs::remove_file(&file).unwrap();
        let history = s
            .lib
            .retry_unused_removal_recovery_with(
                &conflict.result.id,
                &s.bin,
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"authored source");
        assert!(history.iter().any(|h| h.result.id == conflict.result.id
            && h.state == "rolled_back"
            && !h.cleanup_pending));
    }

    // Removals made by earlier versions kept a copy of every file in the data folder.

    /// Rewrites a finished removal as an earlier version left it: its file gone from the music
    /// folder and a copy in a folder of the data folder. Returns the copy.
    fn as_an_earlier_version_left_it(s: &Setup, id: &str, state: &str) -> PathBuf {
        let workspace = s.lib.data_dir().join("maintenance/removal/remove-legacy");
        std::fs::create_dir_all(&workspace).unwrap();
        let copy = workspace.join("original-0");
        std::fs::write(&copy, b"authored source").unwrap();
        let mut op = load_operation(&s.lib, id).unwrap();
        op.schema = "unused-removal-v2".into();
        op.state = state.into();
        if state == "undone" {
            op.undone_by = Some("later-restore".into());
        }
        op.workspace = path_bytes(&workspace.canonicalize().unwrap());
        op.files[0].backup = path_bytes(&copy.canonicalize().unwrap());
        op.files[0].disposal = None;
        op.files[0].staged = None;
        op.result.outcomes = vec![];
        op.cleanup_complete = true;
        op.backups_disposed = false;
        save(&s.lib.conn().unwrap(), &op).unwrap();
        copy
    }

    #[test]
    fn the_copy_of_a_removal_that_still_stands_goes_to_the_trash_under_the_files_own_name() {
        let s = setup();
        let applied = apply(&s).unwrap();
        // The Trash now holds the file; the earlier version had deleted it instead.
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert!(!copy.exists() && !copy.parent().unwrap().exists());
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        let op = load_operation(&s.lib, &applied.id).unwrap();
        assert!(op.backups_disposed && op.workspace.is_empty() && op.files[0].backup.is_empty());
        assert_eq!(op.files[0].disposal, Some(UnusedRemovalDisposal::Trashed));
        assert_eq!(s.lib.legacy_originals_removed().unwrap().0, 1);
        // A second start has nothing left to do.
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.taken().len(), 2);
    }

    #[test]
    fn a_retry_never_hands_one_files_copy_to_another_file_with_the_same_content() {
        // Two files with identical bytes: the Trash takes the first copy and refuses the second.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("music");
        std::fs::create_dir(&root).unwrap();
        for name in ["a.flac", "b.flac"] {
            std::fs::write(root.join(name), b"same bytes").unwrap();
        }
        let lib = Library::open(&dir.path().join("app/db")).unwrap();
        let bin = Bin::new(dir.path().join("bin"));
        let found = scan(&lib, root);
        let ids: Vec<String> = found.files.iter().map(|f| f.id.clone()).collect();
        let token = CancellationToken::default();
        let plan = lib
            .preview_unused_removal_with(&found.id, &ids, &bin, &token)
            .unwrap();
        let applied = lib
            .apply_unused_removal_with(&plan.id, &bin, &token)
            .unwrap();
        for name in ["a.flac", "b.flac"] {
            std::fs::remove_file(bin.folder.join(name)).unwrap();
        }
        // Rewrite it as an earlier version left it: two copies in the data folder.
        let workspace = lib.data_dir().join("maintenance/removal/remove-legacy");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut op = load_operation(&lib, &applied.id).unwrap();
        op.schema = "unused-removal-v2".into();
        op.workspace = path_bytes(&workspace.canonicalize().unwrap());
        for (index, f) in op.files.iter_mut().enumerate() {
            let copy = workspace.join(format!("original-{index}"));
            std::fs::write(&copy, b"same bytes").unwrap();
            f.backup = path_bytes(&copy.canonicalize().unwrap());
            f.disposal = None;
            f.staged = None;
        }
        save(&lib.conn().unwrap(), &op).unwrap();
        let second = bytes_path(&op.files[1].entry.path)
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let already = bin.taken().len();
        bin.refuse(&second);
        lib.resume_unused_removals(&bin).unwrap();
        assert_eq!(bin.taken().len(), already + 1);
        bin.accept_again(&second);
        lib.resume_unused_removals(&bin).unwrap();
        // Both copies are in the Trash, none was deleted for lack of a file.
        assert_eq!(bin.taken().len(), already + 2);
        let op = load_operation(&lib, &applied.id).unwrap();
        assert!(op.backups_disposed);
        assert!(
            op.files
                .iter()
                .all(|f| f.disposal == Some(UnusedRemovalDisposal::Trashed))
        );
    }

    #[test]
    fn the_copy_of_a_removal_that_was_restored_is_deleted_and_not_trashed() {
        let s = setup();
        let applied = apply(&s).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "undone");
        let taken = s.bin.taken().len();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert!(!copy.exists());
        assert_eq!(s.bin.taken().len(), taken);
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
    }

    #[test]
    fn a_copy_the_trash_refuses_stays_for_the_next_start_and_nothing_is_lost() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        s.bin.refuse("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"authored source");
        let op = load_operation(&s.lib, &applied.id).unwrap();
        assert!(!op.backups_disposed);
        s.bin.accept_again("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
    }

    #[test]
    fn a_copy_that_an_interrupted_disposal_renamed_is_still_found_by_its_content() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let held = copy.with_file_name(".dispose-abc-0");
        std::fs::rename(&copy, &held).unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(!held.exists());
    }

    #[test]
    fn a_link_in_the_removal_folder_is_never_followed() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        std::fs::remove_file(&copy).unwrap();
        let outside = s.dir.path().join("outside.flac");
        std::fs::write(&outside, b"authored source").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &copy).unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(std::fs::read(&outside).unwrap(), b"authored source");
        assert!(
            s.bin.taken().len() == 1,
            "only the first removal reached the Trash"
        );
    }

    /// A Trash that never answers: the app exits while the call is running.
    struct Dies;
    impl FileTrash for Dies {
        fn move_to_trash(&self, _: &Path) -> std::result::Result<(), String> {
            panic!("simulated exit during the Trash call");
        }
    }
    fn hash_of(lib: &Library, id: &str) -> String {
        load_operation(lib, id).unwrap().files[0]
            .entry
            .public
            .source_sha256
            .clone()
    }

    #[test]
    fn a_copy_an_interrupted_start_left_in_its_trash_holder_is_found_and_trashed() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let workspace = copy.parent().unwrap().to_owned();
        // The app exits while the Trash call runs: the copy has been renamed into its holder.
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                legacy::retire_with(&s.lib, &Dies, 50)
            }))
            .is_err()
        );
        let holder = workspace.join(format!("trash-{}", &hash_of(&s.lib, &applied.id)[..16]));
        assert!(holder.join("authored.flac").is_file() && !copy.exists());
        // The next start finds it there.
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        let op = load_operation(&s.lib, &applied.id).unwrap();
        assert!(op.backups_disposed && op.workspace.is_empty());
        assert_eq!(op.files[0].disposal, Some(UnusedRemovalDisposal::Trashed));
        assert!(!workspace.exists(), "the holder and the folder are gone");
    }

    #[test]
    fn a_start_interrupted_again_in_the_holder_loses_nothing() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let workspace = copy.parent().unwrap().to_owned();
        for _ in 0..3 {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    legacy::retire_with(&s.lib, &Dies, 50)
                }))
                .is_err()
            );
        }
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(!workspace.exists());
    }

    #[test]
    fn a_holder_copy_the_trash_refuses_stays_in_its_holder_for_the_next_start() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let workspace = copy.parent().unwrap().to_owned();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                legacy::retire_with(&s.lib, &Dies, 50)
            }))
            .is_err()
        );
        s.bin.refuse("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert!(
            !load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
        s.bin.accept_again("authored.flac");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(!workspace.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_removal_folder_that_cannot_be_read_is_left_for_the_next_start() {
        use std::os::unix::fs::PermissionsExt;
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let workspace = copy.parent().unwrap().to_owned();
        // Searchable but not listable: the files can be reached by name, the folder cannot be read.
        std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o333)).unwrap();
        if std::fs::read_dir(&workspace).is_ok() {
            // A privileged user ignores the mode: there is nothing to see.
            std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let resumed = s.lib.resume_unused_removals(&s.bin);
        std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o755)).unwrap();
        resumed.unwrap();
        let op = load_operation(&s.lib, &applied.id).unwrap();
        assert!(!op.backups_disposed && !op.workspace.is_empty());
        assert_eq!(op.files[0].disposal, None, "not declared gone");
        assert!(copy.exists() && s.bin.taken().len() == 1);
        // Readable again at the next start: the copy goes to the Trash.
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_that_cannot_be_read_leaves_the_removal_for_the_next_start() {
        use std::os::unix::fs::PermissionsExt;
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&copy).is_ok() {
            std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        let resumed = s.lib.resume_unused_removals(&s.bin);
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)).unwrap();
        resumed.unwrap();
        let op = load_operation(&s.lib, &applied.id).unwrap();
        assert!(!op.backups_disposed && !op.workspace.is_empty());
        assert_eq!(op.files[0].disposal, None, "not declared gone");
        assert!(copy.exists());
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
    }

    #[test]
    fn a_file_in_a_standing_removals_folder_that_matches_nothing_goes_to_the_trash_not_away() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        let copy = as_an_earlier_version_left_it(&s, &applied.id, "committed");
        let workspace = copy.parent().unwrap().to_owned();
        // A damaged copy: its bytes no longer match any file of the removal.
        std::fs::write(workspace.join("original-7"), b"a damaged copy").unwrap();
        s.bin.refuse("original-7");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        // The Trash refused it: nothing is deleted and the removal waits.
        assert!(workspace.join("original-7").is_file());
        assert!(
            !load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
        s.bin.accept_again("original-7");
        s.lib.resume_unused_removals(&s.bin).unwrap();
        assert_eq!(s.bin.content("original-7"), b"a damaged copy");
        assert_eq!(s.bin.content("authored.flac"), b"authored source");
        assert!(s.bin.deleted().is_empty());
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
        assert!(!workspace.exists());
    }

    #[test]
    fn only_so_many_earlier_removals_are_handled_per_start() {
        let s = setup();
        let applied = apply(&s).unwrap();
        std::fs::remove_file(s.bin.folder.join("authored.flac")).unwrap();
        as_an_earlier_version_left_it(&s, &applied.id, "committed");
        legacy::retire_with(&s.lib, &s.bin, 0).unwrap();
        assert!(
            !load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
        legacy::retire_with(&s.lib, &s.bin, 1).unwrap();
        assert!(
            load_operation(&s.lib, &applied.id)
                .unwrap()
                .backups_disposed
        );
    }

    #[test]
    fn an_earlier_disposal_journal_left_half_done_is_dropped_so_it_cannot_block_a_backup() {
        let s = setup();
        s.lib
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO maintenance_scans VALUES('d','unused_backup_disposal','{\"result\":{\"state\":\"prepared\"}}',1)",
                [],
            )
            .unwrap();
        s.lib.resume_unused_removals(&s.bin).unwrap();
        let left: i64 = s
            .lib
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM maintenance_scans WHERE kind LIKE 'unused_backup_disposal%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }
}
