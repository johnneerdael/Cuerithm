use super::{Snapshot, identity, row_identity};
use crate::{
    Library,
    artifacts::{Artifact, cleanup_temporaries, current_hash, owned_file, rollback, safe_target},
    model::{bytes_path, now, path_bytes},
    publish_files,
};
use anyhow::{Context, Result, ensure};
use cuerithm_autotag::StagedTagWrite;
use cuerithm_cleanup::CleanupReport;
use cuerithm_media::{MediaInfo, file_sha256};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Payload {
    pub before: Snapshot,
    pub metadata: MediaInfo,
    pub report: CleanupReport,
    pub post_identity: Option<String>,
    pub restore_row: Option<serde_json::Value>,
    pub undo_of: Option<String>,
}
pub(super) struct Journal {
    pub id: String,
    pub payload: Payload,
    pub workspace: PathBuf,
    pub state: String,
    pub files: Vec<Artifact>,
}
pub(super) fn load(lib: &Library, id: &str) -> Result<Journal> {
    let conn = lib.conn()?;
    let (payload, workspace, state): (String, Vec<u8>, String) = conn.query_row(
        "SELECT payload_json,workspace,state FROM cleanup_writes WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut q=conn.prepare("SELECT target,staged,backup,before_hash,after_hash,publish_path,displaced_path FROM cleanup_write_files WHERE write_id=?1 ORDER BY ordinal")?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Vec<u8>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let files = rows
        .into_iter()
        .map(|(a, b, c, d, e, f, g)| {
            Ok(Artifact {
                target: bytes_path(&a)?,
                staged: bytes_path(&b)?,
                backup: c.map(|p| bytes_path(&p)).transpose()?,
                before: d,
                after: e,
                publish: bytes_path(&f)?,
                displaced: bytes_path(&g)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Journal {
        id: id.into(),
        payload: serde_json::from_str(&payload)?,
        workspace: bytes_path(&workspace)?,
        state,
        files,
    })
}
pub(super) fn matches(lib: &Library, before: &Snapshot) -> Result<()> {
    let current = row_identity(&*lib.conn()?, &before.track_id)?;
    ensure!(
        identity(&current)? == before.identity,
        "Library metadata or preparation changed; rescan"
    );
    ensure!(
        current_hash(&bytes_path(&before.path)?)?.as_deref() == Some(before.source_sha256.as_str()),
        "Source changed; rescan"
    );
    Ok(())
}
pub(super) fn begin(
    lib: &Library,
    batch: &str,
    proposal: &str,
    payload: &Payload,
    staged: &mut StagedTagWrite,
) -> Result<String> {
    let workspace = staged.workspace().canonicalize()?;
    let root = lib.data_dir().canonicalize()?;
    ensure!(
        workspace.starts_with(&root),
        "Cleanup workspace leaves library"
    );
    ensure!(
        staged.source_before_sha256 == payload.before.source_sha256
            && staged.source_path == bytes_path(&payload.before.path)?,
        "Cleanup source differs"
    );
    let id = uuid::Uuid::new_v4().to_string();
    let parent = staged
        .source_path
        .parent()
        .context("Missing source folder")?;
    let publish = parent.join(format!(".cuerithm-cleanup-{id}.tmp"));
    let displaced = if cfg!(windows) {
        parent.join(format!(".cuerithm-cleanup-{id}-before.tmp"))
    } else {
        publish.clone()
    };
    let files = vec![Artifact {
        target: staged.source_path.clone(),
        staged: staged.path.clone(),
        backup: Some(staged.backup_path.clone()),
        before: Some(staged.source_before_sha256.clone()),
        after: staged.source_after_sha256.clone(),
        publish,
        displaced,
    }];
    insert(lib, batch, proposal, &id, payload, &workspace, &files)?;
    staged.retain_for_journal();
    Ok(id)
}
pub(super) fn insert(
    lib: &Library,
    batch: &str,
    proposal: &str,
    id: &str,
    payload: &Payload,
    workspace: &std::path::Path,
    files: &[Artifact],
) -> Result<()> {
    let root = lib.data_dir().canonicalize()?;
    ensure!(
        workspace.canonicalize()?.starts_with(&root),
        "Invalid cleanup workspace"
    );
    let source = bytes_path(&payload.before.path)?;
    let mut assets = vec![];
    for f in files {
        safe_target(&f.target, &source)?;
        owned_file(&f.staged, workspace, &f.after)?;
        assets.push(f.staged.clone());
        if let (Some(p), Some(h)) = (&f.backup, &f.before) {
            owned_file(p, workspace, h)?;
            assets.push(p.clone());
        }
        ensure!(
            current_hash(&f.target)? == f.before,
            "Cleanup destination changed"
        );
    }
    publish_files::durable_assets(&assets, workspace, &root)?;
    let mut conn = lib.conn()?;
    let tx = conn.transaction()?;
    ensure!(
        identity(&row_identity(&tx, &payload.before.track_id)?)? == payload.before.identity,
        "Library changed during staging"
    );
    for f in files {
        crate::maintenance::ensure_unused_unreserved(&tx, &[path_bytes(&f.target)])?;
        let busy:i64=tx.query_row("SELECT (SELECT COUNT(*) FROM tag_write_files f JOIN tag_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared')+(SELECT COUNT(*) FROM cleanup_write_files f JOIN cleanup_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared') + (SELECT COUNT(*) FROM maintenance_write_files f JOIN maintenance_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','conflict'))",[path_bytes(&f.target)],|r|r.get(0))?;
        ensure!(busy == 0, "Metadata destination is reserved");
    }
    let after = files
        .first()
        .map(|f| f.after.as_str())
        .unwrap_or(&payload.before.source_sha256);
    tx.execute("INSERT INTO cleanup_writes(id,batch_id,track_id,proposal_id,before_hash,after_hash,payload_json,workspace,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![id,batch,payload.before.track_id,proposal,payload.before.source_sha256,after,serde_json::to_string(payload)?,path_bytes(workspace),now()])?;
    for (n, f) in files.iter().enumerate() {
        tx.execute(
            "INSERT INTO cleanup_write_files VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                id,
                n,
                path_bytes(&f.target),
                path_bytes(&f.staged),
                f.backup.as_ref().map(|p| path_bytes(p)),
                f.before,
                f.after,
                path_bytes(&f.publish),
                path_bytes(&f.displaced)
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub(super) fn set_custom_tags(
    conn: &Connection,
    id: &str,
    tags: &[cuerithm_cleanup::CustomTag],
) -> Result<()> {
    let mut groups =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for tag in tags {
        let (group, tag_id) =
            crate::curation::resolve_category_value(conn, &tag.category, &tag.value, true)?;
        groups.entry(group.clone()).or_default().insert(tag_id);
        let single: bool = conn.query_row("SELECT COALESCE((SELECT mode='single' FROM curation_group_properties WHERE category_id=?1),0)",[&group],|row|row.get(0))?;
        ensure!(
            !single || groups[&group].len() <= 1,
            "Cleanup output contains multiple values for a single-select Curate category"
        );
    }
    conn.execute("DELETE FROM track_custom_tags WHERE track_id=?1", [id])?;
    for tags in groups.values() {
        for tag_id in tags {
            conn.execute(
                "INSERT INTO track_custom_tags VALUES(?1,?2) ON CONFLICT DO NOTHING",
                params![id, tag_id],
            )?;
        }
    }
    Ok(())
}
fn commit_metadata(conn: &Connection, payload: &Payload, after: &str) -> Result<()> {
    use cuerithm_cleanup::MetadataField as F;
    let old = &payload.before.row["row"];
    let target = payload.restore_row.as_ref().map(|v| &v["row"]);
    let field = |f: F, name: &str| -> String {
        if let Some(row) = target {
            return row[name].as_str().unwrap_or("").into();
        };
        if payload.report.changes.iter().any(|c| c.field == f) {
            payload
                .report
                .metadata
                .fields
                .get(&f)
                .map(|v| v.join(", "))
                .unwrap_or_default()
        } else {
            old[name].as_str().unwrap_or("").into()
        }
    };
    let title = field(F::Title, "title");
    let artist = field(F::Artist, "artist");
    let album = field(F::Album, "album");
    let genre = field(F::Genre, "genre");
    // The file the write produced is the one the track now has, whichever way it got its tags.
    let metadata = serde_json::to_string(&payload.metadata)?;
    let cleanup = if let Some(row) = target {
        row["cleanup_json"].as_str().map(str::to_owned)
    } else {
        Some(serde_json::to_string(&payload.report)?)
    };
    conn.execute("UPDATE tracks SET title=?1,artist=?2,album=?3,genre=?4,source_hash=?5,metadata_json=?6,cleanup_json=?7,search_text=?8,sort_title=?9,sort_artist=?10,updated=?11 WHERE id=?12",params![title,artist,album,genre,after,metadata,cleanup,format!("{title} {artist} {genre}").to_lowercase(),title.to_lowercase(),artist.to_lowercase(),now(),payload.before.track_id])?;
    set_custom_tags(
        conn,
        &payload.before.track_id,
        &payload.report.metadata.custom_tags,
    )?;
    super::store::store_native(conn, &payload.before.track_id, &payload.report.metadata)?;
    if let Some(id) = &payload.undo_of {
        ensure!(
            conn.execute(
                "UPDATE cleanup_writes SET state='undone' WHERE id=?1 AND state='committed'",
                [id]
            )? == 1,
            "Cleanup was already undone"
        );
    }
    Ok(())
}
/// The write has settled and its private copies are gone: say so in the journal.
fn retire(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE cleanup_write_files SET backup=NULL WHERE write_id=?1",
        [id],
    )?;
    conn.execute(
        "UPDATE cleanup_writes SET cleanup_complete=1 WHERE id=?1",
        [id],
    )?;
    Ok(())
}
pub(super) fn publish(lib: &Library, id: &str) -> Result<()> {
    let _gate = lib
        .inner
        .tag_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
    let mut journal = load(lib, id)?;
    ensure!(
        journal.state == "prepared",
        "Cleanup journal is no longer pending"
    );
    let source = bytes_path(&journal.payload.before.path)?;
    let prepared = (|| -> Result<()> {
        ensure!(
            journal
                .workspace
                .canonicalize()?
                .starts_with(lib.data_dir().canonicalize()?),
            "Invalid cleanup workspace"
        );
        for f in &journal.files {
            safe_target(&f.target, &source)?;
            owned_file(&f.staged, &journal.workspace, &f.after)?;
            if f.before.as_ref() != Some(&f.after) && !f.publish.exists() {
                publish_files::copy_prepared(&f.staged, &f.publish, &f.after)?;
            }
        }
        Ok(())
    })();
    let mut conn = lib.conn()?;
    let result = prepared.and_then(|_| {
        let tx = conn.transaction()?;
        ensure!(
            identity(&row_identity(&tx, &journal.payload.before.track_id)?)?
                == journal.payload.before.identity,
            "Library changed before cleanup publication"
        );
        ensure!(
            current_hash(&source)?.as_deref()
                == Some(journal.payload.before.source_sha256.as_str()),
            "Source changed before cleanup publication"
        );
        for f in &journal.files {
            safe_target(&f.target, &source)?;
            ensure!(
                current_hash(&f.target)? == f.before,
                "Cleanup destination changed"
            );
            if f.before.as_ref() != Some(&f.after) {
                ensure!(
                    file_sha256(&f.publish)? == f.after,
                    "Cleanup publication copy changed"
                );
            }
        }
        for f in &journal.files {
            if f.before.as_ref() == Some(&f.after) {
                continue;
            }
            if let Some(before) = &f.before {
                ensure!(
                    !std::fs::metadata(&f.target)?.permissions().readonly(),
                    "Cleanup target is read only"
                );
                publish_files::replace(&f.publish, &f.target, &f.displaced)?;
                ensure!(
                    file_sha256(&f.displaced)? == *before,
                    "Source changed during cleanup replacement"
                );
            } else {
                publish_files::create(&f.publish, &f.target)?;
            }
            ensure!(
                file_sha256(&f.target)? == f.after,
                "Published cleanup changed"
            );
        }
        let after = journal
            .files
            .first()
            .map(|f| f.after.as_str())
            .unwrap_or(&journal.payload.before.source_sha256);
        commit_metadata(&tx, &journal.payload, after)?;
        Library::bump(&tx)?;
        journal.payload.post_identity = Some(identity(&row_identity(
            &tx,
            &journal.payload.before.track_id,
        )?)?);
        tx.execute(
            "UPDATE cleanup_writes SET state='committed',payload_json=?1,error=NULL WHERE id=?2",
            params![serde_json::to_string(&journal.payload)?, id],
        )?;
        tx.commit()?;
        Ok(())
    });
    if let Err(error) = result {
        let rollback = rollback(&journal.files);
        let message = rollback
            .as_ref()
            .err()
            .map(|e| format!("{error:#}; recovery needs attention: {e:#}"))
            .unwrap_or_else(|| format!("{error:#}"));
        conn.execute(
            "UPDATE cleanup_writes SET state=?1,error=?2 WHERE id=?3",
            params![
                if rollback.is_ok() {
                    "rolled_back"
                } else {
                    "conflict"
                },
                message,
                id
            ],
        )?;
        if rollback.is_ok() && cleanup_temporaries(&journal.files, lib.data_dir(), true).is_ok() {
            retire(&conn, id)?;
        }
        return Err(error);
    }
    if cleanup_temporaries(&journal.files, lib.data_dir(), true).is_ok() {
        retire(&conn, id)?;
    }
    Ok(())
}

impl Library {
    pub(crate) fn recover_cleanup_writes(&self) -> Result<()> {
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let pending = {
            let conn = self.conn()?;
            let mut q = conn.prepare(
                "SELECT id FROM cleanup_writes WHERE state='prepared' ORDER BY created,id",
            )?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in pending {
            let journal = load(self, &id)?;
            let outcome = rollback(&journal.files);
            self.conn()?.execute(
                "UPDATE cleanup_writes SET state=?1,error=?2 WHERE id=?3",
                params![
                    if outcome.is_ok() {
                        "rolled_back"
                    } else {
                        "conflict"
                    },
                    outcome
                        .err()
                        .map(|e| format!("{e:#}"))
                        .unwrap_or_else(|| "Interrupted cleanup rolled back".into()),
                    id
                ],
            )?;
        }
        let cleanup = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT id FROM cleanup_writes WHERE state IN ('committed','rolled_back','undone') AND cleanup_complete=0")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in cleanup {
            let journal = load(self, &id)?;
            if cleanup_temporaries(&journal.files, self.data_dir(), true).is_ok() {
                retire(&*self.conn()?, &id)?;
            }
        }
        let batches = {
            let conn = self.conn()?;
            let mut q =
                conn.prepare("SELECT id,result_json FROM cleanup_batches WHERE state='running'")?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, json) in batches {
            let mut result: super::CleanupBatchResult = serde_json::from_str(&json)?;
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT track_id,state,error FROM cleanup_writes WHERE batch_id=?1 ORDER BY created,id")?;
            let writes = q
                .query_map([&id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (track, state, error) in writes {
                if result.applied.contains(&track)
                    || result.unchanged.contains(&track)
                    || result
                        .conflicted
                        .iter()
                        .chain(&result.failed)
                        .any(|f| f.track_id == track)
                {
                    continue;
                }
                match state.as_str() {
                    "committed" | "undone" => result.applied.push(track),
                    "conflict" => result.conflicted.push(super::CleanupFailure {
                        track_id: track,
                        message: error.unwrap_or_else(|| "Recovery needs attention".into()),
                    }),
                    _ => result.failed.push(super::CleanupFailure {
                        track_id: track,
                        message: error.unwrap_or_else(|| "Interrupted cleanup".into()),
                    }),
                }
            }
            conn.execute(
                "UPDATE cleanup_batches SET state='interrupted',result_json=?1 WHERE id=?2",
                params![serde_json::to_string(&result)?, id],
            )?;
        }
        Ok(())
    }
}
