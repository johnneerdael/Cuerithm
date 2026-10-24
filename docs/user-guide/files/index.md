# Manage your music files

<p class="lead">Bring music in, keep a folder such as Beatport watched, follow every track through Incoming, archive what you stop playing, and look after the library with duplicate checks, missing-file scans and backups. Preparing a track means CueRithm decodes it, measures its key, BPM, beatgrid and energy, places cues, runs the tagging and cleanup you switched on, and then moves it from Incoming to Tracks.</p>

## Start here { #start-here }

New to CueRithm? Do [Prepare your first tracks](../get-started/first-tracks.md). It takes a few minutes, uses a handful of files, and ends with prepared tracks in Tracks. Already have a purchase folder to bring in? Go straight to [Prepare a new Beatport purchase folder](recipes/prepare-a-purchase-folder.md).

<div class="grid cards" markdown>

-   **Bring music in**

    ---

    Drop files, pick them, or let CueRithm watch a folder such as
    `~/Music/Beatport` and prepare whatever lands in it.

    [:octicons-arrow-right-24: Add tracks](add-tracks.md)

    [:octicons-arrow-right-24: Keep a folder in your library](watch-folders.md)

-   **Watch it work**

    ---

    Read the stage on each row, know what "ready" means, and fix a track that
    stops with a message.

    [:octicons-arrow-right-24: Follow a track through Incoming](follow-incoming.md)

    [:octicons-arrow-right-24: Fix a track that needs attention](needs-attention.md)

-   **Choose the rules**

    ---

    Set how cues are placed, save a setup as a preset, and give each folder
    its own rules.

    [:octicons-arrow-right-24: Choose how cues are placed](../analysis/cue-rules.md)

    [:octicons-arrow-right-24: Save your setup as a preset](presets.md)

-   **Redo and tidy up**

    ---

    Measure a finished track again, or move tracks you no longer play out of
    the way without deleting anything.

    [:octicons-arrow-right-24: Analyze a track again](../analysis/analyze-again.md)

    [:octicons-arrow-right-24: Archive tracks](archive.md)

</div>

## Your goal and where to go { #goals }

### Get music in

| Your goal | Page |
|---|---|
| Put a few files or a folder in CueRithm once | [Add tracks by dropping, picking or watching a folder](add-tracks.md) |
| Have every new file in `~/Music/Beatport` prepared automatically | [Keep a folder like Beatport in your library automatically](watch-folders.md) |
| Take a fresh purchase folder from download to prepared tracks | [Prepare a new Beatport purchase folder](recipes/prepare-a-purchase-folder.md) |
| Know whether a file will work before you add it | [Check that your files are supported](supported-formats.md) |
| Rename or move files you already added | [Move or rename files without breaking the library](move-files.md) |

### Watch preparation and fix problems

| Your goal | Page |
|---|---|
| See which stage a track is in and when it will be ready | [Follow a track through Incoming](follow-incoming.md) |
| Understand a warning triangle and an error under a track | [Fix a track that needs attention in Incoming](needs-attention.md) |

### Choose the rules

| Your goal | Page |
|---|---|
| Change where Drop, Breakdown and the other cues land | [Choose how CueRithm places cues](../analysis/cue-rules.md) |
| Keep a tagging, cleanup and cue setup under a name | [Save your setup as a preparation preset](presets.md) |
| Use one setup for one folder and another for the rest | [Use different rules for different folders](folder-presets.md) |
| Measure key, BPM, energy and cues without touching tags | [Analyze tracks without tagging or cleanup](recipes/analyze-only.md) |

### Redo and tidy up

| Your goal | Page |
|---|---|
| Apply new settings to tracks that are already finished | [Analyze a track again](../analysis/analyze-again.md) |
| Take tracks out of Tracks and playlists but keep their analysis | [Archive tracks you no longer play](archive.md) |

### Look after your library

| Your goal | Page |
|---|---|
| Find tracks that are the same recording and keep one | [Find and merge duplicate tracks](library/find-duplicates.md) |
| See which tracks have no file, a damaged file or a changed file | [Find missing, moved or broken files](library/find-missing-files.md) |
| List music files that no track uses, and remove them | [Find and remove unused files](library/clean-up-unused-files.md) |
| Take back a merge or a removal | [Undo a merge or put back removed files](library/undo-maintenance.md) |
| Save a dated copy of the library, or put one back | [Back up your library](library/back-up-library.md) · [Restore your library from a backup](library/restore-library.md) |
| Find the library on disk or see what takes space | [Where CueRithm keeps your data](library/where-data-lives.md) |

More on this part: [Look after your library](library/index.md).

## Three things that surprise new users { #surprises }

- **Rules are captured when a file is added.** If you change a setting while 26 tracks are still queued, those tracks keep the rules they were added with. Tracks that already stopped with an error are the exception: saving settings retries them with the new rules. To apply new settings to a finished track, use [Analyze again](../analysis/analyze-again.md).
- **Preparing does not touch your files unless you ask it to.** With automatic tagging, cleanup and file organization all off, CueRithm only reads your audio. The results live in the library. See [Analyze tracks without tagging or cleanup](recipes/analyze-only.md).
- **A track reaches Tracks only when everything required has finished.** A missing key, an empty beatgrid or a failed required tag keeps it in Incoming with the reason shown, instead of a half-finished row in Tracks. See [what ready requires](follow-incoming.md#what-ready-requires).

## Not what you wanted? { #elsewhere }

- Reading a prepared track's key, BPM and energy, or fixing a wrong cue: [Track analysis](../analysis/index.md).
- Catalog tags from Beatport and Traxsource, cleanup rules, renaming files: [Auto tagging](../tagging/index.md).
- Putting prepared tracks into playlists: [Playlists](../playlists/index.md).
- Sending them to rekordbox or a USB stick: [Sync](../sync/index.md).
- Finding duplicates, missing files and backing up: - A track that does not move: [A track is stuck in Incoming](../help/troubleshooting/incoming-stuck.md), and [I moved or renamed my files](../help/troubleshooting/files-moved.md).

## Related

- [Reference: Incoming settings](../reference/settings/incoming.md) and [Cue generation settings](../reference/settings/cue-generation.md)
- [Status icons and job states](../reference/status-icons.md)
- [How does CueRithm analyse a track?](../how-it-works/pipeline.md)

<!-- verified: apps/desktop/src/App.tsx (Add tracks, drop overlay, Incoming/Archive views, processing indicator); apps/desktop/src/views/Settings.tsx (page list); crates/cuerithm-library/src/jobs.rs (claim_next uses the job's captured settings revision), crates/cuerithm-library/src/prepare.rs (retry_attention on save); legacy guides/incoming-archive.md and get-started/add-music.md re-checked -->

To bring tracks and playlists from another DJ application while keeping its values, use [Import a DJ library](import-library.md). Analysis and file changes are separate choices and start off.
