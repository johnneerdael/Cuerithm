use super::{
    files::{FileProof, proofs_current, resolved_prefix},
    *,
};
use crate::{
    Library,
    model::{bytes_path, path_bytes},
};
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256_cancellable};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
struct DirectoryProof {
    path: Vec<u8>,
    entries: Vec<(Vec<u8>, u8)>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct SavedUnused {
    pub(super) report: UnusedScanReport,
    pub(super) root: Vec<u8>,
    pub(super) files: Vec<FileProof>,
    directories: Vec<DirectoryProof>,
    references: Vec<(Vec<u8>, Option<u64>)>,
    logic: String,
}
fn handle_key(handle: &same_file::Handle) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    handle.hash(&mut hash);
    hash.finish()
}
fn reference_key(path: &Path) -> Option<u64> {
    same_file::Handle::from_path(path)
        .ok()
        .as_ref()
        .map(handle_key)
}
fn referenced(
    path: &Path,
    paths: &BTreeSet<PathBuf>,
    keys: &BTreeMap<u64, Vec<PathBuf>>,
) -> Result<bool> {
    if paths.contains(path) {
        return Ok(true);
    }
    let handle = same_file::Handle::from_path(path)?;
    if let Some(possible) = keys.get(&handle_key(&handle)) {
        for reference in possible {
            let other = same_file::Handle::from_path(reference)
                .context("Cannot recheck a possible library reference")?;
            if handle == other {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn broad_root(path: &Path, home: Option<&Path>) -> bool {
    if path.parent().is_none() || home == Some(path) {
        return true;
    }
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let text = if cfg!(windows) && text.as_bytes().get(1) == Some(&b':') {
        &text[2..]
    } else {
        text
    };
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        matches!(
            text.to_ascii_lowercase().as_str(),
            "" | "/users" | "/windows" | "/program files" | "/program files (x86)" | "/programdata"
        )
    } else {
        matches!(
            text,
            "" | "/Users"
                | "/Volumes"
                | "/Applications"
                | "/System"
                | "/Library"
                | "/private"
                | "/private/var"
                | "/var"
                | "/etc"
                | "/private/etc"
                | "/usr"
                | "/bin"
                | "/sbin"
                | "/opt"
                | "/home"
                | "/media"
                | "/mnt"
                | "/run"
                | "/tmp"
                | "/private/tmp"
        )
    }
}
fn directory(path: &Path) -> Result<Vec<(PathBuf, u8)>> {
    ensure!(
        std::fs::symlink_metadata(path)?.is_dir(),
        "Directory changed or became a symlink"
    );
    let mut rows = vec![];
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        rows.push((
            entry.path(),
            if kind.is_symlink() {
                3
            } else if kind.is_dir() {
                2
            } else if kind.is_file() {
                1
            } else {
                0
            },
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(rows)
}
fn excluded(path: &Path, root: &Path, app: &Path, known: &[PathBuf], is_directory: bool) -> bool {
    if is_directory
        && path.file_name().is_some_and(|n| {
            let n = n.to_string_lossy();
            n.starts_with(".Spotlight")
                || (cfg!(target_os = "macos") && n == "Application Support")
                || matches!(
                    n.as_ref(),
                    ".AppleDouble"
                        | "$RECYCLE.BIN"
                        | "$Recycle.Bin"
                        | "AppData"
                        | ".Trash"
                        | ".Trashes"
                        | ".fseventsd"
                )
        })
    {
        return true;
    }
    if path.starts_with(app)
        || known.iter().any(|p| path.starts_with(p))
        || (is_directory
            && std::iter::once(app)
                .chain(known.iter().map(PathBuf::as_path))
                .any(|folder| same_file::is_same_file(path, folder).unwrap_or(false)))
    {
        return true;
    }
    let parts = path
        .components()
        .map(|p| p.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    let ancestors = if is_directory {
        parts.as_slice()
    } else {
        &parts[..parts.len().saturating_sub(1)]
    };
    if ancestors.iter().any(|p| p == "_Serato_") {
        return true;
    }
    // A selected narrow music folder may live under Windows AppData/Temp or
    // macOS Application Support. Prune reserved names inside that chosen tree;
    // absolute app/system storage and Serato ancestry remain protected above.
    let relative = path.strip_prefix(root).unwrap_or(path);
    let parts = relative
        .components()
        .map(|p| p.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    let scoped = if is_directory {
        parts.as_slice()
    } else {
        &parts[..parts.len().saturating_sub(1)]
    };
    if cfg!(target_os = "macos") && scoped.iter().any(|p| p == "Application Support") {
        return true;
    }
    if cfg!(windows) && scoped.iter().any(|p| p.eq_ignore_ascii_case("AppData")) {
        return true;
    }
    !is_directory
        && path.file_name().is_some_and(|n| {
            let n = n.to_string_lossy();
            n.ends_with(".DS_Store")
                || n.starts_with("._")
                || n.starts_with(".cuerithm-unused-")
                || n.starts_with(".cuerithm-restore-")
        })
}
fn sidecar(path: &Path, all: &[PathBuf]) -> bool {
    let text = path.to_string_lossy();
    if let Some(base) = text.strip_suffix(".vdjstems") {
        return all.iter().any(|p| p == Path::new(base));
    }
    if let Some(base) = text.strip_suffix(".serato-stems") {
        let b = base.as_bytes();
        if b.len() >= 4 && &b[b.len() - 4..b.len() - 1] == b".1." && b[b.len() - 1].is_ascii_digit()
        {
            let prefix = &base[..base.len() - 4];
            return all
                .iter()
                .filter(|p| p.to_string_lossy().starts_with(prefix))
                .take(2)
                .count()
                > 1;
        }
    }
    false
}
fn normalize_extensions(values: &[String]) -> Result<BTreeSet<String>> {
    ensure!(values.len() <= 1000, "Too many extension filters");
    let mut out = BTreeSet::new();
    for value in values {
        let value = value.trim().trim_start_matches('.').to_ascii_lowercase();
        ensure!(
            !value.is_empty()
                && value.len() <= 32
                && value.bytes().all(|b| b.is_ascii_alphanumeric()),
            "Invalid file extension"
        );
        out.insert(value);
    }
    Ok(out)
}
fn allowed(path: &Path, mode: ExtensionMode, extensions: &BTreeSet<String>) -> bool {
    let matched = path
        .extension()
        .is_some_and(|e| extensions.contains(&e.to_string_lossy().to_ascii_lowercase()));
    match mode {
        ExtensionMode::All => true,
        ExtensionMode::Include => matched,
        ExtensionMode::Exclude => !matched,
    }
}
fn stale(report: &mut UnusedScanReport, reason: &str) {
    report.stale = true;
    report.stale_reason = Some(reason.into());
}
impl Library {
    pub fn scan_unused_files(
        &self,
        request: UnusedScanRequest,
        cancel: &CancellationToken,
    ) -> Result<UnusedScanReport> {
        self.scan_unused_files_with_progress(request, cancel, |_| {})
    }
    pub fn scan_unused_files_with_progress(
        &self,
        request: UnusedScanRequest,
        cancel: &CancellationToken,
        mut progress: impl FnMut(FileScanProgress),
    ) -> Result<UnusedScanReport> {
        cancel.check()?;
        ensure!(request.root.is_absolute(), "Choose an absolute folder");
        ensure!(
            !std::fs::symlink_metadata(&request.root)?
                .file_type()
                .is_symlink(),
            "Choose the actual folder, not a directory symlink"
        );
        let root = request.root.canonicalize()?;
        ensure!(
            root.is_dir() && root.parent().is_some(),
            "Choose a music folder, not a filesystem root"
        );
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .and_then(|p| p.canonicalize().ok());
        ensure!(
            !broad_root(&root, home.as_deref())
                && !home
                    .as_ref()
                    .is_some_and(|home| same_file::is_same_file(&root, home).unwrap_or(false)),
            "Choose a narrower music folder; system and home roots are excluded"
        );
        let extensions = if request.extension_mode == ExtensionMode::All {
            BTreeSet::new()
        } else {
            normalize_extensions(&request.extensions)?
        };
        let app = self.data_dir().canonicalize()?;
        let mut known = vec![];
        if let Some(home) = home {
            for name in [
                "_Serato_",
                "PioneerDJ",
                "Traktor",
                "Engine Library",
                "iTunes",
            ] {
                known.push(home.join("Music").join(name));
            }
        }
        if cfg!(windows) {
            for key in ["WINDIR", "ProgramFiles", "ProgramData"] {
                if let Some(path) = std::env::var_os(key) {
                    known.push(resolved_prefix(Path::new(&path))?);
                }
            }
        }
        ensure!(
            !excluded(&root, &root, &app, &known, true),
            "Selected folder is protected application or DJ storage; choose a music folder"
        );
        let (stamp, paths) = {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let stamp = super::scan::stamp(&tx)?;
            let mut statement = tx.prepare("SELECT path FROM tracks WHERE path IS NOT NULL")?;
            let paths = statement
                .query_map([], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(statement);
            tx.commit()?;
            (stamp, paths)
        };
        let mut references = BTreeSet::new();
        let mut reference_keys: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
        let mut reference_proofs = vec![];
        for path in paths {
            cancel.check()?;
            let path = bytes_path(&path)?;
            let path = path.canonicalize().unwrap_or(path);
            let key = reference_key(&path);
            reference_proofs.push((path_bytes(&path), key));
            if let Some(key) = key {
                reference_keys.entry(key).or_default().push(path.clone());
            }
            references.insert(path);
        }
        let mut report = UnusedScanReport {
            id: uuid::Uuid::new_v4().to_string(),
            created: crate::now(),
            stamp,
            request,
            files: vec![],
            size_bytes: 0,
            complete: true,
            issues: vec![],
            stale: false,
            stale_reason: None,
        };
        let mut directories = vec![];
        let mut stack = vec![root.clone()];
        let mut inventory = vec![];
        while let Some(folder) = stack.pop() {
            cancel.check()?;
            if excluded(&folder, &root, &app, &known, true) {
                continue;
            }
            match directory(&folder) {
                Ok(rows) => {
                    directories.push(DirectoryProof {
                        path: path_bytes(&folder),
                        entries: rows.iter().map(|(p, k)| (path_bytes(p), *k)).collect(),
                    });
                    for (path, kind) in rows {
                        cancel.check()?;
                        if excluded(&path, &root, &app, &known, kind == 2) {
                            continue;
                        }
                        if kind == 2 {
                            stack.push(path);
                        } else if kind == 1 {
                            inventory.push(path);
                        }
                    }
                }
                Err(error) => {
                    report.complete = false;
                    report.issues.push(ScanIssue {
                        display_path: folder.to_string_lossy().into(),
                        message: error.to_string(),
                    });
                }
            }
            progress(FileScanProgress {
                completed: 0,
                total: None,
                current_path: Some(folder.to_string_lossy().into()),
            });
        }
        // Sidecar association must see base files even when their extensions
        // are excluded from the removal candidates.
        inventory.sort();
        let paths = inventory
            .iter()
            .filter(|path| allowed(path, report.request.extension_mode, &extensions))
            .collect::<Vec<_>>();
        progress(FileScanProgress {
            completed: 0,
            total: Some(paths.len()),
            current_path: None,
        });
        let mut files = vec![];
        for (i, &path) in paths.iter().enumerate() {
            cancel.check()?;
            if sidecar(path, &inventory) {
                continue;
            }
            match referenced(path, &references, &reference_keys) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    report.complete = false;
                    report.issues.push(ScanIssue {
                        display_path: path.to_string_lossy().into(),
                        message: error.to_string(),
                    });
                    continue;
                }
            }
            let observed = (|| -> Result<(u64, String)> {
                let metadata = std::fs::symlink_metadata(path)?;
                ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "File changed during scan"
                );
                Ok((metadata.len(), file_sha256_cancellable(path, cancel)?))
            })();
            match observed {
                Ok((size, hash)) => {
                    report.size_bytes = report
                        .size_bytes
                        .checked_add(size)
                        .context("Unused-file size overflow")?;
                    let id = format!(
                        "{:x}",
                        Sha256::digest([path_bytes(path), hash.as_bytes().to_vec()].concat())
                    );
                    report.files.push(UnusedFile {
                        id,
                        display_path: path.to_string_lossy().into(),
                        size_bytes: size,
                        source_sha256: hash.clone(),
                    });
                    files.push(FileProof {
                        path: path_bytes(path),
                        state: MaintenanceFileState::Present,
                        hash: Some(hash),
                    });
                }
                Err(error) => {
                    cancel.check()?;
                    report.complete = false;
                    report.issues.push(ScanIssue {
                        display_path: path.to_string_lossy().into(),
                        message: error.to_string(),
                    });
                }
            }
            progress(FileScanProgress {
                completed: i + 1,
                total: None,
                current_path: Some(path.to_string_lossy().into()),
            });
        }
        progress(FileScanProgress {
            completed: paths.len(),
            total: Some(paths.len()),
            current_path: None,
        });
        let mut saved = SavedUnused {
            report,
            root: path_bytes(&root),
            files,
            directories,
            references: reference_proofs,
            logic: super::scan::logic_identity(),
        };
        self.refresh_unused(&mut saved, cancel)?;
        cancel.check()?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if super::scan::stamp(&tx)? != saved.report.stamp {
            stale(&mut saved.report, "Library references changed; scan again");
        }
        tx.execute(
            "INSERT INTO maintenance_scans VALUES(?1,'unused',?2,?3)",
            params![
                saved.report.id,
                serde_json::to_string(&saved)?,
                saved.report.created
            ],
        )?;
        super::saved::prune(&tx)?;
        cancel.check()?;
        tx.commit()?;
        Ok(saved.report)
    }
    fn refresh_unused(&self, saved: &mut SavedUnused, cancel: &CancellationToken) -> Result<()> {
        cancel.check()?;
        let current = {
            let conn = self.conn()?;
            super::scan::stamp(&conn)?
        };
        if saved.logic != super::scan::logic_identity() || current != saved.report.stamp {
            stale(&mut saved.report, "Library or scanner changed; scan again");
        }
        let root = bytes_path(&saved.root)?;
        if resolved_prefix(&saved.report.request.root).ok().as_ref() != Some(&root) {
            stale(&mut saved.report, "Selected folder changed; scan again");
        }
        for (path, key) in &saved.references {
            cancel.check()?;
            if reference_key(&bytes_path(path)?) != *key {
                stale(&mut saved.report, "Referenced files changed; scan again");
                break;
            }
        }
        for proof in &saved.directories {
            cancel.check()?;
            let current = directory(&bytes_path(&proof.path)?).map(|rows| {
                rows.into_iter()
                    .map(|(p, k)| (path_bytes(&p), k))
                    .collect::<Vec<_>>()
            });
            if current.as_ref().ok() != Some(&proof.entries) {
                stale(&mut saved.report, "Directory contents changed; scan again");
                break;
            }
        }
        if !proofs_current(&saved.files, cancel)? {
            stale(&mut saved.report, "File contents changed; scan again");
        }
        Ok(())
    }
    pub fn unused_scan(&self, id: &str) -> Result<UnusedScanReport> {
        self.unused_scan_cancellable(id, &CancellationToken::default())
    }
    pub fn unused_scan_cancellable(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<UnusedScanReport> {
        cancel.check()?;
        let text: String = self
            .conn()?
            .query_row(
                "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind='unused'",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("Unused scan not found")?;
        let mut saved: SavedUnused = serde_json::from_str(&text)?;
        self.refresh_unused(&mut saved, cancel)?;
        Ok(saved.report)
    }
    pub(super) fn removal_scan(&self, id: &str, cancel: &CancellationToken) -> Result<SavedUnused> {
        let text: String = self.conn()?.query_row(
            "SELECT payload_json FROM maintenance_scans WHERE id=?1 AND kind='unused'",
            [id],
            |r| r.get(0),
        )?;
        let mut saved: SavedUnused = serde_json::from_str(&text)?;
        self.refresh_unused(&mut saved, cancel)?;
        ensure!(
            saved.report.complete && !saved.report.stale,
            "Unused scan is partial or stale; scan again"
        );
        Ok(saved)
    }
    pub fn unused_scan_export(&self, id: &str) -> Result<String> {
        self.unused_scan_export_cancellable(id, &CancellationToken::default())
    }
    pub fn unused_scan_export_cancellable(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        Ok(self
            .unused_scan_cancellable(id, cancel)?
            .files
            .iter()
            .map(|f| f.display_path.clone())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub(super) fn ensure_unreferenced(conn: &rusqlite::Connection, paths: &[PathBuf]) -> Result<()> {
    let mut q = conn.prepare("SELECT path FROM tracks WHERE path IS NOT NULL")?;
    let mut references = BTreeSet::new();
    let mut keys: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    for value in q.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        let path = bytes_path(&value?)?;
        let path = path.canonicalize().unwrap_or(path);
        if let Some(key) = reference_key(&path) {
            keys.entry(key).or_default().push(path.clone());
        }
        references.insert(path);
    }
    for path in paths {
        ensure!(
            !referenced(path, &references, &keys)?,
            "Removal file is referenced by the library"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuse_broad_system_and_home_roots_but_allow_music_subfolders() {
        let home = Path::new("/Users/Example");
        for path in ["/", "/Users", "/Users/Example"] {
            assert!(broad_root(Path::new(path), Some(home)), "{path}");
        }
        #[cfg(unix)]
        for path in ["/Volumes", "/Library"] {
            assert!(broad_root(Path::new(path), Some(home)));
        }
        #[cfg(windows)]
        for path in [r"C:\Users", r"C:\Windows", r"\\?\C:\Users"] {
            assert!(broad_root(Path::new(path), None));
        }
        for path in [
            "/Users/Example/Music",
            "/Volumes/Music/Library",
            "/authored/music",
        ] {
            assert!(!broad_root(Path::new(path), Some(home)), "{path}");
        }
    }
}
