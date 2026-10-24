# How does cleanup decide what to change?

<p class="lead">The cleanup engine is a pure function: it takes a track's metadata and your saved rules and returns proposed new metadata. It reads and writes no files itself. The library service around it handles scanning, previews, backups and Undo.</p>

<!-- figure:existing-72-cleanup-preview-how-it-works-cleanup-engine -->
<figure markdown>
![Cleanup preview: 10 proposed changes, 7 unchanged, 0 unavailable](../assets/screens/72-cleanup-preview.webp){ .screen loading=lazy }
<figcaption>What the cleanup engine proposes, before anything is written.</figcaption>
</figure>
<!-- /figure -->

!!! abstract "What this means for you"
    - **Scanning changes nothing.** The preview lists every proposed change, and you can untick any of them before you apply.
    - **Same rules, same result.** The engine has no hidden state, and rules run in the order you list them.
    - **Cleanup touches human-readable fields only**: title, artist, album, album artist, genre, comment, grouping, label, remixer, mix, producer, composer and lyricist. It never changes file paths, audio or analysis.
    - **Protect and ignore lists win.** A protected genre or artist, or an ignored value on a rule, is left exactly as it is.
    - To write rules, see [Write cleanup rules](../tagging/cleanup-rules.md). To try them safely, follow [Clean up messy titles, genres and artists](../tagging/cleanup-messy-titles.md).

## Design rules { #design-rules }

- Rules are deterministic. The engine takes your rules and a track's metadata and nothing else.
- The [eleven Smart Fixes](#smart-fixes) use built-in algorithms. User-written regular expressions are available only in smart playlists and in the optional tagging **Title cleanup pattern**.
- A value that a fix reduces to nothing is removed, for example a title made only of punctuation. The preview shows it as a change to an empty value, so untick it if you disagree. A title that is only digits is left alone by **Number prefix**.
- Text fixes skip ignored values, and skip values that are protected or named in an active genre or artist mapping, in their own roles.
- Cleanup changes metadata fields only. It never changes paths, audio, key, BPM, energy, beatgrid or cues.

## The eleven Smart Fixes { #smart-fixes }

None is enabled until you add it. Each has an **Enabled** switch and an **Ignored values** list.

| Smart Fix | What it does |
|---|---|
| **Casing** | Capitalises values that are entirely upper-case or entirely lower-case in the chosen field (each direction is a switch). Mixed-case values are left alone |
| **Characters to spaces** | Replaces characters you list, such as `_`, with spaces |
| **Encoded characters** | Decodes HTML entities such as `&amp;` |
| **Extract artist** | When Artist is empty, takes it from the title using a separator (default ` - `) and a result number |
| **Extract remixer** | When Remixer is empty, takes it from a title ending in a word such as "remix", optionally removing it from the title |
| **Garbage** | Removes empty brackets, stray edge punctuation and repeated spaces |
| **Number prefix** | Removes a leading track number such as `01 - ` |
| **URLs and email** | Removes web and email addresses, or clears the whole field |
| **Mix parentheses** | Moves a trailing mix name, such as `- Extended Mix` or `Original Mix`, into parentheses |
| **Common title text** | Removes a Camelot key, key and energy, BPM or "Original Mix" from the title |
| **Promotional text** | Removes phrases such as "download from" and store or site names |

## In what order rules run { #order }

```text
enabled Smart Fixes (in list order) → genre conversion and mapping → artist mapping
```

Genre mapping follows chains to the end, rejects cycles, and collapses duplicates. A mapping to nothing deletes the value. Artist mapping covers only the roles you tick: artist, remixer, producer, composer and lyricist.

## Scan, apply, undo { #scan-apply-undo }

1. **Scan** reads the files in scope and runs the engine. It writes nothing. The preview lists proposals and the counts of unchanged and unavailable tracks. The scope is **All ready tracks** or the selection you opened Cleanup with.
2. **Apply** stages each changed file, writes the new tags there, reads them back and checks they equal what was intended. It then publishes with the same atomic, journaled swap as [tag writing](tag-writing.md), together with the library metadata. Each track is independent: a conflict or failure on one does not stop the others.
3. **Undo** writes back the tags and metadata recorded before the apply when the file still matches what the apply produced. If it does not, your newer edit is kept and the track is reported as a conflict. See [Undo a cleanup](../tagging/undo-cleanup.md).

A value that a tag format cannot hold is reported per track as a failure, not truncated.

## What happens to converted genres { #custom-tags }

Genre conversion can turn genres into categorised tags (default category *Subgenres*). Those live in the CueRithm library, not in file tags. See [Group tracks with categories and custom tags](../tagging/curate-tracks.md).

## What limits apply { #validation }

The same limits are enforced in the interface and in the backend. A configuration may hold at most 64 fixes, 4,096 mappings, 4,096 ignored values and 4,096 protected values, each value at most 4,096 bytes. A track may carry at most 128 values per field and 1 MiB of metadata in total. Errors appear in a banner before you can save.

## Automatic mode { #automatic-mode }

With **Clean incoming tracks automatically**, the engine's output joins the same staged write plan as catalog tags, after tagging and before the track leaves Incoming. Cleanup never contacts a provider. It runs without a preview, so test your rules with a manual scan first. See [Run cleanup on every new track](../tagging/cleanup-automatically.md).

## Where you see it in the app { #where-you-see-it }

| Place | What shows |
|---|---|
| **Cleanup** in the sidebar | **Smart Fixes**, **Genres** and **Artists** tabs, **Scan tracks**, the list of proposed changes, and **Undo history** |
| **Settings → Cleanup** | The same rules, and the switch for automatic cleanup |
| **Incoming** | The cleanup stage while a track is being prepared |

If cleanup changed more than you expected, see [Cleanup changed too much](../help/troubleshooting/cleanup-changed-too-much.md).

## Related

- [Write cleanup rules](../tagging/cleanup-rules.md)
- [Clean up messy titles, genres and artists](../tagging/cleanup-messy-titles.md)
- [Undo a cleanup](../tagging/undo-cleanup.md)
- [How are tags written safely?](tag-writing.md)

<!-- verified: crates/cuerithm-cleanup/src/lib.rs (order), model.rs (11 SmartFix kinds, validation limits), mappings.rs (chains, cycles, unique, None deletes), fixes/mod.rs (ignored and protected values; a fix that returns an empty string drops the value; verified by running the engine on '--' with Remove garbage), fixes/remove.rs (number_prefix digits-only guard, common patterns, promotional), fixes/extract.rs (mix_parentheses), fixes/text.rs; crates/cuerithm-library/src/cleanup/*, docs/cleanup.md (scan, apply, undo), prepare.rs (cleanup stage after tagging), apps/desktop CleanupSettings.tsx (template names, Clean incoming tracks automatically, default category Subgenres), Cleanup.tsx (tabs, Scan tracks, All ready tracks, Undo history); legacy technical/cleanup-engine.md re-checked. Changed from legacy: 'a fix that would empty a field leaves it unchanged' does not match the engine (the value is removed) -->
