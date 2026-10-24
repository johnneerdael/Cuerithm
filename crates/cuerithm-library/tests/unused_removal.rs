use cuerithm_library::{
    ExtensionMode, FileTrash, Library, UnusedRemovalDisposal, UnusedRemovalPreview,
    UnusedRemovalResult, UnusedRemovalRole, UnusedScanRequest,
};
use cuerithm_media::CancellationToken;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A Trash that is a folder in the test's own temporary directory.
struct Bin {
    folder: PathBuf,
    taken: Mutex<Vec<String>>,
}
impl Bin {
    fn taken(&self) -> Vec<String> {
        let mut names = self.taken.lock().unwrap().clone();
        names.sort();
        names
    }
}
impl FileTrash for Bin {
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::rename(path, self.folder.join(&name)).map_err(|e| e.to_string())?;
        self.taken.lock().unwrap().push(name);
        Ok(())
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    lib: Library,
    db: PathBuf,
    music: PathBuf,
    bin: Bin,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let db = dir.path().join("app/db");
        let lib = Library::open(&db).unwrap();
        let folder = dir.path().join("bin");
        std::fs::create_dir(&folder).unwrap();
        Self {
            dir,
            lib,
            db,
            music,
            bin: Bin {
                folder,
                taken: Mutex::default(),
            },
        }
    }
    fn preview(
        &self,
        scan_id: &str,
        selected: &[String],
        token: &CancellationToken,
    ) -> anyhow::Result<UnusedRemovalPreview> {
        self.lib
            .preview_unused_removal_with(scan_id, selected, &self.bin, token)
    }
    fn apply(&self, id: &str, token: &CancellationToken) -> anyhow::Result<UnusedRemovalResult> {
        self.lib.apply_unused_removal_with(id, &self.bin, token)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.music.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn scan(&self) -> cuerithm_library::UnusedScanReport {
        self.lib
            .scan_unused_files(
                UnusedScanRequest {
                    root: self.music.clone(),
                    extension_mode: ExtensionMode::Include,
                    extensions: vec!["flac".into()],
                },
                &CancellationToken::default(),
            )
            .unwrap()
    }
    fn add(&self, path: &Path) {
        self.lib
            .enqueue(&[path.to_owned()], self.lib.settings().unwrap().revision)
            .unwrap();
    }
    fn state(&self) -> serde_json::Value {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        let mut result = serde_json::Map::new();
        for table in ["tracks", "jobs", "cue_revisions", "playlist_members"] {
            let mut q = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = q.column_count();
            let rows = q
                .query_map([], |row| {
                    Ok((0..columns)
                        .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                        .collect::<Vec<_>>())
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            result.insert(table.into(), serde_json::json!(rows));
        }
        result.into()
    }
}
#[test]
fn preview_names_selected_files_and_only_the_first_lexicon_sibling_without_removing_anything() {
    let f = Fixture::new();
    let source = f.file("nested/loose.flac", b"authored source");
    let serato = f.file("nested/loose.1.2.serato-stems", b"first sibling");
    let vdj = f.file("nested/loose.flac.vdjstems", b"second sibling");
    let kept = f.file("kept.flac", b"referenced");
    f.add(&kept);
    let before = f.state();
    let scan = f.scan();
    assert_eq!(scan.files.len(), 1);
    let preview = f
        .preview(
            &scan.id,
            &[scan.files[0].id.clone()],
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(preview.files.len(), 2);
    assert_eq!(preview.files[0].role, UnusedRemovalRole::Selected);
    assert_eq!(preview.files[1].role, UnusedRemovalRole::Sibling);
    assert!(
        preview.files[1]
            .display_path
            .ends_with("loose.1.2.serato-stems")
    );
    assert_eq!(preview.size_bytes, 28);
    assert!(source.exists() && serato.exists() && vdj.exists() && kept.exists());
    assert_eq!(f.state(), before);
}
#[test]
fn a_reviewed_removal_moves_the_files_to_the_trash_leaves_the_library_alone_and_keeps_no_copy() {
    let mut f = Fixture::new();
    let source = f.file("nested/loose.flac", b"source bytes");
    let sibling = f.file("nested/loose.flac.vdjstems", b"stem bytes");
    let kept = f.file("kept.flac", b"live");
    f.add(&kept);
    let before = f.state();
    let scan = f.scan();
    let token = CancellationToken::default();
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    assert!(preview.files.iter().all(|file| !file.permanent));
    let applied = f.apply(&preview.id, &token).unwrap();
    assert!(!source.exists() && !sibling.exists());
    assert_eq!(std::fs::read(&kept).unwrap(), b"live");
    assert_eq!(f.state(), before);
    // The Trash has both files under their own names; nothing is kept in the data folder.
    assert_eq!(f.bin.taken(), ["loose.flac", "loose.flac.vdjstems"]);
    assert_eq!(
        std::fs::read(f.bin.folder.join("loose.flac")).unwrap(),
        b"source bytes"
    );
    assert!(
        applied
            .outcomes
            .iter()
            .all(|o| o.disposal == UnusedRemovalDisposal::Trashed)
    );
    assert!(!f.dir.path().join("app/maintenance/removal").exists());
    assert!(f.apply(&preview.id, &token).is_err());
    drop(f.lib);
    f.lib = Library::open(&f.db).unwrap();
    let history = f.lib.unused_removal_history().unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.result.id == applied.id && h.state == "committed" && !h.cleanup_pending)
    );
    assert!(f.dir.path().exists());
}

#[test]
fn stale_and_partial_reports_unknown_ids_and_referenced_siblings_cannot_be_removed() {
    let f = Fixture::new();
    let a = f.file("loose.flac", b"source");
    let sibling = f.file("loose.flac.vdjstems", b"sidecar");
    f.add(&sibling);
    let token = CancellationToken::default();
    let scan = f.scan();
    assert!(
        f.preview(&scan.id, &[scan.files[0].id.clone()], &token)
            .is_err()
    );
    assert!(f.preview(&scan.id, &["unknown".into()], &token).is_err());
    assert!(f.preview(&scan.id, &[], &token).is_err());
    assert!(a.exists() && sibling.exists());
    let f = Fixture::new();
    let path = f.file("loose.flac", b"source");
    let scan = f.scan();
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    std::fs::write(&path, b"later!").unwrap();
    assert!(f.apply(&preview.id, &token).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"later!");
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("UPDATE maintenance_scans SET payload_json=json_set(payload_json,'$.report.complete',json('false')) WHERE id=?1",[&scan.id]).unwrap();
    assert!(
        f.preview(&scan.id, &[scan.files[0].id.clone()], &token)
            .is_err()
    );
}

#[test]
fn later_library_references_and_same_content_replacements_invalidate_the_plan() {
    for replace in [false, true] {
        let f = Fixture::new();
        let path = f.file("loose.flac", b"source");
        let token = CancellationToken::default();
        let scan = f.scan();
        let preview = f
            .preview(&scan.id, &[scan.files[0].id.clone()], &token)
            .unwrap();
        if replace {
            let newer = f.file("replacement", b"source");
            std::fs::remove_file(&path).unwrap();
            std::fs::rename(newer, &path).unwrap();
        } else {
            f.add(&path);
        }
        assert!(f.apply(&preview.id, &token).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"source");
    }
}

#[test]
fn sql_failure_and_cancellation_leave_all_sources_and_library_state_intact() {
    let f = Fixture::new();
    let a = f.file("a.flac", b"first");
    let b = f.file("b.flac", b"second");
    let token = CancellationToken::default();
    let scan = f.scan();
    let selected = scan.files.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
    let preview = f.preview(&scan.id, &selected, &token).unwrap();
    let before = f.state();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_unused_commit BEFORE UPDATE ON maintenance_scans WHEN NEW.kind='unused_removal_operation' AND json_extract(NEW.payload_json,'$.state')='committed' BEGIN SELECT RAISE(ABORT,'injected unused commit failure'); END;").unwrap();
    assert!(f.apply(&preview.id, &token).is_err());
    assert_eq!(std::fs::read(&a).unwrap(), b"first");
    assert_eq!(std::fs::read(&b).unwrap(), b"second");
    assert_eq!(f.state(), before);
    conn.execute_batch("DROP TRIGGER fail_unused_commit;")
        .unwrap();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(f.apply(&preview.id, &cancelled).is_err());
    assert!(a.exists() && b.exists());
    let applied = f.apply(&preview.id, &token).unwrap();
    assert_eq!(applied.outcomes.len(), 2);
    assert!(!a.exists() && !b.exists());
    assert_eq!(f.bin.taken(), ["a.flac", "b.flac"]);
}

#[cfg(unix)]
#[test]
fn symlinked_parents_and_referenced_hardlink_aliases_are_never_removed() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let path = f.file("nested/loose.flac", b"source");
    let scan = f.scan();
    let token = CancellationToken::default();
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    let outside = f.dir.path().join("outside");
    std::fs::rename(f.music.join("nested"), &outside).unwrap();
    symlink(&outside, f.music.join("nested")).unwrap();
    assert!(f.apply(&preview.id, &token).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"source");
    let f = Fixture::new();
    let path = f.file("loose.flac", b"same");
    let reference = f.file("kept.dat", b"same");
    f.add(&reference);
    let scan = f.scan();
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::hard_link(&reference, &path).unwrap();
    assert!(f.apply(&preview.id, &token).is_err());
    assert!(path.exists() && reference.exists());
}

#[test]
fn reviewed_empty_parent_and_metadata_are_removed_with_the_files() {
    let f = Fixture::new();
    let audio = f.file("album/loose.flac", b"authored audio");
    let metadata = f.file("album/.DS_Store", b"authored folder metadata");
    let keep = f.file("keep.txt", b"keep");
    let scan = f.scan();
    let token = CancellationToken::default();
    let plan = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    assert_eq!(
        plan.files.len(),
        2,
        "Folder metadata must be included in the review"
    );
    assert_eq!(plan.directories.len(), 1);
    assert!(
        plan.files
            .iter()
            .any(|p| p.role == UnusedRemovalRole::FolderMetadata)
    );
    assert!(audio.exists() && metadata.exists());
    f.apply(&plan.id, &token).unwrap();
    assert!(!audio.parent().unwrap().exists());
    assert!(f.music.exists());
    assert_eq!(f.bin.taken(), [".DS_Store", "loose.flac"]);
    assert_eq!(
        std::fs::read(f.bin.folder.join("loose.flac")).unwrap(),
        b"authored audio"
    );
    assert_eq!(std::fs::read(&keep).unwrap(), b"keep");
}
#[test]
fn folder_cleanup_keeps_source_sidecar_order_cover_art_and_the_reviewed_root() {
    for sidecar in [true, false] {
        let f = Fixture::new();
        let path = f.file("album/loose.flac", b"source");
        let extra = if sidecar {
            f.file("album/loose.flac.vdjstems", b"stem")
        } else {
            f.file("album/cover.jpg", b"art")
        };
        let scan = f.scan();
        let token = CancellationToken::default();
        let plan = f
            .preview(&scan.id, &[scan.files[0].id.clone()], &token)
            .unwrap();
        assert!(plan.directories.is_empty());
        f.apply(&plan.id, &token).unwrap();
        assert!(path.parent().unwrap().is_dir());
        assert_eq!(extra.exists(), !sidecar);
    }
    let f = Fixture::new();
    f.file("loose.flac", b"root child");
    let scan = f.scan();
    let token = CancellationToken::default();
    let plan = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    assert!(plan.directories.is_empty());
    f.apply(&plan.id, &token).unwrap();
    assert!(f.music.is_dir());
}
#[test]
fn selection_order_can_cleanup_nested_parents_deepest_first() {
    let f = Fixture::new();
    let child = f.file("album/sub/child.flac", b"child");
    let parent = f.file("album/parent.flac", b"parent");
    let scan = f.scan();
    let token = CancellationToken::default();
    let selected = [&child, &parent]
        .into_iter()
        .map(|p| {
            scan.files
                .iter()
                .find(|v| Path::new(&v.display_path).file_name() == p.file_name())
                .unwrap()
                .id
                .clone()
        })
        .collect::<Vec<_>>();
    let plan = f.preview(&scan.id, &selected, &token).unwrap();
    assert_eq!(plan.directories.len(), 2);
    f.apply(&plan.id, &token).unwrap();
    assert!(!child.parent().unwrap().exists());
    assert!(!parent.parent().unwrap().exists());
    assert_eq!(f.bin.taken(), ["child.flac", "parent.flac"]);
}

#[test]
fn extension_filters_do_not_make_associated_stems_eligible_for_unused_removal() {
    for extension_mode in [ExtensionMode::Include, ExtensionMode::Exclude] {
        let f = Fixture::new();
        let kept = f.file("album/kept.flac", b"referenced audio");
        let kept_stems = f.file("album/kept.flac.vdjstems", b"referenced track stem data");
        let kept_serato = f.file(
            "album/kept.1.2.serato-stems",
            b"referenced Serato stem data",
        );
        let loose = f.file("album/loose.wav", b"unreferenced base audio");
        let loose_stems = f.file(
            "album/loose.wav.vdjstems",
            b"associated unreferenced stem data",
        );
        let orphan = f.file("album/orphan.vdjstems", b"orphan stem data");
        let orphan_serato = f.file("album/stray.1.2.serato-stems", b"orphan Serato data");
        f.add(&kept);
        let before = f.state();
        let protected = [&kept, &kept_stems, &kept_serato, &loose, &loose_stems];
        let bytes = protected
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect::<Vec<_>>();
        let token = CancellationToken::default();
        let scan = f
            .lib
            .scan_unused_files(
                UnusedScanRequest {
                    root: f.music.clone(),
                    extension_mode,
                    extensions: if extension_mode == ExtensionMode::Include {
                        vec!["vdjstems".into()]
                    } else {
                        vec!["flac".into(), "wav".into()]
                    },
                },
                &token,
            )
            .unwrap();
        assert!(scan.complete && !scan.stale);
        let mut expected = vec![
            orphan
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ];
        if extension_mode == ExtensionMode::Exclude {
            expected.push(
                orphan_serato
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        expected.sort();
        assert_eq!(
            scan.files
                .iter()
                .map(|file| file.display_path.clone())
                .collect::<Vec<_>>(),
            expected
        );
        let preview = f
            .preview(
                &scan.id,
                &scan
                    .files
                    .iter()
                    .map(|file| file.id.clone())
                    .collect::<Vec<_>>(),
                &token,
            )
            .unwrap();
        assert_eq!(preview.files.len(), expected.len());
        f.apply(&preview.id, &token).unwrap();
        assert!(!orphan.exists());
        assert_eq!(
            orphan_serato.exists(),
            extension_mode == ExtensionMode::Include
        );
        for (path, original) in protected.iter().zip(bytes) {
            assert_eq!(
                std::fs::read(path).unwrap(),
                original,
                "associated source changed: {}",
                path.display()
            );
        }
        assert_eq!(f.state(), before);
        assert_eq!(
            std::fs::read(f.bin.folder.join("orphan.vdjstems")).unwrap(),
            b"orphan stem data"
        );
    }
}

#[test]
fn an_excluded_base_added_after_review_blocks_orphan_sidecar_removal() {
    let f = Fixture::new();
    let stems = f.file("late.flac.vdjstems", b"previously orphan stems");
    let token = CancellationToken::default();
    let scan = f
        .lib
        .scan_unused_files(
            UnusedScanRequest {
                root: f.music.clone(),
                extension_mode: ExtensionMode::Include,
                extensions: vec!["vdjstems".into()],
            },
            &token,
        )
        .unwrap();
    assert_eq!(scan.files.len(), 1);
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    let audio = f.file("late.flac", b"newly present excluded base audio");
    assert!(f.lib.unused_scan(&scan.id).unwrap().stale);
    assert!(f.apply(&preview.id, &token).is_err());
    assert_eq!(std::fs::read(&stems).unwrap(), b"previously orphan stems");
    assert_eq!(
        std::fs::read(&audio).unwrap(),
        b"newly present excluded base audio"
    );
}

#[test]
fn saved_unused_scan_and_removal_plan_from_old_scanner_logic_cannot_be_applied() {
    let f = Fixture::new();
    let audio = f.file("loose.flac", b"authored audio");
    let token = CancellationToken::default();
    let scan = f.scan();
    let preview = f
        .preview(&scan.id, &[scan.files[0].id.clone()], &token)
        .unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    for id in [&scan.id, &preview.id] {
        conn.execute("UPDATE maintenance_scans SET payload_json=json_set(payload_json,'$.logic','old-filtered-sidecar-inventory') WHERE id=?1", [id]).unwrap();
    }
    assert!(f.lib.unused_scan(&scan.id).unwrap().stale);
    assert!(
        f.preview(&scan.id, &[scan.files[0].id.clone()], &token)
            .is_err()
    );
    assert!(f.apply(&preview.id, &token).is_err());
    assert_eq!(std::fs::read(&audio).unwrap(), b"authored audio");
    assert!(f.lib.unused_removal_history().unwrap().is_empty());
}
