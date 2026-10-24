# Tag and clean up your music

<p class="lead">Fill in genres, labels and artwork from Beatport and Traxsource, fix what is wrong by hand, and tidy messy text. Renaming and sorting files has its own tab: [Renaming](../rename/index.md). Pick your goal below; most jobs take a few minutes.</p>

Everything on this tab changes the tags and file names of your tracks. It never changes CueRithm's own measurements: the key, BPM, beatgrid, whole-track energy rating and cues come from [analysis](../how-it-works/pipeline.md), and the library keeps them whatever you write into a file tag. Catalog tagging is optional, it stays off until you switch it on, and it is the only part of CueRithm that uses the internet.

## Start here { #start-here }

**New to catalog tagging?** Follow [Tag new tracks from Beatport and Traxsource](tag-new-tracks-automatically.md). You switch tagging on, add a few tracks and check what changed. It takes about ten minutes and covers the settings every other page on this tab builds on.

<div class="grid cards" markdown>

-   **Tag new tracks automatically**

    ---

    Beatport and Traxsource fill in genre, label, release date and more while a track is in Incoming.

    [:octicons-arrow-right-24: Tag new tracks from Beatport and Traxsource](tag-new-tracks-automatically.md)

-   **Fix a wrong match**

    ---

    Compare the candidates, see what differs and decide field by field what to keep.

    [:octicons-arrow-right-24: Review and fix a wrong match](review-a-match.md)

-   **Edit tags yourself**

    ---

    Change titles, artists, genres, dates, ratings, comments and artwork on one track or many.

    [:octicons-arrow-right-24: Edit tags and artwork](edit-tags-by-hand.md)

-   **Clean up messy text**

    ---

    Strip title suffixes and tidy genres and artists with rules you can preview first.

    [:octicons-arrow-right-24: Clean up messy titles, genres and artists](cleanup-messy-titles.md)

</div>

## In what order things happen { #order }

A new track passes through these steps in [Incoming](../files/follow-incoming.md).

| Step | What happens | Where you control it |
|---|---|---|
| Match | CueRithm searches the providers you chose with the track's first artist and title. This is the only step that uses the internet. | [Choose providers and fields to write](choose-providers.md) |
| Tag | Catalog fields, plus any edit you saved on the track, are prepared. | [Edit tags and artwork](edit-tags-by-hand.md) |
| Clean up | Your cleanup rules tidy titles, genres and artists. | [Run cleanup on every new track](cleanup-automatically.md) |
| Write | Tags go to a staged copy, which is checked before it replaces your file. | [How tags are written safely](../how-it-works/tag-writing.md) |
| Organize | Optional rename or move. | [Rename and sort files into folders](../rename/organize-files.md) |

## Tag from Beatport and Traxsource { #catalog-tagging }

| Your goal | Page |
|---|---|
| Tag new tracks as they arrive, from first switch-on to checking the result | [Tag new tracks from Beatport and Traxsource](tag-new-tracks-automatically.md) |
| Choose which catalogs to ask, in which order, and which fields to write | [Choose providers and fields to write](choose-providers.md) |
| Use SonoVault, Discogs, Volumo or BPM Supreme | [Connect provider accounts](provider-accounts.md) |
| Traxsource shows "Unavailable · configure a solver" | [Get past a Cloudflare block](cloudflare-solver.md) |
| Name a track that has no usable title or artist tag | [Identify a track with no match](identify-unknown-tracks.md) |
| Give every track of one release the same album, label and track total | [Match a folder as a release](match-a-release.md) |
| A track needs attention, or CueRithm chose the wrong mix | [Review and fix a wrong match](review-a-match.md) |

## Fix tags by hand { #by-hand }

| Your goal | Page |
|---|---|
| Change titles, artists, genres, dates, ratings, comments or artwork | [Edit tags and artwork](edit-tags-by-hand.md) |
| Take back a saved edit, find an unfinished draft or finish an interrupted save | [Undo a tag change](tag-history.md) |

## Clean up text { #cleanup }

| Your goal | Page |
|---|---|
| Try cleanup once on a few tracks and see every change first | [Clean up messy titles, genres and artists](cleanup-messy-titles.md) |
| Write your own cleanup rules | [Write cleanup rules](cleanup-rules.md) |
| Apply cleanup to every new track | [Run cleanup on every new track](cleanup-automatically.md) |
| Reverse a cleanup you did not want | [Undo a cleanup](undo-cleanup.md) |

## Group tracks { #organise }

| Your goal | Page |
|---|---|
| Add categories and custom tags that you can filter on | [Group tracks with categories and custom tags](curate-tracks.md) |
| Copy a worked example for a real problem | [Tag and clean-up recipes](recipes/index.md) |

!!! warning "Tag changes are written into your files"
    Every write is checked so the audio stays identical, and CueRithm keeps the original file while the write is under way. Automatic tagging has no Undo button, though, and [library backup](../files/library/back-up-library.md) does not include your music. Practise on copies, and keep your own copy of the music before a large run.

## Not what you wanted? { #elsewhere }

| If you want to… | Go to |
|---|---|
| Add a watch folder so every new purchase is tagged on arrival | [Keep a folder like Beatport in your library](../files/watch-folders.md) |
| Use different tagging rules for different folders | [Use different rules for different folders](../files/folder-presets.md) |
| Run tagging again on tracks already in the library | [Analyze a track again](../analysis/analyze-again.md) |
| Rename files and sort them into folders | [Rename and sort your music files](../rename/index.md) |
| Read a track's key, BPM and energy | [Read a track's key, BPM and energy](../analysis/read-key-bpm-energy.md) |
| Fix a wrong cue, loop or beatgrid | [Fix a wrong cue, loop or beatgrid](../analysis/corrections/index.md) |
| Build playlists from genre, key or energy | [Build playlists and sets](../playlists/index.md) |
| Send tagged tracks to rekordbox or a USB stick | [Send your library to DJ software](../sync/index.md) |

If something goes wrong, start with [Catalog tagging finds nothing](../help/troubleshooting/no-catalog-match.md), [A track is stuck in Incoming](../help/troubleshooting/incoming-stuck.md) or [Cleanup changed too much](../help/troubleshooting/cleanup-changed-too-much.md). Every setting on the Automatic tagging page is listed in [Automatic tagging settings](../reference/settings/automatic-tagging.md), and [How does catalog matching work?](../how-it-works/matching.md) explains the scoring.

<!-- verified: apps/desktop/src/views/TaggingSettings.tsx; apps/desktop/src/App.tsx (toolbar buttons); apps/desktop/src/components/TrackTable.tsx (stage labels); crates/cuerithm-library/src/prepare.rs (order of matching, tagging, cleanup, writing, organizing); AGENTS.md (catalog tagging optional, only network use); legacy guides/library-backup.md (music excluded) re-checked -->
