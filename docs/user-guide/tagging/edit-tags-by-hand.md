# Edit tags and artwork

<p class="lead">Change titles, artists, genres, dates, ratings, comments and cover art on one track or on many, check every change before it is written, and take it back later if you need to. A first edit takes about three minutes.</p>

<!-- figure:existing-88-metadata-editor-light-edit-tags-by-hand -->
<figure markdown>
![Editing common metadata with the library and player still visible](../assets/screens/88-metadata-editor-light.png#only-light){ .screen loading=lazy }
![Editing common metadata with the library and player still visible](../assets/screens/88-metadata-editor-dark.png#only-dark){ .screen loading=lazy }
<figcaption>Edit tags beside the table and the player.</figcaption>
</figure>
<!-- /figure -->

**Before you start**, note that the measured key, BPM, energy and cues are separate and are not edited here.

!!! warning "Saving writes into your music files"
    CueRithm checks that the audio is unchanged and records the values it replaced, but practise on a copy if you are new to this. A saved edit can be taken back with **Undo saved changes**; see [Undo a tag change](tag-history.md).

## 1. Select the tracks and open the editor { #open-the-editor }

Select one or more tracks in **Tracks** or **Incoming**, then choose **Edit metadata** in the toolbar above the table. You can also use **Edit metadata** in a row's actions menu. Playback does not start, and the panel stays above the player. In a window narrower than 1250 pixels it replaces the table until you choose **Back to tracks**.

The panel opens with the list **Selected files**, then **Measured analysis** (read-only key, BPM and energy, shown when one track is selected), then the field groups. A status beside the buttons reads *Draft saved locally* once your input is stored.

## 2. Change a field { #edit-file-tags }

Every field has an action menu next to it. It starts on **Keep**, which leaves the field out of the edit.

| Action | Effect |
|---|---|
| **Keep** | Leave the field alone (the default) |
| **Set** | Replace the field with what you type |
| **Clear** | Remove the field |
| **Append** | Add values and keep the existing ones (fields that hold several values) |

A text box is greyed out until you choose **Set** (or **Append**). Typing nothing never clears a field. Fields that hold several values, such as **Artists**, **Genres** or **Comment**, take one value per line, in order.

| Group | Fields |
|---|---|
| **Recording** | Title, Artists, Version / mix, Genres, Styles |
| **Release** | Album, Album artists, Label, Catalog number, ISRC, Release date, Original release date, Track number, Track total, Disc number, Disc total |
| **Contributors** | Remixers, Producers, Composers, Lyricists, Grouping |
| **Rating and comments** | **Your rating** (one to five stars), **Comment** |
| **File key and BPM overrides** | File key tag, File BPM tag |

Dates accept `YYYY`, `YYYY-MM` or `YYYY-MM-DD` and keep the precision you give, so a year never gains an invented day. Track and disc numbers are whole numbers from 1 to 65,535. A BPM must be above 0 and below 1,000. An unfinished or invalid value is kept in your draft and turns off **Preview changes** until you fix it.

The two **File … tag** fields change what is written inside the file for other software. The measured key and BPM in your library stay as analysed. To correct those, use [Fix a wrong beatgrid](../analysis/corrections/fix-the-beatgrid.md).

**Worked example.** Select `Atoma (Extended Mix)`. **Artists** shows `Anyma (ofc)`. Change its action to **Set**, replace the text with `Anyma` and leave everything else on **Keep**. Under **Measured analysis**, the key <span class="cr-key">2A</span>, the BPM <span class="cr-bpm">130</span> and the energy <span class="cr-energy">6/10</span> do not move. To remove the same suffix from many tracks at once, use [Write cleanup rules](cleanup-rules.md) instead.

## 3. Edit many tracks at once { #many-tracks }

Select several tracks before you open the editor. A field whose values differ shows **Mixed values** and stays on **Keep**, so each track keeps its own value. A field with the same value everywhere shows that value.

Select `Atoma (Extended Mix)`, `No Good (Extended Mix)`, `Other Dimension (Extended Mix)` and `Prophecy (Extended Mix)`. **Genres** shows `Melodic House & Techno` for all four, and **Artists** shows **Mixed values**. Set **Grouping** to `Peak hour` and every track gets it, while no artist changes.

Comments and ratings are matched by identity. Only records that every selected track shares are editable together; select one track to edit its other comment records.

## 4. Change artwork { #artwork }

The **Artwork** section lists the pictures all selected tracks share, for example *Front cover*, with a short hash so identical-looking pictures can be told apart.

- **Replace selected picture** opens a file picker for a JPEG, PNG or WebP image. The image must be under 8 MiB and no larger than 4096 by 4096 pixels.
- **Remove selected picture** removes only that picture.
- **Add a picture** in the list adds a new one. Set **Picture type** (**Container default**, **Front cover**, **Back cover**, **Artist** or **Other**) and a **Description**, then choose **Choose image**.
- **Keep original artwork** cancels a pending change.

A line under the buttons states what will happen. With several tracks selected, replace and remove apply only to pictures that are identical on all of them, and a new picture is added to every track. Other pictures stay as they are.

## 5. Preview, then save { #drafts-preview-and-save }

1. Choose **Preview changes**. The panel lists every track with *ready*, *unchanged* or *failed*, and for each field the **Before** and **After** values.
2. Check the list. A failed track is explained and does not stop the others.
3. Choose **Save changes**. You can also press ++cmd+s++ / ++ctrl+s++: the first press previews, and a second press saves.

Changing anything discards the old preview, and the preview is also dropped if the files change underneath you. The result reads *Publication · saved*, *partial*, *queued* or *recovery required*. Read each track's outcome; a partial batch does not mean every track was saved. Choose **New edit** to continue with the same selection.

If some selected MP3-style files still use ID3v2.3, a checkbox **Allow conversion to ID3v2.4** appears. It is needed when you set several values in one field, or set a release date or original release date. ID3v2.4 can be unreadable to older players, so it is opt-in and the preview names the affected files.

## What happens to the file { #what-happens }

- **Finished track (Tracks):** the change is written now. CueRithm edits a staged copy, decodes it again to confirm the audio is identical, then swaps it in. The values it replaced are recorded for **Undo saved changes**.
- **Track still in Incoming:** the edit is queued and applied by preparation, and the result reads *Publication · queued*. The fields you changed are protected: automatic tagging does not replace them, and cleanup does not rewrite them, although it still tidies the fields you left alone.
- Editing a finished track does not rename or move its file. Use [Rename and sort files into folders](../rename/organize-files.md) when you want that too.

Drafts are saved as you type. Closing the panel, changing the selection or quitting waits until the latest draft is stored. **Drafts and history** (a button above the table and in the editor header) lists unfinished drafts; [Undo a tag change](tag-history.md) explains it.

## What can go wrong { #what-can-go-wrong }

| Symptom | Cause | Fix |
|---|---|---|
| **Preview changes** is disabled | A date or number is invalid, or the draft is still saving | Fix the red field; wait for *Draft saved locally* |
| *Draft not saved* with **Retry saving draft** | The draft could not be stored | Retry. **Discard unsaved changes** drops only what could not be saved. |
| A field is greyed out with a reason | The file format cannot hold it | Pick tracks of a format that can, or skip the field |
| *Selected files changed. Preview changes again before saving.* | A file changed on disk | Choose **Preview changes** again |
| *Native container cannot preserve this exact typed metadata edit; source remains unchanged* | The format cannot store the exact value | Use a simpler value; the file was not changed |

## Related { #related }

- [Undo a tag change](tag-history.md): drafts, history, Undo and recovery.
- [Review and fix a wrong match](review-a-match.md): take values from a catalog instead of typing them.
- [Group tracks with categories and custom tags](curate-tracks.md): labels that live in the library rather than in the file.
- [Work on many tracks at once](../analysis/select-many.md)

<!-- verified: apps/desktop/src/components/MetadataEditor.tsx; MetadataFields.tsx; ArtworkEditor.tsx; MetadataEditResult.tsx; apps/desktop/src/editor-fields.ts and editor-types.ts; use-metadata-draft.ts; apps/desktop/src-tauri/src/metadata_commands.rs (picker filter jpg, jpeg, png, webp; 8 MiB); crates/cuerithm-autotag/src/image.rs (4096 px); crates/cuerithm-autotag/src/editing/native.rs (ID3v2.4 rule); crates/cuerithm-library/src/metadata_edit/service.rs (staged preview, Incoming overrides); crates/cuerithm-autotag/src/write.rs (reviewed_cleanup_fields, protected tags, decode check); apps/desktop/src/test/metadata-editor.test.tsx; legacy guides/metadata-editing.md re-checked -->
