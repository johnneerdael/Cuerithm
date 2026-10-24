# Write cleanup rules

<p class="lead">Build the rule set that tidies titles, genres and artists: the eleven Smart Fixes, genre and artist mappings, what runs in which order, and the limits. Plan on five minutes per rule, plus a preview.</p>

!!! tip "New to this?"
    Follow [Clean up messy titles, genres and artists](cleanup-messy-titles.md) first. It walks through one rule from scan to undo. Worked rules for common problems are in the [recipes](recipes/index.md).

Cleanup has one saved rule set. You edit it in **Cleanup** (sidebar, under **TOOLS**) and also in **Settings → Cleanup**; both places change the same rules, and [automatic cleanup](cleanup-automatically.md) uses them too. In **Cleanup**, press **Save cleanup rules**. In Settings, press **Save settings**. A scan is blocked while there are unsaved rule changes.

## What a rule can change { #what-can-change }

Rules edit human-readable tags only. A rule can touch these thirteen fields: title, artist, album, album artist, genre, comment, grouping, label, mix, remixer, producer, composer and lyricist. Rules never touch paths, audio, key, BPM, energy, beatgrid or cues.

## Save, scan, apply { #the-flow }

1. Edit the rules and press **Save cleanup rules**.
2. Choose the **Scope** (**All ready tracks** or **N selected tracks**) and press **Scan tracks**. Only Ready tracks that are not archived are in scope, and a selection can hold up to 100,000 tracks.
3. Read the **N proposed changes**. Each track shows the fields that change, before and after. Untick the tracks you do not want.
4. Press **Apply N changes**. The result reads "A applied · U unchanged · C conflicts · F failed".
5. Use **Undo** in the history if needed: [Undo a cleanup](undo-cleanup.md).

Editing a rule or the scope discards the old preview.

## Smart Fixes { #smart-fixes }

Smart Fixes are an ordered list of up to 64 rules. None are installed for you. Choose a fix in **New Smart Fix** and press **Add Smart Fix**. Every card has:

<!-- figure:existing-71-cleanup-smart-fixes-2-cleanup-rules -->
<figure markdown>
![Cleanup on the Smart Fixes tab with two rules](../assets/screens/71-cleanup-smart-fixes.webp){ .screen loading=lazy }
<figcaption>Smart Fixes run in the order shown.</figcaption>
</figure>
<!-- /figure -->

- **Enabled**: only enabled rules run.
- The rule's number, **Move up**, **Move down** and **Remove rule**.
- The fix's own options, below.
- **Ignored values, one per line**: whole values that this rule must skip ([details](#ignored-values)).

Four fixes also have a **Field** menu: title, artist, album, album artist, genre, comment, grouping, label, remixer, mix, producer, composer or lyricist. The other seven work on the title (Extract artist and Extract remixer also write to Artist and Remixer).

### Skip a value with Ignored values { #ignored-values }

Type a value on its own line and that rule leaves it alone. A value is skipped only when it equals a line exactly, capitals and spaces included. Each rule has its own list, so you can protect `12 Monkeys` from **Number prefix** and still let other rules change it. Blank lines are dropped, and repeated lines count once.

The examples below use the demo tracks where the library has a matching case, and plain placeholders such as `Signal` where it does not.

### Casing { #fix-casing }

Options: **Field** (default title), **Uppercase values** and **Lowercase values** (both on).

Converts a value that is written entirely in capitals, or entirely in lower case, to capitalised words. A value in mixed case is left alone, so `WITH ME (Extended Mix)` stays as it is. Every word gets a capital, including `and` and `the`. `dj`, `mc` and `va` become `DJ`, `MC` and `VA`, and `vs` stays lower case.

| Before | After |
|---|---|
| `SIGNAL AND SOUND` | `Signal And Sound` |
| `BRACKISH (ORIGINAL MIX)` | `Brackish (Original Mix)` |
| `signal and sound (extended mix)` | `Signal And Sound (Extended Mix)` |
| `DJ KOZE` | `DJ Koze` |

!!! warning "Gotcha"
    Stylised names look like shouting to this rule. Used on the artist field, `SOLANCE` and `SLVR` become `Solance` and `Slvr`, `AC/DC` becomes `Ac/Dc` and `K.ONE` becomes `k.one`. Add such names to **Ignored values**; [Fix capital letters without breaking artist names](recipes/fix-capital-letters.md) shows how.

### Characters to spaces { #fix-characters-to-spaces }

Options: **Characters** (default `_`, up to 64 characters). Works on the title.

Replaces each listed character with a space and trims the edges. Two characters in a row leave two spaces, so follow it with [Garbage](#fix-garbage).

| Before | After |
|---|---|
| `Signal_Track_(Extended_Mix)` | `Signal Track (Extended Mix)` |
| `Signal__Track` | `Signal  Track` (two spaces) |

### Encoded characters { #fix-encoded-characters }

No options. Works on the title. Turns HTML-style codes that a website left behind into the real characters, including doubly encoded ones.

| Before | After |
|---|---|
| `Signal &amp; Sound` | `Signal & Sound` |
| `Signal &amp;amp; Sound` | `Signal & Sound` |
| `Vai N&atilde;o Pare` | `Vai Não Pare` |

A code CueRithm does not know stays as typed.

### Extract artist { #fix-extract-artist }

Options: **Separator** (default ` - `, 1 to 64 characters) and **Result number** (1 to 256, default 1). Works on the title and fills Artist.

It acts only when the track has **no artist at all**. It splits the title at the separator, uses the numbered piece as the artist and keeps the rest as the title. With the default separator, a spaced en dash ( – ) counts too, an em dash does not. A title without the separator is left alone.

| Title | Result number | Artist | Title after |
|---|---|---|---|
| `Example Artist - Example Title (Extended Mix)` | 1 | `Example Artist` | `Example Title (Extended Mix)` |
| `Example Title (Extended Mix) - Example Artist` | 2 | `Example Artist` | `Example Title (Extended Mix)` |
| `Prefix - Example Artist - Example Title` | 2 | `Example Artist` | `Example Title` |

[Fill in a missing artist from the title](recipes/fill-a-missing-artist-from-the-title.md) has the full steps.

### Extract remixer { #fix-extract-remixer }

Options: **Remixer suffix** (default `remix`, 1 to 64 characters, not only spaces) and **Remove remixer from title** (on).

It acts only when the track has no remixer. It looks for a bracketed name that ends in the suffix, such as `(Clara Remix)`, and moves the name into Remixer. Plain mix names such as `(Original Mix)`, `(Extended Remix)` and `(Remix)` are not treated as a remixer.

| Before | After |
|---|---|
| `Signal (Clara Remix)` | title `Signal`, remixer `Clara` |
| `Signal (Clara Club Remix)` | title `Signal`, remixer `Clara` |
| `Signal (Clara & Alex Remix)` | title `Signal`, remixer `Clara & Alex` |
| `PLV (Creeds remix - Extended)` | unchanged: the suffix must sit right before the closing bracket |

Change the suffix to `edit` to catch `(Clara Edit)`.

### Garbage { #fix-garbage }

Options: **Field** (default title).

Trims dashes, underscores, bars and unmatched brackets from the ends, removes empty brackets and squeezes repeated spaces. Running it twice changes nothing the second time, and it keeps real punctuation.

| Before | After |
|---|---|
| `Signal  Sound` | `Signal Sound` |
| `Signal () Sound` | `Signal Sound` |
| `--- Signal --` | `Signal` |
| `Signal [` | `Signal` |
| `C++`, `(Real Version)`, `F*ck The Club (Extended Mix)` | unchanged |

### Number prefix { #fix-number-prefix }

No options. Works on the title. Removes a track number from the front when a separator follows it.

| Before | After |
|---|---|
| `01. Signal` | `Signal` |
| `02 - Signal` | `Signal` |
| `(03) Signal` | `Signal` |
| `17 (Extended Mix)`, `2020Vision`, `Signal 01`, `12345` | unchanged |

### URLs and email { #fix-urls-and-email }

Options: **Field** (default title) and **Clear entire matching field** (off).

Removes web addresses (`https://…`, `www.…`), email addresses and bare domains such as `beatport.com`. File names that end in an audio extension, like `Signal.flac`, are kept. With **Clear entire matching field** on, a value that contains any address is removed completely instead of trimmed.

| Field | Before | Clear field | After |
|---|---|---|---|
| title | `Signal https://example.com` | off | `Signal` |
| comment | `Downloaded from https://example.com/store` | off | `Downloaded from` |
| comment | `Downloaded from https://example.com/store` | on | (empty) |

### Mix parentheses { #fix-mix-parentheses }

No options. Works on the title. Wraps a trailing mix description in brackets: everything after the first ` - `, or a trailing `Original Mix`, `Extended Mix`, `Club Mix` or `Radio Edit` that has no brackets yet. A title that already has its brackets is left alone.

| Before | After |
|---|---|
| `Signal - Extended Mix` | `Signal (Extended Mix)` |
| `Atoma Extended Mix` | `Atoma (Extended Mix)` |
| `Signal - Radio Edit` | `Signal (Radio Edit)` |
| `Signal (Extended Mix)` | unchanged |

!!! warning "Gotcha"
    The fix assumes the first ` - ` separates the title from the mix. If that dash separates an artist from a title, the result is wrong. The demo library's `Nivk, Rabteu - PLV (Creeds remix - Extended)` would become `Nivk, Rabteu (PLV (Creeds remix - Extended)`. Leave the fix off for titles like that, or untick those proposals in the preview.

### Common title text { #fix-common-title-text }

Option: **Pattern**. Works on the title. Add one card per pattern you need.

| Pattern | Removes | Before → after |
|---|---|---|
| **Original Mix suffix** (default) | a trailing `Original Mix`, `(Original Mix)`, `- Original Mix` or `Original`, in any capitalisation | `Brackish (Original Mix)` → `Brackish` |
| **Camelot key** | a key at the front (`8A - `, `12B `) or at the end (` - 9A`), keys 1A to 12B only | `2A - Atoma (Extended Mix)` → `Atoma (Extended Mix)` |
| **Camelot key and energy** | a key and a number of one to three digits in front: `8A - 7 - ` | `11A - 6 - Palm Of Your Hands (Extended Mix)` → `Palm Of Your Hands (Extended Mix)` |
| **BPM suffix** | a trailing number of two or more digits that does not start with 0 | `Signal 128` → `Signal` |

`Extended Mix` is not touched by any pattern: `Atoma (Extended Mix)` and `17 (Extended Mix)` stay as they are. `Signal (Original Remix)` stays too, and so does a title that consists of only `Original Mix`.

!!! warning "Gotcha"
    **BPM suffix** cannot tell a tempo from any other number. `Studio 54` becomes `Studio`. Put such titles in **Ignored values**. See [Strip the key and BPM from titles](recipes/strip-key-and-bpm-from-titles.md).

### Promotional text { #fix-promotional-text }

Options: **Field** (default title).

Removes bracketed promotions such as `[FREE DOWNLOAD]` and `(OUT NOW)`, a leading `PREMIERE -`, and tails such as `purchased from …`, `available on …`, `© 2024 …` or a web address.

| Before | After |
|---|---|
| `Signal [FREE DOWNLOAD]` | `Signal` |
| `PREMIERE - Signal` | `Signal` |
| `Signal purchased from Store` | `Signal` |
| `Signal (Free Download) (Extended Mix)` | `Signal (Extended Mix)` |
| `Signal free spirit`, `Signal - Support the artist` | unchanged |

The word list is broad. A bracket that starts with a promotional word is removed even when it is a real mix name, so `(Free Spirit Remix)`, `(Exclusive Mix)` and `(Promo Mix)` all disappear, and `Signal Demo` loses `Demo`. Always read the preview; [Remove promo text and web addresses](recipes/remove-promo-text-and-urls.md) shows how to protect real titles.

!!! note "A fix can remove a whole value"
    If a fix strips every character of a value, the value is removed. A title of `(( ))`, `www.example.com` or `Demo Tape` would be left empty. The preview shows such a result as **Empty**; untick that track.

## Order of application { #order-of-application }

CueRithm applies the rules in one fixed order:

1. The enabled Smart Fixes, top to bottom. Each rule sees the result of the one above it.
2. Genre conversion and genre mapping.
3. Artist mapping.

Order matters. With `Signal__Track_`, **Characters to spaces** followed by **Garbage** gives `Signal Track`, but Garbage followed by Characters to spaces gives `Signal  Track` with two spaces. Use **Move up** and **Move down** to fix it.

A text fix also leaves a genre or artist alone when the value is on the **Protected** list, or when it is the source or target of a mapping that is switched on. Genre and artist mappings therefore win over, say, a Casing rule.

## Genres { #genres }

Open the **Genres** tab and tick **Enable genre cleanup**.

- **Mappings.** Each row maps an **Exact source value** to a **Canonical value**. The match is exact and case sensitive: `dubstep` does not match `Dubstep`. Tick **Remove** to delete a value instead. Press **Add mapping** for a new row, or type in the box on a library value's row (**Replace … with**), which creates the mapping for you.
- **Chains and duplicates.** `Deep Dubstep` → `Dubstep` → `Bass Music` ends at `Bass Music`. A loop is refused with "Genre cleanup mappings contain a cycle. Change a target to end the chain." If two genres end up identical on one track, only one is kept.
- **Protected genres, one per line.** These are never changed by a mapping or a text fix.
- **Convert genres into categorized tags.** Choose a **Category** (default `Subgenres`) and list the **Genres to convert**. Each converted genre becomes a category choice on the track, and CueRithm creates the category if you do not have it yet. Tick **Remove converted values from Genre** to take them out of the file's genre as well; otherwise the genre stays, and any mapping still applies. Categories live in your library, not in the file's tags. See [Keep Beatport's detailed genre as a category](recipes/keep-subgenres-as-tags.md) and [Group tracks with categories](curate-tracks.md#create-categories).
- **Library values.** The list shows every genre in your Ready tracks with its track count. Use **Search genres**, sort by **Name** or **Track count**, and use **Protect** to add a value to the protected list. **Refresh values** rereads the library. The list appears in **Cleanup** only; in Settings you type mappings by hand.

If a track's only genre is removed or converted away, its Genre field ends up empty.

## Artists { #artists }

Open the **Artists** tab and tick **Enable artist cleanup**. Choose the contributor fields it covers: **artist** (the default), **remixer**, **producer**, **composer** and **lyricist**. Keep at least one ticked. Album artist is not one of them.

Mappings, **Protected artists, one per line** and the library list work as they do for genres, with counts per role such as "artist: 2 · producer: 1". Three things to know:

- A mapping matches a whole value. If the file stores a credit as a single value like `Stylo, Anyma (ofc)`, mapping `Anyma (ofc)` does not change it; map the exact text of that value instead.
- CueRithm never merges similar spellings on its own, and it cannot split one value into several.
- **Remove** deletes a name, for example `Various Artists`, from the roles you ticked.

[Rename an artist everywhere](recipes/rename-an-artist-everywhere.md) has a worked example.

## What apply guarantees { #what-apply-guarantees }

| | Scan tracks | Apply | Undo |
|---|---|---|---|
| File tags | Read only | Rewritten through a staged copy; original kept | Original restored |
| Title, artist, album, genre in the library | Not touched | Updated with the file | Restored |
| Category choices (genre conversion) | Not touched | Added, in the library only | Restored |
| Audio, key, BPM, beatgrid, energy, cues | Not touched | Not touched | Not touched |

- For each track, CueRithm writes the new tags into a staged copy of the file, decodes the copy to confirm the audio is identical, reads the tags back, and only then swaps the copy in. The tags it replaced are recorded for **Undo**, and a copy of the file is kept only while the write is under way (**Settings → Automatic tagging → Recovery backups** shows it).
- The library is updated in the same step: title, artist, album and genre, and any categorised tags.
- The audio, key, BPM, beatgrid, energy and existing cues are preserved. Apply does no new analysis.
- A file whose tags would not change is not rewritten. A change that only adds category choices touches no file at all.
- Each track succeeds or fails alone. A file that changed since the scan, a track edited in the meantime, or a value a tag format cannot hold is reported for that track, and the rest finish. Conflicted tracks need a fresh scan.

[How cleanup works](../how-it-works/cleanup-engine.md) and [How tags are written safely](../how-it-works/tag-writing.md) explain the engine and the write.

## Limits { #limits }

| Item | Limit |
|---|---|
| Smart Fixes | 64 ("Use at most 64 Smart Fixes.") |
| **Characters**, **Separator**, **Remixer suffix** | 64 characters each |
| **Result number** | a whole number from 1 to 256 |
| Mappings per tab | 4,096, no duplicate sources, no loops |
| Ignored and protected values | 4,096 each; each non-blank and at most 4,096 bytes |
| Canonical value, source value | at most 4,096 bytes |
| Genres to convert | 4,096; category name at most 256 bytes |
| Per track | 128 values in one field, 1 MiB of text in all, 4,096 categorised tags |

The editor shows a red banner naming the problem and disables **Save cleanup rules** or **Save settings** until you fix it.

## What can go wrong { #what-can-go-wrong }

| Symptom | Cause | Fix |
|---|---|---|
| **Scan tracks** is grey | Unsaved rule changes ("Save rule changes before scanning."), an error banner, or **N selected tracks** with nothing selected | Press **Save cleanup rules**, fix the banner, or switch **Scope** to **All ready tracks** |
| "N tracks unavailable" or a failure such as "Source changed; rescan the track" | The file is missing, moved or edited since CueRithm last read it | [Find missing, moved or broken files](../files/library/find-missing-files.md), then scan again |
| The preview vanished | You edited a rule or the scope | Scan again |
| A mapping does nothing | The match is exact; the credit is joined into one value; the role is not ticked; the value is protected | Compare the text in the library list, tick the role, remove the protection |
| A track shows **Empty** after | A fix removed every character | Untick the track, or add its title to **Ignored values** |
| "conflicts" in the result | A track changed between scan and apply | Scan again; see [Undo a cleanup](undo-cleanup.md) |

Still unhappy with a batch? Read [Cleanup changed too much](../help/troubleshooting/cleanup-changed-too-much.md).

## Related

- [Clean up messy titles, genres and artists](cleanup-messy-titles.md)
- [Run cleanup on every new track](cleanup-automatically.md)
- [Undo a cleanup](undo-cleanup.md)
- [Tag and clean-up recipes](recipes/index.md)
- [Cleanup settings](../reference/settings/cleanup.md)

<!-- verified: apps/desktop/src/views/CleanupSettings.tsx; apps/desktop/src/views/Cleanup.tsx; apps/desktop/src/cleanup-validation.ts; crates/cuerithm-cleanup/src/{lib,model,mappings}.rs and fixes/*.rs (all examples run through the engine, 2026-10); crates/cuerithm-cleanup/tests/fixtures/lexicon-v1.json; crates/cuerithm-library/src/cleanup/*.rs; docs/cleanup.md; legacy guides/cleanup-rules.md re-checked. Differences from legacy: a fix CAN empty a value (legacy said it leaves the field unchanged); album artist is not an Artist cleanup role. -->
