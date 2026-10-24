use crate::publish_files;
use anyhow::{Context, Result, bail, ensure};
use cuerithm_media::file_sha256;
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub(crate) struct Artifact {
    pub(crate) target: PathBuf,
    pub(crate) staged: PathBuf,
    pub(crate) backup: Option<PathBuf>,
    pub(crate) before: Option<String>,
    pub(crate) after: String,
    pub(crate) publish: PathBuf,
    pub(crate) displaced: PathBuf,
}

pub(crate) fn current_hash(path: &Path) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Publication target is not a regular file"
            );
            Ok(Some(file_sha256(path)?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn owned_file(path: &Path, workspace: &Path, hash: &str) -> Result<()> {
    ensure!(
        path.canonicalize()?.starts_with(workspace),
        "Staged asset leaves its workspace"
    );
    ensure!(file_sha256(path)? == hash, "Staged asset hash mismatch");
    Ok(())
}
pub(crate) fn safe_target(target: &Path, source: &Path) -> Result<()> {
    let parent = source.parent().context("Missing source folder")?;
    ensure!(
        target.starts_with(parent),
        "Tag destination leaves source folder"
    );
    let mut existing = target.parent().context("Missing destination folder")?;
    while !existing.exists() {
        existing = existing.parent().context("Missing destination ancestor")?;
    }
    ensure!(
        existing.canonicalize()?.starts_with(parent),
        "Destination folder changed"
    );
    Ok(())
}
pub(crate) fn rollback(files: &[Artifact]) -> Result<()> {
    rollback_with_proof(files, |_, _| Ok(()))
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ArtifactRestoreProof {
    pub path: crate::organization::NativePath,
    pub parent: crate::organization::Authority,
    pub fingerprint: crate::organization::Fingerprint,
    #[serde(skip)]
    pub from_backup: bool,
    #[serde(default)]
    pub displaced: Option<crate::organization::NativePath>,
}
#[derive(Clone)]
pub(crate) struct RollbackAuthority {
    pub parent: crate::organization::Authority,
    pub before: Option<crate::organization::Fingerprint>,
    pub after: Option<crate::organization::Fingerprint>,
    pub backup: Option<ArtifactRestoreProof>,
}
pub(crate) fn restore_is_owned(
    proof: &ArtifactRestoreProof,
    original: Option<&crate::organization::Fingerprint>,
) -> bool {
    original.is_some_and(|original| {
        original.sha256 == proof.fingerprint.sha256
            && original.bytes == proof.fingerprint.bytes
            && (proof.from_backup || original.identity == proof.fingerprint.identity)
    })
}
pub(crate) fn rollback_with_proof(
    files: &[Artifact],
    before_restore: impl FnMut(&Artifact, &ArtifactRestoreProof) -> Result<()>,
) -> Result<()> {
    rollback_with_authority(files, &[], before_restore)
}
pub(crate) fn rollback_with_authority(
    files: &[Artifact],
    authorities: &[RollbackAuthority],
    before_restore: impl FnMut(&Artifact, &ArtifactRestoreProof) -> Result<()>,
) -> Result<()> {
    rollback_with_authority_and_copy_hook(files, authorities, before_restore, |_, _| Ok(()))
}
fn rollback_with_authority_and_copy_hook(
    files: &[Artifact],
    authorities: &[RollbackAuthority],
    mut before_restore: impl FnMut(&Artifact, &ArtifactRestoreProof) -> Result<()>,
    mut after_copy: impl FnMut(&Path, &crate::organization::Fingerprint) -> Result<()>,
) -> Result<()> {
    ensure!(
        authorities.is_empty() || authorities.len() == files.len(),
        "Missing native rollback authority; all versions retained"
    );
    use crate::organization::{NativeParent, NativePath};
    let mut errors = Vec::new();
    for (ordinal, f) in files.iter().enumerate().rev() {
        let result = (|| -> Result<()> {
            let directory = f.target.parent().context("Missing rollback parent")?;
            let (parent, authority) = if let Some(expected) = authorities.get(ordinal) {
                (
                    NativeParent::reopen(&expected.parent)?,
                    expected.parent.clone(),
                )
            } else {
                NativeParent::capture(directory)?
            };
            let target = Path::new(f.target.file_name().context("Missing rollback name")?);
            let current = parent.optional(target)?;
            if current.as_ref().map(|p| &p.sha256) == f.before.as_ref() {
                if let Some(expected) = authorities.get(ordinal) {
                    ensure!(
                        match (&current, &expected.before) {
                            (Some(current), Some(before)) =>
                                crate::organization::same_native_file(current, before),
                            (None, None) => true,
                            _ => false,
                        },
                        "Unchanged bytes belong to another native object; preserved"
                    );
                    NativeParent::reopen(&authority)?;
                }
                return Ok(());
            }
            let current =
                current.context("Published target disappeared; recovery evidence retained")?;
            ensure!(
                current.sha256 == f.after,
                "Later user edit preserved; original backup retained"
            );
            if let Some(expected) = authorities.get(ordinal) {
                ensure!(
                    expected.after.as_ref().is_some_and(|after| {
                        crate::organization::same_native_file(&current, after)
                    }),
                    "Published target native identity changed; latest target and all recovery versions retained"
                );
            }
            if let Some(before) = &f.before {
                let displaced_name =
                    Path::new(f.displaced.file_name().context("Missing displaced name")?);
                let mut from_backup = false;
                let mut copied_receipt = None;
                let restore = if parent
                    .optional(displaced_name)?
                    .is_some_and(|p| p.sha256 != f.after)
                {
                    displaced_name.to_owned()
                } else {
                    from_backup = true;
                    let backup = f.backup.as_ref().context("Missing original backup")?;
                    let name =
                        PathBuf::from(format!(".cuerithm-restore-{}.tmp", uuid::Uuid::new_v4()));
                    let cancel = cuerithm_media::CancellationToken::default();
                    let created = if let Some(authority) = authorities.get(ordinal) {
                        let receipt = authority
                            .backup
                            .as_ref()
                            .context("Missing original backup native receipt; retained")?;
                        ensure!(
                            receipt.path == NativePath::new(backup),
                            "Original backup path changed; retained"
                        );
                        parent.copy_prepared_verified(
                            backup,
                            &name,
                            before,
                            &cancel,
                            &receipt.parent,
                            &receipt.fingerprint,
                        )?
                    } else {
                        parent.copy_prepared(backup, &name, before, &cancel)?
                    };
                    after_copy(&directory.join(&name), &created)?;
                    copied_receipt = Some(created);
                    name
                };
                let visible = parent.fingerprint(&restore)?;
                let fingerprint = if let Some(created) = copied_receipt {
                    ensure!(
                        crate::organization::same_native_file(&visible, &created),
                        "Created restore copy changed before receipt; all versions retained"
                    );
                    created
                } else {
                    visible
                };
                // The durable callback records this exact inode before exchanging it.
                // An externally captured version may be restored, but its different
                // hash can never authorize replay of the original reviewed decision.
                let displaced = if cfg!(windows) {
                    restore.with_extension("after.tmp")
                } else {
                    restore.clone()
                };
                before_restore(
                    f,
                    &ArtifactRestoreProof {
                        path: NativePath::new(&f.target),
                        parent: authority.clone(),
                        fingerprint: fingerprint.clone(),
                        from_backup,
                        displaced: Some(NativePath::new(&directory.join(&displaced))),
                    },
                )?;
                ensure!(
                    crate::organization::same_native_file(
                        &parent.fingerprint(&restore)?,
                        &fingerprint
                    ),
                    "Restore leaf changed before exchange; all versions retained"
                );
                parent.exchange(&restore, target, &displaced)?;
                let captured = parent.fingerprint(&displaced)?;
                ensure!(
                    captured.identity == current.identity && captured.sha256 == current.sha256,
                    "Concurrent user edit retained in displaced recovery file; manual recovery required"
                );
                let restored = parent.fingerprint(target)?;
                ensure!(
                    restored.identity == fingerprint.identity
                        && restored.sha256 == fingerprint.sha256,
                    "Restored source changed; all recovery versions retained"
                );
                // Known displaced output remains journal evidence until ordinary
                // guarded cleanup. Never remove an unknown exchanged object here.
            } else {
                let captured =
                    PathBuf::from(format!(".cuerithm-rollback-{}.tmp", uuid::Uuid::new_v4()));
                // An absent original needs retirement, not a source restore.
                // Persist the generated leaf and exact published identity before
                // moving it so a crash cannot leave unaccounted source copies.
                before_restore(
                    f,
                    &ArtifactRestoreProof {
                        path: NativePath::new(&f.target),
                        parent: authority.clone(),
                        fingerprint: current.clone(),
                        from_backup: false,
                        displaced: Some(NativePath::new(&directory.join(&captured))),
                    },
                )?;
                parent.rename(target, &parent, &captured)?;
                let observed = parent.fingerprint(&captured)?;
                if observed.identity != current.identity || observed.sha256 != current.sha256 {
                    if parent.optional(target)?.is_none() {
                        parent.rename(&captured, &parent, target)?;
                    }
                    bail!("Concurrent artwork edit preserved");
                }
            }
            if !authorities.is_empty() {
                NativeParent::reopen(&authority)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            errors.push(format!("{e:#}"));
        }
    }
    ensure!(errors.is_empty(), "{}", errors.join("; "));
    Ok(())
}
/// Removes the private copies of a settled write, in the order that never leaves the user
/// without a way back: the staged and displaced copies first (the displaced original is the live
/// restore point until the write has committed), the backup of the original last and only when
/// `retire_backup` says nothing will need it. A backup that an earlier pass already retired is
/// not an error. A write that changed nothing never needed one.
pub(crate) fn cleanup_temporaries(
    files: &[Artifact],
    root: &Path,
    retire_backup: bool,
) -> Result<()> {
    for file in files {
        let changed = file
            .before
            .as_ref()
            .is_some_and(|before| before != &file.after);
        if changed {
            match file.backup.as_ref() {
                // While the original still exists only as the displaced file, the backup must
                // be intact.
                Some(backup) if backup.exists() || !retire_backup => {
                    ensure!(
                        file_sha256(backup)? == *file.before.as_ref().unwrap(),
                        "Original backup is unavailable; temporary recovery bytes retained"
                    );
                    // Also cover journals written by older versions before the explicit
                    // pre-journal durability barrier existed.
                    publish_files::durable_assets(
                        std::slice::from_ref(backup),
                        &backup
                            .parent()
                            .context("Missing backup workspace")?
                            .canonicalize()?,
                        &root.canonicalize()?,
                    )?;
                }
                // Retired by an earlier pass, which removes the displaced original first. If
                // the original is still lying around, something else took the backup: keep it.
                _ if retire_backup => {
                    for path in [&file.publish, &file.displaced] {
                        ensure!(
                            current_hash(path)?.as_ref() != file.before.as_ref(),
                            "Original backup is unavailable; temporary recovery bytes retained"
                        );
                    }
                }
                _ => anyhow::bail!("Missing durable original backup"),
            }
        }
        let mut removed = std::collections::HashSet::new();
        for path in [&file.staged, &file.publish, &file.displaced] {
            if !removed.insert(path) {
                continue;
            }
            if let Some(hash) = current_hash(path)? {
                ensure!(
                    hash == file.after || file.before.as_ref() == Some(&hash),
                    "Changed temporary file retained"
                );
                std::fs::remove_file(path)?;
                publish_files::sync_parent(path)?;
            }
        }
        if (!changed || retire_backup)
            && let Some(backup) = &file.backup
            && let Some(hash) = current_hash(backup)?
        {
            ensure!(
                Some(&hash) == file.before.as_ref(),
                "Changed backup retained"
            );
            std::fs::remove_file(backup)?;
            publish_files::sync_parent(backup)?;
            // The private folder goes with its last file.
            if let Some(folder) = backup.parent()
                && folder.canonicalize().ok() != root.canonicalize().ok()
            {
                let _ = std::fs::remove_dir(folder);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn backup_restore_fixture() -> (tempfile::TempDir, Artifact, RollbackAuthority) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("source");
        let backup = root.join("original");
        std::fs::write(&target, b"published output").unwrap();
        std::fs::write(&backup, b"original input").unwrap();
        let (parent, after) = crate::organization::capture_regular_source(&target).unwrap();
        let (backup_parent, before) = crate::organization::capture_regular_source(&backup).unwrap();
        let authority = RollbackAuthority {
            parent,
            before: Some(before.clone()),
            after: Some(after.clone()),
            backup: Some(ArtifactRestoreProof {
                path: crate::organization::NativePath::new(&backup),
                parent: backup_parent,
                fingerprint: before.clone(),
                from_backup: false,
                displaced: None,
            }),
        };
        let file = Artifact {
            target,
            staged: root.join("staged"),
            backup: Some(backup),
            before: Some(before.sha256),
            after: after.sha256,
            publish: root.join("publication"),
            displaced: root.join("missing-displaced"),
        };
        (temp, file, authority)
    }
    #[test]
    fn restored_copy_receipt_never_adopts_a_same_byte_replacement_after_copy() {
        let (_temp, file, authority) = backup_restore_fixture();
        let mut accepted = 0;
        let result = rollback_with_authority_and_copy_hook(
            std::slice::from_ref(&file),
            std::slice::from_ref(&authority),
            |_, proof| {
                if restore_is_owned(proof, authority.before.as_ref()) {
                    accepted += 1;
                }
                Ok(())
            },
            |path, _created| {
                let bytes = std::fs::read(path)?;
                std::fs::rename(path, path.with_extension("original-copy"))?;
                std::fs::write(path, bytes)?;
                Ok(())
            },
        );
        assert!(
            result.is_err(),
            "unknown replacement of the created copy must be refused"
        );
        assert_eq!(
            accepted, 0,
            "unknown identity must never become durable replay authority"
        );
        assert_eq!(std::fs::read(&file.target).unwrap(), b"published output");
    }
    #[test]
    fn backup_restore_requires_the_saved_native_input_receipt() {
        let (_temp, file, authority) = backup_restore_fixture();
        let backup = file.backup.as_ref().unwrap();
        let bytes = std::fs::read(backup).unwrap();
        std::fs::rename(backup, backup.with_extension("retained-original")).unwrap();
        std::fs::write(backup, bytes).unwrap();
        let mut accepted = 0;
        let result = rollback_with_authority(
            std::slice::from_ref(&file),
            std::slice::from_ref(&authority),
            |_, _| {
                accepted += 1;
                Ok(())
            },
        );
        assert!(
            result.is_err(),
            "equal bytes cannot replace the saved backup's native identity"
        );
        assert_eq!(accepted, 0);
        assert_eq!(std::fs::read(&file.target).unwrap(), b"published output");
    }
    #[test]
    fn rollback_persists_the_known_restore_inode_before_changing_the_source_namespace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("source");
        let displaced = root.join("displaced");
        let backup = root.join("backup");
        std::fs::write(&target, b"published metadata").unwrap();
        std::fs::write(&displaced, b"before metadata").unwrap();
        std::fs::write(&backup, b"before metadata").unwrap();
        let file = Artifact {
            target: target.clone(),
            staged: root.join("staged"),
            backup: Some(backup),
            before: Some(file_sha256(&displaced).unwrap()),
            after: file_sha256(&target).unwrap(),
            publish: displaced.clone(),
            displaced,
        };
        let mut saved = None;
        rollback_with_proof(&[file], |_, proof| {
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"published metadata",
                "intent must persist before atomic restore"
            );
            saved = Some(proof.clone());
            Ok(())
        })
        .unwrap();
        let proof = saved.expect("owned rollback must emit a pre-publication restore identity");
        let (_, actual) = crate::organization::capture_regular_source(&target).unwrap();
        assert_eq!(proof.path, crate::organization::NativePath::new(&target));
        assert_eq!(actual.identity, proof.fingerprint.identity);
        assert_eq!(actual.sha256, proof.fingerprint.sha256);
    }
    #[test]
    fn equal_byte_unknown_displaced_file_cannot_authorize_review_replay() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source");
        let displaced = root.join("displaced");
        let backup = root.join("known-original");
        std::fs::write(&source, b"before").unwrap();
        let (_, original) = crate::organization::capture_regular_source(&source).unwrap();
        std::fs::rename(&source, &backup).unwrap();
        std::fs::write(&source, b"after").unwrap();
        std::fs::write(&displaced, b"before").unwrap();
        let (_, unknown) = crate::organization::capture_regular_source(&displaced).unwrap();
        let file = Artifact {
            target: source.clone(),
            staged: root.join("stage"),
            backup: Some(backup.clone()),
            before: Some(original.sha256.clone()),
            after: file_sha256(&source).unwrap(),
            publish: displaced.clone(),
            displaced,
        };
        let mut authorized = None;
        rollback_with_proof(&[file], |_, proof| {
            authorized = Some(restore_is_owned(proof, Some(&original)));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            authorized,
            Some(false),
            "same bytes in an unknown external inode cannot become our durable restore authority"
        );
        let (_, restored) = crate::organization::capture_regular_source(&source).unwrap();
        assert_eq!(restored.identity, unknown.identity);
        assert!(backup.exists(), "known original backup remains available");
    }
    #[test]
    fn rollback_restores_the_user_version_captured_by_atomic_exchange() {
        let d = tempfile::tempdir().unwrap();
        let directory = d.path().canonicalize().unwrap();
        let target = directory.join("audio");
        let displaced = directory.join("displaced");
        let backup = directory.join("backup");
        std::fs::write(&target, b"our metadata output").unwrap();
        std::fs::write(&displaced, b"latest user edit").unwrap();
        std::fs::write(&backup, b"expected original").unwrap();
        let artifact = Artifact {
            target: target.clone(),
            staged: directory.join("staged"),
            backup: Some(backup.clone()),
            before: Some(file_sha256(&backup).unwrap()),
            after: file_sha256(&target).unwrap(),
            publish: if cfg!(windows) {
                directory.join("publish")
            } else {
                displaced.clone()
            },
            displaced,
        };
        rollback(&[artifact]).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"latest user edit");
        assert_eq!(std::fs::read(backup).unwrap(), b"expected original");
    }
    #[test]
    fn cleanup_keeps_displaced_original_when_its_durable_backup_is_missing() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("target");
        let displaced = d.path().join("old");
        std::fs::write(&target, b"new").unwrap();
        std::fs::write(&displaced, b"original").unwrap();
        let artifact = Artifact {
            target: target.clone(),
            staged: d.path().join("stage"),
            backup: Some(d.path().join("missing")),
            before: Some(file_sha256(&displaced).unwrap()),
            after: file_sha256(&target).unwrap(),
            publish: displaced.clone(),
            displaced: displaced.clone(),
        };
        assert!(cleanup_temporaries(std::slice::from_ref(&artifact), d.path(), true).is_err());
        assert!(cleanup_temporaries(&[artifact], d.path(), false).is_err());
        assert_eq!(std::fs::read(displaced).unwrap(), b"original");
    }
}
