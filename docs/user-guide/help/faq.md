# Frequently asked questions

<p class="lead">Short answers to the questions DJs ask most, grouped by what you are trying to do. Every answer links to the page with the steps.</p>

If your question is about a symptom, start with [Fix a problem](troubleshooting/index.md) instead.

## Is my music safe? { #safety }

### Does CueRithm change my audio files? { #changes-files }

Analysis, playback, playlists and smart playlists never write to your files. CueRithm writes to a file only when you ask it to:

- Automatic tagging and Cleanup change tags. They write to a staged copy, decode it again to check that the playable audio is identical, and hold the original until the write has settled ([How are tags written safely?](../how-it-works/tag-writing.md)). Both are off until you switch them on.
- **Edit metadata** changes the tags you edit ([Edit tags and artwork](../tagging/edit-tags-by-hand.md)).
- **Curate** writes to file tags only if you map categories to file fields ([Group tracks with categories and custom tags](../tagging/curate-tracks.md)).
- **Organize files** can rename, move or copy files ([Rename and sort files into folders](../rename/organize-files.md)).
- Sync to Serato DJ Pro 4 writes cues and grids into the music files, and keeps a backup ([Sync to Serato DJ Pro](../sync/serato-dj-pro.md)).

### Does it move or copy my music? { #moves-files }

Not by default. Incoming, Tracks and Archive are states inside the library, and files stay where they are. Only the File organization setting (**Leave in place**, rename, **Move** or **Copy**) or a reviewed plan in **Organize files** changes that ([Rename and sort files into folders](../rename/organize-files.md)).

### Can I undo what CueRithm changed? { #undo }

Mostly, yes. Cleanup has **Undo** ([Undo a cleanup](../tagging/undo-cleanup.md)), tag edits have history ([Undo a tag change](../tagging/tag-history.md)), a reorganisation can be reversed ([Recover or undo a reorganisation](../rename/undo-organize.md)), a sync has **Restore backup** ([Preview, apply and restore a sync](../sync/preview-apply-restore.md)), a duplicate merge has **Undo merge**, and removing the records of missing tracks has **Undo removal**, for your last 10 removals ([Undo a merge or put back removed files](../files/library/undo-maintenance.md)).

### Where are my original files after a tag write? { #originals }

Your file is the tagged one, in the same folder. While a write is under way CueRithm holds a copy of the original in its data folder under `tagging/`, and deletes it when the write has settled. **Settings** → **Automatic tagging** shows what it holds as **Recovery backups**. Undo of a cleanup or a manual edit writes back the tags it recorded, not a copy of the file ([What CueRithm does not do yet](not-available-yet.md#backup-retention)). [Where CueRithm keeps your data](../files/library/where-data-lives.md) lists the folders.

### Is my music uploaded anywhere? { #uploads }

No. Key, BPM, energy and cues are computed on your computer. Catalog tagging sends a search query (title and artist) to the catalog you chose. Optional recording identification, which is off until you allow it, sends a compressed audio fingerprint and the duration to AcoustID, never the audio file. A Cloudflare solver, if you set one up, receives catalog addresses ([Get past a Cloudflare block](../tagging/cloudflare-solver.md)).

### Does it need an internet connection, or an account? { #internet }

No account. Analysis, cues, playlists, cleanup, sync and backups work offline. Only catalog tagging and recognition use the network, and catalog tagging is off by default ([Tag new tracks from Beatport and Traxsource](../tagging/tag-new-tracks-automatically.md)). Some providers need their own account ([Connect provider accounts](../tagging/provider-accounts.md)).

## Installing and starting { #installing }

### Which computers does CueRithm run on? { #platforms }

macOS on Apple Silicon, Windows on x86-64 and Linux on x86-64. There is no Intel Mac build ([Download and install CueRithm](../get-started/install.md)). The Windows and Linux builds are unsigned ([My computer blocks CueRithm at first launch](troubleshooting/first-launch-blocked.md)).

### The app says "This library is already open" { #already-open }

Only one CueRithm process can use a library at a time. Close the other window or process, including a command line run, and open the app again.

### The app will not start and mentions a decoder hash { #decoder-hash }

The bundled decoder does not match its sealed hash, which means the installation is damaged. Reinstall from a fresh download and run the built-in check ([My computer blocks CueRithm at first launch](troubleshooting/first-launch-blocked.md#diagnostics)).

### Does CueRithm update itself? { #updates }

No. Download new releases from the releases page ([What CueRithm does not do yet](not-available-yet.md#no-auto-update)). A newer version may upgrade your library, and an older version refuses a library that a newer one has opened, so [back up your library](../files/library/back-up-library.md) first.

## Adding and preparing music { #preparing }

### Which audio formats work? { #formats }

MP3, AAC (ADTS and M4A), ALAC, FLAC, WAV, AIFF, Ogg Vorbis, Opus and WMA, all with the decoder that ships inside the app ([Check that your files are supported](../files/supported-formats.md)).

### A file I dropped never appears { #dropped-file }

Files you drop one by one are not filtered by extension, so an unsupported file fails with an error in Incoming. In folders and watch folders CueRithm picks up only these extensions: `mp3`, `aac`, `m4a`, `alac`, `flac`, `wav`, `wave`, `aif`, `aiff`, `aifc`, `ogg`, `oga`, `opus` and `wma`. Hidden files are ignored, and subfolders are included unless you turn off **Include subfolders** ([Add tracks: drop, pick or watch a folder](../files/add-tracks.md)).

### Why is a track stuck in Incoming? { #stuck }

Read the second line of its row. It names the stage or the error ([A track is stuck in Incoming](troubleshooting/incoming-stuck.md), [Fix a track that needs attention](../files/needs-attention.md)).

### Why was my long DJ mix rejected? { #long-mix }

Decoding stops at about 24 minutes, so long mixes do not prepare ([What CueRithm does not do yet](not-available-yet.md#long-tracks)).

### Why did my track not get a Breakdown cue? { #no-breakdown }

A breakdown needs a long enough section after the drop (64 beats by default), and it is dropped if it falls in the first quarter of the track or in the last 30 seconds. You can lower the minimum under **Settings** → **Cue generation** → **Minimum breakdown length**. Tracks that are already finished change only when you choose **Analyze again** ([Choose how cues are placed](../analysis/cue-rules.md), [How are cues placed?](../how-it-works/cues.md)).

### The BPM looks doubled or halved { #bpm-folded }

CueRithm searches 98 to 195 BPM and folds a track into that range, so a 94 BPM track shows as 188. There is no range setting ([My key or BPM differs from another app](troubleshooting/key-or-bpm-differs.md)).

### The beat lines drift on a live-drummer track { #grid-drift }

The grid is one constant tempo. You can correct it by hand ([Fix a wrong beatgrid](../analysis/corrections/fix-the-beatgrid.md)).

### I changed a setting and old tracks did not change { #old-tracks }

Saving settings leaves finished results alone. Choose **Analyze again** on a track to apply the new settings to it ([Analyze a track again](../analysis/analyze-again.md)).

### Does CueRithm read the key or BPM from my tags? { #tags-as-input }

No. It measures key, BPM, energy and cues from the decoded audio and never uses tags, file names or other programs' labels as its answer ([How does CueRithm analyse a track?](../how-it-works/pipeline.md), [Coming from Mixed In Key, Lexicon or rekordbox](../get-started/coming-from-other-tools.md)).

## Tagging and cleanup { #tagging }

### Why is tagging off? { #tagging-off }

It needs the network and rewrites tags, so it is opt-in under **Settings** → **Automatic tagging** ([Choose providers and fields to write](../tagging/choose-providers.md)).

### Tracks are stuck at "Matching catalog" { #matching-catalog }

You may be offline, or the provider may be blocking requests. The tracks stay in Incoming. Check your connection and retry. If Traxsource reports a browser challenge, set up a solver ([Catalog tagging finds nothing](troubleshooting/no-catalog-match.md), [Get past a Cloudflare block](../tagging/cloudflare-solver.md)).

### Cleanup changed too much { #cleanup-too-much }

Undo it from **Undo history** in Cleanup, then narrow the rules or the scope ([Cleanup changed too much](troubleshooting/cleanup-changed-too-much.md)).

## Playlists { #playlists }

### Why can't I add a track to a smart playlist? { #smart-add }

Smart playlists are rule-driven. Use an ordinary playlist for hand-picked tracks, or turn a smart playlist into an ordinary one with **Playlist tools** → **Save ordinary snapshot** ([Create a playlist and add tracks](../playlists/create-a-playlist.md), [Combine, split and tidy playlists](../playlists/combine-and-tidy-playlists.md)).

### My smart playlist is empty or says it is stale { #smart-stale }

Preview its rules again in the editor. If an evaluation fails, the playlist keeps its last good result and shows an error ([Write smart playlist rules](../playlists/smart-rules.md)).

## Syncing to DJ software { #syncing }

### Which DJ software can I sync to? { #destinations }

rekordbox 6 / 7, VirtualDJ, Engine DJ, djay Pro, Serato DJ Pro 4, Pioneer USB and Mixxx. Sync goes one way, from CueRithm to your DJ software, and there is no rekordbox XML ([Send your library to DJ software](../sync/index.md), [Check what works with your DJ software](compatibility.md)).

### Why does nothing show up in rekordbox? { #rekordbox }

Usually rekordbox or its Agent is still running, or the scope or options left things out ([My sync does not show up in rekordbox](troubleshooting/rekordbox-sync-problems.md)).

## Files and libraries { #files }

### I moved my files and now tracks are missing or doubled { #moved-files }

A move inside a watch folder is followed by the track itself (CueRithm 0.1.15 or later), so you only see missing or doubled tracks when the files were edited, could belong to several tracks or left the watch folder. For unchanged files, tick the missing tracks in **Missing & broken** and use **Find moved files…**. Use [Relocate](../files/library/find-missing-files.md#relocate) on the original tracks to review a replacement file or recursively search a folder, and **Find Lost Tracks** for folder-root or drive changes (CueRithm 0.1.16 or later). If the destination already belongs to another track, choose the complete survivor; ordinary playlist links are rewired and guarded Undo is retained. For a file that is gone for good, **Remove from library…** removes only the record. [I moved or renamed my files](troubleshooting/files-moved.md#fix) gives the steps.

### Where is my data? { #data-location }

In the application data folder, which holds the library database, caches and recovery backups. Your music is never inside it ([Where CueRithm keeps your data](../files/library/where-data-lives.md)).

## Accuracy and help { #accuracy }

### How accurate are key, BPM and energy? { #accuracy-how }

CueRithm's results are measured against reference tools on the owner's own library. That is agreement, not proof of musical accuracy, and held-out accuracy is not measured ([How accurate is it?](../how-it-works/accuracy.md)).

### Where do I report a problem? { #report }

On GitHub. Please do not attach copyrighted audio ([Report a problem or ask for a feature](report-a-problem.md)).

## Related

- [Fix a problem](troubleshooting/index.md)
- [What CueRithm does not do yet](not-available-yet.md)
- [Report a problem or ask for a feature](report-a-problem.md)
- [Get help](index.md)

<!-- verified: legacy about/faq.md (every answer re-checked); README.md (nothing uploaded, no account, Install, Analysis and file handling); apps/desktop/src/views/Settings.tsx (File organization modes: leave in place default; Include subfolders in TaggingSettings.tsx; Cue generation Minimum breakdown length); PlaylistTools.tsx (Save ordinary snapshot); RecognitionSettings.tsx and cuerithm-autotag/src/recognition.rs (fingerprint lookup off by default, no audio uploaded); crates/cuerithm-library/src/watch.rs (supported extensions, hidden files), prepare.rs enqueue_paths (dropped files unfiltered); camelot-core/src/lexicon_cues.rs (64 beats, BD_MIN_POS 0.25, BD_MIN_GAP_SEC 30); lexicon_timing/mod.rs (98-195); store.rs ("This library is already open", schema check); no updater; docs/release-readiness.md (Curate file mappings, Serato backups include source audio); coordinator notes on 0.1.14 moved-file behaviour. crates/cuerithm-dj-sync/README.md (Serato previews stage every affected audio file; native media tags) -->
