# Archive tracks you no longer play

<p class="lead">Archive takes tracks out of Tracks and out of your playlists' views without deleting anything: the audio file, the analysis and the cues all stay. It takes a few seconds, and Restore undoes it.</p>

<!-- figure:existing-80-archive-archive -->
<figure markdown>
![The Archive view with one track](../assets/screens/80-archive.webp){ .screen loading=lazy }
<figcaption>Archived tracks leave Tracks and playlists but keep their analysis.</figcaption>
</figure>
<!-- /figure -->

## Archive tracks { #archive-tracks }

1. Select the tracks you want to put away in **Tracks**, a playlist or **Incoming**.
2. Select **Archive** in the toolbar above the table. For one track, you can select the **⋯** button at the end of its row and choose **Archive track** instead.

The tracks leave the list at once. The **Archive** count in the sidebar goes up and the **Tracks** count goes down.

## See what you archived { #archive-view }

Choose **Archive** under LIBRARY in the sidebar. The title area reads *Archived tracks keep their analysis and cues.* The table behaves like Tracks: search, sort, play and select. An empty Archive reads *Your archive is empty*.

## Restore tracks { #restore }

1. Open **Archive** and select the tracks.
2. Select **Restore** in the toolbar, or **⋯** and **Restore to library** on one row.

Where a restored track lands depends on what changed while it was away:

- **Nothing changed** (same file contents, same preparation settings): it reappears in Tracks straight away.
- **The file or your settings changed:** it goes through **Incoming** and is prepared again with today's rules, then returns to Tracks.
- **The file is gone:** the track appears in Incoming with *Source unavailable* and waits. Reconnect the drive or put the file back, then press **Retry**; see [Fix a track that needs attention](needs-attention.md).

## What changes when a track is archived { #what-changes }

| Place | Archived track |
|---|---|
| Tracks and playlist views | Hidden |
| Incoming | A track that was preparing or stuck stops being prepared and leaves Incoming |
| Ordinary playlists | The track stays in the playlist's membership and returns to view when restored |
| Smart playlists | Excluded, unless a rule asks for *Archived* tracks explicitly; see [Smart playlist fields and operators](../reference/smart-playlist-fields.md) |
| Analysis and cues | Kept |
| The audio file | Untouched: not moved, renamed or deleted |
| Analyze again | Refused until you restore the track |

Archive is the way to put a track away, not to delete it, and it does not touch the audio file. To remove a file from disk, delete it in your file manager; the track then shows as missing in [Find missing, moved or broken files](library/find-missing-files.md), where you can also remove its record.

## Archive a track that will not prepare { #archive-stuck }

A track that stops in Incoming with an error you cannot fix, such as a damaged file, can be archived. It leaves Incoming and stops counting in *N need attention*. If you later replace the file, restore the track and it is prepared again.

## Archives and duplicates { #duplicates }

Merging duplicate tracks with **Archive only** archives the records you did not keep and updates your playlists to point at the one you did. You will find those records in Archive. With **Delete from disk** (CueRithm 0.1.17 or later) the other copies' files go to the Trash and their records are removed instead, so nothing is added to Archive ([Delete the extra files instead](library/find-duplicates.md#delete)). The merge has its own undo; see [Find and merge duplicate tracks](library/find-duplicates.md) and [Undo a merge or put back removed files](library/undo-maintenance.md).

## What can go wrong { #what-can-go-wrong }

| What you see | Why | What to do |
|---|---|---|
| A message about file publication running | CueRithm is writing tags or moving a file | Wait a moment and try again |
| A message to recover a pending metadata publication | A tag edit on that track did not finish | Finish or discard it in **Drafts and history** first |
| A track is missing from a playlist | It is archived; membership is kept | Restore it |
| A restored track returned to Incoming | Its file or your settings changed | Wait for it to finish; see [Follow a track through Incoming](follow-incoming.md) |

## Related

- [Follow a track through Incoming](follow-incoming.md)
- [Analyze a track again](../analysis/analyze-again.md)
- [Find and merge duplicate tracks](library/find-duplicates.md)
- [Reorder, remove and restore tracks](../playlists/edit-playlist-tracks.md)

<!-- verified: apps/desktop/src/App.tsx (archiveSelected, archive buttons shown by view, Archive view title/subtitle/empty text); apps/desktop/src/components/TrackTable.tsx (Archive track / Restore to library); apps/desktop/src/components/Sidebar.tsx (Archive count); crates/cuerithm-library/src/store.rs (set_archived: supersedes queued/running/attention jobs; restore re-enqueues with current revision; Source unavailable; counts; error messages for file publication and pending metadata publication); crates/cuerithm-library/src/jobs.rs (enqueue_one: archived tracks return their current job; same hash and revision reuse the completed job; retry refuses archived); crates/cuerithm-library/src/playlists/queries.rs (archived hidden from Tracks views; raw membership kept); crates/cuerithm-smartlists/src/lib.rs (archived only with an Archived rule); apps/desktop/src/test/archive-actions.test.tsx; legacy guides/incoming-archive.md#archive re-checked. Code-read, not run: restore returning straight to Tracks when nothing changed. -->
