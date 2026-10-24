# Where CueRithm keeps your data

<p class="lead">Where is my library, what does CueRithm keep for Undo and for how long, and what is safe to leave alone? This page answers all three and shows what is stored on your computer outside the library.</p>

## Your music stays where it is { #music }

CueRithm never keeps a copy of your music. The library stores a track identity, its original file reference and an optional verified local binding, plus accepted metadata and analysis. A checksum can be unknown for an unresolved imported track. Your files live wherever you put them, for example `~/Music/Beatport`.

Analysis alone never changes a music file. CueRithm writes to one only when you switch something on or ask for it: [automatic tagging](../../tagging/tag-new-tracks-automatically.md), [cleanup](../../tagging/cleanup-messy-titles.md), saving a change in [Edit tags and artwork](../../tagging/edit-tags-by-hand.md), copying from a donor in a [duplicate merge](find-duplicates.md#donor), or [Organize files](../../rename/organize-files.md) renaming or moving it. While it rewrites a file it holds the original, and deletes it when the write has settled. Moving a file yourself in Finder or Explorer breaks the stored location; see [Find missing, moved or broken files](find-missing-files.md#moved-files).

## Find the data folder { #data-folder }

Everything else lives in one folder, named after CueRithm's application identifier `com.cuerithm.app`:

| System | Data folder |
|---|---|
| macOS | `~/Library/Application Support/com.cuerithm.app` |
| Windows | `%APPDATA%\com.cuerithm.app` |
| Linux | `~/.local/share/com.cuerithm.app`, or `$XDG_DATA_HOME/com.cuerithm.app` when that variable is set |

On macOS the `Library` folder is hidden. In Finder choose **Go → Go to Folder…** (++cmd+shift+g++) and paste the path. Your audio files are never inside this folder.

## What is in the folder { #what-is-in-the-folder }

| Item | What it holds | Leave it alone? |
|---|---|---|
| `library.sqlite`, or `libraries/<id>/library.sqlite`, with the matching WAL, SHM and lock files | The library: tracks, analysis, cues, playlists, settings, presets and history. It is one SQLite database. The `.lock` file keeps two copies of CueRithm from opening it together | Yes. Never edit or delete it. Use [Library backup](back-up-library.md) to protect it |
| `active-library.json`, activation journals and `libraries/` | Select the active library and retain replaced libraries for recovery | Yes. Use [Retained libraries](restore-library.md#go-back), not manual file replacement |
| `.library-installation-id` | Identifies a library to same-installation recovery snapshots | Yes. Those snapshots need it |
| `analysis-components/` | The built-in analysis parameters, installed and verified when CueRithm starts | CueRithm recreates it |
| `cache/` (`media`, `artwork`, `waveforms`, `fingerprints`, `dj-timing`) | Working copies and derived data: decode scratch folders, extracted cover art, waveforms, decoded-audio fingerprints and DJ timing. `cache/media` is cleaned at every start | These are caches. Do not touch them while CueRithm is open |
| `tagging/` | The copy of a file being tagged, while the write is under way. It is empty when nothing is being written | Keep while CueRithm is open |
| `cleanup/` | The same for cleanup | Keep while CueRithm is open |
| `metadata-edits/` | Staging for tag and artwork edits you make by hand, and the artwork they replaced | Keep |
| `maintenance/` | `staging/` holds working files for a duplicate merge while it runs. Removed unused files leave nothing here: they are in the Trash | Keep while CueRithm is open |
| `organization-references/` | Recovery records for file reorganisation and for DJ-library references | Keep |
| `sync/` | Previews, journals and the backups of DJ-software syncs, for [restoring a sync](../../sync/preview-apply-restore.md). The backups of a sync are kept for the newest 10 syncs of each DJ program | Keep |
| `library-backups/` | A restore waiting for the next start, and the pre-restore backups of earlier restores | Keep |

The folders marked *Keep* are what recovery and **Restore** use. Delete them and the matching recovery can disappear. These resources are separate from a portable archive, which excludes original audio and executable recovery journals. Same-installation snapshots still check them in place.

A library generation has its own database directory. Library-owned caches and recovery subfolders can sit beside that database; older libraries can still use the top-level location. Keep the whole data folder intact when preserving local recovery. A portable archive is the supported way to transfer library data without copying these machine bindings.

## See what CueRithm keeps for Undo { #space }

The library holds tags, links to files and artwork, never audio. Undo is kept for a fixed number of operations, and what it needs is small:

| Undo of | Kept for | What is kept |
|---|---|---|
| A manual cleanup, a manual tag or artwork edit | Your 10 newest | The tag values that were replaced, and replaced artwork (artwork is part of the library) |
| A duplicate merge, a removal of library records | Your 10 newest together | The removed records, their cues, tags and playlist places, and their cover art |
| A sync to DJ software | The 10 newest per DJ program | The backup of the files the sync replaced |
| Removed unused files | No Undo in CueRithm | The files are in the Trash |

An older operation stays in its history with a note that it can no longer be undone, and what it kept is deleted. Saved scans and reports are kept for the three newest of each kind, and the caches (waveforms, scratch folders) trim themselves.

Open **Settings** at the bottom of the sidebar, choose the **Automatic tagging** page and look for the row **Recovery backups**. It shows the size and file count of the copies of files that are being written right now, from tagging, cleanup and manual edits. It reads 0.0 MB · 0 files when nothing is under way. A library from an earlier version may still show the copies it kept; CueRithm 0.1.19 or later deletes them when it starts, a few hundred writes at a time, so the row shrinks over a few starts.

CueRithm also keeps the database file small. It writes its log (the `-wal` file) in bounded pieces, empties it when it opens and when you quit, and gives back the space that deleted rows leave behind. An older library of up to 0.5 GB is converted once, when CueRithm starts, to do this. That needs free disk space of about three times the size of the library and takes about a second; a bigger one keeps working as before.

## What is stored outside the data folder { #outside }

| What | Where | In a library backup? |
|---|---|---|
| Your music | Where you keep it | No |
| Portable `.cuerithm` files and same-installation snapshot folders | The destination you chose | They are separate backup formats |
| Provider keys and sign-ins | Your system's credential store: Keychain on macOS, Credential Manager on Windows, the Secret Service on Linux | No |
| Theme, waveform style, sidebar and player sizes, table columns | The app's own browser storage on this computer, not the library. See [Switch between light and dark](../../customize/theme.md) and [Resize the sidebar and player](../../customize/layout.md) | No |
| The default watch folder | `Music/Cuerithm/Incoming` inside your Music folder, created for a new library | No (the folder's files are music) |

## Remove everything { #uninstall }

Removing the application does not remove its data folder. To wipe CueRithm completely, quit it, uninstall it and delete the data folder above. That deletes the library and every recovery file. Your music stays. Make a [library backup](back-up-library.md) first if you might want the work back.

Looking for how the library is built? See [How is the library stored?](../../how-it-works/library.md).

## What this means in CueRithm { #summary }

- Your music and the library are separate. Back up both: music with your usual backup, the library with [Library backup](back-up-library.md).
- Undo keeps recorded values, not copies of your music, and only for the newest operations, so it takes little space.
- Leave the data folder to CueRithm. Use the screens in this tab to change what is inside it.

## Related

- [Back up your library](back-up-library.md)
- [Restore your library from a backup](restore-library.md)
- [Undo a merge or put back removed files](undo-maintenance.md)
- [Find the setting you need](../../customize/find-a-setting.md)

<!-- verified: apps/desktop/src-tauri/src/lib.rs (app_data_dir, audio_dir()/Cuerithm/Incoming), host.rs (library.sqlite, cache/media cleaned at start, cache/artwork, waveforms), builtin.rs (analysis-components); crates/cuerithm-library/src/library_backup.rs (library-backups, .library-installation-id, RESOURCE_DIRS tagging/cleanup/maintenance/sync/organization-references/metadata-edits, .lock file); maintenance/scan.rs (cache/fingerprints), removal.rs (maintenance/removal), donors.rs (maintenance/staging), dj_sync_service.rs (cache/dj-timing, sync); tagging.rs tagging_storage (what Recovery backups counts); crates/cuerithm-credentials/src/native.rs; theme.ts, waveform-style.ts, App.tsx, TrackTable.tsx (localStorage); Tauri path docs via ctx7 (app_data_dir = data dir + identifier on macOS, Windows Roaming AppData, Linux XDG_DATA_HOME or ~/.local/share); legacy technical/library.md and get-started/install.md re-checked (legacy marked the Windows and Linux paths unverified; confirmed by Tauri docs; legacy schema 22 is outdated, the code is at 29). UNVERIFIED: that the caches can be rebuilt after deletion, and the exact location of the web storage on each system. ; lean.rs (WAL bounds, reclaim), cache_limits.rs, legacy_originals.rs, saved.rs (three newest scans), undo_window.rs (ten), rekordbox_sync_service/retention.rs and dj_sync_service.rs (ten syncs per target); removal.rs (Trash, nothing in maintenance/removal for new removals)-->

<!-- verified: portable storage and retained generations re-checked against apps/desktop/src-tauri/src/host.rs; crates/cuerithm-library/src/generations.rs; backup/spool.rs; source locators and optional local bindings in portable.rs. -->
