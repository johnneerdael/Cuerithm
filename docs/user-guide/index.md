---
title: CueRithm user guide
hide:
  - navigation
  - toc
---

<div class="cr-hero" markdown>

![CueRithm](assets/brand/cuerithm-logo-on-light-descriptor.svg#only-light){ .cr-hero__logo }
![CueRithm](assets/brand/cuerithm-logo-on-dark-descriptor.svg#only-dark){ .cr-hero__logo }

# Know your music. Shape your set.

CueRithm prepares a DJ library on your own computer. Drop in new music and each track gets a key, a BPM, a beatgrid, one energy rating, labelled cues and a colour waveform. Then tag it from Beatport and Traxsource, clean up titles and genres, build playlists and smart playlists, and send the result to rekordbox, VirtualDJ, Engine DJ, djay Pro, Serato DJ Pro 4, Pioneer USB or Mixxx.

[Prepare your first tracks](get-started/first-tracks.md){ .md-button .md-button--primary }
[Download CueRithm](https://github.com/johnneerdael/CueRithm/releases/latest){ .md-button }

</div>

<figure markdown>
  ![CueRithm showing a track library with key, BPM and energy columns above a colour waveform with five labelled cues](assets/screens/40-player.webp){ .screen loading=lazy }
  <figcaption>The CueRithm library: sidebar, track table and the persistent player with beatgrid, waveform and cues.</figcaption>
</figure>

!!! info "Version 0.1: early software"
    macOS builds for Apple Silicon are signed with a Developer ID and notarized by Apple. Windows and Linux builds are unsigned, and there is no Intel Mac build. [Check what works with your DJ software](help/compatibility.md) and [what is not available yet](help/not-available-yet.md) before you rely on a feature.

<span id="what-cuerithm-does"></span>

## What do you want to do? { #what-do-you-want-to-do }

<div class="grid cards" markdown>

-   :material-folder-music-outline:{ .lg .middle } **Manage your music files**

    ---

    Drop files, pick them or let CueRithm watch a folder. Each track is decoded, analysed and added to your library without moving the file.

    [:octicons-arrow-right-24: Manage your music files](files/index.md)

    Popular: [Keep a folder like Beatport in your library](files/watch-folders.md) · [Follow a track through Incoming](files/follow-incoming.md)

-   :material-playlist-music:{ .lg .middle } **Build playlists and sets**

    ---

    Playlists and folders, smart playlists that keep themselves current, a generator, compatible-track search, flow checks and tracklist matching.

    [:octicons-arrow-right-24: Build playlists and sets](playlists/index.md)

    Popular: [Build a smart playlist](playlists/build-a-smart-playlist.md) · [Find tracks that mix with the one playing](playlists/find-compatible-tracks.md)

-   :material-tag-edit-outline:{ .lg .middle } **Tag from Beatport and Traxsource**

    ---

    Fill in titles, artists, labels, genres and artwork automatically, review every match, and edit anything by hand.

    [:octicons-arrow-right-24: Tag and clean up your music](tagging/index.md)

    Popular: [Tag new tracks automatically](tagging/tag-new-tracks-automatically.md) · [Review and fix a wrong match](tagging/review-a-match.md)

-   :material-broom:{ .lg .middle } **Clean up titles, genres and artists**

    ---

    Strip suffixes, merge genre names and tidy artist credits. Review the changes first and undo them later.

    [:octicons-arrow-right-24: Clean up messy titles, genres and artists](tagging/cleanup-messy-titles.md)

    Popular: [Write cleanup rules](tagging/cleanup-rules.md) · [Cleanup recipes](tagging/recipes/index.md)

-   :material-rename-box:{ .lg .middle } **Rename and sort your files**

    ---

    Name every file from one pattern and sort it into folders by artist, after a preview of every change, with Undo.

    [:octicons-arrow-right-24: Rename and sort your music files](rename/index.md)

    Popular: [Rename and sort files into folders](rename/organize-files.md) · [Build a naming pattern](rename/naming-patterns.md)

-   :material-waveform:{ .lg .middle } **Read, preview and correct tracks**

    ---

    Key, BPM and energy at a glance, a player with a beatgrid, and labelled cues to jump between.

    [:octicons-arrow-right-24: Read, preview and correct your tracks](analysis/index.md)

    Popular: [Read a track's key, BPM and energy](analysis/read-key-bpm-energy.md) · [Jump to the drop with cues](analysis/jump-to-cues.md)

-   :material-target:{ .lg .middle } **Fix a cue, loop or beatgrid**

    ---

    Move a cue, set a loop, correct a grid that starts late, and reuse one cue layout across many tracks.

    [:octicons-arrow-right-24: Fix a wrong cue, loop or beatgrid](analysis/corrections/index.md)

    Popular: [Fix a wrong beatgrid](analysis/corrections/fix-the-beatgrid.md) · [Reuse one cue layout on many tracks](analysis/corrections/cue-templates.md)

-   :material-sync:{ .lg .middle } **Send it to your DJ software**

    ---

    Preview what will change, apply it, and restore the backup if you need to. Seven destinations, including a direct rekordbox sync.

    [:octicons-arrow-right-24: Send your library to DJ software](sync/index.md)

    Popular: [Sync to rekordbox 6 or 7](sync/rekordbox-direct.md) · [Sync to Mixxx](sync/mixxx.md)

-   :material-shield-check-outline:{ .lg .middle } **Look after your library**

    ---

    Find duplicates, missing and unused files, and keep a backup you can restore.

    [:octicons-arrow-right-24: Look after your library](files/library/index.md)

    Popular: [Find missing, moved or broken files](files/library/find-missing-files.md) · [Back up your library](files/library/back-up-library.md)

-   :material-lifebuoy:{ .lg .middle } **Something is not working**

    ---

    Match what you see to a fix, or read the questions people ask first.

    [:octicons-arrow-right-24: Get help](help/index.md)

    Popular: [A track shows a dash or will not play](help/troubleshooting/no-waveform-or-playback.md) · [I moved or renamed my files](help/troubleshooting/files-moved.md)

-   :material-palette-outline:{ .lg .middle } **Make it yours**

    ---

    Light or dark, waveform style and layout, and where to find each setting.

    [:octicons-arrow-right-24: Customize](customize/index.md)

    Popular: [Switch between light and dark](customize/theme.md) · [Find the setting you need](customize/find-a-setting.md)

-   :material-cog-transfer-outline:{ .lg .middle } **Know how it works**

    ---

    How a number is computed, what each analysis stage does and how accurate it is.

    [:octicons-arrow-right-24: How it works](how-it-works/index.md)

    Popular: [How does CueRithm analyse a track?](how-it-works/pipeline.md) · [How accurate is it?](how-it-works/accuracy.md)

-   :material-book-open-variant:{ .lg .middle } **Look something up**

    ---

    Keyboard shortcuts, every setting, cue names, smart playlist fields, file naming tokens and a glossary.

    [:octicons-arrow-right-24: Reference](reference/index.md)

    Popular: [Keyboard shortcuts](reference/keyboard-shortcuts.md) · [Smart playlist fields and operators](reference/smart-playlist-fields.md)

</div>

<span id="where-to-go-next"></span>

## New here? { #new-here }

1. [Download and install CueRithm](get-started/install.md) for [macOS](get-started/platforms/macos.md), [Windows](get-started/platforms/windows.md) or [Linux](get-started/platforms/linux.md).
2. [Prepare your first tracks](get-started/first-tracks.md) with a handful of files.
3. Follow [From a folder of new music to a playlist for the booth](get-started/first-hour.md), or [pick the right guide for your goal](get-started/choose-your-path.md).

Coming from Mixed In Key, Lexicon or rekordbox? [Read what carries across](get-started/coming-from-other-tools.md). Want the recent changes? [What's new](get-started/whats-new.md).

Use the search box (press ++s++ or ++slash++) to find a topic in this guide.

<span id="analysis-and-file-handling"></span>

## How CueRithm treats your files { #your-files }

Analysis and cue generation run offline. Only the optional catalog tagging uses the network. Your audio files stay where they are unless you turn on file organisation or review and apply a maintenance action. Tag writes use a staged copy: CueRithm verifies that the playable audio is unchanged, holds the original until the write has settled and swaps the files atomically. Read [How are tags written safely?](how-it-works/tag-writing.md) and [Where CueRithm keeps your data](files/library/where-data-lives.md).

CueRithm uses [OneTagger](help/about/credits.md#onetagger), FFmpeg, TagLib and other open-source components. Mixed In Key and Lexicon DJ shaped how it behaves; the owner studied them using copies they own and wrote an independent implementation in Rust. Read [how CueRithm was made](help/about/provenance.md).

## Get help and follow changes { #get-help }

Download installers and read release notes on the [GitHub releases page](https://github.com/johnneerdael/CueRithm/releases). If a step in this guide differs from your build, [report a problem](help/report-a-problem.md) with your CueRithm version, your operating system and the steps you tried.

<!-- verified: README.md and legacy index.md (provenance, signing, destinations); roadmap for the seven destinations; links per the plan in _meta/ia.md -->
