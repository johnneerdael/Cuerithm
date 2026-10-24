# Metadata cleanup

Cleanup uses the same saved rules for manual changes and automatic Incoming
preparation. Set automatic preparation in **Settings → Cleanup**. It runs after
configured catalog tagging and before a prepared track enters Tracks. Changing
settings does not rewrite completed tracks.

## Rules

Add Smart Fixes in the order they should run. Each rule has an enable switch and
an exact-value ignore list. The eleven operations cover casing, character
replacement, encoded characters, artist/remixer extraction, garbage, number
prefixes, URL/email removal, mix parentheses, common title text and promotion.
Field selectors apply only to human metadata, never analysis, paths or cues.

Genre and Artist cleanup use exact mappings. Add aliases, choose a canonical
spelling or remove a value. Protect names that should remain unchanged. Artist
roles include artist, remixer, producer, composer and lyricist. Names containing
commas remain one native value when the file stores them that way.

Genre conversion can retain selected source detail in categorized custom tags.
Choose the category and source genres; optionally remove the converted values
from Genre. These categorized tags are stored in Cuerithm's library. Original
provider metadata remains in preparation provenance.

Settings and manual Cleanup explain invalid configurations before saving. Finish
new mapping rows, keep at least one role for enabled Artist cleanup, and correct
the indicated value or limit. Disabled drafts follow the backend's validation
rules; protected and ignored values are still checked.

## Review and apply

Open **Cleanup** in the sidebar. Its Genres and Artists tabs list native values
with distinct affected-track counts. Search or sort by name/count; contributor
values also show counts by role. A legacy track whose source cannot be read is
reported as unavailable.

Choose all ready tracks or the selection captured when opening Cleanup. Save
rule changes, then scan. Scanning reads the selected files and persists a preview
without changing audio tags or track metadata. Review before/after fields and
categorized tags, uncheck unwanted proposals and apply the remaining selection.
Editing rules or scope discards the old preview.

Apply updates file tags and Cuerithm together, with a durable original backup that
is kept only until the write has settled.
It preserves full decoded PCM, key, BPM, beatgrid, track energy and existing cue
history. It does not perform new musical analysis. Unrepresentable metadata,
changed files or concurrent track edits are reported per track; other selected
tracks can still complete. Conflicted tracks need a fresh scan.

## Undo and recovery

Undo history survives restart, including outcome counts and expandable
per-track conflict/failure messages. Track names are loaded when a report is
opened; stable IDs remain visible if the track is unavailable. Undo restores the tag
values recorded before the apply (not a copy of the audio) and the library metadata,
if the current post-apply identity still matches; it writes them through the same
checked write as the apply, so the audio stays what it is. Newer edits are preserved
and reported as conflicts. Fully undone batches no longer offer Undo. Only the newest
ten applied batches stay undoable (`REMOVAL_UNDO_LIMIT`); an older one keeps its
history line and refuses Undo.
Recovery runs before preparation workers start and preserves later user bytes.

Settings shows recovery storage from both preparation and manual cleanup. That is
only the copies of writes that are still under way or waiting for cleanup; a settled
write holds none. Redundant staged files are cleaned after durable publication.

## Validation boundary

The frontend and native configuration validators share 99 original synthetic
cases in `tools/fixtures/cleanup-contract.json`, covering Unicode, byte/scalar
limits, enabled/disabled settings, exact mappings and cycles. Additional frontend
tests cover large collection limits and the visible save/error flow. These
finite checks supplement the backend's authoritative validation.

Source-helper comparisons, intentional data-preserving differences, native
format checks and concurrency/recovery tests are documented in
[Cleanup behavior evidence](research/cleanup-behavior.md). These establish
specific behavior; they do not claim exact Lexicon parity for every input.
Duplicate merging, unused-file cleanup and missing/corrupt-track tools have a
separate [native maintenance API](development/maintenance-api.md). Their dedicated
screens and full-library acceptance remain outstanding; see
[release scope](release-readiness.md).
