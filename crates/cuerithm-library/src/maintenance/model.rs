use crate::SmartStamp;
pub use cuerithm_maintenance::Tolerance as DuplicateTolerance;
pub use cuerithm_maintenance::{GroupEdit, MergeDisposition, Preference, ReviewGroup, ReviewState};
use cuerithm_media::AudioFingerprint;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateReview {
    pub id: String,
    pub scan_id: String,
    pub created: i64,
    pub revision: u64,
    pub stamp: MaintenanceStamp,
    pub groups: Vec<ReviewGroup>,
    pub stale: bool,
    pub stale_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DuplicateReviewEdit {
    Group {
        group_id: String,
        edit: GroupEdit,
    },
    Prefer {
        preference: Preference,
        override_locked: bool,
        use_best_cues: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoredDuplicateTrack {
    pub id: String,
    pub display_path: String,
    pub title: String,
    pub artist: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateIgnores {
    pub stamp: MaintenanceStamp,
    pub tracks: Vec<IgnoredDuplicateTrack>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicatePlaylistChange {
    pub id: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}
/// How many removals keep their Undo data; a merge that deletes files is one of them. Older
/// ones become final: the removed library records are no longer kept, and only the Trash
/// still holds the files. The bound is shared with Missing & broken removals.
pub const DUPLICATE_UNDO_LIMIT: usize = super::undo_window::REMOVAL_UNDO_LIMIT;

/// What proves a file duplicates the track that is kept.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateIdentity {
    /// Byte-for-byte the same file (equal SHA-256).
    SameFile,
    /// The same decoded audio (equal sample rate, channels and decoded-audio digest).
    SameAudio,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateSkipReason {
    /// The audio is not proven identical to the kept track (for example a different mix).
    NotVerifiedIdentical,
    /// The file changed after the scan or the preview.
    FileChanged,
    /// The file is missing, unreadable or otherwise unavailable.
    FileUnavailable,
    /// Another remaining library record still uses this file.
    SharedWithRemainingRecord,
    /// The file is the kept track's own file under another name.
    SameFileAsKeeper,
    /// The track is still being prepared or another operation holds the file.
    Busy,
    /// The operating system could not move the file to the Trash.
    TrashFailed,
}
/// A file a "Delete from disk" merge moves to the Trash, shown in the preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateDeletionFile {
    pub track_id: String,
    pub display_path: String,
    pub size_bytes: u64,
    pub keeper_id: String,
    pub keeper_display_path: String,
    pub identity: DuplicateIdentity,
    pub sha256: String,
    /// The content hash of the kept track's file that this deletion was approved against. Just
    /// before the file is removed, the kept track's file must still be exactly this (or what a
    /// donor copy made of it), or the duplicate stays.
    #[serde(default)]
    pub keeper_sha256: String,
    /// The Trash cannot take this file, so it will be deleted permanently if the Trash
    /// still refuses it when the merge is applied. The preview says so; nothing else is
    /// ever deleted permanently.
    #[serde(default)]
    pub permanent: bool,
}
/// A file a "Delete from disk" merge leaves exactly where it is, with the reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateDeletionSkip {
    pub track_id: String,
    pub display_path: String,
    pub keeper_id: Option<String>,
    pub reason: DuplicateSkipReason,
    pub detail: Option<String>,
}
/// Library records that leave with a deleted file and come back with Undo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateRemovalImpact {
    pub track_id: String,
    pub cue_revisions: usize,
    pub tags: usize,
    pub sync_links: usize,
    pub other_records: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateDeletionPlan {
    pub files: Vec<DuplicateDeletionFile>,
    pub skipped: Vec<DuplicateDeletionSkip>,
    pub impact: Vec<DuplicateRemovalImpact>,
    /// Merges that delete files keep Undo data for this many operations.
    pub undo_limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMergePreview {
    pub id: String,
    pub review_id: String,
    pub review_revision: u64,
    pub created: i64,
    pub stamp: MaintenanceStamp,
    pub plan: cuerithm_maintenance::MergePlan,
    pub playlists: Vec<DuplicatePlaylistChange>,
    /// Present exactly when the plan deletes files.
    #[serde(default)]
    pub deletion: Option<DuplicateDeletionPlan>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMergeResult {
    pub id: String,
    pub preview_id: String,
    pub stamp: MaintenanceStamp,
    pub archived_ids: Vec<String>,
    pub restored_ids: Vec<String>,
    pub affected_playlists: Vec<String>,
    pub undo_of: Option<String>,
    #[serde(default)]
    pub updated_ids: Vec<String>,
    /// Library records removed together with their files moved to the Trash.
    #[serde(default)]
    pub removed_ids: Vec<String>,
    /// Files this merge moved to the Trash.
    #[serde(default)]
    pub trashed: Vec<DuplicateDeletionFile>,
    /// Files this merge deleted permanently because the Trash could not take them.
    #[serde(default)]
    pub permanently_deleted: Vec<DuplicateDeletionFile>,
    /// Files a "Delete from disk" merge did not move, and why.
    #[serde(default)]
    pub not_deleted: Vec<DuplicateDeletionSkip>,
    /// Undo only: sync or DJ-program link rows left as they are because something changed
    /// them after the removal.
    #[serde(default)]
    pub links_not_restored: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMergeHistory {
    pub result: DuplicateMergeResult,
    pub created: i64,
    pub undone_by: Option<String>,
    /// The removed records are no longer kept, so Undo is no longer offered.
    #[serde(default)]
    pub undo_expired: bool,
    #[serde(default)]
    pub undo_limit: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateScanScope {
    #[default]
    FullLibrary,
    SelectedPlaylists,
    FullLibraryFiltered,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DuplicateScanRequest {
    pub scope: DuplicateScanScope,
    pub playlist_ids: Vec<String>,
    pub tolerance: DuplicateTolerance,
    pub ignored_paths: Vec<PathBuf>,
}
impl Default for DuplicateScanRequest {
    fn default() -> Self {
        Self {
            scope: DuplicateScanScope::FullLibrary,
            playlist_ids: vec![],
            tolerance: DuplicateTolerance::None,
            ignored_paths: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaintenanceStamp {
    pub inputs: SmartStamp,
    pub maintenance_revision: u64,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceFileState {
    Present,
    Missing,
    Unavailable,
    Symlink,
    NotRegular,
    Changed,
    Video,
    NoAudio,
    DecodeError,
    Unsupported,
    TimedOut,
    DurationSkipped,
}
impl MaintenanceFileState {
    /// The one availability rule for library audio, shared by Missing & broken and
    /// Duplicates: a file is available when it is a regular, readable file. A duplicate
    /// scan skips decoding very short and very long files (`Duration skipped`); they are
    /// still present and readable. Every other outcome means the file cannot be used.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Present | Self::DurationSkipped)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanTrack {
    pub id: String,
    pub display_path: String,
    pub title: String,
    pub artist: String,
    pub archived: bool,
    pub state: MaintenanceFileState,
    pub detail: Option<String>,
    pub observed_sha256: Option<String>,
    pub source_changed: bool,
    pub fingerprint: Option<AudioFingerprint>,
    pub cached: bool,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateMatchMethod {
    Fingerprint,
    ArtistTitle,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanGroup {
    pub id: String,
    pub method: DuplicateMatchMethod,
    pub track_ids: Vec<String>,
    pub suggested_keeper: Option<String>,
    pub suggested_tags_from: Option<String>,
}
/// A track the scan left out because its audio file is not available. It is never grouped,
/// counted as a duplicate, or offered as a keeper or donor; Missing & broken lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateSkippedTrack {
    pub id: String,
    pub state: MaintenanceFileState,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanReport {
    pub id: String,
    pub created: i64,
    pub stamp: MaintenanceStamp,
    pub request: DuplicateScanRequest,
    pub stale: bool,
    pub stale_reason: Option<String>,
    pub tracks: Vec<DuplicateScanTrack>,
    pub groups: Vec<DuplicateScanGroup>,
    #[serde(default)]
    pub skipped_unavailable: Vec<DuplicateSkippedTrack>,
    pub cache_hits: usize,
    pub fingerprints_computed: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanProgress {
    pub completed: usize,
    pub total: usize,
    pub track_id: Option<String>,
    pub cached: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionMode {
    All,
    Include,
    Exclude,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedScanRequest {
    pub root: PathBuf,
    pub extension_mode: ExtensionMode,
    pub extensions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedFile {
    pub id: String,
    pub display_path: String,
    pub size_bytes: u64,
    pub source_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanIssue {
    pub display_path: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedScanReport {
    pub id: String,
    pub created: i64,
    pub stamp: MaintenanceStamp,
    pub request: UnusedScanRequest,
    pub files: Vec<UnusedFile>,
    pub size_bytes: u64,
    pub complete: bool,
    pub issues: Vec<ScanIssue>,
    pub stale: bool,
    pub stale_reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnusedRemovalRole {
    Selected,
    Sibling,
    FolderMetadata,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnusedRemovalFile {
    pub id: String,
    pub display_path: String,
    pub size_bytes: u64,
    pub source_sha256: String,
    pub role: UnusedRemovalRole,
    /// The Trash cannot take this file, so it is deleted permanently if the Trash refuses it.
    /// Only a file the preview listed this way is ever deleted permanently.
    #[serde(default)]
    pub permanent: bool,
}
/// What happened to one file of a removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnusedRemovalDisposal {
    /// Moved to the Trash or Recycle Bin, where the user can put it back.
    Trashed,
    /// Deleted permanently because the preview said the Trash cannot take it.
    Deleted,
    /// The Trash refused it and the preview did not announce a permanent deletion: the file
    /// is where it was.
    LeftInPlace,
    /// No longer where the removal put it when the work was resumed after an interruption.
    Gone,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedRemovalOutcome {
    pub id: String,
    pub display_path: String,
    pub disposal: UnusedRemovalDisposal,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedRemovalPreview {
    #[serde(default)]
    pub directories: Vec<String>,
    pub id: String,
    pub scan_id: String,
    pub stamp: MaintenanceStamp,
    pub files: Vec<UnusedRemovalFile>,
    pub size_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedRemovalResult {
    #[serde(default)]
    pub directories: Vec<String>,
    pub id: String,
    pub preview_id: String,
    pub stamp: MaintenanceStamp,
    pub files: Vec<UnusedRemovalFile>,
    pub size_bytes: u64,
    pub restored: bool,
    pub undo_of: Option<String>,
    /// What became of each file, once the removal has settled.
    #[serde(default)]
    pub outcomes: Vec<UnusedRemovalOutcome>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedRemovalHistory {
    pub result: UnusedRemovalResult,
    pub state: String,
    pub error: Option<String>,
    pub undone_by: Option<String>,
    pub cleanup_pending: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileScanProgress {
    pub completed: usize,
    pub total: Option<usize>,
    pub current_path: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrokenScanRequest {
    pub inspect_audio: bool,
    /// Archived records are library records too; repairing them needs them listed.
    pub include_archived: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokenScanTrack {
    #[serde(default)]
    pub archived: bool,
    pub id: String,
    pub display_path: String,
    pub state: MaintenanceFileState,
    pub detail: Option<String>,
    pub observed_sha256: Option<String>,
    pub source_changed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokenScanReport {
    pub id: String,
    pub created: i64,
    pub stamp: MaintenanceStamp,
    pub request: BrokenScanRequest,
    #[serde(default)]
    pub decoder_identity: Option<String>,
    pub tracks: Vec<BrokenScanTrack>,
    pub stale: bool,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateRecovery {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub created: i64,
    pub error: Option<String>,
    /// True while the operation had already moved audio files out of their library locations
    /// and the DJ has not acknowledged it yet. Rows without it are plain recovery failures that
    /// can be retried.
    #[serde(default)]
    pub files_moved: bool,
}
