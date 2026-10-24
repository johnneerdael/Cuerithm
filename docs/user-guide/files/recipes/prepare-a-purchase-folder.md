# Prepare a new Beatport purchase folder

<p class="lead">Turn a folder of freshly bought tracks into prepared, tagged tracks in Tracks, with key, BPM, energy and cues, by setting the rules once and pointing CueRithm at the folder. Setup takes about five minutes.</p>

<div class="recipe-meta" markdown>
**Uses:** Settings → Automatic tagging, Cleanup (optional), File organization, Incoming → Add watch folder.
**Works on:** Any folder of supported audio files. The example is `~/Music/Beatport`, 26 FLAC purchases.
**Time:** About five minutes to set up; preparation then runs in the background.
**Undo:** Your files stay where they are. Each original is held while its tags are written and deleted when the write is done, and automatic tagging has no Undo button; correct a tag with [Undo a tag change](../../tagging/tag-history.md) or by hand.
</div>

## The problem

A folder of new purchases holds audio files and nothing else CueRithm knows about. You want every track measured, tagged from Beatport and ready for crates, without touching each file.

| | Before | After |
|---|---|---|
| Library | The files sit in `~/Music/Beatport`; CueRithm has no record of them | 26 tracks in Tracks |
| Key, BPM, energy | None | Brackish (Original Mix): <span class="cr-key">12B</span> <span class="cr-bpm">140</span> <span class="cr-energy">6/10</span><br>Man Like Me (Extended Mix): <span class="cr-key">4A</span> <span class="cr-bpm">128</span> <span class="cr-energy">7/10</span><br>Play With Fire (Original Mix): <span class="cr-key">12A</span> <span class="cr-bpm">140</span> <span class="cr-energy">8/10</span> |
| Cues | None | Man Like Me: Start 0:00, Drop 1:00, Breakdown 1:53, Second Drop 3:15, Last beat 5:15 |
| Tags | As downloaded | Catalog fields written from Beatport, with Traxsource as a second choice |

## The solution

Set the rules first, then add the folder. CueRithm captures the rules when it sees each file, so files that arrive before you save are prepared with the old ones.

1. Choose **Settings**, then **Automatic tagging**. Switch **Automatic tagging** on. In the provider list, tick **Use Beatport** and **Use Traxsource**, and use the arrows to put Beatport at position 1. Beatport and Traxsource are the default selection.
2. Set **No catalog match** to *Keep existing tags and finish*, so a track with no match still reaches Tracks.
3. Open **Cleanup**. Leave **Clean incoming tracks automatically** off for now, or switch it on to tidy titles and genres as files arrive; see [Run cleanup on every new track](../../tagging/cleanup-automatically.md).
4. Open **File organization** and keep **Leave files in place**.
5. Select **Save settings**. Wait for *Settings saved*.
6. Open the **Incoming** page of Settings, select **Add watch folder** and pick `~/Music/Beatport`. Select **Save settings** again.
7. Choose **Incoming** under LIBRARY in the sidebar. The header reads *Preparing 26 tracks*, and each row steps through Queued, Decoding, Analyzing, Matching catalog, Preparing tags and Writing tags.
8. When the header reads *All up to date*, open **Tracks**. Each row has key, BPM, energy and a mini waveform. A check in the status column means *Tagged from catalog match*.

Exact settings used in this recipe:

```text
Settings → Automatic tagging
  Automatic tagging:          on
  Use Beatport:               on (position 1)
  Use Traxsource:             on (position 2)
  No catalog match:           Keep existing tags and finish
Settings → Cleanup
  Clean incoming tracks automatically:   off
Settings → File organization
  When preparation finishes:  Leave files in place
Settings → Incoming
  Watch folder:               ~/Music/Beatport
```

A track that stops with a warning triangle needs you; see [Fix a track that needs attention](../needs-attention.md). A Beatport block shows as a provider error, not as no match; see [Get past a Cloudflare block](../../tagging/cloudflare-solver.md).

## How it works

CueRithm measures key, BPM, beatgrid, energy and cues from the audio itself. Beatport and Traxsource supply catalog fields such as title, artist, label and artwork. Key and BPM written into the file's tags come from CueRithm's own measurement unless you choose *Catalog metadata* under **Key and BPM written to files**. With **Combine providers** off, the first provider with a match supplies the fields. After the tags are written, cues are recalculated with the final genre, then the track moves to Tracks.

## Variations

- **No tagging at all:** [Analyze tracks without tagging or cleanup](analyze-only.md).
- **Hard dance in a subfolder with its own cue rules:** add the subfolder as a watch folder and assign it a preset; see [Use different rules for different folders](../folder-presets.md).
- **Rename files as they arrive:** set **When preparation finishes** to *Rename in place* or *Move to folder* and build a pattern; see [Build a naming pattern](../../rename/naming-patterns.md).
- **Future purchases:** drop them in the same folder. Nothing else to set up.

## Related

- [Keep a folder like Beatport in your library automatically](../watch-folders.md)
- [Tag new tracks from Beatport and Traxsource](../../tagging/tag-new-tracks-automatically.md)
- [Follow a track through Incoming](../follow-incoming.md)
- [From a folder of new music to a playlist for the booth](../../get-started/first-hour.md)

<!-- verified: apps/desktop/src/views/TaggingSettings.tsx (Automatic tagging switch, Use Beatport/Use Traxsource, Move … earlier, No catalog match options, Combine providers, Key and BPM written to files, Recovery backups); apps/desktop/src/views/CleanupSettings.tsx (Clean incoming tracks automatically); apps/desktop/src/views/OrganizationSettings.tsx (When preparation finishes, Leave files in place); apps/desktop/src/views/Settings.tsx; crates/cuerithm-library/src/prepare.rs (stage order; cue refresh with the final genre); docs/automatic-tagging.md; docs/user-guide/_meta/the private demo-library facts sheet for Brackish, Man Like Me, Play With Fire. Code-read, not run: the default provider selection can hide a provider that needs a solver. -->
