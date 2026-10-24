# Set up automatic tagging

<p class="lead">The Automatic tagging section decides whether new tracks are looked up on Beatport, Traxsource and other catalogs, which providers are asked, which tags are written and what happens when nothing matches. This page lists every control with its default. Allow ten minutes to read it once; later you will only look things up.</p>

<!-- figure:existing-12-settings-automatic-tagging-reference-settings-automatic-tagging -->
<figure markdown>
![Automatic tagging settings](../../assets/screens/12-settings-automatic-tagging.webp){ .screen loading=lazy }
<figcaption>The Automatic tagging section of Settings.</figcaption>
</figure>
<!-- /figure -->

!!! tip "New to this?"
    Follow [Tag new tracks from Beatport and Traxsource](../../tagging/tag-new-tracks-automatically.md) first. It turns tagging on for a real folder and shows the result. For the reasoning behind provider and field choices, see [Choose providers and fields to write](../../tagging/choose-providers.md).

Open **Settings → Automatic tagging** and press **Save settings** when you finish. Changes apply to new tracks, watch-folder arrivals and **Analyze again**. They never retag tracks that are already finished. Catalog lookups need the internet. Analysis and cue generation work offline either way.

## Turn tagging on and choose providers { #master }

| Control | What it does | Default | Valid values | Task page |
|---|---|---|---|---|
| **Automatic tagging** | Master switch. When off, CueRithm never writes catalog tags and never contacts a provider while preparing. | Off | On or off | [Tag new tracks from Beatport and Traxsource](../../tagging/tag-new-tracks-automatically.md) |
| **Recovery backups** | Read-only. The size and number of original files CueRithm holds while a tag write is under way; normally 0.0 MB · 0 files. If cleanup work is unfinished, a second line shows "Pending cleanup" and the size of temporary files kept. | n/a | Not editable | [Undo a tag change](../../tagging/tag-history.md) |
| **Catalog providers** | The ten sources, in the order they are asked. Selected providers come first, numbered 1, 2, 3; the rest show a dash. | Beatport (1) and Traxsource (2) selected | Any number of the ten providers | [Choose providers and fields to write](../../tagging/choose-providers.md) |
| **Use** *provider* | Adds or removes a provider. Providers that need an account stay disabled until it is set up. | Beatport, Traxsource | Checked or not | [Choose providers and fields to write](../../tagging/choose-providers.md) |
| **Move** *provider* **earlier** / **later** | Arrows that change the order. Only selected providers can move. | n/a | n/a | [Choose providers and fields to write](../../tagging/choose-providers.md) |
| **Combine providers** | Off: the first provider with a match is used. On: matches from every selected provider are applied, in order. | Off | On or off | [Choose providers and fields to write](../../tagging/choose-providers.md) |
| **No catalog match** | What happens to a track no provider could match. | **Keep existing tags and finish** | **Keep existing tags and finish**; **Keep in Incoming for attention** | [Identify a track with no match](../../tagging/identify-unknown-tracks.md) |

A provider error is different from "no match". If a provider fails, rejects a key or runs out of allowance, the track stays in Incoming so it can be retried ([Catalog tagging finds nothing](../../help/troubleshooting/no-catalog-match.md)).

## The ten providers { #providers }

| Provider | Default | Account | Notes |
|---|---|---|---|
| **Beatport** | Selected, 1st | None | Has its own settings below ([Beatport settings](#beatport)) |
| **Traxsource** | Selected, 2nd | None | If a site answers with a Cloudflare check, it is flagged as blocked and needs a [solver](#solver) |
| **MusicBrainz** | Off | None | "Public metadata · no account required" |
| **SonoVault** | Off | Personal API key | Needs a verified email. Supplies Title, Artist, Album, Label, Genre, Release date, Duration and ISRC. Cannot be selected until its key is saved. |
| **Discogs** | Off | Optional personal token | Anonymous use is limited to 25 requests a minute; a verified token allows up to 60 |
| **Volumo** | Off | Personal API key | Looks up an existing ISRC; it is not a full catalog search. Cannot be selected until its key is saved. |
| **iTunes** | Off | None | |
| **BPM Supreme** | Off | Account email and password | Cannot be selected until the account is saved |
| **Bandcamp** | Off | None | |
| **Deezer** | Off | None | |

Each provider that takes a key or account has a **Set up** *provider* button (**Manage** once something is saved). Opening it shows the controls below. They act on your system credential store at once and are separate from **Save settings**. Saving a key does not select the provider.

| Control | What it does |
|---|---|
| *Provider* **API key**, **personal token**, or **email** and **password** | Masked entry. The text is cleared when you leave and is never shown again. |
| **Save** *provider* **key** / **token** / **account** | Stores the credential |
| **Replace** *provider* **key** / **account** | Opens the entry again; **Cancel replacement** backs out |
| **Test saved** *provider* **connection** | Checks the saved credential. SonoVault uses one metadata credit; no audio is uploaded. Volumo checks one sample ISRC. |
| **Remove** *provider* **key** / **account** | Deletes the saved credential |
| **Cancel provider operation** | Stops a test or save in progress |

The line next to each provider tells you its state:

| Line | Meaning |
|---|---|
| "Public metadata · no account required" | Usable now |
| "Anonymous · 25 requests/minute" | Discogs without a token |
| "Optional · setup required" | Needs a key or account first |
| "Saved · not tested" / "Connection verified" | A credential is stored; verified after a successful test |
| "Credential rejected" | The provider refused it. Replace it. |
| "Verify your email before use" | SonoVault account email not yet verified |
| "Account access restricted" | The provider restricted the account |
| "Quota exhausted" / "Rate or quota limit reached" | Wait; the retry time is shown when the provider gives one |
| "Credential change needs attention" or "Credential store · *state*" | The system credential store could not complete the last change |
| "Unavailable · configure a solver" | The source is blocked by a Cloudflare check |
| "Uses configured solver" | The source goes through your solver |
| "One request at a time" / "Uses tagging concurrency" | How CueRithm paces requests to that provider |

Step by step: [Connect provider accounts](../../tagging/provider-accounts.md).

## Identify unresolved recordings { #fingerprint }

A collapsed group, off by default. When a readable track still has no catalog match, CueRithm can make an audio fingerprint on your computer and ask AcoustID for candidates, then MusicBrainz for details. No audio file is uploaded. Results always wait in Incoming for [edition review](../../tagging/review-a-match.md).

| Control | What it does | Default |
|---|---|---|
| **Allow fingerprint lookup** | Consent to send a compressed fingerprint and the track's length to AcoustID. The consent stays on this computer and is left out of exported presets. | Off |
| **Identify unresolved tracks** | Turns the fallback on, with one lookup per preparation attempt. Available only after consent. | Off |

If this build lacks its AcoustID application identity, the group says so and both boxes stay unavailable. Key, BPM, energy and cues keep their measured values. More: [Identify a track with no match](../../tagging/identify-unknown-tracks.md).

## Cloudflare challenge solver { #solver }

Optional. Only sources that were detected as blocked use it, and it receives their catalog addresses and search terms. Beatport keeps its direct connection.

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Solver API URL** | Address of a FlareSolverr, Byparr or Trawl server: the server address or its `/v1` endpoint. Empty turns the solver off. | Empty | A URL, for example `http://localhost:8191` |
| **Solver timeout** | Seconds allowed for each attempt. Temporary failures are retried once. | `120` | A whole number from `1` to `300` |
| **Retry direct source access** | Forgets which sources were detected as blocked, so CueRithm tries them directly again. Reads "Resetting source access…" while it works. | n/a | n/a |

Detected restrictions are remembered across restarts. Setup: [Get past a Cloudflare block](../../tagging/cloudflare-solver.md).

## Matching { #matching }

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Matching strictness** | How closely title and artist must agree. Higher is stricter. | `0.7` | `0` to `1`, in steps of `0.01` |
| **Tagging concurrency** | How many tracks are tagged at once, shared with analysis workers. Provider limits still apply. | `16` | `1` to `64` |
| **Match duration** | Compare the track's length with the catalog's | Off | On or off |
| **Maximum duration difference** | Seconds of difference allowed when duration matching is on | `30` | `0` to `600` |
| **Match by embedded IDs** | Use provider IDs and ISRC already in the file | Off | On or off |
| **Multiple catalog matches** | Which result to take when several fit | **Best match** | **Best match**; **Oldest release**; **Newest release** |
| **Include subfolders** | Watch and prepare files inside subfolders of watch folders and folders you add | On | On or off |
| **Read artist and title from filename** | Reads the artist and title from the file name, using the **Filename template** | Off | On or off |
| **Filename template** | Pattern used when the switch above is on. `%artists%`, `%artist%` and `%title%` match those parts; other `%words%` match unused parts. | `%artists% - %title%` | A pattern; it is checked when the switch above is on |
| **Title cleanup pattern** | A regular expression removed from the title before matching. Uses Rust `regex` syntax. | Empty | A valid pattern |
| **Skip previously tagged catalog lookups** | Skip the catalog step for tracks carrying a OneTagger or CueRithm marker. Analysis and new cues still run. | Off | On or off |
| **Album matching** | When one Beatport or Traxsource release is identified, reuse it for the remaining incoming files in the same folder | Off | On or off |
| **Album matching ratio** | Share of the folder's incoming files that must match one release before its track list is reused | `0.5` | `0` to `1`, in steps of `0.05` |

How the choices play out: [Match a folder as a release](../../tagging/match-a-release.md) and [How does catalog matching work?](../../how-it-works/matching.md).

## Fields to write { #fields }

**Overwrite all selected fields** and a grid with one row per field. For each field, **Write** decides whether the catalog value is written, and **Overwrite** decides whether it replaces a value already in the file. The small grey line under each field name lists which of your selected providers supply it.

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Overwrite all selected fields** | When on, every written field replaces an existing value and the **Overwrite** boxes are checked and locked. Turn it off to keep existing values, with exceptions per field. | On | On or off |
| **Write** *field* | Writes that field | Genre, BPM, Style, Label, Release date | Checked or not |
| **Overwrite** *field* | Replaces an existing value of that field. Enabled only when **Write** is on and **Overwrite all selected fields** is off. | Follows the master switch | Checked or not |
| **Key and BPM written to files** | Whether the Key and BPM fields hold your measured values or the catalog's. The library always keeps its own measurement. | **Cuerithm analysis** | **Cuerithm analysis**; **Catalog metadata** |
| **Write Camelot keys** | Writes keys in Camelot notation | Off | On or off |
| **Write track energy to files** | Writes the single whole-track energy value (1 to 10) | On | On or off |
| **Write short titles** | Keeps the mix or version out of the title field | Off | On or off |
| **Write year only** | Writes only the year of the release date | Off | On or off |
| **Track number padding** | Minimum digits, so `2` writes `01`, `02` | `0` | `0` to `8` |

The grid has 25 rows. Which providers supply each row:

| Field (row label) | Supplied by |
|---|---|
| Title, Artist, Album, Genre, Label, Release date, Duration | Every provider |
| Version, Album artist, Catalog number | Beatport, Traxsource |
| BPM, Key | Beatport, Traxsource |
| Style, Publish date, Other provider tags, Remixer | Beatport |
| Catalog URL, Provider track ID, Provider release ID, Track number, Track total | Beatport, Traxsource and every other provider except SonoVault |
| Album art | Beatport, Traxsource and every other provider except SonoVault |
| ISRC | Every provider except Traxsource |
| Disc number | Every provider except Beatport, Traxsource and SonoVault |
| Tagging date marker | CueRithm itself: records that and when the track was tagged |

## Genres and styles { #genres }

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Genre and style routing** | Where catalog genres and styles are written | **Genres and styles separately** | **Genres and styles separately**; **Genres only**; **Styles only**; **Merge both into genres**; **Merge both into styles**; **Styles into genres**; **Genres into styles**; **Styles into custom fields** |
| **Merge with existing genres** | Adds catalog genres to the ones already in the file | Off | On or off |
| **Capitalize genres** | Capitalises each genre | Off | On or off |
| **Style field · ID3**, **· VORBIS**, **· MP4** | Names of the tag that holds styles. Shown only when routing is **Styles into custom fields**. | `STYLE` in each | Text |

## Artwork { #artwork }

Turn on **Album art** in the field grid first.

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Artwork destination** | Where catalog artwork goes | **Embed in audio file** | **Embed in audio file**; **External cover file** |
| **Replace all embedded covers** | Used when Album art is set to overwrite. Shown with **Embed in audio file**. | Off | On or off |
| **Cover filename template** | File name for the external cover, with relative subfolders allowed. Shown with **External cover file**; existing embedded covers are kept. | `cover.jpg` when empty | Text |

## File tag format { #tag-format }

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **ID3 version** | Version used when writing ID3 tags | **ID3v2.4** | **ID3v2.4**; **ID3v2.3** |
| **ID3 comment language** | Language code for ID3 comments | Empty | Three letters |
| **Value separator · ID3** | Joins multiple values in ID3 tags | `, ` | Text |
| **Value separator · VORBIS** | Joins multiple values in Vorbis comments. Empty writes separate native values. | Empty | Text or empty |
| **Value separator · MP4** | Joins multiple values in MP4 tags | `, ` | Text |

## Beatport settings { #beatport }

Shown when Beatport is available. Traxsource has no settings of its own.

| Control | What it does | Default | Valid values |
|---|---|---|---|
| **Album art resolution** | Size of the artwork requested, in pixels | `500` | `200` to `1600`, in steps of `100` |
| **Max pages** | How many pages of search results to scan for tracks | `1` | `1` to `10` |
| **Ignore version when matching** | Ignores (Extended Mix), (Original Mix) and similar while scoring titles. Known edition, recording ID and length conflicts still need review. | Off | On or off |

## What can go wrong { #problems }

| Symptom | Cause | Fix |
|---|---|---|
| A provider checkbox is greyed out | It needs a key or account, or it is blocked and no solver is set | Open its **Set up** controls, or configure the [solver](#solver) |
| Tracks stay in Incoming after a lookup | A provider failed, or **No catalog match** is set to **Keep in Incoming for attention** | [Fix a track that needs attention](../../files/needs-attention.md) |
| "Retry loading tagging settings" | The settings could not load | Press it; if it repeats, restart CueRithm |
| Nothing matches | Strictness too high, or the wrong providers | [Catalog tagging finds nothing](../../help/troubleshooting/no-catalog-match.md) |

## Related

- [Tag new tracks from Beatport and Traxsource](../../tagging/tag-new-tracks-automatically.md)
- [Choose providers and fields to write](../../tagging/choose-providers.md)
- [Connect provider accounts](../../tagging/provider-accounts.md)
- [Cleanup settings](cleanup.md), which run after tagging
- [Look up a setting](index.md)

<!-- verified: apps/desktop/src/views/TaggingSettings.tsx (every label, option, min/max/step); apps/desktop/src/components/RecognitionSettings.tsx; apps/desktop/src/components/ProviderSetup.tsx (button names, summary lines); crates/cuerithm-autotag/src/config.rs (AutotagConfig::default, validate: threads 1-64, strictness 0-1, ratio 0-1, track padding 0-8, id3 comm lang 3 letters, Beatport pages 1-10, art 200-1600 step 100); vendor/onetagger/onetagger-tagger/src/lib.rs TaggerConfig::default (tags genre/bpm/style/label/releaseDate, threads 16, strictness 0.7, max duration 30, ratio 0.5, id3v24 true, overwrite true, separators); crates/cuerithm-autotag/src/providers.rs (ten providers, supported tags) and lib.rs (provider_native_field_allowed for SonoVault); vendor/onetagger/providers/{beatport,traxsource}.rs supported_tags and Beatport custom options; crates/cuerithm-autotag/src/recognition.rs; crates/cuerithm-library/src/provider_setup/mod.rs (setup kinds); apps/desktop/src/test/tagging-catalog.json; legacy guides/tagging-settings.md re-checked. UNVERIFIED: the rate-limit wording "up to 60" comes from ProviderSetup.tsx text, not from a request-level test. -->
