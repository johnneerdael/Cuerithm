# Desktop development

Use Rust stable **1.90 or newer** for the Tauri host, Node 22 and pnpm 12.14.0.
The independent analysis crates retain their declared Rust 1.89 floor. Install
the platform's [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
and a native C/C++ compiler/CMake for OpenKeyScan and bundled TagLib ASF support. Linux also needs ALSA headers
(`libasound2-dev` on Ubuntu) for Rust playback. Engine-only builds do not require
WebKit/GTK or ALSA: select `-p camelot-cli` or exclude both desktop and library.

## Build the decoder once

`bash tools/media-runtime/build.sh` downloads the pinned FFmpeg source, verifies
its hash, builds the minimal decoder and verifies codec coverage and system-only
dynamic dependencies. On Intel Macs/Linux install NASM. On Windows run the same
script in MSYS2 MINGW64 with make, curl, NASM, diffutils, MinGW GCC and Python.
See `.github/actions/media-runtime/action.yml` for exact CI setup. Windows codec
executables statically link compiler support rather than requiring MSYS2 DLLs.

The resulting ignored `tools/media-runtime/bin/` contains ffmpeg, ffprobe,
runtime hashes/recipe and LGPL notice. Never substitute Homebrew-linked binaries
or download an unverified decoder while preparing music. Python is build/test
tooling only. The selected built-in parameters are under `models/builtin/`;
music, training captures/labels and library databases remain outside Git.

The native-MD5 recipe also builds the signed-16 PCM encoder and MD5 muxer used
by the read-only maintenance fingerprint API. Rebuild the runtime after changing
its recipe; sealing rejects a cached runtime that lacks those capabilities.
Fingerprint caches must retain decoder identity alongside source content hashes.
See `docs/research/lexicon-library-maintenance.md` for reference coverage and the
observed AAC decoder-version difference. This API does not enable automatic
duplicate merging or file removal.

```sh
pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/desktop tauri dev
```

In unbundled debug mode, set `CUERITHM_MEDIA_RUNTIME` to the absolute runtime
folder if needed. `CUERITHM_DATA_DIR` selects an isolated acceptance database;
avoid pointing tests at a real library. A new acceptance database may read
`CUERITHM_BOOTSTRAP_SETTINGS` (a complete PreparationSettings JSON). Existing
databases always retain their own settings. Normal users use Settings once.

Set watch folders and cue preferences in Settings. Key/energy parameters are
embedded in desktop executables, verified, and atomically cached under app data.
There are no model path controls or first-run model setup. Upgrades preserve user
preferences and requeue unfinished work against the built-in settings revision.
Enabled unavailable stages stay in Incoming with a reason;
there is no placeholder success. Full successful analysis replaces library cues
atomically and promotes into Tracks. Enabled tagging also writes selected file
tags through a staged, PCM-verified publication. Configure providers, field
policies and output formats once in Settings; see [automatic tagging](../automatic-tagging.md).
With tagging disabled, source audio files remain untouched. Direct DJ-database
sync is a subsequent delivery.

## Checks

```sh
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --exclude cuerithm-desktop --release --locked
cargo test -p cuerithm-desktop --lib --locked
python3 -m unittest discover -s tools/fixtures/rekordbox -v
python3 -m unittest discover -s tools/media-runtime -v
```

Required format tests fail if the packaged runtime is absent. The original
generated 14-case fixture archive covers codecs, padding/edit lists, channels,
sample rates, high-depth/float PCM, seek and shared waveform clocks. Lossless
PCM hashes are exact; lossy checkpoints allow 0.00005 float rounding differences
across architectures. Synthetic SVC fixtures exercise the complete stage contract
without claiming trained-model accuracy. Real development-model checks are
explicit and private, with holdouts reserved.

The native output-device test is ignored by headless CI; on a machine with an
output device, run `cargo test -p cuerithm-library --test playback -- --ignored`.
Actual app acceptance must separately check audible preview, cue/seek position,
background promotion, cue replacement, Settings persistence and restart. Browser
mocks and successful device initialization alone are insufficient evidence.

## Unsigned test packaging

```sh
# macOS
pnpm --dir apps/desktop tauri build --bundles app
target/release/bundle/macos/CueRithm.app/Contents/MacOS/cuerithm-desktop --diagnostics

# Linux
pnpm --dir apps/desktop tauri build --bundles deb

# Windows: build the executable, stage it beside a media/ copy of the runtime
pnpm --dir apps/desktop tauri build --no-bundle
```

`--diagnostics` resolves the same platform resource directory as Tauri and
requires the bundled codec files/hashes. It exits with an error if resources are
missing. Linux CI extracts the actual deb before checking it. Windows CI stages
a portable executable with its media folder; macOS archives the actual app with
executable permissions preserved. Artifacts include source/rebuild materials.

The [desktop workflow](../../.github/workflows/desktop.yml) covers Linux x86_64,
Windows x86_64 and macOS arm64 (Intel Macs are not built) on
[GitHub's native runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Tests and diagnostics establish what each run reports; CI configuration alone
does not establish a green build, live Windows/Linux UI behavior, signing,
notarization or target DJ-app round trips. Signing/public release is a later gate.

Cleanup now has a shared Rust rule engine, all-format staged metadata changes,
automatic Incoming integration and a native preview/apply/Undo workspace. See
[cleanup](../cleanup.md) and its [behavior evidence](../research/cleanup-behavior.md).

Ordinary/smart playlists and their desktop controls are integrated. Duplicate
and file-maintenance backends have a [native API](maintenance-api.md), while
their dedicated screens remain a user-owned UI handoff. Direct rekordbox 6/7
sync stays disabled. See [release scope and outstanding gaps](../release-readiness.md)
for the current delivery boundary and [the direct contract](../research/rekordbox-direct-contract.md)
for known identities and missing target acceptance.

## Preview and batch throughput

The main waveform defaults to a 32-second window that follows the playhead.
Click the main waveform, or focus it and press Enter/Space, to switch between
32 seconds and the entire track. The small overview always shows the full
playable track with all cue positions and remains a click-to-seek control. Left/Right jumps backward or
forward one four-beat bar at the analyzed BPM, preserving playback. Repeated
keypresses are ordered against the current native position and clamped to the
playable duration. Text fields and volume retain their normal arrow keys.
Cmd+A/Ctrl+A selects every matching
track in the current view, including unloaded pages; editable fields retain text
selection. Embedded covers are read without modifying audio, bounded to 8 MiB,
validated by image signature, and cached by raw source identity. Missing artwork
uses the music icon. Transient extraction errors retry after remount.

Preparation uses 75% of the logical CPUs available to the process, rounded down,
with a minimum of one worker (14 available cores gives 10 workers). This follows
the requested CPU allocation; it is not a benchmark-derived optimum or a hard CPU
utilization limit. One watcher owns discovery; each worker owns its
analysis engine. Attempt-checked mutations prevent stale callbacks from changing
a reused job ID, and event sequences remain globally ordered. A controlled local
two-track benchmark took 20.054s with one worker and 10.079s with two; key, BPM,
energy and cues matched. This is a local throughput measurement, not a universal
speed guarantee. The underlying detection arithmetic was unchanged.


The searchable column picker includes source tags and audio/container properties
from every populated track, including fields beyond the currently loaded page.
It retains custom column choices/order, and includes independent album art,
location, filetype and library creation/modification dates. Common metadata uses
readable labels; custom tag names remain available. Unprepared rows do not block
field discovery. Opening the picker backfills legacy metadata from unchanged
sources in the background without rerunning musical analysis or replacing cues.
The main picker is a separate accessible dialog. Missing values show a dash.
Plays, Last Played and Danceability are outside the requested scope.

Camelot key chips in the table and player use the number and major/minor colors
of the [provided DJ.Studio reference](https://dj.studio/blog/camelot-wheel).
The numeric key remains visible; unknown values keep a neutral presentation.


The tagging-enabled library uses schema version 3. Publication journals
retain native paths, before/after file identities, exact PCM proof, post-write
metadata and analysis/cue payloads. File preparation and verification occur
outside the database mutex; final ownership checks, atomic replacements and the
result/cue/Ready transaction are serialized. Backup files and workspace directory
entries are flushed before the journal can authorize publication. Existing files
retain displaced bytes until a verified backup is available. Interrupted uncommitted operations
roll back owned bytes and requeue; later user edits remain untouched and require
attention. Audio and external-cover files participate in the same preparation.
The watcher recognizes in-flight owned hashes instead of treating tag writes as
new incoming files. A build without tagging support refuses pending journals
rather than skipping recovery. Committed publication workspaces retain recovery
original backups under application data only until the write has settled; the copy
is deleted when it commits or rolls back, so processing a large library needs room
for the files in flight, not for the whole library. Settings reports the current
original-backup count and size, which is zero when nothing is under way. Libraries
from earlier versions lose the copies they kept (bounded per start; see
`legacy_originals.rs`). Reviewed unused-file removal moves files to the Trash and keeps
no copy. Redundant
staged outputs are removed after successful completion
or successful rollback. True no-op writes retain no duplicate audio. Persisted
cleanup completion avoids rereading historical backups on every startup.

Completed measured analysis is cached by source, immutable settings/model identity,
decoder identity and executable SHA. Unchanged provider retries reuse that DSP
result, including after restart; decoding still verifies the exact playable PCM
for metadata publication. Source/settings/decoder/application changes invalidate
reuse. Explicit Analyze again clears the track's analysis cache.

Windows flush handles require write access; see Microsoft's
[FlushFileBuffers contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)
and [file metadata caching](https://learn.microsoft.com/en-us/windows/win32/fileio/file-caching).
Platform execution and hardware/power-loss testing remain separate evidence.

Provider HTTP fixtures and tag/PCM round trips run on each native desktop target.
Native live-service access is separate: Beatport matched and tagged a generated
audio fixture. Direct Traxsource requests returned HTTP403; the read-only
`solver_probe` example fetched and parsed live search/track/release metadata
through Trawl and verified a genuine empty search. This does not establish
packaged-app or tag-write acceptance. See the [solver guide](../user-guide/tagging/cloudflare-solver.md). The saved defaults and
provider/field catalog can be inspected without network requests using
`cargo run -p cuerithm-autotag --example settings_catalog`. The frontend test
fixture is generated from this command; compare it when changing adapter options.


TagLib remains a statically linked library. On MSVC, its CMake runtime property
and the C++ bridge follow Rust's `crt-static` target feature, so both select the
same CRT mode. TagLib's legacy `/MD` string replacement alone does not cover
modern CMake's [CMP0091 runtime property](https://cmake.org/cmake/help/latest/variable/CMAKE_MSVC_RUNTIME_LIBRARY.html).


## Cleanup native acceptance

On macOS arm64, the packaged 88.03 MiB app passed resource/built-in diagnostics.
An isolated library prepared one generated 64-second FLAC. A native preview left
file bytes unchanged; Apply updated its title; after restart, Undo restored the
original file SHA-256. Complete analysis JSON and existing cue history were
identical throughout. While loaded and playing, the title refreshed after Undo
and the playback clock continued from its current position. Counted native genre
and contributor views preserved a comma-containing artist as one name. Settings
and preview scrolling were inspected in the native window.

This acceptance used generated audio only. It did not normalize the user's
music library or establish live Windows/Linux UI behavior, power-loss hardware
behavior or full Lexicon application parity. Native CI packages and tests provide
their own exact-head evidence.
