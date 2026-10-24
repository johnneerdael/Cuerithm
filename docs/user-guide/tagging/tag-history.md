# Undo a tag change

<p class="lead">Every saved edit is recorded in Drafts and history, where you can read what changed, take it back, or finish a save that was interrupted. Undoing one edit takes about a minute.</p>

**Before you start**, know which change you want to reverse. Edits made with [Edit tags and artwork](edit-tags-by-hand.md), and matches you accept with [Review match](review-a-match.md) on a finished track, are recorded here. Some other changes live elsewhere.

## What you can undo, and where { #what-you-can-undo }

| Change | Where to undo it |
|---|---|
| A saved edit from **Edit metadata** | **Drafts and history → Undo saved changes**, below |
| A match accepted with **Use selected match** on a finished track | The same place |
| Tags written by automatic tagging, or a match accepted on a track still in Incoming | No Undo button, and CueRithm keeps no copy of the original once the write has settled. Correct the fields with [Edit tags and artwork](edit-tags-by-hand.md). |
| A cleanup | [Undo a cleanup](undo-cleanup.md) |
| A rename or move | [Recover or undo a reorganisation](../rename/undo-organize.md) |
| Categories and custom tags | The **Curate** tab in **Drafts and history**; see [Group tracks with categories and custom tags](curate-tracks.md) |
| A cue, loop or beatgrid correction | [Undo or resolve a correction](../analysis/corrections/undo-corrections.md) |

## Undo a saved edit { #results-and-undo }

1. Choose **Drafts and history** in the toolbar above the track table. The button shows a count when drafts are waiting.
2. Stay on the **Metadata edits** tab. Under **Publication history**, each entry shows its state, date and draft revision, and a count such as *1 written*.
3. Choose **Details** on the entry you want. Each track lists its outcome and, per field, the **Before** and **After** values.
4. Choose **Undo saved changes**. CueRithm checks that the file and its tag records are still exactly as that save left them, restores the earlier values through the same checked write, and adds a new entry with the result.

Entries are listed 25 at a time; use **Newer changes** and **Older changes** to move through them. You can also reach the entry straight after a save: the result panel shows **Undo saved changes** next to **New edit**.

If the file is not in the state the save left it in, the button is not offered. If something changes between your click and the write, CueRithm stops and reports that track as failed instead of overwriting a newer edit.

## Read the states { #read-the-states }

| State or outcome | Meaning |
|---|---|
| *saved* | Every changed track was written |
| *partial* | Some tracks failed or were cancelled; open **Details** to see which and why |
| *queued* | The track was still in Incoming. The edit waits for preparation to apply it. |
| *recovery required* | A save was interrupted. See [Finish an interrupted save](#recover). |
| *undone* | An Undo completed |
| *unchanged* (track) | The edit would not change that file, so nothing was written |
| *failed* (track) | That track could not be written; its file is as it was |

## When Undo is not offered { #when-undo-is-not-offered }

**Undo saved changes** appears only while the track is still ready and not archived, its file and tag records are unchanged since the save, nothing else has claimed the file, and the save is one of your 10 newest (CueRithm 0.1.19 or later; an older entry stays in the history without the button). Any later save, cleanup, rename, reorganisation, or an edit made in another program, turns it off for that entry. Undo the newest change first, or make a new edit that sets the values you want.

## Find an unfinished edit { #drafts }

Edits are stored as drafts while you type, so nothing is lost when you change the selection, search, close the panel or quit.

1. Open **Drafts and history**.
2. Under **Pending drafts**, each draft shows how many tracks it covers, its revision and its state.
3. Choose **Open draft** to carry on in the editor, or **Discard draft** to delete it. A draft that is being saved cannot be discarded.

## Finish an interrupted save { #recover }

If CueRithm closed, the computer lost power or the connection to a drive dropped during a save, the entry reads *recovery required*. Choose **Retry recovery** on that entry. CueRithm uses the evidence it kept to complete the save safely, and the state updates. Opening the app does not silently redo or undo the work, so check the result in **Details**.

## See how much space backups use { #recovery-backups }

**Settings → Automatic tagging → Recovery backups** shows the number and size of the original files CueRithm holds while a write is under way, and any temporary files that remain while cleanup is pending. A finished write holds none, so the row normally reads 0.0 MB · 0 files. Undo restores the values the entry lists under **Before**, not a copy of the file. [Where CueRithm keeps your data](../files/library/where-data-lives.md) says where the rest of the data lives.

## What can go wrong { #what-can-go-wrong }

| Symptom | Cause | Fix |
|---|---|---|
| No **Undo saved changes** on an entry | The track changed again, or it is archived or still in Incoming | See [When Undo is not offered](#when-undo-is-not-offered) |
| Entry says *queued* for a long time | The track is still in Incoming | Follow it in [Follow a track through Incoming](../files/follow-incoming.md) |
| *recovery required* stays after **Retry recovery** | The file is open elsewhere or the drive is unavailable | Close other programs, reconnect the drive and retry |
| An edit I made is not listed | Only saved edits appear; unfinished work is under **Pending drafts** | Open the draft and save it |

## Related { #related }

- [Edit tags and artwork](edit-tags-by-hand.md)
- [Review and fix a wrong match](review-a-match.md)
- [How are tags written safely?](../how-it-works/tag-writing.md): the checks behind every write.
- [Back up your library](../files/library/back-up-library.md): does not include music files, so keep your own copy of the audio.

<!-- verified: apps/desktop/src/components/MetadataPending.tsx (labels, 25 per page); MetadataEditor.tsx (Undo saved changes, New edit, Retry recovery); MetadataEditResult.tsx; apps/desktop/src/App.tsx (Drafts and history button, count); crates/cuerithm-library/src/metadata_edit/journal.rs (finish_batch states, eligibility_inner, undo checks); metadata_edit/service.rs; crates/cuerithm-library/src/match_review/tests.rs (accepted Incoming review has no write-only history); apps/desktop/src/views/TaggingSettings.tsx (Recovery backups); legacy guides/metadata-editing.md#results-and-undo and about/roadmap.md (backup retention) re-checked -->
<!-- UNVERIFIED: exact recovery steps CueRithm runs under Retry recovery (completion versus rollback) were summarised from the journal code, not exercised. -->
