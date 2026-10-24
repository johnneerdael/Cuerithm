# What CueRithm does not do yet

<p class="lead">CueRithm is early software. This page lists what is missing, what is limited and what is in development, so you can tell a gap from a fault. Reading the whole page takes about five minutes.</p>

Check the release notes of your download for the features it includes. For what has been tested in each DJ program, see [Check what works with your DJ software](compatibility.md).

## Not in the app { #not-in-the-app }

### Unrecognised rekordbox formats { #rekordbox-formats }

Compatible rekordbox 6 and 7 builds can be written to on macOS, Windows and Wine / Proton. A database or asset format that CueRithm does not recognise stays **Read only**. Collection XML transport is intentionally excluded. Pioneer USB export is a separate drive workflow. See [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md) and [My sync does not show up in rekordbox](troubleshooting/rekordbox-sync-problems.md).

### Traktor and legacy Serato { #traktor-serato }

Sync to Traktor, and to Serato versions before Serato DJ Pro 4, is not available. Serato DJ Pro 4 sync is included ([Sync to Serato DJ Pro](../sync/serato-dj-pro.md)).

### Standalone playlist import and export { #playlist-import-export }

Native playlist export is part of [Send your library to DJ software](../sync/index.md). The app has no menu command to export an M3U or collection XML file, while [Import library](../files/import-library.md) can read M3U playlists. A developer-oriented command line mode can write an M3U8 for ready tracks ([library automation](https://github.com/johnneerdael/CueRithm/blob/main/docs/automation.md)).

### Variable-tempo grids and downbeat detection { #variable-tempo }

CueRithm’s own analysis and constant-grid editor use one BPM and one first beat. A live drummer or a tempo change drifts away from the beat lines. You can correct the saved constant grid ([Fix a wrong beatgrid](../analysis/corrections/fix-the-beatgrid.md)), but there is no variable-tempo detector or general variable-grid editor. A [supported Lexicon backup](../files/library/restore-library.md#lexicon) retains its source tempo markers; unsupported DJ-sync timing is refused rather than flattened. The detector searches 98 to 195 BPM and folds other tempos into that range, with no setting to change it ([My key or BPM differs from another app](troubleshooting/key-or-bpm-differs.md)).

### Per-segment energy and energy-labelled cues { #per-segment-energy }

Deliberately not offered. CueRithm rates the whole track once, from 1 to 10 ([How is energy rated?](../how-it-works/energy.md)).

### Playback-history tools and danceability analysis { #plays-danceability }

CueRithm does not predict danceability or provide a playback-history dashboard. Supported Lexicon imports can preserve source play counts, dates and danceability provenance; those imported values are not new CueRithm measurements.

### Other OneTagger workflows { #other-onetagger }

Spotify and quick tag are not included, and automatic preparation rejects Shazam, post-commands and lyric (LRC) files. Renaming files is available through [Rename and sort files into folders](../rename/organize-files.md). See [Credits and open source](about/credits.md#onetagger) for what CueRithm takes from OneTagger.

### A manual key correction { #manual-key }

You can correct cues, loops and the beatgrid by hand, but not the measured key. **Edit metadata** changes the key tag in the file only ([Edit tags and artwork](../tagging/edit-tags-by-hand.md)).

### Loop playback in the player { #loop-playback }

Saved loops are kept with their start and end and are sent to DJ software that supports them. The player does not repeat a loop: local preview seeks instead of looping ([Set a loop](../analysis/corrections/set-loops.md)).

### A way to choose how long Undo lasts { #backup-retention }

From CueRithm 0.1.19 or later CueRithm keeps no copy of your audio after a write has settled, and Undo lasts for your 10 newest merges, removals, cleanups, tag edits and syncs per DJ program. The number is fixed: there is no setting to change it or to keep more. Cleanup has **Undo**, and Sync history offers a guarded **Restore backup** and recovery for interrupted work. A file you removed as unused is in the Trash, not in CueRithm. The copies of writes that are under way are shown as **Recovery backups** in **Settings** → **Automatic tagging**.

### An audio device picker { #no-device-picker }

Playback uses the system's default output. Choose the device in your system's sound settings ([A track shows a dash or will not play](troubleshooting/no-waveform-or-playback.md)).

## Known limitations { #known-limitations }

### Long tracks do not prepare { #long-tracks }

Decoding stops at about 24 minutes, so long DJ mixes fail with `Media scan exceeded its output limit` and stay in Incoming. There is no setting to raise the limit ([A track is stuck in Incoming](troubleshooting/incoming-stuck.md), [How are audio files decoded?](../how-it-works/decoding.md)).

### Catalog matches are not guaranteed { #catalog-coverage }

Traxsource may need a separately hosted [FlareSolverr-compatible solver](../tagging/cloudflare-solver.md). Live fetching and parsing were verified with Trawl, but universal catalog coverage and packaged-app acceptance are unverified. Beatport has one successful direct API probe. A missing match is not always a fault ([Catalog tagging finds nothing](troubleshooting/no-catalog-match.md)).

### Windows, Linux and Intel Macs { #platforms }

There is no Intel Mac build. Windows and Linux builds are unsigned, and their native playback and restart acceptance is not complete ([My computer blocks CueRithm at first launch](troubleshooting/first-launch-blocked.md)).

### Older versions cannot open a newer library { #library-schema }

The library schema is version 31. An older CueRithm refuses a library that a newer one has opened, with `Library schema is newer than this application`, and there is no downgrade. [Back up your library](../files/library/back-up-library.md) before you update.

### Measured agreement, not accuracy { #accuracy-not-measured }

Published figures measure agreement with reference tools on the owner's own library. Held-out accuracy, real-music energy agreement and large-library analysis performance are not measured ([How accurate is it?](../how-it-works/accuracy.md)).

### How much of each DJ program was tested { #dj-program-checks }

Mixxx has not been run on Windows. Linux acceptance covers Debian's 2.5.0 package. Other packages and FAAD-based AAC decoding are unverified, and the tested macOS and Debian Mixxx builds cannot play WMA. The other destinations were checked against file formats only. See [Check what works with your DJ software](compatibility.md#other-destinations).

### No auto-update { #no-auto-update }

CueRithm does not update itself. Download new releases from the [releases page](https://github.com/johnneerdael/CueRithm/releases/latest) ([What's new in CueRithm](../get-started/whats-new.md)).

## Relocation and portable libraries { #missing-and-moved-files }

They need CueRithm 0.1.16 or later.

Reviewed [Relocate and Find Lost Tracks](../files/library/find-missing-files.md#relocate) reconnect existing tracks to moved files without moving audio or automatically preparing them again. [Find moved files](../files/library/find-missing-files.md#find-moved) does the same for unchanged files by their exact content, and [Remove from library](../files/library/find-missing-files.md#remove) clears out the records of files that are gone for good, with Undo; no audio file is touched. Moves inside the same watch folder are followed when the file contents identify one missing ready track and one new file (CueRithm 0.1.15 or later). Ambiguous or edited files need review; choose the complete survivor when a destination is already occupied.

### Reconnect moved music { #reconnect-moved-music }

Choose **Relocate** for selected tracks, or **Find Lost Tracks** in **Maintenance → Missing & broken** for a root or drive change. Review candidates, replacements and collisions. Guarded Undo is available in **Previous relocations**. [The reconnection guide](../files/library/find-missing-files.md#find-lost-tracks) covers preflight backups and unavailable drives.

### Moves outside watch folders { #moves-outside-watch-folders }

Automatic move tracking works inside one watch folder and only for an exact, unambiguous content match. A file moved out of the watch folder, a file that was edited or re-encoded, a track that is archived or still being prepared, and identical files that could belong to more than one track are left alone for you to review. Reconnect them with [Find moved files](../files/library/find-missing-files.md#find-moved) or [Relocate](../files/library/find-missing-files.md#relocate) ([I moved or renamed my files](troubleshooting/files-moved.md)).

### Portable backup, restore and Lexicon migration { #portable-backup }

[Library backup](../files/library/back-up-library.md#create) creates metadata-only `.cuerithm` archives. [Restore](../files/library/restore-library.md#connect-folders) maps source music roots explicitly, stages a replacement and restarts into it, retaining the previous library. Unresolved tracks remain in the library and playlists. Music is copied separately.

The supported Lexicon backup profile is `lexicon-main-db-v26`. Import replaces a library; it does not merge independent libraries. Other versions or mismatched schemas are refused. Imported cues and musical values remain separate from a completed CueRithm analysis.

Synthetic format, path-flavour and scale tests do not establish every real-machine migration. Nine source/destination OS combinations, device playback and the original private Lexicon sample remain separate acceptance work. Check the [recorded verification scope](https://github.com/johnneerdael/CueRithm/blob/main/docs/development/relocation-portable-backups.md) and your download’s release notes.

## DJ-library import limits { #in-development }

DJ-library import is separate from reviewed relocation and portable replacement. Broader DJ-application, physical-device and platform acceptance continues.

### Bring other DJ libraries into CueRithm { #dj-library-import }

The supported Lexicon ZIP profile above replaces the library. [Import library](../files/import-library.md) instead brings selected tracks, fields and playlists from supported DJ sources into the existing library. New reader profiles have authored fixture coverage; acceptance in the original applications and on devices remains unverified. Mixed In Key Windows storage and native reference repair for moving imported originals remain unqualified.

## Related

- [Check what works with your DJ software](compatibility.md)
- [Frequently asked questions](faq.md)
- [Report a problem or ask for a feature](report-a-problem.md)
- [Fix a problem](troubleshooting/index.md)

<!-- verified: apps/desktop/src/views/PortableBackup.tsx; Relocation.tsx; MaintenanceBroken.tsx (Find moved files, Remove from library); crates/cuerithm-library/src/watch_moves.rs (what is followed and what is left alone); crates/cuerithm-library/src/store.rs (schema 30); backup/lexicon_stream.rs; maintenance/relocate.rs; docs/development/relocation-portable-backups.md. Unrelated limits retained from upstream guide; relocation/portable planned claims replaced with current source behaviour and explicit acceptance boundaries. -->
