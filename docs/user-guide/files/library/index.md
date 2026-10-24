# Look after your library

<p class="lead">Find duplicate and missing files, clear out music nothing uses, and keep a backup you can restore. Every check starts with a scan that changes nothing, and every change is previewed before you apply it.</p>

## Start here

Begin with [Find and merge duplicate tracks](find-duplicates.md). It is the one tutorial in this tab, and it teaches the pattern that every tool here follows: **scan, review, preview, apply, undo**. If you came because tracks went missing after you moved files, go straight to [Find missing, moved or broken files](find-missing-files.md).

<div class="grid cards" markdown>

-   **I have two copies of the same track**

    ---

    Scan for tracks that sound identical, keep one and archive the rest.

    [:octicons-arrow-right-24: Find and merge duplicate tracks](find-duplicates.md)

-   **Tracks stopped playing after I moved files**

    ---

    Find lost references and reconnect them while preserving the chosen track’s work.

    [:octicons-arrow-right-24: Find missing, moved or broken files](find-missing-files.md)

-   **I want a backup**

    ---

    Save the library's playlists, cues, analysis and settings, then restore them if something goes wrong.

    [:octicons-arrow-right-24: Back up your library](back-up-library.md)

</div>

## Your goal → page

| Your goal | Page |
|---|---|
| Find tracks that are the same recording and keep one | [Find and merge duplicate tracks](find-duplicates.md) |
| See which tracks have no file, a damaged file or a changed file | [Find missing, moved or broken files](find-missing-files.md) |
| List music files in a folder that no track uses, and remove them | [Find and remove unused files](clean-up-unused-files.md) |
| Take back a merge or a removal | [Undo a merge or put back removed files](undo-maintenance.md) |
| Clear out tracks whose files are gone, or reconnect files you moved | [Find missing, moved or broken files](find-missing-files.md) |
| Retry an operation that was interrupted | [Undo a merge or put back removed files](undo-maintenance.md#recover) |
| See what CueRithm keeps for Undo and how much space it takes | [Where CueRithm keeps your data](where-data-lives.md#space) |
| Save a dated copy of the library | [Back up your library](back-up-library.md) |
| Restore a portable archive or import a supported Lexicon ZIP | [Restore your library from a backup](restore-library.md#inspect) |
| Connect old music folders on another computer | [Connect music folders](restore-library.md#connect-folders) |
| Return to a retained library | [Go back to before the restore](restore-library.md#go-back) |
| Reconnect moved tracks or replace a drive root | [Relocate and Find Lost Tracks](find-missing-files.md#relocate) |
| Find the library on disk or see what takes space | [Where CueRithm keeps your data](where-data-lives.md) |

## How the tools work

**Maintenance** and **Library backup** sit under TOOLS in the sidebar. Maintenance has four tabs.

| Tab | What it does | What it changes |
|---|---|---|
| **Duplicates** | Finds tracks that are the same recording, leaving out tracks whose file is not available | You choose per merge. **Archive only** archives the copies you do not keep and never touches a music file. **Delete from disk** moves verified identical copies to the Trash and removes their records. Both repoint playlists |
| **Missing & broken** | Reports missing or unreadable files; **Remove from library…**, **Find moved files…** and **Find Lost Tracks** repair them | Removal deletes library records only, never audio. Reconnecting changes reviewed library references and survivor membership, without moving audio |
| **Unused files** | Lists files in a folder that no track uses | Removes the files you select, after a preview, keeping a copy for Restore |
| **History & recovery** | Undo and retry for the tools above, and where removed unused files went | Reverses or finishes their changes |

!!! note "Which version"
    **Remove from library…**, **Find moved files…** and following moves inside a watch folder need CueRithm 0.1.15 or later. **Find Lost Tracks**, portable `.cuerithm` backups and restore need CueRithm 0.1.16 or later. **Delete from disk**, and leaving tracks whose file is not available out of Duplicates, need CueRithm 0.1.17 or later. Unused files that go to the Trash, and a limit of 10 for every merge, need CueRithm 0.1.19 or later.

Relocation has its own **Previous relocations** list and guarded Undo in the [reconnection workspace](find-missing-files.md#undo-relocation). Portable replacement has [retained-library recovery](restore-library.md#go-back), rather than replaying old file-write Undo journals.

Every maintenance tool follows the same rhythm. A scan reads your library and files and saves a report. You review the report and make choices. You preview exactly what will change. You apply it. Anything that changes your library or files can then be undone from **History & recovery**, while nothing relevant has changed since.

One operation runs at a time. A progress line shows what is happening, and **Cancel operation** stops it; the operation finishes cleanly first. If you leave the Maintenance screen, a running scan is cancelled. Reports and reviews are saved, so after a restart use **Reload saved report** or **Resume saved review**. A report goes stale when your library or files change; scan again.

## Not what you wanted?

- To put a track aside without touching its file, [archive it](../archive.md).
- To bring music in again after a move, read [Move or rename files without breaking the library](../move-files.md).
- To undo a tag change or a cleanup, see [Undo a tag change](../../tagging/tag-history.md) and [Undo a cleanup](../../tagging/undo-cleanup.md).
- To fix a problem you can see, go to [Fix a problem](../../help/troubleshooting/index.md), for instance [I moved or renamed my files](../../help/troubleshooting/files-moved.md).

## Related

- [Archive tracks you no longer play](../archive.md)
- [Preview, apply and restore a sync](../../sync/preview-apply-restore.md)
- [How is the library stored?](../../how-it-works/library.md)
- [What CueRithm does not do yet](../../help/not-available-yet.md)

<!-- verified: apps/desktop/src/views/Maintenance.tsx (tab labels, one-operation-at-a-time, cancel on leave), MaintenanceShared.tsx, MaintenanceDuplicates.tsx, MaintenanceBroken.tsx, MaintenanceUnused.tsx, MaintenanceHistory.tsx, LibraryBackup.tsx; components/Sidebar.tsx (TOOLS group: Maintenance, Library backup); ia.md -->
