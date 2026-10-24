mod capture;
mod journal;
mod model;
mod overrides;
mod service;
use capture::Capture;
pub use model::*;
pub use service::MetadataEditService;
use service::{FrozenPreview, FrozenRow, PreviousPicture, StoredStage, UndoLink};

const MAX_METADATA_JSON: usize = 64 * 1024 * 1024;
pub(super) fn bounded_json<T: serde::Serialize>(value: &T) -> anyhow::Result<String> {
    struct Limited(Vec<u8>);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_METADATA_JSON {
                return Err(std::io::Error::other(
                    "Complete metadata scope exceeds64MiB; narrow selection",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Limited(Vec::new());
    serde_json::to_writer(&mut output, value)?;
    Ok(String::from_utf8(output.0)?)
}

mod incoming_undo;

mod curation;
mod previews;
pub(crate) use curation::child_undo_available as curation_child_undo_available;
