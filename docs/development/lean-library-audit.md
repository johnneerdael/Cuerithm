# Lean library audit

The library is meant to hold tags, links to files and artwork. It must never hold audio.
Music removed from the library must leave nothing behind, and Undo is bounded: ten
operations. This page is the audit behind issue #46. It lists every place CueRithm retains data
in its data folder or in the library database, what keeps it, how big it gets and what was
decided and shipped. The verdict column says what the change did; section 8 lists what it
deliberately left and why.

How to read the numbers: sizes marked *measured* come from a read-only inspection of a copy of
one real library (26 tracks, a rekordbox collection of 3,417 tracks, 36,752 cue rows). Sizes marked
*estimate* are computed from the field widths in the code and are not measurements. No
per-track data from that library is recorded here.

## Measured on one real library

| Item | Before | After this change |
|---|---:|---:|
| `library.sqlite` | 252 MiB | 31 MiB |
| `library.sqlite-wal` | 129 MiB | 0 (truncated at open) |
| `tracks` (26 rows) | 0.7 MiB | 0.7 MiB |
| `native_records` (62,208 rows) | 99.4 MiB + 6.9 MiB index | 21.7 MiB + 6.1 MiB index |
| `native_track_links` (3,417 rows) | 20.8 MiB | 1.1 MiB |
| Free pages inside the database file | 31,248 pages (122 MiB, 48% of the file) | 0 |
| `cache/artwork` (26 files) | 35 MiB | unchanged (artwork is allowed) |
| `cache/waveforms` (29 files) | 4.1 MiB | unchanged (rebuilt on demand) |
| `analysis-components` | 18 MiB | unchanged (the bundled model, reinstalled when missing) |

Upgrading that copy from schema 28 to the new schema 31 took 1.2 seconds in a debug build: the
migration itself, one rewrite of the live data to enable incremental auto-vacuum, and a
truncated log.

The copy had no saved scans, reviews, previews, cleanup or metadata history and no tag-write
originals, so those rows below are *estimates* from the code, not measurements.

## 1. The SQLite file itself

| What | Where | Writer / reader | Why it exists | Bound today | Size | Verdict |
|---|---|---|---|---|---|---|
| Write-ahead log | `library.sqlite-wal` | SQLite; one long-lived connection (`store.rs`) | WAL journal mode, `synchronous=FULL` | None: no `journal_size_limit`, default autocheckpoint, no checkpoint on exit (the process ends without closing the connection) | measured 129 MiB | **Bound**: `journal_size_limit` 16 MiB, autocheckpoint 1000 pages, `TRUNCATE` checkpoint at open, after large operations and on a clean shutdown |
| Free pages | inside `library.sqlite` | SQLite | rows deleted by earlier versions | None: no `auto_vacuum`, no `VACUUM` anywhere | measured 122 MiB | **Reclaim**: new libraries use incremental auto-vacuum; an older one is converted once by a single `VACUUM` when it opens (only when the live data is up to 512 MiB and the disk has room for three times that: measured 0.7 s for 256 MiB and 2.4 s for 1 GiB, linear, so 2 GiB would hold the window back for 5 s or more), then freed in bounded steps. `reclaim_storage` converts up to 2 GiB on request; no app command calls it yet, so an older library above 512 MiB keeps its free pages |
| Library-switch and restore staging databases | `libraries/<id>/`, `library-backups/` | `generations.rs`, `library_backup.rs` | recovery after a library restore | None; superseded generations are kept | one full database each | **Keep** (explicit user recovery; not derived data) |
| Catalog runtime database | `catalog-runtime/catalog.sqlite3` | provider setup | provider response cache | rows and bytes capped (256 entries, 32 MiB) | measured 0.1 MiB | **Keep** |

## 2. DJ-software copies

| What | Where | Writer / reader | Why it exists | Bound before | Size | Verdict (shipped) |
|---|---|---|---|---|---|---|
| `native_records.json` | table, one row per record of the rekordbox database | `native_sync::store_records`; read back only by `native_playlist_sync` for the four playlist and MyTag tables | baseline "kept for conflict detection" | None | measured 99 MiB (62k rows) | **Smaller form**: every record keeps its key and the SHA-256 of its content (all that a later observation compares). The content is kept only for `djmdPlaylist`, `djmdSongPlaylist`, `djmdMyTag` and `djmdSongMyTag`. Schema 30 converts existing rows. A paired-run test shows the same decisions from both forms |
| `native_track_links.observation_json` | table | `native_sync`; the reconciliation review for a *linked* track, the portable backup | last observed rekordbox track | None | measured 18 MiB for 3,417 tracks, none linked | **Smaller form**: a DJ-software track with no library track keeps `observation_hash` and no observation; a linked track keeps its observation |
| `native_track_links.baseline_json` | table | `native_sync`, `rekordbox_sync_service` | the real three-way merge baseline | one row per linked track | measured 7 KB | **Keep** |
| `rekordbox_sync_operations.journal_json` | table, one row per sync | `rekordbox_sync_service` | frozen plan, receipts and `ledger_before` (the rows of the seven `native_*` tables), needed to restore a sync | None | one full copy of the mirror per sync | **Bounded**: the ledger is the lean form (same digest whichever form the rows were in; schema 31 rewrites old ledgers and re-hashes the ones that still match); only the newest ten applied syncs of a target keep a journal, an older one is replaced by a short summary |
| `sync/rekordbox/operations/<id>/` | files: `before.db` (a copy of the rekordbox database), staged image, `build/`, `audio/` | `rekordbox_sync_service` | restore, crash recovery | kept for ever once applied | a copy of the rekordbox database per sync | **Bounded** with the journal: removed when the sync leaves the newest ten of its target, or at once when it was restored or rolled back |
| `sync/rekordbox/assets/<id>/`, `sync/<id>/` for Engine DJ, Serato, VirtualDJ, djay Pro, Mixxx, Pioneer USB | files: `staged/N`, `backup/N`, `journal.json`, `library.json` | `cuerithm-dj-sync` `transaction.rs` | restore a sync | applied journals stay for ever | one copy of every file written or replaced (media files for Serato, Engine DJ and Pioneer USB) | **Bounded**: `staged` and `backup` are freed for syncs beyond the newest ten of a target and for restored ones; the journal stays, History says the backup was removed and Restore is refused. A sync not yet acknowledged is never touched |
| `import_sources`, `import_snapshots`, `import_observations`, `import_blobs`, `import_items`, `import_bindings`, `import_*` (schema 30, added by "import and refresh DJ libraries") | tables | `import/` | one snapshot per inspection or refresh with every observed record as JSON (`import_observations.json`), content-addressed blobs, items and bindings | none: a refresh adds a snapshot and nothing prunes an older one | not measured (no import in the measured library); grows with the DJ library times the number of refreshes | **Not changed**: it arrived after this audit and its retention is its own decision. It does not use `native_records` or `native_track_links` content: the import reads only a link's identity and library track, so the lean form changes none of its decisions (`import_and_refresh_decide_the_same_from_the_full_and_the_lean_native_copies`). Follow-up: bound the snapshots kept per source |
| `native_reconciliation_plans`, `native_playlist_acknowledgements`, `native_playlist_bindings` | tables | native sync | reviews and ownership | bindings replaced on rebind | small | **Keep** |
| `sync_links` | table | no live writer | legacy baselines | none | small | **Keep** |
| `sync/baselines/*.json`, `sync/ownership/*.json` | files | `dj_sync_service` | id to hash maps per target | one per target, replaced | KBs | **Keep** |
| `organization-references/<op>/` | files | `organization_references` | recovery of DJ-library path references | none | DJ library files, not audio | **Keep** (reported in storage); outside this change |

## 3. Audio copies in the data folder

Every row here is a whole audio file kept in the data folder. After this change none remains once
the operation has settled.

| Write | Where | Created by | Read after commit by | Bound before | Verdict (shipped) |
|---|---|---|---|---|---|
| Automatic tagging (Incoming) | `tagging/cuerithm-tags-*/original.<ext>`, plus `art-original-N.jpg` for a cover file | `cuerithm-autotag` `write.rs` | nothing: automatic tagging has no Undo. Crash recovery and rollback use it only before commit | none | **Purged**: removed with the write's private folder when it settles; copies from earlier versions are removed once, 500 journals per start (`legacy_originals.rs`, only files inside the data folder, regular, not links) |
| Manual cleanup | `cleanup/cuerithm-tags-*/original.<ext>`; every Undo added `cleanup/cuerithm-undo-*/` with two more copies | `cleanup/mod.rs`, `artifacts.rs` | Undo copied the original bytes back | none | **Value-based Undo**: the journal records the tag values replaced; Undo writes them through the same checked write. The backup is deleted on completion; the ten newest applied batches keep their journal and older ones lose it |
| Manual metadata edit | `metadata-edits/staging/cuerithm-tags-*/`: two full copies per previewed track from Preview until apply, discard or supersede; `original.<ext>` kept after commit; Undo added `current.audio` and `restore.audio` | `metadata_edit/service.rs`, `journal.rs` | Undo of an edit to a finished track copied the original bytes back | previews retired when stale or discarded; committed backups never | **Value-based Undo**: recorded previous values, plus the replaced artwork as `metadata_artwork_assets` (artwork is allowed). The backup is retired once the artwork is kept; the ten newest saves stay undoable. An edit accepted for a track still in Incoming keeps its original only until the worker has written it: then the pictures it replaced become artwork assets and the audio is deleted (`retire_incoming_originals`, also run at open). Such a save counts in the same window of ten as every other save; an older one, or one that was undone, releases its pictures (`release_incoming_save`), and the Undo of an Incoming save keeps no audio of its own. Preview staging is unchanged (see section 8) |
| Duplicate merge with a donor tag copy | `maintenance/staging/cuerithm-tags-*/original.<ext>` (the keeper) plus `undo-*/` with two more copies | `maintenance/donors.rs` | Undo copied the original bytes back | none; archive-only merges never entered the ten-operation window | **Value-based Undo**: the keeper's tag snapshot before and after the copy is recorded and Undo writes it back; every committed merge, archive-only included, joins the ten-operation window |
| Reviewed unused-file removal | `maintenance/removal/remove-*/original-N` (every removed file and its sibling) | `maintenance/removal.rs` | Restore | none; disposal only on explicit user action. The file itself was **deleted permanently**, not sent to the Trash | **Purged**: the files go to the system Trash, no copy is kept, nothing is deleted unless the preview named the file as one the Trash cannot take. Restore and backup disposal are removed. Copies from earlier versions go to the Trash (restorable removals) or are deleted (restored ones), 50 removals per start |
| DJ-software sync (non-rekordbox) | `sync/<id>/staged`, `backup` | `cuerithm-dj-sync` | restore a sync | prepared previews only | **Bounded** (section 2) |
| rekordbox sync | `sync/rekordbox/operations/<id>/audio/*`, converted WAV copies in `sync/rekordbox/assets/<id>/staged` | `rekordbox_sync_service` | restore, recovery | prepared operations only | **Bounded** (section 2) |
| Decode scratch | `cache/media/cuerithm-*` | `cuerithm-media` | the decode in flight | cleaned at every start | **Keep** (transient) |
| DJ timing and fingerprint scratch | `cache/dj-timing/*`, `cache/fingerprints/*` | `dj_sync_service`, `maintenance/scan.rs`, `broken.rs` | the sync or scan in flight | `TempDir`, crash residue not swept | **Swept**: an entry untouched for a day is removed when the library opens (`cache_limits.rs`) |
| Staging directories orphaned by a crash between staging and the journal insert | `tagging/`, `cleanup/`, `metadata-edits/staging/`, `maintenance/staging/` | the writers above | nothing | none | **Not changed**: see section 8 |
| File organisation (Move to folder, Rename in place) | a hidden `.cuerithm-<id>-<n>.original` beside the user's file, in the user's music folder | `organization/coordinator.rs` | Undo of the batch and DJ-library path recovery | never deleted | **Not changed**: see section 8 |

Also on the user's music folder: while a write is in flight a dot-file `.cuerithm-<id>.tmp`
holds the displaced original. It is the live restore point and is removed after commit; it is not
an app-data copy.

## 4. Saved scans, reviews, previews and reports

All of these are rows of `maintenance_scans (id, kind, payload_json, created)`. Path byte arrays
serialise as JSON number arrays (about 0.35 KB per path). *Estimates* from the code, per track in
the library unless noted.

| Kind | Content | Written / read by | Needed by | Bound before | Verdict (shipped) |
|---|---|---|---|---|---|
| `duplicates` | every non-archived track with id, path, title, artist, hash, fingerprint and file proof, about 1.7 KB per track (50,000 tracks: about 85 MiB) | `scan.rs`; review creation | beginning a review | a removal deletes all, a relocation deletes all; a new scan adds another | **Bounded**: newest 3 (`maintenance/saved.rs`), never a scan a kept plan was made from; pruned when one is saved and at open |
| `duplicate_review` | a complete copy of the scan plus the groups, rewritten on every edit | `review.rs`; merge preview | the preview and the apply | purged on any removal | **Bounded**: newest 3, never a review a kept merge plan points to |
| `duplicate_merge_plan` | a third full copy | `merge.rs`; apply | apply | purged on any removal | **Bounded**: newest 3 |
| `broken` | about 0.8 KB per track | `broken.rs`; missing previews | previews | purged by id on a removal, deleted on relocation | **Bounded**: newest 3 |
| `unused` | every library path (as byte arrays) and a listing of every scanned folder | `unused.rs` | removal preview and apply | only relocation cleared it; **not** purged on a removal | **Bounded**: newest 3, and purged on any removal. Neither it nor `unused_removal_plan` is pruned while an unused-file removal is unfinished |
| `unused_removal_plan` | up to 10,000 files at about 0.6 KB | `removal.rs` | apply | kept after it was consumed | **Bounded**: newest 3 |
| `unused_removal_operation` | history of unused removals | `removal.rs` | recovery, History | never pruned | **Bounded**: the newest 20 settled removals keep a line (a path-and-hash list, no audio); one unfinished, or still holding a copy from an earlier version, is never dropped |
| `unused_backup_disposal*` | backup disposal journals and previews | `retention.rs` (removed) | Restore | never pruned | **Purged**: nothing creates them any more; leftovers are deleted at the first resume |
| `missing_removal_preview`, `missing_relink_preview` | previews | `missing.rs` | apply | pruned after a day on the next save | **Keep** |
| `missing_repair` | one history line per repair; removal lines trimmed to a summary by the ten-operation window | `missing.rs` | Undo | removals bounded | **Keep**: relink and Undo lines are small |
| `duplicate_merge`, `duplicate_merge_undo` | legacy path, no longer called by the app | `merge.rs` | none | none | **Keep** (legacy) |
| `maintenance_fingerprints` | one row per track per engine identity; a new identity per release left the old rows behind | `scan.rs` | duplicate scans | cascades on track delete | **Bounded**: rows of an engine identity other than the current one are deleted when a scan stores new ones |
| `maintenance_writes`, `maintenance_write_files` | duplicate-merge journals, with before and after track images | `donors.rs` | Undo, recovery | only removals left the window | **Bounded**: every committed merge joins the ten-operation window; a merge that leaves it drops its stored records and covers and keeps its history line |
| UI references | the last scan and review ids in browser storage | `MaintenanceShared.tsx` | reopening a view | n/a | a missing row already means "scan again" |

A cleanup never deletes: a prepared or conflicting write, a write with unfinished cleanup or an
unacknowledged rollback; the committed writes inside the ten-operation window and every History
line; the newest scans the UI still points to; a `duplicate_merge_plan` and the review it points
to; a live unused scan and its plan; `maintenance_track_epochs` and `maintenance_ignored_tracks`
of live tracks.

## 5. History and payload tables

| Table | Content | Bound before | Verdict (shipped) |
|---|---|---|---|
| `tag_writes`, `cleanup_writes`, `metadata_edit_writes` (+ `_files`, `_rows`, `_batches`) | journals with `analysis_json` (a second full analysis), metadata, receipts and path BLOBs | none; only a flag marks cleanup done | **Bounded** for cleanup (journals of batches beyond the ten newest are deleted with their private files) and metadata edits (`undo_expired`); tag-write journals stay as history, their `backup` path is cleared when the original goes. The second `analysis_json` copy stays: an interrupted write needs it |
| `cleanup_plans`, `cleanup_batches` | frozen plan with a proposal per track, result | none | **Bounded**: the newest 3 plans are kept (`prune_cleanup`); batches stay as a history line |
| `metadata_edit_drafts`, `_previews`, `_overrides` | drafts and previews, up to 64 MiB per row | previews retired when stale or discarded | **Keep**; the preview's staged audio is the transient cost in section 8 |
| `metadata_artwork_assets` + `metadata-edits/assets/<id>` | artwork bytes | never deleted | **Keep** (artwork is allowed); referenced by Undo, dropped when the edit leaves the window |
| `preparation_checkpoints` | evidence copy of the analysis and metadata (only when organisation is on) | completion only sets a state | **Keep** (outside this change) |
| `preparation_match_snapshots`, `_generations`, `_review_artwork` | match evidence, up to 2 MiB per row | per track, except review artwork of older generations | **Purged**: review artwork of every generation that no remaining track holds, on any removal |
| `preparation_analysis` | a second copy of the analysis per source hash and settings revision | removed with orphaned hashes | **Keep** |
| `playlist_edits`, `playlist_previews` | history diffs including member lists; previews expire on use | never swept | **Keep** |
| `organization_*` | plans, batches, operations, references | discard only | **Keep** (outside this change) |
| `curation_*`, `manual_publication_*` | publications and mutations, never pruned | none | **Keep** (small) |
| `relocation_plans` | 24 hours | garbage-collected at open | **Keep** |
| `artifact_cleanup_moves` | rows stay after they are removed | none | **Keep** (small) |
| `jobs`, `cue_revisions`, `accepted_musical_revisions` | analysis history | none | **Keep**; rebuilt only on request |

## 6. Caches

| What | Bound before | Verdict (shipped) |
|---|---|---|
| `cache/artwork/<source_hash>` (each up to 8 MiB) | removed when no track or removal still needs it | **Keep** (artwork is allowed; part of the cover kept for Undo) |
| `cache/waveforms/<key>.json` | removed for the two cached resolutions only, on a removal | **Bounded**: least recently used files go above 256 MiB, down to 192 MiB (a cache hit refreshes the file's time). A removal still removes the two cached resolutions of its content |
| `analysis-components/<sha>.onnx` | older hashes are never removed | **Keep**: one fixed bundled model |

## 7. Purge on removal

`maintenance/purge.rs` is the one list. The audit found these gaps, all closed:

| Gap | Fix |
|---|---|
| `maintenance_scans` kind `unused` names every library path and was not purged | purged on any removal |
| `preparation_review_artwork` of older generations stays | every generation that no remaining track holds |
| `tests/removal_engine.rs` exempted `maintenance_scans` and `maintenance_writes` from its trace check | the check covers every table that names a track; `every_table_that_names_a_track_is_accounted_for` enumerates the schema and fails for a table nobody listed |

Waveform files of resolutions the player does not ask for are not found by key (the key is a hash of
content, decoder and resolution); the size limit in section 6 covers them.

## 8. What this change does and does not do

Done: bounded and truncated log, reclaimable space, the lean DJ-software copy with a migration that
converts existing rows, value-based Undo and no audio copy once an operation settles, unused files
to the Trash, bounded saved scans, history, sync restore data and caches, and a purge test driven by
the schema.

Deliberately not changed, with the reason:

- **File organisation's hidden `.original` copies** (`Move to folder`, `Rename in place`). They sit
  in the user's music folder, are announced in the preview as *Estimated additional space*, and are
  what Undo of the batch and DJ-library path recovery read. Bounding them means deciding when a
  move becomes final; that needs its own decision. They are an audio copy outside the data folder.
- **Preview staging of manual metadata edits**: two full copies per previewed track from Preview
  until the preview is applied, discarded or superseded. They are the operation's own working files,
  retired when stale.
- **Staging folders orphaned by a crash** between staging and the journal insert. A sweep needs an
  exhaustive reference check against every journal; each holds at most one write's file, and there
  is no evidence of them accumulating.
- `tracks.analysis_json` (about 27 KB per track) and `preparation_analysis`: they are the product,
  not a copy of audio. A second copy in `tag_writes.analysis_json` stays because the journal needs
  it to finish an interrupted write.
- The four playlist and MyTag record tables keep their content in `native_records` (9.5 MiB for the
  measured collection): native playlist sync compares them with its bindings.
- Artwork, in the cache and in `metadata-edits/assets`.
- Library backups, retained library generations and the pre-restore snapshot: they are the user's
  own recovery data.
- `organization-references`, `curation_*`, `organization_*`, `preparation_checkpoints`: small, and
  a bound there needs its own review.
- The Undo limit is fixed at ten (`REMOVAL_UNDO_LIMIT`); there is no setting.

<!-- measured on a copy of one real library, read-only; sizes of tables without data are estimates from the code. -->
