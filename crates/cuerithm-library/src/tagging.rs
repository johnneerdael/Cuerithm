use crate::artifacts::{
    Artifact, ArtifactRestoreProof, current_hash, owned_file, rollback_with_authority, safe_target,
};
use crate::{
    JobRun, Library, SettingsSnapshot,
    model::{bytes_path, now, path_bytes},
    publish_files,
};
use anyhow::{Context, Result, ensure};
use camelot_core::FileAnalysis;
use cuerithm_autotag::StagedTagWrite;
use cuerithm_media::{MediaInfo, file_sha256};
use rusqlite::{Connection, params};
use std::path::PathBuf;
mod native_artifacts;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagPublicationBoundary {
    CopiesPrepared,
    BeforeExchange,
    Published,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ArtifactAuthority {
    path: crate::organization::NativePath,
    parent: crate::organization::Authority,
    before: Option<crate::organization::Fingerprint>,
    #[serde(default)]
    published: Option<crate::organization::Fingerprint>,
    #[serde(default)]
    private_staged: Option<ArtifactRestoreProof>,
    #[serde(default)]
    private_backup: Option<ArtifactRestoreProof>,
    #[serde(default)]
    retired: Vec<ArtifactRestoreProof>,
}
struct Journal {
    id: String,
    job: String,
    track: String,
    attempt: u32,
    revision: String,
    before: String,
    after: String,
    analysis: FileAnalysis,
    metadata: MediaInfo,
    provenance: serde_json::Value,
    cleanup: Option<cuerithm_cleanup::CleanupReport>,
    native_metadata: Option<cuerithm_cleanup::Metadata>,
    editor_receipt: Option<String>,
    input_source_proof: Option<String>,
    restore_source_proof: Option<String>,
    authorities: Vec<ArtifactAuthority>,
    workspace: PathBuf,
    state: String,
    files: Vec<Artifact>,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct TaggingStorage {
    pub original_files: u64,
    pub original_bytes: u64,
    pub pending_cleanup: u64,
    pub retained_cleanup_files: u64,
    pub retained_cleanup_bytes: u64,
    pub retained_cleanup_unknown_sizes: u64,
}
impl Library {
    pub fn tagging_storage(&self) -> Result<TaggingStorage> {
        let (paths, pending) = {
            let conn = self.conn()?;
            let mut query = conn
                .prepare("SELECT backup FROM tag_write_files WHERE backup IS NOT NULL UNION SELECT backup FROM cleanup_write_files WHERE backup IS NOT NULL UNION SELECT backup FROM metadata_edit_files WHERE backup IS NOT NULL")?;
            let paths = query
                .query_map([], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let pending = conn.query_row(
                "SELECT COUNT(*) FROM (SELECT 'tag_write' AS owner_kind,id AS owner_id FROM tag_writes WHERE state IN ('committed','rolled_back') AND cleanup_complete=0 UNION SELECT 'cleanup_write',id FROM cleanup_writes WHERE state IN ('committed','rolled_back','undone') AND cleanup_complete=0 UNION SELECT 'metadata_edit',id FROM metadata_edit_writes WHERE state IN ('committed','undone') AND error IS NOT NULL UNION SELECT 'metadata_preview',id FROM metadata_edit_previews WHERE state='cleanup_required' UNION SELECT owner_kind,owner_id FROM artifact_cleanup_moves WHERE state!='removed')",
                [], |r| r.get(0),
            )?;
            (paths, pending)
        };
        let mut paths = paths.into_iter().collect::<std::collections::BTreeSet<_>>();
        let mut offset = 0;
        loop {
            let page = self.metadata_override_backup_paths_page(offset, 200)?;
            let count = page.len();
            paths.extend(page);
            if count < 200 {
                break;
            }
            offset += count;
        }
        let cleanup = self.artifact_cleanup_storage()?;
        let mut storage = TaggingStorage {
            pending_cleanup: pending,
            retained_cleanup_files: cleanup.retained_files,
            retained_cleanup_bytes: cleanup.retained_bytes,
            retained_cleanup_unknown_sizes: cleanup.unknown_sizes,
            ..Default::default()
        };
        for path in paths {
            match std::fs::symlink_metadata(bytes_path(&path)?) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                    storage.original_files += 1;
                    storage.original_bytes = storage.original_bytes.saturating_add(meta.len());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(storage)
    }
    pub fn begin_tag_write(
        &self,
        run: &JobRun,
        staged: &mut StagedTagWrite,
        result: &FileAnalysis,
    ) -> Result<String> {
        Self::validate_analysis(result)?;
        let encoded = serde_json::to_string(result)?;
        let _: FileAnalysis =
            serde_json::from_str(&encoded).context("Analysis contains invalid numeric values")?;
        let media = result
            .media
            .as_ref()
            .context("Tag publication requires PCM provenance")?;
        ensure!(
            media.source_sha256 == staged.source_before_sha256
                && media.pcm_sha256 == staged.pcm_sha256
                && media.decoder == staged.decoder_identity
                && media.clock.frames == staged.frames
                && media.clock.sample_rate == 44100,
            "Analysis and tag snapshot differ"
        );
        ensure!(
            result.analysis.duration_seconds == media.clock.duration_seconds(),
            "Tag analysis clock differs"
        );
        ensure!(
            staged.source_path == run.path && staged.source_before_sha256 == run.source_sha256,
            "Tag source identity differs from job"
        );
        let workspace = staged.workspace().canonicalize()?;
        ensure!(
            workspace.starts_with(self.data_dir().canonicalize()?),
            "Tag workspace must be inside application data"
        );
        owned_file(&staged.path, &workspace, &staged.source_after_sha256)?;
        owned_file(
            &staged.backup_path,
            &workspace,
            &staged.source_before_sha256,
        )?;
        self.verify_protected_match_review_source(run)?;
        self.verify_metadata_override_source_for_run(run)?;
        let input_source = Self::preparation_source_checkpoint(&run.path, &run.path)?;
        ensure!(
            input_source.parts().2.sha256 == run.source_sha256,
            "Source changed before tag publication"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let mut files = vec![Artifact {
            target: run.path.clone(),
            staged: staged.path.clone(),
            backup: Some(staged.backup_path.clone()),
            before: Some(staged.source_before_sha256.clone()),
            after: staged.source_after_sha256.clone(),
            publish: PathBuf::new(),
            displaced: PathBuf::new(),
        }];
        for sidecar in &staged.sidecars {
            owned_file(&sidecar.path, &workspace, &sidecar.after_sha256)?;
            if let (Some(path), Some(hash)) = (&sidecar.backup_path, &sidecar.before_sha256) {
                owned_file(path, &workspace, hash)?;
            }
            files.push(Artifact {
                target: sidecar.target_path.clone(),
                staged: sidecar.path.clone(),
                backup: sidecar.backup_path.clone(),
                before: sidecar.before_sha256.clone(),
                after: sidecar.after_sha256.clone(),
                publish: PathBuf::new(),
                displaced: PathBuf::new(),
            });
        }
        let mut seen = std::collections::HashSet::new();
        for (n, file) in files.iter_mut().enumerate() {
            safe_target(&file.target, &run.path)?;
            ensure!(
                seen.insert(file.target.clone()),
                "Duplicate tag destinations"
            );
            ensure!(
                current_hash(&file.target)? == file.before,
                "Tag destination changed during staging"
            );
            let parent = file.target.parent().unwrap();
            file.publish = parent.join(format!(".cuerithm-{id}-{n}.tmp"));
            file.displaced = if cfg!(windows) {
                parent.join(format!(".cuerithm-{id}-{n}-before.tmp"))
            } else {
                file.publish.clone()
            };
        }
        let authorities = files
            .iter()
            .map(|file| {
                let (parent, authority) = crate::organization::NativeParent::capture(
                    file.target.parent().context("Missing publication parent")?,
                )?;
                let name = std::path::Path::new(
                    file.target
                        .file_name()
                        .context("Missing publication name")?,
                );
                let before = parent.optional(name)?;
                ensure!(
                    before.as_ref().map(|p| &p.sha256) == file.before.as_ref(),
                    "Publication source changed before journal capture"
                );
                Ok(ArtifactAuthority {
                    path: crate::organization::NativePath::new(&file.target),
                    parent: authority,
                    before,
                    published: None,
                    private_staged: Some(native_artifacts::captured(&file.staged)?),
                    private_backup: file
                        .backup
                        .as_ref()
                        .map(|path| native_artifacts::captured(path))
                        .transpose()?,
                    retired: vec![],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let assets: Vec<_> = files
            .iter()
            .flat_map(|file| {
                std::iter::once(file.staged.clone()).chain(file.backup.iter().cloned())
            })
            .collect();
        publish_files::durable_assets(&assets, &workspace, &self.data_dir().canonicalize()?)?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        ensure!(Self::run_owns(&tx, run)?, "Preparation was superseded");
        crate::cleanup::store::validate_report(
            &tx,
            &run.track_id,
            Self::preparation_cleanup_config(&run.snapshot)?,
            staged.cleanup.as_ref(),
        )?;
        for file in &files {
            crate::maintenance::ensure_unused_unreserved(&tx, &[path_bytes(&file.target)])?;
            let busy:i64=tx.query_row("SELECT (SELECT COUNT(*) FROM tag_write_files f JOIN tag_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared') + (SELECT COUNT(*) FROM cleanup_write_files f JOIN cleanup_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state='prepared') + (SELECT COUNT(*) FROM maintenance_write_files f JOIN maintenance_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','conflict')) + (SELECT COUNT(*) FROM metadata_edit_files f JOIN metadata_edit_writes w ON w.id=f.write_id WHERE f.target=?1 AND w.state IN ('prepared','publishing','recovery_required','conflict'))",[path_bytes(&file.target)],|r|r.get(0))?;
            ensure!(
                busy == 0,
                "Another preparation owns this destination; retry tagging"
            );
        }
        tx.execute(
            "UPDATE jobs SET stage='writing_tags',updated=?1 WHERE id=?2",
            params![now(), run.id],
        )?;
        tx.execute("INSERT INTO tag_writes(id,job_id,track_id,attempt,settings_revision,before_hash,after_hash,pcm_hash,frames,decoder,analysis_json,metadata_json,provenance_json,workspace,created,cleanup_json,native_metadata_json,editor_receipt_json,input_source_proof_json,artifact_authorities_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",params![id,run.id,run.track_id,run.attempt,run.settings_revision,staged.source_before_sha256,staged.source_after_sha256,staged.pcm_sha256,staged.frames,staged.decoder_identity,serde_json::to_string(result)?,serde_json::to_string(&staged.metadata)?,serde_json::to_string(&staged.provenance)?,path_bytes(&workspace),now(),staged.cleanup.as_ref().map(serde_json::to_string).transpose()?,serde_json::to_string(&staged.native_metadata)?,staged.editor_receipt_json,serde_json::to_string(&input_source)?,serde_json::to_string(&authorities)?])?;
        for (n, f) in files.iter().enumerate() {
            tx.execute(
                "INSERT INTO tag_write_files VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
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
        if run.snapshot.settings.organization.enabled() {
            Self::insert_preparation_checkpoint_tx(
                &tx,
                &run.id,
                &run.track_id,
                &run.source_sha256,
                &staged.source_after_sha256,
                run.attempt,
                Some(&id),
                "tagging",
                None,
            )?;
        }
        tx.commit()?;
        staged.retain_for_journal();
        Ok(id)
    }
    pub(crate) fn run_owns(conn: &Connection, run: &JobRun) -> Result<bool> {
        Ok(conn.query_row("SELECT j.state='running' AND j.attempts=?2 AND j.settings_revision=?3 AND j.source_hash=?4 AND t.current_job=j.id AND t.archived=0 AND t.id=?5 AND t.path=?6 FROM jobs j JOIN tracks t ON t.id=j.track_id WHERE j.id=?1",params![run.id,run.attempt,run.settings_revision,run.source_sha256,run.track_id,path_bytes(&run.path)],|r|r.get(0))?)
    }
    fn load_tag_write(&self, id: &str) -> Result<Journal> {
        let conn = self.conn()?;
        let mut journal=conn.query_row("SELECT id,job_id,track_id,attempt,settings_revision,before_hash,after_hash,analysis_json,metadata_json,provenance_json,workspace,state,cleanup_json,native_metadata_json,editor_receipt_json,input_source_proof_json,restore_source_proof_json,artifact_authorities_json FROM tag_writes WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,u32>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?,r.get::<_,Vec<u8>>(10)?,r.get::<_,String>(11)?,r.get::<_,Option<String>>(12)?,r.get::<_,Option<String>>(13)?,r.get::<_,Option<String>>(14)?,r.get::<_,Option<String>>(15)?,r.get::<_,Option<String>>(16)?,r.get::<_,Option<String>>(17)?)))?;
        let mut stmt=conn.prepare("SELECT target,staged,backup,before_hash,after_hash,publish_path,displaced_path FROM tag_write_files WHERE write_id=?1 ORDER BY ordinal")?;
        let encoded = stmt
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
        let files = encoded
            .into_iter()
            .map(
                |(target, staged, backup, before, after, publish, displaced)| {
                    Ok(Artifact {
                        target: bytes_path(&target)?,
                        staged: bytes_path(&staged)?,
                        backup: backup.map(|p| bytes_path(&p)).transpose()?,
                        before,
                        after,
                        publish: bytes_path(&publish)?,
                        displaced: bytes_path(&displaced)?,
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;
        Ok(Journal {
            authorities: journal
                .17
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or_default(),
            id: std::mem::take(&mut journal.0),
            job: journal.1,
            track: journal.2,
            attempt: journal.3,
            revision: journal.4,
            before: journal.5,
            after: journal.6,
            analysis: serde_json::from_str(&journal.7)?,
            metadata: serde_json::from_str(&journal.8)?,
            provenance: serde_json::from_str(&journal.9)?,
            cleanup: journal.12.map(|s| serde_json::from_str(&s)).transpose()?,
            native_metadata: journal.13.map(|s| serde_json::from_str(&s)).transpose()?,
            editor_receipt: journal.14,
            input_source_proof: journal.15,
            restore_source_proof: journal.16,
            workspace: bytes_path(&journal.10)?,
            state: journal.11,
            files,
        })
    }
    pub fn publish_preparation(&self, run: &JobRun, id: &str) -> Result<()> {
        self.publish_preparation_with(run, id, |_| Ok(()))
    }
    pub(crate) fn publish_preparation_with(
        &self,
        run: &JobRun,
        id: &str,
        mut boundary: impl FnMut(TagPublicationBoundary) -> Result<()>,
    ) -> Result<()> {
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut journal = self.load_tag_write(id)?;
        self.verify_protected_match_review_source(run)?;
        self.verify_metadata_override_source_for_run(run)?;
        ensure!(
            journal.id == id
                && journal.before == run.source_sha256
                && journal.state == "prepared"
                && journal.job == run.id
                && journal.attempt == run.attempt
                && journal.revision == run.settings_revision,
            "Publication was superseded"
        );
        let parents = journal
            .authorities
            .iter()
            .map(|a| crate::organization::NativeParent::reopen(&a.parent))
            .collect::<Result<Vec<_>>>()?;
        let preparation = self
            .prepare_native_tag_artifacts(&mut journal, &parents, run)
            .and_then(|_| boundary(TagPublicationBoundary::CopiesPrepared));
        let retain_checkpoint = self.has_preparation_checkpoint(&journal.job)?;
        let needs_native_proof=retain_checkpoint||self.has_protected_match_review(&run.id)?||self.conn()?.query_row("SELECT EXISTS(SELECT 1 FROM metadata_edit_overrides WHERE job_id=?1 AND state='claimed')",[&run.id],|r|r.get::<_,bool>(0))?;
        let proof = if needs_native_proof && preparation.is_ok() {
            let main = journal.files.first().context("Missing publication audio")?;
            if let Some(parent) = parents.first() {
                let target = if main.before.as_deref() == Some(main.after.as_str()) {
                    &main.target
                } else {
                    &main.publish
                };
                let proof = ArtifactRestoreProof {
                    from_backup: false,
                    displaced: None,
                    path: crate::organization::NativePath::new(&run.path),
                    parent: journal.authorities[0].parent.clone(),
                    fingerprint: parent.fingerprint(std::path::Path::new(
                        target.file_name().context("Missing native output name")?,
                    ))?,
                };
                Some(serde_json::from_value(serde_json::to_value(proof)?)?)
            } else {
                Some(Self::preparation_source_checkpoint(
                    &run.path,
                    if main.before.as_deref() == Some(main.after.as_str()) {
                        &main.target
                    } else {
                        &main.publish
                    },
                )?)
            }
        } else {
            None
        };
        let mut conn = self.conn()?;
        let result = preparation.and_then(|_| {
            let tx = conn.transaction()?;
            ensure!(Self::run_owns(&tx, run)?, "Preparation was superseded");
            Self::validate_analysis(&journal.analysis)?;
            let settings: String = tx.query_row(
                "SELECT json FROM settings WHERE revision=?1",
                [&run.settings_revision],
                |r| r.get(0),
            )?;
            let snapshot: SettingsSnapshot = serde_json::from_str(&settings)?;
            crate::cleanup::store::validate_report(
                &tx,
                &journal.track,
                Self::preparation_cleanup_config(&snapshot)?,
                journal.cleanup.as_ref(),
            )?;
            for f in &journal.files {
                safe_target(&f.target, &run.path)?;
                ensure!(
                    current_hash(&f.target)? == f.before,
                    "Source or artwork changed before publication"
                );
                if f.before != Some(f.after.clone()) {
                    ensure!(
                        file_sha256(&f.publish)? == f.after,
                        "Publication copy changed"
                    );
                }
            }
            for (ordinal,f) in journal.files.iter().enumerate() {
                if f.before.as_deref() == Some(f.after.as_str()) {
                    continue;
                }
                if let Some(authority)=journal.authorities.get(ordinal){let expected=authority.published.as_ref().context("Missing owned publication copy receipt")?;let observed=parents[ordinal].fingerprint(std::path::Path::new(f.publish.file_name().context("Missing publication leaf")?))?;ensure!(crate::organization::same_native_file(&observed,expected),"Publication temporary native identity changed; retained");}
                boundary(TagPublicationBoundary::BeforeExchange)?;
                if let Some(before) = &f.before {
                    ensure!(
                        !std::fs::metadata(&f.target)?.permissions().readonly(),
                        "Publication target is read only"
                    );
                    if let Some(parent)=parents.get(ordinal) {
                        let names=(&f.publish.file_name().context("Missing publication name")?,&f.target.file_name().context("Missing target name")?,&f.displaced.file_name().context("Missing displaced name")?);
                        let expected=journal.authorities[ordinal].before.as_ref().context("Missing original native identity")?;
                        let observed=parent.fingerprint(std::path::Path::new(names.1))?;
                        ensure!(observed.identity==expected.identity && observed.sha256==*before,"Native publication source changed");
                        parent.exchange(std::path::Path::new(names.0),std::path::Path::new(names.1),std::path::Path::new(names.2))?;
                        let captured=parent.fingerprint(std::path::Path::new(names.2))?;
                        ensure!(captured.identity==expected.identity && captured.sha256==*before,"Source changed during native exchange; displaced bytes retained");
                    } else {ensure!(journal.input_source_proof.is_none(),"Protected publication lacks native authority");publish_files::replace(&f.publish,&f.target,&f.displaced)?;ensure!(file_sha256(&f.displaced)?==*before,"Source changed during atomic replacement");}
                } else if let Some(parent)=parents.get(ordinal) {
                    parent.rename(std::path::Path::new(f.publish.file_name().unwrap()),parent,std::path::Path::new(f.target.file_name().unwrap()))?;
                } else { publish_files::create(&f.publish,&f.target)?; }
                ensure!(
                    file_sha256(&f.target)? == f.after,
                    "Published file changed before commit"
                );
            }
            boundary(TagPublicationBoundary::Published)?;
            for (ordinal,authority) in journal.authorities.iter().enumerate(){
                let live=crate::organization::NativeParent::reopen(&authority.parent)?;
                let name=std::path::Path::new(journal.files[ordinal].target.file_name().context("Missing target name")?);
                let observed=live.fingerprint(name)?;let pinned=parents[ordinal].fingerprint(name)?;
                let expected=if journal.files[ordinal].before.as_deref()==Some(journal.files[ordinal].after.as_str()){authority.before.as_ref()}else{authority.published.as_ref()}.context("Missing saved native output identity")?;
                ensure!(crate::organization::same_native_file(&observed,expected)&&crate::organization::same_native_file(&pinned,expected),"Published namespace or native object changed before acknowledgment; all versions retained");
            }
            Self::store_analysis_tx(
                &tx,
                &journal.job,
                &journal.track,
                &journal.analysis,
                &journal.metadata,
            )?;
            tx.execute(
                "UPDATE tracks SET source_hash=?1 WHERE id=?2",
                params![journal.after, journal.track],
            )?;
            if snapshot.settings.autotag_enabled {
                tx.execute(
                    "UPDATE tracks SET tagging_json=?1 WHERE id=?2",
                    params![serde_json::to_string(&journal.provenance)?, journal.track],
                )?;
            }
            if let Some(native) = &journal.native_metadata {
                crate::cleanup::store::store_native(&tx, &journal.track, native)?;
            }
            if let Some(report) = &journal.cleanup {
                crate::cleanup::store::apply_report(&tx, &journal.track, report)?;
            }
            tx.execute(
                "UPDATE tag_writes SET state='committed',error=NULL WHERE id=?1",
                [id],
            )?;
            tx.execute("DELETE FROM organization_owned_sidecars WHERE track_id=?1",[&journal.track])?;
            tx.execute("INSERT INTO organization_owned_sidecars(track_id,path,source_hash,manifest_id,ordinal) SELECT ?1,target,after_hash,write_id,ordinal FROM tag_write_files WHERE write_id=?2 AND ordinal>0",params![journal.track,id])?;
            Self::ack_metadata_override_tx(&tx,run,&journal.before,journal.editor_receipt.as_deref())?;
            Self::ack_protected_match_review_tx(&tx,run,id,&journal.after,proof.as_ref().map(|p|p.parts()))?;
            if !snapshot.settings.organization.enabled(){Self::finish_protected_match_review_tx(&tx,run)?;}
            if !snapshot.settings.organization.enabled() && let Some(proof)=&proof {
                let(path,parent,fingerprint)=proof.parts();
                Self::finalize_metadata_override_tx(&tx,run,path,parent,fingerprint)?;
            }
            if snapshot.settings.organization.enabled() {
                Self::prepare_checkpoint_tx(&tx,&journal.job,proof.context("Missing prepared output native proof")?)?;
            }
            tx.commit()?;
            Ok(())
        });
        if let Err(error) = result {
            drop(conn);
            let rollback = self.rollback_tag_journal(&journal);
            let conn = self.conn()?;
            let message = match &rollback {
                Ok(_) => format!("{error:#}"),
                Err(e) => format!("{error:#}; recovery needs attention: {e:#}"),
            };
            conn.execute(
                "UPDATE tag_writes SET state=?1,error=?2 WHERE id=?3",
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
            drop(conn);
            if rollback.is_ok()
                && !retain_checkpoint
                && self.cleanup_native_tag_journal(&journal).is_ok()
            {
                self.conn()?
                    .execute("UPDATE tag_writes SET cleanup_complete=1 WHERE id=?1", [id])?;
            }
            return Err(error);
        }
        drop(conn);
        if !retain_checkpoint && self.cleanup_native_tag_journal(&journal).is_ok() {
            self.conn()?
                .execute("UPDATE tag_writes SET cleanup_complete=1 WHERE id=?1", [id])?;
        }
        // An Incoming save that was just written no longer needs the original audio it kept,
        // and may push an older one out of the window of ten saves that can be undone.
        let _ = self.prune_metadata_undo();
        let _ = self.retire_incoming_originals();
        Ok(())
    }
    pub(crate) fn replay_checkpoint_tag_write(&self, run: &JobRun, id: &str) -> Result<()> {
        self.replay_checkpoint_tag_write_with(run, id, |_| Ok(()))
    }
    pub(crate) fn replay_checkpoint_tag_write_with(
        &self,
        run: &JobRun,
        id: &str,
        after_intent: impl FnOnce(&str) -> Result<()>,
    ) -> Result<()> {
        let journal = self.load_tag_write(id)?;
        ensure!(
            journal.job == run.id
                && journal.before == run.source_sha256
                && journal.revision == run.settings_revision,
            "Tag checkpoint identity differs"
        );
        ensure!(
            matches!(journal.state.as_str(), "rolled_back" | "prepared"),
            "Tag checkpoint needs manual recovery before retry"
        );
        for file in &journal.files {
            safe_target(&file.target, &run.path)?;
            owned_file(&file.staged, &journal.workspace, &file.after)?;
            if let (Some(backup), Some(before)) = (&file.backup, &file.before) {
                owned_file(backup, &journal.workspace, before)?;
            }
            ensure!(
                current_hash(&file.target)? == file.before,
                "Source or artwork changed after tag checkpoint"
            );
        }
        let mut replay_proof = None;
        if journal.input_source_proof.is_some() {
            let mut known = Vec::new();
            for encoded in [&journal.input_source_proof, &journal.restore_source_proof]
                .into_iter()
                .flatten()
            {
                known.push(serde_json::from_str::<ArtifactRestoreProof>(encoded)?);
            }
            for proof in known {
                if proof.path != crate::organization::NativePath::new(&run.path)
                    || proof.fingerprint.sha256 != run.source_sha256
                {
                    continue;
                }
                let parent = crate::organization::NativeParent::reopen(&proof.parent)?;
                let actual = parent.fingerprint(std::path::Path::new(
                    run.path.file_name().context("Missing replay source name")?,
                ))?;
                if actual.identity == proof.fingerprint.identity
                    && actual.sha256 == proof.fingerprint.sha256
                    && actual.bytes == proof.fingerprint.bytes
                {
                    replay_proof = Some(proof);
                    break;
                }
            }
            ensure!(
                replay_proof.is_some(),
                "Tag replay source is not the exact original/restore object retained by its journal"
            );
        } else {
            ensure!(
                !self.has_protected_match_review(&run.id)?,
                "Legacy hash-only tag journals cannot rebind a protected review decision"
            );
        }
        // Preserve the previous attempt's journal and private publication names.
        // Its rollback may have left known before bytes there. Reuse only the
        // durable, hash-verified staged outputs and original backup assets.
        let next = uuid::Uuid::new_v4().to_string();
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            ensure!(Self::run_owns(&tx, run)?, "Preparation was superseded");
            if let Some(proof) = &replay_proof {
                Self::rebind_protected_match_replay_tx(&tx, run, id, proof)?;
                Self::rebind_metadata_override_replay_tx(
                    &tx,
                    run,
                    id,
                    &proof.path,
                    &proof.parent,
                    &proof.fingerprint,
                )?;
            }
            ensure!(tx.execute("UPDATE tag_writes SET state='rolled_back'  WHERE id=?1 AND state IN ('rolled_back','prepared')",[id])?==1,"Tag checkpoint changed");
            tx.execute("INSERT INTO tag_writes(id,job_id,track_id,attempt,settings_revision,before_hash,after_hash,pcm_hash,frames,decoder,analysis_json,metadata_json,provenance_json,workspace,created,cleanup_json,native_metadata_json,editor_receipt_json,input_source_proof_json,artifact_authorities_json) SELECT ?1,job_id,track_id,?2,settings_revision,before_hash,after_hash,pcm_hash,frames,decoder,analysis_json,metadata_json,provenance_json,workspace,?3,cleanup_json,native_metadata_json,editor_receipt_json,input_source_proof_json,artifact_authorities_json FROM tag_writes WHERE id=?4",params![next,run.attempt,now(),id])?;
            if let Some(proof) = &replay_proof {
                tx.execute(
                    "UPDATE tag_writes SET input_source_proof_json=?1 WHERE id=?2",
                    params![serde_json::to_string(proof)?, next],
                )?;
            }
            for (ordinal, file) in journal.files.iter().enumerate() {
                let parent = file
                    .target
                    .parent()
                    .context("Missing tag publication parent")?;
                let publish = parent.join(format!(".cuerithm-{next}-{ordinal}.tmp"));
                let displaced = if cfg!(windows) {
                    parent.join(format!(".cuerithm-{next}-{ordinal}-before.tmp"))
                } else {
                    publish.clone()
                };
                tx.execute(
                    "INSERT INTO tag_write_files VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        next,
                        ordinal,
                        path_bytes(&file.target),
                        path_bytes(&file.staged),
                        file.backup.as_ref().map(|p| path_bytes(p)),
                        file.before,
                        file.after,
                        path_bytes(&publish),
                        path_bytes(&displaced)
                    ],
                )?;
            }
            ensure!(tx.execute("UPDATE preparation_checkpoints SET payload_json=json_set(payload_json,'$.tag_write',?1),updated=?2 WHERE job_id=?3 AND state='tagging' AND attempt=?4 AND json_extract(payload_json,'$.tag_write')=?5",params![next,now(),run.id,run.attempt,id])?==1,"Tag checkpoint journal changed");
            tx.commit()?;
        }
        after_intent(&next)?;
        // Claim the accepted override for this fresh owned attempt. Staged bytes
        // and its immutable editor receipt are reused rather than recomputed.
        let _ = self.metadata_override_for_run(run)?;
        self.publish_preparation(run, &next)
    }
    pub(crate) fn cleanup_checkpoint_tags(&self, job: &str) -> Result<()> {
        let ids: Vec<String> = {
            let conn = self.conn()?;
            let mut q=conn.prepare("SELECT id FROM tag_writes WHERE job_id=?1 AND state IN ('committed','rolled_back') AND cleanup_complete=0")?;
            q.query_map([job], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in ids {
            let journal = self.load_tag_write(&id)?;
            self.cleanup_native_tag_journal(&journal)?;
            self.conn()?
                .execute("UPDATE tag_writes SET cleanup_complete=1 WHERE id=?1", [id])?;
        }
        Ok(())
    }
    fn rollback_tag_journal(&self, journal: &Journal) -> Result<()> {
        let authorities = journal
            .authorities
            .iter()
            .map(|authority| crate::artifacts::RollbackAuthority {
                parent: authority.parent.clone(),
                before: authority.before.clone(),
                after: authority.published.clone(),
                backup: authority.private_backup.clone(),
            })
            .collect::<Vec<_>>();
        let outcome = rollback_with_authority(&journal.files, &authorities, |file, proof| {
            if let Some(expected) = journal.authorities.iter().find(|a| a.path == proof.path) {
                ensure!(
                    proof.parent.path == expected.parent.path
                        && proof.parent.ancestors == expected.parent.ancestors,
                    "Rollback parent changed; original and displaced versions retained"
                );
            }
            if file.before.is_none() {
                let mut authorities: Vec<ArtifactAuthority> = self
                    .conn()?
                    .query_row(
                        "SELECT artifact_authorities_json FROM tag_writes WHERE id=?1",
                        [&journal.id],
                        |r| r.get::<_, String>(0),
                    )
                    .map(|json| serde_json::from_str(&json))??;
                let authority = authorities
                    .iter_mut()
                    .find(|a| a.path == proof.path)
                    .context("Missing absent-original retirement authority")?;
                ensure!(
                    authority.published.as_ref().is_some_and(|published| {
                        crate::organization::same_native_file(published, &proof.fingerprint)
                    }),
                    "Sidecar output changed before retirement; retained"
                );
                let retired = ArtifactRestoreProof {
                    path: proof
                        .displaced
                        .clone()
                        .context("Missing sidecar retirement leaf")?,
                    parent: proof.parent.clone(),
                    fingerprint: proof.fingerprint.clone(),
                    from_backup: false,
                    displaced: None,
                };
                if !authority
                    .retired
                    .iter()
                    .any(|saved| saved.path == retired.path)
                {
                    authority.retired.push(retired);
                }
                self.conn()?.execute(
                    "UPDATE tag_writes SET artifact_authorities_json=?1 WHERE id=?2",
                    params![serde_json::to_string(&authorities)?, journal.id],
                )?;
                // Never turn absent-original retirement into source-replay proof.
                return Ok(());
            }
            let owned = crate::artifacts::restore_is_owned(
                proof,
                journal
                    .authorities
                    .iter()
                    .find(|authority| authority.path == proof.path)
                    .and_then(|authority| authority.before.as_ref()),
            );
            if !owned {
                // Restore the displaced user's object, but never turn an unknown
                // equal-byte replacement into authority for a protected retry.
                if journal
                    .files
                    .first()
                    .is_some_and(|main| main.target == file.target)
                {
                    self.conn()?.execute(
                        "UPDATE tag_writes SET restore_source_proof_json=NULL WHERE id=?1",
                        [&journal.id],
                    )?;
                }
                return Ok(());
            }
            let mut authorities: Vec<ArtifactAuthority> = self
                .conn()?
                .query_row(
                    "SELECT artifact_authorities_json FROM tag_writes WHERE id=?1",
                    [&journal.id],
                    |r| r.get::<_, Option<String>>(0),
                )?
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or_default();
            if let Some(authority) = authorities.iter_mut().find(|a| a.path == proof.path) {
                authority.parent = proof.parent.clone();
                authority.before = Some(proof.fingerprint.clone());
                if let (Some(path), Some(published)) = (&proof.displaced, &authority.published) {
                    let retired = ArtifactRestoreProof {
                        path: path.clone(),
                        parent: proof.parent.clone(),
                        fingerprint: published.clone(),
                        from_backup: false,
                        displaced: None,
                    };
                    if !authority.retired.iter().any(|old| old.path == retired.path) {
                        authority.retired.push(retired);
                    }
                }
                self.conn()?.execute(
                    "UPDATE tag_writes SET artifact_authorities_json=?1 WHERE id=?2",
                    params![serde_json::to_string(&authorities)?, journal.id],
                )?;
            }
            if journal
                .files
                .first()
                .is_some_and(|main| main.target == file.target)
            {
                self.conn()?.execute("UPDATE tag_writes SET restore_source_proof_json=?1 WHERE id=?2 AND state IN ('prepared','rolled_back','conflict')",params![serde_json::to_string(proof)?,journal.id])?;
            }
            Ok(())
        });
        if outcome.is_ok() {
            self.rebind_restored_tag_execution(journal)?;
        }
        outcome
    }
    pub fn recover_tag_writes(&self) -> Result<()> {
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let ids: Vec<String> = {
            let conn = self.conn()?;
            let mut stmt = conn
                .prepare("SELECT id FROM tag_writes WHERE state='prepared' ORDER BY created,id")?;
            stmt.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in ids {
            let journal = self.load_tag_write(&id)?;
            let outcome = self.rollback_tag_journal(&journal);
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let owned:bool=tx.query_row("SELECT t.current_job=?2 AND t.archived=0 AND j.attempts=?3 FROM tracks t JOIN jobs j ON j.id=?2 WHERE t.id=?1",params![journal.track,journal.job,journal.attempt],|r|r.get(0))?;
            if owned {
                tx.execute(
                    "UPDATE jobs SET state=?1,stage=?2,error=?3 WHERE id=?4",
                    params![
                        if outcome.is_ok() {
                            "queued"
                        } else {
                            "attention"
                        },
                        if outcome.is_ok() {
                            "queued"
                        } else {
                            "tag_recovery"
                        },
                        outcome.as_ref().err().map(|e| format!("{e:#}")),
                        journal.job
                    ],
                )?;
            }
            tx.execute(
                "UPDATE tag_writes SET state=?1,error=?2 WHERE id=?3",
                params![
                    if outcome.is_ok() {
                        "rolled_back"
                    } else {
                        "conflict"
                    },
                    outcome.as_ref().err().map(|e| format!("{e:#}")),
                    id
                ],
            )?;
            tx.commit()?;
        }
        let committed: Vec<String> = {
            let conn = self.conn()?;
            let mut stmt = conn.prepare("SELECT id FROM tag_writes WHERE state IN ('committed','rolled_back') AND cleanup_complete=0")?;
            stmt.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in committed {
            if let Ok(journal) = self.load_tag_write(&id)
                && !self.has_preparation_checkpoint(&journal.job)?
                && self.cleanup_native_tag_journal(&journal).is_ok()
            {
                self.conn()?
                    .execute("UPDATE tag_writes SET cleanup_complete=1 WHERE id=?1", [id])?;
            }
        }
        Ok(())
    }
}
