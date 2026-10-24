use super::*;
use anyhow::{Context, Result, ensure};
use cuerithm_autotag::{
    ArtworkEdit, ArtworkValue, CommentEdit, CommentIdentity, MetadataEditorReceipt, MetadataPatch,
    TextEdit, ValueEdit,
};
use cuerithm_media::CancellationToken;
impl MetadataEditService {
    /// Stage the inverse of every committed Incoming override; nothing is published here.
    pub(super) fn stage_incoming_undo(
        &self,
        original: &str,
        offset: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<super::journal::JournaledRow>> {
        let rows = {
            let conn = self.library.conn()?;
            let mut q=conn.prepare("SELECT o.job_id,o.payload_json,o.receipt_json,o.final_json,r.payload_json FROM metadata_edit_overrides o JOIN metadata_edit_rows r ON r.batch_id=o.batch_id AND r.track_id=o.track_id WHERE o.batch_id=?1 AND o.state='committed' AND o.undo_batch_id IS NULL AND o.final_json IS NOT NULL ORDER BY r.ordinal")?;
            q.query_map([original], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut journaled = vec![];
        for (index, (job, patch, receipt, proof, frozen)) in rows.into_iter().enumerate() {
            let old: FrozenRow = serde_json::from_str(&frozen)?;
            let outcome = (|| -> Result<FrozenRow> {
                cancel.check()?;
                let proof: super::overrides::FinalOverride = serde_json::from_str(&proof)?;
                let receipt: MetadataEditorReceipt = serde_json::from_str(&receipt)?;
                let editor: MetadataEditorPatch = serde_json::from_str(&patch)?;
                let capture = self.capture(&old.public.track_id, cancel)?;
                ensure!(
                    capture.public.state == "ready"
                        && capture.path == proof.path
                        && capture.parent.ancestors == proof.parent.ancestors
                        && capture.fingerprint.identity == proof.fingerprint.identity
                        && capture.fingerprint.sha256 == proof.fingerprint.sha256
                        && capture.public.path_revision == proof.path_revision
                        && capture.public.metadata_revision == proof.metadata_revision
                        && capture.job.as_deref() == Some(job.as_str()),
                    "Incoming metadata Undo source or revision changed"
                );
                ensure!(
                    capture.public.native == receipt.after,
                    "Incoming metadata changed after its saved revision"
                );
                let native = self.native_patch(&editor)?;
                let inverse = self.inverse_patch(&native, &receipt, &mut |ordinal, identity| {
                    // A picture kept as an artwork asset needs no original audio.
                    if let Some(kept) = old
                        .kept_pictures
                        .iter()
                        .flatten()
                        .find(|picture| picture.ordinal == ordinal)
                    {
                        return self.library.metadata_artwork_content(&kept.asset_id);
                    }
                    ensure!(
                        !old.original_retired,
                        "The original artwork of this save is no longer available"
                    );
                    // Not retired yet: the picture is read from the original file this save kept.
                    let original = old
                        .stage
                        .as_ref()
                        .context("Missing original metadata snapshot for artwork Undo")?;
                    super::previews::verify_backup(&self.library, original)?;
                    let backup = crate::model::bytes_path(&original.backup)?;
                    crate::artifacts::owned_file(
                        &backup,
                        &crate::model::bytes_path(&original.workspace)?,
                        &original.before,
                    )?;
                    let info = self.runtime.probe_cancellable(&backup, cancel)?;
                    cuerithm_autotag::metadata_artwork_bytes(&backup, &info, identity)
                })?;
                let (stage, after) = self.stage_row(&capture, &inverse, cancel)?;
                Ok(FrozenRow {
                    public: MetadataEditRow {
                        track_id: capture.public.track_id.clone(),
                        title: capture.public.title.clone(),
                        before: capture.public.native.clone(),
                        after,
                        outcome: "ready".into(),
                        fields: super::service::field_outcomes(&native, "ready", None),
                        error: None,
                    },
                    capture,
                    stage: Some(stage),
                    undo: Some(UndoLink::Override(job.clone())),
                    kept_pictures: None,
                    original_retired: false,
                    patch: None,
                })
            })();
            let row = match outcome {
                Ok(row) => row,
                Err(error) => {
                    let mut row = old;
                    row.stage = None;
                    row.public.outcome = "failed".into();
                    row.public.error = Some(format!("{error:#}"));
                    row
                }
            };
            journaled.push(super::journal::JournaledRow {
                id: uuid::Uuid::new_v4().to_string(),
                ordinal: offset + index,
                track_id: row.public.track_id.clone(),
                state: row.public.outcome.clone(),
                payload: bounded_json(&row)?,
                error: row.public.error.clone(),
            });
        }
        Ok(journaled)
    }
    /// The patch that turns `receipt.after` back into `receipt.before`, for the fields `patch`
    /// changed. `picture` supplies the bytes of an original picture the edit replaced or removed
    /// (its position among the original pictures, and its identity).
    pub(super) fn inverse_patch(
        &self,
        patch: &MetadataPatch,
        receipt: &MetadataEditorReceipt,
        picture: &mut dyn FnMut(usize, &cuerithm_autotag::ArtworkIdentity) -> Result<Vec<u8>>,
    ) -> Result<MetadataPatch> {
        let before = &receipt.before;
        let after = &receipt.after;
        let mut inverse = MetadataPatch::default();
        for (field, edit) in &patch.text {
            if !matches!(edit, TextEdit::Keep) {
                let values = before.text.get(field).cloned().unwrap_or_default();
                inverse.text.insert(
                    *field,
                    if values.is_empty() {
                        TextEdit::Clear
                    } else {
                        TextEdit::Set { values }
                    },
                );
            }
        }
        macro_rules! scalar {
            ($field:ident) => {
                if !matches!(patch.$field, ValueEdit::Keep) {
                    inverse.$field = before
                        .$field
                        .clone()
                        .map(|value| ValueEdit::Set { value })
                        .unwrap_or(ValueEdit::Clear);
                }
            };
        }
        scalar!(release_date);
        scalar!(original_date);
        scalar!(track_number);
        scalar!(track_total);
        scalar!(disc_number);
        scalar!(disc_total);
        scalar!(bpm);
        for edit in &patch.ratings {
            if !matches!(edit.value, ValueEdit::Keep) {
                inverse.restore_rating_record(
                    edit.identity.clone(),
                    before
                        .ratings
                        .iter()
                        .find(|r| r.identity == edit.identity)
                        .cloned(),
                );
            }
        }
        for edit in &patch.comments {
            if matches!(edit.value, TextEdit::Keep) {
                continue;
            }
            let identity = &edit.identity;
            let previous = before.comments.iter().find(|c| {
                c.identity.language == identity.language
                    && c.identity.description == identity.description
            });
            let current = after.comments.iter().find(|c| {
                c.identity.language == identity.language
                    && c.identity.description == identity.description
            });
            let identity = current
                .map(|c| c.identity.clone())
                .unwrap_or(CommentIdentity {
                    language: identity.language.clone(),
                    description: identity.description.clone(),
                    sha256: String::new(),
                });
            inverse.comments.push(CommentEdit {
                identity,
                value: previous
                    .map(|c| TextEdit::Set {
                        values: c.values.clone(),
                    })
                    .unwrap_or(TextEdit::Clear),
            });
        }
        // Pictures are only restored when the edit changed them. A save that replaced a picture
        // with itself, or removed one and added the same one back, left the artwork as it was:
        // there is nothing to put back, and no original picture was kept (the save keeps them
        // under the same condition) for an Undo to ask for.
        if !patch.artwork.is_empty() && before.artwork != after.artwork {
            let mut mapping = (0..before.artwork.len()).map(Some).collect::<Vec<_>>();
            let mut targets = patch
                .artwork
                .iter()
                .filter_map(|e| {
                    e.target
                        .as_ref()
                        .map(|t| (t.ordinal, e.replacement.is_some()))
                })
                .collect::<Vec<_>>();
            targets.sort_by_key(|(i, _)| std::cmp::Reverse(*i));
            for (index, replaced) in targets {
                ensure!(
                    index < mapping.len(),
                    "Original selected artwork is missing"
                );
                if !replaced {
                    mapping.remove(index);
                }
            }
            for edit in &patch.artwork {
                if edit.target.is_none() && edit.replacement.is_some() {
                    mapping.push(None);
                }
            }
            ensure!(
                mapping.len() == after.artwork.len(),
                "Prepared artwork identity receipt differs"
            );
            for (index, origin) in mapping.iter().enumerate() {
                if origin.is_none() {
                    inverse.artwork.push(ArtworkEdit {
                        target: Some(after.artwork[index].identity.clone()),
                        replacement: None,
                    });
                }
            }
            let mut resulting = mapping.iter().filter_map(|i| *i).collect::<Vec<_>>();
            for edit in &patch.artwork {
                let Some(target) = &edit.target else { continue };
                let index = target.ordinal;
                let previous = before
                    .artwork
                    .get(index)
                    .context("Missing selected original picture")?;
                let value = ArtworkValue {
                    kind: previous.identity.kind,
                    description: previous.identity.description.clone(),
                    bytes: picture(index, &previous.identity)?,
                };
                let current = mapping.iter().position(|i| *i == Some(index));
                inverse.artwork.push(ArtworkEdit {
                    target: current.map(|i| after.artwork[i].identity.clone()),
                    replacement: Some(value),
                });
                if current.is_none() {
                    resulting.push(index);
                }
            }
            inverse.reorder_artwork(
                (0..before.artwork.len())
                    .map(|index| {
                        resulting
                            .iter()
                            .position(|i| *i == index)
                            .context("Original artwork order is missing")
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(inverse)
    }
}
