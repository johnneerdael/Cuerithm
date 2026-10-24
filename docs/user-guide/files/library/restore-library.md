# Restore your library from a backup

<p class="lead">Restore a portable library, import a supported Lexicon backup or return to a retained library. Review the contents and music-folder mappings first; activation then restarts CueRithm.</p>

**Before you start:** keep a separate copy of your music. A `.cuerithm` archive holds library data only. Restore replaces the active library and retains the current one for [recovery](#go-back); it does not merge their later edits or change files in DJ software.

!!! note "Which version"
    Restoring a portable `.cuerithm` backup, mapping music folders and importing a Lexicon backup need CueRithm 0.1.16 or later.

## Check the backup { #inspect }

1. Choose **Library backup** under TOOLS in the sidebar.
2. For a Lexicon import, check **Lexicon timestamp timezone** before inspecting. Use the time zone that should interpret source dates without an explicit offset.
3. Select **Inspect backup or Lexicon ZIP** and choose the `.cuerithm` file or Lexicon backup ZIP.
4. Read **Restore portable library** or **Import Lexicon library**, its profile, counts, **Compatibility notes** and **Review excluded or unsupported data**.

Inspection checks the archive before replacing anything. Corrupt files, unsupported required features, invalid references and unsupported Lexicon schemas are refused. [Back up your library](back-up-library.md#create) if you need a fresh portable copy first.

### Import a Lexicon backup { #lexicon }

The supported profile is `lexicon-main-db-v26`, a backup ZIP containing its root `main.db`. A version number alone is not a compatibility promise: the required schema must match too. The ZIP and the installed Lexicon application remain unchanged.

Tracks, accepted musical values, signed cue times, tempo markers, playlist folders and memberships, custom tags and supported smart definitions are preserved. An unsupported smart definition remains disabled with its source data retained. Imported streaming references remain unavailable for local playback.

Imported values are accepted library data, not a claim that CueRithm has analysed the audio. Import does not enqueue preparation or replace those cues. [Analyze again](../../analysis/analyze-again.md) is a separate, explicit action. Imported variable-tempo data is retained; a DJ sync refuses timing it cannot represent instead of flattening it.

Merging libraries and importing arbitrary DJ databases are separate work. See [What is still planned](../../help/not-available-yet.md#dj-library-import) for that distinction.

## Connect music folders { #connect-folders }

Under **Connect music folders**, each source root shows its original path and track count. Select **Choose folder** beside it and choose that music subtree on this computer. For example, map a source `D:/Music` to the folder that now contains the same relative albums and files. Copy the audio yourself before doing this.

Leave a destination empty (**Keep unresolved**) when a drive is offline or you have not copied those files. The metadata, cues and playlists stay in the library. Foreign Windows drive and network paths are preserved as source references; they do not become local paths automatically.

CueRithm verifies candidate files before binding them. Case collisions, unsupported names, changed known file hashes, unreadable files or unsafe links leave affected tracks unresolved with a warning. Use [Relocate](find-missing-files.md#relocate) later for individual files; see [When a whole drive is missing](find-missing-files.md#drive) before substituting another location.

## Schedule the restore { #schedule }

1. Read the active library identifier under **Replace and retain**.
2. If exclusions or unsupported data are listed, review them and tick **I accept these compatibility losses.**
3. Tick **Replace this active library with the reviewed backup and restart CueRithm.**
4. Select **Restore and restart CueRithm**.

A new library is validated and staged before the restart. If CueRithm asks you to review unsaved drafts, finish that review. Cancelling the restart cancels the pending replacement and keeps the current library active.

## Apply it { #apply }

The restart closes the current library before opening the staged one. The previous library is retained. After activation, check the restored and unresolved counts, [preview a connected track](../../analysis/preview-a-track.md), and review [playlists](../../playlists/edit-playlist-tracks.md).

Automatic tagging, cleanup, watch-folder bindings and external DJ writes start disabled. Configure them deliberately after checking the restored library. Incoming intent can be preserved without creating runnable preparation jobs.

## When a restore is blocked { #blocked }

| What happens | What to do |
|---|---|
| Archive damage, invalid data, an unsupported profile or a resource limit is reported | Use another intact copy or a supported backup. Do not edit archive internals |
| The source archive or current library changed after inspection | Inspect again and repeat the review |
| A chosen folder cannot supply a verified file | Review the warning, keep the track unresolved and [reconnect it separately](find-missing-files.md#relocate) |
| Writers cannot stop, storage is unavailable or staging fails | Finish the reported work or free space, then retry; the current library remains active |
| Start-up cannot validate the proposed replacement | CueRithm retains or returns to the previous validated library; keep the data folder intact and [report the error](../../help/report-a-problem.md) |

Restoring library data cannot undo music-tag changes. Use the original operation's [tag history](../../tagging/tag-history.md), [cleanup Undo](../../tagging/undo-cleanup.md) or [sync Restore backup](../../sync/preview-apply-restore.md) for those operations.

## Go back to before the restore { #go-back }

In **Library backup**, find **Retained libraries**. Choose a recovery library, read which active library will be kept, tick its activation confirmation and select **Use retained library and restart**.

This returns to that complete library. Edits made afterwards are not merged into it; the library you leave is retained in turn. Keep these directories untouched as explained in [Where CueRithm keeps your data](where-data-lives.md#what-is-in-the-folder).

## Restore a same-installation snapshot { #installation-snapshots }

The older folder format has a separate workflow. Expand **Same-installation recovery snapshots**, select **Inspect backup folder** and choose its generated `CueRithm-…` folder containing `library.sqlite` and `manifest.json`. Read **Restore preview**, tick **Replace my current preparation state with this backup on the next launch.**, then choose **Restore on next launch**.

Quit and reopen CueRithm to apply it. **Cancel scheduled restore** cancels it before restart. A **Restore blocked** result leaves the current library unchanged; resolve the reported consistency problem and choose **Retry on next launch**, or cancel.

This format requires the same installation and compatible app/schema, with saved music locations, hashes and recovery resources unchanged. Later tagging, organisation, removal or sync can invalidate it. A pre-restore snapshot is kept under that library's `library-backups/pre-restore-…` directory; inspect its inner `CueRithm-…` folder to restore it through the same controls. For moved music or another computer, use the portable workflow above.

## Related

- [Back up your library](back-up-library.md#create)
- [Reconnect missing or moved files](find-missing-files.md#relocate)
- [Where CueRithm keeps your data](where-data-lives.md#what-is-in-the-folder)
- [Undo a merge or put back removed files](undo-maintenance.md#recover)

<!-- verified: apps/desktop/src/views/PortableBackup.tsx; LibraryBackup.tsx; portable-backup-api.ts; apps/desktop/src-tauri/src/portable_commands.rs; exit_commands.rs; host.rs; crates/cuerithm-library/src/generations.rs; backup/{spool,validate,lexicon,lexicon_stream}.rs; library_backup.rs; tests/library_generations.rs; tests/portable_backup.rs; tests/lexicon_spool.rs. Full real-machine migration and original private Lexicon sample acceptance remain separate from synthetic profile tests. -->
