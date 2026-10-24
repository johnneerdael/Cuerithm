# How accurate is it?

<p class="lead">The published figures measure agreement between CueRithm and reference tools on one development library. They do not establish that a key, a grid or a cue is musically right. This page lists what was measured, against what, and what it leaves open.</p>

!!! warning "Agreement is not accuracy"
    Every figure below compares CueRithm with another program's output. Where the reference is a tool such as Mixed In Key or Lexicon, neither is affiliated with CueRithm, and its labels are not independently annotated musical truth. Agreement on the owner's own library does not establish accuracy on your music.

!!! abstract "What this means for you"
    - **Treat key as a strong suggestion** and check unfamiliar tracks by ear, especially minor against major and neighbouring keys.
    - **Check the grid** on tracks with live drums or tempo changes.
    - **Treat energy as a coarse, whole-track rating.**
    - **Treat automatic cues as starting points.** Audition them and [correct saved cues](../analysis/corrections/index.md) where needed. Corrections do not change the measured detector results.
    - If a result differs from another app, [My key or BPM differs from another app](../help/troubleshooting/key-or-bpm-differs.md) helps you decide who is right.

## What was measured { #the-test-library }

| Result | Compared with | Scope | Not established |
|---|---|---|---|
| Key | Stored Mixed In Key 11 key labels | 282 development tracks, one per audio group | Held-out accuracy; major-key coverage |
| BPM and first beat | A reference worker for the Lexicon timing behaviour CueRithm follows | The same 282 development groups, one stated worker configuration | Whether the grid is musically right; other tempo ranges and tempo changes |
| Energy | 29 synthetic audio cases from the reference helper | Synthetic signals only | Any real music |
| Cues | The same reference worker | The same development groups, one stated configuration | Other cue settings; unseen music; any listening study |

The development tracks are one representative of each of 282 audio groups in the owner's library, all complete, integrity-verified FLAC files. Failures stay in the denominator of any figure. A separate reserved set of 82 groups was kept aside, and none of the figures here use it. No held-out evaluation has been published.

## How well does the key agree? { #key }

- **274 of 282** tracks agree exactly (97.16%) with stored Mixed In Key 11 keys.
- The 8 disagreements: 5 neighbouring fifths in the same mode (one Camelot step), 1 relative major or minor, 2 parallel major or minor. Listening or independent annotation has not established which disputed labels are correct.
- **Camelot-compatible** (the same key or a harmonic neighbour): 280 of 282 (99.29%). This is a separate diagnostic; exact agreement remains 274.
- Only 8 of the 282 tracks have a major-key label, and 7 of those 8 matched (267 of 274 minor labels matched). Broader major-key coverage is needed, and held-out agreement is unverified.
- The shipped Rust implementation predicts the same class as the upstream network on all 282 tracks and keeps the same 274 of 282 agreement.

## How well do BPM and the first beat agree? { #tempo-and-first-beat }

- Against the reference worker, the shipped detector's results are recorded as agreeing exactly on the 282 development groups under one stated configuration. This verifies the port against that worker on those tracks. It does not show that the grid is musically right.
- The detector assumes one constant tempo between 98 and 195 BPM and a first-beat anchor. Gaps include other tempo ranges, long intros, irregular rhythm, tempo changes and complete music files outside the development set. See [How are BPM and the beatgrid found?](tempo.md).

## What was checked for energy { #energy }

- The formula matches the reference helper on 26 of 29 synthetic cases. The other 3 (empty, shorter than one frame, silent) deliberately return no rating here.
- **Real-music agreement has not been measured.**

## How well do the cues agree? { #cues }

- Cue agreement with the reference worker was recorded on the development groups under one stated configuration. That says nothing about other **Minimum breakdown length**, **Drop at start** or **Emergency loop** settings, about genre-dependent cases, or about music outside the set.
- No listening ratings have been collected, so there is no evidence that the cues are what a DJ would choose.

## Platforms and scale { #platforms-and-scale }

- Decoding stops at about 24 minutes, so long mixes do not prepare.
- Windows and Linux builds are unsigned, and their native playback, import and restart acceptance is not complete. There is no Intel Mac build.
- Large-library performance and memory use are unmeasured, and so is how long it takes to remove the copies that earlier versions kept from a very large library.
- There is no auto-update.

See [What CueRithm does not do yet](../help/not-available-yet.md) for the full list.

## What this means for you { #what-this-means-for-you }

Use CueRithm to do the bulk work, then spend your attention where it pays: audition the cues of tracks you will play, glance at the grid of anything with live drums, and trust a key most when it agrees with your ear. If a result is clearly wrong, an issue on [GitHub](https://github.com/johnneerdael/CueRithm/issues) with the file type and what you expected helps. Please do not attach copyrighted audio. [Report a problem or ask for a feature](../help/report-a-problem.md) explains how.

## Related

- [How is the key detected?](key.md)
- [How are BPM and the beatgrid found?](tempo.md)
- [How are cues placed?](cues.md)
- [Fix a wrong cue, loop or beatgrid](../analysis/corrections/index.md)

<!-- verified: docs/research/openkeyscan-comparison.md and engine-benchmarks.md (274/282, 97.16%, 5/1/2 disagreements, 280/282, 8 major with 7 matched, 267/274 minor, 282 representatives of 364 groups, 82 reserved groups, no listening ratings), openkeyscan-rust-backend.md (Rust port matches upstream class 282/282; complete integrity-verified FLAC), docs/release-readiness.md (282 development groups, one stated worker configuration, no universal accuracy percentage; energy real-music agreement unmeasured; platform and scale acceptance; no Intel macOS artifact), docs/superpowers/specs/2026-10-18-lexicon-timing.md (reference behaviour), crates/camelot-core/tests/lexicon_energy.rs and fixtures/lexicon-energy/reference.json (29 cases, 3 with no rating), AGENTS.md (agreement is not musical accuracy), legacy about/roadmap.md (Known limitations: 24 minutes, unsigned Windows and Linux builds, no Intel Mac build, no auto-update); legacy technical/accuracy.md re-checked. Removed from legacy because not in tracked docs: track-length range and genre of the development set, the exposed-holdout remark, the 1,261-cue count, the 58 of 282 earlier tempo search figure, the Lexicon version number. -->
