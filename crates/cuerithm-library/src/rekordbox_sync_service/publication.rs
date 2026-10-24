use super::*;
fn transaction(library: &Library, op: &Operation) -> Result<FileTransaction> {
    FileTransaction::open(&root(library).join("assets"), &op.asset_journal_id)
}
fn mark(library: &Library, op: &mut Operation, status: &str) -> Result<()> {
    op.status = status.into();
    op.updated = crate::model::now();
    let connection = library.conn()?;
    save(&connection, op)
}
fn current_target(
    library: &Library,
    require_current_configuration: bool,
    op: &Operation,
    cancel: &CancellationToken,
) -> Result<(DatabaseKey, DatabaseSchema)> {
    let paths = DiscoveryPaths::current()?;
    verify_environment(op, cancel)?;
    let current = if require_current_configuration {
        if let Some(binding) = &op.configuration {
            library.verify_sync_target_configuration(
                binding,
                &SavedSyncTarget::Rekordbox {
                    target: op
                        .configuration_target
                        .as_ref()
                        .unwrap_or(&op.target)
                        .clone(),
                },
            )?;
        }
        let current = resolve_request_target(library, &op.request, &paths, cancel)?;
        ensure!(
            digest(&current)? == digest(&op.target)?,
            "Configured native destination changed"
        );
        if matches!(
            current.installation.version_source.as_str(),
            "version_resource" | "bundle_plist"
        ) {
            let inspected = inspect_manual_installation(&current.installation.executable, None)?;
            ensure!(
                inspected.version == current.installation.version
                    && inspected.major == current.installation.major,
                "Installed rekordbox version changed; create a new preview"
            );
        }
        current
    } else {
        op.target.clone()
    };
    let key = key_for(&current, paths.platform, cancel)?;
    let db = ReadOnlyDatabase::open(&current.database, &key)?;
    let schema = db.schema()?;
    require_capability(&current, &schema, cancel)?;
    drop(db);
    Ok((key, schema))
}
fn verify_asset_sources(op: &Operation, cancel: &CancellationToken) -> Result<()> {
    for asset in &op.asset_sources {
        ensure_not_native_file(&op.target.database, &asset.path)?;
        ensure!(
            file_sha256_cancellable(&asset.path, cancel)? == asset.hash,
            "Original native assets changed; restoring old references would be unsafe"
        );
    }
    Ok(())
}
fn verify_restore_sources(op: &Operation, cancel: &CancellationToken) -> Result<()> {
    for source in &op.sources {
        ensure_not_native_file(&op.target.database, &source.path)?;
        ensure!(
            file_sha256_cancellable(&source.path, cancel)? == source.hash,
            "A synced audio source changed after publication; restore refused"
        );
    }
    verify_asset_sources(op, cancel)?;
    let snapshot = op
        .before_snapshot
        .as_ref()
        .context("Native before-image is unavailable")?;
    ensure!(
        Some(file_sha256_cancellable(snapshot, cancel)?) == op.before_snapshot_hash,
        "Native before-image proof changed"
    );
    Ok(())
}
fn backup(
    library: &Library,
    op: &mut Operation,
    lease: &NativeDatabaseLease,
    key: &DatabaseKey,
    cancel: &CancellationToken,
) -> Result<()> {
    let workspace = root(library).join("operations").join(&op.id);
    let snapshot = lease.snapshot(&workspace, key, cancel)?;
    let path = workspace.join("before.db");
    ensure!(
        !path.exists(),
        "Native before-image already exists without a durable journal"
    );
    fs::copy(&snapshot.path, &path)?;
    File::options()
        .read(true)
        .write(true)
        .open(&path)?
        .sync_all()?;
    #[cfg(unix)]
    File::open(&workspace)?.sync_all()?;
    ensure!(
        file_sha256_cancellable(&path, cancel)? == snapshot.sha256,
        "Native backup proof differs"
    );
    op.before_snapshot = Some(path);
    op.before_snapshot_hash = Some(snapshot.sha256);
    mark(library, op, "publishing")
}
fn outcome(op: &Operation, status: &str) -> RekordboxSyncOutcome {
    let mut tracks = op
        .acknowledgements
        .iter()
        .map(|a| a.local_id.as_str())
        .collect::<BTreeSet<_>>();
    tracks.extend(
        op.resolved
            .iter()
            .filter(|track| {
                track.created
                    || op.plan.rows.iter().any(|row| {
                        row.table == "djmdContent" && row.id == track.identity.content_id
                    })
            })
            .map(|track| track.local_id.as_str()),
    );
    RekordboxSyncOutcome {
        id: op.id.clone(),
        status: status.into(),
        tracks: tracks.len(),
        changed_rows: if status == "applied" {
            op.preview.changed_rows
        } else {
            0
        },
        acknowledged_fields: if status == "applied" {
            op.acknowledgements.len()
        } else {
            0
        },
        skipped_fields: op.preview.skipped_fields,
    }
}
fn acknowledge(
    library: &Library,
    op: &mut Operation,
    lease: &NativeDatabaseLease,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    cancel.check()?;
    guard()?;
    ensure!(
        lease.state(cancel)? == op.after,
        "Native publication proof changed before acknowledgement"
    );
    transaction(library, op)?.verify_applied()?;
    verify_environment(op, cancel)?;
    verify_sources(library, op, cancel)?;
    let mut collection = lease.read_collection(CollectionReadOptions::default(), cancel)?;
    {
        let paths = op.target.environment.resolver()?;
        project_collection(&paths, &mut collection)?;
        paths.verify()?;
        verify_environment(op, cancel)?;
    }
    // A new native content path may be a separately published compatible copy.
    // Bind its verified identity to the ORIGINAL local track before observation,
    // so import cannot create a second local song or preparation job for a copy.
    {
        let mut connection = library.conn()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        verify_local_rows(&tx, op)?;
        for resolved in op.resolved.iter().filter(|track| track.created) {
            let published = collection
                .tracks
                .iter()
                .find(|track| track.identity.content_id == resolved.identity.content_id)
                .context("New native content publication is missing")?;
            ensure!(
                !crate::native_sync::changed_identity(&resolved.identity, &published.identity),
                "New native content identity differs from its publication receipt"
            );
            let existing:Option<Option<String>>=tx.query_row("SELECT local_track_id FROM native_track_links WHERE target_id=?1 AND content_id=?2",params![op.native_target_id,resolved.identity.content_id],|r|r.get(0)).optional()?;
            if let Some(existing) = existing {
                ensure!(
                    existing.as_deref() == Some(&resolved.local_id),
                    "New native content identity was claimed by a different local track"
                );
            } else {
                tx.execute("INSERT INTO native_track_links(target_id,content_id,local_track_id,identity_json,observation_json,observation_hash,baseline_json,status,updated) VALUES(?1,?2,?3,?4,NULL,NULL,'{}','linked',?5)",params![op.native_target_id,resolved.identity.content_id,resolved.local_id,serde_json::to_string(&published.identity)?,crate::model::now()])?;
            }
        }
        tx.commit()?;
    }
    let observed = library.observe_rekordbox_collection(
        &collection,
        NativeImportOptions {
            settings_revision: op.settings_revision.clone(),
            content_ids: None,
            initial_owners: BTreeMap::new(),
        },
        cancel,
    )?;
    ensure!(
        observed.target_id == op.native_target_id,
        "Published native identity changed"
    );
    if let Some(plan) = &op.playlist_plan {
        library.acknowledge_native_playlist_plan(plan, &collection, &op.receipts)?;
    }
    let _organization_gate = library
        .inner
        .organization_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
    #[cfg(feature = "autotag")]
    let _tag_gate = library
        .inner
        .tag_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
    verify_environment(op, cancel)?;
    verify_sources(library, op, cancel)?;
    // Cache asset path observations in one read-only phase before the local
    // ledger transaction can create journals in a mapped ancestor. Never carry
    // an unverified case index across our own SQLite directory mutations.
    let paths = op.target.environment.resolver()?;
    let mut asset_observations = BTreeMap::new();
    for accepted in &op.acknowledgements {
        if !matches!(accepted.field.as_str(), "beat_grid" | "artwork") {
            continue;
        }
        let published = collection
            .tracks
            .iter()
            .find(|track| track.identity.content_id == accepted.content_id)
            .context("Acknowledged native track is absent")?;
        let observed = match accepted.field.as_str() {
            "beat_grid" => published
                .source
                .analysis_path
                .as_deref()
                .filter(|path| !path.is_empty())
                .map(|path| assets::grid_observation_resolved(&op.target, &paths, path))
                .transpose()?
                .unwrap_or(Value::Null),
            "artwork" => published
                .source
                .artwork_path
                .as_deref()
                .filter(|path| !path.is_empty())
                .map(|path| assets::artwork_observation_resolved(&op.target, &paths, path, cancel))
                .transpose()?
                .unwrap_or(Value::Null),
            _ => unreachable!(),
        };
        asset_observations.insert(
            (accepted.content_id.clone(), accepted.field.clone()),
            observed,
        );
    }
    paths.verify()?;
    drop(paths);
    verify_environment(op, cancel)?;
    let mut connection = library.conn()?;
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    verify_local_rows(&tx, op)?;
    for track in &op.resolved {
        let published = collection
            .tracks
            .iter()
            .find(|r| r.identity.content_id == track.identity.content_id)
            .context("Published native track disappeared")?;
        ensure!(
            !crate::native_sync::changed_identity(&track.identity, &published.identity),
            "Published native track identity differs"
        );
        let local_id: Option<String> = tx.query_row(
            "SELECT local_track_id FROM native_track_links WHERE target_id=?1 AND content_id=?2",
            params![op.native_target_id, track.identity.content_id],
            |r| r.get(0),
        )?;
        ensure!(
            local_id.as_deref() == Some(&track.local_id),
            "Published native track linked to another local track"
        );
        let writes_cues = op.acknowledgements.iter().any(|a| {
            a.local_id == track.local_id
                && a.field == "cues"
                && matches!(
                    a.decision,
                    NativeFieldDecision::UseLocal | NativeFieldDecision::ClearRemote
                )
        });
        let media = op
            .sources
            .iter()
            .find(|source| source.id == track.local_id)
            .and_then(|source| source.native_media.as_ref());
        if writes_cues || media.is_some() {
            let existing:Option<(String,Option<String>,String)>=tx.query_row("SELECT cue_ids_json,source_json,cue_proofs_json FROM native_sync_track_ownership WHERE target_id=?1 AND local_track_id=?2",params![op.native_target_id,track.local_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let cue_ids = if writes_cues {
                ensure!(
                    track
                        .owned_cue_ids
                        .iter()
                        .all(|id| published.cues.iter().any(|cue| &cue.id == id)),
                    "Published native cue ownership receipt differs"
                );
                serde_json::to_string(&track.owned_cue_ids)?
            } else {
                existing.as_ref().map_or("[]".into(), |row| row.0.clone())
            };
            let cue_proofs = if writes_cues {
                let proofs = published
                    .cues
                    .iter()
                    .filter(|cue| track.owned_cue_ids.contains(&cue.id))
                    .map(|cue| {
                        (
                            cue.id.clone(),
                            CueOwnershipProof {
                                uuid: cue.uuid.clone(),
                                content_uuid: cue.content_uuid.clone(),
                                raw_hash: cue.raw_hash.clone(),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                serde_json::to_string(&proofs)?
            } else {
                existing.as_ref().map_or("{}".into(), |row| row.2.clone())
            };
            let source_json = media
                .map(serde_json::to_string)
                .transpose()?
                .or_else(|| existing.as_ref().and_then(|row| row.1.clone()));
            if let Some(media) = media {
                ensure!(
                    published
                        .source
                        .resolved_path
                        .as_deref()
                        .is_some_and(|path| Path::new(path).canonicalize().ok().is_some_and(
                            |physical| media.native_path.canonicalize().ok() == Some(physical)
                        )),
                    "Published native audio relationship differs from the prepared source proof"
                );
            }
            tx.execute("INSERT INTO native_sync_track_ownership(target_id,local_track_id,identity_json,cue_ids_json,cue_proofs_json,source_json) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(target_id,local_track_id) DO UPDATE SET identity_json=excluded.identity_json,cue_ids_json=excluded.cue_ids_json,cue_proofs_json=excluded.cue_proofs_json,source_json=excluded.source_json",params![op.native_target_id,track.local_id,serde_json::to_string(&published.identity)?,cue_ids,cue_proofs,source_json])?;
        }
    }
    let mut by_content: BTreeMap<String, Vec<&FieldAcknowledgement>> = BTreeMap::new();
    for acknowledgement in &op.acknowledgements {
        by_content
            .entry(acknowledgement.content_id.clone())
            .or_default()
            .push(acknowledgement);
    }
    for (content, accepted) in by_content {
        let published = collection
            .tracks
            .iter()
            .find(|r| r.identity.content_id == content)
            .context("Acknowledged native track is absent")?;
        let remote = crate::native_sync::remote_fields(published)?;
        let saved: String = tx.query_row(
            "SELECT baseline_json FROM native_track_links WHERE target_id=?1 AND content_id=?2",
            params![op.native_target_id, content],
            |r| r.get(0),
        )?;
        let mut fields: BTreeMap<String, NativeFieldBaseline> = serde_json::from_str(&saved)?;
        for accepted in accepted {
            let value = match accepted.field.as_str() {
                "beat_grid" | "artwork" => asset_observations
                    .get(&(content.clone(), accepted.field.clone()))
                    .cloned()
                    .context("Published asset observation is unavailable")?,
                _ => remote
                    .get(&accepted.field)
                    .cloned()
                    .context("Published field observation is unavailable")?,
            };
            let owner = fields
                .get(&accepted.field)
                .map_or(NativeFieldOwner::Shared, |f| f.owner);
            fields.insert(
                accepted.field.clone(),
                NativeFieldBaseline {
                    local: accepted.local.clone(),
                    remote: value,
                    owner,
                    acknowledged: true,
                },
            );
        }
        tx.execute("UPDATE native_track_links SET baseline_json=?1,updated=?2 WHERE target_id=?3 AND content_id=?4",params![serde_json::to_string(&fields)?,crate::model::now(),op.native_target_id,content])?;
    }
    op.ledger_after_hash = Some(digest(&ledger::capture(&tx, &op.native_target_id)?)?);
    let result = outcome(op, "applied");
    op.outcome = Some(result.clone());
    op.status = "applied".into();
    op.updated = crate::model::now();
    save(&tx, op)?;
    guard()?;
    cancel.check()?;
    tx.commit()?;
    Ok(result)
}
fn apply(
    library: &Library,
    op: &mut Operation,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    ensure!(
        op.status == "prepared",
        "Recover the interrupted native operation before applying again"
    );
    ensure!(
        op.preview.can_apply,
        "This preview has no accepted native write capability"
    );
    let organization_gate = library
        .inner
        .organization_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
    #[cfg(feature = "autotag")]
    let tag_gate = library
        .inner
        .tag_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
    cancel.check()?;
    guard()?;
    verify_environment(op, cancel)?;
    verify_sources(library, op, cancel)?;
    {
        let connection = library.conn()?;
        verify_native_observation(&connection, op)?;
    }
    if let Some(plan) = &op.playlist_plan {
        library.validate_native_playlist_plan(plan)?;
    }
    let mut lease = NativeDatabaseLease::acquire(&op.target.database, key, cancel)?;
    ensure!(
        lease.state(cancel)? == op.before,
        "Native database changed after preview"
    );
    // Never open/hash/reopen the original database while this lease is held.
    // Before-image+journal are durable before creating any new native resources.
    backup(library, op, &lease, key, cancel)?;
    let files = transaction(library, op)?;
    files.apply(|| {
        cancel.check()?;
        lease.state(cancel)?;
        verify_environment(op, cancel)?;
        guard()?;
        Ok(())
    })?;
    files.verify_applied()?;
    let mut local_connection = library.conn()?;
    let local_tx =
        local_connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    verify_local_rows(&local_tx, op)?;
    let result =
        lease.apply_rows_frozen_checked(&op.plan, &op.before, &op.after, cancel, || {
            verify_local_rows(&local_tx, op)?;
            verify_native_observation(&local_tx, op)?;
            verify_environment(op, cancel)?;
            verify_source_bytes(op, cancel)?;
            files.verify_applied()?;
            guard()?;
            Ok(())
        })?;
    op.status = "native_published".into();
    op.updated = crate::model::now();
    save(&local_tx, op)?;
    local_tx.commit()?;
    drop(local_connection);
    ensure!(
        result.after_state == op.after,
        "Committed native state differs from the frozen staged proof; recover this operation"
    );
    #[cfg(feature = "autotag")]
    drop(tag_gate);
    drop(organization_gate);
    acknowledge(library, op, &lease, cancel, guard)
}
fn verify_ledger(library: &Library, op: &Operation) -> Result<()> {
    if let Some(expected) = &op.ledger_after_hash {
        let connection = library.conn()?;
        ensure!(
            digest(&ledger::capture(&connection, &op.native_target_id)?)? == *expected,
            "Native synchronization ownership changed after publication; restore refused"
        );
    }
    Ok(())
}
fn restore_finish(
    library: &Library,
    op: &mut Operation,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    // Match apply's order: service lock -> organization -> tagging -> native
    // lease/local connection. Retain both gates through the final local commit;
    // a SQLite transaction alone cannot exclude workers changing physical files.
    let _organization_gate = library
        .inner
        .organization_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Organization gate interrupted"))?;
    #[cfg(feature = "autotag")]
    let _tag_gate = library
        .inner
        .tag_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Publication gate interrupted"))?;
    guard()?;
    cancel.check()?;
    verify_environment(op, cancel)?;
    verify_restore_sources(op, cancel)?;
    verify_ledger(library, op)?;
    if op.status != "restoring" {
        ensure!(
            matches!(op.status.as_str(), "applied" | "native_published"),
            "Recover the interrupted native operation before restoring"
        );
        transaction(library, op)?.verify_applied()?;
    }
    let mut lease = NativeDatabaseLease::acquire(&op.target.database, key, cancel)?;
    if op.status != "restoring" {
        ensure!(
            lease.state(cancel)? == op.after,
            "Native database changed after publication; restore refused"
        );
        mark(library, op, "restoring")?;
    }
    let mut local_connection = library.conn()?;
    let local_tx =
        local_connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(expected) = &op.ledger_after_hash {
        ensure!(
            digest(&ledger::capture(&local_tx, &op.native_target_id)?)? == *expected,
            "Native synchronization ownership changed after publication; restore refused"
        );
    }
    let current = lease.state(cancel)?;
    ensure!(
        current == op.after || current == op.before,
        "Native database changed after publication; restore refused"
    );
    let snapshot = op
        .before_snapshot
        .as_ref()
        .context("Native before-image is unavailable")?;
    ensure!(
        Some(file_sha256_cancellable(snapshot, cancel)?) == op.before_snapshot_hash,
        "Native before-image proof changed"
    );
    if current != op.before {
        transaction(library, op)?.verify_applied()?;
        // Switch database references back FIRST. Old immutable assets were never
        // replaced. Only then remove newly created versioned resources.
        verify_environment(op, cancel)?;
        guard()?;
        let restored =
            lease.restore_snapshot_checked(snapshot, key, &op.before, &op.after, cancel)?;
        ensure!(
            restored == op.before,
            "Native restore before-image state differs"
        );
    }
    transaction(library, op)?.restore(|| {
        cancel.check()?;
        lease.state(cancel)?;
        verify_environment(op, cancel)?;
        guard()?;
        Ok(())
    })?;
    transaction(library, op)?.verify_restored()?;
    ledger::restore(&local_tx, &op.ledger_before)?;
    let result = outcome(op, "restored");
    op.status = "restored".into();
    op.updated = crate::model::now();
    op.outcome = Some(result.clone());
    save(&local_tx, op)?;
    guard()?;
    cancel.check()?;
    local_tx.commit()?;
    Ok(result)
}
fn recover(
    library: &Library,
    op: &mut Operation,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    if op.status == "restoring" {
        return restore_finish(library, op, key, cancel, guard);
    }
    cancel.check()?;
    guard()?;
    verify_environment(op, cancel)?;
    let lease = NativeDatabaseLease::acquire(&op.target.database, key, cancel)?;
    ensure!(
        matches!(op.status.as_str(), "publishing" | "native_published"),
        "Native operation does not require recovery"
    );
    let current = lease.state(cancel)?;
    if current == op.after
        && (op.after != op.before
            || transaction(library, op)?.inspect()?.state
                == cuerithm_dj_sync::transaction::JournalState::Applied)
    {
        transaction(library, op)?.verify_applied()?;
        return acknowledge(library, op, &lease, cancel, guard);
    }
    ensure!(
        current == op.before,
        "Native database changed during interruption; recovery refused"
    );
    // A pre-commit interruption can leave only unreferenced new resources.
    // Roll those back without changing the original database or ownership.
    transaction(library, op)?.restore(|| {
        cancel.check()?;
        lease.state(cancel)?;
        verify_environment(op, cancel)?;
        guard()?;
        Ok(())
    })?;
    transaction(library, op)?.verify_restored()?;
    let result = outcome(op, "rolled_back");
    op.outcome = Some(result.clone());
    mark(library, op, "rolled_back")?;
    Ok(result)
}
impl Library {
    pub fn apply_rekordbox_sync(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncOutcome> {
        let _lock = lock(self)?;
        let mut op = load(self, id)?;
        if op.status == "applied" {
            return op.outcome.context("Applied native outcome missing");
        }
        let (key, _) = current_target(self, true, &op, cancel)?;
        let outcome = apply(self, &mut op, &key, cancel, &ensure_closed)?;
        // The oldest syncs of this target give up their backups.
        let _ = retention::prune_locked(self, retention::RESTORE_LIMIT);
        Ok(outcome)
    }
    pub fn restore_rekordbox_sync(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncOutcome> {
        let _lock = lock(self)?;
        let mut op = load(self, id)?;
        if op.status == "restored" {
            return op.outcome.context("Restored native outcome missing");
        }
        ensure!(
            matches!(op.status.as_str(), "applied" | "native_published"),
            "Recover the interrupted native operation before restoring"
        );
        let (key, _) = current_target(self, false, &op, cancel)?;
        let outcome = restore_finish(self, &mut op, &key, cancel, &ensure_closed)?;
        // A restored sync can never be restored again: it gives up its backup now.
        let _ = retention::prune_locked(self, retention::RESTORE_LIMIT);
        Ok(outcome)
    }
    pub fn recover_rekordbox_sync(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<RekordboxSyncOutcome> {
        let _lock = lock(self)?;
        let mut op = load(self, id)?;
        if matches!(op.status.as_str(), "applied" | "restored" | "rolled_back") {
            return op.outcome.context("Settled native outcome missing");
        }
        let (key, _) = current_target(self, false, &op, cancel)?;
        recover(self, &mut op, &key, cancel, &ensure_closed)
    }
}
#[cfg(test)]
pub(super) fn fixture_apply(
    library: &Library,
    id: &str,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    let mut op = load(library, id)?;
    apply(library, &mut op, key, cancel, guard)
}
#[cfg(test)]
pub(super) fn fixture_recover(
    library: &Library,
    id: &str,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    let mut op = load(library, id)?;
    recover(library, &mut op, key, cancel, guard)
}
#[cfg(test)]
pub(super) fn fixture_restore(
    library: &Library,
    id: &str,
    key: &DatabaseKey,
    cancel: &CancellationToken,
    guard: &impl Fn() -> Result<()>,
) -> Result<RekordboxSyncOutcome> {
    let mut op = load(library, id)?;
    restore_finish(library, &mut op, key, cancel, guard)
}
