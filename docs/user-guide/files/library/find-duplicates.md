# Find and merge duplicate tracks

<p class="lead">In about ten minutes you will scan your library for tracks that sound identical, keep the copy you want, choose what happens to the others, preview the change, apply it and undo it again. Nothing leaves your disk unless you ask for it and the preview lists the file.</p>

<!-- figure:existing-maintenance-duplicates-find-duplicates -->
<figure markdown>
![Duplicate review with independent keeper and donor choices](../../assets/screens/maintenance-duplicates.png){ .screen loading=lazy }
<figcaption>Choose a keeper for each group and, optionally, a tag and cue donor.</figcaption>
</figure>
<!-- /figure -->

**You need:**

- [ ] CueRithm 0.1.17 or later with prepared tracks in **Tracks**
- [ ] At least one pair of identical tracks, both with their file in place. To make a practice pair, copy one file from a [watch folder](../watch-folders.md) into a second subfolder of the same watch folder and wait until **Tracks** counts it. The screenshots use *Man Like Me (Extended Mix)* by Kevin de Vries and Platero (<span class="cr-key">4A</span>, <span class="cr-bpm">128</span> BPM, <span class="cr-energy">7/10</span>, 5:15), once in `~/Music/Beatport` and once in a subfolder.

Duplicates only works with tracks whose file is there. A track whose file is missing, or sits on a drive that is not connected, is left out of the scan: it never joins a group, is never counted as a duplicate and can never be a keeper or a donor. The scan tells you how many it left out and sends you to [Missing & broken](find-missing-files.md#scan), where you can repair or remove them.

You choose what happens to the copies you do not keep. **Archive only** keeps every file where it is and moves the extra records to Archive. **Delete from disk** moves each verified identical copy to the Trash and removes its record. This tutorial uses **Archive only**; [Delete the extra files instead](#delete) covers the other choice.

!!! note "Older versions"
    Version 0.1.16 and earlier can still list a track whose file is missing inside a group, marked **Unavailable**, and always archive the copies you do not keep. Never choose an unavailable track as keeper, and leave such a group unselected: applying it would archive the track with the missing file and give its playlist places to the available one.

## 1. Scan for duplicates { #scan }

Choose **Maintenance** under TOOLS in the sidebar. The **Duplicates** tab is already open. Leave **Scope** on **Full library**, **Match method** on **Exact decoded audio** and **Exclude paths, one per line** empty, then select **Scan duplicates**.

CueRithm decodes each track and compares the sound itself, so it finds the same recording in a FLAC and a WAV even when the file names and tags differ. A progress line reads *Scanning duplicates…* with a count such as `26 / 27`. The first scan decodes every file, which takes a while on a large library. Later scans reuse what they stored and say so: *1 fingerprints computed · 26 cached*.

The result reads **1 duplicate group**. Scanning changes nothing: no track, playlist, tag or file.

If the scan left tracks out because their file is not available, a warning above the table says so, for example *2 tracks skipped: file not found. Review them in Missing & broken.* Where the reasons differ it names each one: *file not found*, *location not available* (a folder or drive that cannot be reached) or *file cannot be read*. Select **Open Missing & broken** to go there. Those tracks stay out of every group, whichever match method you choose.

## 2. Open the review { #review }

Select **Review groups**. The section **Review duplicates** lists every group as an article headed **Group 1**, **Group 2** and so on. The badge beside the heading reads **Exact audio · unlocked** when the sound matched exactly, or **Artist/title candidates · unlocked** when only the names matched.

Each group has a table with one row per track:

| Column | What it shows |
|---|---|
| **Track / location** | Artist, title and the full path of the file. Two copies usually share a title, so the path is how you tell them apart. A line **Extra 1:** appears when the track has text in its Extra 1 field. |
| **Bitrate** | The file's bitrate in kbps, or a dash when unknown. |
| **Cues** | How many cues the track has in the library. |
| **Availability** | **Present**. Every track in a group had its file when the scan ran. It reads **Unavailable** only if the file was lost after the scan, and preview and apply then refuse the merge. |
| **Ignore** | A checkbox that keeps this track out of duplicate work. See [Keep a track out of duplicate groups](#ignore). |

Your choices in the review are saved as you make them. If you quit, select **Resume saved review** next time.

## 3. Choose the copy to keep { #keeper }

Under the table, the **Keeper** menu names the track that stays in **Tracks**. CueRithm has already suggested one. It follows these rules in order and considers only available tracks that are not ignored:

1. A 320 kbps or higher MP3 wins when it has as many cues as the track with the most cues.
2. Otherwise the track with the most cues wins, provided its bitrate is known. If the best-cued track has no known bitrate, the highest-bitrate track wins and the best-cued track is suggested as **Tag and cue donor** (exact-audio groups only).
3. When copies sit in one folder, a file whose name ends in `copy`, `(1)` or ` 1` loses to its original. A remaining tie goes to the track that entered the library first.

For the practice pair both copies have five cues, so the copy that entered the library first is suggested. Change **Keeper** if you prefer the other file. Tracks that are **Unavailable** or ignored are greyed out in the menu.

Tick the checkbox beside **Group 1** to include it in the merge. A group is selectable only when it is unlocked and its keeper (and donor, if you chose one) is available.

## 4. Preview the change { #preview }

Above the groups, under **Keeper preferences**, the box **What happens to the other tracks in each group** asks for a choice. **Delete from disk** and **Archive only** both start unselected, and **Preview selected groups** stays off until you pick one, because the two choices change very different things. Choose **Archive only** for this tutorial. Changing the choice later discards a preview you already made.

Select **Preview selected groups**. **Review merge changes** lists, per group, the keeper, which tracks will be archived and which tags, cues and grid will be copied. The line **N playlist changes** opens the ordered before and after list of every ordinary playlist that holds a duplicate. The keeper takes the first slot a duplicate occupied, and the other duplicates drop out of that list.

The preview names tracks by title only, so two copies of one song read the same. Compare with the **Track / location** column if you need to be sure which copy is which. Nothing has changed yet. **Discard preview** leaves everything as it was.

## 5. Apply the merge { #apply }

Select **Apply reviewed merge**. The message reads *Merge complete. 1 archived · 0 updated. Undo is in History & recovery.* The scan and review clear, because both are out of date.

Check three things:

- **Tracks** in the sidebar counts one fewer, and **Archive** counts one more. The archived copy keeps its analysis and cues. See [Archive tracks you no longer play](../archive.md).
- Both files are still on disk. **Archive only** never touches an audio file.
- Any ordinary playlist that held the archived copy now lists the keeper at the same place.

## 6. Undo the merge { #undo }

Open the **History & recovery** tab. Under **Duplicate merges**, the newest entry reads **Duplicate merge** with the time and the counts *1 archived · 0 restored · 0 updated · 1 playlists*. Select **Undo merge**. The message reads *Merge undone. Original files and library state restored.* The archived copy returns to **Tracks** and the playlists are as they were.

Undo works only while nothing relevant has changed since. [Undo a merge or put back removed files](undo-maintenance.md) lists exactly what blocks it.

## Delete the extra files instead { #delete }

Choose **Delete from disk** when you want the copies gone from your drive, not just from the library. Scan, review and choose the keeper as before, tick **Delete from disk** and select **Preview selected groups**. Nothing is deleted until you apply, and the preview shows exactly which files would go.

**Which files can go.** A file is listed only when every one of these holds right now:

- it is available and has not changed since the scan;
- it is proven identical to the kept track: the same file bytes, or the same decoded sound, sample rate and channel count;
- it is not the kept track's own file under another name, not used by any other track that stays in your library, and not in use by another operation;
- neither the track nor the kept track has unfinished work, such as a tag write, a cleanup or a file organisation, and the track is not part of an open metadata draft.

So the groups found by **Artist/title** candidates, which may be different mixes, are never deleted. They can still be archived. The kept track's file is never deleted, and neither are cover images and other files beside the audio.

**What the preview shows.** Under **Review merge changes** you see a table of **File**, **Size** and **Duplicate of** (with *Identical file* or *Same decoded audio*) under *N files will be moved to the Trash*, a list *N files will not be deleted* that gives the reason for each file left alone, and **Library records that leave with them**, such as *3 cue revisions · 1 tag · 2 sync links · 5 other records*. Playlists that held a removed track point to the kept track instead, in the same place.

| Reason shown for a file that stays | What it means |
|---|---|
| The audio is not proven identical to the kept track. | The match was by name, or the sound differs. |
| The file changed after the scan. | Scan again. |
| The file is not available. | See [Missing & broken](find-missing-files.md). |
| Another library track still uses this file. | A second record points at the same file, for example through a link. |
| This is the kept track’s own file. | Two records share one file. |
| The track or file is in use by another operation. | Wait for it to finish. A busy kept track (*The kept track is busy: …*) counts too, and so does work that starts while you apply. The file and its record stay as they are, and the other work carries on. |
| Part of an open metadata draft; save or discard the draft first. | An unsaved metadata edit lists this track. Deleting it would break the draft for the other tracks in it. Save or discard the draft under **Drafts and history** (see [Undo a tag change](../../tagging/tag-history.md)), then preview again. Archive only is not affected. |
| The Trash could not take the file. | The Trash refused it when you applied. The file and its record stay where they are. |

**Trash or permanent.** The files go to the Trash on macOS, the Recycle Bin on Windows and the freedesktop.org Trash on Linux. Some files cannot be trashed, for example on a network share or a read-only volume. The preview lists those first, under *N files cannot go to the Trash and will be permanently deleted*, with the warning that they cannot be recovered, and the apply button names them: **Delete 3 files (1 permanently) and merge**. Only those files are ever deleted for good, and only after the Trash has been tried and has refused them. A file the preview sent to the Trash that the Trash then refuses stays where it is.

When you select the button, the message reads, for example, *Merge complete. 2 files moved to the Trash · 0 permanently deleted · 0 not deleted · 0 updated. Undo is in History & recovery.* and **Last merge** lists every file.

**Tags from a donor.** If you chose a **Tag and cue donor**, its tags, cues and grid are copied onto the keeper first. Without a donor, the removed tracks' tags and cues leave the library with their records.

**Undo brings back the records, not the files.** **Undo merge** restores the library records, their playlist places and their metadata, but it cannot take a file out of the Trash. Restore the files from the Trash yourself; Undo refuses to run if a different file now sits where a removed track's file was, and if the restored file was already added again as a new track. CueRithm keeps the records for your last 10 removals, counting duplicate merges that delete files and [Missing & broken removals](find-missing-files.md#remove) together; after that the removal is final. Merges that only archive count toward the same 10. See [Undo a merge or put back removed files](undo-maintenance.md#undo-delete).

Removing a track also clears what was built from it: its cached waveforms, fingerprints and analysis results, and the saved scans that listed it. The cover art stays until the removal can no longer be undone.

## Choose a match method { #match-methods }

**Match method** decides what counts as a duplicate. Everything except **Exact decoded audio** finds candidates by name, and a candidate can be a different mix.

| Match method | What it finds | Use it when |
|---|---|---|
| **Exact decoded audio** | Tracks whose decoded sound is identical: the same samples, sample rate and channel count, in any file format. It also lists tracks with the same artist and title and a length within five seconds as **Artist/title candidates**. | You want certain duplicates. This is the default. |
| **Artist/title · low tolerance** | Same artist, and titles that are nearly identical (about 90 % similar). | Typos and small spelling differences. |
| **Artist/title · medium tolerance** | Same artists and the same title once version words such as *Extended Mix*, *Radio Edit* or *Remix* are ignored. Length is not compared. | The same song in several mixes. |
| **Artist/title · high tolerance** | Similar titles (about 70 % similar, 50 % for very short titles) with version words ignored. | Messy tags. Expect false matches. |
| **Artist/title · ignore parentheses** | Like high tolerance, with everything in brackets removed first. | Titles that differ only in a bracketed note. |

A track shorter than 15 seconds or longer than 25 minutes is not decoded. It appears under **File checks needing attention** as **Duration skipped** and can still match by name.

!!! warning "Gotcha"
    An extended mix and a radio edit are different recordings. Before you merge an **Artist/title candidates** group, compare the lengths and listen. Leave the group unselected, or tick **Ignore** on the track that is a different mix. **Delete from disk** never deletes such a file; only **Archive only** applies to it.

## Choose what to scan { #scope }

| **Scope** | What is compared |
|---|---|
| **Full library** | Every track that is not in Archive. |
| **Selected playlists only** | Tracks in the playlists you tick, including archived members. |
| **Full-library groups touching selected playlists** | The whole library, but only groups that contain a track from the playlists you tick. |

With either playlist scope, a list of your playlists, folders and smart playlists appears. **Scan duplicates** stays off until you tick one. **Exclude paths, one per line** skips every track under the folders you list, as full paths such as `/Users/you/Music/Samples`.

## Let a preference pick keepers { #preferences }

Under **Keeper preferences**, **Prefer** sets the keeper in every group at once. Choose a rule, fill in **Folder path** or **Text** when the rule needs one, and select **Apply preference**.

| **Prefer** | Picks |
|---|---|
| **Highest bitrate** | The available track with the highest bitrate. |
| **Most cue points** | The available track with the most cues. |
| **Folder** | A track whose file sits under the path you enter. |
| **Title contains** | The first track whose title contains your text. |
| **Extra 1 contains** / **Extra 1 does not contain** | A track by the text in its Extra 1 field. |

A preference changes a group only when it finds a better track, and it skips locked groups unless **Override locked groups** is ticked. **Use separate best-cue donor** also sets the track with the most cues as donor when the new keeper has fewer.

## Copy tags and cues from another copy { #donor }

The copy with the best audio is not always the copy with the best work. Choose it as **Keeper**, then pick the other copy under **Tag and cue donor**. On apply, the donor's artist, title, genre, key, BPM, energy, comment, label and the other tag fields, its cues and its beatgrid are copied onto the keeper. Empty donor cues or grid leave the keeper's own in place.

This is the one step that writes to a music file: the keeper's file gets the donor's tags. CueRithm records the keeper's tags from before the copy, and **Undo merge** writes them back (CueRithm 0.1.19 or later). It keeps no copy of the audio, and the audio of the file never changes. Both tracks must be finished in **Tracks**; a track still in Incoming stops the merge with *Prepare incoming tracks before merging*.

**Lock group** protects a group from preferences and from being selected. When a locked group has a donor, **Unlock for copy only** copies the donor's tags, cues and grid onto the other copies and archives nothing, and **Unlock for merge** makes it a normal merge.

## Keep a track out of duplicate groups { #ignore }

Tick **Ignore** beside a track that is a genuine variant, for example a different mix that only looks like a duplicate. An ignored track cannot be keeper or donor, is never archived and no longer forms groups. The choice is saved with the library. To reverse it, open **Ignored tracks** below the review, select **Load ignored tracks** and then **Stop ignoring**. That works even when the review it came from is out of date.

## What can go wrong { #problems }

| What you see | Why | What to do |
|---|---|---|
| **Scan duplicates** is greyed out | **Scope** needs at least one playlist | Tick a playlist or choose **Full library** |
| **0 duplicate groups** | The copies differ in sound (an MP3 and a FLAC), one is in Archive, or the file is shorter than 15 seconds | Try **Artist/title · medium tolerance**, or scan with **Selected playlists only** to include archived members |
| A banner reads *Inputs changed. Rescan before continuing.* | The library, a playlist or a file changed after the scan | Select **Reload review**, or scan again |
| The checkbox beside **Group 1** is greyed out | The group is locked, or its keeper or donor is ignored or unavailable | Choose another keeper or donor, or unlock the group |
| *Another maintenance operation is still running* | Only one scan or apply runs at a time | Wait, or select **Cancel operation** |
| *Playlist references changed; preview again* | A playlist changed between preview and apply | Preview again |
| *Duplicate comparison budget exceeded; narrow the scan scope* | A name-based scan compared too many pairs | Use a playlist scope or **Exclude paths** |
| *N tracks skipped: file not found…* above the groups | Those tracks have no file to compare | Select **Open Missing & broken** and repair or remove them |
| *Cannot merge: … no longer an available audio file* | A file was lost after the scan | Scan again, and review the track in Missing & broken |
| **Preview selected groups** is greyed out | No group is ticked, or you have not chosen **Delete from disk** or **Archive only** | Tick a group and choose |
| *Nothing in this preview can be deleted.* | Every file failed a check, so **Delete** has nothing to do | Read the reasons under *will not be deleted*, or choose **Archive only** |

For tracks the scan left out, see [Find missing, moved or broken files](find-missing-files.md). For other file trouble, see [I moved or renamed my files](../../help/troubleshooting/files-moved.md).

## What you learned

- **Exact decoded audio** finds identical sound regardless of format, name or tags.
- Tracks whose file is not available never appear in a group; Missing & broken is where you deal with them.
- You review a group, choose a keeper, choose **Archive only** or **Delete from disk**, preview and only then apply.
- Archiving moves the other copy to Archive, **Delete from disk** moves its file to the Trash, and both repoint playlists and can be undone from **History & recovery**.

## Next steps

- [Undo a merge or put back removed files](undo-maintenance.md)
- [Find missing, moved or broken files](find-missing-files.md)
- [Remove the records of files that are gone](find-missing-files.md#remove)
- [Archive tracks you no longer play](../archive.md)
- [Reorder, remove and restore tracks in a playlist](../../playlists/edit-playlist-tracks.md)

<!-- verified: apps/desktop/src/views/MaintenanceDuplicates.tsx (skipped notice, disposition choice, MergePreview, Delete N files (K permanently) and merge, Last merge), MaintenanceShared.tsx (skippedNotice, skipReasons, undoLimitNote), MaintenanceHistory.tsx, Maintenance.tsx; crates/cuerithm-maintenance/src/lib.rs (preselect, tolerances), review.rs (preferences, lock, ignore), merge.rs (archive_ids, delete_ids, playlist replacement), import.rs, normalize.rs; crates/cuerithm-library/src/maintenance/scan.rs (scopes, 15 s / 1500 s skip, exclude paths, unavailable tracks skipped_unavailable), deletion.rs (which files may be deleted, permanent), donors.rs (apply_confirmed, undo, links_not_restored), trash.rs (SystemTrash), files.rs (availability refusal text), purge.rs, track_removal.rs, undo_window.rs (shared limit 10, cover kept); tests/maintenance_availability.rs, maintenance_deletion.rs, removal_engine.rs; docs/development/maintenance-api.md -->
