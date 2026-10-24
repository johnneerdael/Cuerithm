//! Direct-parent cleanup follows the primary-file/sidecar order of the source helper.
use super::*;
use std::{collections::BTreeMap, ffi::OsString};
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Folder {
    path: Vec<u8>,
    identity: Identity,
    mode: Option<u32>,
    readonly: bool,
    restored: Option<Identity>,
}
fn inventory(path: &Path) -> Result<BTreeMap<OsString, bool>> {
    ensure!(
        std::fs::symlink_metadata(path)?.is_dir(),
        "Cleanup directory changed"
    );
    std::fs::read_dir(path)?
        .map(|row| {
            let row = row?;
            Ok((row.file_name(), row.file_type()?.is_file()))
        })
        .collect()
}
pub(super) fn plan(
    entries: &mut Vec<Entry>,
    root: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<Folder>> {
    let mut folders = vec![];
    let mut removed = BTreeSet::new();
    let mut files = entries
        .iter()
        .map(|e| e.path.clone())
        .collect::<BTreeSet<_>>();
    let mut virtuals: BTreeMap<PathBuf, BTreeMap<OsString, bool>> = BTreeMap::new();
    for e in entries.clone() {
        cancel.check()?;
        let path = bytes_path(&e.path)?;
        let parent = path.parent().context("Missing cleanup parent")?;
        if !virtuals.contains_key(parent) {
            virtuals.insert(parent.to_owned(), inventory(parent)?);
        }
        let children = virtuals.get_mut(parent).unwrap();
        children.remove(path.file_name().context("Missing filename")?);
        if e.public.role != UnusedRemovalRole::Selected
            || parent == root
            || removed.contains(parent)
        {
            continue;
        }
        if !children
            .iter()
            .all(|(name, regular)| *regular && (name == ".DS_Store" || name == "Thumbs.db"))
        {
            continue;
        }
        guard(parent, root)?;
        for name in children.keys() {
            let metadata = parent.join(name);
            if files.insert(path_bytes(&metadata)) {
                entries.push(entry(
                    &metadata,
                    root,
                    UnusedRemovalRole::FolderMetadata,
                    cancel,
                )?);
            }
        }
        children.clear();
        let meta = std::fs::symlink_metadata(parent)?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            Some(meta.permissions().mode())
        };
        #[cfg(windows)]
        let mode = None;
        folders.push(Folder {
            path: path_bytes(parent),
            identity: identity_kind(parent, true)?,
            mode,
            readonly: meta.permissions().readonly(),
            restored: None,
        });
        removed.insert(parent.to_owned());
        if let Some(grand) = parent.parent() {
            if !virtuals.contains_key(grand) {
                virtuals.insert(grand.to_owned(), inventory(grand)?);
            }
            virtuals
                .get_mut(grand)
                .unwrap()
                .remove(parent.file_name().context("Missing folder name")?);
        }
    }
    folders.sort_by_key(|f| std::cmp::Reverse(f.path.len()));
    Ok(folders)
}
pub(super) fn display(folders: &[Folder]) -> Result<Vec<String>> {
    folders
        .iter()
        .map(|f| Ok(bytes_path(&f.path)?.to_string_lossy().into()))
        .collect()
}
pub(super) fn validate(folders: &[Folder], root: &Path) -> Result<()> {
    for f in folders {
        let path = bytes_path(&f.path)?;
        guard(&path, root)?;
        ensure!(
            identity_kind(&path, true)? == f.identity,
            "Cleanup directory identity changed"
        );
    }
    Ok(())
}
pub(super) fn guard_missing(path: &Path, root: &Path) -> Result<()> {
    ensure!(
        root.canonicalize()? == root && std::fs::symlink_metadata(root)?.is_dir(),
        "Removal root changed"
    );
    ensure!(
        path.is_absolute() && path.starts_with(root) && path != root,
        "Recovery path leaves reviewed root"
    );
    let parent = path.parent().context("Missing recovery parent")?;
    ensure!(
        super::super::files::resolved_prefix(parent)? == parent,
        "Recovery parent changed or became a symlink"
    );
    Ok(())
}
pub(super) fn cleanup(op: &Operation) -> Result<()> {
    #[cfg(test)]
    super::tests::before_folder_cleanup(op);
    let remove = op.kind == "remove" && op.state == "committed" && op.undone_by.is_none();
    let rollback = op.kind == "restore" && op.state == "rolled_back";
    if !remove && !rollback {
        return Ok(());
    }
    let root = bytes_path(&op.root)?;
    for folder in &op.directories {
        let path = bytes_path(&folder.path)?;
        guard_missing(&path, &root)?;
        if missing(&path)? {
            continue;
        }
        let expected = if remove {
            Some(&folder.identity)
        } else {
            folder.restored.as_ref()
        };
        if expected.is_none() {
            continue;
        }
        ensure!(
            expected == Some(&identity_kind(&path, true)?),
            "Newer directory preserved during cleanup"
        );
        // Never recurse: a new file or directory must prevent removal.
        match std::fs::remove_dir(&path) {
            Ok(()) => crate::publish_files::sync_parent(&path)?,
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(e) => return Err(e).context("Remove reviewed empty directory"),
        }
    }
    Ok(())
}
