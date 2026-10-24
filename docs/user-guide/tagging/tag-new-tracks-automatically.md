# Tag new tracks from Beatport and Traxsource

<p class="lead">In about ten minutes you will switch on catalog tagging, add a few tracks and watch CueRithm fill in their tags from Beatport or Traxsource. The key, BPM, energy and cues stay exactly as CueRithm measured them.</p>

**You need:**

- [ ] CueRithm installed with an open library ([Download and install CueRithm](../get-started/install.md))
- [ ] An internet connection. Catalog lookups are the only part of CueRithm that uses one.
- [ ] Two or three music files to practise on, copied out of your purchase folder (for example from `~/Music/Beatport`)

!!! tip "Practise on copies"
    Tagging rewrites the tags inside your files. CueRithm checks that the audio is unchanged, but it keeps no copy of the original once the write is done and the app has no button that restores it. Copy a few files to a practice folder first. The examples below use `Man Like Me (Extended Mix)`.

## 1. Switch on automatic tagging { #switch-on }

Choose **Settings** at the bottom of the sidebar, then **Automatic tagging** in the list of settings pages. Tick **Automatic tagging** at the top.

<!-- figure:existing-12-settings-automatic-tagging-tag-new-tracks-automatically -->
<figure markdown>
![Automatic tagging settings](../assets/screens/12-settings-automatic-tagging.webp){ .screen loading=lazy }
<figcaption>Automatic tagging is switched on under Settings → Automatic tagging.</figcaption>
</figure>
<!-- /figure -->

Under the checkbox you read *Catalog lookups use an internet connection. Analysis and cue generation work offline.* Further down, the provider list shows **Beatport** as 1 and **Traxsource** as 2, both ticked. The other eight providers are unticked.

CueRithm ships with tagging off so a fresh install never contacts a catalog on its own. Analysis, playback and cues work with tagging off.

## 2. Check the providers { #providers }

Leave Beatport first and Traxsource second. With **Combine providers** off, CueRithm asks Beatport first and uses the first provider that finds a match; it only asks Traxsource if Beatport finds nothing or fails.

If **Traxsource** shows *Unavailable · configure a solver*, untick it for now. Traxsource sometimes asks visitors to pass a Cloudflare check, and you can set that up later in [Get past a Cloudflare block](cloudflare-solver.md). To use other providers, see [Choose providers and fields to write](choose-providers.md#providers).

## 3. Decide what happens without a match { #no-match }

Leave **No catalog match** on **Keep existing tags and finish**. A track the catalogs do not know then keeps its own tags and still moves into Tracks. Neither catalog covers every release, and CueRithm has not measured how many of your tracks they will find, so expect some misses. Choose **Keep in Incoming for attention** if you would rather look at every miss yourself.

A provider *error*, such as being offline, is never treated as "no match". The track waits in Incoming so you can retry it.

## 4. Check the fields to write { #fields }

Scroll to **Fields to write**. Out of the box CueRithm writes **Genre**, **BPM**, **Style**, **Label** and **Release date**. Title, artist, album, key, artwork, ISRC and identifiers are off, and a field that is off is never touched.

**Overwrite all selected fields** is on, which means the catalog's genre replaces the genre already in the file. Turn it off to fill only empty fields.

**BPM** here writes CueRithm's own measured value, because **Key and BPM written to files** is set to **Cuerithm analysis**. The catalog's BPM is not used unless you change that setting.

## 5. Save the settings { #save }

Choose **Save settings** at the top right. The button reads *Settings saved* for a moment.

Settings apply to tracks that arrive from now on, to watch-folder arrivals and to **Analyze again**. They do not change tracks already in Tracks. Saving also retries every track in Incoming that was waiting for attention.

## 6. Add the practice tracks { #add-tracks }

Choose **Add tracks** in the header and pick your practice files, or drag them onto the window. Open **Incoming** in the sidebar.

Each row shows the stage it is in: *Queued*, *Decoding*, *Analyzing*, *Matching catalog*, *Preparing tags*, *Writing tags* and *Applying cues*. *Matching catalog* is the step that goes online. For `Man Like Me (Extended Mix)` it sends Kevin de Vries as the first artist and a cleaned version of the title to Beatport, never the audio or the file path. [Choose providers and fields to write](choose-providers.md#what-is-sent) lists everything that leaves your computer.

When a track finishes it leaves Incoming and appears in **Tracks**.

## 7. Check what changed { #check }

In **Tracks**, look at the narrow status column left of the titles:

| Icon (hover text) | Meaning |
|---|---|
| Check mark (*Tagged from catalog match*) | A provider matched and its fields were written |
| Check mark (*Already tagged*) | **Skip previously tagged catalog lookups** is on and the file carried a marker |
| Tag icon (*No catalog match; existing tags kept*) | Nothing matched, and your policy kept the existing tags |
| Warning triangle (*Needs attention*) | The track is waiting in Incoming for you |

Select `Man Like Me (Extended Mix)` and choose **Edit metadata**. The **Release** section shows the label and release date now stored in the file. Under **Measured analysis** the key is still <span class="cr-key">4A</span>, the BPM still <span class="cr-bpm">128</span> and the energy still <span class="cr-energy">7/10</span>. Tagging never changes those library values. Choose **Back to tracks** when you are done.

## 8. Handle a track that needs attention { #attention }

A warning triangle means CueRithm stopped rather than guess. Typical reasons are two candidates whose similarity differs by one percentage point or less, a mix that differs from the version in the file, or a provider that could not be reached. Select the triangle to open **Review match**, or press the row's retry button once you are back online.

[Review and fix a wrong match](review-a-match.md) walks through the choices.

!!! info "How your files are protected"
    - CueRithm writes tags to a staged copy, never into your file in place.
    - It decodes that copy again and compares it with the original, frame for frame. A mismatch stops the write.
    - If you change the file in another program while CueRithm works, the write stops and the track is prepared again.
    - The original is kept only until the write has settled. **Settings → Automatic tagging → Recovery backups** shows the copies that exist while a write is under way.

    See [How are tags written safely?](../how-it-works/tag-writing.md) for the full sequence.

## What you learned { #learned }

- Catalog tagging is off until you tick **Automatic tagging** and choose **Save settings**.
- Beatport and Traxsource are the default providers, and the first one with a match wins unless you turn on **Combine providers**.
- Only the fields you tick under **Fields to write** are touched, and **Overwrite all selected fields** decides whether existing values are replaced.
- Measured key, BPM, energy and cues are never replaced by catalog data.
- A track that CueRithm cannot match with confidence waits for you in Incoming.

## Next steps { #next }

- [Choose providers and fields to write](choose-providers.md): add providers, tune matching and decide which fields may be overwritten.
- [Review and fix a wrong match](review-a-match.md): choose between candidates and keep or replace each field.
- [Edit tags and artwork](edit-tags-by-hand.md): correct a tag yourself when no catalog has it.
- [Keep a folder like Beatport in your library](../files/watch-folders.md): tag every purchase automatically when it lands.

<!-- verified: apps/desktop/src/views/TaggingSettings.tsx (labels, defaults); apps/desktop/src/test/tagging-catalog.json (default tags, platforms, overwrite, no_match); apps/desktop/src/components/TrackTable.tsx (stage labels, status icon titles); apps/desktop/src/App.tsx (toolbar, Add tracks); apps/desktop/src-tauri/src/commands.rs save_settings (retry_attention); crates/cuerithm-autotag/src/lib.rs match_track_with_album (first provider wins, ties 0.01, errors); crates/cuerithm-autotag/src/write.rs (measured BPM, staged decode check); crates/cuerithm-library/src/prepare.rs (superseded retry); legacy learn/auto-tagging.md re-checked -->
