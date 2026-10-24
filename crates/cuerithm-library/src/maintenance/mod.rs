mod deletion;
#[cfg(feature = "autotag")]
mod donor_state;
#[cfg(feature = "autotag")]
mod donors;
mod files;
mod merge;
#[cfg(feature = "autotag")]
pub use donors::DuplicateMergeService;
mod model;
mod purge;
mod review;
mod saved;
mod scan;
mod track_removal;
mod trash;
mod undo_window;
pub use model::*;
pub use trash::{FileTrash, SystemTrash};
pub use undo_window::REMOVAL_UNDO_LIMIT;

mod broken;
mod missing;
pub use missing::*;
mod removal;
mod unused;

#[cfg(feature = "autotag")]
pub(crate) use removal::ensure_unused_unreserved;
