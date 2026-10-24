use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncTarget {
    pub id: String,
    pub installation: TargetInstallation,
    pub database: PathBuf,
    pub options_file: PathBuf,
    pub asset_root: PathBuf,
    #[serde(default, skip_serializing_if = "TargetEnvironment::is_native")]
    pub environment: TargetEnvironment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RekordboxManualTarget {
    pub executable: PathBuf,
    pub version_hint: Option<String>,
    pub database: PathBuf,
    pub options_file: PathBuf,
    pub asset_root: PathBuf,
    #[serde(default)]
    pub environment: TargetEnvironment,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum RekordboxSyncScope {
    Full,
    Playlist { playlist_id: String },
    Changed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RekordboxSyncOptions {
    pub metadata_fields: BTreeSet<String>,
    pub cues: bool,
    pub beat_grid: bool,
    pub artwork: bool,
    pub playlists: bool,
    pub my_tags: bool,
    pub materialize_smart_playlists: bool,
    pub convert_incompatible_audio: bool,
    pub nearest_color: bool,
}
impl Default for RekordboxSyncOptions {
    fn default() -> Self {
        Self {
            metadata_fields: [
                "title",
                "artist",
                "album",
                "album_artist",
                "genre",
                "label",
                "comment",
                "remixer",
                "composer",
                "lyricist",
                "mix_name",
                "key",
                "bpm",
                "rating",
                "release_year",
                "release_date",
                "track_number",
                "disc_number",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            cues: true,
            beat_grid: true,
            artwork: true,
            playlists: true,
            my_tags: true,
            materialize_smart_playlists: false,
            convert_incompatible_audio: false,
            nearest_color: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RekordboxSyncRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SyncConfigurationBinding>,
    pub target_id: String,
    pub scope: RekordboxSyncScope,
    pub options: RekordboxSyncOptions,
    #[serde(default)]
    pub decisions: BTreeMap<String, BTreeMap<String, NativeFieldDecision>>,
}
/// Runtime resources are deliberately never serialized in a plan or settings.
pub struct RekordboxSyncRuntime {
    pub discovery_paths: DiscoveryPaths,
    pub media_runtime: Option<cuerithm_media::MediaRuntime>,
}
impl RekordboxSyncRuntime {
    pub fn current(media: Option<cuerithm_media::MediaRuntime>) -> Result<Self> {
        Ok(Self {
            discovery_paths: DiscoveryPaths::current()?,
            media_runtime: media,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncPreflight {
    pub target: RekordboxSyncTarget,
    pub readable: bool,
    pub writable: bool,
    pub restrictions: Vec<String>,
    pub schema_fingerprint: Option<String>,
    pub identity: Option<DatabaseIdentity>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncFieldReview {
    pub track_id: String,
    pub title: String,
    /// A missing destination track is an addition, not an unreadable field.
    #[serde(default)]
    pub new_track: bool,
    pub field: String,
    pub local: NativeFieldObservation,
    pub remote: NativeFieldObservation,
    pub result: NativeReconciliation,
    pub decision: Option<NativeFieldDecision>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RekordboxEntityKind {
    Playlist,
    Folder,
    Category,
    Tag,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RekordboxEntityAction {
    Create,
    Update,
    Remove,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RekordboxEntityOwnership {
    New,
    Owned,
    Matched,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RekordboxEntityMember {
    pub title: String,
    pub position: Option<i64>,
    pub external: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RekordboxEntityRepresentation {
    Folder,
    Ordinary,
    NativeSmart,
    Category,
    Tag,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RekordboxEntityState {
    pub name: String,
    pub parent_path: Vec<String>,
    pub representation: RekordboxEntityRepresentation,
    /// Native, one-based sibling position.
    pub position: i64,
    pub member_count: usize,
    /// Bounded excerpt in actual native order; member_count includes all members.
    pub members: Vec<RekordboxEntityMember>,
    pub external_member_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxEntityChange {
    pub local_id: String,
    pub kind: RekordboxEntityKind,
    pub action: RekordboxEntityAction,
    pub ownership: RekordboxEntityOwnership,
    pub materialized_smart: bool,
    pub before: Option<RekordboxEntityState>,
    pub after: Option<RekordboxEntityState>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxPendingEntity {
    pub kind: RekordboxEntityKind,
    pub name: String,
    pub unresolved_members: usize,
    pub member_titles: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncPreview {
    pub id: String,
    pub target_id: String,
    pub library_revision: u64,
    pub fields: Vec<RekordboxSyncFieldReview>,
    #[serde(default)]
    pub entity_changes: Vec<RekordboxEntityChange>,
    #[serde(default)]
    pub pending_entities: Vec<RekordboxPendingEntity>,
    pub tracks: usize,
    pub changed_rows: usize,
    pub skipped_fields: usize,
    pub warnings: Vec<String>,
    pub can_apply: bool,
    pub media_copies: usize,
    pub copied_audio_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncOutcome {
    pub id: String,
    pub status: String,
    pub tracks: usize,
    pub changed_rows: usize,
    pub acknowledged_fields: usize,
    pub skipped_fields: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RekordboxSyncHistoryEntry {
    pub id: String,
    pub target_id: String,
    pub status: String,
    pub created: i64,
    pub updated: i64,
    pub outcome: Option<RekordboxSyncOutcome>,
    /// False once the sync is older than the ten kept for restore: only its summary remains.
    pub restore_available: bool,
}
