use super::*;
use crate::{Library, model::now};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub(super) const MAX_JSON: usize = 64 * 1024 * 1024;
pub(super) fn json<T: Serialize>(value: &T) -> Result<String> {
    struct Limited(Vec<u8>);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_JSON {
                return Err(std::io::Error::other(
                    "Complete Curate scope exceeds64MiB; narrow selection",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut result = Limited(Vec::new());
    serde_json::to_writer(&mut result, value)?;
    Ok(String::from_utf8(result.0)?)
}
pub(super) fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(value)?)))
}
pub(super) fn tax_revision(conn: &Connection) -> Result<u64> {
    Ok(conn.query_row(
        "SELECT value FROM meta WHERE key='curation_taxonomy_revision'",
        [],
        |row| row.get(0),
    )?)
}
pub(super) fn page(offset: usize, limit: usize) -> Result<()> {
    ensure!(
        (1..=200).contains(&limit) && offset <= i64::MAX as usize,
        "Curate page requires limit1..200 and a valid offset"
    );
    Ok(())
}
pub(super) fn valid_request(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 128 && !id.contains('\0'),
        "Invalid Curate request ID"
    );
    Ok(())
}
pub(super) fn mutation(
    library: &Library,
    id: &str,
    kind: &str,
    hash: &str,
) -> Result<Option<String>> {
    let row: Option<(String, String, String)> = library
        .conn()?
        .query_row(
            "SELECT kind,digest,receipt_json FROM curation_mutations WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(saved_kind, saved_hash, text)| {
        ensure!(
            saved_kind == kind && saved_hash == hash,
            "Curate request ID was reused with different content"
        );
        Ok(text)
    })
    .transpose()
}
pub(super) fn save_mutation(
    conn: &Connection,
    id: &str,
    kind: &str,
    hash: &str,
    receipt: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO curation_mutations VALUES(?1,?2,?3,?4,?5)",
        params![id, kind, hash, receipt, now()],
    )?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Input {
    pub path: Option<Vec<u8>>,
    pub source_hash: Option<String>,
    pub path_revision: u64,
    pub metadata_revision: u64,
    pub current_job: Option<String>,
    pub state: String,
}
#[derive(Clone)]
pub(super) struct Head {
    pub track_id: String,
    pub title: String,
    pub revision: u64,
    pub acknowledged: u64,
    pub desired: CurationValues,
    pub committed: CurationValues,
    pub state: String,
    pub error: Option<String>,
    pub active: Option<String>,
    pub last: Option<String>,
    pub native_stale: bool,
    pub input: Input,
    pub measured: serde_json::Value,
    pub native_required: bool,
    pub mappings: Vec<super::mapping::FrozenMapping>,
    pub taxonomy_revision: u64,
    pub reexport: bool,
    pub last_native: Option<String>,
    pub current_input: Input,
    pub input_proof: Option<String>,
    pub discardable: bool,
}
pub(super) fn categories(
    conn: &Connection,
    id: &str,
) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut query=conn.prepare("SELECT t.category_id,t.id FROM track_custom_tags l JOIN custom_tags t ON t.id=l.tag_id LEFT JOIN curation_group_properties g ON g.category_id=t.category_id LEFT JOIN curation_value_properties v ON v.tag_id=t.id WHERE l.track_id=?1 AND COALESCE(g.deleted,0)=0 AND COALESCE(v.deleted,0)=0 ORDER BY t.category_id,t.id")?;
    let rows = query
        .query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = std::collections::BTreeMap::new();
    for (group, value) in rows {
        result.entry(group).or_insert_with(Vec::new).push(value);
    }
    Ok(result)
}
type TrackFacts = (
    Option<Vec<u8>>,
    Option<String>,
    u64,
    u64,
    Option<String>,
    String,
    bool,
    String,
    Option<String>,
    Option<f64>,
    Option<u8>,
);
type SavedHead = (
    u64,
    u64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    String,
);
pub(super) fn head(library: &Library, id: &str) -> Result<Head> {
    let (facts, saved, categories, measured, title) = {
        let conn = library.conn()?;
        let(path,hash,path_rev,meta_rev,job,state,archived,title,key,bpm,energy):TrackFacts=conn.query_row("SELECT path,source_hash,path_revision,metadata_revision,current_job,state,archived,title,musical_key,bpm,energy FROM tracks WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?))).context("Unknown Curate track")?;
        ensure!(
            !archived && matches!(state.as_str(), "ready" | "incoming"),
            "Choose an active Ready or Incoming track; restore an archived track before Retry or Discard"
        );
        let facts = Input {
            path,
            source_hash: hash,
            path_revision: path_rev,
            metadata_revision: meta_rev,
            current_job: job,
            state,
        };
        let saved:Option<SavedHead>=conn.query_row("SELECT revision,acknowledged_revision,desired_json,committed_json,state,error,active_publication_id,last_publication_id,native_stale,input_json FROM curation_tracks WHERE track_id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?))).optional()?;
        let categories = categories(&conn, id)?;
        (
            facts,
            saved,
            categories,
            serde_json::json!({"key":key,"bpm":bpm,"energy":energy}),
            title,
        )
    };
    let mut result = Head {
        track_id: id.into(),
        title,
        revision: 0,
        acknowledged: 0,
        desired: CurationValues {
            categories: categories.clone(),
            ..Default::default()
        },
        committed: Default::default(),
        state: "saved".into(),
        error: None,
        active: None,
        last: None,
        native_stale: false,
        input: facts.clone(),
        measured,
        native_required: false,
        mappings: vec![],
        taxonomy_revision: 0,
        reexport: false,
        last_native: None,
        current_input: facts.clone(),
        input_proof: None,
        discardable: true,
    };
    result.committed = result.desired.clone();
    if let Some((rev, ack, desired, committed, state, error, active, last, stale, input)) = saved {
        ensure!(
            desired.len() <= MAX_JSON && committed.len() <= MAX_JSON && input.len() <= 1024 * 1024,
            "Curate stored payload exceeds bounded size"
        );
        result.revision = rev;
        result.acknowledged = ack;
        result.desired = serde_json::from_str(&desired)?;
        result.committed = serde_json::from_str(&committed)?;
        // Legacy cleanup shares stable category links. Preserve any group that
        // this pending revision did not edit instead of replacing its later value.
        let groups = result
            .committed
            .categories
            .keys()
            .chain(categories.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for group in groups {
            if result.desired.categories.get(&group) == result.committed.categories.get(&group) {
                match categories.get(&group) {
                    Some(values) => {
                        result.desired.categories.insert(group, values.clone());
                    }
                    None => {
                        result.desired.categories.remove(&group);
                    }
                }
            }
        }
        result.committed.categories = categories;
        result.state = state;
        result.error = error;
        result.active = active;
        result.last = last;
        result.native_stale = stale;
        result.input = serde_json::from_str(&input)?;
    }
    let native_plan:Option<(bool,String,u64,bool,Option<String>)>=library.conn()?.query_row("SELECT native_required,pending_mappings_json,pending_taxonomy_revision,pending_reexport,last_native_publication_id FROM curation_tracks WHERE track_id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
    if let Some((required, plan, revision, reexport, last)) = native_plan {
        ensure!(plan.len() <= MAX_JSON, "Native Curate plan exceeds bound");
        result.native_required = required;
        result.mappings = serde_json::from_str(&plan)?;
        result.taxonomy_revision = revision;
        result.reexport = reexport;
        result.last_native = last;
    }
    result.input_proof = library
        .conn()?
        .query_row(
            "SELECT input_proof_json FROM curation_tracks WHERE track_id=?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    result.native_stale |= library.conn()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM curation_native_stale WHERE track_id=?1)",
        [id],
        |row| row.get::<_, bool>(0),
    )?;
    result.discardable = discardable(&*library.conn()?, result.active.as_deref())?;
    Ok(result)
}
pub(super) fn discardable(conn: &Connection, active: Option<&str>) -> Result<bool> {
    let Some(active) = active else {
        return Ok(true);
    };
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM curation_publications p WHERE p.id=?1 AND p.state IN ('error','recovery_required','waiting_incoming') AND NOT EXISTS(SELECT 1 FROM metadata_edit_writes w WHERE w.batch_id=p.child_batch_id AND w.state IN ('committed','undone')) AND NOT EXISTS(SELECT 1 FROM metadata_edit_overrides o JOIN jobs j ON j.id=o.job_id WHERE o.batch_id=p.child_batch_id AND (o.state IN ('committed','undone','expired') OR j.state='running' OR EXISTS(SELECT 1 FROM tag_writes w WHERE w.job_id=o.job_id AND w.state='prepared'))))",[active],|row|row.get(0))?)
}
pub(super) fn public(head: Head) -> CurationTrackState {
    CurationTrackState {
        track_id: head.track_id,
        title: head.title,
        desired_revision: head.revision,
        acknowledged_revision: head.acknowledged,
        desired: head.desired,
        committed: head.committed,
        state: head.state.clone(),
        error: head.error,
        active_publication_id: head.active,
        native_stale: head.native_stale,
        can_retry: matches!(
            head.state.as_str(),
            "error" | "recovery_required" | "waiting_incoming"
        ),
        can_discard: head.discardable && head.revision > head.acknowledged,
        measured: head.measured,
    }
}
pub(super) fn apply_values(conn: &Connection, track: &str, values: &CurationValues) -> Result<()> {
    conn.execute("DELETE FROM track_custom_tags WHERE track_id=?1", [track])?;
    for (group, ids) in &values.categories {
        ensure!(ids.len()<=1 || !conn.query_row("SELECT COALESCE((SELECT mode='single' FROM curation_group_properties WHERE category_id=?1),0)",[group],|row|row.get::<_,bool>(0))?,"Curate category changed to single selection; reconcile pending values");
        for id in ids {
            ensure!(conn.query_row("SELECT EXISTS(SELECT 1 FROM custom_tags t LEFT JOIN curation_group_properties g ON g.category_id=t.category_id LEFT JOIN curation_value_properties v ON v.tag_id=t.id WHERE t.id=?1 AND t.category_id=?2 AND COALESCE(g.deleted,0)=0 AND COALESCE(v.deleted,0)=0)",params![id,group],|row|row.get::<_,bool>(0))?,"Curate taxonomy changed; assignment not published");
            conn.execute(
                "INSERT INTO track_custom_tags VALUES(?1,?2)",
                params![track, id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod discard_tests {
    use super::*;

    fn conn(override_state: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE curation_publications(id TEXT PRIMARY KEY,state TEXT,child_batch_id TEXT);
             CREATE TABLE metadata_edit_writes(batch_id TEXT,state TEXT);
             CREATE TABLE metadata_edit_overrides(job_id TEXT,batch_id TEXT,state TEXT);
             CREATE TABLE jobs(id TEXT PRIMARY KEY,state TEXT);
             CREATE TABLE tag_writes(job_id TEXT,state TEXT);
             INSERT INTO curation_publications VALUES('publication','waiting_incoming','child');
             INSERT INTO jobs VALUES('job','completed');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata_edit_overrides VALUES('job','child',?1)",
            [override_state],
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_publication_whose_incoming_edit_was_written_is_not_discardable_whatever_became_of_it() {
        for written in ["committed", "undone", "expired"] {
            assert!(
                !discardable(&conn(written), Some("publication")).unwrap(),
                "{written}: the tags were written"
            );
        }
        // An edit that was never written leaves nothing to keep.
        for waiting in ["queued", "claimed", "discarded"] {
            assert!(
                discardable(&conn(waiting), Some("publication")).unwrap(),
                "{waiting}"
            );
        }
    }
}
