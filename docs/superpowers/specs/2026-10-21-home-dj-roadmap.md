# Home-DJ workflow roadmap

Date: 2026-10-21

Status: feature scope and delivery order approved in conversation. The first
delivery design and implementation plan were also approved on 2026-10-21.
The six home-DJ workflows below are implemented on the feature branch, with
local validation recorded in the pull request. They have not been released.
Direct rekordbox writes are now enabled on the branch for exact macOS builds
6.8.7.0303 and 7.2.19.0342 after isolated application acceptance; other profiles
remain read-only. Final PR checks and release publication remain separate.

## Intended outcome

Make CueRithm useful throughout home preparation and practice: correct automatic
results, explore compatible tracks, understand a set's flow, build playlists from
reference tracklists, maintain a collection and recover preparation work.

## Existing baseline

VirtualDJ, Engine DJ, djay Pro, Serato DJ Pro 4 and Pioneer USB adapters and the
desktop Sync workspace are implemented. Do not rebuild these integrations.
Native application and physical-player acceptance remain distinct from their
implementation and fixture validation.

Direct rekordbox 6/7 synchronization now has accepted native macOS profiles,
reviewed field decisions and coordinated database/assets publication. See the
current acceptance matrix in `docs/research/rekordbox-direct-contract.md`.
Duplicate review, merge, donor transfer, missing/unused scans and recovery also
exist as backend operations and now have dedicated maintenance screens.

## Delivery sequence and completion criteria

1. **Finish direct rekordbox synchronization.** Reuse the existing direct-native
   contract and Sync workspace. Add field reconciliation, verified native writes,
   playlist/tag/rule mappings and recoverable publication. Require closed-target
   and reopen acceptance separately for supported rekordbox 6 and 7 profiles.
   See `2026-10-21-rekordbox-sync-completion-design.md` in this directory.
2. **Finish maintenance screens.** Connect the existing maintenance APIs to
   branded duplicate review, missing-file and unused-file screens. Preserve
   independent keeper/donor choice, exact-audio versus metadata-candidate labels,
   cancellation, partial/stale results, explicit reviewed removal, Undo and
   restart recovery. Missing-file detection must not promise automatic relocation
   that the current API does not provide; design any identity-checked relocation
   operation explicitly before exposing it.
3. **Manual musical correction.** Edit cue position/name/color/slot, saved loops
   and beatgrid anchors/BPM, including half/double corrections. Add reusable cue
   templates with previews and preservation of manual work. Keep raw measured
   results separate from manual publication, make changes durable and undoable,
   and verify preview/playback/sync use the same saved timeline. Preserve the
   validated detector calculations and all required audio formats.
4. **Find compatible tracks.** Choose a source track, BPM tolerance, key mode,
   optional whole-track energy range and genre/custom-tag restrictions. Offer
   explicit half/double-BPM inclusion, preview candidates and save selections to
   a playlist. Explain each result with the actual matching conditions. Unknown
   key, BPM or energy must not silently satisfy an enabled condition. Define
   deterministic ranking and missing-value behavior in this delivery's spec.
5. **Playlist flow overview.** Show ordered tracks with whole-track energy, BPM
   changes and adjacent key compatibility. Selection follows existing playlist
   and player behavior; unknown measurements remain visibly unknown. Describe
   compatibility as a preparation aid, not proof that two tracks sound good
   together. Avoid inferred transition timing or per-segment energy.
6. **Pasted tracklist matcher.** Accept pasted artist/title lines and text/CSV
   files, show parsed input, review candidate recordings/remixes and unmatched
   rows, then create an ordered local playlist from explicit selections. Preserve
   ambiguity; fuzzy title similarity is not recording identity. Start with local
   matching and avoid adding streaming-service dependencies to this delivery.
7. **Library backup and restore.** Provide dated, coherent backups of database
   preparation state and relevant settings, with an explicit music-file exclusion
   and location manifest. Validate compatibility and archive contents before a
   guarded restore; retain a recoverable pre-restore snapshot. Resolve external
   file-tag consistency and existing journal dependencies in the backup spec:
   restoring a database alone must not claim to reverse music-file edits.

Maintenance is one of the six approved improvements; it appears early because
the backend is already present. Direct rekordbox completion is the existing
requirement, not a seventh newly invented feature. Its native mappings, publication
and application acceptance remain incomplete; the six independent home-DJ
workflows proceeded while that work remains disabled.

## Delivery boundaries

Use a separate spec, implementation plan and reviewable change for each delivery.
Reuse current Rust-library, typed Tauri and React boundaries; preserve the current
brand and settings experience. Include updated user documentation and rendered
UI verification with each implemented workflow. Verify current-head tests,
formatting, strict lint and relevant builds; report native acceptance separately.

Keep music and installed applications intact during development; use isolated
generated collections and artifact copies. Publish aggregate findings and
authored fixtures only. Leave existing uncommitted work in the main checkout
untouched. Preserve automated release versioning.

Mobile, cloud/streaming expansion, plays/last-played history, danceability,
per-segment energy and renewed Mixed In Key cue inference are outside this
roadmap. Audio-device selection and general backup-retention management require
separate scope decisions rather than being silently folded into these features.
Backup retention was decided separately in issue 46: the library keeps no copy of
audio after a write settles, Undo and sync restore are bounded to ten operations,
and `docs/development/lean-library-audit.md` lists every other retention and its bound.

## Evidence and limits

The feature comparison inspected a copy of installed Lexicon 1.11.15. Bundled
implementation and help content establish static feature evidence, not observed
execution of every workflow or guaranteed subscription-tier availability.

Current CueRithm status comes from `docs/release-readiness.md`,
`docs/development/maintenance-api.md`, `docs/research/rekordbox-direct-contract.md`
and the current Sync interface. No new implementation or product acceptance is
claimed by this roadmap.
