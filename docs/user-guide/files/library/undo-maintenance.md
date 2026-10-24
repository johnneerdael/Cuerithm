# Undo a merge or put back removed files

<p class="lead">Undo a duplicate merge, put back library records you removed, find the files you removed as unused in the Trash, and retry anything that was interrupted. All of it happens in History &amp; recovery, and the history survives restarts.</p>

**Before you start:** you need a merge from [Find and merge duplicate tracks](find-duplicates.md), a repair from [Find missing, moved or broken files](find-missing-files.md) or a removal from [Find and remove unused files](clean-up-unused-files.md) to undo. Scans change nothing. A [relocation](find-missing-files.md#undo-relocation) has its own **Previous relocations** list and Undo controls in the reconnection workspace. A [portable restore](restore-library.md#go-back) uses retained-library recovery instead.

!!! note "Which version"
    **Missing-file repairs** and their Undo need CueRithm 0.1.15 or later. Merges that delete files, and one shared limit of 10 removals across both kinds, need CueRithm 0.1.17 or later. In 0.1.15 and 0.1.16 the limit of 10 counts Missing & broken removals only. Unused files that go to the Trash, Undo that writes back recorded tags instead of a copy of the audio, and a limit of 10 for every merge and for each kind of sync backup need CueRithm 0.1.19 or later.

## Open History & recovery { #open }

Choose **Maintenance** under TOOLS in the sidebar and open the **History & recovery** tab. The note at the top reads *History is saved across restarts. Undo checks current files and library state before changing anything.* Select **Refresh history** after another view changed something.

The tab has four sections: **Recovery**, **Duplicate merges**, **Missing-file repairs** and **Unused-file removals**. When something needs you, a button on the other Maintenance tabs reads, for example, *1 recovery item needs attention* and opens this tab.

## Undo a duplicate merge { #undo-merge }

Under **Duplicate merges**, find the entry **Duplicate merge** with its time and counts, such as *1 archived · 0 restored · 0 updated · 1 playlists*. Select **Undo merge**. The message reads *Merge undone. Original files and library state restored.*

Undo puts back what the merge changed:

- The archived tracks return to **Tracks**.
- Every playlist the merge touched has its old order again.
- If you copied tags and cues from a donor, the keeper's file gets its old tags back and the library gets its old tags, cues and grid. CueRithm recorded the keeper's tags from before the copy for this reason. It keeps no copy of the audio: the audio of the file does not change, and the tags are written back the same careful way they were written.

The entry then reads **Undone by** followed by an ID, and it cannot be undone twice. The undo itself is listed as **Merge undo**, which has no button of its own. To merge again, scan again.

### When Undo is refused { #refused }

Undo checks that nothing relevant has changed since the merge, because putting old data over newer work would lose it. Edits to unrelated tracks, new tracks arriving in Incoming and smart playlists refreshing do not block it.

| Message | Why | What to do |
|---|---|---|
| *Later relevant edits prevent Undo* or *Later track edits prevent Undo* | You edited, re-tagged or archived one of the merged tracks after the merge | Undo is no longer possible for that merge. Fix the tracks by hand |
| *Later playlist edits prevent Undo* | A playlist the merge changed was edited afterwards | Put the playlist back as it was after the merge, or fix it by hand |
| *Source changed; Undo needs review* | The keeper's file was modified by another app or by CueRithm after the merge | Undo is blocked to protect your newer file |
| *Later tag edits prevent Undo* | The tags in the keeper's file are no longer the ones the merge wrote | Undo is blocked to protect your newer edit. Fix the tags by hand |
| *Duplicate operation is not available for Undo* | It was already undone | Nothing to do |

An older merge recorded by an earlier build, which only archived tracks, uses a stricter rule: *Later library or playlist changes prevent Undo* appears as soon as anything in the library changed.

### Undo a merge that deleted files { #undo-delete }

A merge that used **Delete from disk** has an entry that also reads, for example, *1 removed from the library · 1 file moved to the Trash*, plus *· 1 permanently deleted* or *· 1 not deleted* when that applies. A file that was not deleted is listed with its reason. Select **Undo merge**. The message reads *Merge undone. The removed library records are back. Files moved to the Trash stay there until you restore them.*

Undo puts back the library records, their cues, tags, links and history, their places in playlists and the keeper's old tags when a donor changed them. It cannot take a file out of the Trash, and a permanently deleted file is gone. Restore the files from the Trash first or afterwards: if you put a file back where it was, it must be the very same file. A sync or DJ-program link that changed after the removal, for example because the sync linked the same record to the kept track, is left as it is. The message then adds, for example, *2 sync or DJ-program links were left as they are because they changed after the removal.*

Undo is refused, and nothing is changed, when:

| Message | Why | What to do |
|---|---|---|
| *Cannot undo: a different file is now at …* | Another file sits where a removed track's file was | Move it away, or put the original file back from the Trash |
| *Cannot undo: the identity or file location of … is already used by a track in the library* | The file was restored and added again as a new track | Remove that new track first, or keep the current library |
| *Undo is no longer available: the removed library records were kept only for the last 10 removals …* | The removal fell out of the Undo window | The removal is final; restore the files from the Trash by hand if you still need them |

The entry of a removal that is final reads *The removal is final.* and has no button.

## Your last 10 merges and removals { #window }

CueRithm keeps what Undo needs for your **10 most recent merges and removals of library records**, counting every duplicate merge (including **Archive only**) and [Missing & broken removals](find-missing-files.md#remove) together. When another one makes it eleven, the oldest stays in the history without its details and can no longer be undone. Undoing one gives its place back. The cover art of removed music is kept for as long as its removal can still be undone, and deleted with the saved records.

## Undo a removal or a relink { #undo-repairs }

Under **Missing-file repairs**, **Removed from library** entries come from **Remove from library…** and **Moved files found** entries come from **Find moved files…**. An entry shows the time and counts, such as *3 tracks · 5 playlist memberships · 1 sync links · 1 rekordbox links unlinked*, and **Affected tracks** lists each track with its location. Select **Undo removal** or **Undo relink**.

- **Undo removal** reads *Removal undone. The records, playlist positions and metadata are back.* Each track returns with its cues, tags, links and history, in its old place in every playlist; a playlist you edited since gets the track back at its old position or at the end. A rekordbox link or a sync link that something changed after the removal, for example because rekordbox was scanned again, is left as it is and listed as *changed after the removal … and were left as they are*.
- **Undo relink** reads *Relink undone. The tracks point at their old locations again.* If the file is still inside a watch folder, a note says CueRithm will follow it again the next time that folder is scanned (for example when the app starts); move it back as well to keep the old location.

Undo refuses, and changes nothing, when a different file now sits at the old location (*Cannot undo: a different file is now at …*), when another track uses the identity or the location, when the audio of a relinked track changed after it was relinked, or when the track has unfinished work. Undo checks the old location before it locks the library, so a refusal is quick.

To drop the saved records of a removal early, select **Discard saved records…**, tick **I understand this removal can no longer be undone.** and select **Discard saved records**. The history line stays, reading *Saved records discarded; this removal can no longer be undone.*, and the cover art kept for it is deleted.

## Find files you removed as unused { #unused-removals }

Under **Unused-file removals**, each removal reads **File removal · committed** with its file count and total size, and a line such as *3 files moved to the Trash*. Expand **Reviewed paths** to see every file, with its role, what happened to it (*moved to the Trash*, *deleted permanently*, *left where it was* or *no longer in its folder*) and every folder that was removed.

There is no **Restore** button, because CueRithm keeps no copy of these files: they are in the Trash on macOS and Linux, or the Recycle Bin on Windows, each under its own name. Open it and use **Put Back** (macOS) or **Restore** (Windows and most Linux file managers) to return a file to its folder. A file you put back is a normal file again: if no library track uses it, it shows up as unused in the next scan.

- A file marked *deleted permanently* was one the preview named as unable to go to the Trash. It cannot be recovered.
- A file marked *left where it was* was refused by the Trash and is still in its folder. The reason is shown beside it.
- *no longer in its folder* means CueRithm was interrupted after the file had left and could not tell where it went; look in the Trash.

The list keeps your 20 most recent removals. The history line of an older one is dropped, and nothing else changes.

## Recover an interrupted operation { #recover }

If CueRithm quits or loses power in the middle of a merge or a removal, it rolls the operation back the next time it starts. A file you changed in the meantime is never overwritten. It is kept, the operation is marked as a conflict, and the entry appears under **Recovery** or in the removals list with the error shown. A removal of unused files that had already been recorded is finished the next time CueRithm starts: the remaining files go to the Trash, and a file you edited in the meantime stays where it is.

If CueRithm quit after a delete merge had already moved files to the Trash, but before the library recorded it, the merge is rolled back and an entry reading `apply · rolled_back` stays under **Recovery**, with the error naming the files. Their library records were kept, so those tracks show as missing in [Missing & broken](find-missing-files.md#scan): restore the files from the Trash, or remove the records. Only this kind of entry has **Acknowledge**. Select it once you have read the entry; it does not come back. Any other entry under **Recovery**, including an `apply · rolled_back` one that only waited on a temporary-file cleanup, keeps **Retry merge recovery**.

Fix the cause first: free the path, reconnect the drive or put the newer file where you want it. Then select the retry button on the entry:

| Button | Where |
|---|---|
| **Retry merge recovery** | **Recovery**, on every entry except one that already moved files to the Trash (that one has **Acknowledge**) |
| **Retry file recovery** | A removal of unused files whose state is not *committed*, or that has an error or *Cleanup pending* |

Retry never overwrites an unknown later version of a file. It reports the error again if the cause is still there.

## Undo things outside Maintenance { #elsewhere }

| You want to undo | Go to |
|---|---|
| A cleanup | [Undo a cleanup](../../tagging/undo-cleanup.md) |
| A tag or artwork edit | [Undo a tag change](../../tagging/tag-history.md) |
| Renamed or moved files | [Recover or undo a reorganisation](../../rename/undo-organize.md) |
| A sync to DJ software | [Preview, apply and restore a sync](../../sync/preview-apply-restore.md). Restore keeps its backup for the newest 10 syncs of each DJ program |
| An archived track | Select it in **Archive** and choose **Restore**, once its file is in place. See [Archive tracks you no longer play](../archive.md) |
| The whole library state | [Restore your library from a backup](restore-library.md) |

## What can go wrong { #problems }

| What you see | Why | What to do |
|---|---|---|
| **Undo merge** is missing from an entry | It is a **Merge undo** entry, already undone, or a removal that is now final | Scan and merge again if needed |
| **Undo removal** is missing from an entry | It is an undo entry, already undone, or its saved records were discarded or pushed out of the last 10 | Nothing to restore. Scan Missing & broken again if the file is back |
| A removed unused file has no **Restore files** button | CueRithm keeps no copy of it | Put it back from the Trash |
| *Cleanup pending* beside a removal | Temporary files could not be deleted yet | Select **Retry file recovery** |
| *Another maintenance operation is still running* | Only one operation runs at a time | Wait, or select **Cancel operation** |
| History shows an error banner for one section | That section could not be read | Select **Refresh history** |

## Related

- [Find and merge duplicate tracks](find-duplicates.md)
- [Find missing, moved or broken files](find-missing-files.md)
- [Find and remove unused files](clean-up-unused-files.md)
- [Restore your library from a backup](restore-library.md)
- [Where CueRithm keeps your data](where-data-lives.md)

<!-- verified: apps/desktop/src/views/MaintenanceHistory.tsx (four sections, outcome labels, Retry file recovery), MaintenanceUnused.tsx; Maintenance.tsx, MaintenanceShared.tsx (undoLimitNote, skipReasons); apps/desktop/src/test/maintenance.test.tsx, missing-repair.test.tsx; crates/cuerithm-library/src/maintenance/donors.rs (recorded tag values, undo checks and messages, links_not_restored), undo_window.rs (shared limit 10, all merges), track_removal.rs, missing.rs, purge.rs (covers), merge.rs (version 1 journal rule), removal.rs and removal/legacy.rs (Trash, outcomes, history of 20), saved.rs (KEEP_REMOVAL_HISTORY); tests/removal_engine.rs, maintenance_deletion.rs, maintenance_duplicates.rs, missing_repair.rs, unused_removal.rs; docs/development/maintenance-api.md -->
