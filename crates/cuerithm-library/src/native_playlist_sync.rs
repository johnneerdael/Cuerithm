//! Explicit local playlist/MyTag import and side-effect-free native export plans.
//! Names never establish ownership; only verified publication receipts can do so.
use crate::{Library, PlaylistKind, PlaylistNode, playlists};
use anyhow::{Context, Result, ensure};
use cuerithm_rekordbox::{ImportedPlaylist, ImportedTag, RekordboxCollection};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlaylistImportOptions {
    pub native_revision: u64,
    pub playlist_revision: u64,
    /// Native smart rules are opaque unless their complete semantics are known.
    /// Accepting a native ID here explicitly imports its captured whole list.
    pub materialize_native_ids: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlaylistImportReport {
    pub target_id: String,
    pub changed: bool,
    pub playlist_revision: u64,
    /// Playlist history entry only; playlist Undo does not revert MyTag edits.
    pub edit_id: Option<String>,
    pub playlists_created: usize,
    pub categories_created: usize,
    pub tags_created: usize,
    pub materialized_native_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlaylistExportOptions {
    pub native_revision: u64,
    /// Select explicit local identities. Ancestors must be selected or bound.
    pub playlist_ids: Vec<String>,
    pub category_ids: Vec<String>,
    pub materialize_smart_playlists: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePlaylistIdentity {
    pub native_id: String,
    pub uuid: Option<String>,
    pub expected_raw_hash: String,
    pub owned: bool,
    pub owned_link_ids: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativePlaylistRepresentation {
    Folder,
    Ordinary {
        content_ids: Vec<String>,
    },
    /// An unchanged opaque definition retains its native semantics, never a
    /// partially translated subset. Members are the captured native results.
    NativeSmart {
        opaque_rule: String,
        content_ids: Vec<String>,
    },
    MaterializedSmart {
        content_ids: Vec<String>,
        definition_json: Value,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum NativePlaylistMutation {
    Upsert {
        local_id: String,
        identity: Option<NativePlaylistIdentity>,
        parent_local_id: Option<String>,
        name: String,
        position: u32,
        representation: NativePlaylistRepresentation,
        /// Matched/imported entities never authorize removal of foreign links.
        preserve_external_members: bool,
    },
    Remove {
        local_id: String,
        identity: NativePlaylistIdentity,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum NativeTagMutation {
    Upsert {
        local_id: String,
        identity: Option<NativePlaylistIdentity>,
        category_local_id: Option<String>,
        name: String,
        /// Zero-based sibling position, matching local playlist positions.
        position: u32,
        content_ids: Vec<String>,
        preserve_external_links: bool,
    },
    Remove {
        local_id: String,
        identity: NativePlaylistIdentity,
        category: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlaylistPlan {
    pub target_id: String,
    pub native_revision: u64,
    pub playlist_revision: u64,
    pub library_revision: u64,
    pub source_fingerprint: String,
    pub playlists: Vec<NativePlaylistMutation>,
    pub tags: Vec<NativeTagMutation>,
    /// Existing parents can be referenced without being mutated.
    pub bindings: BTreeMap<String, NativePlaylistIdentity>,
    pub existing_native_ids: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePlaylistObjectKind {
    Playlist,
    Category,
    Tag,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlaylistPublicationReceipt {
    pub kind: NativePlaylistObjectKind,
    pub local_id: String,
    pub native_id: String,
    /// Supply only IDs whose insertion was verified in this publication.
    /// Legacy coordinator receipts can include retained links; acknowledgement
    /// requires their original binding and unchanged relationship generation.
    pub created_link_ids: BTreeSet<String>,
}
#[derive(Debug, Clone)]
struct Binding {
    kind: String,
    native_id: String,
    uuid: Option<String>,
    local_id: String,
    owned: bool,
    remote: Value,
    local: Value,
    revision: u64,
    owned_links: BTreeSet<String>,
}
fn bindings(conn: &Connection, target: &str) -> Result<Vec<Binding>> {
    let mut stmt=conn.prepare("SELECT kind,native_id,uuid,local_id,owned,remote_json,local_json,native_revision,owned_links_json FROM native_playlist_bindings WHERE target_id=?1 ORDER BY kind,native_id")?;
    let rows = stmt
        .query_map([target], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, u64>(7)?,
                r.get::<_, String>(8)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(
            |(kind, native_id, uuid, local_id, owned, remote, local, revision, links)| {
                Ok(Binding {
                    kind,
                    native_id,
                    uuid,
                    local_id,
                    owned,
                    remote: serde_json::from_str(&remote)?,
                    local: serde_json::from_str(&local)?,
                    revision,
                    owned_links: serde_json::from_str(&links)?,
                })
            },
        )
        .collect()
}
fn save_binding(conn: &Connection, target: &str, b: &Binding) -> Result<()> {
    conn.execute("INSERT INTO native_playlist_bindings(target_id,kind,native_id,uuid,local_id,owned,remote_json,local_json,native_revision,owned_links_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(target_id,kind,native_id) DO UPDATE SET remote_json=excluded.remote_json,local_json=excluded.local_json,native_revision=excluded.native_revision,owned_links_json=excluded.owned_links_json",params![target,b.kind,b.native_id,b.uuid,b.local_id,b.owned,serde_json::to_string(&b.remote)?,serde_json::to_string(&b.local)?,b.revision,serde_json::to_string(&b.owned_links)?])?;
    Ok(())
}
fn native_revision(conn: &Connection, target: &str, expected: u64) -> Result<()> {
    let actual: u64 = conn.query_row(
        "SELECT revision FROM native_targets WHERE id=?1",
        [target],
        |r| r.get(0),
    )?;
    ensure!(
        actual == expected,
        "Native collection changed; observe and review it again"
    );
    Ok(())
}
fn root_parent(id: Option<&str>) -> Option<&str> {
    id.filter(|id| !id.is_empty() && *id != "root" && *id != "0")
}
fn playlist_local(row: &playlists::StoredNode) -> Result<Value> {
    // Preserve user edit identity separately; a no-op import must not alter Undo.
    Ok(json!({"node":row.node,"members":row.members}))
}
fn tag_local(conn: &Connection, kind: &str, id: &str) -> Result<Option<Value>> {
    if kind == "category" {
        return Ok(conn
            .query_row(
                "SELECT c.name FROM custom_tag_categories c LEFT JOIN curation_group_properties g ON g.category_id=c.id WHERE c.id=?1 AND COALESCE(g.deleted,0)=0",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|name| json!({"name":name})));
    }
    let row = conn
        .query_row(
            "SELECT t.category_id,t.name FROM custom_tags t LEFT JOIN curation_value_properties v ON v.tag_id=t.id LEFT JOIN curation_group_properties g ON g.category_id=t.category_id WHERE t.id=?1 AND COALESCE(v.deleted,0)=0 AND COALESCE(g.deleted,0)=0",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    row.map(|(category, name)| {
        let tracks = conn
            .prepare("SELECT track_id FROM track_custom_tags WHERE tag_id=?1 ORDER BY track_id")?
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"category_id":category,"name":name,"track_ids":tracks}))
    })
    .transpose()
}
fn track_maps(
    conn: &Connection,
    target: &str,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>)> {
    let rows=conn.prepare("SELECT content_id,local_track_id,status FROM native_track_links WHERE target_id=?1 AND local_track_id IS NOT NULL AND observation_hash IS NOT NULL")?.query_map([target],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut remote = BTreeMap::new();
    let mut local = BTreeMap::new();
    for (content, track, status) in rows {
        if status != "linked" {
            continue;
        }
        ensure!(
            local.insert(track.clone(), content.clone()).is_none(),
            "Ambiguous native track mapping"
        );
        remote.insert(content, track);
    }
    Ok((remote, local))
}
fn link_uuids(
    input: &RekordboxCollection,
    table: &str,
    ids: BTreeSet<&str>,
) -> BTreeMap<String, Value> {
    let mut result = ids
        .into_iter()
        .map(|id| (id.to_owned(), Value::Null))
        .collect::<BTreeMap<_, _>>();
    if result.is_empty() {
        return result;
    }
    for row in input.records.get(table).into_iter().flatten() {
        if let Some(value) = result.get_mut(&row.id) {
            *value = match row.values.get("UUID") {
                Some(cuerithm_rekordbox::TargetValue::Text(uuid)) => json!(uuid),
                _ => Value::Null,
            };
        }
    }
    result
}
fn retained_imported_links(previous: &Binding, current: &Value) -> BTreeSet<String> {
    let hashes = |remote: &Value| {
        remote["links"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|link| {
                Some((
                    link["id"].as_str()?.to_owned(),
                    link["raw_hash"].as_str()?.to_owned(),
                ))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = hashes(&previous.remote);
    let after = hashes(current);
    previous
        .owned_links
        .iter()
        .filter(|id| {
            let (Some(before_hash), Some(after_hash)) = (before.get(*id), after.get(*id)) else {
                return false;
            };
            match (
                previous.remote["link_uuids"][*id].as_str(),
                current["link_uuids"][*id].as_str(),
            ) {
                (Some(before), Some(after)) => before == after,
                (None, None) => before_hash == after_hash,
                _ => false,
            }
        })
        .cloned()
        .collect()
}
fn playlist_remote(input: &RekordboxCollection, p: &ImportedPlaylist) -> Value {
    let mut links: Vec<_> = input
        .playlist_links
        .iter()
        .filter(|l| l.playlist_id == p.id && l.deleted == Some(false))
        .collect();
    links.sort_by_key(|l| (l.position, &l.id));
    let uuids = link_uuids(
        input,
        "djmdSongPlaylist",
        links.iter().map(|link| link.id.as_str()).collect(),
    );
    json!({"entity":p,"content_ids":links.iter().map(|l|&l.content_id).collect::<Vec<_>>(),"links":links,"link_uuids":uuids})
}
fn tag_remote(input: &RekordboxCollection, t: &ImportedTag) -> Value {
    let mut links: Vec<_> = input
        .tag_links
        .iter()
        .filter(|l| l.tag_id == t.id && l.deleted == Some(false))
        .collect();
    links.sort_by_key(|l| &l.id);
    let uuids = link_uuids(
        input,
        "djmdSongMyTag",
        links.iter().map(|link| link.id.as_str()).collect(),
    );
    json!({"entity":t,"links":links,"link_uuids":uuids})
}
fn identity(b: &Binding) -> NativePlaylistIdentity {
    NativePlaylistIdentity {
        native_id: b.native_id.clone(),
        uuid: b.uuid.clone(),
        expected_raw_hash: b.remote["entity"]["raw_hash"]
            .as_str()
            .unwrap_or_default()
            .into(),
        owned: b.owned,
        owned_link_ids: b.owned_links.clone(),
    }
}
/// A target revision may advance for unrelated track fields. Require complete
/// entity and active relationship proofs before retaining a stale binding.
type RelationshipProofs = BTreeMap<String, String>;
type LinkObservations = BTreeMap<String, Option<RelationshipProofs>>;
#[derive(Default)]
struct BindingObservationCache {
    tables: BTreeMap<String, LinkObservations>,
}
fn binding_observation_is_current(
    conn: &Connection,
    target: &str,
    b: &Binding,
    cache: &mut BindingObservationCache,
) -> Result<bool> {
    let (table, links, parent) = if b.kind == "playlist" {
        ("djmdPlaylist", "djmdSongPlaylist", "PlaylistID")
    } else {
        ("djmdMyTag", "djmdSongMyTag", "MyTagID")
    };
    let entity: Option<String> = conn
        .query_row(
            "SELECT json FROM native_records WHERE target_id=?1 AND table_name=?2 AND record_id=?3",
            params![target, table, b.native_id],
            |r| r.get(0),
        )
        .optional()?;
    let entity = entity
        .map(|row| serde_json::from_str::<cuerithm_rekordbox::TargetRecord>(&row))
        .transpose()?;
    if entity.as_ref().map(|row| row.fingerprint.as_str())
        != b.remote["entity"]["raw_hash"].as_str()
    {
        return Ok(false);
    }
    if !cache.tables.contains_key(links) {
        let rows = conn
            .prepare("SELECT json FROM native_records WHERE target_id=?1 AND table_name=?2")?
            .query_map(params![target, links], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut owners = LinkObservations::new();
        for row in rows {
            let row: cuerithm_rekordbox::TargetRecord = serde_json::from_str(&row)?;
            let owner = match row.values.get(parent) {
                Some(cuerithm_rekordbox::TargetValue::Text(id)) => id.clone(),
                Some(cuerithm_rekordbox::TargetValue::Integer(id)) => id.to_string(),
                _ => continue,
            };
            if row.values.get("rb_local_deleted")
                == Some(&cuerithm_rekordbox::TargetValue::Integer(1))
            {
                continue;
            }
            let records = owners
                .entry(owner)
                .or_insert_with(|| Some(RelationshipProofs::new()));
            if row.values.get("rb_local_deleted")
                != Some(&cuerithm_rekordbox::TargetValue::Integer(0))
            {
                *records = None;
                continue;
            }
            if let Some(records) = records {
                records.insert(row.id, row.fingerprint);
            }
        }
        cache.tables.insert(links.into(), owners);
    }
    let observations = cache
        .tables
        .get(links)
        .expect("captured native relationship table");
    let current = match observations.get(&b.native_id) {
        Some(Some(rows)) => Some(rows),
        Some(None) => return Ok(false),
        None => None,
    };
    let Some(expected) = b.remote["links"].as_array() else {
        return Ok(false);
    };
    let mut previous = BTreeMap::new();
    for row in expected {
        let (Some(id), Some(hash)) = (row["id"].as_str(), row["raw_hash"].as_str()) else {
            return Ok(false);
        };
        previous.insert(id.to_owned(), hash.to_owned());
    }
    Ok(current.map_or(previous.is_empty(), |current| current == &previous))
}
pub(crate) fn source_fingerprint(conn: &Connection) -> Result<String> {
    let tree = playlists::load(conn)?;
    let tags = conn
        .prepare("SELECT id FROM custom_tag_categories ORDER BY id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let tag_ids = conn
        .prepare("SELECT id FROM custom_tags ORDER BY id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut values = vec![serde_json::to_value(tree)?];
    for id in tags {
        values.push(json!([id, tag_local(conn, "category", &id)?]));
    }
    for id in tag_ids {
        values.push(json!([id, tag_local(conn, "tag", &id)?]));
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&values)?)
    ))
}
fn content_ids(
    conn: &Connection,
    mapping: &BTreeMap<String, String>,
    tracks: &[String],
) -> Result<Vec<String>> {
    tracks
        .iter()
        .map(|track| {
            let ready: bool = conn.query_row(
                "SELECT state='ready' AND archived=0 FROM tracks WHERE id=?1",
                [track],
                |r| r.get(0),
            )?;
            ensure!(ready, "Only prepared active tracks can be exported");
            mapping
                .get(track)
                .cloned()
                .context("Playlist member has no unambiguous native track identity")
        })
        .collect()
}
fn ordered_subset(wanted: &[String], actual: &[String]) -> bool {
    let mut next = 0;
    for id in actual {
        if wanted.get(next) == Some(id) {
            next += 1;
        }
    }
    next == wanted.len()
}
fn verify_receipt_identity(
    plan: &NativePlaylistPlan,
    previous: Option<&NativePlaylistIdentity>,
    table: &str,
    id: &str,
    uuid: Option<&String>,
) -> Result<()> {
    if let Some(previous) = previous {
        ensure!(
            previous.native_id == id && previous.uuid.as_ref() == uuid,
            "Published native identity changed"
        );
    } else {
        ensure!(
            !plan.existing_native_ids.contains(&format!("{table}:{id}")),
            "Publication cannot claim an existing external native object"
        );
    }
    Ok(())
}
fn receipt_links(
    plan: &NativePlaylistPlan,
    previous: Option<&NativePlaylistIdentity>,
    binding: Option<&Binding>,
    receipt: &NativePlaylistPublicationReceipt,
    remote: &Value,
    table: &str,
    wanted: &[String],
) -> Result<BTreeSet<String>> {
    let present: BTreeSet<_> = remote["links"]
        .as_array()
        .context("Native publication links missing")?
        .iter()
        .filter_map(|l| l["id"].as_str().map(str::to_owned))
        .collect();
    // Retaining an ID is safe only when the frozen identity still matches its
    // original ownership binding, and the relationship has the same generation
    // and content. A reorder changes its raw hash but preserves its UUID.
    let retained = match (previous, binding) {
        (Some(previous), Some(binding)) if identity(binding) == *previous => {
            retained_imported_links(binding, remote)
                .into_iter()
                .filter(|id| {
                    let content = |value: &Value| {
                        value["links"]
                            .as_array()
                            .and_then(|links| {
                                links
                                    .iter()
                                    .find(|link| link["id"].as_str() == Some(id.as_str()))
                            })
                            .and_then(|link| link["content_id"].as_str())
                            .map(str::to_owned)
                    };
                    content(&binding.remote)
                        .is_some_and(|before| content(remote).as_ref() == Some(&before))
                })
                .collect::<BTreeSet<_>>()
        }
        _ => BTreeSet::new(),
    };
    for id in &receipt.created_link_ids {
        ensure!(
            present.contains(id)
                && (!plan.existing_native_ids.contains(&format!("{table}:{id}"))
                    || retained.contains(id)),
            "Cannot claim an external or missing native member link"
        );
    }
    if let Some(previous) = previous {
        for link in remote["links"]
            .as_array()
            .context("Native publication links missing")?
        {
            if link["id"]
                .as_str()
                .is_some_and(|id| previous.owned_link_ids.contains(id))
            {
                ensure!(
                    link["content_id"]
                        .as_str()
                        .is_some_and(|id| wanted.iter().any(|wanted| wanted == id)),
                    "Removed owned member link remains in the published collection"
                );
            }
        }
    }
    let mut owned = retained;
    owned.extend(receipt.created_link_ids.iter().cloned());
    Ok(owned)
}

impl Library {
    /// Recheck immediately before staging/publication.
    pub fn validate_native_playlist_plan(&self, plan: &NativePlaylistPlan) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        native_revision(&tx, &plan.target_id, plan.native_revision)?;
        let revision: u64 =
            tx.query_row("SELECT value FROM meta WHERE key='revision'", [], |r| {
                r.get(0)
            })?;
        ensure!(
            revision == plan.library_revision
                && playlists::revision(&tx)? == plan.playlist_revision
                && source_fingerprint(&tx)? == plan.source_fingerprint,
            "Local library changed; review playlist export again"
        );
        tx.commit()?;
        Ok(())
    }
    /// Record ownership only after the coordinator has verified publication and
    /// observed the resulting collection. Repeating a receipt is idempotent.
    pub fn acknowledge_native_playlist_plan(
        &self,
        plan: &NativePlaylistPlan,
        published: &RekordboxCollection,
        receipts: &[NativePlaylistPublicationReceipt],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(plan)?));
        let receipt_hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!([published, receipts]))?)
        );
        let acknowledged:Option<String>=tx.query_row("SELECT receipt_hash FROM native_playlist_acknowledgements WHERE target_id=?1 AND plan_hash=?2",params![plan.target_id,plan_hash],|r|r.get(0)).optional()?;
        if let Some(previous) = acknowledged {
            ensure!(
                previous == receipt_hash,
                "Native playlist publication receipt changed after acknowledgement"
            );
            tx.commit()?;
            return Ok(());
        }
        ensure!(
            source_fingerprint(&tx)? == plan.source_fingerprint,
            "Local playlist or MyTag changed before publication acknowledgement"
        );
        let (observed, revision): (String, u64) = tx.query_row(
            "SELECT observation_hash,revision FROM native_targets WHERE id=?1",
            [&plan.target_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            observed == format!("{:x}", Sha256::digest(serde_json::to_vec(published)?)),
            "Publication receipt is not the observed native collection"
        );
        ensure!(
            revision >= plan.native_revision,
            "Native publication acknowledgement is stale"
        );
        let mut receipt_map = BTreeMap::new();
        for r in receipts {
            let kind = match r.kind {
                NativePlaylistObjectKind::Playlist => "playlist",
                NativePlaylistObjectKind::Category => "category",
                NativePlaylistObjectKind::Tag => "tag",
            };
            ensure!(
                receipt_map.insert((kind, r.local_id.as_str()), r).is_none(),
                "Duplicate native publication receipt"
            );
        }
        let old = bindings(&tx, &plan.target_id)?;
        let tree = playlists::load(&tx)?;
        let mut expected_receipts = 0;
        for mutation in &plan.playlists {
            match mutation {
                NativePlaylistMutation::Remove { local_id, identity } => {
                    ensure!(
                        identity.owned,
                        "Cannot acknowledge deletion of an external playlist"
                    );
                    ensure!(
                        !published
                            .playlists
                            .iter()
                            .any(|p| p.id == identity.native_id && p.deleted == Some(false)),
                        "Owned playlist deletion was not published"
                    );
                    tx.execute("DELETE FROM native_playlist_bindings WHERE target_id=?1 AND kind='playlist' AND native_id=?2 AND local_id=?3",params![plan.target_id,identity.native_id,local_id])?;
                }
                NativePlaylistMutation::Upsert {
                    local_id,
                    identity: previous,
                    parent_local_id,
                    name,
                    position,
                    representation,
                    ..
                } => {
                    expected_receipts += 1;
                    let receipt = receipt_map
                        .get(&("playlist", local_id.as_str()))
                        .context("Missing playlist publication receipt")?;
                    let entity = published
                        .playlists
                        .iter()
                        .find(|p| p.id == receipt.native_id && p.deleted == Some(false))
                        .context("Published playlist is missing")?;
                    verify_receipt_identity(
                        plan,
                        previous.as_ref(),
                        "djmdPlaylist",
                        &receipt.native_id,
                        entity.uuid.as_ref(),
                    )?;
                    ensure!(
                        &entity.name == name,
                        "Published playlist name differs from the reviewed plan"
                    );
                    ensure!(
                        entity.sequence == i64::from(*position) + 1,
                        "Published playlist order differs from the reviewed plan"
                    );
                    let expected_parent = parent_local_id
                        .as_ref()
                        .map(|id| {
                            receipt_map
                                .get(&("playlist", id.as_str()))
                                .map(|r| r.native_id.as_str())
                                .or_else(|| plan.bindings.get(id).map(|b| b.native_id.as_str()))
                                .context("Published parent identity is missing")
                        })
                        .transpose()?;
                    ensure!(
                        root_parent(entity.parent_id.as_deref()) == expected_parent,
                        "Published playlist parent differs from the reviewed plan"
                    );
                    let remote = playlist_remote(published, entity);
                    let wanted = match representation {
                        NativePlaylistRepresentation::Folder => {
                            ensure!(entity.attribute == 1, "Published folder kind differs");
                            &[][..]
                        }
                        NativePlaylistRepresentation::Ordinary { content_ids }
                        | NativePlaylistRepresentation::MaterializedSmart { content_ids, .. } => {
                            ensure!(
                                entity.attribute == 0 && entity.smart_rule.is_none(),
                                "Published ordinary list kind differs"
                            );
                            content_ids.as_slice()
                        }
                        NativePlaylistRepresentation::NativeSmart {
                            opaque_rule,
                            content_ids,
                        } => {
                            ensure!(
                                entity.attribute == 4
                                    && entity.smart_rule.as_ref() == Some(opaque_rule),
                                "Published native smart definition differs"
                            );
                            content_ids.as_slice()
                        }
                    };
                    let actual: Vec<String> =
                        serde_json::from_value(remote["content_ids"].clone())?;
                    ensure!(
                        ordered_subset(wanted, &actual),
                        "Published playlist membership/order differs from reviewed membership"
                    );
                    let owned_links = receipt_links(
                        plan,
                        previous.as_ref(),
                        old.iter().find(|binding| {
                            binding.kind == "playlist"
                                && binding.local_id == *local_id
                                && binding.native_id == receipt.native_id
                        }),
                        receipt,
                        &remote,
                        "djmdSongPlaylist",
                        wanted,
                    )?;
                    save_binding(
                        &tx,
                        &plan.target_id,
                        &Binding {
                            kind: "playlist".into(),
                            native_id: entity.id.clone(),
                            uuid: entity.uuid.clone(),
                            local_id: local_id.clone(),
                            owned: previous.as_ref().is_none_or(|b| b.owned),
                            remote,
                            local: playlist_local(
                                tree.get(local_id).context("Local playlist disappeared")?,
                            )?,
                            revision,
                            owned_links,
                        },
                    )?;
                }
            }
        }
        for mutation in &plan.tags {
            match mutation {
                NativeTagMutation::Remove {
                    local_id,
                    identity,
                    category,
                } => {
                    ensure!(
                        identity.owned,
                        "Cannot acknowledge deletion of an external MyTag"
                    );
                    ensure!(
                        !published
                            .tags
                            .iter()
                            .any(|t| t.id == identity.native_id && t.deleted == Some(false)),
                        "Owned MyTag deletion was not published"
                    );
                    tx.execute("DELETE FROM native_playlist_bindings WHERE target_id=?1 AND kind=?2 AND native_id=?3 AND local_id=?4",params![plan.target_id,if *category{"category"}else{"tag"},identity.native_id,local_id])?;
                }
                NativeTagMutation::Upsert {
                    local_id,
                    identity: previous,
                    category_local_id,
                    name,
                    position,
                    content_ids,
                    ..
                } => {
                    let kind = if category_local_id.is_none() {
                        "category"
                    } else {
                        "tag"
                    };
                    expected_receipts += 1;
                    let receipt = receipt_map
                        .get(&(kind, local_id.as_str()))
                        .context("Missing MyTag publication receipt")?;
                    let entity = published
                        .tags
                        .iter()
                        .find(|t| t.id == receipt.native_id && t.deleted == Some(false))
                        .context("Published MyTag is missing")?;
                    verify_receipt_identity(
                        plan,
                        previous.as_ref(),
                        "djmdMyTag",
                        &receipt.native_id,
                        entity.uuid.as_ref(),
                    )?;
                    ensure!(
                        &entity.name == name
                            && entity.attribute == if kind == "category" { 1 } else { 0 },
                        "Published MyTag differs from reviewed value"
                    );
                    ensure!(
                        entity.sequence == i64::from(*position) + 1,
                        "Published MyTag order differs from reviewed plan"
                    );
                    let expected_parent = category_local_id
                        .as_ref()
                        .map(|id| {
                            receipt_map
                                .get(&("category", id.as_str()))
                                .map(|r| r.native_id.as_str())
                                .or_else(|| plan.bindings.get(id).map(|b| b.native_id.as_str()))
                                .context("Published MyTag category identity missing")
                        })
                        .transpose()?;
                    ensure!(
                        root_parent(entity.parent_id.as_deref()) == expected_parent,
                        "Published MyTag category differs from reviewed plan"
                    );
                    let remote = tag_remote(published, entity);
                    let actual: BTreeSet<_> = remote["links"]
                        .as_array()
                        .context("Missing MyTag links")?
                        .iter()
                        .filter_map(|l| l["content_id"].as_str())
                        .collect();
                    ensure!(
                        content_ids.iter().all(|c| actual.contains(c.as_str())),
                        "Published MyTag membership differs from reviewed plan"
                    );
                    let owned_links = receipt_links(
                        plan,
                        previous.as_ref(),
                        old.iter().find(|binding| {
                            binding.kind == kind
                                && binding.local_id == *local_id
                                && binding.native_id == receipt.native_id
                        }),
                        receipt,
                        &remote,
                        "djmdSongMyTag",
                        content_ids,
                    )?;
                    save_binding(
                        &tx,
                        &plan.target_id,
                        &Binding {
                            kind: kind.into(),
                            native_id: entity.id.clone(),
                            uuid: entity.uuid.clone(),
                            local_id: local_id.clone(),
                            owned: previous.as_ref().is_none_or(|b| b.owned),
                            remote,
                            local: tag_local(&tx, kind, local_id)?
                                .context("Local MyTag disappeared")?,
                            revision,
                            owned_links,
                        },
                    )?;
                }
            }
        }
        ensure!(
            expected_receipts == receipts.len(),
            "Unexpected native publication receipt"
        );
        // A native revision may advance for unrelated fields; untouched bindings
        // remain valid only if the corresponding entity AND links still match.
        for b in old {
            let touched = plan.playlists.iter().any(|m| match m {
                NativePlaylistMutation::Upsert { local_id, .. }
                | NativePlaylistMutation::Remove { local_id, .. } => local_id == &b.local_id,
            }) || plan.tags.iter().any(|m| match m {
                NativeTagMutation::Upsert { local_id, .. }
                | NativeTagMutation::Remove { local_id, .. } => local_id == &b.local_id,
            });
            if touched {
                continue;
            }
            let current = if b.kind == "playlist" {
                published
                    .playlists
                    .iter()
                    .find(|p| p.id == b.native_id)
                    .map(|p| playlist_remote(published, p))
            } else {
                published
                    .tags
                    .iter()
                    .find(|t| t.id == b.native_id)
                    .map(|t| tag_remote(published, t))
            };
            if current.as_ref() == Some(&b.remote) {
                let mut b = b;
                b.revision = revision;
                save_binding(&tx, &plan.target_id, &b)?;
            }
        }
        tx.execute(
            "INSERT INTO native_playlist_acknowledgements VALUES(?1,?2,?3)",
            params![plan.target_id, plan_hash, receipt_hash],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Capture complete local membership and owned deletion decisions. This
    /// does not reserve native IDs or authorize publication of an unknown row.
    pub fn plan_native_playlists(
        &self,
        target: &str,
        options: NativePlaylistExportOptions,
    ) -> Result<NativePlaylistPlan> {
        self.plan_native_playlists_with_tracks(target, options, &BTreeMap::new())
    }
    pub(crate) fn plan_native_playlists_with_tracks(
        &self,
        target: &str,
        options: NativePlaylistExportOptions,
        reserved_tracks: &BTreeMap<String, String>,
    ) -> Result<NativePlaylistPlan> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        native_revision(&tx, target, options.native_revision)?;
        let saved = bindings(&tx, target)?;
        let mut binding_observations = BindingObservationCache::default();
        let tree = playlists::load(&tx)?;
        let (_, mut tracks) = track_maps(&tx, target)?;
        for (local, native) in reserved_tracks {
            ensure!(
                !tracks.iter().any(
                    |(existing_local, existing_native)| existing_native == native
                        && existing_local != local
                ),
                "Reserved native track identity collides with an existing mapping"
            );
            ensure!(
                tracks.get(local).is_none_or(|existing| existing == native),
                "Reserved native track identity differs from the existing mapping"
            );
            tracks.insert(local.clone(), native.clone());
        }
        let existing_native_ids = tx
            .prepare("SELECT table_name||':'||record_id FROM native_records WHERE target_id=?1")?
            .query_map([target], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        let mut plan = NativePlaylistPlan {
            target_id: target.into(),
            native_revision: options.native_revision,
            playlist_revision: playlists::revision(&tx)?,
            library_revision: tx.query_row(
                "SELECT value FROM meta WHERE key='revision'",
                [],
                |r| r.get(0),
            )?,
            source_fingerprint: source_fingerprint(&tx)?,
            playlists: vec![],
            tags: vec![],
            bindings: BTreeMap::new(),
            existing_native_ids,
        };
        for b in &saved {
            ensure!(
                plan.bindings
                    .insert(b.local_id.clone(), identity(b))
                    .is_none(),
                "Ambiguous local native object identity"
            );
        }
        let selected: BTreeSet<_> = options.playlist_ids.iter().cloned().collect();
        ensure!(
            selected.len() == options.playlist_ids.len(),
            "Duplicate playlist export selection"
        );
        for id in &selected {
            let row = tree.get(id).context("Unknown playlist export selection")?;
            let bound = saved
                .iter()
                .find(|b| b.kind == "playlist" && &b.local_id == id);
            if let Some(b) = bound {
                ensure!(
                    b.revision == options.native_revision
                        || binding_observation_is_current(
                            &tx,
                            target,
                            b,
                            &mut binding_observations
                        )?,
                    "Native playlist changed; import and reconcile the current collection first"
                );
            }
            if row.node.deleted {
                if let Some(b) = bound.filter(|b| b.owned) {
                    ensure!(
                        !b.remote["links"]
                            .as_array()
                            .is_some_and(|links| links.iter().any(|link| link["id"]
                                .as_str()
                                .is_none_or(|id| !b.owned_links.contains(id)))),
                        "External playlist members prevent owned playlist removal"
                    );
                    ensure!(
                        !saved.iter().any(|child| child.kind == "playlist"
                            && !child.owned
                            && child.remote["entity"]["parent_id"] == b.native_id),
                        "External child playlists prevent owned folder removal"
                    );
                    plan.playlists.push(NativePlaylistMutation::Remove {
                        local_id: id.clone(),
                        identity: identity(b),
                    });
                }
                continue;
            }
            if let Some(b) = bound
                && playlist_local(row)? == b.local
            {
                continue;
            }
            if let Some(parent) = &row.node.parent_id {
                ensure!(
                    selected.contains(parent)
                        || saved
                            .iter()
                            .any(|b| b.kind == "playlist" && &b.local_id == parent),
                    "Select or bind the parent playlist before export"
                );
                if !selected.contains(parent) {
                    let parent_binding = saved
                        .iter()
                        .find(|b| b.kind == "playlist" && &b.local_id == parent)
                        .context("Native parent binding is missing")?;
                    ensure!(
                        parent_binding.revision == options.native_revision
                            || binding_observation_is_current(
                                &tx,
                                target,
                                parent_binding,
                                &mut binding_observations
                            )?,
                        "Native parent playlist changed; import and reconcile it first"
                    );
                }
            }
            let representation = match row.node.kind {
                PlaylistKind::Folder => NativePlaylistRepresentation::Folder,
                PlaylistKind::Playlist => {
                    let original = bound.filter(|b| b.remote["entity"]["attribute"] == 4);
                    if let Some(b) = original {
                        if b.local["members"] == serde_json::to_value(&row.members)? {
                            NativePlaylistRepresentation::NativeSmart {
                                opaque_rule: b.remote["entity"]["smart_rule"]
                                    .as_str()
                                    .context("Native smart definition unavailable")?
                                    .into(),
                                content_ids: serde_json::from_value(
                                    b.remote["content_ids"].clone(),
                                )?,
                            }
                        } else {
                            ensure!(
                                options.materialize_smart_playlists.contains(id),
                                "Native smart membership changed; explicitly materialize the complete list"
                            );
                            NativePlaylistRepresentation::MaterializedSmart {
                                content_ids: content_ids(&tx, &tracks, &row.members)?,
                                definition_json: b.remote["entity"]["smart_rule"].clone(),
                            }
                        }
                    } else {
                        NativePlaylistRepresentation::Ordinary {
                            content_ids: content_ids(&tx, &tracks, &row.members)?,
                        }
                    }
                }
                PlaylistKind::Smartlist => {
                    ensure!(
                        options.materialize_smart_playlists.contains(id),
                        "Complete smart semantics require explicit whole-list materialization"
                    );
                    let status:(String,Option<u64>,Option<u64>,Option<u64>)=tx.query_row("SELECT state,input_revision,playlist_revision,preference_revision FROM smart_playlist_status WHERE playlist_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
                    let input: u64 = tx.query_row(
                        "SELECT value FROM meta WHERE key='smart_input_revision'",
                        [],
                        |r| r.get(0),
                    )?;
                    let prefs: u64 = tx.query_row(
                        "SELECT value FROM meta WHERE key='playlist_preferences_revision'",
                        [],
                        |r| r.get(0),
                    )?;
                    ensure!(
                        status.0 == "ready"
                            && status.1 == Some(input)
                            && status.2 == Some(plan.playlist_revision)
                            && status.3 == Some(prefs),
                        "Smart playlist results changed or need refresh before materialization"
                    );
                    let members=tx.prepare("SELECT track_id FROM smart_playlist_members WHERE playlist_id=?1 ORDER BY position")?.query_map([id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                    NativePlaylistRepresentation::MaterializedSmart {
                        content_ids: content_ids(&tx, &tracks, &members)?,
                        definition_json: serde_json::to_value(&row.node.definition)?,
                    }
                }
            };
            plan.playlists.push(NativePlaylistMutation::Upsert {
                local_id: id.clone(),
                identity: bound.map(identity),
                parent_local_id: row.node.parent_id.clone(),
                name: row.node.name.clone(),
                position: row.node.position,
                representation,
                preserve_external_members: true,
            });
        }
        let categories: BTreeSet<_> = options.category_ids.iter().cloned().collect();
        ensure!(
            categories.len() == options.category_ids.len(),
            "Duplicate MyTag category export selection"
        );
        let mut next_category_position = saved
            .iter()
            .filter(|b| b.kind == "category")
            .filter_map(|b| b.remote["entity"]["sequence"].as_u64())
            .max()
            .unwrap_or(0) as u32;
        for category in categories {
            let current = tag_local(&tx, "category", &category)?;
            let bound = saved
                .iter()
                .find(|b| b.kind == "category" && b.local_id == category);
            if let Some(b) = bound {
                ensure!(
                    b.revision == options.native_revision
                        || binding_observation_is_current(
                            &tx,
                            target,
                            b,
                            &mut binding_observations
                        )?,
                    "Native MyTags changed; import and reconcile first"
                );
            }
            if let Some(current) = current {
                if bound.is_none_or(|b| b.local != current) {
                    plan.tags.push(NativeTagMutation::Upsert {
                        local_id: category.clone(),
                        identity: bound.map(identity),
                        category_local_id: None,
                        name: current["name"].as_str().unwrap().into(),
                        position: bound
                            .and_then(|b| b.remote["entity"]["sequence"].as_u64())
                            .map(|p| p.saturating_sub(1) as u32)
                            .unwrap_or_else(|| {
                                let p = next_category_position;
                                next_category_position += 1;
                                p
                            }),
                        content_ids: vec![],
                        preserve_external_links: true,
                    });
                }
                let tag_ids = tx
                    .prepare("SELECT t.id FROM custom_tags t LEFT JOIN curation_value_properties v ON v.tag_id=t.id WHERE t.category_id=?1 AND COALESCE(v.deleted,0)=0 ORDER BY t.id")?
                    .query_map([&category], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut next_tag_position = saved
                    .iter()
                    .filter(|b| b.kind == "tag" && b.local["category_id"] == category)
                    .filter_map(|b| b.remote["entity"]["sequence"].as_u64())
                    .max()
                    .unwrap_or(0) as u32;
                for id in &tag_ids {
                    let current = tag_local(&tx, "tag", id)?.unwrap();
                    let bound = saved.iter().find(|b| b.kind == "tag" && &b.local_id == id);
                    if let Some(b) = bound {
                        ensure!(
                            b.revision == options.native_revision
                                || binding_observation_is_current(
                                    &tx,
                                    target,
                                    b,
                                    &mut binding_observations
                                )?,
                            "Native MyTag changed; import and reconcile first"
                        );
                        if b.local == current {
                            continue;
                        }
                    }
                    let members: Vec<String> =
                        serde_json::from_value(current["track_ids"].clone())?;
                    plan.tags.push(NativeTagMutation::Upsert {
                        local_id: id.clone(),
                        identity: bound.map(identity),
                        category_local_id: Some(category.clone()),
                        name: current["name"].as_str().unwrap().into(),
                        position: bound
                            .and_then(|b| b.remote["entity"]["sequence"].as_u64())
                            .map(|p| p.saturating_sub(1) as u32)
                            .unwrap_or_else(|| {
                                let p = next_tag_position;
                                next_tag_position += 1;
                                p
                            }),
                        content_ids: content_ids(&tx, &tracks, &members)?,
                        preserve_external_links: true,
                    });
                }
                for b in saved
                    .iter()
                    .filter(|b| b.kind == "tag" && b.owned && b.local["category_id"] == category)
                {
                    if tag_local(&tx, "tag", &b.local_id)?.is_none() {
                        plan.tags.push(NativeTagMutation::Remove {
                            local_id: b.local_id.clone(),
                            identity: identity(b),
                            category: false,
                        });
                    }
                }
            } else if let Some(b) = bound.filter(|b| b.owned) {
                // A category cannot remove external child tags by implication.
                ensure!(
                    !saved.iter().any(|child| child.kind == "tag"
                        && !child.owned
                        && child.local["category_id"] == category),
                    "External MyTags prevent owned category removal"
                );
                for child in saved.iter().filter(|child| {
                    child.kind == "tag" && child.owned && child.local["category_id"] == category
                }) {
                    plan.tags.push(NativeTagMutation::Remove {
                        local_id: child.local_id.clone(),
                        identity: identity(child),
                        category: false,
                    });
                }
                plan.tags.push(NativeTagMutation::Remove {
                    local_id: category.clone(),
                    identity: identity(b),
                    category: true,
                });
            }
        }
        tx.commit()?;
        Ok(plan)
    }
    /// Convert the already observed native snapshot into local entities. This
    /// transaction never prepares tracks, changes ready metadata, or publishes.
    pub fn import_native_playlists(
        &self,
        target: &str,
        input: &RekordboxCollection,
        options: NativePlaylistImportOptions,
    ) -> Result<NativePlaylistImportReport> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        native_revision(&tx, target, options.native_revision)?;
        ensure!(
            playlists::revision(&tx)? == options.playlist_revision,
            "Playlists changed; refresh the import"
        );
        let expected: String = tx.query_row(
            "SELECT observation_hash FROM native_targets WHERE id=?1",
            [target],
            |r| r.get(0),
        )?;
        let actual = format!("{:x}", Sha256::digest(serde_json::to_vec(input)?));
        ensure!(
            expected == actual,
            "Typed import differs from the observed native collection"
        );
        let old = bindings(&tx, target)?;
        let mut by_native: BTreeMap<(String, String), Binding> = old
            .iter()
            .cloned()
            .map(|b| ((b.kind.clone(), b.native_id.clone()), b))
            .collect();
        let before = playlists::load(&tx)?;
        let mut after = before.clone();
        let mut report = NativePlaylistImportReport {
            target_id: target.into(),
            changed: false,
            playlist_revision: options.playlist_revision,
            edit_id: None,
            playlists_created: 0,
            categories_created: 0,
            tags_created: 0,
            materialized_native_ids: vec![],
        };
        let (track_map, _) = track_maps(&tx, target)?;
        let mut active_ids = BTreeSet::new();
        let mut playlist_ids = BTreeMap::new();
        for p in input.playlists.iter().filter(|p| p.deleted == Some(false)) {
            ensure!(
                active_ids.insert(p.id.clone()),
                "Duplicate native playlist identity"
            );
            ensure!(
                [0, 1, 4].contains(&p.attribute),
                "Unsupported native playlist kind"
            );
            if p.attribute == 4 {
                ensure!(
                    options.materialize_native_ids.contains(&p.id),
                    "Native smart playlist requires explicit whole-list materialization: {}",
                    p.id
                );
                ensure!(
                    p.smart_rule.is_some(),
                    "Native smart playlist definition is missing"
                );
                report.materialized_native_ids.push(p.id.clone());
            }
            let key = ("playlist".into(), p.id.clone());
            let id = if let Some(b) = by_native.get(&key) {
                ensure!(
                    b.uuid == p.uuid,
                    "Native playlist ID was reused; reconcile its identity first"
                );
                let current = before
                    .get(&b.local_id)
                    .context("Imported local playlist was removed; reconcile it first")?;
                ensure!(
                    playlist_local(current)? == b.local,
                    "Local playlist changed; review both sides before importing"
                );
                b.local_id.clone()
            } else {
                report.playlists_created += 1;
                uuid::Uuid::new_v4().to_string()
            };
            playlist_ids.insert(p.id.clone(), id);
        }
        let mut sorted: Vec<_> = input
            .playlists
            .iter()
            .filter(|p| p.deleted == Some(false))
            .collect();
        sorted.sort_by_key(|p| (p.sequence, &p.id));
        let mut counters: BTreeMap<Option<String>, u32> = BTreeMap::new();
        for p in sorted {
            let id = playlist_ids[&p.id].clone();
            let parent = root_parent(p.parent_id.as_deref())
                .map(|p| {
                    playlist_ids
                        .get(p)
                        .cloned()
                        .context("Native playlist parent is missing")
                })
                .transpose()?;
            let position = counters.entry(parent.clone()).or_insert(0);
            while before.values().any(|r| {
                !r.node.deleted
                    && r.node.parent_id == parent
                    && r.node.position == *position
                    && !playlist_ids.values().any(|id| id == &r.node.id)
            }) {
                *position += 1;
            }
            let remote = playlist_remote(input, p);
            let content: Vec<String> = serde_json::from_value(remote["content_ids"].clone())?;
            let mut seen = BTreeSet::new();
            let members = content
                .iter()
                .filter_map(|id| track_map.get(id))
                .filter(|id| seen.insert((*id).clone()))
                .cloned()
                .collect();
            let node = PlaylistNode {
                id: id.clone(),
                parent_id: parent,
                kind: if p.attribute == 1 {
                    PlaylistKind::Folder
                } else {
                    PlaylistKind::Playlist
                },
                name: p.name.clone(),
                position: *position,
                favorite_position: before.get(&id).and_then(|r| r.node.favorite_position),
                definition: None,
                deleted: false,
                deleted_by: None,
            };
            *position += 1;
            let row = playlists::StoredNode {
                last_edit_id: before
                    .get(&id)
                    .map(|r| r.last_edit_id.clone())
                    .unwrap_or_default(),
                node,
                members,
            };
            let local = playlist_local(&row)?;
            after.insert(id.clone(), row);
            let key = ("playlist".into(), p.id.clone());
            let owned = by_native.get(&key).is_some_and(|b| b.owned);
            let owned_links = by_native
                .get(&key)
                .map(|b| retained_imported_links(b, &remote))
                .unwrap_or_default();
            by_native.insert(
                key,
                Binding {
                    kind: "playlist".into(),
                    native_id: p.id.clone(),
                    uuid: p.uuid.clone(),
                    local_id: id,
                    owned,
                    remote,
                    local,
                    revision: options.native_revision,
                    owned_links,
                },
            );
        }
        // Existing local deletion and absent native rows require an explicit
        // reconciliation decision, rather than implicitly deleting local work.
        let mut category_ids = BTreeMap::new();
        let mut tags_seen = BTreeSet::new();
        for t in input.tags.iter().filter(|t| t.deleted == Some(false)) {
            ensure!(
                tags_seen.insert(t.id.clone()),
                "Duplicate native MyTag identity"
            );
            ensure!(
                [0, 1].contains(&t.attribute),
                "Unsupported native MyTag hierarchy"
            );
            let kind = if t.attribute == 1 { "category" } else { "tag" };
            let key = (kind.into(), t.id.clone());
            if let Some(b) = by_native.get(&key) {
                ensure!(
                    b.uuid == t.uuid,
                    "Native MyTag ID was reused; reconcile its identity first"
                );
                ensure!(
                    tag_local(&tx, kind, &b.local_id)?.as_ref() == Some(&b.local),
                    "Local MyTag changed; review both sides before importing"
                );
            } else if by_native
                .keys()
                .any(|(k, id)| id == &t.id && k != kind && k != "playlist")
            {
                anyhow::bail!("Native MyTag ID changed kind");
            }
            if kind == "category" {
                ensure!(
                    root_parent(t.parent_id.as_deref()).is_none(),
                    "Nested native MyTag categories are unsupported"
                );
                let id = by_native
                    .get(&key)
                    .map(|b| b.local_id.clone())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                if !by_native.contains_key(&key) {
                    report.categories_created += 1;
                }
                ensure!(tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM custom_tag_categories WHERE name=?1 AND id!=?2)",params![t.name,id],|r|r.get::<_,bool>(0))?,"Native MyTag category name collides with an external local category");
                tx.execute("INSERT INTO custom_tag_categories VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name WHERE name!=excluded.name",params![id,t.name])?;
                category_ids.insert(t.id.clone(), id.clone());
                let owned = by_native.get(&key).is_some_and(|b| b.owned);
                let remote = tag_remote(input, t);
                let owned_links = by_native
                    .get(&key)
                    .map(|b| retained_imported_links(b, &remote))
                    .unwrap_or_default();
                by_native.insert(
                    key,
                    Binding {
                        kind: kind.into(),
                        native_id: t.id.clone(),
                        uuid: t.uuid.clone(),
                        local_id: id.clone(),
                        owned,
                        remote,
                        local: tag_local(&tx, kind, &id)?.unwrap(),
                        revision: options.native_revision,
                        owned_links,
                    },
                );
            }
        }
        for t in input
            .tags
            .iter()
            .filter(|t| t.deleted == Some(false) && t.attribute == 0)
        {
            let category = category_ids
                .get(t.parent_id.as_deref().unwrap_or(""))
                .context("Native MyTag category is missing")?;
            let key = ("tag".into(), t.id.clone());
            let previous = by_native.get(&key);
            let id = previous
                .map(|b| b.local_id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let owned = previous.is_some_and(|b| b.owned);
            if previous.is_none() {
                report.tags_created += 1;
            }
            ensure!(tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM custom_tags WHERE category_id=?1 AND name=?2 AND id!=?3)",params![category,t.name,id],|r|r.get::<_,bool>(0))?,"Native MyTag name collides with an external local tag");
            tx.execute("INSERT INTO custom_tags VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET category_id=excluded.category_id,name=excluded.name WHERE category_id!=excluded.category_id OR name!=excluded.name",params![id,category,t.name])?;
            let wanted: BTreeSet<_> = input
                .tag_links
                .iter()
                .filter(|l| l.tag_id == t.id && l.deleted == Some(false))
                .filter_map(|l| track_map.get(&l.content_id))
                .cloned()
                .collect();
            let current: BTreeSet<String> = tx
                .prepare("SELECT track_id FROM track_custom_tags WHERE tag_id=?1")?
                .query_map([&id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for track in current.difference(&wanted) {
                tx.execute(
                    "DELETE FROM track_custom_tags WHERE tag_id=?1 AND track_id=?2",
                    params![id, track],
                )?;
            }
            for track in wanted.difference(&current) {
                tx.execute(
                    "INSERT INTO track_custom_tags VALUES(?1,?2)",
                    params![track, id],
                )?;
            }
            let remote = tag_remote(input, t);
            let owned_links = by_native
                .get(&key)
                .map(|b| retained_imported_links(b, &remote))
                .unwrap_or_default();
            by_native.insert(
                key,
                Binding {
                    kind: "tag".into(),
                    native_id: t.id.clone(),
                    uuid: t.uuid.clone(),
                    local_id: id.clone(),
                    owned,
                    remote,
                    local: tag_local(&tx, "tag", &id)?.unwrap(),
                    revision: options.native_revision,
                    owned_links,
                },
            );
        }
        playlists::tree::validate(&after, &playlists::tree::known_tracks(&tx)?)?;
        let (a, mut b) = playlists::diff(&before, &after);
        if !a.is_empty() {
            let edit = uuid::Uuid::new_v4().to_string();
            for row in b.values_mut().flatten() {
                row.last_edit_id = edit.clone();
            }
            playlists::persist(&tx, &a, &b)?;
            tx.execute("INSERT INTO playlist_edits(id,kind,before_json,after_json,created) VALUES(?1,'edit',?2,?3,?4)",params![edit,serde_json::to_string(&a)?,serde_json::to_string(&b)?,crate::now()])?;
            report.edit_id = Some(edit);
            report.changed = true;
        }
        for b in by_native.values() {
            let old = old
                .iter()
                .find(|old| old.kind == b.kind && old.native_id == b.native_id);
            report.changed |= old.is_none_or(|old| old.local != b.local || old.remote != b.remote);
            save_binding(&tx, target, b)?;
        }
        if report.changed && report.edit_id.is_none() {
            Self::bump(&tx)?;
        }
        report.playlist_revision = playlists::revision(&tx)?;
        tx.commit()?;
        Ok(report)
    }
}
