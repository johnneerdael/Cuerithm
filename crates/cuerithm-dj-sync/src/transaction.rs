//! Durable publication of already staged target files, with byte-level stale
//! checks and recoverable backups across database, playlists, media and ANLZ.
mod case_renames;
#[cfg(test)]
#[path = "transaction/case_tests.rs"]
mod case_tests;
use anyhow::{Context, Result, ensure};
pub use case_renames::{
    CaseRenameChange, RenameBefore, case_alias, plan_case_renames, translate_case_renames,
};

/// Read a native file through any persisted case-rename interruption phase.
pub fn observed_file_hash(journal: &FileJournal, path: &Path) -> Result<Option<String>> {
    case_renames::file(journal, path)
}
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    #[serde(with = "crate::path_serde")]
    pub target: PathBuf,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub bytes: u64,
}
/// A caller's staged replacement or deletion, before journal-owned copying.
#[derive(Debug, Clone)]
pub struct FilePublication {
    pub target: PathBuf,
    pub before_sha256: Option<String>,
    pub staged: Option<PathBuf>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: DirectoryEntryKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryChange {
    #[serde(with = "crate::path_serde")]
    pub target: PathBuf,
    /// None means absent; Some(empty) means an existing empty directory.
    pub before_manifest: Option<Vec<DirectoryEntry>>,
    pub after_exists: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Prepared,
    Applying,
    Applied,
    Restoring,
    Restored,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileJournal {
    pub id: String,
    pub state: JournalState,
    pub changes: Vec<FileChange>,
    #[serde(default)]
    pub directories: Vec<DirectoryChange>,
    #[serde(default)]
    pub renames: Vec<CaseRenameChange>,
}
/// Each journal owns its staging files; no source path is used as a staged input.
pub struct FileTransaction {
    root: PathBuf,
}

pub fn file_hash(path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot inspect sync file: {}", path.display()));
        }
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Sync path is not a regular file: {}",
        path.display()
    );
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(Some(format!("{:x}", hash.finalize())))
}
/// Capture direct names and entry types without following child symbolic links.
pub fn directory_manifest(path: &Path) -> Result<Option<Vec<DirectoryEntry>>> {
    safe_ancestors(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Cannot inspect sync directory"),
    };
    ensure!(
        metadata.is_dir(),
        "Sync directory path is not a directory: {}",
        path.display()
    );
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        entries.push(DirectoryEntry {
            name: entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Sync directory contains a non-UTF-8 child name"))?,
            kind: if kind.is_symlink() {
                DirectoryEntryKind::Symlink
            } else if kind.is_dir() {
                DirectoryEntryKind::Directory
            } else if kind.is_file() {
                DirectoryEntryKind::File
            } else {
                DirectoryEntryKind::Other
            },
        });
    }
    entries.sort();
    Ok(Some(entries))
}
fn valid_manifest(entries: &[DirectoryEntry]) -> Result<()> {
    let mut names = BTreeSet::new();
    for entry in entries {
        let mut parts = Path::new(&entry.name).components();
        ensure!(
            matches!(parts.next(), Some(std::path::Component::Normal(_))) && parts.next().is_none(),
            "Directory manifest contains an invalid child name"
        );
        ensure!(
            names.insert(&entry.name),
            "Duplicate directory manifest child"
        );
    }
    ensure!(
        entries.windows(2).all(|pair| pair[0] < pair[1]),
        "Directory manifest must be sorted"
    );
    Ok(())
}
fn child_manifest(
    journal: &FileJournal,
    directory: &DirectoryChange,
    current: bool,
) -> Result<Vec<DirectoryEntry>> {
    let mut entries: BTreeMap<_, _> = directory
        .before_manifest
        .iter()
        .flatten()
        .map(|entry| (entry.name.clone(), entry.kind))
        .collect();
    for file in journal.changes.iter().filter(|c| {
        c.target.parent().is_some_and(|p| {
            translate_case_renames(p, &journal.renames)
                == translate_case_renames(&directory.target, &journal.renames)
        })
    }) {
        let exists = if current {
            case_renames::file(journal, &file.target)?.is_some()
        } else {
            file.after_sha256.is_some()
        };
        set_child(
            &mut entries,
            &file.target,
            exists.then_some(DirectoryEntryKind::File),
        )?;
    }
    for child in journal.directories.iter().filter(|c| {
        c.target.parent().is_some_and(|p| {
            translate_case_renames(p, &journal.renames)
                == translate_case_renames(&directory.target, &journal.renames)
        })
    }) {
        let exists = if current {
            case_renames::directory(journal, &child.target)?.is_some()
        } else {
            child.after_exists
        };
        set_child(
            &mut entries,
            &child.target,
            exists.then_some(DirectoryEntryKind::Directory),
        )?;
    }
    case_renames::overlay_children(journal, directory, current, &mut entries)?;
    Ok(entries
        .into_iter()
        .map(|(name, kind)| DirectoryEntry { name, kind })
        .collect())
}
fn set_child(
    entries: &mut BTreeMap<String, DirectoryEntryKind>,
    path: &Path,
    kind: Option<DirectoryEntryKind>,
) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Directory child path is not UTF-8")?;
    if let Some(kind) = kind {
        entries.insert(name.into(), kind);
    } else {
        entries.remove(name);
    }
    Ok(())
}
fn directory_order(journal: &FileJournal) -> Vec<&DirectoryChange> {
    let mut directories: Vec<_> = journal.directories.iter().collect();
    directories.sort_by(|a, b| {
        a.target
            .components()
            .count()
            .cmp(&b.target.components().count())
            .then_with(|| a.target.cmp(&b.target))
    });
    directories
}
fn check_planned_parents(journal: &FileJournal, path: &Path, restoring: bool) -> Result<()> {
    let mut parent = path.parent();
    while let Some(path) = parent {
        match fs::symlink_metadata(case_renames::physical(path, journal)?) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Sync parent is not a regular directory"
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ensure!(
                    journal.directories.iter().any(|dir| dir.target == path
                        && if restoring {
                            dir.before_manifest.is_some()
                        } else {
                            dir.after_exists
                        }),
                    "Missing directory ancestor must be explicitly planned: {}",
                    path.display()
                );
            }
            Err(error) => return Err(error.into()),
        }
        parent = path.parent();
    }
    Ok(())
}
fn preflight_directories(journal: &FileJournal, restoring: bool) -> Result<()> {
    if !restoring {
        // Distinct spelling entries for one native object must be collapsed into
        // a typed rename, never independently removed/recreated in one journal.
        use std::hash::{Hash, Hasher};
        let mut native: BTreeMap<u64, Vec<&Path>> = BTreeMap::new();
        for path in journal
            .changes
            .iter()
            .map(|c| c.target.as_path())
            .chain(journal.directories.iter().map(|c| c.target.as_path()))
        {
            if !path.exists() {
                continue;
            }
            let handle = same_file::Handle::from_path(path)?;
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            handle.hash(&mut hash);
            drop(handle);
            let candidates = native.entry(hash.finish()).or_default();
            for prior in candidates.iter() {
                ensure!(
                    !case_alias(prior, path)? && !case_alias(path, prior)?,
                    "Case aliases must use a typed rename instead of independent changes"
                );
            }
            candidates.push(path);
        }
    }
    let mut paths = BTreeSet::new();
    for directory in &journal.directories {
        safe_ancestors(&directory.target)?;
        ensure!(
            paths.insert(&directory.target)
                && !journal
                    .changes
                    .iter()
                    .any(|file| file.target == directory.target),
            "Duplicate directory publication path"
        );
        if let Some(manifest) = &directory.before_manifest {
            valid_manifest(manifest)?;
        }
        let actual = case_renames::directory(journal, &directory.target)?;
        let after = child_manifest(journal, directory, false)?;
        ensure!(
            directory.after_exists || after.is_empty(),
            "Directory removal contains unplanned or retained children: {}",
            directory.target.display()
        );
        if restoring {
            if let Some(actual) = actual {
                ensure!(
                    directory.before_manifest.is_some() || directory.after_exists,
                    "Unexpected directory after sync"
                );
                ensure!(
                    actual == child_manifest(journal, directory, true)?,
                    "Directory changed after sync: {}",
                    directory.target.display()
                );
            } else {
                ensure!(
                    directory.before_manifest.is_none() || !directory.after_exists,
                    "A retained directory disappeared after sync: {}",
                    directory.target.display()
                );
            }
        } else {
            ensure!(
                actual == directory.before_manifest,
                "Sync directory preview is stale: {}",
                directory.target.display()
            );
        }
        if if restoring {
            directory.before_manifest.is_some()
        } else {
            directory.after_exists
        } {
            check_planned_parents(journal, &directory.target, restoring)?;
        }
    }
    if !journal.directories.is_empty() {
        for file in &journal.changes {
            if if restoring {
                file.before_sha256.is_some()
            } else {
                file.after_sha256.is_some()
            } {
                check_planned_parents(journal, &file.target, restoring)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn safe_ancestors(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "Sync publication path must be absolute");
    ensure!(
        !path.components().any(|part| matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )),
        "Sync publication path must be normalized"
    );
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "Sync publication path traverses a symbolic link: {}",
                ancestor.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
/// Bytes in a file or folder of regular files; links are not followed and count for nothing.
fn owned_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        0
    } else if metadata.is_dir() {
        fs::read_dir(path)
            .map(|rows| rows.flatten().map(|row| owned_size(&row.path())).sum())
            .unwrap_or(0)
    } else {
        metadata.len()
    }
}
pub(crate) fn durable_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Journal parent missing")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temp, value)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    sync_directory(parent)?;
    Ok(())
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
fn publish_copy(source: &Path, target: &Path, allow_missing_parents: bool) -> Result<()> {
    safe_ancestors(target)?;
    let parent = target.parent().context("Sync target parent missing")?;
    if allow_missing_parents {
        fs::create_dir_all(parent)?;
    } else {
        ensure!(parent.is_dir(), "Explicit sync parent directory is missing");
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut File::open(source)?, &mut temp)?;
    if target.exists() {
        temp.as_file()
            .set_permissions(fs::metadata(target)?.permissions())?;
    }
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(target).map_err(|error| error.error)?;
    sync_directory(parent)?;
    Ok(())
}
impl FileTransaction {
    pub fn create(journal_root: &Path) -> Result<Self> {
        fs::create_dir_all(journal_root)?;
        let journal_root = fs::canonicalize(journal_root).context("Resolve sync journal root")?;
        let id = uuid::Uuid::new_v4().to_string();
        let root = journal_root.join(&id);
        fs::create_dir(&root)?;
        fs::create_dir(root.join("staged"))?;
        fs::create_dir(root.join("backup"))?;
        let this = Self { root };
        this.save(&FileJournal {
            id,
            state: JournalState::Prepared,
            changes: vec![],
            directories: vec![],
            renames: vec![],
        })?;
        Ok(this)
    }
    pub fn open(journal_root: &Path, id: &str) -> Result<Self> {
        let parsed = uuid::Uuid::parse_str(id).context("Invalid sync journal identity")?;
        ensure!(parsed.to_string() == id, "Invalid sync journal identity");
        let journal_root = fs::canonicalize(journal_root).context("Resolve sync journal root")?;
        let this = Self {
            root: journal_root.join(id),
        };
        safe_ancestors(&this.root)?;
        let journal = this.inspect()?;
        ensure!(journal.id == id, "Sync journal identity mismatch");
        Ok(this)
    }
    /// Remove an unpublished preview without ever touching its native targets.
    /// Keep its journal until payload cleanup succeeds so a failed discard can
    /// be retried from history. Applied/interrupted publications retain backups.
    pub fn discard(journal_root: &Path, id: &str) -> Result<()> {
        let parsed = uuid::Uuid::parse_str(id).context("Invalid sync journal identity")?;
        ensure!(parsed.to_string() == id, "Invalid sync journal identity");
        // Normalize the supplied storage boundary, including ordinary host
        // aliases such as macOS /var. Never canonicalize the UUID child: a
        // symlink there must remain an error rather than escape this boundary.
        let journal_root = match fs::canonicalize(journal_root) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("Resolve sync journal root"),
        };
        let this = Self {
            root: journal_root.join(id),
        };
        safe_ancestors(&this.root)?;
        let _lease = this.lock()?;
        match fs::symlink_metadata(&this.root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(metadata) => ensure!(metadata.is_dir(), "Sync journal path is not a directory"),
        }
        let journal = this.inspect()?;
        ensure!(journal.id == id, "Sync journal identity mismatch");
        ensure!(
            journal.state == JournalState::Prepared,
            "Only an unapplied sync preview can be discarded"
        );
        this.discard_payload(&journal)
    }

    /// Frees the copies a finished transaction keeps only for restoring it: the staged files and
    /// the backups of what it replaced. The journal and the request stay, so the transaction
    /// is still listed and can still be inspected; it can no longer be restored. Only a
    /// transaction that was applied or restored is touched, and nothing outside its own
    /// `staged` and `backup` folders. Returns how many bytes were freed.
    pub fn expire_payload(journal_root: &Path, id: &str) -> Result<u64> {
        let parsed = uuid::Uuid::parse_str(id).context("Invalid sync journal identity")?;
        ensure!(parsed.to_string() == id, "Invalid sync journal identity");
        let journal_root = match fs::canonicalize(journal_root) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error).context("Resolve sync journal root"),
        };
        let this = Self {
            root: journal_root.join(id),
        };
        safe_ancestors(&this.root)?;
        let _lease = this.lock()?;
        match fs::symlink_metadata(&this.root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Sync journal path is not a directory"
            ),
        }
        let journal = this.inspect()?;
        ensure!(journal.id == id, "Sync journal identity mismatch");
        ensure!(
            matches!(
                journal.state,
                JournalState::Applied | JournalState::Restored
            ),
            "Only a finished sync can give up its restore copies"
        );
        let mut freed = 0;
        for folder in ["staged", "backup"] {
            let folder = this.root.join(folder);
            let Ok(metadata) = fs::symlink_metadata(&folder) else {
                continue;
            };
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            for entry in fs::read_dir(&folder)? {
                let path = entry?.path();
                freed += owned_size(&path);
                crate::private_cleanup::remove(&path)?;
            }
        }
        Ok(freed)
    }

    fn discard_payload(&self, journal: &FileJournal) -> Result<()> {
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_name() == "journal.json" {
                continue;
            }
            crate::private_cleanup::remove(&entry.path())?;
        }
        crate::private_cleanup::remove(&self.root.join("journal.json"))?;
        if let Err(error) = fs::remove_dir(&self.root) {
            // A platform sharing violation or a newly appeared child can leave
            // the directory behind. Restore its Prepared record for retry.
            self.save(journal)
                .context("Restore the failed discard's retry journal")?;
            return Err(error).context("Remove discarded sync preview directory");
        }
        sync_directory(self.root.parent().context("Sync journal parent missing")?)
    }
    pub fn path(&self) -> &Path {
        &self.root
    }
    pub fn inspect(&self) -> Result<FileJournal> {
        Ok(serde_json::from_reader(File::open(
            self.root.join("journal.json"),
        )?)?)
    }
    fn save(&self, journal: &FileJournal) -> Result<()> {
        durable_json(&self.root.join("journal.json"), journal)
    }
    fn lock(&self) -> Result<File> {
        safe_ancestors(&self.root)?;
        // Keep the lock outside disposable payloads. Windows cannot remove an
        // opened lock file/directory, and closing it early would race apply.
        let locks = self
            .root
            .parent()
            .context("Sync journal parent missing")?
            .join(".locks");
        safe_ancestors(&locks)?;
        fs::create_dir_all(&locks)?;
        let lock = locks.join(
            self.root
                .file_name()
                .context("Sync journal identity missing")?,
        );
        safe_ancestors(&lock)?;
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock)?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(error)
                    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                        && started.elapsed() < std::time::Duration::from_millis(250) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(error) => return Err(error).context("This sync operation is already running"),
            }
        }
        Ok(file)
    }
    /// Capture a replacement with the caller's fingerprint from before staging.
    /// `None` as staged input represents an explicitly planned deletion.
    pub fn add(
        &self,
        target: &Path,
        before_sha256: Option<String>,
        staged: Option<&Path>,
    ) -> Result<()> {
        self.add_batch(
            &[FilePublication {
                target: target.into(),
                before_sha256,
                staged: staged.map(Path::to_path_buf),
            }],
            &[],
        )
    }
    /// Guard or change a directory; batches avoid repeatedly saving growing journals.
    pub fn add_directory(
        &self,
        target: &Path,
        before_manifest: Option<Vec<DirectoryEntry>>,
        after_exists: bool,
    ) -> Result<()> {
        self.add_batch(
            &[],
            &[DirectoryChange {
                target: target.into(),
                before_manifest,
                after_exists,
            }],
        )
    }
    pub fn add_case_rename(&self, from: &Path, to: &Path, before: RenameBefore) -> Result<()> {
        let _lease = self.lock()?;
        let mut journal = self.inspect()?;
        self.prepare_case_rename(&mut journal, from, to, before)?;
        self.save(&journal)
    }
    fn prepare_case_rename(
        &self,
        journal: &mut FileJournal,
        from: &Path,
        to: &Path,
        before: RenameBefore,
    ) -> Result<()> {
        ensure!(
            journal.state == JournalState::Prepared,
            "Sync preview is no longer editable"
        );
        ensure!(
            !from.starts_with(&self.root) && !to.starts_with(&self.root),
            "Case rename overlaps its journal"
        );
        ensure!(
            case_alias(from, to)?,
            "Case rename paths do not identify one native entry"
        );
        for prior in &journal.renames {
            ensure!(
                !same_file::is_same_file(&prior.from, from)?,
                "Duplicate native case rename source"
            );
        }
        match &before {
            RenameBefore::File { sha256 } => ensure!(
                file_hash(from)?.as_ref() == Some(sha256),
                "Case rename file changed during preparation"
            ),
            RenameBefore::Directory { manifest } => {
                valid_manifest(manifest)?;
                ensure!(
                    directory_manifest(from)?.as_ref() == Some(manifest),
                    "Case rename directory changed during preparation"
                );
            }
        }
        let temporary = from.with_file_name(format!(
            ".cuerithm-rename-{}-{}",
            journal.id,
            journal.renames.len()
        ));
        ensure!(
            matches!(fs::symlink_metadata(&temporary),Err(ref e) if e.kind()==std::io::ErrorKind::NotFound),
            "Case rename temporary path already exists"
        );
        journal.renames.push(CaseRenameChange {
            from: from.into(),
            to: to.into(),
            temporary,
            before,
        });
        case_renames::validate_plan(journal)
    }
    /// Stage a complete batch, then serialize/fsync its journal exactly once.
    /// A validation/copy failure leaves the prior journal and all native files intact.
    pub fn add_batch(
        &self,
        files: &[FilePublication],
        directories: &[DirectoryChange],
    ) -> Result<()> {
        let _lease = self.lock()?;
        let mut journal = self.inspect()?;
        ensure!(
            journal.state == JournalState::Prepared,
            "Sync preview is no longer editable"
        );
        let mut targets: BTreeSet<_> = journal
            .changes
            .iter()
            .map(|c| translate_case_renames(&c.target, &journal.renames))
            .chain(
                journal
                    .directories
                    .iter()
                    .map(|c| translate_case_renames(&c.target, &journal.renames)),
            )
            .collect();
        let mut staged_paths = BTreeSet::new();
        let staged = (|| -> Result<()> {
            for directory in directories {
                safe_ancestors(&directory.target)?;
                ensure!(
                    !directory.target.starts_with(&self.root),
                    "Sync directory overlaps its journal"
                );
                ensure!(
                    targets.insert(translate_case_renames(&directory.target, &journal.renames)),
                    "Duplicate sync publication path"
                );
                if let Some(manifest) = &directory.before_manifest {
                    valid_manifest(manifest)?;
                }
                ensure!(
                    case_renames::directory(&journal, &directory.target)?
                        == directory.before_manifest,
                    "Directory changed while preparing sync"
                );
                journal.directories.push(directory.clone());
            }
            for file in files {
                safe_ancestors(&file.target)?;
                ensure!(
                    !file.target.starts_with(&self.root),
                    "Sync target overlaps its journal"
                );
                ensure!(
                    targets.insert(translate_case_renames(&file.target, &journal.renames)),
                    "Duplicate sync publication path"
                );
                ensure!(
                    case_renames::file(&journal, &file.target)? == file.before_sha256,
                    "Target changed while preparing sync"
                );
                let destination = self
                    .root
                    .join("staged")
                    .join(journal.changes.len().to_string());
                let after_sha256 = if let Some(source) = &file.staged {
                    staged_paths.insert(destination.clone());
                    fs::copy(source, &destination)?;
                    // A device copy may originate on read-only media. Keep the
                    // private staging copy writable without changing its source.
                    let mut permissions = fs::metadata(&destination)?.permissions();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        permissions.set_mode(permissions.mode() | 0o200);
                    }
                    #[cfg(windows)]
                    {
                        // Windows read-only attribute; Unix uses owner-write above.
                        #[allow(clippy::permissions_set_readonly_false)]
                        permissions.set_readonly(false);
                    }
                    fs::set_permissions(&destination, permissions)?;
                    File::options()
                        .read(true)
                        .write(true)
                        .open(&destination)?
                        .sync_all()?;
                    file_hash(&destination)?
                } else {
                    None
                };
                if after_sha256 == file.before_sha256 {
                    if file.staged.is_some() {
                        fs::remove_file(&destination)?;
                    }
                    continue;
                }
                let bytes = if file.staged.is_some() {
                    fs::metadata(&destination)?.len()
                } else {
                    0
                };
                journal.changes.push(FileChange {
                    target: file.target.clone(),
                    before_sha256: file.before_sha256.clone(),
                    after_sha256,
                    bytes,
                });
            }
            case_renames::validate_plan(&journal)?;
            Ok(())
        })();
        if let Err(error) = staged {
            for path in staged_paths {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(cleanup) => {
                        return Err(error
                            .context(format!("Could not remove failed batch staging: {cleanup}")));
                    }
                }
            }
            return Err(error);
        }
        // Retain staged bytes if durable_json persisted but its directory fsync failed.
        self.save(&journal)
    }
    /// Check the native before-state before finishing an interrupted local restore acknowledgement.
    pub fn verify_restored(&self) -> Result<FileJournal> {
        let _lease = self.lock()?;
        let journal = self.inspect()?;
        ensure!(
            journal.state == JournalState::Restored,
            "Sync restoration is not complete"
        );
        case_renames::verify(&journal, true)?;
        for change in &journal.changes {
            ensure!(
                case_renames::file(&journal, &change.target)? == change.before_sha256,
                "Restored sync file changed before acknowledgement recovery"
            );
        }
        for directory in &journal.directories {
            ensure!(
                case_renames::directory(&journal, &directory.target)? == directory.before_manifest,
                "Restored sync directory changed before acknowledgement recovery"
            );
        }
        Ok(journal)
    }
    /// Recheck the app and every target before publishing. A failed or interrupted
    /// apply retains all backups and can be recovered with `restore`.
    pub fn apply(&self, check_target_closed: impl Fn() -> Result<()>) -> Result<FileJournal> {
        let _lease = self.lock()?;
        let mut journal = self.inspect()?;
        ensure!(
            journal.state == JournalState::Prepared,
            "Sync preview was already applied or interrupted"
        );
        check_target_closed()?;
        case_renames::preflight(&journal, false)?;
        preflight_directories(&journal, false)?;
        for (index, change) in journal.changes.iter().enumerate() {
            safe_ancestors(&case_renames::physical(&change.target, &journal)?)?;
            ensure!(
                case_renames::file(&journal, &change.target)? == change.before_sha256,
                "Sync preview is stale: {}",
                change.target.display()
            );
            if let Some(hash) = &change.after_sha256 {
                ensure!(
                    file_hash(&self.root.join("staged").join(index.to_string()))?.as_ref()
                        == Some(hash),
                    "Staged sync file changed"
                );
            }
            if change.before_sha256.is_some() {
                let backup = self.root.join("backup").join(index.to_string());
                fs::copy(case_renames::physical(&change.target, &journal)?, &backup)?;
                File::options()
                    .read(true)
                    .write(true)
                    .open(&backup)?
                    .sync_all()?;
                ensure!(
                    file_hash(&backup)? == change.before_sha256,
                    "Target changed while creating backup"
                );
            }
        }
        sync_directory(&self.root.join("backup"))?;
        journal.state = JournalState::Applying;
        self.save(&journal)?;
        case_renames::apply(&journal, &check_target_closed)?;
        for directory in directory_order(&journal) {
            if directory.before_manifest.is_none() && directory.after_exists {
                check_target_closed()?;
                safe_ancestors(&case_renames::physical(&directory.target, &journal)?)?;
                fs::create_dir(case_renames::physical(&directory.target, &journal)?)
                    .context("Create planned sync directory")?;
                sync_directory(
                    case_renames::physical(&directory.target, &journal)?
                        .parent()
                        .context("Directory parent missing")?,
                )?;
            }
        }
        for (index, change) in journal.changes.iter().enumerate() {
            check_target_closed()?;
            // Removed SQLite sidecars must stay absent until the new main file
            // is published; unrelated SQLite writers may not be DJ processes.
            for previous in &journal.changes[..index] {
                let name = previous.target.to_string_lossy();
                if previous.after_sha256.is_none()
                    && ["-wal", "-shm", "-journal"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
                {
                    ensure!(
                        file_hash(&previous.target)?.is_none(),
                        "SQLite sidecar was recreated during sync"
                    );
                }
            }
            ensure!(
                case_renames::file(&journal, &change.target)? == change.before_sha256,
                "Target changed during sync"
            );
            if change.after_sha256.is_some() {
                publish_copy(
                    &self.root.join("staged").join(index.to_string()),
                    &case_renames::physical(&change.target, &journal)?,
                    journal.directories.is_empty(),
                )?;
            } else {
                fs::remove_file(case_renames::physical(&change.target, &journal)?)?;
                sync_directory(
                    case_renames::physical(&change.target, &journal)?
                        .parent()
                        .unwrap(),
                )?;
            }
            ensure!(
                case_renames::file(&journal, &change.target)? == change.after_sha256,
                "Sync write verification failed"
            );
        }
        for directory in directory_order(&journal).into_iter().rev() {
            if directory.before_manifest.is_some() && !directory.after_exists {
                check_target_closed()?;
                ensure!(
                    case_renames::directory(&journal, &directory.target)? == Some(Vec::new()),
                    "Directory gained an unexpected child before removal"
                );
                fs::remove_dir(case_renames::physical(&directory.target, &journal)?)?;
                sync_directory(
                    case_renames::physical(&directory.target, &journal)?
                        .parent()
                        .context("Directory parent missing")?,
                )?;
            }
        }
        check_target_closed()?;
        for change in &journal.changes {
            ensure!(
                case_renames::file(&journal, &change.target)? == change.after_sha256,
                "Target changed before sync verification completed: {}",
                change.target.display()
            );
        }
        for directory in &journal.directories {
            let expected = directory
                .after_exists
                .then(|| child_manifest(&journal, directory, false))
                .transpose()?;
            ensure!(
                case_renames::directory(&journal, &directory.target)? == expected,
                "Sync directory write verification failed"
            );
        }
        journal.state = JournalState::Applied;
        case_renames::verify(&journal, false)?;
        self.save(&journal)?;
        Ok(journal)
    }
    /// Verify a completed publication before recovering its local acknowledgement.
    pub fn verify_applied(&self) -> Result<FileJournal> {
        let _lease = self.lock()?;
        let journal = self.inspect()?;
        ensure!(
            journal.state == JournalState::Applied,
            "Sync publication is not complete"
        );
        case_renames::verify(&journal, false)?;
        for change in &journal.changes {
            safe_ancestors(&case_renames::physical(&change.target, &journal)?)?;
            ensure!(
                case_renames::file(&journal, &change.target)? == change.after_sha256,
                "Published sync changed before acknowledgement recovery"
            );
        }
        for directory in &journal.directories {
            let expected = directory
                .after_exists
                .then(|| child_manifest(&journal, directory, false))
                .transpose()?;
            ensure!(
                case_renames::directory(&journal, &directory.target)? == expected,
                "Published sync directory changed before acknowledgement recovery"
            );
        }
        Ok(journal)
    }
    /// Restore only when every current file still matches a known transaction
    /// state. Concurrent external edits cause a conflict before any restoration.
    pub fn restore(&self, check_target_closed: impl Fn() -> Result<()>) -> Result<FileJournal> {
        let _lease = self.lock()?;
        let mut journal = self.inspect()?;
        ensure!(
            matches!(
                journal.state,
                JournalState::Applying | JournalState::Applied | JournalState::Restoring
            ),
            "Sync has no applied changes to restore"
        );
        check_target_closed()?;
        case_renames::preflight(&journal, true)?;
        preflight_directories(&journal, true)?;
        for (index, change) in journal.changes.iter().enumerate() {
            safe_ancestors(&case_renames::physical(&change.target, &journal)?)?;
            let current = case_renames::file(&journal, &change.target)?;
            ensure!(
                current == change.after_sha256 || current == change.before_sha256,
                "Target changed after sync: {}",
                change.target.display()
            );
            if change.before_sha256.is_some() {
                ensure!(
                    file_hash(&self.root.join("backup").join(index.to_string()))?
                        == change.before_sha256,
                    "Sync backup changed"
                );
            }
        }
        journal.state = JournalState::Restoring;
        self.save(&journal)?;
        for directory in directory_order(&journal) {
            if directory.before_manifest.is_some()
                && case_renames::directory(&journal, &directory.target)?.is_none()
            {
                check_target_closed()?;
                safe_ancestors(&case_renames::physical(&directory.target, &journal)?)?;
                fs::create_dir(case_renames::physical(&directory.target, &journal)?)?;
                sync_directory(
                    case_renames::physical(&directory.target, &journal)?
                        .parent()
                        .context("Directory parent missing")?,
                )?;
            }
        }
        for (index, change) in journal.changes.iter().enumerate().rev() {
            check_target_closed()?;
            let current = case_renames::file(&journal, &change.target)?;
            if current == change.before_sha256 {
                continue;
            }
            ensure!(
                current == change.after_sha256,
                "Target changed during restore"
            );
            if change.before_sha256.is_some() {
                publish_copy(
                    &self.root.join("backup").join(index.to_string()),
                    &case_renames::physical(&change.target, &journal)?,
                    journal.directories.is_empty(),
                )?;
            } else if case_renames::physical(&change.target, &journal)?.exists() {
                fs::remove_file(case_renames::physical(&change.target, &journal)?)?;
                sync_directory(
                    case_renames::physical(&change.target, &journal)?
                        .parent()
                        .unwrap(),
                )?;
            }
            ensure!(
                case_renames::file(&journal, &change.target)? == change.before_sha256,
                "Sync restore verification failed"
            );
        }
        for directory in directory_order(&journal).into_iter().rev() {
            if directory.before_manifest.is_none()
                && case_renames::directory(&journal, &directory.target)?.is_some()
            {
                check_target_closed()?;
                ensure!(
                    case_renames::directory(&journal, &directory.target)? == Some(Vec::new()),
                    "Directory gained an unexpected child during restore"
                );
                fs::remove_dir(case_renames::physical(&directory.target, &journal)?)?;
                sync_directory(
                    case_renames::physical(&directory.target, &journal)?
                        .parent()
                        .context("Directory parent missing")?,
                )?;
            }
        }
        case_renames::restore(&journal, &check_target_closed)?;
        for directory in &journal.directories {
            ensure!(
                case_renames::directory(&journal, &directory.target)? == directory.before_manifest,
                "Sync directory restore verification failed"
            );
        }
        journal.state = JournalState::Restored;
        self.save(&journal)?;
        Ok(journal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_journal_roots_support_open_publication_and_prepared_discard() {
        let cwd = std::env::current_dir().unwrap();
        let temp = tempfile::Builder::new()
            .prefix(".relative-sync-test-")
            .tempdir_in(&cwd)
            .unwrap();
        let relative = temp.path().strip_prefix(&cwd).unwrap().join("journals");
        let native = fs::canonicalize(temp.path()).unwrap().join("native.db");
        let staged = native.with_extension("staged");
        fs::write(&native, b"before").unwrap();
        fs::write(&staged, b"after").unwrap();
        let transaction = FileTransaction::create(&relative).unwrap();
        assert!(transaction.path().is_absolute());
        let id = transaction.inspect().unwrap().id;
        transaction
            .add(&native, file_hash(&native).unwrap(), Some(&staged))
            .unwrap();
        let reopened = FileTransaction::open(&relative, &id).unwrap();
        reopened.apply(|| Ok(())).unwrap();
        reopened.restore(|| Ok(())).unwrap();
        assert_eq!(fs::read(&native).unwrap(), b"before");
        let prepared = FileTransaction::create(&relative).unwrap();
        let id = prepared.inspect().unwrap().id;
        FileTransaction::discard(&relative, &id).unwrap();
        FileTransaction::discard(&relative, &id).unwrap();
        assert!(!prepared.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn journal_root_alias_is_normalized_but_uuid_child_alias_stays_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let journals = root.join("journals");
        fs::create_dir(&journals).unwrap();
        let alias = root.join("host-alias");
        std::os::unix::fs::symlink(&journals, &alias).unwrap();
        let transaction = FileTransaction::create(&alias).unwrap();
        let id = transaction.inspect().unwrap().id;
        assert_eq!(
            FileTransaction::open(&alias, &id).unwrap().path(),
            transaction.path()
        );
        FileTransaction::discard(&alias, &id).unwrap();
        assert!(!transaction.path().exists());
        let outside = FileTransaction::create(&root.join("outside")).unwrap();
        let id = outside.inspect().unwrap().id;
        std::os::unix::fs::symlink(outside.path(), journals.join(&id)).unwrap();
        assert!(FileTransaction::open(&alias, &id).is_err());
        assert!(FileTransaction::discard(&alias, &id).is_err());
        assert!(outside.path().join("journal.json").exists());
    }

    #[test]
    fn discarded_previews_free_payloads_without_touching_targets_and_are_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let native = root.join("native.db");
        let staged = root.join("copy.db");
        fs::write(&native, b"native before").unwrap();
        fs::write(&staged, b"prepared after").unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        transaction
            .add(&native, file_hash(&native).unwrap(), Some(&staged))
            .unwrap();
        let id = transaction.inspect().unwrap().id;
        fs::create_dir(transaction.path().join("build")).unwrap();
        fs::write(
            transaction.path().join("build/large-copy"),
            b"private export payload",
        )
        .unwrap();
        FileTransaction::discard(&root.join("journals"), &id).unwrap();
        assert!(!transaction.path().exists());
        assert_eq!(fs::read(&native).unwrap(), b"native before");
        FileTransaction::discard(&root.join("journals"), &id).unwrap();
        assert!(transaction.apply(|| Ok(())).is_err());
        assert_eq!(fs::read(&native).unwrap(), b"native before");
        assert!(FileTransaction::discard(&root.join("journals"), "../native.db").is_err());
    }

    #[test]
    fn a_finished_sync_gives_up_its_restore_copies_but_keeps_its_journal_and_targets() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let journals = root.join("journals");
        let native = root.join("native.db");
        let staged = root.join("copy.db");
        fs::write(&native, b"native before").unwrap();
        fs::write(&staged, b"published after").unwrap();
        let transaction = FileTransaction::create(&journals).unwrap();
        transaction
            .add(&native, file_hash(&native).unwrap(), Some(&staged))
            .unwrap();
        let id = transaction.inspect().unwrap().id;
        // A prepared sync keeps everything: it can still be applied.
        assert!(FileTransaction::expire_payload(&journals, &id).is_err());
        assert!(transaction.path().join("staged/0").exists());
        transaction.apply(|| Ok(())).unwrap();
        assert!(transaction.path().join("backup/0").exists());
        // An unrelated file beside the payload is never touched.
        fs::write(transaction.path().join("request.json"), b"{}").unwrap();
        fs::create_dir(transaction.path().join("build")).unwrap();
        fs::write(transaction.path().join("build/kept"), b"other").unwrap();
        let freed = FileTransaction::expire_payload(&journals, &id).unwrap();
        // The backup of what was replaced and the staged copy of what was published.
        assert_eq!(
            freed,
            (b"native before".len() + b"published after".len()) as u64
        );
        assert_eq!(fs::read(&native).unwrap(), b"published after");
        assert_eq!(
            fs::read_dir(transaction.path().join("backup"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(transaction.path().join("staged"))
                .unwrap()
                .count(),
            0
        );
        assert!(transaction.path().join("request.json").exists());
        assert!(transaction.path().join("build/kept").exists());
        assert_eq!(transaction.inspect().unwrap().state, JournalState::Applied);
        // Nothing is left to restore from.
        assert!(transaction.restore(|| Ok(())).is_err());
        assert_eq!(fs::read(&native).unwrap(), b"published after");
        // Again, and for a transaction that is gone, is not an error.
        assert_eq!(FileTransaction::expire_payload(&journals, &id).unwrap(), 0);
        assert_eq!(
            FileTransaction::expire_payload(&journals, &uuid::Uuid::new_v4().to_string()).unwrap(),
            0
        );
        assert!(FileTransaction::expire_payload(&journals, "../native.db").is_err());
    }

    #[test]
    fn discard_rejects_every_published_or_interrupted_state_and_retains_backups() {
        for state in [
            JournalState::Applying,
            JournalState::Applied,
            JournalState::Restoring,
            JournalState::Restored,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let transaction = FileTransaction::create(&root.join("journals")).unwrap();
            let mut journal = transaction.inspect().unwrap();
            journal.state = state;
            transaction.save(&journal).unwrap();
            fs::write(transaction.path().join("backup/0"), b"recovery evidence").unwrap();
            let before = fs::read(transaction.path().join("journal.json")).unwrap();
            assert!(
                FileTransaction::discard(&root.join("journals"), &journal.id)
                    .unwrap_err()
                    .to_string()
                    .contains("unapplied")
            );
            assert_eq!(
                fs::read(transaction.path().join("journal.json")).unwrap(),
                before
            );
            assert_eq!(
                fs::read(transaction.path().join("backup/0")).unwrap(),
                b"recovery evidence"
            );
        }
    }

    #[test]
    fn concurrent_apply_preserves_publication_and_discard_rejects_completed_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let native = root.join("native.db");
        let staged = root.join("after.db");
        fs::write(&native, b"before").unwrap();
        fs::write(&staged, b"after").unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        transaction
            .add(&native, file_hash(&native).unwrap(), Some(&staged))
            .unwrap();
        let id = transaction.inspect().unwrap().id;
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let apply = std::thread::spawn(move || {
            let first = std::cell::Cell::new(true);
            transaction.apply(|| {
                if first.replace(false) {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                Ok(())
            })
        });
        entered_rx.recv().unwrap();
        let journals = root.join("journals");
        let discard_id = id.clone();
        let discard = std::thread::spawn(move || FileTransaction::discard(&journals, &discard_id));
        release_tx.send(()).unwrap();
        assert_eq!(apply.join().unwrap().unwrap().state, JournalState::Applied);
        let error = discard.join().unwrap().unwrap_err().to_string();
        assert!(
            error.contains("unapplied") || error.contains("already running"),
            "{error}"
        );
        assert!(
            FileTransaction::discard(&root.join("journals"), &id)
                .unwrap_err()
                .to_string()
                .contains("unapplied")
        );
        assert_eq!(fs::read(&native).unwrap(), b"after");
        assert_eq!(
            fs::read(root.join("journals").join(&id).join("backup/0")).unwrap(),
            b"before"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discard_rejects_a_journal_symlink_outside_its_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let original = FileTransaction::create(&root.join("outside")).unwrap();
        let id = original.inspect().unwrap().id;
        let journals = root.join("journals");
        fs::create_dir(&journals).unwrap();
        std::os::unix::fs::symlink(original.path(), journals.join(&id)).unwrap();
        assert!(FileTransaction::discard(&journals, &id).is_err());
        assert!(original.path().join("journal.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_discard_keeps_a_retryable_journal_until_payload_cleanup_succeeds() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        let id = transaction.inspect().unwrap().id;
        let build = transaction.path().join("build");
        fs::create_dir(&build).unwrap();
        fs::write(build.join("copy"), b"remaining staged payload").unwrap();
        fs::write(build.join("permission-probe"), b"probe").unwrap();
        fs::set_permissions(&build, fs::Permissions::from_mode(0o500)).unwrap();
        if fs::remove_file(build.join("permission-probe")).is_ok() {
            // Privileged test runners can bypass this fixture's permissions.
            fs::set_permissions(&build, fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        assert!(FileTransaction::discard(&root.join("journals"), &id).is_err());
        assert_eq!(transaction.inspect().unwrap().state, JournalState::Prepared);
        assert!(build.join("copy").is_file());
        fs::set_permissions(&build, fs::Permissions::from_mode(0o700)).unwrap();
        FileTransaction::discard(&root.join("journals"), &id).unwrap();
        assert!(!transaction.path().exists());
    }

    #[cfg(windows)]
    #[test]
    fn abandoned_prepared_readonly_copies_are_discarded_without_changing_source_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("original-resource");
        fs::write(&source, b"original resource bytes").unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions).unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        let id = transaction.inspect().unwrap().id;
        // No BuildDirectory guard owns this tree: model a killed preparation.
        let abandoned = transaction.path().join("build-abandoned/nested");
        fs::create_dir_all(&abandoned).unwrap();
        fs::copy(&source, abandoned.join("resource")).unwrap();
        fs::copy(&source, transaction.path().join("loose-resource")).unwrap();
        assert!(
            fs::metadata(abandoned.join("resource"))
                .unwrap()
                .permissions()
                .readonly()
        );
        FileTransaction::discard(&root.join("journals"), &id).unwrap();
        assert!(!transaction.path().exists());
        assert_eq!(fs::read(&source).unwrap(), b"original resource bytes");
        assert!(fs::metadata(&source).unwrap().permissions().readonly());
        // Restore only the synthetic source fixture for TempDir's own teardown.
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        // Windows-only test: clear the fixture's read-only attribute.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&source, permissions).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_opened_payload_blocks_discard_without_losing_its_retry_journal() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        let id = transaction.inspect().unwrap().id;
        let payload = transaction.path().join("large-copy");
        fs::write(&payload, b"staged payload").unwrap();
        let opened = File::options()
            .read(true)
            .share_mode(1)
            .open(&payload)
            .unwrap();
        assert!(FileTransaction::discard(&root.join("journals"), &id).is_err());
        assert_eq!(transaction.inspect().unwrap().state, JournalState::Prepared);
        drop(opened);
        FileTransaction::discard(&root.join("journals"), &id).unwrap();
        assert!(!transaction.path().exists());
    }

    #[test]
    fn readonly_source_can_be_staged_without_changing_source_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let source = root.join("readonly.wav");
        let target = root.join("device-copy.wav");
        fs::write(&source, b"synthetic read-only audio").unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions).unwrap();
        let transaction = FileTransaction::create(&root.join("journals")).unwrap();
        transaction.add(&target, None, Some(&source)).unwrap();
        transaction.apply(|| Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), fs::read(&source).unwrap());
        assert!(fs::metadata(&source).unwrap().permissions().readonly());
        transaction.restore(|| Ok(())).unwrap();
        assert!(!target.exists());
        assert!(fs::metadata(&source).unwrap().permissions().readonly());
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        #[cfg(windows)]
        {
            // Windows fixture attribute; Unix uses explicit owner permissions.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
        }
        fs::set_permissions(source, permissions).unwrap();
    }
    #[test]
    fn batch_stages_multiple_files_and_directories_as_one_plan() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let staged = root.join("new");
        fs::write(&staged, b"new").unwrap();
        let folder = root.join("folder");
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add_batch(
            &[
                FilePublication {
                    target: folder.join("one"),
                    before_sha256: None,
                    staged: Some(staged.clone()),
                },
                FilePublication {
                    target: folder.join("two"),
                    before_sha256: None,
                    staged: Some(staged),
                },
            ],
            &[DirectoryChange {
                target: folder.clone(),
                before_manifest: None,
                after_exists: true,
            }],
        )
        .unwrap();
        assert_eq!(tx.inspect().unwrap().changes.len(), 2);
        tx.apply(|| Ok(())).unwrap();
        assert_eq!(fs::read(folder.join("two")).unwrap(), b"new");
        tx.restore(|| Ok(())).unwrap();
        assert!(!folder.exists());
    }
    #[test]
    fn case_only_leaf_rename_retains_content_and_restores_original_spelling() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let from = root.join("set.vdjfolder");
        let to = root.join("Set.vdjfolder");
        fs::write(&from, b"playlist").unwrap();
        if !to.exists() {
            return;
        }
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add_case_rename(
            &from,
            &to,
            RenameBefore::File {
                sha256: file_hash(&from).unwrap().unwrap(),
            },
        )
        .unwrap();
        tx.apply(|| Ok(())).unwrap();
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(names.contains(&to.file_name().unwrap().into()));
        assert!(!names.contains(&from.file_name().unwrap().into()));
        tx.restore(|| Ok(())).unwrap();
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(names.contains(&from.file_name().unwrap().into()));
        assert_eq!(fs::read(from).unwrap(), b"playlist");
    }
    #[test]
    fn empty_directory_creation_and_restore() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        let target = root.join("empty.subfolders");
        tx.add_directory(&target, None, true).unwrap();
        tx.apply(|| Ok(())).unwrap();
        assert!(target.is_dir());
        tx.restore(|| Ok(())).unwrap();
        assert!(!target.exists());
    }
    #[test]
    fn empty_directory_removal_and_restore_retains_original_existence() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("empty.subfolders");
        fs::create_dir(&target).unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add_directory(&target, directory_manifest(&target).unwrap(), false)
            .unwrap();
        tx.apply(|| Ok(())).unwrap();
        assert!(!target.exists());
        tx.restore(|| Ok(())).unwrap();
        assert_eq!(directory_manifest(&target).unwrap(), Some(vec![]));
    }
    #[test]
    fn stale_directory_manifest_blocks_every_publication() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let folder = root.join("folder");
        fs::create_dir(&folder).unwrap();
        let database = root.join("database");
        let staged = root.join("stage");
        fs::write(&database, b"before").unwrap();
        fs::write(&staged, b"after").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add(&database, file_hash(&database).unwrap(), Some(&staged))
            .unwrap();
        tx.add_directory(&folder, Some(vec![]), true).unwrap();
        fs::write(folder.join("external"), b"keep").unwrap();
        assert!(tx.apply(|| Ok(())).is_err());
        assert_eq!(fs::read(database).unwrap(), b"before");
        assert_eq!(tx.inspect().unwrap().state, JournalState::Prepared);
    }
    #[test]
    fn unplanned_existing_child_blocks_removal_before_any_file_change() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let folder = root.join("folder");
        fs::create_dir(&folder).unwrap();
        let foreign = folder.join("foreign");
        fs::write(&foreign, b"keep").unwrap();
        let database = root.join("database");
        let staged = root.join("stage");
        fs::write(&database, b"before").unwrap();
        fs::write(&staged, b"after").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add(&database, file_hash(&database).unwrap(), Some(&staged))
            .unwrap();
        tx.add_directory(&folder, directory_manifest(&folder).unwrap(), false)
            .unwrap();
        assert!(tx.apply(|| Ok(())).is_err());
        assert_eq!(fs::read(database).unwrap(), b"before");
        assert_eq!(fs::read(foreign).unwrap(), b"keep");
    }
    fn subtree_move(root: &Path) -> (FileTransaction, PathBuf, PathBuf) {
        let old = root.join("old.subfolders");
        let new = root.join("new.subfolders");
        let child = old.join("child.subfolders");
        fs::create_dir_all(&child).unwrap();
        fs::write(child.join("set.vdjfolder"), b"playlist").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add_directory(&child, directory_manifest(&child).unwrap(), false)
            .unwrap();
        tx.add_directory(&old, directory_manifest(&old).unwrap(), false)
            .unwrap();
        tx.add_directory(&new.join("child.subfolders"), None, true)
            .unwrap();
        tx.add_directory(&new, None, true).unwrap();
        tx.add(
            &child.join("set.vdjfolder"),
            file_hash(&child.join("set.vdjfolder")).unwrap(),
            None,
        )
        .unwrap();
        tx.add(
            &new.join("child.subfolders/set.vdjfolder"),
            None,
            Some(&child.join("set.vdjfolder")),
        )
        .unwrap();
        (tx, old, new)
    }
    #[test]
    fn nested_subtree_moves_and_restores_all_original_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (tx, old, new) = subtree_move(&root);
        tx.apply(|| Ok(())).unwrap();
        assert!(!old.exists());
        assert_eq!(
            fs::read(new.join("child.subfolders/set.vdjfolder")).unwrap(),
            b"playlist"
        );
        tx.restore(|| Ok(())).unwrap();
        assert!(!new.exists());
        assert_eq!(
            fs::read(old.join("child.subfolders/set.vdjfolder")).unwrap(),
            b"playlist"
        );
    }
    #[test]
    fn interrupted_file_and_directory_actions_restore_after_reopening() {
        for phase in 0..3 {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let (tx, old, new) = subtree_move(&root);
            let id = tx.inspect().unwrap().id;
            let result = tx.apply(|| {
                let stop = match phase {
                    0 => new.exists() && !new.join("child.subfolders").exists(),
                    1 => {
                        !old.join("child.subfolders/set.vdjfolder").exists()
                            && !new.join("child.subfolders/set.vdjfolder").exists()
                    }
                    _ => old.exists() && !old.join("child.subfolders").exists(),
                };
                ensure!(!stop, "Injected interruption");
                Ok(())
            });
            assert!(result.is_err(), "phase {phase}");
            assert_eq!(tx.inspect().unwrap().state, JournalState::Applying);
            FileTransaction::open(&root.join("journals"), &id)
                .unwrap()
                .restore(|| Ok(()))
                .unwrap();
            assert!(!new.exists());
            assert_eq!(
                fs::read(old.join("child.subfolders/set.vdjfolder")).unwrap(),
                b"playlist"
            );
        }
    }
    #[test]
    fn restore_rejects_unknown_children_before_modifying_any_known_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (tx, old, new) = subtree_move(&root);
        tx.apply(|| Ok(())).unwrap();
        let foreign = new.join("child.subfolders/external");
        fs::write(&foreign, b"keep").unwrap();
        assert!(tx.restore(|| Ok(())).is_err());
        assert!(!old.exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert!(new.join("child.subfolders/set.vdjfolder").is_file());
        fs::remove_file(foreign).unwrap();
        tx.restore(|| Ok(())).unwrap();
        assert!(!new.exists());
    }
    #[test]
    fn interrupted_restore_reopens_and_completes_with_original_empty_parents() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (tx, old, new) = subtree_move(&root);
        tx.apply(|| Ok(())).unwrap();
        let id = tx.inspect().unwrap().id;
        assert!(
            tx.restore(|| {
                ensure!(
                    !old.join("child.subfolders").is_dir(),
                    "Injected interruption after directory recreation"
                );
                Ok(())
            })
            .is_err()
        );
        assert_eq!(tx.inspect().unwrap().state, JournalState::Restoring);
        FileTransaction::open(&root.join("journals"), &id)
            .unwrap()
            .restore(|| Ok(()))
            .unwrap();
        assert!(!new.exists());
        assert_eq!(
            fs::read(old.join("child.subfolders/set.vdjfolder")).unwrap(),
            b"playlist"
        );
    }
    #[test]
    fn typed_directory_mode_requires_explicit_missing_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add_directory(&root.join("missing/child"), None, true)
            .unwrap();
        assert!(tx.apply(|| Ok(())).is_err());
        assert!(!root.join("missing").exists());
    }
    #[test]
    fn old_file_journals_deserialize_without_directory_entries() {
        let journal: FileJournal = serde_json::from_value(
            serde_json::json!({"id":"legacy","state":"prepared","changes":[]}),
        )
        .unwrap();
        assert!(journal.directories.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn directory_manifests_do_not_follow_links_and_publication_rejects_link_ancestors() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let folder = root.join("folder");
        fs::create_dir(&folder).unwrap();
        symlink(outside.path(), folder.join("linked")).unwrap();
        let manifest = directory_manifest(&folder).unwrap().unwrap();
        assert_eq!(manifest[0].kind, DirectoryEntryKind::Symlink);
        assert!(directory_manifest(&folder.join("linked")).is_err());
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        assert!(
            tx.add_directory(&folder.join("linked/new"), None, true)
                .is_err()
        );
        assert!(!outside.path().join("new").exists());
    }
    #[test]
    fn recreated_wal_blocks_the_next_main_database_publication() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let database = root.join("database.db");
        let wal = root.join("database.db-wal");
        let stage = root.join("new.db");
        fs::write(&database, b"original database").unwrap();
        fs::write(&wal, b"original WAL").unwrap();
        fs::write(&stage, b"new database").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add(&wal, file_hash(&wal).unwrap(), None).unwrap();
        tx.add(&database, file_hash(&database).unwrap(), Some(&stage))
            .unwrap();
        let calls = std::cell::Cell::new(0);
        let result = tx.apply(|| {
            let n = calls.get();
            calls.set(n + 1);
            if n == 2 {
                fs::write(&wal, b"foreign recreated WAL")?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&database).unwrap(), b"original database");
        assert_eq!(fs::read(&wal).unwrap(), b"foreign recreated WAL");
        assert_eq!(tx.inspect().unwrap().state, JournalState::Applying);
    }
    #[test]
    fn durable_apply_restore_and_stale_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let target = root.join("database");
        let staged = root.join("new");
        fs::write(&target, b"original").unwrap();
        fs::write(&staged, b"updated").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add(&target, file_hash(&target).unwrap(), Some(&staged))
            .unwrap();
        let id = tx.inspect().unwrap().id;
        tx.apply(|| Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"updated");
        FileTransaction::open(&root.join("journals"), &id)
            .unwrap()
            .restore(|| Ok(()))
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"original");
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        tx.add(&target, file_hash(&target).unwrap(), Some(&staged))
            .unwrap();
        fs::write(&target, b"external").unwrap();
        assert!(tx.apply(|| Ok(())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"external");
    }
    #[test]
    fn partial_apply_is_recoverable_and_restore_refuses_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let staged = root.join("staged");
        fs::write(&staged, b"after").unwrap();
        let tx = FileTransaction::create(&root.join("journals")).unwrap();
        for name in ["one", "two"] {
            let p = root.join(name);
            fs::write(&p, b"before").unwrap();
            tx.add(&p, file_hash(&p).unwrap(), Some(&staged)).unwrap();
        }
        let calls = std::cell::Cell::new(0);
        assert!(
            tx.apply(|| {
                let n = calls.get();
                calls.set(n + 1);
                ensure!(n < 2, "Simulated process start");
                Ok(())
            })
            .is_err()
        );
        assert_eq!(tx.inspect().unwrap().state, JournalState::Applying);
        assert_eq!(fs::read(root.join("one")).unwrap(), b"after");
        fs::write(root.join("one"), b"foreign edit").unwrap();
        assert!(tx.restore(|| Ok(())).is_err());
        assert_eq!(fs::read(root.join("two")).unwrap(), b"before");
        fs::write(root.join("one"), b"after").unwrap();
        tx.restore(|| Ok(())).unwrap();
        assert_eq!(fs::read(root.join("one")).unwrap(), b"before");
    }
}
