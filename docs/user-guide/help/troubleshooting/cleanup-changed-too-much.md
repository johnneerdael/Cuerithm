# Cleanup changed more than I expected

<p class="lead">Cleanup rewrites titles, artists, genres and other tags according to the rules you saved. If it changed tracks you wanted to keep as they were, you can undo the run, tighten the rules and run it again. This takes about ten minutes.</p>

## Why it happens { #why }

Cleanup only edits human-readable metadata such as title, artist, album and genre. Your audio, key, BPM, beatgrid, energy and cues stay as they are. Within that, the result is the sum of your saved rules, so surprises come from one of these:

| Cause | Example from the demo library |
|---|---|
| **The scope was wider than you thought.** With nothing selected, the scope is **All ready tracks**. | Five tracks end in `(Original Mix)`: *Brackish*, *Come To Life*, *Desert*, *Play With Fire* and *Undercurrent*. A **Common title text** fix with the **Original Mix suffix** pattern removes the suffix from all five at once. |
| **A rule is broader than its name suggests.** Each Smart Fix has its own options. | **Characters to spaces** replaces every listed character in the title. **Casing** turns fully upper-case or lower-case values into title case. |
| **Rules run in order, and later ones see earlier results.** | Smart Fixes run in list order, followed by genre mappings and artist mappings. |
| **A mapping rewrote a whole genre or artist.** | Genre and Artist mappings change every track that has that exact value. Values you protect are left alone. |
| **Cleanup ran on its own.** | **Clean incoming tracks automatically** in **Settings** → **Cleanup** applies the saved rules to every new track after catalog tagging and before the track enters Tracks. |
| **You applied every proposal without reading it.** | **Apply N changes** writes all ticked proposals, and all proposals start ticked. |

Cleanup records the tags it replaced. That is what makes undo possible.

## Fix it { #fix }

1. **Undo the run.** Choose **Cleanup** in the sidebar. In **Undo history**, find the run by its time and track count and choose **Undo**. [Undo a cleanup](../../tagging/undo-cleanup.md) has the details. Undo restores the original file bytes and library metadata when the file still matches what the cleanup wrote. If you edited a track afterwards, CueRithm keeps your newer edit and reports a conflict instead of overwriting it.
2. **If the run is not in Undo history**, it was probably made automatically while tracks were prepared. Fix individual values with **Edit metadata** ([Edit tags and artwork](../../tagging/edit-tags-by-hand.md), [Undo a tag change](../../tagging/tag-history.md)). CueRithm keeps the originals in the data folder as recovery backups.
3. **Stop automatic runs.** In **Settings** → **Cleanup**, turn off **Clean incoming tracks automatically** and choose **Save settings**. Changing rules or settings never rewrites tracks that are already finished.
4. **Tighten the rules.** In **Cleanup**, on **Smart Fixes**:
    - Untick **Enabled** on a rule you do not want, or remove it.
    - Add **Ignored values** (one per line, exact whole values) to skip particular titles.
    - On **Genres** and **Artists**, use **Protect** for values that must never change.
    - Check the order with **Move up** and **Move down**.
   [Write cleanup rules](../../tagging/cleanup-rules.md) explains every fix.
5. **Narrow the scope.** Select the tracks you want in **Tracks**, then open **Cleanup** and set **Scope** to the selection.
6. **Scan, read, untick, then apply.** Choose **Save cleanup rules**, then **Scan tracks**. A scan writes nothing. Read each proposal's before and after, untick the ones you do not want, and only then choose **Apply**. [Clean up messy titles, genres and artists](../../tagging/cleanup-messy-titles.md) shows the whole flow.

## Check it worked { #check }

- After **Undo**, the history shows an *Undo* entry for the batch, and **Tracks** shows the original titles and artists again.
- The cleanup result line reads *N applied · 0 unchanged · 0 conflicts · 0 failed* for the second, narrower run.
- A scan with the new rules lists only the changes you intended.

## Still stuck? { #still-stuck }

If a conflict stays after Undo, expand its entry in **Undo history** to read the per-track message. [Report the problem](../report-a-problem.md) with that message. Leave out file names that identify your private library.

## Related

- [Undo a cleanup](../../tagging/undo-cleanup.md)
- [Write cleanup rules](../../tagging/cleanup-rules.md)
- [Run cleanup on every new track](../../tagging/cleanup-automatically.md)
- [How does cleanup decide what to change?](../../how-it-works/cleanup-engine.md)

<!-- verified: apps/desktop/src/views/Cleanup.tsx (Scope All ready tracks / N selected tracks, Scan tracks, Select all proposals, Apply N changes, Cleanup result line, Undo history, Undo, conflict details), CleanupSettings.tsx (Clean incoming tracks automatically), Settings.tsx; crates/cuerithm-library/src/cleanup/mod.rs (cleanup_history reads cleanup batches); docs/cleanup.md (Undo and recovery, preservation); the private demo-library facts sheet (five titles ending Original Mix: tracks 3, 5, 7, 18, 22); legacy guides/cleanup-rules.md and learn/cleanup.md re-checked. UNVERIFIED: that automatic Incoming cleanup never appears in Undo history (history reads manual cleanup batches; preparation writes go through the tag-write journal) -->
