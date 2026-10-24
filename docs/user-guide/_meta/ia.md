# CueRithm user guide: information architecture

Not published (`_meta/` is excluded in `mkdocs.yml`). This file explains how the guide is
organised and why, where each feature is deep-linked from, what the demo library is, and
how old URLs are kept working. `nav.yml` is the authoritative list of pages and their
order; `style.md` says how to write a page; `figures/README.md` says how screenshots are
requested; the private demo-library facts sheet is the real data every example uses (kept in the maintainer's private `.analysis/` area, not in Git, because it lists per-track measurements).

## Why the guide was rebuilt

The previous guide had five tabs named after document types (Learn, Guides, Under the
hood, About, Get started). A reader with a goal ("my files moved", "tag a folder from
Beatport", "send my playlists to rekordbox") could not tell which tab held the answer. Page
titles were feature nouns ("Maintenance", "Player, waveform and cues"), 21 guides sat in
one flat list, 64 screenshots covered 56 pages, and Get started linked to three pages.

The new guide follows the TagGroove model:

- **Tabs are features in the order a DJ meets them**, and each holds that feature's
  tutorial, task pages, recipes and reference. A tab landing page is a map: *Your goal →
  Page* tables that list every page of the tab.
- **Page titles are goals in the reader's words**, not feature names: "Find and merge
  duplicate tracks", not "Maintenance".
- **Get started is a complete start path with deep links**: requirements, a card and two
  deep links per feature, a first-hour route, what to do when something goes wrong, and an
  undo table.
- **Every UI page has screenshots**: existing real-data captures now, a fresh light and
  dark set from the installed app later.
- **Old links keep working.** `docs/hooks/redirects.json` maps every page and split
  anchor the guide ever published to its new home.

## Tabs

| Tab | Folder | For the reader who wants to… |
|---|---|---|
| Home | `index.md` | see what CueRithm is and pick a goal |
| Get started | `get-started/` | install, learn the window, prepare first tracks, be routed everywhere |
| Manage files | `files/`, `files/library/`, `files/recipes/` | bring music in, watch folders, follow Incoming, presets, archive, and look after the library (duplicates, missing files, backups) |
| Playlists | `playlists/` | build playlists, smart playlists and sets; find what mixes with what |
| Auto tagging | `tagging/` | tag from Beatport and Traxsource, review matches, edit tags, clean up titles, genres and artists, curate |
| Renaming | `rename/` | name files from a pattern and sort them into folders, with Undo |
| Track analysis | `analysis/` | find and read tracks, preview, jump to cues, check and correct the beatgrid, control how tracks are analysed |
| Sync | `sync/` | send playlists, tracks, cues and grids to seven DJ destinations |
| Customize | `customize/` | change theme, waveform style, layout |
| How it works | `how-it-works/` | know how a number or decision is computed |
| Reference | `reference/` | look up shortcuts, settings, rule fields, tokens, status icons, terms |
| Help | `help/` | fix a problem, read the FAQ, see what works with which DJ software and what is not available yet |

## Where each feature is deep-linked from

Get started and Home link to every row, so the start path stays short and the depth lives
in the tab. A feature that is in the app but not in this table is a documentation gap.

| Feature (app label) | Landing | Task deep link used on Get started and Home |
|---|---|---|
| Add tracks, Incoming, watch folders | `files/index.md` | `files/watch-folders.md`, `files/follow-incoming.md` |
| Preparation presets, Archive | `files/index.md` | `files/presets.md`, `files/archive.md` |
| Maintenance (duplicates, missing, unused), Library backup | `files/library/index.md` | `files/library/find-missing-files.md`, `files/library/back-up-library.md` |
| Playlists, folders, smart playlists | `playlists/index.md` | `playlists/create-a-playlist.md`, `playlists/build-a-smart-playlist.md` |
| Playlist tools, generator, compatible tracks, flow, tracklist matcher | `playlists/index.md` | `playlists/generate-playlists.md`, `playlists/find-compatible-tracks.md` |
| Automatic tagging (Beatport, Traxsource), Review match, Edit metadata | `tagging/index.md` | `tagging/tag-new-tracks-automatically.md`, `tagging/review-a-match.md` |
| Cleanup, Curate | `tagging/index.md` | `tagging/cleanup-messy-titles.md`, `tagging/curate-tracks.md` |
| Organize files | `rename/index.md` | `rename/organize-files.md` |
| Key, BPM, energy, columns, search | `analysis/index.md` | `analysis/read-key-bpm-energy.md` |
| Player, waveform, cues, beatgrid | `analysis/index.md` | `analysis/jump-to-cues.md`, `analysis/check-the-beatgrid.md` |
| Manual cue, loop and grid corrections | `analysis/corrections/index.md` | `analysis/corrections/fix-the-beatgrid.md` |
| Sync (rekordbox, Pioneer USB, VirtualDJ, Engine DJ, djay Pro, Serato, Mixxx) | `sync/index.md` | `sync/rekordbox-direct.md`, `sync/mixxx.md` |
| Settings, theme, waveform style | `customize/index.md` | `customize/theme.md` |

## Demo library (real data)

Every example comes from one real library: the owner's installed CueRithm with its
configured tag sources, watching `~/Music/Beatport` (26 FLAC files from a Beatport purchase
folder). The per-track values are in a private facts sheet kept under `.analysis/` (not in Git). Screenshots of the demo playlists are
captured after the playlists are created in the app (screenshots are scheduled with the
fixes). Page text refers to these playlists by exact name:

| Name | Kind | Rule or contents |
|---|---|---|
| Club nights | folder | holds the two ordinary playlists below |
| Warm-up set | ordinary playlist | eight hand-ordered tracks (order in the private demo-library facts sheet) |
| Peak hour | ordinary playlist | eight hand-ordered tracks, energy 6 and up |
| Melodic house 128+ | smart playlist | `genre` contains Melodic House and `BPM` is at least 128 (10 tracks) |
| Energy 7 and up | smart playlist | `Energy` is at least 7 (9 tracks) |
| Mixes with 7A | smart playlist | `Key` is compatible with key 7A, Harmonic mode (6A, 7A, 8A and 7B; 6 tracks) |
| Added this week | smart playlist | `Date added` is in the last 7 days (an ordinary smart playlist, not an automatic one) |

## Describing released behaviour; pending product changes

The guide describes the **latest release** (0.1.16) plus current `main` where verified. A behaviour
that depends on the version carries one short note, never a warning that the feature is missing:

- **Shipped in 0.1.15** (watch-folder move tracking; Missing & broken **Remove from library…**
  with Undo, **Find moved files…**, archived tracks in the scan; the *File not found* preview text;
  readable status labels) is current behaviour. Pages that need a note say "CueRithm 0.1.15 or
  later".
- **Shipped in 0.1.16** (Relocate, Find Lost Tracks and Undo relocation; portable `.cuerithm`
  backup and restore with folder mapping; Lexicon v26 import) is current behaviour. Pages that need
  a note say "CueRithm 0.1.16 or later": `files/library/find-missing-files.md`,
  `files/library/index.md`, `files/library/back-up-library.md`, `files/library/restore-library.md`,
  `files/move-files.md`, `help/faq.md`, `help/not-available-yet.md` and
  `help/troubleshooting/files-moved.md`.
- **Added by the next release** (0.1.17, the Duplicates and removals work) is described as current
  behaviour with the note "CueRithm 0.1.17 or later", written exactly like that and once per page so
  that `grep -rn "0.1.17 or later" docs/user-guide` finds every one: Duplicates leaves out tracks
  whose file is not available; the merge asks for **Delete from disk** (Trash; permanent only for
  files the preview names, with the apply button naming them) or **Archive only**, with nothing
  preselected; duplicate merges that delete files and Missing & broken removals share one Undo list
  of the last 10 removals; a removed track's cover stays until its removal leaves that list; removal
  also clears the caches and saved scans built from the track; an interrupted deletion stays under
  Recovery until acknowledged. The affected pages are `files/library/find-duplicates.md`,
  `files/library/find-missing-files.md`, `files/library/undo-maintenance.md`,
  `files/library/index.md` and `files/archive.md`.

- **Added by the release after that** (0.1.19, the lean library work) is described as current
  behaviour with the note "CueRithm 0.1.19 or later", written exactly like that and once per page:
  removed unused files go to the Trash and CueRithm keeps no copy (no Restore files, no backup
  disposal); Undo of a cleanup, a tag edit and a donor tag copy writes back recorded tag values
  instead of copying an original file back, and a copy of a file is held only while its write is
  under way; Undo covers the 10 newest operations of each kind, with every merge sharing the
  limit of 10 removals; restoring a sync keeps its backup for the newest 10 syncs of each DJ
  program; the library keeps its log and free space small, and DJ-library records as hashes. The
  affected pages are `files/library/clean-up-unused-files.md`, `undo-maintenance.md`,
  `where-data-lives.md`, `find-duplicates.md`, `find-missing-files.md`, `index.md`,
  `how-it-works/library.md`, `how-it-works/tag-writing.md`, `tagging/tag-history.md`,
  `tagging/undo-cleanup.md`, `sync/preview-apply-restore.md` and `help/not-available-yet.md`.

`get-started/whats-new.md` has a section for every release; its release notes are the source for
what shipped. When 0.1.17 ships: confirm the number against `gh release view`, add its section to
`whats-new.md`, and keep "CueRithm 0.1.17 or later" as the minimum version (move the note's
wording to the new section; correct the number if the release order changed). The *In development*
list in `help/not-available-yet.md` holds only what is planned and not in any release: DJ-library
import. Everything else there is a limitation of the current app.

## Redirects: links from outside the guide

Everything that ever pointed into the guide must still land on the right page:

- `docs/hooks/redirects.json` has one entry for every page in `legacy-pages.txt` that no
  longer exists at the same path. Split pages list `anchors` so each old `#section` goes to
  the page that now holds it. The build fails if a target page or anchor does not exist.
  `tools/docs/map_anchors.py` matches old anchors to new heading ids.
- `tools/docs/check_guide.py` fails if a legacy page has neither a page nor a redirect.
- Links in the repository are updated by `tools/docs/update_external_links.py` and
  `tools/docs/move_pages.py`: the README, `docs/*.md`, design notes and plans.
- Published GitHub release notes keep their old links; the redirects keep them working.

## Screenshots

See `figures/README.md`. `figures/existing-images.json` places the existing real-data
screenshots in the new pages until a fresh light and dark pair of the same scene exists
(then the pair replaces it). Every writer's figure requests are the shot list for the
capture pass: `python3 tools/docs/embed_figures.py --list-pending`.

## Status

Progress is computed, never edited by hand: `python3 tools/docs/check_guide.py` lists
navigation entries without a file, pages not in the navigation, legacy URLs without a
redirect, and figure requests that point at a missing page or heading.
