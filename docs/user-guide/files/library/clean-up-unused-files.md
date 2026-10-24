# Find and remove unused files

<p class="lead">Scan a music folder for files that no track in your library uses, check the list, export it or remove the files you choose. A scan takes a few minutes per large folder, and removal always shows a preview first.</p>

<!-- figure:existing-maintenance-unused-preview-clean-up-unused-files -->
<figure markdown>
![Reviewed unused-file removal with an associated sidecar and an empty parent folder](../../assets/screens/maintenance-unused-preview.png){ .screen loading=lazy }
<figcaption>The preview lists every file that would be removed.</figcaption>
</figure>
<!-- /figure -->

**Before you start:** pick one folder, not your whole Music folder. CueRithm compares every file in it with your library, so a narrow folder gives a short, readable list.

!!! note "Which version"
    Removed files go to the Trash or Recycle Bin, and CueRithm keeps no copy, from CueRithm 0.1.19 or later. Earlier versions deleted the files and kept a copy in the data folder for a **Restore files** button.

## Scan a folder { #scan }

1. Choose **Maintenance** under TOOLS in the sidebar and open the **Unused files** tab.
2. In **Scan folder**, type a folder path or select the folder button (**Choose scan folder**) to pick one.
3. Set **Extension filter**. **All extensions** lists every file. **Include extensions** lists only the types you name, and **Exclude extensions** lists everything except them. Type the names in **Extensions**, for example `wav, flac, mp3`, separated by commas or spaces. A leading dot is ignored.
4. Select **Scan folder**.

A progress line reads *Scanning unused files…* with the folder being read. Scanning and exporting never remove files.

Prefer **Include extensions** with your audio types. With **All extensions** the list also contains cover images, text files and anything else in the folder, because none of those belongs to a library track.

## Read the result { #result }

The heading reads, for example, **3 unused files · 41.2 MB**, with a badge **Complete scan** or **Partial scan**, and the line **Scanned root:** shows the exact folder that was read. The table lists **Select**, **Unused path** and **Size**.

A **Partial scan** means CueRithm could not read part of the folder, and the files it could not read are listed with the reason. A partial report can be exported but not used for removal. Fix the cause, for example a permissions problem, and scan again.

## What counts as unused { #what-counts }

A file is unused when no library record points at it. CueRithm matches by path and also by the file itself, so a shortcut, a hard link or a differently spelled path to a used file still counts as used.

- Tracks in [Archive](../archive.md) still use their files. Archiving a track protects its file from this scan.
- Tracks still in [Incoming](../follow-incoming.md) count as using their files too.
- Stem partners are never listed alone. A VirtualDJ `.vdjstems` file or a Serato `.serato-stems` file travels with its song: when you select the song, the first partner found appears in the preview as an **Associated sidecar**.

## Folders CueRithm will not scan { #protected }

To keep you away from files you did not mean to touch, a scan refuses these:

| Not scanned | What you see |
|---|---|
| Your home folder, drive roots and system folders such as `/Users`, `/Applications` or `C:\Windows` | *Choose a narrower music folder; system and home roots are excluded* |
| CueRithm's own data folder and DJ library folders under Music: `_Serato_`, `PioneerDJ`, `Traktor`, `Engine Library`, `iTunes` | *Selected folder is protected application or DJ storage; choose a music folder* |
| A folder that is a symbolic link | *Choose the actual folder, not a directory symlink* |
| Inside the folder: trash folders, `Application Support` on macOS, `AppData` on Windows, `.DS_Store` and `._` files | Skipped without a message |

## Export the list instead { #export }

Select **Export path list** to see the paths as plain text in a box titled *Path list from report*. Copy it into a note or a script. Nothing is written to disk and nothing is removed. The note under the box says whether the report was *Complete* or *Partial* and whether it is *Current when exported* or *Stale; paths may have changed*.

## Preview and remove { #remove }

1. Tick **Select** beside each file you want to remove, or tick **Select all**. Nothing is selected for you.
2. Select **Preview removal**. The section **Review files to remove** lists every file with its role (**Selected file**, **Associated sidecar** or **Folder metadata**) and size, and says how many files and how much space are involved.
3. If removing the files would leave a folder empty, **Reviewed empty parent folders** lists it. A folder is removed only when nothing is left in it except `.DS_Store` or `Thumbs.db`. The folder you scanned always stays, and so does a folder that still holds anything else, such as cover art.
4. Select **Remove reviewed files**. The message reads, for example, *Removal complete. 3 of 3 files moved to the Trash.* **Discard preview** backs out.

The files go to the Trash on macOS and Linux and to the Recycle Bin on Windows, each under its own name, so you can put it back with the operating system (**Put Back** in the macOS Trash). CueRithm keeps no copy of them and has no **Restore** button for them. The record of the removal stays in [History & recovery](undo-maintenance.md#unused-removals).

### Files that cannot go to the Trash { #permanent }

Some places have no Trash: a network share, a read-only volume, a drive that the system does not keep a Trash on. The preview marks each file CueRithm expects the Trash to refuse with **Deleted permanently**, and a warning above the list says how many there are.

!!! danger "A file marked Deleted permanently cannot be recovered"
    CueRithm still tries the Trash first. Only a file the preview marked is ever deleted, and only after the Trash refused it. Any other file the Trash refuses stays where it was, and the message says so, for example *1 left where it was because the Trash refused it.* Discard the preview if you do not want a marked file deleted.

Removal is refused when a report is out of date. Any change to the library, or to a file in the scan, marks it **Stale**, and **Select all** and **Preview removal** switch off. Scan again.

## What can go wrong { #problems }

| What you see | Why | What to do |
|---|---|---|
| **Scan folder** is greyed out | No folder is set, or **Include extensions** has no extensions | Fill in the missing field |
| *Partial scan. Resolve the issues and scan again before removal.* | A folder or file could not be read | Fix access, then scan again |
| *Library or maintenance inputs changed. Reload or rescan before removal.* | The library changed after the scan | Select **Scan folder** again |
| A file you expect is not listed | A library track, even an archived one, uses it | Check **Archive**, or [find missing files](find-missing-files.md) |
| *1 left where it was because the Trash refused it* | The system Trash could not take the file and the preview had not marked it | Nothing was deleted. Free the cause, for example reconnect the drive, then scan and remove again |
| A removal shows *Cleanup pending* in History & recovery | CueRithm quit before every file reached the Trash | Start CueRithm again. It finishes the removal, or select **Retry file recovery** |

## Related

- [Undo a merge or put back removed files](undo-maintenance.md)
- [Find and merge duplicate tracks](find-duplicates.md)
- [Archive tracks you no longer play](../archive.md)
- [Where CueRithm keeps your data](where-data-lives.md)

<!-- verified: apps/desktop/src/views/MaintenanceUnused.tsx, Maintenance.tsx, MaintenanceShared.tsx; crates/cuerithm-library/src/maintenance/unused.rs (referenced by path and file identity, archived records included, protected roots and DJ folders, exclusions, sidecars), removal.rs (preview with `permanent`, Trash, outcomes), removal/legacy.rs, removal/folders.rs (parent cleanup); docs/development/maintenance-api.md; legacy guides/maintenance.md re-checked. The 3 files / 41.2 MB values are placeholders for a real scene and must match the captured figure -->
