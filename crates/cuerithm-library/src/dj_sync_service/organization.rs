//! Known exported desktop references, kept separate from metadata acknowledgement.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct KnownDjReference {
    pub key: String,
    pub target: SyncTarget,
    #[serde(with = "cuerithm_dj_sync::path_serde")]
    pub path: PathBuf,
    pub independent: bool,
    pub registration: String,
    pub catalog_digest: String,
}
impl Library {
    pub(crate) fn organization_sync_lock(&self) -> Result<File> {
        service_lock(&self.data_dir().join("sync"))
    }
    pub(crate) fn known_organization_exports(&self, track: &str) -> Result<Vec<KnownDjReference>> {
        let root = self.data_dir().join("sync");
        let folder = root.join("baselines");
        if !folder.exists() {
            return Ok(vec![]);
        }
        let mut histories = vec![];
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir()
                || uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
            {
                continue;
            }
            let path = entry.path().join("library.json");
            if !path.exists() {
                continue;
            }
            let saved: LibraryJournal = serde_json::from_reader(File::open(path)?)?;
            if saved.acknowledged
                && !saved.restore_acknowledged
                && saved.sources.iter().any(|s| s.id == track)
            {
                ensure!(
                    FileTransaction::open(&root, &saved.id)?.inspect()?.state
                        == JournalState::Applied,
                    "Known native export journal is not applied; recover Sync history first"
                );
                histories.push(saved);
            }
        }
        let mut refs = vec![];
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let baseline: Baseline = serde_json::from_reader(File::open(entry.path())?)?;
            ensure!(baseline.version == 1, "Unsupported native sync baseline");
            if !baseline.track_hashes.contains_key(track) {
                continue;
            }
            let target = baseline
                .target
                .clone()
                .context("Known native export has no registered target")?;
            let key = digest(&target.identity_target())?;
            let mut matching = histories
                .iter()
                .filter(|h| {
                    digest(&h.request.target.identity_target()).is_ok_and(|hash| hash == key)
                        && h.track_hashes.get(track) == baseline.track_hashes.get(track)
                })
                .collect::<Vec<_>>();
            matching.sort_by_key(|h| h.created);
            let saved = *matching
                .last()
                .context("Known native export lacks acknowledged source-path history")?;
            let source = saved
                .sources
                .iter()
                .find(|source| source.id == track)
                .context("Native export source binding is missing")?;
            let independent = target.software == DjSoftware::PioneerUsb
                || (target.software == DjSoftware::EngineDj
                    && cuerithm_dj_sync::export_media_is_independent(
                        &target,
                        &saved.request.options,
                    ));
            ensure!(
                matching
                    .iter()
                    .filter(|h| h.created == saved.created)
                    .all(|h| h.request.options.engine_copy_audio
                        == saved.request.options.engine_copy_audio
                        && h.sources
                            .iter()
                            .find(|s| s.id == track)
                            .is_some_and(|s| s.path == source.path)),
                "Native export history has ambiguous source ownership"
            );
            let mut path = source.path.clone();
            let rebound:Option<(Vec<u8>,i64)>=self.conn()?.query_row("SELECT path,updated FROM organization_reference_paths WHERE track_id=?1 AND target_key=?2",params![track,key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((native, at)) = rebound
                && at >= saved.created
            {
                path = crate::model::bytes_path(&native)?;
            }
            let ledger = ownership::read(&root, &target)?;
            let registration = ledger.registration.clone();
            let mut logical = ledger;
            logical.registration.clear();
            refs.push(KnownDjReference {
                key,
                target,
                path,
                independent,
                registration,
                catalog_digest: digest(&(
                    baseline.track_hashes.get(track),
                    saved.id.clone(),
                    independent,
                    logical,
                ))?,
            });
        }
        refs.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(refs)
    }
    pub(crate) fn organization_target_registration(&self, target: &SyncTarget) -> Result<String> {
        ownership::registration(target)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct OrganizationOwnershipTransition {
    target: SyncTarget,
    before: ownership::OwnershipLedger,
    after: ownership::OwnershipLedger,
}
impl Library {
    pub(crate) fn plan_organization_ownership(
        &self,
        target: &SyncTarget,
        before_files: &BTreeMap<PathBuf, Option<String>>,
        after_files: &BTreeMap<PathBuf, Option<String>>,
    ) -> Result<OrganizationOwnershipTransition> {
        let root = self.data_dir().join("sync");
        let before = ownership::read(&root, target)?;
        ensure!(
            !before.registration.is_empty(),
            "Native reference has no registered ownership ledger"
        );
        let mut after = before.clone();
        after.registration = ownership::registration(target)?;
        if target.software == DjSoftware::VirtualDj {
            for (database, (_, state)) in &mut after.stores {
                let home = database.parent().context("VirtualDJ home missing")?;
                for binding in &mut state.bindings {
                    let path = home.join(&binding.locator);
                    if let (Some(Some(before)), Some(Some(current))) =
                        (before_files.get(&path), after_files.get(&path))
                        && before != current
                    {
                        ensure!(
                            binding.fingerprint == *before
                                && file_hash(&path)?.as_ref() == Some(current),
                            "Owned VirtualDJ playlist changed outside the acknowledged path repair"
                        );
                        binding.fingerprint = current.clone();
                    }
                }
            }
        }
        Ok(OrganizationOwnershipTransition {
            target: target.clone(),
            before,
            after,
        })
    }
    pub(crate) fn publish_organization_ownership(
        &self,
        transition: &OrganizationOwnershipTransition,
    ) -> Result<()> {
        let root = self.data_dir().join("sync");
        let current = ownership::read(&root, &transition.target)?;
        let installed = digest(&current)? == digest(&transition.after)?;
        ensure!(
            installed || digest(&current)? == digest(&transition.before)?,
            "Native ownership changed before reference acknowledgement"
        );
        ensure!(
            ownership::registration(&transition.target)? == transition.after.registration,
            "Repaired native database was replaced before ownership publication"
        );
        if !installed {
            ownership::save(&root, &transition.target, &transition.after)?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Library {
    pub(crate) fn seed_organization_reference_fixture(
        &self,
        track: &str,
        target: SyncTarget,
        independent: bool,
    ) -> Result<()> {
        let root = self.data_dir().join("sync");
        let transaction = FileTransaction::create(&root)?;
        if target.path.is_file() {
            transaction.add(&target.path, file_hash(&target.path)?, Some(&target.path))?;
        }
        let journal = transaction.apply(|| Ok(()))?;
        let (path, hash): (Vec<u8>, String) = self.conn()?.query_row(
            "SELECT path,source_hash FROM tracks WHERE id=?1",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let sources = vec![Source {
            id: track.into(),
            path: crate::model::bytes_path(&path)?,
            hash: hash.clone(),
        }];
        let track_hashes = BTreeMap::from([(track.into(), "fixture-native-projection".into())]);
        let saved = LibraryJournal {
            target_root: None,
            selected_target: None,
            version: 1,
            id: journal.id.clone(),
            revision: self.revision()?,
            request: DjSyncRequest {
                configuration: None,
                target: target.clone(),
                options: SyncOptions {
                    engine_copy_audio: independent,
                    ..Default::default()
                },
                scope: DjSyncScope::Modified {
                    track_ids: vec![track.into()],
                },
            },
            sources,
            track_hashes: track_hashes.clone(),
            playlist_hashes: BTreeMap::new(),
            replace_baseline: false,
            created: crate::model::now(),
            acknowledged: true,
            error: None,
            ownership_before: Default::default(),
            ownership_after: None,
            ownership_restored: None,
            restore_acknowledged: false,
            preview_registration: String::new(),
            restore_expired: false,
        };
        atomic_json(&transaction.path().join("library.json"), &saved)?;
        let mut ledger = ownership::read(&root, &target)?;
        ledger.version = 1;
        ledger.generation += 1;
        ledger.last_journal = journal.id.clone();
        ledger.registration = ownership::registration(&target)?;
        ownership::save(&root, &target, &ledger)?;
        let mut baseline = read_baseline(&root, &target)?.unwrap_or_default();
        baseline.version = 1;
        baseline.target = Some(target.clone());
        baseline.journal_id = journal.id;
        baseline.track_hashes.extend(track_hashes);
        baseline.source_hashes.insert(track.into(), hash);
        let path = baseline_path(&root, &target)?;
        fs::create_dir_all(path.parent().unwrap())?;
        atomic_json(&path, &baseline)
    }
}
