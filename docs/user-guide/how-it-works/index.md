# How CueRithm works

<p class="lead">Every number CueRithm shows is computed on your computer, from the audio, by code you can read. This tab explains how, in about five minutes per question, so you know what to trust and what to check by ear.</p>

!!! abstract "What this means for you"
    - **Analysis is offline.** Key, BPM, beatgrid, energy, cues and waveform are calculated from the decoded audio. Tags, filenames and other apps' labels are never used as input to those results.
    - **Analysis never changes your music files.** CueRithm writes to a file only when you switch tagging, cleanup or file organisation on, or save an edit. While it writes, it holds the original until the write has settled.
    - **The numbers are measurements, not verdicts.** Check an unfamiliar track by ear, especially its key and cues. [How accurate is it?](accuracy.md) says exactly what has and has not been measured.

## Which page answers my question? { #your-question }

| Your question | Page |
|---|---|
| What happens between dropping a file in and seeing it in Tracks? | [How does CueRithm analyse a track?](pipeline.md) |
| Why does a file fail to decode, and why do long mixes stay in Incoming? | [How are audio files decoded?](decoding.md) |
| Why is a key different from another app, and what is Camelot? | [How is the key detected?](key.md) |
| Why is a BPM doubled or halved, and why does the grid drift? | [How are BPM and the beatgrid found?](tempo.md) |
| What does 7/10 mean, and why is there only one number? | [How is energy rated?](energy.md) |
| Why is there a cue here, or no Breakdown at all? | [How are cues placed?](cues.md) |
| What do the waveform colours mean? | [How is the waveform drawn?](waveform.md) |
| Where is my library stored, and what breaks if I move a file? | [How is the library stored?](library.md) |
| Can writing tags damage a file, and can I undo it? | [How are tags written safely?](tag-writing.md) |
| Why did CueRithm choose this release, or stop and ask me? | [How does catalog matching work?](matching.md) |
| What exactly will Cleanup change? | [How does cleanup decide what to change?](cleanup-engine.md) |
| When does a smart playlist update? | [How do smart playlists stay current?](smart-engine.md) |
| Can I trust these numbers? | [How accurate is it?](accuracy.md) |

## How a track goes from file to ready { #flow }

```text
file → wait → decode → analyse → [match] → [tag and clean up] → check and save → Tracks
```

Steps in brackets run only when you turned automatic tagging or cleanup on.

| Step | What happens | Label in Incoming | Read more |
|---|---|---|---|
| 1. Wait | CueRithm checks that the file has stopped changing | Waiting for file | [Pipeline](pipeline.md#1-waiting-for-the-file) |
| 2. Decode | The bundled decoder turns any supported format into one 44.1 kHz stereo timeline | Decoding | [Decoding](decoding.md) |
| 3. Analyse | Key, BPM and first beat, energy and cues are calculated | Analyzing | [Key](key.md), [tempo](tempo.md), [energy](energy.md), [cues](cues.md) |
| 4. Match | The track is looked up in the catalogs you chose | Matching catalog | [Matching](matching.md) |
| 5. Tag and clean up | Tags and cleanup edits are written to a staged copy, which is verified | Preparing tags, Writing tags | [Tag writing](tag-writing.md), [cleanup](cleanup-engine.md) |
| 6. Check and save | The results are checked, then saved to the library in one step | Writing tags, or Applying cues when tagging and cleanup are off | [Pipeline](pipeline.md#5-publication-and-promotion) |
| 7. Move to Tracks | The track leaves Incoming | (the row moves) | [Follow a track through Incoming](../files/follow-incoming.md) |

## The short version { #short-version }

- **One decoder.** A bundled, LGPL-licensed FFmpeg 9.0.2 turns every supported format into the same 44.1 kHz stereo float samples. You install no codec pack.
- **Key** comes from a small neural network (OpenKeyScan3, MIT licence) that reads a constant-Q spectrum of the whole track.
- **BPM and the first beat** come from a signal-processing detector with many explicit correction rules. The result is one constant BPM and one anchor.
- **Energy** is one rating from 1 to 10 for the whole track, from a fixed acoustic formula. There is no energy per section or per cue.
- **Cues** come from a bar-by-bar analysis of the track's low-frequency energy.
- **The library** is one SQLite database. Your audio is not stored in it. Tag writes are staged, verified, journaled and swapped in atomically.
- **The network is used for one thing**: catalog lookups when automatic tagging is on. Everything else works offline.

## Not what you wanted? { #not-what-you-wanted }

- To *do* something rather than understand it, start at [Pick the right guide for your goal](../get-started/choose-your-path.md).
- To read what a number means on screen, see [Read a track's key, BPM and energy](../analysis/read-key-bpm-energy.md).
- For terms such as constant-Q, anchor or journal, see the [glossary](../reference/glossary.md).
- If a result looks wrong, [My key or BPM differs from another app](../help/troubleshooting/key-or-bpm-differs.md) is the quickest route.

## Related

- [How does CueRithm analyse a track?](pipeline.md)
- [How accurate is it?](accuracy.md)
- [How CueRithm was made](../help/about/provenance.md)
- [Credits and open source](../help/about/credits.md)

<!-- verified: crates/cuerithm-library/src/prepare.rs (stages, 5 s rescan), apps/desktop/src/components/TrackTable.tsx (stage labels), apps/desktop/src-tauri/src/builtin.rs (built-in key model, lexicon timing and energy), tools/media-runtime/manifest.json (FFmpeg 9.0.2); THIRD_PARTY_NOTICES.md (licences); legacy technical/index.md re-checked -->
