# Clean up messy titles, genres and artists

<p class="lead">In about ten minutes you will remove "(Original Mix)" from five track titles, fold one genre spelling into another, read every proposed change before anything is written, apply the batch and then undo it.</p>

Cleanup tidies the text in your tags with rules you write once. It never changes the audio, and it never changes CueRithm's measurements: the key, BPM, beatgrid, whole-track energy rating and cues stay exactly as they were.

**You need:**

- [ ] A few Ready tracks in **Tracks**. Cleanup does not touch tracks that are still in Incoming or in Archive.
- [ ] Five tracks whose titles end in `(Original Mix)`. The screenshots use five from a Beatport purchase folder: *Brackish*, *Come To Life*, *Desert*, *Play With Fire* and *Undercurrent*.
- [ ] Music you can afford to rewrite. Apply puts new tags into the files, so practise on a few tracks, or on copies, before you run it on the whole library.

## 1. Select the tracks and open Cleanup { #open-cleanup }

In **Tracks**, [search](../analysis/find-tracks.md) for `Original Mix` and [select the matching tracks](../analysis/select-many.md). Then choose **Cleanup** under **TOOLS** in the sidebar.

<!-- figure:existing-70-cleanup-empty-cleanup-messy-titles -->
<figure markdown>
![The Cleanup tool on the Smart Fixes tab with no rules yet](../assets/screens/70-cleanup-empty.webp){ .screen loading=lazy }
<figcaption>Cleanup starts empty.</figcaption>
</figure>
<!-- /figure -->

Cleanup opens with the title **Cleanup**, the line "Review metadata changes. Keep your audio, analysis and cues." and three tabs: **Smart Fixes**, **Genres** and **Artists**. Under the tabs, the **Scope** menu reads **5 selected tracks**, because Cleanup takes whatever was selected when you opened it. If your search also lists tracks marked missing, leave those unselected.

*Why:* a small scope makes the first run quick to check. Later you can switch **Scope** to **All ready tracks**.

## 2. Add a Smart Fix that removes "(Original Mix)" { #add-a-smart-fix }

On the **Smart Fixes** tab, choose **Common title text** in the **New Smart Fix** menu and press **Add Smart Fix**. A card titled **Smart Fix: Common title text** appears. Its **Pattern** menu already reads **Original Mix suffix**, which is what you want. Press **Save cleanup rules**.

<!-- figure:existing-71-cleanup-smart-fixes-cleanup-messy-titles -->
<figure markdown>
![Cleanup with two Smart Fix rules](../assets/screens/71-cleanup-smart-fixes.webp){ .screen loading=lazy }
<figcaption>Add the rules you want before scanning.</figcaption>
</figure>
<!-- /figure -->

The card shows an **Enabled** box, the rule's number in the list and buttons to move it up or down or remove it. **Scan tracks** stays grey until you save, with the note "Save rule changes before scanning." This makes the scan and the later apply use exactly the same rules.

See [Common title text](cleanup-rules.md#fix-common-title-text) for the other three patterns and for what each Smart Fix does.

## 3. Fold one genre spelling into another { #merge-a-genre }

Open the **Genres** tab and tick **Enable genre cleanup**. CueRithm lists every genre in your library with its track count; press **Refresh values** if the list looks stale. Type `Dubstep` into **Search genres** to narrow it.

Find the row for `140 / Deep Dubstep / Grime`, click the box that says **Keep existing** and type `Dubstep`. Press **Save cleanup rules**.

*Why:* a mapping matches the whole genre text exactly, capitals included. Here it changes `140 / Deep Dubstep / Grime` into `Dubstep` and leaves every other genre alone. [Merge similar genres into one](recipes/merge-similar-genres.md) takes this further.

## 4. Scan and read the preview { #scan }

Scroll to the **Scope** menu, check that it still says **5 selected tracks** and press **Scan tracks**. A scan reads the files and builds a preview. It writes nothing: the note beside the button says "Scanning previews changes without writing file tags."

<!-- figure:existing-72-cleanup-preview-cleanup-messy-titles -->
<figure markdown>
![Cleanup preview: 10 proposed changes, 7 unchanged, 0 unavailable](../assets/screens/72-cleanup-preview.webp){ .screen loading=lazy }
<figcaption>Scanning previews every change before anything is written.</figcaption>
</figure>
<!-- /figure -->

You should see **5 proposed changes** and the line "0 unchanged · 0 unavailable". Each track has its own card with a checkbox and the fields that would change:

| Track | Field | Before | After |
|---|---|---|---|
| Brackish | title | `Brackish (Original Mix)` | `Brackish` |
| | genre | `140 / Deep Dubstep / Grime` | `Dubstep` |
| Come To Life | title | `Come To Life (Original Mix)` | `Come To Life` |
| Desert | title | `Desert (Original Mix)` | `Desert` |
| Play With Fire | title | `Play With Fire (Original Mix)` | `Play With Fire` |
| Undercurrent | title | `Undercurrent (Original Mix)` | `Undercurrent` |

*Play With Fire* already has the genre `Dubstep`, so only its title changes. Untick the checkbox beside any track you want to skip; the button then counts only the ticked ones. If you change a rule or the scope now, the preview disappears and you scan again, so you never apply a stale plan.

## 5. Apply the changes { #apply }

Press **Apply 5 changes**. CueRithm shows "Applying cleanup…" and then the **Cleanup result**: "5 applied · 0 unchanged · 0 conflicts · 0 failed".

<!-- figure:existing-74-cleanup-applied-cleanup-messy-titles -->
<figure markdown>
![Cleanup result: 10 applied, 0 unchanged, 0 conflicts, 0 failed, with the Undo history](../assets/screens/74-cleanup-applied.webp){ .screen loading=lazy }
<figcaption>The result, with Undo history below.</figcaption>
</figure>
<!-- /figure -->

For each track CueRithm writes the new tags into a staged copy of the file, checks that the copy still decodes to the same audio and that the tags read back as intended, and only then swaps it in. The tags it replaced are recorded so the batch can be undone, and the track's title and genre in the library are updated at the same time. [How tags are written safely](../how-it-works/tag-writing.md) has the details.

Switch to **Tracks**. The five titles no longer end in `(Original Mix)`. Key, BPM, energy and cues are unchanged, and a track that was playing keeps playing.

## 6. Undo the batch { #undo }

Return to **Cleanup** and scroll to **Undo history**. The batch is listed as **Cleanup** with its date and time, the state **complete** and "5 tracks". Press **Undo** beside it.

A second entry, **Undo**, appears, and the original titles and genre are back in the files and in the library. If you had edited a track again since the apply, Undo keeps your newer edit and reports that track as a conflict instead of overwriting it. The history survives a restart. More on this in [Undo a cleanup](undo-cleanup.md).

## What you learned

- **Smart Fixes**, **Genres** and **Artists** are three tools that share one saved rule set.
- A scan is a preview. Nothing is written until you press **Apply**, and you can untick any proposal first.
- Apply writes the file tags and the library together, records the tags it replaced for Undo and leaves analysis and cues alone.
- **Undo history** reverses a whole batch, track by track.

## Next steps

- [Write cleanup rules](cleanup-rules.md): every Smart Fix, with its options and an example.
- [Run cleanup on every new track](cleanup-automatically.md): use the same rules on each purchase as it arrives.
- [Tag and clean-up recipes](recipes/index.md): ready-made rules for common problems.
- [Cleanup changed too much](../help/troubleshooting/cleanup-changed-too-much.md): what to do if a batch did more than you expected.

<!-- verified: apps/desktop/src/views/Cleanup.tsx; apps/desktop/src/views/CleanupSettings.tsx; apps/desktop/src/App.tsx (onView captures selection for Cleanup); apps/desktop/src/test/cleanup.test.tsx; crates/cuerithm-cleanup (Original Mix suffix and genre mapping outputs run through the engine); crates/cuerithm-library/src/cleanup/mod.rs and journal.rs (scan, apply, undo); legacy learn/cleanup.md re-checked -->
