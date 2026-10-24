# Release scope and outstanding gaps

Updated 2026-10-21. This is the release-stabilization scope requested after the
integration work. Finish and review the existing implementation before adding
features. The broader product plans remain open; merging this release does not
claim complete Lexicon or OneTagger parity.

## Included behavior

| Area | Current delivery |
| --- | --- |
| Incoming | Watched folders and imports run configured analysis, tagging and cleanup, replace cues, then move successful tracks into Tracks. Failed required stages remain actionable in Incoming. A file moved or renamed inside a watch folder is followed by the track it belongs to instead of becoming a second track. |
| Audio | Bundled native decoding and preview for MP3, AAC/ADTS, AAC/M4A, ALAC/M4A, FLAC, WAV, AIFF, Ogg Vorbis, Opus and WMA. |
| Analysis | Built-in OpenKeyScan key detection, local Lexicon-compatible timing and structural cues, saved grid/cue publication, and one compiled acoustic energy rating. Completed results change on explicit reanalysis. |
| Tagging | Reused OneTagger Beatport/Traxsource integrations with persistent provider, matching, field, artwork and format settings. Provider availability is subject to the limits below. |
| Cleanup | Eleven ordered Smart Fixes, genre/artist mappings, protected values, preview/apply and guarded persistent Undo for files and library together. |
| File organization | Opt-in Incoming naming/move/copy and manual complete-scope preview/apply/history/recovery/Undo use the same native coordinator. Required file/reference stages gate Ready; unchanged bytes retain analysis and playback snapshots. Native acceptance gates below remain distinct. |
| Match review and metadata | Source-bound catalog review, protected Incoming choices, typed common-field/artwork editing, durable drafts, guarded Save/Undo and recovery, with measured analysis/cues preserved. Temporary publication artifacts use native ownership and recoverable quarantine; unknown replacements remain visible for cleanup. |
| Curate | Stable categories, a separate user rating and notes are library-only by default. Durable desired revisions, background publication, pending/history and guarded Undo preserve later edits; optional file mappings use the metadata writer. |
| Preparation presets | Immutable portable rule revisions, local folder assignments and per-source capture extend Settings. Import/export excludes credentials, global transport and native destination grants; disconnected folders and destinations require explicit reconnection. |
| Playlists | Ordinary lists/folders, local smart rules, automatic membership refresh, tools/generation, preview and guarded Undo, with native desktop controls. |
| Maintenance | Native APIs for duplicate review (tracks whose file is not available are never grouped or merged; Missing & broken lists them), donor merge/Undo with a required choice between moving verified duplicate files to the Trash and archiving only (files the Trash cannot take are deleted permanently only after the preview names them; the Trash path has no automated test against a real Trash), missing/unused-file scans (Missing & broken can remove the records of files that are gone and relink moved files by exact content, never touching audio; its removals and the deleting merges share one engine and one Undo history of the newest ten), reviewed removal/restore, parent cleanup and explicit backup disposal. Dedicated maintenance screens now expose scanning, review, apply, Undo/Restore and recovery; final packaged/native acceptance remains separate. |
| DJ software Sync | Current VirtualDJ 8.5, Engine DJ schema 3, djay Pro, Serato DJ Pro 4, Pioneer USB and Mixxx (schema 39 and newer, by Mixxx's own compatibility marker) adapters and desktop Sync. Native smart export with whole-list fallbacks, timing conversion, owned playlist identity/deletion, typed directory/case-rename journals and recoverable apply/restore acknowledgement are present. Native DJ/player acceptance is separate. |
| Native rekordbox Sync | Direct desktop synchronization, explicit field/conflict decisions, native identities, database/assets publication, recovery/restore and playlist/MyTag previews. Exact macOS builds 6.8.7.0303 and 7.2.19.0342 are write-accepted after private application-created-library tests; other builds and Windows remain read-only. The five other DJ targets retain their Sync workspace. |

No collection XML transport is part of the product. Plays, Last Played and
Danceability and Mixed In Key segment-energy
or cue imitation are outside the approved scope.

Smart playlists can explicitly include Incoming tracks; ordinary Tracks,
manual playlist tables and automatic playlist generation keep their prepared
track defaults. Release Date rules use complete calendar-date tags under the
library's time zone. Partial dates and timestamp strings remain unknown rather
than acquiring an invented calendar day. See the
[smart-rule adapter policies](research/smart-playlist-parity.md).

The library upgrades to schema 27. Migration preserves existing tracks, jobs,
cue ownership and legacy sync links; older applications refuse this newer
schema. A database downgrade is not implemented. Completed analysis remains
unchanged until explicit reanalysis.

## Home-DJ workflow additions

Current source adds manual correction/template previews with durable Undo, compatible-track filtering, an ordered playlist flow view, text/CSV matching with explicit recording choices and atomic playlist creation, and same-installation library backup/guarded startup restoration. These additions preserve raw DSP results and source music. Local loops retain starts/ends and active-loop metadata, but local preview seeks rather than looping and active loops export to supported Engine DJ, VirtualDJ and Pioneer USB memory cues. Library restoration is separate from reversing external file-tag or DJ-software changes.

Direct rekordbox writes are enabled only for the two tested macOS application/schema pairs. Both passed private new-track, metadata/artwork/MyTag, cue/constant-grid/loop playback, playlist, no-op, recovery and guarded-restore checks. The public version 7 API additionally passed discovery/preflight/no-op apply/restore without changing native bytes or modification times. See the [accepted profiles and evidence limits](research/rekordbox-direct-contract.md#accepted-native-profiles).

## Outstanding work after this release

| Priority | Gap | Evidence needed to close it |
| --- | --- | --- |
| More native profiles | Native rekordbox Windows execution and additional application builds remain read-only. The editor publishes one constant BPM/first-beat anchor; native variable-grid publication needs separate acceptance. | Repeat the isolated application-created-library write/reopen/playback/conflict/recovery/restore matrix for each exact platform/build/schema. Physical player playback remains separate. |
| Native acceptance | Dedicated maintenance screens and confirmations are implemented with React workflow tests and authored screenshots. | Final packaged review/apply/Undo/recovery, removal/disposal confirmations and larger copied-library acceptance. |
| Analysis parity | Manual cue/loop/constant-grid corrections, named templates and custom anchors are implemented. Automatic preparation still uses factory publication rows; equal tempo-bound options remain unavailable. | Equal-input comparisons of remaining automatic settings paths, final packaged manual editing/Undo and saved target playback. |
| Provider acceptance | Direct Traxsource requests returned HTTP 403. A read-only native probe with the configured Trawl transport fetched and matched a live track including release metadata, and correctly returned no-match for an empty search. No end-to-end file write or packaged-app acceptance is established. Beatport has a native successful probe, not universal catalog coverage. | A real native Traxsource match/write and representative provider coverage checks without bypassing failed required stages. |
| Organization acceptance | Known linked references require path-only repair. Rekordbox references without a supported registered writer block organization; arbitrary external references cannot be discovered. Per-track native database backups can amplify storage and are included in conservative preflight. | Current-head native fixtures for publication/recovery/reference repair, plus separate native application reopen and large-library storage checks. Physical DJ player acceptance is not implied. |
| Metadata acceptance | Generated native fixtures cover supported formats, exact rating/comment/picture identities, staged interruptions, Incoming Move/Ready/Undo and unchanged PCM/analysis/cues. Synthetic browser captures validate both themes and compact layouts; controller tests cover draft flush and Quit receipts. | Final packaged native-window quit/reopen acceptance and broader representative user-library runs remain separate. Full binary-tag parity is not claimed. |
| Curate and preset acceptance | Local outbox, mapped publication and immutable source-rule capture have native regression coverage. Browser/controller checks cover both themes, compact layouts, complete filtered selections and local-save receipts. | Final packaged native-window restart and representative large-library acceptance remain separate. Local categories are not automatically exported as native DJ categories. |
| Energy acceptance | The local formula matches the recorded usable synthetic helper cases. Real-music scalar-energy agreement and exact WebAudio resampling equivalence remain unmeasured. | A frozen real-music comparison with identical inputs/configuration and separately reported missing/unrated results. |
| Analysis acceptance | Development-set timing/cue agreement does not prove unseen-track musical correctness or all cue configurations. | Independent held-out evaluation and broader native GUI comparisons, with raw detection and saved publication evaluated separately. |
| Platform acceptance | Hosted builds/tests and generated fixtures do not establish every native interaction on all desktops. No Intel macOS release artifact is configured. | Final Windows/Linux native playback, import/settings/restart/recovery acceptance and representative common-format checks; a separate Intel build if added later. |
| Scale acceptance | Full user-library maintenance, provider throughput, CPU/memory use and retained recovery-storage growth need larger-library measurement. Workers use 75% of available logical CPUs; that policy is not a proven optimum or utilization cap. | Reproducible large-library benchmarks and operation/recovery checks using copies. |
| Optional future work | All-In-One structural/deep analysis remains research, not an integrated optional desktop add-on. | Native model/frontend/separation/postprocessing integration and a separately accepted optional workflow. |

Preparation, manual cleanup and DJ Sync keep recovery copies only while an operation is
under way. Undo of a tag write restores recorded tag values; it needs no copy of the audio.
Sync history exposes guarded restore and interrupted-operation recovery for the newest ten
syncs of each DJ program (older ones keep a summary and report that their backup was removed);
Serato sync backups include affected source audio while those syncs are kept, and Pioneer
conversion protects the affected USB copies. Reviewed unused-file removal moves files to the
Trash. Disk-space claims must distinguish logical backup deletion from filesystem snapshot
storage. See `docs/development/lean-library-audit.md` for every retention and its bound.

The current application uses the existing CueRithm branding and Settings/table/player
surfaces. Feature work extends those surfaces without replacing the theme.

The [approved five-PR preparation program](plans/2026-10-20-002-feat-library-preparation-five-pr-delivery-plan.md)
tracks organization, match review/editing, Curate/presets and individually accepted
providers. Its implementation status is separate from this document's native
acceptance and release-publication gates.

## Acceptance evidence and deliberate differences

- [Automatic tagging](automatic-tagging.md) distinguishes deterministic provider
  fixtures from live-service results and documents the supported settings.
- [Cleanup](cleanup.md) and [behavior evidence](research/cleanup-behavior.md)
  describe preservation policies and source-helper differences.
- [Timing/cue comparisons](research/lexicon-beatgrid.md) record exact agreement
  on 282 development groups under one stated worker configuration. They do not
  establish a universal accuracy percentage.
- [Saved grid/cue publication](research/lexicon-publication.md) records the
  separately tested rounding/template boundary and the reported-track native
  acceptance check. Raw markers and published cues are different evidence layers.
- [Local energy](research/local-track-energy.md) documents the formula,
  synthetic checks and intentional unrated silence/short-input behavior.
- [Smart rules](research/smart-playlist-parity.md) and
  [playlist tools](research/playlist-tools-and-generation.md) document finite
  helper comparisons and explicit rule/rounding policy differences.
- [Maintenance API](development/maintenance-api.md) documents publication,
  recovery, disposal and the remaining UI handoff.
- [Curation and presets](development/curation-and-presets-api.md) documents
  desired/publication revisions, shared metadata journals and per-source settings
  capture. The [Curate](user-guide/tagging/curate-tracks.md) and
  [preset](user-guide/files/presets.md) guides describe the controls.
- [Direct Rekordbox contract](research/rekordbox-direct-contract.md) records the
  copied-data boundary. Initial per-field observations remain unacknowledged;
  storing an import baseline is not proof that either library was synchronized.

## Merge and publication gates

The release PR must include current-head local verification, the outstanding
limits above, resolved review findings and a completed Codex review of its
latest commit. Require the repository PR gate and Linux checks
before squash-merging; the other platforms run on main. Do not use an older approval or an earlier green run as
approval of a changed head.

After merge, [the release workflow](development/pr-workflow.md) builds and
publishes automatically only after every platform component succeeds. An explicitly declared
build repair may retain unaffected checks from a checked ancestor, with fresh results required
for the repaired components; see the linked workflow guide.
Check the resulting signing/notarization, publishing job and downloadable assets
before reporting a release as available. A merged PR or unsigned local app is
not publication evidence. No manual version bump or release tag is part of this
stabilization change.

## Native DJ sync

`crates/cuerithm-dj-sync` adds independent current-format adapters for VirtualDJ,
Engine DJ, djay Pro, Serato DJ Pro 4, Pioneer USB and Mixxx, with a desktop Sync workspace.
Mixxx writes its SQLite library directly in an Add only or Manage library mode, can
create a library where Mixxx has none and replace a whole one, and was run as the
installed Mixxx 2.5.6 on macOS (168 checks passed, and 72 more for other formats) and as Debian's Mixxx 2.5.0 on Linux in a container (105). Its storage contract, measured MP3
alignment and the unverified AAC, Windows, Linux and newer-Mixxx gaps are recorded in
`docs/research/mixxx-sync-contract.md`.

Mixxx feature coverage (tests are the evidence; the installed Mixxx 2.5.6 on macOS adds
the runs noted):

| Capability | Status |
| --- | --- |
| Tracks, metadata, playlists and crates | Covered. Adapter and service tests for both containers; the installed Mixxx ran with playlists |
| Hot cues with names, colours and saved loops | Covered. Slots, names, exact RGB, loops and the rows Mixxx's loader would discard are tested; the installed Mixxx ran with names, positions and a loop |
| Beatgrids with correct timing across audio formats | Covered for WAV, FLAC, MP3, AAC, ALAC, AIFF, Ogg Vorbis and Opus in the installed Mixxx 2.5.6 on macOS and in Debian's Mixxx 2.5.0 on Linux (MP3 through libmad, measured with a libmad probe; the AIFF, Ogg, Opus and FFmpeg-AAC click positions come from FFmpeg's decode). WMA cannot be opened by either. Windows was not run, and Linux builds with FAAD were not either, so those AAC cases stay open |
| Preview, conflict handling and recoverable backups | Covered. Preview, stale-preview, running-Mixxx, half-written-library and ambiguous-match refusals, and byte-identical restore after update, create and replace are tested |
| Protection for changes users make inside Mixxx | Covered. Add only never touches existing tracks, Manage replaces only the chosen groups, edited playlists are reset or appended to, hidden and locked rows and a user's scan folders are untouched, and the staged copy is verified before publishing |
Implemented behavior includes measured export timing, supported native smart-rule
compilation with exact whole-list fallbacks, durable playlist ownership and owned
removal propagation, batch staging, typed file/directory/case-rename journals and
recoverable apply/restore acknowledgement. Extra prepared metadata and explicit
calendar/key-mode context reach the compiler. Older Lexicon formats are excluded;
direct rekordbox uses its own reviewed desktop workflow and the exact accepted
macOS profiles documented above.

Targeted final artifact review has resolved ANLZ path collisions, hidden catalog
discovery, dotted VirtualDJ names, Serato explicit null clears, djay filename indexes
and raw Engine BPM precision. Current-head validation and integration evidence are
recorded in [PR 12](https://github.com/johnneerdael/CueRithm/pull/12).
See [format evidence](research/dj-software-formats.md),
[timing](research/dj-sync-timing.md), [ownership](research/dj-sync-playlist-ownership.md)
and [smart-rule policies](research/dj-sync-smart-rules.md). Native application/device
acceptance and release publication are separate evidence layers; this implementation is
not a declaration of shipped cross-application parity.
