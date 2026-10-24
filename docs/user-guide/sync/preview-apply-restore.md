# Preview, apply and restore a sync

<p class="lead">Every sync follows the same four steps: close the DJ program, preview, apply that preview, and keep a backup you can restore. This page explains what each step does, what is guaranteed if something is interrupted, and how to recover or undo a sync. It takes about ten minutes to read.</p>

## The flow at a glance { #flow }

| Step | rekordbox 6 / 7 | VirtualDJ, Engine DJ, djay Pro, Serato, Pioneer USB, Mixxx |
|---|---|---|
| Preview | **Preview rekordbox sync** ([tutorial](rekordbox-direct.md#preview)) | **Preview sync** |
| Apply | **Sync rekordbox** | **Sync to** followed by the destination name, for example **Sync to Mixxx** |
| Drop an unused preview | **Discard preview** | **Discard preview** |
| History | **rekordbox sync history** | **Sync history** |
| Finish an interrupted sync | **Recover sync** | **Finish sync** |
| Undo a sync | **Restore backup**, then **Confirm restore** | **Restore backup** |

## 1. Close the DJ program { #close }

CueRithm checks the running processes before it previews and again before it applies. It never closes a program for you. The programs it looks for are rekordbox and rekordbox Agent (also for Pioneer USB, which you prepare with rekordbox closed), VirtualDJ, Engine DJ, djay, Serato DJ Pro and Mixxx. If one is running you see "Close the target DJ software before syncing". Quit it and try again; in the rekordbox view, press the refresh button first so the status updates.

## 2. Preview { #preview }

A preview changes nothing in your DJ library. CueRithm reads the destination, builds the complete change on a private copy in its own data folder, and shows you the result: how many tracks and playlists, which files or rows change, and every warning. Read the warnings; they name tracks that were skipped, colours that were changed, and formats that need conversion.

<!-- figure:existing-85-sync-preview-2-preview-apply-restore -->
<figure markdown>
![A sync preview of 24 tracks and three playlists](../assets/screens/85-sync-preview.webp){ .screen loading=lazy }
<figcaption>The preview says what will change before anything is written.</figcaption>
</figure>
<!-- /figure -->

The preview is bound to the exact state it was built from. It is thrown away automatically when you:

- change the destination, library, [saved setup](saved-destinations.md#use-a-setup), [scope](choose-what-to-sync.md#scopes), playlist or any [option](sync-options.md)
- change anything in CueRithm that the sync reads, such as a tag, a cue or a playlist
- leave the Sync view

An unused preview that is still open is listed in the history with **Discard preview**. Discard it before you prepare another sync to the same rekordbox library; rekordbox refuses a second one until you do ("Discard the existing preview or recover the interrupted rekordbox operation first"). A preview holds a private copy of your rekordbox database, or the files it would change, until you apply or discard it.

## 3. Apply { #apply }

Apply publishes only the preview you are looking at. Immediately before it writes, CueRithm checks again that nothing moved:

- your CueRithm library and preparation settings are unchanged
- each audio file in the sync is byte-for-byte what was analysed
- the destination is byte-for-byte what the preview saw; for rekordbox that includes every table and any edit still held in the database's journal
- the DJ program is still closed

If any check fails you see a message such as "Library changed; create a new rekordbox preview" or "Native database changed after preview". Preview again. Nothing was written.

When the check passes, the sync runs. A result appears: for rekordbox **Sync completed** with counts of tracks, rows changed, fields acknowledged and fields skipped; for the other destinations "Sync complete. Your backup is available below." A second sync of data that is already there says "Everything is already synced." and changes nothing.

## What apply guarantees { #guarantees }

These hold for every destination:

- **A backup exists before the first change.** Each file CueRithm is about to replace or remove is copied into its own sync record first. For rekordbox the backup is a verified copy of the whole library database.
- **Your original music files are not rewritten.** The exception is Serato, which writes cues and the grid into the audio files' tags ([Sync to Serato DJ Pro](serato-dj-pro.md)). Pioneer USB always creates copies on the stick, and Engine DJ and rekordbox create copies only when you switch the copy options on.
- **Stale means stop.** A change made in the destination after the preview stops the apply instead of being overwritten.
- **An interrupted sync is never reported as a success.** The "last sync" record that **Modified since last sync** relies on advances only after the sync is completed or recovered.
- **CueRithm never ends another program's process and never deletes a database's journal files** to get unstuck.

For rekordbox the order of work adds three more guarantees. CueRithm takes an exclusive lock on the database and saves the verified backup first. It then writes the new analysis and artwork files as new versions, leaving the existing files alone. It changes the database last, in one database transaction, and checks inside that transaction that the result is exactly what the preview showed. If rekordbox or its Agent starts while CueRithm is writing files, the database is not changed.

## Find a sync in the history { #history }

The history lists previews, completed syncs and syncs that were interrupted. It survives restarting CueRithm.

| rekordbox status | Meaning | Buttons |
|---|---|---|
| `prepared` | A preview that was never applied | **Discard preview** |
| `publishing` | Stopped while writing, before the database changed | **Recover sync** |
| `native published` | The database changed but CueRithm did not finish recording it | **Recover sync**, **Restore backup** |
| `applied` | Complete | **Restore backup** |
| `restoring` | A restore stopped partway | **Recover sync** |
| `restored`, `rolled back` | Settled | none |

For the other destinations the history shows the destination name, a state (`prepared`, `applying`, `applied`, `restoring`, `restored`), the track count and the date. **Discard preview** appears for `prepared`; **Finish sync** appears when the files were published but CueRithm has not recorded the success; **Restore backup** appears for `applied`, `applying` and `restoring`; **Finish restore** appears when a restore finished but was not recorded.

## Recover an interrupted sync { #recover }

If CueRithm quits, the computer sleeps or power is lost during a sync, keep the destination connected and available, reopen CueRithm and open the history.

- **Recover sync** (rekordbox) looks at the library. If the database already holds the planned result it finishes recording it and reports **Sync completed**. If the database is still in its original state it removes the half-written files and reports **Sync rolled back**. If the database matches neither, because something else changed it, recovery refuses ("Native database changed during interruption; recovery refused") and changes nothing.
- **Finish sync** (other destinations) completes the recording of a sync whose files were already published. **Restore backup** is available when you would rather roll the interrupted sync back.
- **Finish restore** completes a restore that stopped partway.

Reconnect a USB stick at its original mount location or drive letter before you resume its history. If the file system cannot identify the volume or its catalog, sync stops before publishing.

## Restore a backup { #restore }

**Restore backup** puts the destination back as it was before that sync. rekordbox asks you to confirm first: **Restore this backup?**, then **Confirm restore** or **Keep current library**. Close the DJ program before you restore.

A restore is allowed only while the destination still matches the result of that sync. It stops with a message, and changes nothing, when:

- the DJ program changed the library or its files after the sync ("Native database changed after publication; restore refused")
- an audio file that was part of the sync changed ("A synced audio source changed after publication; restore refused")
- a later sync owns the same destination. Restore the later one first ("A later sync owns this target; restore that sync first")

Do your checking in the DJ program with a copy or a small playlist, and restore soon, because any edit you make in the program afterwards blocks the restore. A restore does not overwrite your current music files. After it, the next preview starts from the restored state: for rekordbox, tracks the sync had added count as new again, and for the other destinations **Modified since last sync** sends everything again.

Serato's backup includes the audio files whose tags it changed; a Pioneer USB conversion backs up and restores the affected copies on the stick.

### How long a backup lasts { #backup-lasts }

CueRithm keeps the backup of the newest 10 syncs of each DJ program. When an eleventh sync of the same program finishes, the oldest one stays in **Sync history** with its state and counts, and a line reads *The backup was removed after ten newer syncs of this program* (rekordbox: *of this database*). It has no **Restore backup** button any more, and the backup files it used are deleted from CueRithm's data folder. A restored sync, or one rolled back after an interruption, gives its backup up at once, because it cannot be restored twice. A sync that has not been finished or acknowledged keeps its backup however old it is. This limit needs CueRithm 0.1.19 or later; earlier versions kept every backup, including the audio copies of Serato and Pioneer USB syncs, without a limit.

## What can go wrong { #what-can-go-wrong }

| What you see | Why | What to do |
|---|---|---|
| "Close the target DJ software before syncing" | The program, or for rekordbox its Agent, is running | Quit it and preview again |
| "Another DJ sync operation is running" or "Another maintenance operation is still running" | Sync, Maintenance or another long job is already working | Wait for it, or **Cancel** it |
| "Library changed; create a new rekordbox preview" | You edited something after previewing | Preview again |
| "Restore refused" messages | The destination changed after the sync | The sync stays in place. Fix things in the DJ program, or keep the result |
| The history shows `publishing` or `applying` after a crash | The sync was interrupted | Use **Recover sync**, **Finish sync** or **Restore backup** as described above |

For rekordbox symptoms that are not about a failed sync, see [My sync does not show up in rekordbox](../help/troubleshooting/rekordbox-sync-problems.md).

## Related

- [Sync to rekordbox 6 or 7](rekordbox-direct.md): the first sync, step by step
- [Choose what to sync](choose-what-to-sync.md): what a preview covers
- [Set the sync options](sync-options.md): changes that make a new preview necessary
- [Where CueRithm keeps your data](../files/library/where-data-lives.md): the backups live there

<!-- verified: apps/desktop/src/views/RekordboxSync.tsx; views/Sync.tsx (history buttons, messages); src/test/rekordbox-sync.test.tsx; src/test/sync.test.tsx; apps/desktop/src-tauri/src/rekordbox_sync_commands.rs (error text); crates/cuerithm-library/src/rekordbox_sync_service/publication.rs (apply, restore_finish, recover), tests.rs (interruption tests); crates/cuerithm-library/src/dj_sync_service.rs (restore_dj_sync, history_dj_sync); crates/cuerithm-dj-sync/src/discovery.rs (ensure_closed, process names); docs/research/dj-sync-playlist-ownership.md; legacy guides/sync.md re-checked -->
<!-- UNVERIFIED: that merely launching rekordbox without editing anything leaves a restore possible; the contract only records that a tag reload and a reanalysis inside rekordbox block it -->
