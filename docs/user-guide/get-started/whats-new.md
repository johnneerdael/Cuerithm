# What's new in CueRithm

<p class="lead">The eight most recent releases, newest first, in plain words and grouped by what they let you do. Every item links to the page that explains it. Reading it takes about five minutes.</p>

CueRithm is pre-1.0 software. The version you have is the `version` in the line printed by the [built-in check](install.md#check-that-the-install-is-healthy). To get a newer one, follow [Update to a newer version](install.md#update); the full text of every release, with its downloads, is on the [releases page](https://github.com/johnneerdael/CueRithm/releases).

## Find a feature by what it does { #by-benefit }

| You want to… | Arrived in | Guide |
|---|---|---|
| Reconnect moved files when a folder or drive changes | 0.1.16 | [Find missing, moved or broken files](../files/library/find-missing-files.md#relocate) |
| Take your library to another computer | 0.1.16 | [Back up your library](../files/library/back-up-library.md) and [Restore your library from a backup](../files/library/restore-library.md) |
| Bring preparation in from a Lexicon backup | 0.1.16 | [Import a Lexicon backup](../files/library/restore-library.md#lexicon) |
| Keep moved files as the same track | 0.1.15 | [Move or rename files without breaking the library](../files/move-files.md) |
| Clear out tracks whose files are gone | 0.1.15 | [Remove the records of files that are gone](../files/library/find-missing-files.md#remove) |
| Reconnect moved files by their content | 0.1.15 | [Find moved files by their content](../files/library/find-missing-files.md#find-moved) |
| Sync to Mixxx | 0.1.14 | [Sync to Mixxx](../sync/mixxx.md) |
| Sync straight into rekordbox | 0.1.11, simplified in 0.1.13 | [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md) |
| Sync a Windows DJ library from Linux | 0.1.12 | [Use a Windows DJ library on Linux](../sync/windows-libraries-on-linux.md) |
| Correct cues, loops and beatgrids | 0.1.11 | [Fix a wrong cue, loop or beatgrid](../analysis/corrections/index.md) |
| Find duplicates and missing files | 0.1.11 | [Look after your library](../files/library/index.md) |
| Find and plan what mixes | 0.1.11 | [Find tracks that mix with the one playing](../playlists/find-compatible-tracks.md) |
| Back up and restore the library | 0.1.11 | [Back up your library](../files/library/back-up-library.md) |
| Edit tags and artwork by hand | 0.1.10 | [Edit tags and artwork](../tagging/edit-tags-by-hand.md) |
| Save presets and use them per folder | 0.1.10 | [Save your setup as a preparation preset](../files/presets.md) |
| Tag from more catalogs | 0.1.10 | [Choose providers and fields to write](../tagging/choose-providers.md) |
| Rename and sort files | 0.1.9 | [Rename and sort files into folders](../rename/organize-files.md) |

## Version 0.1.16: reconnect moved music and move your library { #v0-1-16 }

Released 23 October 2026.

- **Reconnect moved music.** Use **Relocate** or **Find Lost Tracks** to keep saved cues and playlist membership when files move. Review uncertain matches and undo library changes: [Find missing, moved or broken files](../files/library/find-missing-files.md#relocate) and [Undo relocation](../files/library/find-missing-files.md#undo-relocation).
- **Take your library to another computer.** Export a metadata-only `.cuerithm` backup, copy music separately, and map its folders when restoring. Your previous library stays available for recovery: [Back up your library](../files/library/back-up-library.md#create) and [Connect music folders](../files/library/restore-library.md#connect-folders).
- **Bring preparation from Lexicon.** Import the supported Lexicon v26 backup profile, including saved cues and tempo changes. Other Lexicon versions remain unsupported: [Import a Lexicon backup](../files/library/restore-library.md#lexicon).

## Version 0.1.15: moved music stays in your library { #v0-1-15 }

Released 23 October 2026.

If **Missing & broken** in your copy has no **Find moved files…** button, you are on version 0.1.14 or older.

- **Moved music stays in your library.** If you move or rename files inside a watch folder, even while CueRithm is closed, the track keeps its analysis, cues, playlists and tags instead of turning into a duplicate plus a broken entry. Files that match more than one track are left alone for you to review. See [Move or rename files without breaking the library](../files/move-files.md) and [Keep a folder like Beatport in your library automatically](../files/watch-folders.md).
- **Clean up tracks whose files are gone.** In **Maintenance**, **Missing & broken** lets you select some or all missing tracks, review what changes (playlists and links), and remove only their library entries, never the audio. Undo stays available for your 10 most recent removals: [Remove the records of files that are gone](../files/library/find-missing-files.md#remove) and [Undo a merge or put back removed files](../files/library/undo-maintenance.md#undo-repairs).
- **Reconnect moved files.** Pick a folder or a file and CueRithm finds your missing tracks by their exact content, shows the matches, and reconnects them with cues, playlists and tags intact: [Find moved files by their content](../files/library/find-missing-files.md#find-moved).
- **See why a preview is empty.** The Tracks table and the player now say *File not found - move it back or run Maintenance* instead of a bare dash, and other empty previews explain themselves too: audio that could not be read, no waveform data, or a library request that failed. See [A track shows a dash or will not play](../help/troubleshooting/no-waveform-or-playback.md).
- **Archived tracks are included.** Missing & broken can list archived tracks too, so they can be repaired or removed the same way.
- **Readable status labels.** *Unavailable* in the **Outcome** column no longer breaks in the middle of the word.
- **Clearer labels and the right shortcut for your system.** Incoming names the cleanup step *Cleaning up tags*. In **Review match**, differences read in plain words such as *Edition differs* and *ISRC differs*, and sources use their real names, such as Beatport, Traxsource and MusicBrainz. The small waveform in Tracks follows your saved cues, so it updates after you correct a track. The search box and sidebar hints show Ctrl on Windows and Linux and the Command key on a Mac, the search shortcut no longer jumps out from behind an open dialog, and the sidebar shows your real version number. See [Review and fix a wrong match](../tagging/review-a-match.md) and [Keyboard shortcuts](../reference/keyboard-shortcuts.md).
- **Help text matches what the app does.** Album matching works with Beatport and Traxsource, saved active loops go to rekordbox, VirtualDJ, Engine DJ and Pioneer USB, and the playlist generator makes energy lists for 1 to 10 only, without an empty zero list: [Generate playlists](../playlists/generate-playlists.md).

## Version 0.1.14: sync to Mixxx { #v0-1-14 }

Released 23 October 2026.

If **Sync to** in your copy does not list Mixxx, you are on version 0.1.13 or older.

**Sync to Mixxx.** Send your beatgrids, keys, named and coloured hot cues, saved loops and playlists straight into your Mixxx library. Close Mixxx first; every sync keeps a backup you can restore. It was tried with Mixxx 2.5.6 on macOS and with Debian's Mixxx 2.5.0 on Linux; Windows is not tried yet. See [Sync to Mixxx](../sync/mixxx.md) and [Check what works with your DJ software](../help/compatibility.md#mixxx).

- **Start Mixxx with your library already in it.** If you have never opened Mixxx, CueRithm can create its library for you, and Mixxx then opens with your tracks and does not ask for a music folder.
- **Replace your whole Mixxx library.** As with rekordbox, you can have Mixxx's library rebuilt from CueRithm's. Mixxx's current library is kept in the backup and comes back with **Restore backup**. This needs the full library.
- **Keep what you did in Mixxx.** Adding only leaves tracks Mixxx already has exactly as they are. Managing the library replaces only what you tick, and never your own Mixxx playlists, history or folders.
- **Smart playlists become playlists.** Mixxx has no smart playlists, so they arrive as ordinary playlists or crates of the tracks they hold today.
- **Works with newer Mixxx releases.** CueRithm goes by the library itself, and the preview tells you when your Mixxx is newer than what was tried.
- **Beatgrids line up across formats.** WAV, FLAC, MP3, AAC, Apple Lossless, AIFF, Ogg Vorbis and Opus grids and cues were checked against where Mixxx decodes the first beat. Mixxx cannot open WMA files, so those tracks are added but will not play there.

## Version 0.1.13: faster rekordbox syncs { #v0-1-13 }

Released 22 October 2026.

- **Sync new tracks with fewer steps.** Tracks that are new to rekordbox take the CueRithm data you selected automatically. Empty fields no longer prompt again on the next sync, and differences on tracks rekordbox already has remain available for review. See [Add only new tracks to rekordbox](../sync/rekordbox-new-tracks.md) and [Resolve rekordbox sync conflicts](../sync/rekordbox-conflicts.md).
- **Find rekordbox in one Sync menu.** Choose rekordbox 6 / 7 from the same **Sync to** menu as your other DJ libraries: [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md).

## Version 0.1.12: set-up once, sync to Windows libraries { #v0-1-12 }

Released 22 October 2026.

- **Reuse your DJ library setup.** Save application locations and drive mappings for your DJ apps, including Windows apps used through Wine or Proton on Linux. Playback through Wine or Proton is not yet verified. See [Save a destination for next time](../sync/saved-destinations.md) and [Use a Windows DJ library on Linux](../sync/windows-libraries-on-linux.md).
- **Set up rekordbox libraries more easily.** Configure custom rekordbox 6 and 7 libraries. When you sync a mapped Windows library from Linux, track references keep their Windows paths: [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md).
- **Keep your existing library details.** VirtualDJ keeps its configured Windows library, playlist layout and extra cue and loop settings. djay keeps existing tracks linked when equivalent Windows file paths are written differently. See [Sync to VirtualDJ](../sync/virtualdj.md) and [Sync to djay Pro](../sync/djay-pro.md).
- **Sync Engine libraries across drives.** Music can live on a different Windows drive from the Engine library: [Sync to Engine DJ](../sync/engine-dj.md).
- **Keep useful track and loop details.** Existing bitrate, file size and track numbers stay when the source has no replacement value. USB libraries retain active-loop settings, and compatible WAV copies record their output bitrate. See [Prepare a Pioneer USB stick](../sync/pioneer-usb.md) and [Set the sync options](../sync/sync-options.md).
- **Preview large libraries sooner.** Library previews through Wine or Proton no longer scan the same music folders repeatedly: [Preview, apply and restore a sync](../sync/preview-apply-restore.md).

## Version 0.1.11: sync to rekordbox, fix and plan { #v0-1-11 }

Released 21 October 2026.

**Send your library straight to rekordbox**

- Sync reviewed metadata, artwork, cues, loops, constant beatgrids, playlists and My Tags directly to rekordbox on macOS builds 6.8.7.0303 and 7.2.19.0342. Close rekordbox and its Agent before applying changes. Later releases widened which builds can be written to; [Sync to rekordbox 6 or 7](../sync/rekordbox-direct.md) has the current rule.

**Keep the library healthy**

- Review duplicates, missing files and unused files, with merge, removal, Undo and recovery controls: [Find and merge duplicate tracks](../files/library/find-duplicates.md), [Find missing, moved or broken files](../files/library/find-missing-files.md), [Find and remove unused files](../files/library/clean-up-unused-files.md) and [Undo a merge or put back removed files](../files/library/undo-maintenance.md).
- Back up library preparation state and settings, including metadata artwork, Curate, presets and Undo history, then schedule a guarded restore in the same CueRithm installation. Music files are excluded and must stay consistent with the backup: [Back up your library](../files/library/back-up-library.md) and [Restore your library from a backup](../files/library/restore-library.md).
- Opening your library upgrades it automatically, and older CueRithm versions cannot reopen the upgraded library. [Where CueRithm keeps your data](../files/library/where-data-lives.md) explains what is stored.

**Correct what analysis got wrong**

- Correct saved cues, loop boundaries and constant BPM and first-beat grids, with Undo. Local loop preview jumps to the start; repeating playback is unsupported. See [Move or add a cue](../analysis/corrections/move-or-add-cues.md), [Set a loop](../analysis/corrections/set-loops.md), [Fix a wrong beatgrid](../analysis/corrections/fix-the-beatgrid.md) and [Undo or resolve a correction](../analysis/corrections/undo-corrections.md).
- Save named cue templates, preview their results and apply them while preserving manual corrections by default: [Reuse one cue layout on many tracks](../analysis/corrections/cue-templates.md).

**Better exports to your DJ software**

- Active loops are preserved when you export to Engine DJ, VirtualDJ and Pioneer USB memory cues: [Sync to Engine DJ](../sync/engine-dj.md), [Sync to VirtualDJ](../sync/virtualdj.md) and [Prepare a Pioneer USB stick](../sync/pioneer-usb.md).
- djay keeps its beatgrid phase anchors when the tempo stays the same, and reuses native track identities through stored file bookmarks: [Sync to djay Pro](../sync/djay-pro.md).

**Plan a set**

- Find compatible tracks by key, BPM and optional energy, genre or tag filters, audition matches and save the selected tracks to a playlist: [Find tracks that mix with the one playing](../playlists/find-compatible-tracks.md).
- Review a playlist's BPM changes, adjacent key compatibility and whole-track energy in playlist order: [Check the flow of a playlist](../playlists/check-playlist-flow.md).
- Match pasted or imported text or CSV tracklists against your prepared library and build an ordered playlist from your choices: [Rebuild a set from a tracklist](../playlists/match-a-tracklist.md).

## Version 0.1.10: edit, tag and reuse your setup { #v0-1-10 }

Released 21 October 2026.

**Edit and organise tracks yourself**

- Select one track or many and change common metadata, ratings, comments and artwork. You see a before-and-after preview first, and Undo puts the old values back as long as the file has not changed since: [Edit tags and artwork](../tagging/edit-tags-by-hand.md) and [Undo a tag change](../tagging/tag-history.md).
- Edits are saved as you go. If you move to another view or quit with edits unsaved, they are still there when you come back. Editing a track never disturbs its analysis, and an edit you save for a track in Incoming is kept exactly as you reviewed it: automatic tagging and cleanup do not change the fields you edited, and cleanup still tidies the rest.
- Organise tracks into your own categories, add notes and give them a personal rating from the Curate panel. These values stay in your CueRithm library by default, separate from file comments and ratings. Curating saves as you go, and the Pending list offers Retry, Discard and Undo: [Group tracks with categories and custom tags](../tagging/curate-tracks.md).
- See which original files CueRithm keeps so that Undo works, separately from temporary clean-up files: [Where CueRithm keeps your data](../files/library/where-data-lives.md).

**Tag from more places, and check each match**

- Choose more tagging sources: MusicBrainz, Discogs, iTunes, Deezer, Bandcamp, SonoVault, Volumo and BPM Supreme, alongside Beatport and Traxsource. The new sources are optional and start switched off: [Choose providers and fields to write](../tagging/choose-providers.md).
- Save optional provider keys securely and test connections in Settings. Discogs works without an account; SonoVault and Volumo need a personal key only when you choose to use them: [Connect provider accounts](../tagging/provider-accounts.md).
- Review recording and release details before accepting a match, including MusicBrainz IDs and dates that contain only a year or month. Tracks that need attention in Incoming can be sorted out from inside the library: open Review, compare the catalog matches, see which edition each is, and choose which tags to take field by field. See [Review and fix a wrong match](../tagging/review-a-match.md) and [Fix a track that needs attention in Incoming](../files/needs-attention.md).
- Identify unresolved recordings with a locally generated audio fingerprint after giving permission to send it, then choose the correct release: [Identify a track with no match](../tagging/identify-unknown-tracks.md).

**Reuse your setup**

- Save cue, tagging, cleanup and file-organization settings as named preparation presets, and choose a different preset for each watched folder. Changing a preset leaves tracks you already prepared alone: [Save your setup as a preparation preset](../files/presets.md) and [Use different rules for different folders](../files/folder-presets.md).
- Export and import presets to back up your rules or move them to another machine. Keys and local connections stay private.

**Automate**

- Run library reports, preparation, metadata edits and file organization through the command interface, with previews and recovery for interrupted changes. It is aimed at developers; see [library automation](https://github.com/johnneerdael/CueRithm/blob/main/docs/automation.md) in the repository.

## Version 0.1.9: rename and sort your files { #v0-1-9 }

Released 21 October 2026.

- **Name and place your files from one pattern.** New and optional: pick a naming pattern and choose whether CueRithm renames, moves or copies tracks once Incoming has finished with them, or run it on tracks already in your library. You see a full preview first, and existing files are never overwritten. See [Rename and sort files into folders](../rename/organize-files.md) and [Build a naming pattern](../rename/naming-patterns.md).
- **Keep your DJ libraries pointing at the right files.** When a move or rename changes where a track lives, CueRithm also updates it in your linked DJ libraries and keeps a backup. If a library cannot be updated, the track is not moved, so your DJ library never points at missing files.
- **Undo a reorganisation.** Every organize run is saved in History, and Undo only touches files that have not changed since: [Recover or undo a reorganisation](../rename/undo-organize.md).
- **Decide what happens to copies.** If you copy files and later edit an original, you choose whether to keep the copy CueRithm made or add the changed file to Incoming as a new track.

## Earlier versions { #earlier }

Versions 0.1.0 to 0.1.8 are on the [releases page](https://github.com/johnneerdael/CueRithm/releases). [What CueRithm does not do yet](../help/not-available-yet.md) lists what is still missing.

## Related

- [Download and install CueRithm](install.md) explains how to update.
- [Send your library to DJ software](../sync/index.md) collects the sync features in one place.
- [Pick the right guide for your goal](choose-your-path.md) maps every feature to its page.

<!-- verified: gh release view v0.1.16, v0.1.15, v0.1.13, v0.1.12, v0.1.11, v0.1.10 and v0.1.9 (body and publishedAt; wording follows the published notes, internal and commit lists omitted); gh pr view 39 (merged 2026-10-23, release-notes section only, not in v0.1.13 per gh release list); no auto-update confirmed in apps/desktop/src-tauri/src/lib.rs (no updater plugin); linked pages exist in docs/user-guide on this branch; help/compatibility.md and sync/rekordbox-direct.md for the current rekordbox build rule. -->
