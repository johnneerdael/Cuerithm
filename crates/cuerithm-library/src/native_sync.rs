//! Durable native observations and initial import. Observation never acknowledges
//! synchronization, changes a target database, or overwrites existing local work.
use crate::*;
use anyhow::{Context, Result, ensure};
use cuerithm_media::{CancellationToken, file_sha256_cancellable};
use cuerithm_rekordbox::{
    DatabaseIdentity, ImportedTrack, RekordboxCollection, TargetRecord, TargetValue, TrackIdentity,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};

type Fields = BTreeMap<String, NativeFieldBaseline>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFieldOwner {
    Local,
    Remote,
    Shared,
    Unowned,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeFieldBaseline {
    pub local: Value,
    pub remote: Value,
    pub owner: NativeFieldOwner,
    /// False for initial observations, including initially equal values. Only a
    /// separately verified synchronization can establish an agreed baseline.
    pub acknowledged: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeImportOptions {
    pub settings_revision: String,
    pub content_ids: Option<BTreeSet<String>>,
    /// Applied only when a link first acquires a local track. Policy changes and
    /// reconciliation of existing links require their own explicit operation.
    pub initial_owners: BTreeMap<String, NativeFieldOwner>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeImportReport {
    pub target_id: String,
    pub revision: u64,
    pub changed: bool,
    pub tracks_created: usize,
    pub jobs_created: usize,
    pub statuses: BTreeMap<String, usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTargetState {
    pub id: String,
    pub identity: DatabaseIdentity,
    pub schema_fingerprint: String,
    pub revision: u64,
    pub records: usize,
    pub tracks: usize,
    pub issues: Vec<cuerithm_rekordbox::ImportIssue>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTrackLink {
    pub content_id: String,
    pub local_track_id: Option<String>,
    pub baseline_identity: TrackIdentity,
    pub status: String,
    pub observation: Option<ImportedTrack>,
    pub fields: Fields,
}
const TABLES: &[&str] = &[
    "djmdProperty",
    "agentRegistry",
    "djmdContent",
    "djmdArtist",
    "djmdAlbum",
    "djmdGenre",
    "djmdLabel",
    "djmdKey",
    "djmdCue",
    "contentCue",
    "djmdPlaylist",
    "djmdSongPlaylist",
    "djmdMyTag",
    "djmdSongMyTag",
];
/// The record tables whose stored content later code reads back: the playlist and MyTag
/// rows and their membership rows, which native playlist sync compares with what it bound.
/// Every other table keeps only each record's identity and hash, enough to tell which records
/// changed between two observations. The rekordbox database itself is the source of the
/// rest, and every sync reads it again.
pub(crate) const RECORD_CONTENT_TABLES: [&str; 4] = [
    "djmdPlaylist",
    "djmdSongPlaylist",
    "djmdMyTag",
    "djmdSongMyTag",
];
/// The stored `json` of a record whose content is not kept.
const CONTENT_NOT_KEPT: &str = "";
const TEXT_FIELDS: &[&str] = &[
    "title",
    "artist",
    "album",
    "album_artist",
    "genre",
    "label",
    "comment",
    "remixer",
    "composer",
    "lyricist",
    "mix_name",
];
const OTHER_FIELDS: &[&str] = &[
    "key",
    "bpm",
    "duration_seconds",
    "release_year",
    "release_date",
    "track_number",
    "disc_number",
    "rating",
    "cues",
    "tag_ids",
    "source_path",
];
fn hash<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    hash_with_cancel(value, None)
}
fn hash_with_cancel<T: Serialize + ?Sized>(
    value: &T,
    cancel: Option<&CancellationToken>,
) -> Result<String> {
    struct Sink<'a> {
        hash: Sha256,
        cancel: Option<&'a CancellationToken>,
    }
    impl Write for Sink<'_> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.cancel.is_some_and(|c| c.is_cancelled()) {
                return Err(std::io::Error::other("Native import cancelled"));
            }
            self.hash.update(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink {
        hash: Sha256::new(),
        cancel,
    };
    serde_json::to_writer(&mut sink, value)?;
    Ok(format!("{:x}", sink.hash.finalize()))
}

fn target_id(input: &RekordboxCollection) -> Result<String> {
    hash(&json!([
        "rekordbox",
        input.identity.database_id,
        input.identity.device_id.as_deref().unwrap_or("")
    ]))
}
fn validate(
    input: &RekordboxCollection,
    opts: &NativeImportOptions,
    cancel: &CancellationToken,
) -> Result<()> {
    ensure!(
        !input.identity.database_id.trim().is_empty(),
        "Native database identity is missing"
    );
    ensure!(
        input.schema_fingerprint.len() == 64
            && input
                .schema_fingerprint
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "Native schema identity is invalid"
    );
    ensure!(
        input.records.len() == TABLES.len()
            && TABLES.iter().all(|t| input.records.contains_key(*t)),
        "Native collection is incomplete"
    );
    ensure!(
        opts.initial_owners
            .keys()
            .all(|k| TEXT_FIELDS.contains(&k.as_str()) || OTHER_FIELDS.contains(&k.as_str())),
        "Unsupported native ownership field"
    );
    let mut count = 0usize;
    for (table, records) in &input.records {
        let mut ids = BTreeSet::new();
        for record in records {
            cancel.check()?;
            count += 1;
            ensure!(count <= 2_000_000, "Native collection row budget exceeded");
            ensure!(
                &record.table == table && !record.id.is_empty() && ids.insert(&record.id),
                "Duplicate or invalid native record identity"
            );
            ensure!(
                record
                    .values
                    .values()
                    .all(|v| !matches!(v,TargetValue::Real(f) if !f.is_finite())),
                "Native record contains nonfinite numeric data"
            );
            ensure!(
                table != "agentRegistry" || record.id == "localUpdateCount",
                "Unrelated native registry records cannot be imported"
            );
        }
    }
    let mut ids = BTreeSet::new();
    for track in &input.tracks {
        cancel.check()?;
        ensure!(
            track.identity.database_id == input.identity.database_id
                && !track.identity.content_id.is_empty()
                && ids.insert(track.identity.content_id.as_str()),
            "Duplicate or mismatched native content identity"
        );
        ensure!(
            track.bpm.is_none_or(|v| v.is_finite() && v >= 0.)
                && track
                    .duration_seconds
                    .is_none_or(|v| v.is_finite() && v >= 0.),
            "Native track contains invalid numeric data"
        );
        ensure!(
            track
                .cues
                .iter()
                .all(|c| c.start_seconds.is_none_or(f64::is_finite)
                    && c.end_seconds.is_none_or(f64::is_finite)),
            "Native cues contain nonfinite timing"
        );
    }
    let raw_ids = input.records["djmdContent"]
        .iter()
        .map(|r| r.id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(ids == raw_ids, "Native content projection is incomplete");
    if let Some(selected) = &opts.content_ids {
        ensure!(
            selected.iter().all(|id| ids.contains(id.as_str())),
            "Native selection is stale"
        );
    }
    Ok(())
}
fn vector(value: &str) -> Value {
    if value.is_empty() {
        json!([])
    } else {
        json!([value])
    }
}
pub(crate) fn remote_fields(track: &ImportedTrack) -> Result<BTreeMap<String, Value>> {
    let mut values = serde_json::to_value(&track.metadata)?
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<BTreeMap<_, _>>();
    for key in TEXT_FIELDS {
        let value = values.get(*key).and_then(Value::as_str).unwrap_or("");
        values.insert((*key).into(), vector(value));
    }
    if track.metadata.key.is_empty() {
        values.insert("key".into(), Value::Null);
    }
    values.insert("bpm".into(), json!(track.bpm));
    values.insert("duration_seconds".into(), json!(track.duration_seconds));
    values.insert("cues".into(), serde_json::to_value(&track.cues)?);
    values.insert("tag_ids".into(), json!(track.tag_ids));
    values.insert("source_path".into(), json!(track.source.path));
    Ok(values)
}
type LocalValues = (
    Vec<u8>,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<f64>,
    Option<f64>,
    String,
    Option<String>,
    Option<String>,
);
pub(crate) fn local_fields(conn: &Connection, id: &str) -> Result<BTreeMap<String, Value>> {
    let (path,title,artist,album,genre,key,bpm,duration,info,native,latest): LocalValues=conn.query_row(
        "SELECT path,title,artist,album,genre,musical_key,bpm,duration,metadata_json,native_metadata_json,(SELECT json FROM cue_revisions WHERE track_id=tracks.id ORDER BY created DESC,rowid DESC LIMIT 1) FROM tracks WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?)))?;
    let mut result = TEXT_FIELDS
        .iter()
        .map(|k| ((*k).into(), json!([])))
        .chain(OTHER_FIELDS.iter().map(|k| ((*k).into(), Value::Null)))
        .collect::<BTreeMap<String, Value>>();
    if let Ok(info) = serde_json::from_str::<cuerithm_media::MediaInfo>(&info) {
        for field in TEXT_FIELDS {
            let alias = if *field == "mix_name" { "mix" } else { field };
            if let Some(value) = info.tags.get(alias) {
                result.insert((*field).into(), vector(value));
            }
        }
        for field in [
            "release_year",
            "release_date",
            "track_number",
            "disc_number",
            "rating",
        ] {
            if let Some(value) = info.tags.get(field) {
                result.insert(field.into(), json!(value));
            }
        }
    }
    if let Some(native) =
        native.and_then(|s| serde_json::from_str::<cuerithm_cleanup::Metadata>(&s).ok())
    {
        for (key, values) in native.fields {
            let key = serde_json::to_value(key)?.as_str().unwrap().to_owned();
            let key = if key == "mix" {
                "mix_name".to_owned()
            } else {
                key
            };
            if TEXT_FIELDS.contains(&key.as_str()) {
                result.insert(key, json!(values));
            }
        }
    }
    for (name, value) in [
        ("title", title),
        ("artist", artist),
        ("album", album),
        ("genre", genre),
    ] {
        result.insert(name.into(), vector(&value));
    }
    result.insert("key".into(), json!(key));
    result.insert("bpm".into(), json!(bpm));
    result.insert("duration_seconds".into(), json!(duration));
    result.insert(
        "source_path".into(),
        json!(crate::model::bytes_path(&path)?.to_string_lossy()),
    );
    if let Some(latest) = latest {
        let json: Value = serde_json::from_str(&latest)?;
        result.insert("cues".into(), json.get("cues").cloned().unwrap_or(json));
    }
    Ok(result)
}
#[derive(Clone)]
struct StoredLink {
    local: Option<String>,
    identity: TrackIdentity,
    fields: Fields,
    status: String,
    observation_hash: Option<String>,
    /// False for a row whose observation was not kept: only a DJ-software track that is
    /// linked to a library track keeps its observation, the others keep its hash.
    has_observation: bool,
}
fn links(conn: &Connection, target: &str) -> Result<BTreeMap<String, StoredLink>> {
    let mut stmt=conn.prepare("SELECT content_id,local_track_id,identity_json,baseline_json,status,observation_hash,observation_json IS NOT NULL FROM native_track_links WHERE target_id=?1")?;
    let rows = stmt
        .query_map([target], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(
            |(id, local, identity, fields, status, observation_hash, has_observation)| {
                Ok((
                    id,
                    StoredLink {
                        local,
                        identity: serde_json::from_str(&identity)?,
                        fields: serde_json::from_str(&fields)?,
                        status,
                        observation_hash,
                        has_observation,
                    },
                ))
            },
        )
        .collect()
}
enum Candidate {
    Existing(String),
    New(PathBuf, String),
    Unavailable(&'static str),
}
fn eligibility(track: &ImportedTrack, selected: bool) -> Option<&'static str> {
    if track.deleted == Some(true) {
        Some("deleted_in_target")
    } else if track.deleted.is_none() {
        Some("unknown_deletion_state")
    } else if !selected {
        Some("unselected")
    } else if track
        .identity
        .master_song_id
        .as_deref()
        .is_none_or(str::is_empty)
    {
        Some("invalid_identity")
    } else if track
        .source
        .resolved_path
        .as_deref()
        .is_none_or(str::is_empty)
    {
        Some("unsupported_location")
    } else {
        None
    }
}
pub(crate) fn changed_identity(a: &TrackIdentity, b: &TrackIdentity) -> bool {
    a.database_id != b.database_id
        || a.content_id != b.content_id
        || (a.uuid.is_some() && b.uuid.is_some() && a.uuid != b.uuid)
        || (a.master_song_id.is_some()
            && b.master_song_id.is_some()
            && a.master_song_id != b.master_song_id)
}
fn store_records(
    tx: &Transaction<'_>,
    target: &str,
    input: &RekordboxCollection,
    cancel: &CancellationToken,
) -> Result<bool> {
    let mut existing = {
        let mut stmt =
            tx.prepare("SELECT table_name,record_id FROM native_records WHERE target_id=?1")?;
        stmt.query_map([target], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?
    };
    let mut changed = false;
    let mut stmt=tx.prepare_cached("INSERT INTO native_records(target_id,table_name,record_id,json,hash) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(target_id,table_name,record_id) DO UPDATE SET json=excluded.json,hash=excluded.hash WHERE native_records.hash!=excluded.hash")?;
    for (table, records) in &input.records {
        for record in records {
            cancel.check()?;
            existing.remove(&(table.clone(), record.id.clone()));
            let json = serde_json::to_string(record)?;
            let digest = format!("{:x}", Sha256::digest(json.as_bytes()));
            let kept = if RECORD_CONTENT_TABLES.contains(&table.as_str()) {
                json.as_str()
            } else {
                CONTENT_NOT_KEPT
            };
            changed |= stmt.execute(params![target, table, record.id, kept, digest])? > 0;
        }
    }
    drop(stmt);
    for (table, id) in existing {
        cancel.check()?;
        changed |= tx.execute(
            "DELETE FROM native_records WHERE target_id=?1 AND table_name=?2 AND record_id=?3",
            params![target, table, id],
        )? > 0;
    }
    Ok(changed)
}
impl Library {
    pub fn import_rekordbox_collection(
        &self,
        input: &RekordboxCollection,
        options: NativeImportOptions,
        cancel: &CancellationToken,
    ) -> Result<NativeImportReport> {
        self.observe_or_import_rekordbox_collection(input, options, cancel, true)
    }
    /// Record a coherent native observation and link already registered local
    /// sources. Sync previews never implicitly import native-only music/jobs.
    pub(crate) fn observe_rekordbox_collection(
        &self,
        input: &RekordboxCollection,
        options: NativeImportOptions,
        cancel: &CancellationToken,
    ) -> Result<NativeImportReport> {
        self.observe_or_import_rekordbox_collection(input, options, cancel, false)
    }
    fn observe_or_import_rekordbox_collection(
        &self,
        input: &RekordboxCollection,
        options: NativeImportOptions,
        cancel: &CancellationToken,
        allow_new_sources: bool,
    ) -> Result<NativeImportReport> {
        cancel.check()?;
        validate(input, &options, cancel)?;
        self.settings_revision(&options.settings_revision)?;
        let target = target_id(input)?;
        let digest = hash_with_cancel(input, Some(cancel))?;
        let current_links = {
            let conn = self.conn()?;
            links(&conn, &target)?
        };
        let mut candidates = BTreeMap::new();
        let mut sources: BTreeMap<PathBuf, String> = BTreeMap::new();
        for track in &input.tracks {
            cancel.check()?;
            let id = &track.identity.content_id;
            if let Some(status) = eligibility(
                track,
                options.content_ids.as_ref().is_none_or(|s| s.contains(id)),
            ) {
                candidates.insert(id.clone(), Candidate::Unavailable(status));
                continue;
            }
            if let Some(local) = current_links.get(id).and_then(|l| l.local.clone()) {
                candidates.insert(id.clone(), Candidate::Existing(local));
                continue;
            }
            let path = PathBuf::from(track.source.resolved_path.as_ref().unwrap());
            if !path.is_absolute() {
                candidates.insert(id.clone(), Candidate::Unavailable("unsupported_location"));
                continue;
            }
            let path = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    candidates.insert(
                        id.clone(),
                        Candidate::Unavailable(if e.kind() == std::io::ErrorKind::NotFound {
                            "missing_source"
                        } else {
                            "source_unavailable"
                        }),
                    );
                    continue;
                }
            };
            if !path.is_file() {
                candidates.insert(id.clone(), Candidate::Unavailable("not_regular"));
                continue;
            }
            let existing: Option<String> = self
                .conn()?
                .query_row(
                    "SELECT id FROM tracks WHERE path=?1",
                    [crate::model::path_bytes(&path)],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id_local) = existing {
                candidates.insert(id.clone(), Candidate::Existing(id_local));
                continue;
            }
            if !allow_new_sources {
                candidates.insert(id.clone(), Candidate::Unavailable("not_in_local_library"));
                continue;
            }
            let source_hash = if let Some(hash) = sources.get(&path) {
                hash.clone()
            } else {
                let hash = match file_sha256_cancellable(&path, cancel) {
                    Ok(hash) => hash,
                    Err(_) => {
                        cancel.check()?;
                        candidates.insert(id.clone(), Candidate::Unavailable("source_unavailable"));
                        continue;
                    }
                };
                sources.insert(path.clone(), hash.clone());
                hash
            };
            candidates.insert(id.clone(), Candidate::New(path, source_hash));
        }
        // Hashes above concern unregistered sources. Recheck rows below and
        // never reset a row that a watcher/preparer registered meanwhile.
        #[cfg(feature = "autotag")]
        let _gate = self
            .inner
            .tag_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let selected: String = tx.query_row(
            "SELECT revision FROM selected_settings WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            selected == options.settings_revision,
            "Preparation settings changed during native import"
        );
        cancel.check()?;
        let previous: Option<(String, u64)> = tx
            .query_row(
                "SELECT observation_hash,revision FROM native_targets WHERE id=?1",
                [&target],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let observation_changed = previous.as_ref().is_none_or(|p| p.0 != digest);
        let mut changed = observation_changed;
        tx.execute("INSERT OR IGNORE INTO native_targets(id,provider,database_id,device_id,identity_json,schema_fingerprint,observation_hash,issues_json,revision,created,updated) VALUES(?1,'rekordbox',?2,?3,?4,?5,?6,?8,1,?7,?7)",params![target,input.identity.database_id,input.identity.device_id.as_deref().unwrap_or(""),serde_json::to_string(&input.identity)?,input.schema_fingerprint,digest,crate::model::now(),serde_json::to_string(&input.issues)?])?;
        if observation_changed {
            changed |= store_records(&tx, &target, input, cancel)?;
        }
        let mut old_links = links(&tx, &target)?;
        let mut report = NativeImportReport {
            target_id: target.clone(),
            revision: previous.as_ref().map_or(1, |p| p.1),
            changed: false,
            tracks_created: 0,
            jobs_created: 0,
            statuses: BTreeMap::new(),
        };
        for track in &input.tracks {
            cancel.check()?;
            let id = &track.identity.content_id;
            let old = old_links.remove(id);
            let mut local = old.as_ref().and_then(|l| l.local.clone());
            let mut fields = old.as_ref().map(|l| l.fields.clone()).unwrap_or_default();
            let identity = old
                .as_ref()
                .filter(|l| l.local.is_some())
                .map(|l| l.identity.clone())
                .unwrap_or_else(|| track.identity.clone());
            let mut status = "linked";
            if old.as_ref().is_some_and(|l| {
                l.local.is_some() && changed_identity(&l.identity, &track.identity)
            }) {
                status = "identity_changed";
            } else {
                match &candidates[id] {
                    Candidate::Unavailable(value) => status = value,
                    Candidate::Existing(id_local) => {
                        let exists: bool = tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM tracks WHERE id=?1)",
                            [id_local],
                            |r| r.get(0),
                        )?;
                        if exists {
                            local = Some(id_local.clone())
                        } else {
                            status = "source_changed_retry"
                        }
                    }
                    Candidate::New(path, source_hash) => {
                        let existing: Option<String> = tx
                            .query_row(
                                "SELECT id FROM tracks WHERE path=?1",
                                [crate::model::path_bytes(path)],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(existing) = existing {
                            local = Some(existing)
                        } else {
                            let job = Self::enqueue_one(
                                &tx,
                                path,
                                source_hash,
                                &options.settings_revision,
                            )?;
                            let id_local: String = tx.query_row(
                                "SELECT track_id FROM jobs WHERE id=?1",
                                [&job],
                                |r| r.get(0),
                            )?;
                            let meta = &track.metadata;
                            tx.execute("UPDATE tracks SET title=?1,artist=?2,album=?3,genre=?4,search_text=?5,sort_title=?6,sort_artist=?7 WHERE id=?8",params![meta.title,meta.artist,meta.album,meta.genre,format!("{} {} {}",meta.title,meta.artist,meta.genre).to_lowercase(),meta.title.to_lowercase(),meta.artist.to_lowercase(),id_local])?;
                            local = Some(id_local);
                            report.tracks_created += 1;
                            report.jobs_created += 1;
                        }
                    }
                }
            }
            if fields.is_empty()
                && status == "linked"
                && let Some(local) = &local
            {
                let before = local_fields(&tx, local)?;
                for (field, remote) in remote_fields(track)? {
                    fields.insert(
                        field.clone(),
                        NativeFieldBaseline {
                            local: before.get(&field).cloned().unwrap_or(Value::Null),
                            remote,
                            owner: options
                                .initial_owners
                                .get(&field)
                                .copied()
                                .unwrap_or(NativeFieldOwner::Unowned),
                            acknowledged: false,
                        },
                    );
                }
            }
            let observed_hash = hash(track)?;
            if old.as_ref().is_none_or(|l| {
                l.local != local
                    || l.status != status
                    || l.observation_hash.as_deref() != Some(observed_hash.as_str())
                    || l.fields != fields
                    || (local.is_some() && !l.has_observation)
            }) {
                tx.execute("INSERT INTO native_track_links(target_id,content_id,local_track_id,identity_json,observation_json,observation_hash,baseline_json,status,updated) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(target_id,content_id) DO UPDATE SET local_track_id=excluded.local_track_id,identity_json=excluded.identity_json,observation_json=excluded.observation_json,observation_hash=excluded.observation_hash,baseline_json=excluded.baseline_json,status=excluded.status,updated=excluded.updated",params![target,id,local,serde_json::to_string(&identity)?,local.as_ref().map(|_| serde_json::to_string(track)).transpose()?,observed_hash,serde_json::to_string(&fields)?,status,crate::model::now()])?;
                changed = true;
            }
            *report.statuses.entry(status.into()).or_default() += 1;
        }
        for (id, old) in old_links {
            if old.status != "missing_from_target" {
                tx.execute("UPDATE native_track_links SET status='missing_from_target',observation_json=NULL,observation_hash=NULL,updated=?1 WHERE target_id=?2 AND content_id=?3",params![crate::model::now(),target,id])?;
                changed = true;
            }
            *report
                .statuses
                .entry("missing_from_target".into())
                .or_default() += 1;
        }
        if changed {
            report.revision = if let Some((_, rev)) = previous {
                rev.checked_add(1)
                    .context("Native target revision exhausted")?
            } else {
                1
            };
            tx.execute("UPDATE native_targets SET identity_json=?1,schema_fingerprint=?2,observation_hash=?3,revision=?4,updated=?5,issues_json=?7 WHERE id=?6",params![serde_json::to_string(&input.identity)?,input.schema_fingerprint,digest,report.revision,crate::model::now(),target,serde_json::to_string(&input.issues)?])?;
        }
        report.changed = changed;
        cancel.check()?;
        tx.commit()?;
        drop(conn);
        if observation_changed {
            self.tidy_after_bulk_change();
        }
        Ok(report)
    }
    pub fn native_targets(&self) -> Result<Vec<NativeTargetState>> {
        let ids = {
            let conn = self.conn()?;
            let mut stmt =
                conn.prepare("SELECT id FROM native_targets ORDER BY updated DESC,id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        ids.iter().map(|id| self.native_target(id)).collect()
    }
    pub fn native_target(&self, target: &str) -> Result<NativeTargetState> {
        let conn = self.conn()?;
        let (identity, schema, revision): (String, String, u64) = conn.query_row(
            "SELECT identity_json,schema_fingerprint,revision FROM native_targets WHERE id=?1",
            [target],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let records = conn.query_row(
            "SELECT COUNT(*) FROM native_records WHERE target_id=?1",
            [target],
            |r| r.get(0),
        )?;
        let tracks=conn.query_row("SELECT COUNT(*) FROM native_track_links WHERE target_id=?1 AND observation_hash IS NOT NULL",[target],|r|r.get(0))?;
        let issues: String = conn.query_row(
            "SELECT issues_json FROM native_targets WHERE id=?1",
            [target],
            |r| r.get(0),
        )?;
        Ok(NativeTargetState {
            id: target.into(),
            identity: serde_json::from_str(&identity)?,
            schema_fingerprint: schema,
            revision,
            records,
            tracks,
            issues: serde_json::from_str(&issues)?,
        })
    }
    pub fn native_track_link(&self, target: &str, content: &str) -> Result<NativeTrackLink> {
        let conn = self.conn()?;
        let (local,status,observation,fields,identity):(Option<String>,String,Option<String>,String,String)=conn.query_row("SELECT local_track_id,status,observation_json,baseline_json,identity_json FROM native_track_links WHERE target_id=?1 AND content_id=?2",params![target,content],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        Ok(NativeTrackLink {
            content_id: content.into(),
            local_track_id: local,
            baseline_identity: serde_json::from_str(&identity)?,
            status,
            observation: observation.map(|s| serde_json::from_str(&s)).transpose()?,
            fields: serde_json::from_str(&fields)?,
        })
    }
    pub fn native_record(
        &self,
        target: &str,
        table: &str,
        id: &str,
    ) -> Result<Option<TargetRecord>> {
        let json:Option<String>=self.conn()?.query_row("SELECT json FROM native_records WHERE target_id=?1 AND table_name=?2 AND record_id=?3",params![target,table,id],|r|r.get(0)).optional()?;
        json.map(|s| {
            ensure!(
                s != CONTENT_NOT_KEPT,
                "The content of {table} records is not kept, only their identity and hash"
            );
            serde_json::from_str(&s).map_err(Into::into)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_during_streaming_digest_is_an_error_not_an_interrupted_write_retry() {
        struct Cancels<'a>(&'a CancellationToken);
        impl Serialize for Cancels<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                s: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                self.0.cancel();
                "cancelled".serialize(s)
            }
        }
        let token = CancellationToken::default();
        assert!(hash_with_cancel(&Cancels(&token), Some(&token)).is_err());
    }
}
