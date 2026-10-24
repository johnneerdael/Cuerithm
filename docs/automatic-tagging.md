# Automatic tagging

Enable **Settings → Automatic tagging** and save once. The same configuration
applies to new imports, watch-folder arrivals and explicit Analyze again jobs.
It does not automatically retag tracks already in the library when Settings
change. Existing offline configurations stay offline until enabled.

Preparation runs full audio analysis, catalog matching, staged metadata writing
and replacement cues before moving the track from Incoming to Tracks. Saved
file-organization rules run last, after accepted tagging and cleanup, before Ready.
The default leaves audio at its location. Key, BPM, beatgrid, cue and whole-track energy
results in the library remain measured analysis, independently of catalog tags.

## Providers and matching

Choose from Beatport, Traxsource, MusicBrainz, SonoVault, Discogs,
iTunes, BPM Supreme, Bandcamp, Deezer and Volumo. Beatport and Traxsource
remain the default selection; additional providers are opt-in. Move providers
earlier or later to set their order. A source marked as restricted that has no working solver shows as
unavailable and cannot be newly selected; a saved selection stays visible so you
can clear it.

MusicBrainz needs no account. Discogs works anonymously at CueRithm's limit of
25 requests/minute; an optional token verified against the account allows up to
60. End users do not need to register a developer application. SonoVault is
optional and requires a personal API key; Volumo also requires a personal API key
when selected, and BPM Supreme requires an account.
The other listed providers have no end-user credential setup in CueRithm.
Provider availability and supplied fields can differ.

Open a provider's **Set up** or **Manage** controls to save, test, replace or remove
its credential in the system credential store. These actions take effect
immediately and are separate from **Save settings**, which applies provider
selection and tagging rules. SonoVault links to [free API-key signup](https://sonovault.now/signup):
verify the account email before testing. Each SonoVault connection test uses
one metadata credit. A test key's mock response does not prove live access.
Unknown allowance or reset times stay unknown; rate or quota errors are not
reported as no match. CueRithm does not discover credentials from user key files.
See the [provider setup steps](user-guide/tagging/provider-accounts.md).

Volumo tokens are created in the account's [API-token settings](https://volumo.com/profile/api-tokens).
Its official API resolves an ISRC to links; CueRithm can read public track-page
metadata from the returned Volumo link. This is not a full free catalog-search
API. Connection testing checks one sample ISRC, without uploading audio. An
unavailable sample leaves the key unverified rather than declaring it invalid.

With Combine providers off, the first successful selected provider supplies metadata.
With it on, every selected provider is consulted and matches apply in order.
Configured provider failures leave the job in Incoming for retry; they are not
reported as successful empty results. Traxsource uses the shared public-catalog
fetcher to detect Cloudflare challenges. Detected restrictions are persisted,
and a marked source stops direct requests until explicitly reset. Configure an
optional FlareSolverr-compatible URL under Automatic tagging; affected sources
use its `/v1` endpoint, with a 120-second default solve timeout and one retry for
timeouts or temporary unavailability.
See [solver setup](user-guide/tagging/cloudflare-solver.md) for FlareSolverr, Byparr
and Trawl, source availability, caching and troubleshooting.

Known edition/recording conflicts and tied best candidates stay in Incoming for
review, including disagreements between combined providers. Oldest/newest release
preferences select among compatible candidates. Ignoring mix text for title
similarity still retains recording/edition checks. Matching evidence is bound to
the source, settings and claimed preparation attempt and survives attention/restart.

Matching settings include title/artist strictness, optional duration tolerance,
embedded IDs/ISRC, oldest/newest release preference, filename parsing and title
cleanup patterns. Skip previously tagged recognizes OneTagger and Cuerithm date
markers; audio analysis and cue replacement still run. Album matching can reuse
an identified Beatport or Traxsource release for the remaining Incoming files in the same
folder and settings revision. It does not retag older library files in that folder.

Tagging concurrency is shared across analysis workers. Traxsource permits one
active request at a time. Audio workers independently use 75% of process-available
logical cores, rounded down with a minimum of one. Include subfolders controls
both watched folders and folder imports, even when catalog tagging is disabled.

Provider request budgets are shared across libraries and survive restarts.
A second CueRithm process can report private credential access as busy; public
providers and offline analysis/editing remain available. Public lookups still
need working catalog storage and an internet connection.

## Identify unresolved recordings

**Identify unresolved recordings** is off by default. To use it, allow fingerprint
lookup, enable **Identify unresolved tracks**, then save settings. Chromaprint
creates the fingerprint locally; CueRithm sends the compressed fingerprint and
duration to AcoustID, without uploading audio. The packaged build supplies the
registered application identity; no end-user application key is required. A build
without that identity shows lookup as unavailable.

Identification is a fallback when normal matching cannot resolve a readable
track, with one lookup per preparation attempt. MusicBrainz provides possible
recordings and releases. Even a recognized recording stays in Incoming for
edition review; it does not authorize automatic release or mix selection. Short
clips may not produce a usable fingerprint. Recognition does not replace measured
key, BPM, energy or cues.

Personal credentials, destination connections and local transmission consent are
excluded from portable preset exports. Loading a preset preserves local consent.
Spotify is skipped. AudD and RapidAPI are research/trial integrations and are not
selectable providers.

## Fields and output

Select fields to write and choose overwrite-all or individual overwrite exceptions.
Unselected fields remain unchanged. Available fields come from the selected
provider implementations; the editor shows which providers supply each field.
Custom Style/Other tags mappings cannot target protected standard or opaque DJ
records. Preparation provenance records whether selected fields were written,
kept or unavailable; configured required fields must be present before completion.
Key/BPM file tags can use measured analysis or catalog metadata. Track energy is
a separate optional file write and remains a single whole-track rating.

Genre/style routing offers eight routes, optional merging/capitalization and
per-format custom style fields. Choose embedded artwork or an external cover file
beside the audio using a relative filename template. External mode retains
existing embedded covers. Enable Album art under
Fields to write to retrieve artwork. Artwork overwrites follow the same field
policy. File-format settings include ID3v2.3/2.4, separators, comment language,
year-only dates and track-number padding. Beatport adds artwork resolution,
search-page count and version matching controls.

If a catalog returns no match, either retain existing catalog metadata and finish
with configured measured tags, or leave the track in Incoming for attention.
A no-match outcome is retained in preparation provenance. Failed writes retain
previous cues and use journaled backups/recovery. A copy of the original audio
is kept in application data only while a write is under way: once it has settled
(committed or rolled back) the copy is removed, and a library from an earlier
version loses the copies it kept, a bounded number per start. Settings shows what
is still held. Automatic tagging has no Undo. Redundant staged outputs are cleaned
automatically, and no-op writes retain no duplicate audio.
Unchanged provider retries reuse completed full analysis. If a file changes during lookup,
its new bytes are analysed before publication; an older attempt cannot overwrite
a newer job.

## Validation and current access

Deterministic local HTTP fixtures validate provider parsing, matching,
failures, expiry and concurrency. Generated audio fixtures validate metadata and
artwork changes across the supported formats with exact before/after decoded
PCM and frame checks. These checks establish pipeline behavior, not catalog
coverage or musical-analysis accuracy.

A native Beatport catalog probe returned a public diagnostic match through its
catalog API. Direct Traxsource requests returned HTTP 403 on the validation
machine. A read-only Trawl probe retrieved live search, track and release pages
that the Rust adapter parsed, including key, BPM and release metadata. This
establishes that fetch/parse path, not a file-write or packaged-app acceptance. A blocked provider stays actionable in
Incoming, without silently bypassing a required tagging stage.

The integration excludes Spotify/audio-feature workflows, Shazam, QuickTag
and post-commands. [Manual metadata editing and match review](user-guide/tagging/edit-tags-by-hand.md)
are available independently of automatic tagging. The catalog integration reuses pinned OneTagger
source under GPLv3; desktop licensing and modifications are documented in
[third-party notices](../THIRD_PARTY_NOTICES.md). The separate analysis engine
remains MIT.

Settings → File organization saves opt-in rename, move and copy rules. The
Organize files workspace previews complete existing-track scopes and exposes
actual results, recovery and guarded Undo. Required reference repair and file
cleanup finish before Incoming promotion. See the
[file organization guide](user-guide/rename/organize-files.md) for naming,
copy-original conflicts and the known-reference boundary.
