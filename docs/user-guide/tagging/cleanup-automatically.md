# Run cleanup on every new track

<p class="lead">Switch on one setting and every new track gets your cleanup rules while it is still in Incoming, so titles, genres and artists arrive tidy. It takes about two minutes once the rules exist.</p>

**Before you start**

- Try your rules on a few tracks first with [Clean up messy titles, genres and artists](cleanup-messy-titles.md). Automatic cleanup writes into every new file, and it has no Undo button of its own.
- Make sure the rules are the ones you want in [Write cleanup rules](cleanup-rules.md). Manual cleanup and automatic cleanup share one rule set.

## 1. Switch it on { #switch-on }

1. Choose **Settings** in the sidebar and open the **Cleanup** page. Its heading reads "Shared rules for automatic Incoming and manual cleanup."
2. Tick **Clean incoming tracks automatically**. The note beside it reads "Apply after catalog tags, before completed tracks enter your library."
3. Check the **Smart Fixes**, **Genres** and **Artists** sections below it. They are the same rules you edit in **Cleanup**. In Settings the genre and artist lists do not show your library's values, so type mappings with **Add mapping**.
4. Press **Save settings**. If a rule is invalid the button stays grey and a banner names the problem.

From now on, every track that starts preparation after you saved goes through cleanup. Tracks already in **Tracks** are not rewritten; use [manual cleanup](cleanup-messy-titles.md) for those.

## What happens to a new track { #what-happens }

Cleanup is one step of [preparation in Incoming](../files/follow-incoming.md):

1. CueRithm decodes and [analyses](../how-it-works/pipeline.md) the file.
2. If [automatic tagging](tag-new-tracks-automatically.md) is on, it matches the track in the catalogs you chose.
3. It prepares the tags: catalog fields first, then any [edit you saved on this track](edit-tags-by-hand.md), then your cleanup rules.
4. It writes the result into a staged copy of the file and checks that the audio is unchanged.
5. [Cues are placed](../analysis/cue-rules.md). If cleanup changed the genre, placement uses the cleaned genre.
6. The staged copy replaces the original file in one swap. The original is kept only until the write has settled.
7. Optional [file organization](../rename/organize-files.md) runs last, and the track moves into **Tracks**.

You do not need automatic tagging for this. With tagging off, cleanup runs on the tags already in the file and nothing goes online. Cleanup never contacts a provider.

Categorised tags created by [genre conversion](cleanup-rules.md#genres) are attached to the track in the library at the same step.

## Your own edits win { #your-edits }

If you saved an edit in **Edit metadata** on an Incoming track, cleanup does not rewrite the fields you changed. It still tidies the fields you left alone. This keeps a title you fixed by hand from being changed back by a rule.

## What automatic cleanup does not do { #what-it-does-not-do }

- **No Undo history.** Only manual batches appear under **Undo history** in **Cleanup**. For an automatic run, correct the tag by hand with [Edit tags and artwork](edit-tags-by-hand.md), or change the rule and run a manual cleanup. The original file is kept as a recovery backup; its size appears under **Settings → Automatic tagging → Recovery backups**.
- **No rewrite of old tracks.** Changing a rule affects only tracks that start preparation afterwards. A track that is already being prepared keeps the rules that were saved when it was queued.
- **No measurements changed.** Key, BPM, beatgrid, energy and the position of cues come from analysis, not from your tags.

## Use different rules for different folders { #presets }

A [preparation preset](../files/presets.md) stores its own cleanup switch and rules, together with cue, tagging and file organization rules. A watch folder with a preset assigned uses the preset's rules; every other folder uses the global rules from Settings. See [Use different rules for different folders](../files/folder-presets.md).

## What can go wrong { #what-can-go-wrong }

| Symptom | Cause | Fix |
|---|---|---|
| A new track kept its messy title | The setting was not saved, the track was queued before you saved, or its folder has a preset with cleanup switched off | Check **Settings → Cleanup**, then run manual cleanup on the track; check the folder's preset |
| A track needs attention after cleanup | The result cannot be saved, for example a genre conversion would put two choices into a **One choice** category and the job stops with an error | Fix the rule or the category, then follow [Fix a track that needs attention](../files/needs-attention.md) |
| Genres in the library changed unexpectedly | A genre mapping is on | Read [Genres](cleanup-rules.md#genres); pause the rule by unticking **Enable genre cleanup** |
| Everything looks right but a smart playlist went empty | The playlist matched a genre or title your rules renamed | Edit the playlist rule in [Write smart playlist rules](../playlists/smart-rules.md) |

For a broader checklist, see [Cleanup changed too much](../help/troubleshooting/cleanup-changed-too-much.md).

## Related

- [Write cleanup rules](cleanup-rules.md)
- [Undo a cleanup](undo-cleanup.md)
- [Follow a track through Incoming](../files/follow-incoming.md)
- [Cleanup settings](../reference/settings/cleanup.md)

<!-- verified: apps/desktop/src/views/CleanupSettings.tsx (Settings page, labels); apps/desktop/src/views/Settings.tsx; crates/cuerithm-library/src/prepare.rs (order: tag plan with cleanup, cue refresh with staged genre, publish, then organization); crates/cuerithm-library/tests/cue_genre.rs; crates/cuerithm-library/src/preparation_presets/model.rs (cleanup in preset rules); docs/automatic-tagging.md; apps/desktop/src/api.ts (only undoCleanup exists, no automatic undo); legacy guides/cleanup-rules.md#automatic-cleanup re-checked -->
