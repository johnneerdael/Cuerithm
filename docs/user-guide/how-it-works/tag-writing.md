# How are tags written safely?

<p class="lead">Writing tags, cleaning metadata and saving an edit are the only times CueRithm changes your audio files. Every one goes through a staged copy, an audio check and a journaled swap, so a crash, power cut or competing edit cannot leave a damaged file.</p>

!!! abstract "What this means for you"
    - **Your audio samples are verified unchanged.** The tagged copy is decoded again and must hold identical audio before it replaces your file. A mismatch stops the write with *Tagging changed playable audio or timeline*.
    - **The original is kept only while the write is under way.** CueRithm holds a copy of the file in the data folder until the write has settled, then deletes it. Undo of a manual cleanup or edit writes back the tag values recorded before the change, so no copy of your audio stays. See [Where CueRithm keeps your data](../files/library/where-data-lives.md).
    - **Your later edits win.** If you change a file while CueRithm is recovering, rollback stops with *Later user edit preserved; original backup retained* instead of overwriting you.
    - **Automatic tagging has no Undo button.** Manual Cleanup and manual metadata edits do: see [Undo a tag change](../tagging/tag-history.md) and [Undo a cleanup](../tagging/undo-cleanup.md).

## Which tags go where { #which-tags-go-where }

The container is detected from the file's real codec and format, not from its extension.

| Container | Tag system | Notes |
|---|---|---|
| MP3 | ID3v2.4 (default) or v2.3 | Written with the `id3` library |
| AAC (ADTS) | ID3v2 | Prepended to the stream |
| AAC and ALAC in M4A | MP4 / iTunes atoms | Standard atoms plus freeform `com.apple.iTunes:*` fields |
| FLAC | Vorbis comments | |
| Ogg Vorbis, Opus | Vorbis comments | |
| WAV | ID3 chunk, mirrored into `LIST/INFO` | Other chunks preserved |
| AIFF | ID3 chunk | |
| WMA | ASF attributes | Through a statically linked TagLib 2.3.2, used for WMA only |

For measured data, key uses the standard key field of each format and BPM uses the BPM field. MP4 gets both the integer `tmpo` atom and a two-decimal `BPM` freeform field. Energy is a custom `ENERGY` field, and an optional `CUERITHM_TAGGEDDATE` marker records when a file was tagged. Cues and beatgrids are never written to tags. The key is written as a Camelot code only when **Write Camelot keys** is on; otherwise it is the standard name such as "A minor".

### Which ID3 version is written { #id3-versions }

If the setting says v2.4 and a file has v2.3, it is upgraded when a write is needed, preserving literal slashes in names such as "AC/DC" and keeping frame flags and encodings. Writing v2.3 uses the older date frames and Latin-1 picture encoding for DJ-software compatibility. A tag is rewritten only when something actually changes.

## The safeguards { #the-seven-safeguards }

1. **Snapshot.** The decoder copies the source to a private snapshot and checks its SHA-256 before and after the copy. A change aborts the write (*Source changed during capture*).
2. **Two copies.** CueRithm copies the snapshot into the data folder as `original.<ext>` (your recovery backup while the write is under way) and `staged.<ext>`, and checks the original's hash again.
3. **Edit the copy only.** If an existing tag cannot be read, or an ID3 tag looks malformed, CueRithm refuses the write rather than discard it.
4. **Verify the audio.** The staged file is decoded again and must have the same frame count and an identical PCM hash. For cleanup and edits, the tags read back must also equal the intended values exactly (*Metadata cannot be represented exactly in this tag format*). A field you marked required must be present afterwards.
5. **Check the source again.** CueRithm hashes the original file once more to confirm you have not changed it in the meantime.
6. **Journal, flush, swap.** A journal row records the intended change. The files and their folders are flushed to disk, then the staged file replaces the original in one atomic exchange (macOS `RENAME_SWAP`, Linux `RENAME_EXCHANGE`, Windows `ReplaceFileW`). CueRithm checks the displaced original's hash. The destination must stay inside the source file's folder.
7. **Only then, the library.** The database transaction stores the analysis and cues and marks the track ready. If tagging fails, the track stays in Incoming and keeps its previous cues.

## Rollback and crash recovery { #rollback-and-crash-recovery }

- Any failure after the swap rolls the files back and marks the journal *rolled_back*, or *conflict* if the rollback could not complete.
- **A later edit by you is never overwritten.** If the file's hash is neither the original nor CueRithm's, rollback stops with *Later user edit preserved; original backup retained*.
- After a crash, a journal still in *prepared* is rolled back at the next start and the job is queued again.
- A cover file saved beside the audio and the audio itself are published and rolled back as one unit.

## Backups and retention { #backups }

While a write is under way the original bytes of the changed file are in the data folder under `tagging/<workspace>/original.<ext>`, and manual cleanup and manual metadata edits use `cleanup/` and `metadata-edits/`. They are what rollback and crash recovery read. When the write has settled (it committed, or it was rolled back) CueRithm deletes the copy and its folder. A write that changes nothing keeps no backup, and redundant staged copies are removed after the swap is durable. The total of the copies that exist appears as **Recovery backups** on **Settings → Automatic tagging**; it is normally zero.

Undo does not need the original. A manual cleanup or manual edit records the tag values it replaced, and Undo writes those values back into the file as it is now, through the same staged, checked and journaled write (CueRithm 0.1.19 or later; earlier versions kept the original file for ever and Undo copied it back). Artwork you replaced by hand is kept, because artwork is part of the library. Only the 10 newest manual cleanups and edits can be undone. Automatic tagging in Incoming has no Undo button.

A library from an earlier version still holds those copies. When CueRithm starts it deletes the ones whose write has settled, a few hundred writes at a time, and only files it created inside its own data folder.

## Why WMA uses TagLib { #taglib }

TagLib 2.3.2 is built statically with all other formats, shared libraries and tests disabled, and a small C++ bridge exposes only the staged-file ASF operations. One checked build-time modification keeps repeated ASF values together so multi-artist order is preserved. Its licences are retained in the vendored folder. You never install TagLib.

## Where you see it in the app { #where-you-see-it }

| Place | What shows |
|---|---|
| **Incoming** | *Preparing tags*, then *Writing tags*, under the track title. A failure shows a warning icon and the message |
| **Settings → Automatic tagging** | **Fields to write**, **Write Camelot keys**, **Write track energy to files**, and the **Recovery backups** size |
| **Cleanup** and the metadata editor | Preview, apply, history and Undo |

## Related

- [Tag new tracks from Beatport and Traxsource](../tagging/tag-new-tracks-automatically.md)
- [Undo a tag change](../tagging/tag-history.md)
- [How is the library stored?](library.md)
- [Where CueRithm keeps your data](../files/library/where-data-lives.md)

<!-- verified: crates/cuerithm-autotag/src/input.rs (audio_format from codec and container), write.rs (stage_prepared_cancellable: original.<ext>, staged.<ext>, hash checks, re-decode, PCM hash, required fields, ENERGY, CUERITHM_TAGGEDDATE, key and BPM fields incl. MP4 tmpo), id3_integrity.rs, vendor/onetagger/onetagger-tag (wav.rs INFO mirror; id3.rs v2.3 Latin-1), crates/cuerithm-library/src/tagging.rs (journal, durable assets, exchange, rollback, committed), publish_files.rs (renamex_np RENAME_SWAP, renameat2 RENAME_EXCHANGE, ReplaceFileW), artifacts.rs (safe_target; 'Later user edit preserved'), vendor/taglib/README.md, THIRD_PARTY_NOTICES.md, docs/automatic-tagging.md and docs/cleanup.md (no-op writes keep no backup; originals retained until a retention action), legacy about/roadmap.md (backup retention controls unfinished); legacy technical/tag-writing.md re-checked. Dropped from legacy: 'not symbolic links' (not confirmed) -->
