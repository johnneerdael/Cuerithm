# Maintenance native API

This is the backend handoff for the user-owned UI. Import `nativeMaintenance`
from `apps/desktop/src/maintenance-api.ts`; its `MaintenanceBridge` type and
`maintenance-types.ts` define the requests and results. No styling or navigation
changes are included in this checkpoint.

## Scanning and cancellation

Create a fresh `crypto.randomUUID()` for every long operation, retain it for
cancellation, and subscribe to progress before starting. Filter progress by both
`operation_id` and `kind`. Unsubscribe when the view leaves; ignore late results
from an older operation. One maintenance operation runs at a time. Cancel it
with `cancel(operationId)` and wait for its original promise to settle before
starting another; cancellation acknowledgement does not release the slot.

- `scanDuplicates(operationId, request)`: full library, selected playlists, or
  full-library groups intersecting selected playlists. Specify all request fields.
  The `method` distinguishes exact decoded audio from artist/title candidates.
  Exact grouping requires matching native PCM digest, sample rate and channel
  count. Rescan reports made with an older scanner implementation before review
  or merge; raw fingerprint caching remains separate from classification.
  A track whose audio file is not available is never grouped, counted as a
  duplicate, offered as keeper or donor, or merged, whatever the match method.
  Availability is the file-state classification `scanBroken` uses: only `present`
  and `duration_skipped` (present, too short or long to fingerprint) are
  available. `skipped_unavailable` lists the tracks left out (`id` and `state`)
  so the view can send the user to Missing & broken; their rows stay in `tracks`.
- `scanUnused(operationId, request)`: explicit folder with all/include/exclude
  extension filters. Filters choose removal candidates; sidecar association uses
  the complete regular-file inventory, including excluded base audio files.
  Show `complete` and `issues`; partial is not a complete scan.
- `scanBroken(operationId, {inspect_audio})`: availability or strict decoding.
  Preserve distinct missing, unavailable, unsupported, timed-out and decode-error
  states. Presence alone does not establish decodability.
- `duplicateScan`, `unusedScan` and `brokenScan`: reload a persisted report using
  a fresh operation ID and its report ID. Reload rechecks source proofs and
  revisions; always surface `stale` and `stale_reason`.
- `exportUnused(operationId, reportId)`: return the path-list text for the report.
  This writes no disk file and performs no removal. Retain report/staleness context
  when offering its text to the user.

Native errors use `cancelled`, `maintenance_busy`, `worker_failed` or the existing
`operation_failed` code and a message. App shutdown cancels active maintenance
work and waits for the worker to release its slot. Scans do not change track
metadata, audio files, cues, archives, preparation jobs or playlist membership.

## Duplicate review

1. `beginReview(operationId, scanId)` creates a review from a fresh scan.
2. `getReview(operationId, reviewId)` reloads its current choices and rechecks
   source/scope proofs. Choices survive restart.
3. `editReview(operationId, reviewId, expectedRevision, edit)` applies a group
   edit or a batch preference. Use the revision from the last successful response.
   On a revision conflict, reload before editing again. On stale inputs, rescan.

Group edits select a keeper, set/clear the independent tag/cue donor, lock,
unlock or toggle a track ignore. Unlocking a locked group with a donor enters
`copy_cues`; forced unlock enters normal merge selection. Selecting the keeper
as donor clears the donor. Preferences optionally override locks and select a
separate best-cue donor when they change the keeper. Streaming/play statistics
are outside the approved scope.

An ignored selected keeper remains visible; it cannot be applied. Manual
unavailable or ignored keeper/donor choices are rejected. A review's own ignore
edit adopts the resulting ignore revision; other reviews/scans remain tied to
their earlier inputs and become stale. `onChange` lets other open views reload.

`ignores()` provides a coherent stamped list independent of any review.
`setIgnores(trackIds, ignored, expectedStamp)` can restore ignored tracks even
when their original review is stale. All IDs must still exist; unknown IDs or a
stale stamp reject the entire edit. Reload the list after a conflict.

## Merge preview and history

`previewMerge(operationId, reviewId, expectedRevision, groups, disposition)`
persists a concrete plan for the selected group IDs. `disposition` is required and
has no default: `"archive"` archives the other records and leaves every audio file
where it is; `"delete"` moves each verified duplicate audio file to the operating
system Trash or Recycle Bin and removes its library record. Without a choice there
is no preview, so nothing can be applied. The preview checks current review,
library, source and scope identities, and re-checks that the audio file of every
participating track is still available (a vanished file refuses the preview with
a message that names the track and points to Missing & broken). Its groups retain
the mode and exact-audio/candidate distinction, keeper, `archive_ids` or
`delete_ids`, unarchive actions and explicit donor-to-target copy tasks. Playlist
changes include ordered before/after membership arrays.

A `"delete"` preview also returns `deletion`: every file that will be moved
(`display_path`, `size_bytes`, the keeper it duplicates and whether it is the same
file or the same decoded audio), every file that will be left alone with its
reason, the library records that leave with each file, and `undo_limit`. A file is
listed only when it is available, unchanged since the scan, proven identical to the
kept track (equal SHA-256, or equal decoded-audio digest, sample rate and channel
count), not the keeper's own file under another name, not used by any other
remaining library record (symbolic links, hard links and path aliases included) and
not reserved by another operation. Artist/title candidates that are different mixes
are therefore never deleted. Sidecars and cover images are never touched.

Each listed file carries `permanent`: `true` when the Trash cannot be expected to take
it (a network share or read-only volume on macOS, or a volume where the Trash refused a
file earlier in the run). The view lists those files on their own line and names them on
the confirm button. Only those files are ever deleted permanently, and only after the
Trash has been tried and has refused them; a file the preview announced for the Trash
that the Trash then refuses stays where it is with its record, and a new preview lists
it as permanent. `FileTrash::can_trash` only reports and never changes anything.

When a delete merge removes records it uses the same removal engine as Missing & broken
(see [Removing library records](#removing-library-records)): what is saved for Undo, what is
purged instead, what Undo checks and how many removals stay undoable are defined once and
are identical for both. A delete merge also refuses a track with unfinished work (an unfinished
file organization, tag write, cleanup, metadata save, category save or preparation), the same
guard Missing & broken applies. That guard also covers an open metadata draft: a track listed in
a draft that is not saved, discarded or undone (`track_removal::in_open_draft`, the one definition
Missing & broken uses too) is never deleted, because the draft keeps the ids it was made for and
previewing it fails at the first one that is gone, which would break it for the tracks that remain.
The reason reads *Part of an open metadata draft; save or discard the draft first*. A draft that
lists only the kept track does not block, since that record stays and the draft remains usable, and
an archive-only merge is never blocked by a draft. The same check covers the file and the track that
keeps the audio: a kept track that is still being prepared or has unfinished work, or either file
reserved by another operation's prepared write, leaves the duplicate where it is (`busy`). It runs when the preview is made and once more
inside the write transaction, immediately before each file is disposed of. Donor staging can take
minutes, and a tag, cleanup, metadata or organization write that starts in that time would
otherwise be removed together with the record, silently abandoning the operation. The merge's own
journal does not count as a reservation. The transaction is an `IMMEDIATE` one that holds the write
lock until the commit, so nothing new can start between that look and the Trash or permanent
deletion that follows; the identity checks below come after it. A relocation or portable restore
holds the library's exclusive lease; every other library access, including the merge's next one,
then fails with `LibraryBusy` before any file has moved (the journal it had reserved is rolled
back by the usual recovery), and the lease cannot be taken while the merge's transaction is open.

`mergeHistory()` returns committed transaction records and `undone_by` markers,
ordered newest first, with `undo_expired` and `undo_limit` (the shared
`REMOVAL_UNDO_LIMIT`, ten). These are history records; they do not promise that undo
remains available after later source or library changes.

## Coordinated publication and undo

`applyMerge(operationId, previewId, confirmDeletion)` runs the complete
donor/archive/reference coordinator. A preview that deletes files is applied only
with `confirmDeletion: true`, which the view sends only from its explicit "Delete N
files and merge" action, and only for the files the preview listed. It stages all selected recipient files and validates their native
metadata, decoded PCM and source identities before reserving publication targets.
Durable backups and a prepared journal precede any exchange. Metadata, custom tags,
manual cue revisions, grid publication, archives, playlist membership, consumed
preview token and committed journal share one SQLite commit. The raw recipient
analysis remains intact; its presentation publication can reflect the imported
cues/grid with manual-import provenance. An archive-only merge removes no music
files. A delete merge moves files to the Trash only through the
`FileTrash`/`SystemTrash` boundary (`NSFileManager` on macOS, the Recycle Bin on
Windows, the freedesktop.org Trash on Linux). It deletes permanently only a file the
preview announced as `permanent`, after the Trash refused it; any other file the Trash
cannot take stays with its record and is reported per file.
Before each file moves, its availability is checked again. The kept track's file is hashed
and must still be what the deletion was approved against (`keeper_sha256` in the preview, or
what a donor copy made of it); then the copy is hashed through one open file, which also gives
its identity (device and inode, or the Windows file id, with size and modification time).
Immediately before the Trash call, and again before a permanent deletion, both paths must still
lead to those very files, unchanged. A copy swapped in under its name keeps the duplicate with
*The file changed after it was verified.*, and a survivor that was replaced or edited keeps it
with *The kept track's file changed after it was verified.* The Trash and permanent deletion act
on a path, so the check and the call are not one atomic step: a change in that instant is not
excluded. The two hashed files stay open until the call returns; on Windows the Recycle Bin call
with those handles open has not been tried, and Delete from disk needs one manual run on a real
Windows machine before a release that includes it. The library records only the files that
really moved. If the library commit fails after files were moved, the operation is rolled back
and its error lists the files that are already in the Trash; their records were kept, so they
appear in Missing & broken. A restart after an interruption lists the same files.

`undoMerge(operationId, operationIdToUndo)` uses a new reverse file journal and
restores the captured library/cue/tag/member state. The library keeps no copy of the
audio: a donor tag copy records the keeper's tag values from before and after the copy
(`transfers`), and Undo writes the "before" values back through the same checked transfer
that made the copy, so the audio stays what it is. It refuses (`Later tag edits prevent
Undo`) when the file's tags are no longer the recorded "after" values. A merge that was
made by an earlier version and still holds a retained original is undone from that copy
once; the copy is deleted when the merge is undone or leaves the Undo window. For a delete
merge it restores the removed library records (every row that
depends on them, found through the schema's foreign keys), their playlist positions
and metadata; it cannot take a file out of the Trash. The removed records are kept
only for the newest ten removals (`undo_limit`), duplicate merges that delete files and
Missing & broken removals counted together; older entries keep their summary, report
`undo_expired` and refuse Undo, so the removal is final. Archive-only merges and every
other committed merge count in the same window: a merge that leaves it frees its recorded
tag values and the covers it holds. Undo is refused, before any
file is staged, when a removed track's identity or file location is already used by
another track, or when a different file now sits where a removed track's file was.
`links_not_restored` counts the sync and DJ-program link rows that were left as they are
because something changed them after the removal. Use `mergeHistory()` to discover operations after restart. Track/playlist
entity epochs reject relevant edits and A/B/A cycles. Unrelated Incoming results
and derived smart-list refreshes do not invalidate a version2 undo. Older version1
archive-only history retains its original conservative undo contract.

Both operations emit `library-change` and `maintenance-change` after success.
`updated_ids`, `archived_ids`, `restored_ids`, `removed_ids` and
`affected_playlists` identify what to refresh; `trashed` and `not_deleted` give the
per-file outcome of a delete merge. Reload tracks and playlist state after the event; do not infer
publication from a staging/progress event. Copy-only mode imports selected tags,
cues and grid without archive/reference actions.

App startup rolls back interrupted prepared operations. An interrupted undo
returns to the previously committed after-state and remains retryable. Later user
file edits are preserved and recorded as recovery conflicts with backups retained.
`recovery()` lists pending/conflicted operations and incomplete recovery cleanup, and a delete
merge that was rolled back after files had already left their place (state `rolled_back`, `error`
naming the files; their records were kept) until `retryRecovery` acknowledges it. Every row has
`files_moved`, true while such an operation is unacknowledged: show **Acknowledge** only for a
`rolled_back` row with `files_moved`, and **Retry merge recovery** for every other row (a rolled-back
row that only waits on cleanup, or a `conflict` row, is retried and must not be dismissed);
`retryRecovery(operationId, journalId)` retries guarded rollback/cleanup after the
user resolves the conflicting source/location. It cannot overwrite an unknown
later file version. Show these errors rather than presenting a partial merge as
successful. Cancellation propagates through hashing, decoding and tag staging;
once exchanges have started, rollback finishes before the operation settles.

Schema8 permits manual cue revisions with a null `job_id`; it preserves older
analysis-job cue ownership. A no-tagging build refuses pending file journals so
it cannot skip required recovery. Native tag, cleanup and duplicate writes share
target reservations and the publication gate. Source/scope/review/epoch checks
remain server-side.

Generated tests exercise all fourteen audio layouts, SQL failure, restart at
prepared/exchanged points, interrupted undo, subsequent file edits, relevant ABA,
unrelated Incoming changes and migration. A separate packaged macOS test app
exercised real invoke Apply/Undo, cue/grid display through track details, history,
playlist replacement/restoration and zero pending recovery on generated WAVs.
These checks do not establish branded maintenance-screen or full user-library
acceptance. The user-owned UI can now connect its review/apply/undo controls to
these commands. Reviewed disk-removal APIs are described below; their dedicated
screens remain unfinished.


## Removing library records

A duplicate merge that deletes files and a Missing & broken removal take track records
out of the library through one engine, so they cannot differ. `record_snapshot.rs` finds
everything that belongs to a track through the schema's own foreign keys (a table added by a
later migration is covered without changing it) and `maintenance/track_removal.rs` drives it.
Only rows are involved; the engine never reads, moves or deletes an audio file or sidecar.

- **Saved for Undo.** The track record and every row that depends on it (cues, tags, links,
  job, tagging and metadata history, curation state, ignore marks), as data keyed by declared
  columns. SQLite row numbers are neither stored nor restored, so every table needs a declared
  primary key (a test enforces it). A row that only optionally refers to a removed row stays
  and has the reference cleared; the rekordbox track link is the case, and it then reads "not
  in local library". Playlist positions are rewritten by the caller before the records go and
  saved with the history entry.
- **Purged, not saved.** Rebuildable per-track rows (`maintenance_fingerprints`,
  `smart_playlist_members`, `curation_native_stale`, `preparation_reuse`,
  `preparation_match_previews`), the track's change-bookkeeping row, review artwork of its
  match generations, cached analysis results, waveform cache files of content no remaining
  track or job shares, and every saved duplicate scan, review and merge plan plus any file
  report or repair preview that names a removed track. A derived table is kept in the saved
  image only if a saved row still refers to it. `maintenance/purge.rs` is the one list.
- **The cover is kept for Undo.** With the audio gone (missing, or in the Trash) the cached cover
  can be the only copy of the picture, and Undo brings the record back without it. The removal's
  saved record names the covers it holds (`artwork_hashes`); they are deleted when that record is
  dropped (it leaves the window, or `discardUndo`), unless a track or another undoable removal
  still needs the same content. Undo itself keeps them: the restored track uses them again.
- **What Undo checks first.** Nothing is changed unless every removed track's identity and file
  location are still free and any file sitting at an old location has the content the record
  describes (for example the file restored from the Trash). Hashing happens before any lock is
  taken; the write transaction only re-checks the file's size and modification time. A key now in
  use or a parent that is gone refuses Undo as a whole. A sync link, DJ-program ownership row or rekordbox link that
  something changed after the removal is never overwritten or taken over: it stays as it is now
  and is counted.
- **One bound.** `REMOVAL_UNDO_LIMIT` (ten) newest removals keep their saved records,
  duplicate merges that delete files and Missing & broken removals counted together
  (`DUPLICATE_UNDO_LIMIT` and `MISSING_UNDO_WINDOW` are the same value). Older removals keep
  their history line without the records and without track details; Undo is refused. An undone
  removal frees its place. A Missing & broken removal can also be discarded explicitly
  (`discardUndo`).

## Missing & broken repairs

`scanBroken(operationId, {inspect_audio, include_archived})` lists tracks whose files are gone;
archived tracks are included on request. The repair bridge (`missing-repair-api.ts`,
`MissingRepairBridge`) acts on a fresh report. Every call takes a new operation ID and follows
the same cancellation protocol as the scans.

- `previewRemoval(operationId, scanId, selected)` returns a `MissingPreview`: per-row outcome
  (`ready`, `blocked` with a reason), the playlist memberships, sync links and rekordbox links
  that leave, archived and unreachable counts, and `undo_limit`. Only a file that is definitely
  gone can be removed; a reachable file, a track with unfinished work or an open metadata draft
  is kept. `applyRemoval(operationId, previewId)` re-checks every row and removes only the
  records. **No audio file, sidecar or cover image is ever touched.** A row that changed since
  the preview is skipped with its reason.
- `previewRelink(operationId, scanId, selected, locations)` hashes the files in the chosen
  locations and pairs a track with a file only when its bytes equal the stored source SHA-256
  and exactly one free file matches. `applyRelink` hashes again, then points the track at the
  file in place (same id, cues, playlists, links and history; owned artwork rows follow).
- `history()` lists repairs newest first (`undone_by`, `undo_discarded`, `undo_limit`).
  `undo(operationId, id)` reverses one removal or relink as a unit, or changes nothing and says
  why; a relink Undo verifies the audio hash and the old location before changing a path.
  `discardUndo` frees the saved records of a removal that was not undone. An Undo result may
  carry `notes`: for a relink whose file is still inside a watch folder it says the next scan
  will follow the file again.

A file moved inside a watch folder is followed before it can become a second track: when a
discovered file has the content of exactly one ready track whose own file is gone from the same
watch folder, that track's path is updated (`relinked` stage event) instead of admitting a
duplicate. Ambiguous cases, such as true duplicates, are left to Duplicates and Missing & broken.

## Reviewed unused-file removal

Use `previewUnusedRemoval(operationId, scanId, selectedIds)` with IDs from a
complete, fresh unused-file report. The persisted preview returns the exact
selected files, the first present Lexicon-style Serato/VirtualDJ sibling, hashes,
roles and total bytes, and marks each file `permanent` when the operating system
Trash cannot be expected to take it (`FileTrash::can_trash`: a network share, a
read-only volume, or a volume where the Trash already refused a file in this run). It
accepts no arbitrary source paths. Show all preview entries, including siblings, and
name the `permanent` ones, before the explicit removal action.

`applyUnusedRemoval(operationId, previewId)` moves the files to the Trash or Recycle Bin.
**The library keeps no copy of any file.** It rechecks the scan, current references
(including archived records and physical aliases), source identities, root containment and
shared destination reservations, then works in two journaled steps:

1. Each file is moved, same volume and no replace, to a hidden `.cuerithm-unused-*.tmp`
   name beside it, and the journal (paths, identities and hashes only) is committed
   together with the consumed preview.
2. Each file is then given its own name back (a free spare name such as
   `name (removed 1).ext` if something else took it) and handed to `FileTrash`, one at a time.
   The journal records the name before the rename and the result after the Trash has
   answered. Immediately before the Trash call the file must still be the approved one
   (same file on disk, same hash); a file edited in the meantime is left where it is.

The result lists one outcome per file (`outcomes`): `trashed`, `deleted` (permanent,
only for a file the preview marked `permanent`, and only after the Trash refused it),
`left_in_place` (the Trash refused it and it was not marked: the file is where it was,
with the reason) or `gone` (no longer where the removal put it when an interruption was
resumed). There is no Restore: the user puts a file back from the Trash with the operating
system.

`unusedRemovalHistory()` lists operations and their outcomes after restart; the newest
twenty settled removals are kept, older ones are dropped (a removal that is not finished,
or that still holds a copy an earlier version kept, is never dropped).

Interrupted prepared operations roll back when the library opens. A later user file stays
in place and yields a visible conflict with the captured source retained. A removal that
had already committed is finished by `resume_unused_removals(trash)`, which the host runs
after the library has opened (opening alone never touches the Trash); it is also what
`retryUnusedRecovery(operationId, journalId)` does for one operation. Unresolved recovery
reserves its source against another removal, tagging, cleanup or donor publication.
Never hide `state`, `error` or `cleanup_pending`.

Previews include `directories` and explicit `folder_metadata` entries when parent
cleanup would remove `.DS_Store` or `Thumbs.db`. Folder planning follows selected
primary-file/sidecar order. It preserves cover art and the reviewed root. Cleanup
removes only reviewed empty parents once their files are gone, never recurses, and
leaves a parent containing new files (or a file the Trash refused) alone.

### Copies kept by earlier versions

Earlier versions deleted the file and kept a copy in `maintenance/removal/remove-*/original-N`
for a Restore button, with a separate explicit "disposal". Both are gone. `resume_unused_removals`
also moves what is left from those versions, at most 50 removals per start: the copy of a
removal that still stands goes to the Trash under the file's own name (it is the only copy
left, so it is not deleted); the copy of a removal that was restored or rolled back is deleted
(the file is back in place). A copy is found by its content hash, not its name, and only a
regular file inside the removal folder of the data folder is touched (never a link). A copy
the Trash refuses stays for the next start. Half-done disposal journals are dropped. The
journals of those removals stay readable (`unused-removal-v1` and `-v2`).

## What the library keeps

The library holds tags, links to files and artwork, never audio. This is what is kept beyond
that, and for how long. `docs/development/lean-library-audit.md` has the full table with sizes.

| What | Bound |
|---|---|
| Undo of a removal (duplicate merge that deletes files, Missing & broken removal) and of a merge | the newest ten (`REMOVAL_UNDO_LIMIT`), all merges counted; the saved records and covers are freed when one leaves |
| Undo of a manual cleanup | the newest ten applied batches; it restores recorded tag values, written through the checked write path |
| Undo of a metadata edit | the newest ten; recorded values plus the replaced artwork (artwork is allowed), no audio |
| Original audio of a tag write | only while the write is under way; deleted when it settles. Earlier versions' copies are deleted once, 500 journals per start, only files the application created |
| Saved scans, reviews and previews (`maintenance_scans`) | the newest 3 of each kind, never one a kept plan or an unfinished removal still needs; pruned when one is saved and when the library opens |
| History of unused-file removals | the newest 20 settled |
| Rekordbox and other DJ-program syncs | restore data for the newest ten syncs of each target; an older one keeps a summary (`restore_available: false` / `restore_expired`) |
| DJ-library record copies (`native_records`, `native_track_links`) | a hash for every record; the content only of the four playlist and MyTag tables and of tracks linked to a library track |
| Caches | waveforms and cover art are rebuilt on demand and removed with the content they belong to |
| SQLite write-ahead log | `journal_size_limit` 16 MiB, autocheckpoint every 1000 pages, a `TRUNCATE` checkpoint when the library opens, after a large import and on a clean exit |
| Free pages in `library.sqlite` | new libraries use incremental auto-vacuum; an older one is converted once by a single `VACUUM` when it opens (only when the live data is up to 512 MiB, about a second, and the disk has room for three times that plus 256 MiB; `reclaim_storage` converts up to 2 GiB on request, and no app command calls it yet), then freed in bounded steps |

Removing a track record purges everything derived from it (`maintenance/purge.rs` is the one
list; `tests/removal_engine.rs` enumerates the schema and fails for a table that names a
track and is neither saved for Undo nor purged).
