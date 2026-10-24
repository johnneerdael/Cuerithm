# From a folder of new music to a playlist for the booth

<p class="lead">In about an hour you take a folder of 26 new purchases through CueRithm: prepare them, read their key, BPM and energy, jump to the drops, look at tagging and cleanup, build a Warm-up set playlist and send it to rekordbox. Each step ends with the tutorial that goes deeper.</p>

**You need:**

- [ ] CueRithm installed ([Download and install CueRithm](install.md))
- [ ] A folder of new music. The examples use `~/Music/Beatport`: 26 FLAC files, about two hours of music.
- [ ] For step 7 only: rekordbox 6 or 7 on this computer. Without it, stop after step 6 or choose another DJ program in step 7.

The track names, keys and times below are from that folder. Yours will differ; the steps do not.

## 1. Point CueRithm at the folder { #point-at-folder }

1. Choose **Settings** in the sidebar, then **Incoming**.
2. Select **Add watch folder** and pick your music folder.
3. Select **Save settings**.

Within a few seconds the files appear in **Incoming**. A watch folder keeps working: tomorrow's purchases are prepared the moment they land in it.

**Why now:** tagging and cleanup are off until you turn them on, and they apply as tracks pass through Incoming. If you want Beatport and Traxsource tags written to new tracks, switch on **Automatic tagging** in Settings before you save this folder. If not, leave it; step 5 shows the other routes.

**Go deeper:** [Keep a folder like Beatport in your library automatically](../files/watch-folders.md), or for a one-off import [Add tracks: drop, pick or watch a folder](../files/add-tracks.md).

## 2. Let Incoming finish { #wait }

Open **Incoming**. The header reads *Preparing 26 tracks* and each row names its stage. When the header says *All up to date*, **Incoming** is empty and **Tracks** shows 26. Keep working while it runs; CueRithm uses about three quarters of your processor cores.

**You should see** every track in **Tracks** with a key chip, BPM, energy and a mini waveform. A row with a warning triangle needs you instead.

**Go deeper:** [Prepare your first tracks](first-tracks.md) is this step as a tutorial, [Follow a track through Incoming](../files/follow-incoming.md) explains the stages, and [Fix a track that needs attention](../files/needs-attention.md) covers a warning triangle.

## 3. Read the numbers { #read-numbers }

In **Tracks**, click the **Energy** header twice: the first click sorts lowest first, the second puts the highest first. The strongest track is **Play With Fire (Original Mix)** by SampliFire and Jkyl & Hyde, <span class="cr-energy">8/10</span> at <span class="cr-bpm">140</span> BPM in <span class="cr-key">12A</span>. Eight tracks follow at <span class="cr-energy">7/10</span>, among them:

| Track | Key | BPM | Energy |
|---|---|---|---|
| Come To Life (Original Mix) | 7A | 130 | 7/10 |
| Other Dimension (Extended Mix) | 6A | 128 | 7/10 |
| Here In My Arms (Enjoy The Silence) (Extended Mix) | 5A | 132 | 7/10 |
| Man Like Me (Extended Mix) | 4A | 128 | 7/10 |

The key is a Camelot code: the number is the position on the wheel and the letter is minor (A) or major (B). Tracks at <span class="cr-key">6A</span>, <span class="cr-key">7A</span> and <span class="cr-key">8A</span> sit next to each other and mix cleanly. Energy is one rating for the whole track, never one per section.

**Go deeper:** [Read a track's key, BPM and energy](../analysis/read-key-bpm-energy.md) is the tutorial, and [Understand Camelot keys](../analysis/camelot-keys.md) explains the wheel. If another app disagrees, see [My key or BPM differs from another app](../help/troubleshooting/key-or-bpm-differs.md).

## 4. Jump to the drop { #cues }

Double-click **Come To Life (Original Mix)** by Cassian and AR/CO. It loads into the player and plays. Below the waveform, click **Drop** to land at about 0:59, **Breakdown** at 1:51, **Drop #2** at 3:01 and **End / Fade-out** at 4:26. Press ++arrow-right++ to jump one bar, which at 130 BPM is about 1.8 seconds.

**You should see** the playhead move and the beat lines under the cue markers line up with the kick. If a cue or the grid is wrong on a track, you can correct it.

**Go deeper:** [Jump to the drop with cues](../analysis/jump-to-cues.md), [Check the beatgrid at beat level](../analysis/check-the-beatgrid.md) and [Fix a wrong cue, loop or beatgrid](../analysis/corrections/index.md).

## 5. Look at tagging and cleanup (optional) { #tidy }

CueRithm can tag tracks from Beatport and Traxsource as they arrive, and it can tidy titles, genres and artists afterwards. Both are optional and both write to the file's tags, always with the original kept so that Undo works. For example, track 13 is stored as `Nivk, Rabteu - PLV (Creeds remix - Extended)`: its artists are in the title. Here is a safe way to see what cleanup would do:

1. Choose **Cleanup** in the sidebar.
2. With **Scope** set to **All ready tracks**, select **Scan tracks**.

A scan only previews. It reports *No metadata changes needed with these rules* until you have written rules, and otherwise lists each proposed change with the before and after. Nothing is written until you select **Apply**.

**Go deeper:** [Clean up messy titles, genres and artists](../tagging/cleanup-messy-titles.md) is the tutorial, [Tag new tracks from Beatport and Traxsource](../tagging/tag-new-tracks-automatically.md) turns on automatic tagging, and [Edit tags and artwork](../tagging/edit-tags-by-hand.md) fixes one track by hand.

## 6. Build the Warm-up set { #playlist }

1. Select the **+** (**Create playlist**) beside **PLAYLISTS**, name it `Club nights` and set **Playlist type** to **Folder**.
2. Create another, named `Warm-up set`, as a **Playlist** inside it.
3. In **Tracks**, hold ++cmd++ (++ctrl++ on Windows and Linux) and click these eight tracks in this order.
4. Choose **Club nights / Warm-up set** in the **Choose playlist…** menu and select **Add to playlist**.

| # | Track | Key | BPM | Energy |
|---|---|---|---|---|
| 1 | When The Fire Starts To Burn (Extended Mix) | 7A | 126 | 6/10 |
| 2 | Palm Of Your Hands (Extended Mix) | 11A | 127 | 6/10 |
| 3 | Desert (Original Mix) | 12A | 128 | 6/10 |
| 4 | F\*ck The Club (Extended Mix) | 12A | 128 | 6/10 |
| 5 | Murda (Extended Mix) | 1A | 128 | 6/10 |
| 6 | Shake The Block (Extended Mix) | 1A | 128 | 6/10 |
| 7 | Underground (Extended Mix) | 2A | 129 | 6/10 |
| 8 | Atoma (Extended Mix) | 2A | 130 | 6/10 |

**You should see** the playlist climb from <span class="cr-bpm">126</span> to <span class="cr-bpm">130</span> BPM with the keys stepping around the wheel. Tracks are added in the order you clicked them.

**Go deeper:** [Create a playlist and add tracks](../playlists/create-a-playlist.md) is the full tutorial, [Check the flow of a playlist](../playlists/check-playlist-flow.md) reads the transitions back to you, and [Build a smart playlist](../playlists/build-a-smart-playlist.md) lets CueRithm pick tracks by rule.

## 7. Send it to rekordbox { #sync }

!!! warning "Close rekordbox first"
    Quit rekordbox and rekordbox Agent before you preview or sync. CueRithm writes to rekordbox's database directly and saves a backup you can restore.

1. Choose **Sync** in the sidebar and set **Sync to** to **rekordbox 6 / 7**.
2. Under **Installed library**, pick your rekordbox library. The panel should read *Ready for preview*. *Read only* means CueRithm can look but not write to that build.
3. Set **What to sync** to **Playlist or folder**, then choose **Club nights**, the folder that holds **Warm-up set**.
4. Leave **Include in sync** at its defaults and select **Preview rekordbox sync**.
5. Read the summary, then select **Sync rekordbox**.

**You should see** *Sync completed* with the track and row counts. Open rekordbox: the **Club nights** folder is there with **Warm-up set** inside it, and the tracks carry their cues and grids. Tracks that rekordbox does not know yet take CueRithm's data automatically; for tracks it already has, the preview lists the fields that differ so you can choose. **rekordbox sync history** keeps a **Restore backup** button.

CueRithm has tested direct rekordbox writes on macOS builds 6.8.7.0303 and 7.2.19.0342. Other DJ programs use the same **Sync to** menu: VirtualDJ, Engine DJ, djay Pro, Serato DJ Pro 4 and Pioneer USB.

**Go deeper:** [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md), [Add only new tracks to rekordbox](../sync/rekordbox-new-tracks.md), [Resolve rekordbox sync conflicts](../sync/rekordbox-conflicts.md), and [My sync does not show up in rekordbox](../help/troubleshooting/rekordbox-sync-problems.md).

## What you learned

- A watch folder turns new purchases into prepared tracks without further clicks.
- Key, BPM and one whole-track energy rating are in the table; cues and the grid are in the player.
- Cleanup and tagging preview first. Cleanup records the tags it replaces, so you can undo it.
- A playlist stays in your library until you sync it, and sync is previewed, backed up and restorable.

## Next steps

- [Build a smart playlist](../playlists/build-a-smart-playlist.md) keeps *Energy 7 and up* or *Mixes with 7A* current as music arrives.
- [Send your library to DJ software](../sync/index.md) compares every destination.
- [Back up your library](../files/library/back-up-library.md) before you rely on it at a gig.
- [Pick the right guide for your goal](choose-your-path.md) lists everything else CueRithm does.

<!-- verified: apps/desktop/src/views/Settings.tsx (Incoming page, Add watch folder, Save settings, nine pages); App.tsx (header status texts, Sort by energy via TrackTable sort keys, Create playlist, Choose playlist… and Add to playlist via PlaylistMembershipControls.tsx, PlaylistEditor.tsx fields); views/Cleanup.tsx (Scope, All ready tracks, Scan tracks, "No metadata changes needed with these rules.", Apply, Undo history; "Scanning previews changes without writing file tags"); views/Sync.tsx and RekordboxSync.tsx (Sync to options, Installed library, What to sync, Include in sync defaults from rekordbox-sync-types.ts, Preview rekordbox sync, Sync rekordbox, Sync completed, rekordbox sync history, Restore backup, close rekordbox and Agent); crates/cuerithm-rekordbox/src/write_profile.rs and docs/research/rekordbox-direct-contract.md (tested builds, read-only restrictions); PlayerPanel.tsx (cue strip, arrow keys jump 240/bpm seconds); lexicon_publication.rs (published cue names); values and track order from _meta/the private demo-library facts sheet and playlists/create-a-playlist.md. -->
