//! Generated audio in a temporary folder, a temporary library and a stand-in Trash.
//! Nothing here touches a real library, the real Trash or any music folder.
#![allow(dead_code)]
use cuerithm_library::{
    DuplicateReview, DuplicateScanReport, DuplicateScanRequest, FileTrash, Library, PlaylistEdit,
    PlaylistKind,
};
use cuerithm_media::{CancellationToken, MediaRuntime};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub db: PathBuf,
    pub lib: Library,
    pub runtime: MediaRuntime,
    pub ids: Vec<String>,
    pub paths: Vec<PathBuf>,
}
impl Fixture {
    /// Four tracks: 0 and 1 are the same audio (1 carries extra bytes), 2 is different
    /// audio, 3 is archived. Every track is called North - Signal and is 20 seconds long.
    pub fn new() -> Self {
        let mut f = Self::empty();
        for (i, seed) in [37, 37, 43, 37].into_iter().enumerate() {
            let path = f.dir.path().join(format!("track-{i}.wav"));
            wav(&path, seed);
            if i == 1 {
                let mut bytes = std::fs::read(&path).unwrap();
                bytes.extend_from_slice(b"JUNK\x04\0\0\0tag!");
                let size = (bytes.len() - 8) as u32;
                bytes[4..8].copy_from_slice(&size.to_le_bytes());
                std::fs::write(&path, bytes).unwrap();
            }
            f.import(path);
        }
        f.lib.set_archived(&f.ids[3], true).unwrap();
        f
    }
    /// A library with no tracks.
    pub fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app/library.sqlite");
        let lib = Library::open(&db).unwrap();
        let root = std::env::var_os("CUERITHM_MEDIA_RUNTIME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin")
            });
        let runtime = MediaRuntime::from_bundle(&root).unwrap();
        Self {
            dir,
            db,
            lib,
            runtime,
            ids: vec![],
            paths: vec![],
        }
    }
    /// Add a ready 20-second track called North - Signal whose audio depends on `seed`.
    pub fn add_track(&mut self, name: &str, seed: i32) -> usize {
        let path = self.dir.path().join(name);
        wav(&path, seed);
        self.import(path)
    }
    /// Import an existing file as a ready track called North - Signal.
    pub fn import(&mut self, path: PathBuf) -> usize {
        let job = self
            .lib
            .enqueue(
                std::slice::from_ref(&path),
                self.lib.settings().unwrap().revision,
            )
            .unwrap()[0]
            .clone();
        let id = self
            .lib
            .jobs()
            .unwrap()
            .iter()
            .find(|r| r.id == job)
            .unwrap()
            .track_id
            .clone();
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        conn.execute("UPDATE tracks SET title='Signal',artist='North',duration=20,state='ready',analysis_json='{\"untouched\":true}',metadata_json='{\"codec\":\"pcm_s16le\",\"sample_rate\":8000,\"channels\":1,\"tags\":{},\"properties\":{\"audio.bit_rate\":\"128000\"}}' WHERE id=?1",[&id]).unwrap();
        // A ready track has finished its preparation job.
        conn.execute(
            "UPDATE jobs SET state='completed',stage='completed' WHERE track_id=?1",
            [&id],
        )
        .unwrap();
        self.ids.push(id);
        self.paths.push(path);
        self.ids.len() - 1
    }
    pub fn playlist(&self, id: &str, tracks: Vec<String>) {
        self.lib
            .apply_playlist_edit(
                PlaylistEdit::Create {
                    id: Some(id.into()),
                    parent_id: None,
                    kind: PlaylistKind::Playlist,
                    name: id.into(),
                    definition: None,
                },
                self.lib.playlist_state().unwrap().revision,
            )
            .unwrap();
        self.lib
            .apply_playlist_edit(
                PlaylistEdit::AddMembers {
                    id: id.into(),
                    track_ids: tracks,
                },
                self.lib.playlist_state().unwrap().revision,
            )
            .unwrap();
    }
    /// Every row that belongs to a track or its history, in a comparable form.
    pub fn content(&self) -> Vec<String> {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        // Rows are compared by their declared columns and keys: SQLite's own row numbers are
        // handed out again after a delete and mean nothing, so Undo does not restore them.
        ["SELECT json_group_array(json_array(id,hex(path),title,artist,archived,source_hash,state,analysis_json,metadata_json,current_job)) FROM (SELECT * FROM tracks ORDER BY id)","SELECT json_group_array(json_array(id,track_id,state,stage,attempts,settings_revision)) FROM (SELECT * FROM jobs ORDER BY id)","SELECT json_group_array(json_array(id,track_id,job_id,json,created)) FROM (SELECT * FROM cue_revisions ORDER BY id)","SELECT json_group_array(json_array(playlist_id,track_id,position)) FROM (SELECT * FROM playlist_members ORDER BY playlist_id,position)","SELECT json_group_array(json_array(track_id,tag_id)) FROM (SELECT * FROM track_custom_tags ORDER BY track_id,tag_id)","SELECT json_group_array(json_array(track_id,target,external_id,baseline_json)) FROM (SELECT * FROM sync_links ORDER BY track_id,target)","SELECT json_group_array(json_array(target_id,content_id,local_track_id,status)) FROM (SELECT * FROM native_track_links ORDER BY target_id,content_id)"].iter().map(|sql|conn.query_row(sql,[],|r|r.get(0)).unwrap()).collect()
    }
    /// Like [`content`](Self::content) without the columns that describe the bytes of a file: a
    /// donor copy's Undo writes the previous tag values back, so the file it leaves has the old
    /// tags but not the old bytes.
    pub fn content_ignoring_file_identity(&self) -> Vec<String> {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        ["SELECT json_group_array(json_array(id,hex(path),title,artist,archived,state,analysis_json,current_job)) FROM (SELECT * FROM tracks ORDER BY id)","SELECT json_group_array(json_array(id,track_id,state,stage,attempts,settings_revision)) FROM (SELECT * FROM jobs ORDER BY id)","SELECT json_group_array(json_array(id,track_id,job_id,json,created)) FROM (SELECT * FROM cue_revisions ORDER BY id)","SELECT json_group_array(json_array(playlist_id,track_id,position)) FROM (SELECT * FROM playlist_members ORDER BY playlist_id,position)","SELECT json_group_array(json_array(track_id,tag_id)) FROM (SELECT * FROM track_custom_tags ORDER BY track_id,tag_id)","SELECT json_group_array(json_array(track_id,target,external_id,baseline_json)) FROM (SELECT * FROM sync_links ORDER BY track_id,target)","SELECT json_group_array(json_array(target_id,content_id,local_track_id,status)) FROM (SELECT * FROM native_track_links ORDER BY target_id,content_id)"].iter().map(|sql|conn.query_row(sql,[],|r|r.get(0)).unwrap()).collect()
    }
    /// Every tag one file holds and its decoded audio.
    #[cfg(feature = "autotag")]
    pub fn file_state(&self, index: usize) -> (cuerithm_autotag::TransferSnapshot, String) {
        let path = &self.paths[index];
        let tags =
            cuerithm_autotag::read_transfer_metadata(path, &self.runtime.probe(path).unwrap())
                .unwrap();
        let cache = tempfile::tempdir().unwrap();
        let pcm = self
            .runtime
            .decode(path, cache.path())
            .unwrap()
            .pcm_sha256()
            .unwrap();
        (tags, pcm)
    }
    /// The library describes the file the track really has.
    pub fn assert_library_knows_file(&self, index: usize) {
        let recorded: String = rusqlite::Connection::open(&self.db)
            .unwrap()
            .query_row(
                "SELECT source_hash FROM tracks WHERE id=?1",
                [&self.ids[index]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(recorded, hash(&self.paths[index]));
    }
    pub fn scan(&self, tolerance: cuerithm_library::DuplicateTolerance) -> DuplicateScanReport {
        self.lib
            .scan_duplicates(
                DuplicateScanRequest {
                    tolerance,
                    ..DuplicateScanRequest::default()
                },
                &self.runtime,
                &CancellationToken::default(),
            )
            .unwrap()
    }
    /// A scan and a review of its groups.
    pub fn review(&self, tolerance: cuerithm_library::DuplicateTolerance) -> DuplicateReview {
        let scan = self.scan(tolerance);
        self.lib
            .begin_duplicate_review(&scan.id, &CancellationToken::default())
            .unwrap()
    }
    pub fn track_exists(&self, id: &str) -> bool {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }
    pub fn archived(&self, id: &str) -> bool {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        conn.query_row("SELECT archived FROM tracks WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
    }
}
pub fn wav(path: &Path, seed: i32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..160000 {
        out.write_sample::<i16>(((i * seed) % 20001 - 10000) as i16)
            .unwrap();
    }
    out.finalize().unwrap();
}
/// The same audio as `wav(path, seed)` with an extra chunk after it, so the file has bytes (and
/// so a content hash) of its own.
pub fn wav_with_extra_chunk(path: &Path, seed: i32) {
    wav(path, seed);
    let mut bytes = std::fs::read(path).unwrap();
    bytes.extend_from_slice(b"JUNK\x04\0\0\0tag!");
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}
pub fn hash(path: &Path) -> String {
    cuerithm_media::file_sha256(path).unwrap()
}

/// A Trash that is a folder in the test's own temporary directory.
pub struct FakeTrash {
    pub folder: PathBuf,
    /// File names that this Trash refuses to take.
    pub refuse: Mutex<BTreeSet<String>>,
    /// File names for which this Trash says in advance that it cannot be used.
    pub unavailable: Mutex<BTreeSet<String>>,
    pub taken: Mutex<Vec<String>>,
    pub deleted: Mutex<Vec<String>>,
}
impl FakeTrash {
    pub fn new(folder: PathBuf) -> Self {
        std::fs::create_dir_all(&folder).unwrap();
        Self {
            folder,
            refuse: Mutex::default(),
            unavailable: Mutex::default(),
            taken: Mutex::default(),
            deleted: Mutex::default(),
        }
    }
    /// The Trash is expected to be unusable for this file name (before any attempt).
    pub fn unavailable(&self, name: &str) {
        self.unavailable.lock().unwrap().insert(name.into());
    }
    pub fn available_again(&self, name: &str) {
        self.unavailable.lock().unwrap().remove(name);
    }
    /// File names deleted permanently, in order.
    pub fn deleted(&self) -> Vec<String> {
        self.deleted.lock().unwrap().clone()
    }
    pub fn refuse(&self, name: &str) {
        self.refuse.lock().unwrap().insert(name.into());
    }
    pub fn taken(&self) -> Vec<String> {
        self.taken.lock().unwrap().clone()
    }
}
/// A Trash that is always expected to work; used where a preview must not depend on the
/// machine's real volumes. It must never be asked to move a file.
pub struct Capable;
impl FileTrash for Capable {
    fn move_to_trash(&self, _: &Path) -> Result<(), String> {
        unreachable!("previews never move files")
    }
}

impl FileTrash for FakeTrash {
    fn can_trash(&self, path: &Path) -> bool {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        !self.unavailable.lock().unwrap().contains(&name)
    }
    fn delete_permanently(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
        self.deleted.lock().unwrap().push(name);
        Ok(())
    }
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if self.refuse.lock().unwrap().contains(&name) {
            return Err("The Trash is not available".into());
        }
        std::fs::rename(path, self.folder.join(&name)).map_err(|e| e.to_string())?;
        self.taken.lock().unwrap().push(name);
        Ok(())
    }
}
