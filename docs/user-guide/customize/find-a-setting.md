# Find the setting you need

<p class="lead">Say what you want to change and find the control that does it, in Settings or in one of the places outside it. Looking something up takes about a minute.</p>

## Open Settings { #open }

Choose **Settings** at the bottom of the sidebar. The sliders button **Preparation settings**, at the top right of Tracks, Incoming and Archive, opens the same place. **Library** at the top left takes you back.

Nine pages run down the left side: **Incoming**, **Cue generation**, **Automatic tagging**, **Cleanup**, **File organization**, **Preparation presets**, **Library preferences**, **Smart Playlists** and **Appearance**. For every control on each page, its default and its range, see [Look up a setting](../reference/settings/index.md).

## Find a setting by what you want to do { #by-goal }

| I want to… | Go to | Learn more |
|---|---|---|
| Watch another folder, or stop watching one | **Settings → Incoming** | [Keep a folder like Beatport in your library](../files/watch-folders.md) |
| Use different rules for one folder | **Settings → Incoming**, under *Folder preparation presets* | [Use different rules for different folders](../files/folder-presets.md) |
| Save, load, export or import a whole setup | **Settings → Preparation presets** | [Save your setup as a preset](../files/presets.md) |
| Change how cues are placed | **Settings → Cue generation** | [Choose how cues are placed](../analysis/cue-rules.md) |
| Switch Beatport or Traxsource tagging on or off, or pick the fields to write | **Settings → Automatic tagging** | [Choose providers and fields to write](../tagging/choose-providers.md) |
| Add a provider key or sign in | **Settings → Automatic tagging** | [Connect provider accounts](../tagging/provider-accounts.md) |
| Clean up titles, genres and artists on every new track | **Settings → Cleanup** | [Run cleanup on every new track](../tagging/cleanup-automatically.md) |
| Rename, move or copy files after preparation | **Settings → File organization** | [Rename and sort files into folders](../rename/organize-files.md) |
| Set the time zone for calendar rules, or how strict key matching is | **Settings → Library preferences** | [Look up a setting](../reference/settings/index.md) |
| Have CueRithm build smart playlists by artist, BPM, release date, genre or key | **Settings → Smart Playlists** | [Keep smart playlists up to date automatically](../playlists/automatic-smart-playlists.md) |
| Change waveform colours | **Settings → Appearance** | [Choose a waveform style](waveform-style.md) |
| Switch between light and dark | **Colour theme** button in the header | [Switch between light and dark](theme.md) |
| Resize or collapse the sidebar, or resize the player | The dividers and ++cmd+b++ / ++ctrl+b++ | [Resize the sidebar and player](layout.md) |
| Choose or reorder table columns | The **Columns** button at the end of the table header | [Show the columns you need](../analysis/choose-columns.md) |
| Save a DJ-software destination | **Sync** in the sidebar | [Save a destination for next time](../sync/saved-destinations.md) |
| Reuse a cue layout | **Edit cues & grid** in the player | [Reuse one cue layout on many tracks](../analysis/corrections/cue-templates.md) |
| See the copies CueRithm holds while a tag write is under way | **Settings → Automatic tagging**, row **Recovery backups** | [Where CueRithm keeps your data](../files/library/where-data-lives.md#space) |

## Save a setting { #save }

Most pages share one draft. You can move between **Incoming**, **Cue generation**, **Automatic tagging**, **Cleanup**, **File organization** and **Preparation presets** and your unsaved changes stay. Select **Save settings** at the top right to save them all at once. The button reads *Saving…* and then *Settings saved*. Nothing takes effect until you save.

Three pages work differently:

| Page | How it saves |
|---|---|
| **Library preferences** | Its own button, **Save library preferences** |
| **Smart Playlists** | Its own button, **Save Smart Playlist settings** |
| **Appearance** | Immediately. There is no button |

Provider sign-ins and keys also save the moment you confirm them, and they do not enable a provider by themselves.

Saved preparation settings apply to tracks prepared afterwards. A track already in Incoming keeps the settings it was queued with, and finished tracks stay as they are. To run a track again with the new settings, use [Analyze a track again](../analysis/analyze-again.md).

If another part of CueRithm changes settings while you have unsaved edits, a banner reads *Settings changed elsewhere. Your unsaved draft is retained. Reload before saving.* Select **Discard settings draft and reload** to start again from what is saved.

## What is not a setting { #not-settings }

- **Keyboard shortcuts** are fixed. See [Keyboard shortcuts](../reference/keyboard-shortcuts.md).
- **Analysis models** are built in. There are no model paths to set, and no tempo range control.
- **Audio output** always uses your system's default device. CueRithm has no output picker.

## Related

- [Look up a setting](../reference/settings/index.md)
- [Switch between light and dark](theme.md)
- [Choose a waveform style](waveform-style.md)
- [Resize the sidebar and player](layout.md)

<!-- verified: apps/desktop/src/views/Settings.tsx (pages array, Save settings visibility per page, conflict banner, draft shared across pages), PlaylistPreferences.tsx (Save library preferences, Save Smart Playlist settings), PreparationPresetsSettings.tsx, TaggingSettings.tsx (Recovery backups), App.tsx (Preparation settings button, theme button, Cmd/Ctrl+B, resize separators), components/Sidebar.tsx, PreparationFolderAssignments.tsx (Folder preparation presets), PlayerPanel.tsx (Edit cues & grid control); docs/development/desktop.md (no model path controls); crates/cuerithm-library/src/jobs.rs (jobs pin settings_revision); reference/settings/index.md for the section list. TrackTable.tsx (Columns button); no audio output picker was found in the source -->
