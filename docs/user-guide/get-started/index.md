# Start using CueRithm

<p class="lead">CueRithm turns a folder of new music into a library you can play from: every track gets a key, a BPM, a beatgrid, one energy rating and labelled cues, and you decide what happens next. This page takes you from the download to your first prepared tracks, then points to the guide for whatever you want to do after that.</p>

Analysis runs on your computer without an account or an upload, and your music files stay where they are. Start with a handful of tracks, or with a folder you can afford to experiment on, until you have seen what each step does.

## What you need { #what-you-need }

| | |
|---|---|
| **Systems** | A Mac with Apple silicon (signed and notarized), a 64-bit Windows PC or a 64-bit Linux PC (both unsigned). There is no Intel Mac build. Details: [Check that your computer can run CueRithm](install.md#requirements). |
| **Audio formats** | MP3, AAC (ADTS and M4A), ALAC (M4A), FLAC, WAV, AIFF, Ogg Vorbis, Opus and WMA. Protected media is not supported, and decoding stops at about 24 minutes. See [Check that your files are supported](../files/supported-formats.md). |
| **Internet** | Not needed to analyse, place cues, build playlists or clean up. Only the optional catalog tagging and recording identification use the network. |
| **Extra software** | None. The decoder ships inside the download. |
| **Disk** | CueRithm keeps your music where it is. It stores the library, caches and backups in its own data folder: [where CueRithm keeps your data](../files/library/where-data-lives.md). |

## What do you want to do? { #what-you-can-do }

<div class="grid cards" markdown>

-   :material-folder-music-outline:{ .lg .middle } **Manage your music files**

    ---

    Drop files, pick them or let CueRithm watch a folder such as `~/Music/Beatport`. Each track is decoded, analysed, optionally tagged and cleaned, then added to Tracks.

    [:octicons-arrow-right-24: Keep a folder in your library](../files/watch-folders.md)

    [:octicons-arrow-right-24: Follow a track through Incoming](../files/follow-incoming.md)

-   :material-playlist-music:{ .lg .middle } **Build playlists and sets**

    ---

    Ordinary playlists, folders and smart playlists that keep themselves current, plus a generator, a harmonic "what mixes with this" search and a flow check.

    [:octicons-arrow-right-24: Build a smart playlist](../playlists/build-a-smart-playlist.md)

    [:octicons-arrow-right-24: Find tracks that mix with the one playing](../playlists/find-compatible-tracks.md)

-   :material-tag-edit-outline:{ .lg .middle } **Tag and clean up**

    ---

    Tag new tracks from Beatport and Traxsource, review each match, and clean up messy titles, genres and artists.

    [:octicons-arrow-right-24: Tag new tracks automatically](../tagging/tag-new-tracks-automatically.md)

    [:octicons-arrow-right-24: Clean up messy titles, genres and artists](../tagging/cleanup-messy-titles.md)

-   :material-rename-box:{ .lg .middle } **Rename and sort your files**

    ---

    Name every file from one pattern and sort it into folders, after a preview of every change.

    [:octicons-arrow-right-24: Rename and sort files into folders](../rename/organize-files.md)

    [:octicons-arrow-right-24: Build a naming pattern](../rename/naming-patterns.md)

-   :material-waveform:{ .lg .middle } **Read, preview and fix tracks**

    ---

    See key, BPM and energy at a glance, preview a track, jump to its cues and correct a cue, loop or beatgrid that is off.

    [:octicons-arrow-right-24: Read a track's key, BPM and energy](../analysis/read-key-bpm-energy.md)

    [:octicons-arrow-right-24: Fix a wrong beatgrid](../analysis/corrections/fix-the-beatgrid.md)

-   :material-sync:{ .lg .middle } **Send it to your DJ software**

    ---

    Preview, apply and restore a sync to rekordbox 6 or 7, VirtualDJ, Engine DJ, djay Pro, Serato DJ Pro 4, Pioneer USB or Mixxx.

    [:octicons-arrow-right-24: Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md)

    [:octicons-arrow-right-24: Sync to Mixxx](../sync/mixxx.md)

-   :material-shield-check-outline:{ .lg .middle } **Look after your library**

    ---

    Find duplicates and missing files, back up the library and restore it.

    [:octicons-arrow-right-24: Find missing, moved or broken files](../files/library/find-missing-files.md)

    [:octicons-arrow-right-24: Back up your library](../files/library/back-up-library.md)

-   :material-lifebuoy:{ .lg .middle } **Something went wrong**

    ---

    Match what you see to a fix: a dash instead of a waveform, a track stuck in Incoming, a sync that did not arrive.

    [:octicons-arrow-right-24: Fix a problem](../help/troubleshooting/index.md)

    [:octicons-arrow-right-24: Frequently asked questions](../help/faq.md)

-   :material-compass-outline:{ .lg .middle } **Not sure where to start?**

    ---

    A goal-by-goal map of the whole guide, and a page for people coming from Mixed In Key, Lexicon or rekordbox.

    [:octicons-arrow-right-24: Pick the right guide for your goal](choose-your-path.md)

    [:octicons-arrow-right-24: Coming from Mixed In Key, Lexicon or rekordbox](coming-from-other-tools.md)

</div>

## Your first hour { #first-hour }

1. **Install.** Pick the download for your computer in [Choose the right download](install.md#choose-download), then follow [Install on a Mac](platforms/macos.md), [Install on Windows](platforms/windows.md) or [Install on Linux](platforms/linux.md). Compare the SHA-256 digest with the one GitHub shows ([Verify your download](install.md#verify-download)) and run the built-in check at the end of the install page.
2. **Learn the window.** A sidebar, a track table and a player that stays on screen. [Find your way around the window](main-window.md) names every part and links each one to its task page, from [the sidebar](main-window.md#the-sidebar) to [the player](main-window.md#the-player).
3. **Add a few tracks.** [Prepare your first tracks](first-tracks.md) walks through **Add tracks**, watching Incoming and finding the result in Tracks ([step 2: add tracks](first-tracks.md#2-add-tracks)). To make a folder permanent, set it up as a [watch folder](../files/watch-folders.md).
4. **Read the numbers.** Open a track and read its key, BPM and energy: [Read a track's key, BPM and energy](../analysis/read-key-bpm-energy.md). If a key looks unfamiliar, [Understand Camelot keys](../analysis/camelot-keys.md) explains the notation.
5. **Jump to the drop.** The player shows the beatgrid and the cues. [Jump to the drop with cues](../analysis/jump-to-cues.md) shows the buttons, and [Check the beatgrid at beat level](../analysis/check-the-beatgrid.md) shows how to zoom to the beats.
6. **Make a playlist.** [Create a playlist and add tracks](../playlists/create-a-playlist.md), then try [Build a smart playlist](../playlists/build-a-smart-playlist.md) so it fills itself.
7. **Send it on.** When the playlist looks right, [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md) or choose another destination on [Send your library to DJ software](../sync/index.md).

Prefer one guided run from start to finish? [From a folder of new music to a playlist for the booth](first-hour.md) does steps 3 to 7 on a real purchase folder.

<span id="files-and-saved-analysis"></span>

## Three things that surprise new users { #surprises }

- **Incoming and Tracks are states, not folders.** Preparing a track never moves or copies your audio file. The track stays where it is on disk; only its state in the library changes. See [Follow a track through Incoming](../files/follow-incoming.md#what-ready-requires).
- **Analysis happens once.** A prepared track keeps its results until you choose **Analyze again**; changing a setting does not silently rewrite finished tracks. See [Analyze a track again](../analysis/analyze-again.md).
- **Tagging and cleanup are off until you switch them on, and both write to the file's tags.** Catalog tagging is the only part of CueRithm that uses the network. Read [Tag new tracks from Beatport and Traxsource](../tagging/tag-new-tracks-automatically.md) before you enable it, and [How are tags written safely?](../how-it-works/tag-writing.md) to see what protects a file while it is written.

## If something goes wrong { #recover }

Every change that touches your files or library has a way back, and a few have limits you should know first.

| You did this | You can undo it with | Read |
|---|---|---|
| Applied a cleanup | **Undo** in the cleanup history | [Undo a cleanup](../tagging/undo-cleanup.md) |
| Saved tag edits or applied a match | Edit history | [Undo a tag change](../tagging/tag-history.md) |
| Reorganised files into folders | **Undo** or recovery in the organiser history | [Recover or undo a reorganisation](../rename/undo-organize.md) |
| Merged duplicates, removed missing tracks or removed unused files | **History & recovery** in Maintenance | [Undo a merge or put back removed files](../files/library/undo-maintenance.md) |
| Synced to DJ software | **Restore backup** in the sync history | [Preview, apply and restore a sync](../sync/preview-apply-restore.md) |
| Changed a cue, loop or beatgrid by hand | Undo in the corrections | [Undo or resolve a correction](../analysis/corrections/undo-corrections.md) |
| Need the whole library back | A library backup | [Restore your library from a backup](../files/library/restore-library.md) |

| You see this | Go to |
|---|---|
| A dash instead of a waveform, or a track that will not play | [A track shows a dash or will not play](../help/troubleshooting/no-waveform-or-playback.md) |
| Files you moved or renamed now show as missing | [I moved or renamed my files](../help/troubleshooting/files-moved.md) |
| A track that stays in Incoming | [A track is stuck in Incoming](../help/troubleshooting/incoming-stuck.md) |
| Catalog tagging that finds nothing | [Catalog tagging finds nothing](../help/troubleshooting/no-catalog-match.md) |
| A key or BPM that differs from another app | [My key or BPM differs from another app](../help/troubleshooting/key-or-bpm-differs.md) |
| A sync that does not show up in your DJ software | [My sync does not show up in rekordbox](../help/troubleshooting/rekordbox-sync-problems.md) |
| Your computer blocking CueRithm when you open it | [My computer blocks CueRithm at first launch](../help/troubleshooting/first-launch-blocked.md) |

## Next steps

- [Pick the right guide for your goal](choose-your-path.md) lists every task in the guide by what you are trying to do.
- [What's new](whats-new.md) summarises the recent releases for DJs.
- [Check what works with your DJ software](../help/compatibility.md) and [What CueRithm does not do yet](../help/not-available-yet.md) say what has been checked and what has not.
- [Keyboard shortcuts](../reference/keyboard-shortcuts.md) and [Find the setting you need](../customize/find-a-setting.md) cover the rest of the window.

<!-- verified: docs/user-guide/get-started/install.md and main-window.md (anchors and requirements re-read); legacy get-started/index.md facts (state-not-folder, analysis once); decoding limit and formats per prepare/supported-formats.md; destinations per _meta/legacy/about/roadmap.md -->
