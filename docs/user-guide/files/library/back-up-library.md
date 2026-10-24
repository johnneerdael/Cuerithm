# Back up your library

<p class="lead">Save tracks, playlists, cues and settings in one portable `.cuerithm` file. Music stays separate. Allow several minutes for a large library.</p>

!!! note "Which version"
    Portable `.cuerithm` backups need CueRithm 0.1.16 or later.

Use **Library backup** under TOOLS in the sidebar. The main panel creates portable archives; **Same-installation recovery snapshots** opens the older recovery-folder workflow. [Restore your library](restore-library.md#inspect) explains how to choose between them.

!!! note "Libraries with DJ imports"
    If you have inspected or imported a source through [Import library](../import-library.md), portable export is blocked because it cannot preserve those source records, imported tracks and playlists yet. Use **Same-installation recovery snapshots** to retain them on this installation.

## What a backup holds { #what-it-holds }

| In a portable `.cuerithm` archive | Kept separately |
|---|---|
| Tracks, including archived and unresolved tracks, with their accepted metadata, cues, tempo markers and available analysis | Music, artwork and waveform caches |
| Playlist folders, ordinary membership, smart definitions and custom tags | Evaluated smart-playlist results, which are rebuilt |
| Selected musical data and, by default, prior musical and analysis revisions | Runnable preparation jobs and executable Undo journals |
| Portable analysis, cleanup and tagging preferences; library time zone and key-mixing mode | Provider credentials, watch-folder bindings, audio-device choices and DJ-library write permissions |
| Source folder references and historical DJ-library observations | Backup copies kept for restoring a sync, and the history of removed unused files |

An archive can contain private track metadata and original folder references. Keep it on storage you trust. It is not encrypted. See [Where CueRithm keeps your data](where-data-lives.md#outside) for settings stored outside the library.

Creating a portable backup does not read, hash or copy your music. Missing files and disconnected drives do not prevent their library records being saved. A portable archive does not reverse [tag changes](../../tagging/tag-history.md) or [DJ-software syncs](../../sync/preview-apply-restore.md).

## Create a backup { #create }

1. Choose **Library backup** in the sidebar.
2. Leave **Include musical revision history** selected to keep previous musical and analysis revisions. Clear it only when you want the selected revisions without that history.
3. Select **Export .cuerithm**, choose the destination and save.
4. Wait for **Portable backup saved**, then check the track count and path.

CueRithm verifies the completed archive before publishing it. An existing destination is not silently overwritten. **Cancel operation** removes the unfinished task copy and leaves your library active.

Copy your music separately before [restoring on another computer](restore-library.md#connect-folders). Keep another copy of the archive on a different disk if your usual backup disk also holds the music.

## Why a backup can be refused { #blocked }

Finish Incoming work and resolve unfinished file writes, [maintenance recovery](undo-maintenance.md#recover), [metadata saves](../../tagging/tag-history.md) or [sync recovery](../../sync/preview-apply-restore.md) before exporting. CueRithm does not save a half-applied filesystem operation as a portable library.

A full disk, unavailable destination, malformed stored data or a resource limit also stops export. Read the reported reason and retry after fixing it. A failed export does not replace the current library or publish a partial archive.

## Keep the backup safe { #keep }

Keep the `.cuerithm` file intact. Make a new one after important changes to playlists, corrections or metadata. Music may have moved since an older portable backup: review its [folder mappings](restore-library.md#connect-folders) when restoring, rather than putting files back just to match their old paths.

CueRithm does not delete your archives automatically. [Retained libraries](restore-library.md#go-back) are separate local recovery copies; neither replaces an independent backup of your music.

## Same-installation recovery snapshots { #installation-snapshots }

Expand **Same-installation recovery snapshots** when you specifically need the older database-and-manifest recovery format. It includes local preparation state and history, with strict checks against this installation, app version, music and recovery resources. It is not a portable migration archive.

<!-- figure:existing-library-backup-back-up-library -->
<figure markdown>
![Library backup with a restore preview and the player still visible](../../assets/screens/library-backup.png){ .screen loading=lazy }
<figcaption>Create a backup, and review a restore before it is scheduled.</figcaption>
</figure>
<!-- /figure -->

1. Let preparation and recovery finish.
2. Select **Create backup** inside this expanded section and choose a folder.
3. Wait for **Backup saved**. Keep `library.sqlite` and `manifest.json` together in the generated `CueRithm-…` folder.

This snapshot checks music-file hashes and retained resources, so it can take longer when drives are slow. Its database limit is 1 GiB, with up to 50,000 source locations and further recovery-inventory limits. Missing sources must retain their saved unavailable state. Later tag writes, file moves or changed recovery resources can make an older snapshot ineligible for restore.

For its separate confirmation and next-launch workflow, see [Restore a same-installation snapshot](restore-library.md#installation-snapshots). Use the portable workflow above for another computer or for music that has moved.

## Related

- [Restore your library from a backup](restore-library.md#inspect)
- [Reconnect moved files](find-missing-files.md#relocate)
- [Where CueRithm keeps your data](where-data-lives.md#what-is-in-the-folder)
- [Undo a merge or put back removed files](undo-maintenance.md#recover)

<!-- verified: apps/desktop/src/views/LibraryBackup.tsx; PortableBackup.tsx; portable-backup-api.ts; apps/desktop/src-tauri/src/portable_commands.rs; crates/cuerithm-library/src/backup/{container,spool,projection}.rs; library_backup.rs; tests/portable_backup.rs; tests/library_backup.rs. Existing screenshot depicts the same-installation snapshot panel. -->
