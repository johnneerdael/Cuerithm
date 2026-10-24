# Find missing, moved or broken files

<p class="lead">Find tracks whose files are missing or unreadable, then reconnect them without losing their library identity, or remove the records of files that are really gone. A scan changes nothing, and every repair is previewed first and can be undone.</p>

Start with the [scan](#scan). Then choose what fits: [Find moved files](#find-moved) when you know a folder that holds them unchanged, [Relocate](#relocate) for selected tracks and [Find Lost Tracks](#find-lost-tracks) for a folder or drive change, or [Remove from library](#remove) for tracks whose files are gone for good. Reconnecting changes library references, not music files, and removing deletes only library records. For a move you want CueRithm to perform itself, use [Organize files](../../rename/organize-files.md).

!!! note "Which version"
    Removing records, **Find moved files…**, the **Include archived tracks** option and automatic following of moves inside a watch folder need CueRithm 0.1.15 or later. **Relocate**, **Find Lost Tracks** and **Undo relocation** need CueRithm 0.1.16 or later.

## Scan the library files { #scan }

Choose **Maintenance** under TOOLS in the sidebar and open the **Missing & broken** tab. Select **Scan library files**. The progress line reads *Checking library files…* with a count.

The result heading reads, for example, **14 files need attention**, and the line below it gives how many tracks were checked and how: *40 checked · Availability only*. The table has the columns **Select**, **Recorded location**, **Outcome** and **Details**. Present files are hidden unless you tick **Show present files**. **Include archived tracks** is ticked by default, so archived tracks are checked too and can be repaired or removed like any other; untick it to leave them out, and the line then omits *Archived tracks included*. A track that is not linked to a local file has no path to check and is not listed.

Even in the quick mode CueRithm reads each present file to compare its checksum with the one stored when the track was prepared. That is how it notices a file edited elsewhere.

## Read the outcome of each file { #outcomes }

| **Outcome** | What it means | What to do |
|---|---|---|
| **Missing** | The file is not at the recorded location, but its folder exists. | Find the file, or [repair the library](#repair). |
| **Unavailable** | The folder itself is gone or cannot be read: an unplugged drive, a renamed or deleted folder, or a permissions problem. **Details** gives the system's message. | Reconnect the drive first. See [When a whole drive is missing](#drive). |
| **Symbolic link** | The recorded path is a symbolic link. CueRithm does not follow links. | Replace the link with the real file at that location. |
| **Not a regular file** | The recorded path is a folder or another kind of item. | Restore the file. |
| **Changed** | The file changed while the scan was reading it. | Scan again. |
| **Unsupported**, **Timed out**, **Decode error**, **Video**, **No audio** | Only with **Inspect audio decoding** on. The decoder could not use the file. **Details** gives its message. | See [Check that present files can be decoded](#inspect). |

A present file can still carry the flag **· Source changed** in **Details**. It means the file's bytes differ from what CueRithm recorded when it prepared the track, for example because another app edited the tags. Review saved timing before using a replacement. A [portable library backup](back-up-library.md#create) can preserve the current records without reading the audio; a same-installation recovery snapshot still has stricter file-consistency checks. [Analyze again](../../analysis/analyze-again.md) is a separate action that can replace analysis and cues.

## Why moved files look like a missing track and a duplicate { #moved-files }

A track keeps a library identity and a file reference. A move in Finder or Explorer can leave that reference pointing at the old location. Inside a watch folder CueRithm follows the move for you: when a new file has exactly the content of one missing, prepared, unarchived track from the same watch folder, that track's location is updated and nothing new is added. It also does so when you moved the files while CueRithm was closed, the next time it scans the folder (for example at start-up). The track keeps its analysis, cues, playlists and tags.

A move is left alone for you to review when the file could belong to more than one track (true duplicates), when the file was edited or re-encoded, when it left the watch folder, or when the track is archived or still being prepared. A file that is still being copied is checked again once it has settled. Those cases look like the example below until you repair them with [Find moved files](#find-moved) or [Relocate](#relocate), which keep the chosen track's identity.

Before CueRithm followed moves, 14 files moved up one level inside `~/Music/Beatport` of the demo library, out of a subfolder named after the release, produced this. Afterwards the library held 40 tracks for 26 files: 12 untouched, 14 old tracks whose file is gone, and 14 new tracks for the same files.

| | Old track | New track |
|---|---|---|
| File | Gone from the recorded location | At the new location |
| **Missing & broken** | Listed as **Missing**, or **Unavailable** once you also deleted the emptied subfolder | Not listed |
| Playlists, hand-corrected cues, categories | Still there | None. Prepared from scratch |

You can recognise the situation from three signs:

1. **Tracks** counts more tracks than you have music files.
2. The scan lists a block of files that all share one folder, the old one.
3. In **Tracks**, with the **Location** column shown (see [Show the columns you need](../../analysis/choose-columns.md)) and sorted by **TRACK**, every affected title appears twice, once with the old path and once with the new.

Files that CueRithm moves itself with [Organize files](../../rename/organize-files.md) keep their records. Only moves made outside CueRithm break the location. To avoid the problem in future, read [Move or rename files without breaking the library](../move-files.md).

## Repair the library today { #repair }

Reconnect the existing track when you want to keep its playlists, corrections and categories. Do not use [duplicate merging](find-duplicates.md#keeper) as a substitute for reviewing which moved track should survive.

### Find moved files by their content { #find-moved }

Use this when the files still exist somewhere and nothing about them has changed. CueRithm matches them to your missing tracks by their exact content, so a renamed or moved copy is found and an edited or re-encoded file is never linked by mistake.

1. Scan the library files and select the missing tracks: tick their rows, or tick **Select all missing**. Only **Missing** and **Unavailable** rows have a checkbox. **Select unreachable** adds the **Unavailable** ones and **Clear selection** starts over.
2. Select **Find moved files…**. Under **Search location**, type the folder, or select **Choose search folder**. A single file works too.
3. Select **Search for matching files**. The result reads, for example, *2 files found · 7 files checked*. Each selected track shows a **Result** and, when there is a match, **Found at**: *Will change*, *Not found*, *Several matches* or *Already in the library*. Only files in the location you chose are read, and CueRithm compares each file's SHA-256 with the one stored for the track.
4. Select **Relink N tracks**. CueRithm hashes each file once more and relinks only the ones that are still the exact bytes. The message reads *Relinked 2 tracks to the files you moved. Undo is in History & recovery. Scan again to refresh this list.*

The track is updated in place: its identity, analysis, cues, playlists, links and history stay attached, and artwork that CueRithm wrote beside the audio follows it. No audio file is moved, tagged or prepared again. A row is left alone, with the reason shown, when:

- no file with the same content was found, or a file with the same name was found whose content differs (*It was not relinked*);
- several selected tracks have identical content (select one at a time), or several identical files were found (choose a narrower location);
- the identical file is already another library track (use [Duplicates](find-duplicates.md) to merge them);
- the track or file changed after the preview, or the track has unfinished work.

Undo is under **Missing-file repairs** in [History & recovery](undo-maintenance.md#undo-repairs). **Relocate** is the choice when the replacement is a different recording or format, when a whole folder or drive changed, or when you want a portable backup first.

### Reconnect selected tracks { #relocate }

1. In **Tracks**, select the affected tracks and choose **Relocate** from the toolbar or track menu.
2. For one track, choose its replacement audio file. For several, choose a folder to search recursively. Present tracks are skipped in this selection workflow.
3. Select **Review matches** and read **Recorded location**, **Candidate and evidence** and **Decision**. Ambiguous matches remain unselected; choose a candidate or leave **Skip this entry**.
4. For a different recording or format, tick **Accept replacement; check saved cue timing** only after reviewing it. A filename suggestion does not prove identical audio; matching known encoded-file hashes are stronger evidence.
5. Keep **Create a portable preflight backup (recommended)** enabled and select **Choose backup folder**. A failed backup blocks relocation. Turning it off is an explicit choice.
6. Confirm the reviewed changes and select **Reconnect … entries**.

The chosen track keeps its identity, metadata, cues and playlist links. Reconnection does not move audio, write tags, prepare the track again or sync a DJ library. Changed content can make saved analysis stale; listen and check its timing before choosing [Analyze again](../../analysis/analyze-again.md).

### Replace a folder or drive root { #find-lost-tracks }

In **Maintenance → Missing & broken**, open **Find Lost Tracks**. Choose a **Recorded music root**, or enter the **Recorded source root**, then choose the **New music root**. The change applies to a complete leading folder path, not matching text elsewhere in a name.

**Missing only** is the default. Choose **All tracks, including archived entries** only when you intend to review available tracks too. **Include unavailable storage references** is separate: reconnect an offline drive first when possible. **Replace extension (optional)** is an explicit replacement, not a claim that the recording is unchanged.

Select **Review matches**, then use the same candidate, backup and confirmation steps above. **Search incomplete** identifies unsearched locations. If the search pauses at its safety limit, choose **Continue search** before treating the results as complete.

### Choose one complete survivor { #survivor }

A destination may already belong to another track. Choose **Keep source entry**, **Keep existing entry** or **Skip collision**. For a shared destination group, choose one **Group survivor** for the whole group.

CueRithm keeps that survivor’s complete metadata and cues, archives the other track records and rewires their ordinary playlist links. It does not combine selected fields from different tracks silently. Review every affected entry before confirming.

### Undo a relocation { #undo-relocation }

Select **Undo relocation** after applying, or reopen this workspace and use **Previous relocations → Undo saved relocation** after a restart. Undo checks the affected library state and file proofs again; later relevant changes can block it.

Undo restores the old library references and survivor changes. It does not move audio back, so an old reference can become unresolved again. [Retained library recovery](restore-library.md#go-back) is a separate whole-library operation.

### A. Put the files back { #put-back }

You can instead move files back to their **Recorded location** yourself, with the original names, then scan again. This changes the filesystem rather than accepting a new canonical location in CueRithm. Review any newly discovered copies before archiving them.

### B. Archive the old tracks { #archive-old }

If you no longer need an old track, select it in **Tracks** and choose **Archive**, or **Archive track** in its row menu. Archiving is not a reconnection: work attached only to that track stays with it. To preserve that work on a moved file, review [Relocate](#relocate) first.

### What Archive does { #what-archive-does }

Archive hides a track from Tracks and normal playlist views without deleting its music. The record, metadata and cues remain in Archive. [Archive tracks you no longer play](../archive.md) explains restoring one; a file reference can still need reconnection afterwards.

## Remove the records of files that are gone { #remove }

Use this when a file is gone for good, because you deleted it on purpose or lost it, and you want the track out of your library. It removes **library records only**. Audio files, artwork and folders are never deleted, moved or changed.

1. Scan the library files and select the tracks as described in [Find moved files](#find-moved). Only a file that is really gone can be selected; a file that is present, or comes back before you apply, is kept.
2. Select **Remove from library…**. The preview **Remove N records from the library** says *Only the library records are removed. Audio files, artwork and folders are never touched.* and counts what leaves with them: playlist memberships, with the playlists they are in, DJ-software sync links, and rekordbox links, which are unlinked while rekordbox itself is not changed. It also names selected tracks that will be kept, with the reason, such as an unfinished job or an open metadata draft.
3. If some selected files are in a folder or on a drive that could not be reached, a warning appears. If a drive is unplugged, reconnect it instead (see [When a whole drive is missing](#drive)). Otherwise tick **No drive is unplugged. Remove these records anyway.**
4. Tick **I understand these library records will be removed. I can undo this in History & recovery.** and select **Remove N records**. The message reads *Removed 3 library records. Undo is in History & recovery. Scan again to refresh this list.*

Playlists lose the track and keep the order of the others. A track's cues, analysis history, tags and metadata are saved with the removal, so **Undo removal** can bring everything back, including the track's place in each playlist. The cached waveforms, fingerprints and analysis results built from the removed audio are deleted, unless another track has the same content, and so are saved scans that listed the track. The cover art stays until the saved records are dropped.

CueRithm keeps the saved records of your last 10 removals. In CueRithm 0.1.17 or later, duplicate merges that [delete files](find-duplicates.md#delete) count toward the same 10, and with CueRithm 0.1.19 or later so do merges that only archive. When an eleventh removal pushes the oldest one out, its history line stays but it can no longer be undone. To drop the saved records of a removal sooner, use **Discard saved records…** in [History & recovery](undo-maintenance.md#undo-repairs).

## When a whole drive is missing { #drive }

If hundreds of tracks read **Unavailable** and their locations start with the same drive or network path, the storage is probably offline. Reconnect it, wait for it to mount, and scan again. Do not archive these tracks. CueRithm cannot tell a disconnected drive from a deleted folder, which is why the outcome is **Unavailable** rather than **Missing** when a folder is absent.

## Check that present files can be decoded { #inspect }

Tick **Inspect audio decoding** before you scan to decode every present file with the bundled decoder. The note beside the box reads *Decode audio to distinguish unsupported formats, timeouts and decoding errors.* The result then names the decoder, and a file that exists but cannot be read shows **Decode error**, **Unsupported** or **Timed out**. Without the tick the note reads *Check file availability. A present file has not been checked for decoding errors.* A damaged file you cannot replace will not play; see [A track shows a dash or will not play](../../help/troubleshooting/no-waveform-or-playback.md).

## What can go wrong { #problems }

| What you see | Why | What to do |
|---|---|---|
| **0 files need attention** but a track will not play | The quick scan checks availability only | Tick **Inspect audio decoding** and scan again |
| A banner reads *Library or maintenance inputs changed. Reload or rescan.* | A track, file or playlist changed after the scan | Select **Scan library files** again |
| *Another maintenance operation is still running* | Only one scan or apply runs at a time | Wait, or select **Cancel operation** |
| The old tracks still read **Missing** after you moved the files back | A name differs, even by one character, or the folder is different | Compare with **Recorded location** and correct it |
| Many tracks show **· Source changed** | The encoded file differs from the saved source | Review the recording and timing first; [Analyze again](../../analysis/analyze-again.md) can replace saved analysis and cues |
| **Remove from library…** is greyed out | No track is selected, or the report is out of date | Tick at least one **Missing** or **Unavailable** row, or scan again |
| A selected track is listed under *will be kept* | Its file is at its place again, it has unfinished work or an open metadata draft, or it changed after the scan | Finish or discard that work and scan again |
| A file with the right name was found but not relinked | Its content is different from the stored one | Use [Relocate](#relocate) and review it, or put the original back |

## Related

- [Find and merge duplicate tracks](find-duplicates.md)
- [Move or rename files without breaking the library](../move-files.md)
- [I moved or renamed my files](../../help/troubleshooting/files-moved.md)
- [Archive tracks you no longer play](../archive.md)
- [Undo a merge or put back removed files](undo-maintenance.md)
- [Keep a folder like Beatport in your library automatically](../watch-folders.md)

<!-- verified: apps/desktop/src/views/Relocation.tsx; MaintenanceBroken.tsx (scan options, selection, Remove from library preview and confirmations, Find moved files, labels and messages); MaintenanceHistory.tsx; missing-repair-api.ts; App.tsx; components/TrackTable.tsx; apps/desktop/src-tauri/src/relocation_commands.rs, missing_commands.rs; crates/cuerithm-library/src/maintenance/relocate.rs, missing.rs (preview rules, reasons, relink, undo, discard, covers), track_removal.rs, purge.rs, undo_window.rs; watch_moves.rs (when a move is followed); relink.rs; tests/relocation.rs, missing_repair.rs, watch_moves.rs, removal_engine.rs; maintenance/broken.rs. Historical 14-file example retained from the upstream demo-library guide; no new track data invented. -->
