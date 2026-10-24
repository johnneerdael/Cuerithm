use super::*;
use chrono::TimeZone;
struct PreviewCleanup<'a> {
    library: &'a Library,
    id: String,
    workspace: PathBuf,
    asset_journal: Option<String>,
    committed: bool,
}
impl Drop for PreviewCleanup<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        // Never remove a payload if SQLite may already have registered it.
        let registered = self.library.conn().and_then(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM rekordbox_sync_operations WHERE id=?1)",
                [&self.id],
                |r| r.get::<_, bool>(0),
            )?)
        });
        if !matches!(registered, Ok(false)) {
            return;
        }
        if let Some(id) = &self.asset_journal {
            let _ = FileTransaction::discard(&root(self.library).join("assets"), id);
        }
        let _ = fs::remove_dir_all(&self.workspace);
    }
}
fn scope(scope: &RekordboxSyncScope) -> DjSyncScope {
    match scope {
        RekordboxSyncScope::Playlist { playlist_id } => DjSyncScope::Playlist {
            id: playlist_id.clone(),
        },
        _ => DjSyncScope::Full,
    }
}
fn metadata_value(field: &str, value: &Value) -> Result<TargetValue> {
    if let Some(items) = value.as_array() {
        ensure!(
            items.iter().all(Value::is_string),
            "Native text metadata has invalid values"
        );
        return Ok(TargetValue::Text(
            items
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    if let Some(v) = value.as_str() {
        if matches!(
            field,
            "release_year" | "track_number" | "disc_number" | "rating"
        ) {
            return Ok(TargetValue::Integer(
                v.parse().context("Native numeric metadata is invalid")?,
            ));
        }
        return Ok(TargetValue::Text(v.into()));
    }
    if let Some(v) = value.as_i64() {
        return Ok(TargetValue::Integer(v));
    }
    if let Some(v) = value.as_f64() {
        ensure!(v.is_finite(), "Native metadata is nonfinite");
        return Ok(TargetValue::Real(v));
    }
    anyhow::bail!("Use explicit Clear in rekordbox for a missing local value")
}
pub(super) fn field_choice(
    local: &NativeFieldObservation,
    remote: &NativeFieldObservation,
    result: &NativeReconciliation,
    explicit: Option<NativeFieldDecision>,
) -> Result<Option<NativeFieldDecision>> {
    let decision = explicit.or(match result {
        NativeReconciliation::Proposed {
            decision: NativeFieldDecision::UseLocal,
        } if matches!(local,NativeFieldObservation::Present(value) if value.is_null()) => None,
        NativeReconciliation::Proposed { decision } => Some(*decision),
        _ => None,
    });
    match decision {
        Some(NativeFieldDecision::ClearLocal) => {
            anyhow::bail!("Clearing local music metadata is not supported by rekordbox sync")
        }
        Some(NativeFieldDecision::UseLocal) => ensure!(
            matches!(local,NativeFieldObservation::Present(v) if !v.is_null()),
            "Use explicit Clear in rekordbox instead of sending a null local value"
        ),
        Some(NativeFieldDecision::UseRemote) => ensure!(
            matches!(
                (local, remote),
                (
                    NativeFieldObservation::Present(_),
                    NativeFieldObservation::Present(_)
                )
            ),
            "Both field observations are required for acknowledgement; skip unavailable fields"
        ),
        Some(NativeFieldDecision::ClearRemote) => ensure!(
            matches!(local, NativeFieldObservation::Present(_)),
            "The source field observation is unavailable; skip it until it can be read"
        ),
        Some(NativeFieldDecision::AcceptEqual) => ensure!(
            matches!((local,remote),(NativeFieldObservation::Present(a),NativeFieldObservation::Present(b)) if a==b),
            "Accept equal requires two equal available observations"
        ),
        _ => {}
    }
    Ok(decision)
}
fn owned_cues(
    library: &Library,
    target: &str,
    track: &str,
    remote: Option<&ImportedTrack>,
) -> Result<BTreeSet<String>> {
    let connection = library.conn()?;
    let row:Option<(String,String,String)>=connection.query_row("SELECT identity_json,cue_ids_json,cue_proofs_json FROM native_sync_track_ownership WHERE target_id=?1 AND local_track_id=?2",params![target,track],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((identity, ids, proofs)) = row {
        let identity: TrackIdentity = serde_json::from_str(&identity)?;
        ensure!(
            remote.is_some_and(|r| !crate::native_sync::changed_identity(&identity, &r.identity)),
            "Native cue ownership identity changed"
        );
        let ids: BTreeSet<String> = serde_json::from_str(&ids)?;
        let proofs: BTreeMap<String, CueOwnershipProof> = serde_json::from_str(&proofs)?;
        let remote = remote.expect("validated native cue owner");
        Ok(ids
            .into_iter()
            .filter(|id| {
                let (Some(proof), Some(cue)) =
                    (proofs.get(id), remote.cues.iter().find(|cue| &cue.id == id))
                else {
                    return false;
                };
                cue.content_uuid == proof.content_uuid
                    && if proof.uuid.is_some() {
                        cue.uuid == proof.uuid
                    } else {
                        cue.uuid.is_none() && cue.raw_hash == proof.raw_hash
                    }
            })
            .collect())
    } else {
        Ok(BTreeSet::new())
    }
}
fn previous_native_media(
    library: &Library,
    target: &str,
    track: &SyncTrack,
    remote: &ImportedTrack,
) -> Result<Option<cuerithm_dj_sync::NativeSourceDescriptor>> {
    let connection = library.conn()?;
    let row:Option<(String,Option<String>)>=connection.query_row("SELECT identity_json,source_json FROM native_sync_track_ownership WHERE target_id=?1 AND local_track_id=?2",params![target,track.id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
    let Some((identity, Some(source))) = row else {
        return Ok(None);
    };
    let identity: TrackIdentity = serde_json::from_str(&identity)?;
    let source: cuerithm_dj_sync::NativeSourceDescriptor = serde_json::from_str(&source)?;
    Ok(
        (!crate::native_sync::changed_identity(&identity, &remote.identity)
            && remote.source.resolved_path.as_deref().is_some_and(|path| {
                Path::new(path).canonicalize().ok().is_some_and(|physical| {
                    source.native_path.canonicalize().ok() == Some(physical)
                })
            }))
        .then_some(source),
    )
}
fn owned_audio_relationship(
    library: &Library,
    target: &str,
    track: &SyncTrack,
    remote: &ImportedTrack,
) -> Result<bool> {
    let Some(source) = previous_native_media(library, target, track, remote)? else {
        return Ok(false);
    };
    let hash: String = library.conn()?.query_row(
        "SELECT source_hash FROM tracks WHERE id=?1",
        [&track.id],
        |row| row.get(0),
    )?;
    Ok(source.converted && source.original.sha256 == hash && source.original.path == track.path)
}
fn native_media_offset(source: &cuerithm_dj_sync::NativeSourceDescriptor) -> Result<f64> {
    cuerithm_dj_sync::timing::offset_seconds(
        &SyncTrack {
            native_path: None,
            path: source.native_path.clone(),
            sample_rate: source.sample_rate,
            export_timing: Some(source.export_timing.clone()),
            ..Default::default()
        },
        cuerithm_dj_sync::DjSoftware::Rekordbox,
    )
}
fn native_identity(
    identity: &NativePlaylistIdentity,
    table: &str,
    link_table: &str,
    collection: &RekordboxCollection,
) -> Result<NativeEntityIdentity> {
    let links = collection
        .records
        .get(link_table)
        .into_iter()
        .flatten()
        .filter(|r| identity.owned_link_ids.contains(&r.id))
        .map(|r| NativeOwnedRow {
            id: r.id.clone(),
            expected_raw_hash: r.fingerprint.clone(),
        })
        .collect();
    ensure!(
        collection
            .records
            .get(table)
            .into_iter()
            .flatten()
            .any(|r| r.id == identity.native_id && r.fingerprint == identity.expected_raw_hash),
        "Native playlist identity proof changed"
    );
    Ok(NativeEntityIdentity {
        native_id: identity.native_id.clone(),
        uuid: identity.uuid.clone(),
        expected_raw_hash: identity.expected_raw_hash.clone(),
        owned: identity.owned,
        owned_links: links,
    })
}
fn entity_ref(id: &str, plan: &NativePlaylistPlan) -> NativeEntityRef {
    if let Some(binding) = plan.bindings.get(id) {
        NativeEntityRef::Native(binding.native_id.clone())
    } else {
        NativeEntityRef::Local(id.into())
    }
}
fn entities(
    plan: &NativePlaylistPlan,
    collection: &RekordboxCollection,
) -> Result<Vec<NativeEntityDraft>> {
    let mut result = vec![];
    for item in &plan.playlists {
        let draft = match item {
            NativePlaylistMutation::Upsert {
                local_id,
                identity,
                parent_local_id,
                name,
                position,
                representation,
                ..
            } => {
                let (attribute, smart_rule, members) = match representation {
                    NativePlaylistRepresentation::Folder => (1, None, vec![]),
                    NativePlaylistRepresentation::Ordinary { content_ids }
                    | NativePlaylistRepresentation::MaterializedSmart { content_ids, .. } => (
                        0,
                        None,
                        content_ids
                            .iter()
                            .map(|id| NativeEntityRef::Native(id.clone()))
                            .collect(),
                    ),
                    NativePlaylistRepresentation::NativeSmart {
                        opaque_rule,
                        content_ids,
                    } => (
                        4,
                        Some(opaque_rule.clone()),
                        content_ids
                            .iter()
                            .map(|id| NativeEntityRef::Native(id.clone()))
                            .collect(),
                    ),
                };
                NativeEntityDraft {
                    kind: NativeEntityKind::Playlist,
                    local_id: local_id.clone(),
                    identity: identity
                        .as_ref()
                        .map(|i| native_identity(i, "djmdPlaylist", "djmdSongPlaylist", collection))
                        .transpose()?,
                    parent: parent_local_id.as_deref().map(|id| entity_ref(id, plan)),
                    name: name.clone(),
                    attribute,
                    sequence: i64::from(*position) + 1,
                    smart_rule,
                    members,
                    remove: false,
                }
            }
            NativePlaylistMutation::Remove { local_id, identity } => NativeEntityDraft {
                kind: NativeEntityKind::Playlist,
                local_id: local_id.clone(),
                identity: Some(native_identity(
                    identity,
                    "djmdPlaylist",
                    "djmdSongPlaylist",
                    collection,
                )?),
                parent: None,
                name: String::new(),
                attribute: 0,
                sequence: 1,
                smart_rule: None,
                members: vec![],
                remove: true,
            },
        };
        result.push(draft);
    }
    for item in &plan.tags {
        let draft = match item {
            NativeTagMutation::Upsert {
                local_id,
                identity,
                category_local_id,
                name,
                position,
                content_ids,
                ..
            } => NativeEntityDraft {
                kind: NativeEntityKind::MyTag,
                local_id: local_id.clone(),
                identity: identity
                    .as_ref()
                    .map(|i| native_identity(i, "djmdMyTag", "djmdSongMyTag", collection))
                    .transpose()?,
                parent: category_local_id.as_deref().map(|id| entity_ref(id, plan)),
                name: name.clone(),
                attribute: i64::from(category_local_id.is_none()),
                sequence: i64::from(*position) + 1,
                smart_rule: None,
                members: content_ids
                    .iter()
                    .map(|id| NativeEntityRef::Native(id.clone()))
                    .collect(),
                remove: false,
            },
            NativeTagMutation::Remove {
                local_id,
                identity,
                category,
            } => NativeEntityDraft {
                kind: NativeEntityKind::MyTag,
                local_id: local_id.clone(),
                identity: Some(native_identity(
                    identity,
                    "djmdMyTag",
                    "djmdSongMyTag",
                    collection,
                )?),
                parent: None,
                name: String::new(),
                attribute: i64::from(*category),
                sequence: 1,
                smart_rule: None,
                members: vec![],
                remove: true,
            },
        };
        result.push(draft);
    }
    Ok(result)
}
// Keep review payloads bounded without truncating the frozen mutation plan.
const ENTITY_REVIEW_LIMIT: usize = 1_000;
const MEMBER_REVIEW_LIMIT: usize = 50;
fn review_name(value: &str) -> String {
    value.chars().take(240).collect()
}
fn pending_entities(
    library: &Library,
    target: &str,
    local: &cuerithm_dj_sync::SyncLibrary,
    options: &RekordboxSyncOptions,
    reserved: &BTreeMap<String, String>,
) -> Result<Vec<RekordboxPendingEntity>> {
    let connection = library.conn()?;
    let mut mapped = connection.prepare("SELECT local_track_id FROM native_track_links WHERE target_id=?1 AND status='linked' AND observation_hash IS NOT NULL AND local_track_id IS NOT NULL GROUP BY local_track_id HAVING COUNT(*)=1")?
        .query_map([target], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    mapped.extend(reserved.keys().cloned());
    let mut result = vec![];
    let title = |id: &str| -> Result<String> {
        let title: Option<String> = connection
            .query_row("SELECT title FROM tracks WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(review_name(
            title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("Untitled track"),
        ))
    };
    if options.playlists {
        for playlist in &local.playlists {
            let missing: Vec<_> = playlist
                .track_ids
                .iter()
                .filter(|id| !mapped.contains(*id))
                .collect();
            if !missing.is_empty() {
                result.push(RekordboxPendingEntity {
                    kind: RekordboxEntityKind::Playlist,
                    name: review_name(&playlist.name),
                    unresolved_members: missing.len(),
                    member_titles: missing
                        .iter()
                        .take(MEMBER_REVIEW_LIMIT)
                        .map(|id| title(id))
                        .collect::<Result<_>>()?,
                });
            }
        }
    }
    if options.my_tags {
        let tags = connection
            .prepare("SELECT t.id,t.name FROM custom_tags t LEFT JOIN curation_value_properties v ON v.tag_id=t.id LEFT JOIN curation_group_properties g ON g.category_id=t.category_id WHERE COALESCE(v.deleted,0)=0 AND COALESCE(g.deleted,0)=0 ORDER BY t.category_id,t.id")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, name) in tags {
            let missing = connection
                .prepare(
                    "SELECT track_id FROM track_custom_tags WHERE tag_id=?1 ORDER BY track_id",
                )?
                .query_map([id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter(|id| !mapped.contains(id))
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                result.push(RekordboxPendingEntity {
                    kind: RekordboxEntityKind::Tag,
                    name: review_name(&name),
                    unresolved_members: missing.len(),
                    member_titles: missing
                        .iter()
                        .take(MEMBER_REVIEW_LIMIT)
                        .map(|id| title(id))
                        .collect::<Result<_>>()?,
                });
            }
        }
    }
    ensure!(
        result.len() <= ENTITY_REVIEW_LIMIT,
        "Too many pending playlist or My Tag changes; select a smaller sync scope"
    );
    Ok(result)
}
fn entity_state(
    collection: &RekordboxCollection,
    kind: NativeEntityKind,
    id: &str,
    owned_links: &BTreeSet<String>,
) -> Result<Option<RekordboxEntityState>> {
    let entities: BTreeMap<_, _> = match kind {
        NativeEntityKind::Playlist => collection
            .playlists
            .iter()
            .filter(|p| p.deleted != Some(true))
            .map(|p| {
                (
                    p.id.as_str(),
                    (p.name.as_str(), p.parent_id.as_deref(), p.sequence),
                )
            })
            .collect(),
        NativeEntityKind::MyTag => collection
            .tags
            .iter()
            .filter(|p| p.deleted != Some(true))
            .map(|p| {
                (
                    p.id.as_str(),
                    (p.name.as_str(), p.parent_id.as_deref(), p.sequence),
                )
            })
            .collect(),
    };
    let Some((name, parent, position)) = entities.get(id) else {
        return Ok(None);
    };
    let mut parent_path = vec![];
    let mut parent = *parent;
    let mut seen = BTreeSet::new();
    while let Some(id) = parent.filter(|id| !matches!(*id, "root" | "0" | "")) {
        ensure!(
            seen.insert(id),
            "Native entity review hierarchy contains a cycle"
        );
        let (name, next, _) = entities
            .get(id)
            .context("Native entity review parent is missing")?;
        ensure!(
            parent_path.len() < 32,
            "Native entity review hierarchy exceeds its limit"
        );
        parent_path.push(review_name(name));
        parent = *next;
    }
    parent_path.reverse();
    let mut members: Vec<_> = match kind {
        NativeEntityKind::Playlist => collection
            .playlist_links
            .iter()
            .filter(|link| link.playlist_id == id && link.deleted != Some(true))
            .map(|link| {
                (
                    link.content_id.as_str(),
                    Some(link.position),
                    !owned_links.contains(&link.id),
                )
            })
            .collect(),
        NativeEntityKind::MyTag => collection
            .tag_links
            .iter()
            .filter(|link| link.tag_id == id && link.deleted != Some(true))
            .map(|link| {
                (
                    link.content_id.as_str(),
                    None,
                    !owned_links.contains(&link.id),
                )
            })
            .collect(),
    };
    members.sort_by(|a, b| (a.1, a.0).cmp(&(b.1, b.0)));
    let member_count = members.len();
    let external_member_count = members.iter().filter(|(_, _, external)| *external).count();
    let titles: BTreeMap<_, _> = collection
        .tracks
        .iter()
        .map(|track| {
            (
                track.identity.content_id.as_str(),
                track.metadata.title.as_str(),
            )
        })
        .collect();
    Ok(Some(RekordboxEntityState {
        name: review_name(name),
        parent_path,
        position: *position,
        member_count,
        external_member_count,
        representation: match kind {
            NativeEntityKind::Playlist => {
                let playlist = collection
                    .playlists
                    .iter()
                    .find(|playlist| playlist.id == id)
                    .expect("reviewed playlist");
                if playlist.attribute == 1 {
                    RekordboxEntityRepresentation::Folder
                } else if playlist.smart_rule.is_some() || playlist.attribute == 4 {
                    RekordboxEntityRepresentation::NativeSmart
                } else {
                    RekordboxEntityRepresentation::Ordinary
                }
            }
            NativeEntityKind::MyTag => {
                if collection
                    .tags
                    .iter()
                    .any(|tag| tag.id == id && tag.attribute == 1)
                {
                    RekordboxEntityRepresentation::Category
                } else {
                    RekordboxEntityRepresentation::Tag
                }
            }
        },
        members: members
            .into_iter()
            .take(MEMBER_REVIEW_LIMIT)
            .map(|(id, position, external)| RekordboxEntityMember {
                title: review_name(
                    titles
                        .get(id)
                        .copied()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Untitled track"),
                ),
                position,
                external,
            })
            .collect(),
    }))
}
pub(super) fn entity_changes(
    plan: Option<&NativePlaylistPlan>,
    drafts: &[NativeEntityDraft],
    resolved: &[NativeResolvedEntity],
    before: &RekordboxCollection,
    after: &RekordboxCollection,
) -> Result<Vec<RekordboxEntityChange>> {
    ensure!(
        drafts.len() <= ENTITY_REVIEW_LIMIT,
        "Too many playlist or My Tag changes to review; select a smaller sync scope"
    );
    let mut changes = vec![];
    for draft in drafts {
        let resolved = resolved
            .iter()
            .find(|entity| entity.local_id == draft.local_id)
            .context("Reviewed native entity identity missing")?;
        let mut owned_links: BTreeSet<String> = draft
            .identity
            .iter()
            .flat_map(|identity| identity.owned_links.iter().map(|link| link.id.clone()))
            .collect();
        let previous = entity_state(before, draft.kind, &resolved.native_id, &owned_links)?;
        owned_links.extend(resolved.owned_link_ids.iter().cloned());
        let next = entity_state(after, draft.kind, &resolved.native_id, &owned_links)?;
        let folder = draft.attribute == 1
            || (draft.remove
                && before
                    .playlists
                    .iter()
                    .any(|playlist| playlist.id == resolved.native_id && playlist.attribute == 1));
        let kind = match draft.kind {
            NativeEntityKind::Playlist if folder => RekordboxEntityKind::Folder,
            NativeEntityKind::Playlist => RekordboxEntityKind::Playlist,
            NativeEntityKind::MyTag if draft.attribute == 1 => RekordboxEntityKind::Category,
            NativeEntityKind::MyTag => RekordboxEntityKind::Tag,
        };
        changes.push(RekordboxEntityChange {
            local_id: draft.local_id.clone(), kind,
            action: if draft.remove { RekordboxEntityAction::Remove } else if draft.identity.is_none() { RekordboxEntityAction::Create } else { RekordboxEntityAction::Update },
            ownership: match &draft.identity { None => RekordboxEntityOwnership::New, Some(identity) if identity.owned => RekordboxEntityOwnership::Owned, Some(_) => RekordboxEntityOwnership::Matched },
            materialized_smart: plan.is_some_and(|plan| plan.playlists.iter().any(|mutation| matches!(mutation, NativePlaylistMutation::Upsert { local_id, representation: NativePlaylistRepresentation::MaterializedSmart { .. }, .. } if local_id == &draft.local_id))),
            before: previous, after: next,
        });
    }
    Ok(changes)
}
pub(super) enum PreviewPermit {
    Production,
    #[cfg(test)]
    AuthoredFixture,
}
#[allow(clippy::too_many_arguments)]
pub(super) fn preview(
    library: &Library,
    request: RekordboxSyncRequest,
    target: RekordboxSyncTarget,
    key: &DatabaseKey,
    runtime: Option<&cuerithm_media::MediaRuntime>,
    cancel: &CancellationToken,
    permit: PreviewPermit,
) -> Result<RekordboxSyncPreview> {
    cancel.check()?;
    let environment_snapshot = target.environment.freeze()?;
    let target_roots = capture_target_roots(&target)?;
    let (configuration, configuration_target) = selected_configuration(library, &request, &target)?;
    let installation_hash = file_sha256_cancellable(&target.installation.executable, cancel)?;
    ensure!(
        request
            .options
            .metadata_fields
            .iter()
            .all(|f| f == "source_path"
                || RekordboxSyncOptions::default().metadata_fields.contains(f)),
        "Unsupported selected native metadata field"
    );
    let id = uuid::Uuid::new_v4().to_string();
    let workspace = root(library).join("operations").join(&id);
    fs::create_dir_all(&workspace)?;
    let mut cleanup = PreviewCleanup {
        library,
        id: id.clone(),
        workspace: workspace.clone(),
        asset_journal: None,
        committed: false,
    };
    let (mut collection, before, snapshot) = {
        let lease = NativeDatabaseLease::acquire(&target.database, key, cancel)?;
        let before = lease.state(cancel)?;
        let collection = lease.read_collection(CollectionReadOptions::default(), cancel)?;
        let snapshot = lease.snapshot(&workspace, key, cancel)?;
        (collection, before, snapshot)
    };
    // Observe mapped references before the local observation transaction can
    // create/remove SQLite journals in a mapped ancestor shared by test setups.
    {
        let paths = target.environment.resolver()?;
        project_collection(&paths, &mut collection)?;
        paths.verify()?;
        environment_snapshot.verify()?;
        for root in &target_roots {
            root.verify()?;
        }
    }
    let mut warnings = vec![];
    let capability = require_capability(&target, &snapshot.schema, cancel);
    let fixture_permit = match permit {
        PreviewPermit::Production => false,
        #[cfg(test)]
        PreviewPermit::AuthoredFixture => true,
    };
    let writable = capability.is_ok() || fixture_permit;
    if let Err(e) = capability
        && !fixture_permit
    {
        warnings.push(e.to_string());
    }
    for track in &collection.tracks {
        if let Some(path) = &track.source.resolved_path {
            ensure_not_native_file(&target.database, Path::new(path))?;
        }
    }
    let observed = library.observe_rekordbox_collection(
        &collection,
        NativeImportOptions {
            settings_revision: library.settings()?.revision,
            content_ids: None,
            initial_owners: BTreeMap::new(),
        },
        cancel,
    )?;
    let native_target_id = observed.target_id;
    let revision = library.revision()?;
    let settings_revision = library.settings()?.revision;
    let mut local_library = library.native_sync_library(scope(&request.scope), runtime)?;
    // All following path loops share one read-only directory-index phase.
    // Journal writes above have completed; cached evidence is never refreshed.
    let paths = target.environment.resolver()?;
    let mut sources = vec![];
    let mut reviews = vec![];
    let mut acknowledgements = vec![];
    let mut drafts = vec![];
    let mut asset_sources = vec![];
    let mut publications = vec![];
    let mut prepared_audio = vec![];
    let mut media_copies = 0;
    let mut copied_audio_bytes = 0u64;
    let mut seen_decisions = BTreeSet::new();
    let mut pending_creation_choice = false;
    let mut projected_sources = BTreeMap::new();
    for track in &mut local_library.tracks {
        cancel.check()?;
        let (local, epoch, link) = {
            let connection = library.conn()?;
            let fields = crate::native_sync::local_fields(&connection, &track.id)?;
            let epoch = connection.query_row(
                "SELECT epoch FROM maintenance_track_epochs WHERE id=?1",
                [&track.id],
                |r| r.get(0),
            )?;
            let links=connection.prepare("SELECT content_id FROM native_track_links WHERE target_id=?1 AND local_track_id=?2")?.query_map(params![native_target_id,track.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ensure!(
                links.len() <= 1,
                "Multiple native identities match this local track"
            );
            drop(connection);
            (
                fields,
                epoch,
                links
                    .first()
                    .map(|id| library.native_track_link(&native_target_id, id))
                    .transpose()?,
            )
        };
        if let Some(link) = &link {
            ensure!(
                link.status == "linked",
                "Native track identity changed or needs reconciliation"
            );
        }
        let remote = link.as_ref().and_then(|l| {
            collection
                .tracks
                .iter()
                .find(|r| r.identity.content_id == l.content_id)
        });
        if let (Some(link), Some(remote)) = (&link, remote) {
            ensure!(
                !crate::native_sync::changed_identity(&link.baseline_identity, &remote.identity),
                "Native content ID was reused"
            );
        }
        let remote_fields = remote
            .map(crate::native_sync::remote_fields)
            .transpose()?
            .unwrap_or_default();
        let prior_owned_cues = owned_cues(library, &native_target_id, &track.id, remote)?;
        let mut selected = request.options.metadata_fields.clone();
        let source_differs = remote
            .and_then(|r| r.source.path.as_deref())
            .is_some_and(|path| {
                paths
                    .to_host(Path::new(path))
                    .ok()
                    .and_then(|p| p.canonicalize().ok())
                    != track.path.canonicalize().ok()
            })
            && !remote
                .map(|r| owned_audio_relationship(library, &native_target_id, track, r))
                .transpose()?
                .unwrap_or(false);
        if source_differs {
            selected.insert("source_path".into());
        }
        if request.options.cues {
            selected.insert("cues".into());
        }
        if request.options.beat_grid {
            selected.insert("beat_grid".into());
        }
        if request.options.artwork {
            selected.insert("artwork".into());
        }
        let mut chosen = BTreeMap::new();
        let mut relevant = false;
        for field in selected {
            let local_value = match field.as_str() {
                "cues" => Some(assets::cue_observation(library, &track.id)?),
                "beat_grid" => Some(serde_json::to_value(&track.beatgrid)?),
                "artwork" => runtime
                    .map(|r| {
                        cuerithm_dj_sync::native_rekordbox_artwork(&track.path, r, cancel)
                            .map(|art| art.map_or(Value::Null, |art| json!(art.observation_sha256)))
                    })
                    .transpose()?,
                _ => local.get(&field).cloned(),
            };
            let remote_value = match field.as_str() {
                "beat_grid" => remote
                    .and_then(|r| r.source.analysis_path.as_deref())
                    .filter(|v| !v.is_empty())
                    .map(|path| assets::grid_observation_resolved(&target, &paths, path))
                    .transpose()?,
                "artwork" => remote
                    .map(|r| {
                        r.source
                            .artwork_path
                            .as_deref()
                            .filter(|v| !v.is_empty())
                            .map(|path| {
                                assets::artwork_observation_resolved(&target, &paths, path, cancel)
                            })
                            .transpose()
                            .map(|value| value.unwrap_or(Value::Null))
                    })
                    .transpose()?,
                _ => remote_fields.get(&field).cloned(),
            };
            let local_observation = local_value.clone().map_or(
                NativeFieldObservation::Unavailable,
                NativeFieldObservation::Present,
            );
            let remote_observation = remote_value.clone().map_or(
                NativeFieldObservation::Unavailable,
                NativeFieldObservation::Present,
            );
            let baseline = link
                .as_ref()
                .and_then(|l| l.fields.get(&field))
                .cloned()
                .unwrap_or(NativeFieldBaseline {
                    local: local_value.unwrap_or(Value::Null),
                    remote: remote_value.unwrap_or(Value::Null),
                    owner: NativeFieldOwner::Unowned,
                    acknowledged: false,
                });
            // A new track has no remote values to reconcile or overwrite.
            // Export the selected data and leave absent values to native defaults.
            let mut result = if remote.is_none() {
                match &local_observation {
                    NativeFieldObservation::Present(value)
                        if !value.is_null()
                            && value.as_str() != Some("")
                            && !value.as_array().is_some_and(Vec::is_empty) =>
                    {
                        NativeReconciliation::Proposed {
                            decision: NativeFieldDecision::UseLocal,
                        }
                    }
                    _ => NativeReconciliation::Unchanged,
                }
            } else {
                reconcile_field(&baseline, &local_observation, &remote_observation)
            };
            if field == "cues"
                && matches!(
                    result,
                    NativeReconciliation::Proposed {
                        decision: NativeFieldDecision::UseLocal
                    }
                )
                && remote.is_some_and(|remote| {
                    remote
                        .cues
                        .iter()
                        .any(|cue| !prior_owned_cues.contains(&cue.id))
                })
            {
                result = NativeReconciliation::Conflict {
                    reason: NativeConflict::Ownership,
                    suggestion: Some(NativeFieldDecision::UseLocal),
                };
            }
            let explicit = request
                .decisions
                .get(&track.id)
                .and_then(|d| d.get(&field))
                .copied();
            if explicit.is_some() {
                seen_decisions.insert((track.id.clone(), field.clone()));
            }
            let decision =
                field_choice(&local_observation, &remote_observation, &result, explicit)?;
            relevant |= !matches!(result, NativeReconciliation::Unchanged);
            // Accept the native defaults for omitted new-track fields only
            // after creation commits. Otherwise the next sync would ask about
            // those same empty fields. This acknowledges without writing them.
            let acknowledgement = decision
                .filter(|d| *d != NativeFieldDecision::Skip)
                .or_else(|| {
                    (remote.is_none()
                        && explicit.is_none()
                        && matches!(result, NativeReconciliation::Unchanged))
                    .then_some(NativeFieldDecision::UseRemote)
                });
            if let Some(decision) = acknowledgement
                && let NativeFieldObservation::Present(value) = &local_observation
            {
                acknowledgements.push(FieldAcknowledgement {
                    local_id: track.id.clone(),
                    content_id: remote
                        .map(|r| r.identity.content_id.clone())
                        .unwrap_or_default(),
                    field: field.clone(),
                    local: value.clone(),
                    decision,
                });
            }
            if let Some(decision) = decision.filter(|d| *d != NativeFieldDecision::Skip) {
                chosen.insert(field.clone(), decision);
            }
            reviews.push(RekordboxSyncFieldReview {
                track_id: track.id.clone(),
                title: track.title.clone(),
                new_track: remote.is_none(),
                field,
                local: local_observation,
                remote: remote_observation,
                result,
                decision,
            });
        }
        if remote.is_some()
            && matches!(request.scope, RekordboxSyncScope::Changed)
            && !relevant
            && chosen.is_empty()
        {
            continue;
        }
        ensure_not_native_file(&target.database, &track.path)?;
        let source_hash = file_sha256_cancellable(&track.path, cancel)?;
        let prepared_hash: String = library.conn()?.query_row(
            "SELECT source_hash FROM tracks WHERE id=?1",
            [&track.id],
            |r| r.get(0),
        )?;
        ensure!(
            source_hash == prepared_hash,
            "Audio source changed after preparation; prepare the track again before native sync"
        );
        sources.push(SourceProof {
            id: track.id.clone(),
            path: track.path.clone(),
            hash: source_hash,
            epoch,
            local_fields: local,
            native_media: None,
        });
        let write_cues = matches!(
            chosen.get("cues"),
            Some(NativeFieldDecision::UseLocal | NativeFieldDecision::ClearRemote)
        );
        let write_grid = matches!(
            chosen.get("beat_grid"),
            Some(NativeFieldDecision::UseLocal | NativeFieldDecision::ClearRemote)
        );
        ensure!(
            !matches!(
                chosen.get("beat_grid"),
                Some(NativeFieldDecision::ClearRemote)
            ),
            "Clearing the complete native beatgrid is unsupported; retain or export the saved grid"
        );
        let write_artwork = matches!(chosen.get("artwork"), Some(NativeFieldDecision::UseLocal));
        let clear_artwork = matches!(
            chosen.get("artwork"),
            Some(NativeFieldDecision::ClearRemote)
        );
        let mut fields = BTreeMap::new();
        let mut mapped_fields = request.options.metadata_fields.clone();
        if chosen.contains_key("source_path") {
            mapped_fields.insert("source_path".into());
        }
        for field in &mapped_fields {
            match chosen.get(field) {
                Some(NativeFieldDecision::UseLocal) => {
                    fields.insert(
                        field.clone(),
                        metadata_value(field, &sources.last().unwrap().local_fields[field])?,
                    );
                }
                Some(NativeFieldDecision::ClearRemote) => {
                    fields.insert(field.clone(), TargetValue::Null);
                }
                _ => {}
            }
        }
        ensure!(
            !matches!(
                chosen.get("source_path"),
                Some(NativeFieldDecision::ClearRemote)
            ),
            "Clearing a native audio location is unsupported; retain or explicitly relink it"
        );
        let relocate_audio = matches!(
            chosen.get("source_path"),
            Some(NativeFieldDecision::UseLocal)
        ) && source_differs;
        ensure!(
            !source_differs || !(write_cues || write_grid) || relocate_audio,
            "Native audio location differs; explicitly use the CueRithm source location before exporting timing"
        );
        if remote.is_none() && !matches!(fields.get("title"), Some(TargetValue::Text(_))) {
            pending_creation_choice = true;
            continue;
        }
        let mut regenerate_waveforms = false;
        let mut export_track = track.clone();
        // An existing compatible copy remains the destination for metadata-only
        // edits; the original source can be outside the Windows namespace.
        let previous_copy = if relocate_audio {
            None
        } else {
            remote
                .map(|remote| previous_native_media(library, &native_target_id, track, remote))
                .transpose()?
                .flatten()
                .filter(|media| {
                    media.converted
                        && media.original.path == track.path
                        && media.original.sha256 == sources.last().unwrap().hash
                })
        };
        let mut native_audio_path = previous_copy
            .map(|media| media.native_path)
            .unwrap_or_else(|| track.path.clone());
        if remote.is_none() || write_cues || write_grid || relocate_audio {
            let runtime =
                runtime.context("Native audio compatibility requires the bundled media runtime")?;
            let audio = cuerithm_dj_sync::prepare_native_rekordbox_source(
                &track.path,
                &workspace.join("audio"),
                request.options.convert_incompatible_audio,
                runtime,
                cancel,
            )?;
            let descriptor = &audio.descriptor;
            ensure!(
                descriptor.original.sha256 == sources.last().unwrap().hash,
                "Native audio compatibility proof belongs to changed source bytes"
            );
            if let Some(previous) = remote
                .map(|remote| previous_native_media(library, &native_target_id, track, remote))
                .transpose()?
                .flatten()
            {
                regenerate_waveforms = previous.pcm_sha256 != descriptor.pcm_sha256
                    || match (
                        native_media_offset(&previous),
                        native_media_offset(descriptor),
                    ) {
                        (Ok(previous), Ok(current)) => (previous - current).abs() > 1e-12,
                        _ => true,
                    };
                if regenerate_waveforms {
                    warnings.push(format!(
                        "Rebuild rekordbox waveforms for updated audio in '{}'",
                        track.title
                    ));
                }
            }
            export_track.path = descriptor.native_path.clone();
            export_track.export_timing = Some(descriptor.export_timing.clone());
            export_track.sample_rate = descriptor.sample_rate;
            export_track.duration_seconds = descriptor.duration_seconds;
            if descriptor.converted {
                let previous = if remote
                    .map(|remote| {
                        owned_audio_relationship(library, &native_target_id, track, remote)
                    })
                    .transpose()?
                    .unwrap_or(false)
                {
                    let json:Option<String>=library.conn()?.query_row("SELECT source_json FROM native_sync_track_ownership WHERE target_id=?1 AND local_track_id=?2",params![native_target_id,track.id],|r|r.get(0))?;
                    json.map(|json| {
                        serde_json::from_str::<cuerithm_dj_sync::NativeSourceDescriptor>(&json)
                    })
                    .transpose()?
                } else {
                    None
                };
                if let Some(previous) = &previous {
                    ensure_not_native_file(&target.database, &previous.native_path)?;
                    ensure!(
                        file_sha256_cancellable(&previous.native_path, cancel)? == previous.sha256,
                        "Owned compatible audio changed outside CueRithm; reconcile the native source before exporting timing"
                    );
                    asset_sources.push(AssetSource {
                        path: previous.native_path.clone(),
                        hash: previous.sha256.clone(),
                    });
                }
                if let Some(previous) =
                    previous.filter(|previous| previous.sha256 == descriptor.sha256)
                {
                    native_audio_path = previous.native_path;
                } else {
                    native_audio_path = target.asset_root.join(format!(
                        "Contents/CUERITHM/{id}/{}.wav",
                        &digest(&track.id)?[..24]
                    ));
                    publications.push(FilePublication {
                        target: native_audio_path.clone(),
                        before_sha256: None,
                        staged: Some(descriptor.native_path.clone()),
                    });
                    media_copies += 1;
                    copied_audio_bytes = copied_audio_bytes
                        .checked_add(descriptor.size_bytes)
                        .context("Native audio copy size overflow")?;
                }
                fields.insert(
                    "source_path".into(),
                    TargetValue::Text(
                        native_audio_path
                            .to_str()
                            .context("Native audio path is not Unicode")?
                            .into(),
                    ),
                );
            }
            // These are measured properties of the chosen native audio file,
            // independent of optional user tag mappings.
            {
                fields.insert(
                    "file_type".into(),
                    TargetValue::Integer(descriptor.native_file_type),
                );
                fields.insert(
                    "duration_seconds".into(),
                    TargetValue::Integer(descriptor.duration_seconds.ceil() as i64),
                );
                fields.insert(
                    "file_size".into(),
                    TargetValue::Integer(
                        i64::try_from(descriptor.size_bytes)
                            .context("Audio source is too large")?,
                    ),
                );
                fields.insert(
                    "sample_rate".into(),
                    TargetValue::Integer(i64::from(descriptor.sample_rate)),
                );
                fields.insert(
                    "bit_depth".into(),
                    TargetValue::Integer(i64::from(descriptor.bits_per_sample)),
                );
                if let Some(bitrate) = descriptor.bitrate_kbps {
                    fields.insert(
                        "bitrate".into(),
                        TargetValue::Integer(bitrate.round() as i64),
                    );
                }
            }
            let mut proof = descriptor.clone();
            proof.native_path = native_audio_path.clone();
            sources.last_mut().unwrap().native_media = Some(proof);
            prepared_audio.push(audio);
        }
        // Validate the file rekordbox will reference, after an explicitly
        // requested compatible copy has selected its publication location.
        let projected = paths.to_native(&native_audio_path)?;
        let key = if target.environment.is_native() {
            projected.to_string_lossy().into_owned()
        } else {
            projected.to_string_lossy().to_lowercase()
        };
        let physical = match native_audio_path.canonicalize() {
            Ok(physical) => physical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => native_audio_path.clone(),
            Err(error) => return Err(error).context("Native audio location is unavailable"),
        };
        if let Some(prior) = projected_sources.insert(key, physical.clone()) {
            ensure!(
                prior == physical,
                "Selected audio sources collide under destination path semantics"
            );
        }
        let mut owned = prior_owned_cues;
        // An explicit reviewed choice authorizes replacing the currently listed,
        // understood cue records. Observation or an automatic proposal does not.
        if write_cues
            && request
                .decisions
                .get(&track.id)
                .and_then(|d| d.get("cues"))
                .is_some_and(|d| {
                    matches!(
                        d,
                        NativeFieldDecision::UseLocal | NativeFieldDecision::ClearRemote
                    )
                })
        {
            for cue in remote.into_iter().flat_map(|r| &r.cues) {
                if cue.memory || cue.slot.is_some_and(|slot| slot < 8) {
                    ensure!(
                        cue.start_seconds.is_some_and(|v| v.is_finite() && v >= 0.)
                            && cue.end_seconds.is_none_or(|v| v.is_finite() && v >= 0.),
                        "A listed native cue has unsupported timing; retain the native cues"
                    );
                    owned.insert(cue.id.clone());
                }
            }
        }
        let need_assets = write_cues
            || write_grid
            || write_artwork
            || clear_artwork
            || relocate_audio
            || remote.is_none();
        let mut native_cues = vec![];
        let mut cue_bpm = None;
        let mut cue_colors = vec![];
        let bundle = if need_assets {
            let timed = if remote.is_none() || write_cues || write_grid || relocate_audio {
                cuerithm_dj_sync::timing::export_track(
                    &export_track,
                    cuerithm_dj_sync::DjSoftware::Rekordbox,
                )?
            } else {
                export_track.clone()
            };
            if write_cues && !matches!(chosen.get("cues"), Some(NativeFieldDecision::ClearRemote)) {
                cue_bpm = Some(timed.bpm);
                let projected = assets::cues(library, &timed, request.options.nearest_color)?;
                native_cues = projected.cues;
                cue_colors = projected.colors;
                if projected.converted > 0 {
                    warnings.push(format!(
                        "{} cue colors for '{}' use the nearest supported rekordbox palette",
                        projected.converted, track.title
                    ));
                }
            }
            let bundle = assets::build(
                library,
                &target,
                &paths,
                &id,
                &timed,
                &native_audio_path,
                &track.path,
                remote,
                &native_cues,
                &owned,
                write_cues,
                write_grid,
                relocate_audio,
                regenerate_waveforms,
                write_artwork,
                clear_artwork,
                runtime,
                cancel,
            )?;
            asset_sources.extend(bundle.sources.clone());
            publications.extend(bundle.publications.clone());
            bundle
        } else {
            assets::Bundle {
                analysis_path: None,
                artwork_path: None,
                sources: vec![],
                publications: vec![],
            }
        };
        if let Some(path) = bundle.artwork_path {
            fields.insert("artwork_path".into(), TargetValue::Text(path));
        }
        let replace_cues = remote
            .into_iter()
            .flat_map(|r| &r.cues)
            .filter(|cue| owned.contains(&cue.id))
            .map(|cue| NativeOwnedRow {
                id: cue.id.clone(),
                expected_raw_hash: cue.raw_hash.clone(),
            })
            .collect();
        let cue_colors = write_cues.then_some(cue_colors);
        let destination_audio_path =
            audio_reference_resolved(&paths, &native_audio_path, remote, relocate_audio)?;
        if fields.contains_key("source_path") {
            fields.insert(
                "source_path".into(),
                TargetValue::Text(
                    destination_audio_path
                        .to_str()
                        .context("Native audio path is not Unicode")?
                        .to_owned(),
                ),
            );
        }
        drafts.push(NativeTrackDraft {
            local_id: track.id.clone(),
            identity: remote.map(|r| r.identity.clone()),
            expected_raw_hash: remote.map(|r| r.raw_hash.clone()),
            source_path: destination_audio_path,
            fields,
            cues: write_cues.then_some(native_cues),
            cue_bpm,
            cue_colors,
            replace_cues: if write_cues { replace_cues } else { vec![] },
            analysis_path: bundle.analysis_path,
        });
    }
    for (track, fields) in &request.decisions {
        for field in fields.keys() {
            ensure!(
                seen_decisions.contains(&(track.clone(), field.clone())),
                "Decision is outside the selected track scope or mapped fields"
            );
        }
    }
    // Reserve track identities once, then resolve playlists against those exact
    // IDs using the same frozen seed. A second planning pass must not reallocate.
    let seed: [u8; 32] = Sha256::digest(id.as_bytes()).into();
    let datetime = chrono_tz::UTC
        .timestamp_millis_opt(crate::model::now())
        .single()
        .context("Invalid native timestamp")?;
    let mut plan_request = NativePlanRequest {
        source: snapshot.path.clone(),
        now: datetime.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string(),
        iso_now: datetime.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        id_seed: seed,
        tracks: drafts,
        entities: vec![],
        auxiliary_records: vec![],
        share_root: Some(target.asset_root.clone()),
    };
    let tracks_only = prepare_native_rows(&collection, &plan_request)?;
    let reserved = tracks_only
        .tracks
        .iter()
        .map(|r| (r.local_id.clone(), r.identity.content_id.clone()))
        .collect::<BTreeMap<_, _>>();
    for acknowledgement in &mut acknowledgements {
        if acknowledgement.content_id.is_empty()
            && let Some(id) = reserved.get(&acknowledgement.local_id)
        {
            acknowledgement.content_id = id.clone();
        }
    }
    acknowledgements.retain(|a| !a.content_id.is_empty());
    let pending_entities = pending_entities(
        library,
        &native_target_id,
        &local_library,
        &request.options,
        &reserved,
    )?;
    if !pending_entities.is_empty() {
        warnings.push("Playlist and My Tag changes await missing tracks. Include their tracks and title metadata in the sync scope, then preview again. All entity changes are withheld from this preview.".into());
    }
    if pending_creation_choice {
        warnings.push("New rekordbox tracks need a title. Include title in Metadata fields without skipping it, then preview again.".into());
    }
    let playlist_plan = if pending_entities.is_empty()
        && (request.options.playlists || request.options.my_tags)
    {
        let playlist_ids = if request.options.playlists {
            let mut selected = local_library
                .playlists
                .iter()
                .map(|playlist| playlist.id.clone())
                .collect::<BTreeSet<_>>();
            let connection = library.conn()?;
            let removed=connection.prepare("SELECT b.local_id FROM native_playlist_bindings b JOIN playlists p ON p.id=b.local_id WHERE b.target_id=?1 AND b.kind='playlist' AND b.owned=1 AND p.deleted=1 ORDER BY b.local_id")?.query_map([&native_target_id],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let subtree = if let RekordboxSyncScope::Playlist { playlist_id } = &request.scope {
                Some(connection.prepare("WITH RECURSIVE subtree(id) AS (SELECT id FROM playlists WHERE id=?1 UNION SELECT p.id FROM playlists p JOIN subtree s ON p.parent_id=s.id) SELECT id FROM subtree")?.query_map([playlist_id],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<BTreeSet<_>>>()?)
            } else {
                None
            };
            selected.extend(
                removed
                    .into_iter()
                    .filter(|id| subtree.as_ref().is_none_or(|ids| ids.contains(id))),
            );
            selected.into_iter().collect()
        } else {
            vec![]
        };
        let category_ids = if request.options.my_tags {
            let connection = library.conn()?;
            connection
                .prepare("SELECT id FROM custom_tag_categories UNION SELECT local_id FROM native_playlist_bindings WHERE target_id=?1 AND kind='category' AND owned=1 ORDER BY 1")?
                .query_map([&native_target_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            vec![]
        };
        let materialize = if request.options.materialize_smart_playlists {
            local_library
                .playlists
                .iter()
                .filter(|p| p.smart.is_some())
                .map(|p| p.id.clone())
                .collect()
        } else {
            BTreeSet::new()
        };
        let plan = library.plan_native_playlists_with_tracks(
            &native_target_id,
            NativePlaylistExportOptions {
                native_revision: observed.revision,
                playlist_ids,
                category_ids,
                materialize_smart_playlists: materialize,
            },
            &reserved,
        )?;
        plan_request.entities = entities(&plan, &collection)?;
        Some(plan)
    } else {
        None
    };
    let mut prepared = prepare_native_rows(&collection, &plan_request)?;
    ensure!(
        digest(&prepared.tracks)? == digest(&tracks_only.tracks)?,
        "Frozen native track allocation changed while planning playlists"
    );
    let staged = stage_native_write(&prepared.plan, &workspace, key, cancel)?;
    let (after, staged_collection) = {
        let lease = NativeDatabaseLease::acquire(&staged.snapshot.path, key, cancel)?;
        (
            lease.state(cancel)?,
            lease.read_collection(CollectionReadOptions::default(), cancel)?,
        )
    };
    let entity_changes = entity_changes(
        playlist_plan.as_ref(),
        &plan_request.entities,
        &prepared.entities,
        &collection,
        &staged_collection,
    )?;
    let receipts = prepared
        .entities
        .iter()
        .filter(|e| !e.removed)
        .map(|e| NativePlaylistPublicationReceipt {
            kind: match e.kind {
                NativeEntityKind::Playlist => NativePlaylistObjectKind::Playlist,
                NativeEntityKind::MyTag => {
                    if plan_request
                        .entities
                        .iter()
                        .any(|d| d.local_id == e.local_id && d.attribute == 1)
                    {
                        NativePlaylistObjectKind::Category
                    } else {
                        NativePlaylistObjectKind::Tag
                    }
                }
            },
            local_id: e.local_id.clone(),
            native_id: e.native_id.clone(),
            // Resolved ownership includes retained links. An insertion receipt
            // must claim only rows absent from the frozen native before-image.
            created_link_ids: e
                .owned_link_ids
                .iter()
                .filter(|id| {
                    let table = match e.kind {
                        NativeEntityKind::Playlist => "djmdSongPlaylist",
                        NativeEntityKind::MyTag => "djmdSongMyTag",
                    };
                    prepared.plan.rows.iter().any(|row| {
                        row.table == table && &row.id == *id && row.expected_fingerprint.is_none()
                    })
                })
                .cloned()
                .collect(),
        })
        .collect();
    // Finish case/root verification before writing the prepared asset journal
    // or the final local operation row. No mapped paths cross this boundary
    // without their operation-scoped evidence being checked.
    paths.verify()?;
    drop(paths);
    cancel.check()?;
    let transaction = FileTransaction::create(&root(library).join("assets"))?;
    let asset_journal_id = transaction.inspect()?.id;
    cleanup.asset_journal = Some(asset_journal_id.clone());
    if let Err(error) = assets::add_publications(&transaction, &publications) {
        FileTransaction::discard(&root(library).join("assets"), &asset_journal_id)?;
        return Err(error);
    }
    let skipped = reviews
        .iter()
        .filter(|r| r.decision.is_none() || r.decision == Some(NativeFieldDecision::Skip))
        .filter(|r| !matches!(r.result, NativeReconciliation::Unchanged))
        .count();
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} fields retain unresolved or skipped differences"
        ));
    }
    if media_copies > 0 {
        warnings.push(format!("Create {media_copies} compatible 16-bit stereo WAV copies using {copied_audio_bytes} bytes; original music stays unchanged"));
    }
    let preview = RekordboxSyncPreview {
        id: id.clone(),
        target_id: target.id.clone(),
        library_revision: revision,
        fields: reviews,
        entity_changes,
        pending_entities: pending_entities.clone(),
        tracks: prepared.tracks.len(),
        changed_rows: staged.changed_rows,
        skipped_fields: skipped,
        warnings,
        can_apply: writable && pending_entities.is_empty() && !pending_creation_choice,
        media_copies,
        copied_audio_bytes,
    };
    prepared.plan.source = target.database.clone();
    let ledger_before = {
        let connection = library.conn()?;
        ledger::capture(&connection, &native_target_id)?
    };
    let now = crate::model::now();
    let op = Operation {
        version: 1,
        environment_snapshot: Some(environment_snapshot),
        installation_hash: Some(installation_hash),
        target_roots,
        configuration,
        configuration_target,
        id: id.clone(),
        target,
        native_target_id,
        native_revision: observed.revision,
        request,
        revision,
        settings_revision,
        sources,
        asset_sources,
        before,
        after,
        plan: prepared.plan,
        resolved: prepared.tracks,
        playlist_plan,
        receipts,
        acknowledgements,
        preview: preview.clone(),
        asset_journal_id,
        before_snapshot: None,
        before_snapshot_hash: None,
        ledger_before,
        ledger_after_hash: None,
        status: "prepared".into(),
        created: now,
        updated: now,
        outcome: None,
    };
    cancel.check()?;
    verify_environment(&op, cancel)?;
    {
        let connection = library.conn()?;
        save(&connection, &op)?;
    }
    cleanup.committed = true;
    Ok(preview)
}
