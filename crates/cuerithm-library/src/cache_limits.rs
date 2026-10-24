//! Bounds on the caches in the data folder.
//!
//! Everything here is rebuilt on demand, so a bound only costs work, never data:
//!
//! - **Waveforms** (`cache/waveforms`, one file per source file, decoder and resolution). A file
//!   that is tagged again has a new hash and leaves the waveforms of its old bytes behind. The
//!   folder is kept under a size limit by deleting the files that were used longest ago.
//! - **Scratch** (`cache/fingerprints`, `cache/dj-timing`, `cache/media`). Scans and syncs work in
//!   temporary folders that they remove themselves; a crash leaves one behind. A folder untouched
//!   for a day cannot belong to a running operation and is removed.
//!
//! Cover art is not trimmed here: it is the one cache the library keeps on purpose (see
//! `maintenance/purge.rs`).
use crate::Library;
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

/// The waveform cache is trimmed when it grows past this size...
pub const WAVEFORM_CACHE_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
/// ...down to this size, so that one new waveform does not trim again.
pub const WAVEFORM_CACHE_TARGET_BYTES: u64 = 192 * 1024 * 1024;
/// How old a scratch folder is before it can only be a crash leftover.
pub const SCRATCH_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Entries looked at per folder, so that a surprise folder cannot stall opening the library.
const MAX_ENTRIES: usize = 200_000;

/// What a trim removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheTrim {
    pub files_removed: u64,
    pub bytes_freed: u64,
}

fn is_cache_name(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|c| c.is_ascii_hexdigit()))
}

/// Deletes the least recently used waveform files until the folder is within `target`, once it
/// is over `limit`. Only files this application names (`<sha256>.json`) are touched, never links.
pub(crate) fn trim_waveforms(dir: &Path, limit: u64, target: u64) -> CacheTrim {
    let Ok(rows) = std::fs::read_dir(dir) else {
        return CacheTrim::default();
    };
    let mut files: Vec<(SystemTime, u64, std::path::PathBuf)> = vec![];
    let mut total = 0u64;
    for row in rows.flatten().take(MAX_ENTRIES) {
        let path = row.path();
        let name = row.file_name();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || !is_cache_name(&name.to_string_lossy()) {
            continue;
        }
        total += meta.len();
        files.push((
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
            path,
        ));
    }
    let mut trim = CacheTrim::default();
    if total <= limit {
        return trim;
    }
    files.sort();
    for (_, size, path) in files {
        if total <= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
            trim.files_removed += 1;
            trim.bytes_freed += size;
        }
    }
    trim
}

/// Deletes the entries of a scratch folder that nothing has touched for `max_age`.
pub(crate) fn sweep_scratch(dir: &Path, max_age: Duration) -> CacheTrim {
    let Ok(rows) = std::fs::read_dir(dir) else {
        return CacheTrim::default();
    };
    let mut trim = CacheTrim::default();
    let now = SystemTime::now();
    for row in rows.flatten().take(MAX_ENTRIES) {
        let path = row.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let idle = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if idle.is_none_or(|idle| idle < max_age) {
            continue;
        }
        let size = size_of(&path);
        let removed = if meta.is_dir() && !meta.file_type().is_symlink() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if removed.is_ok() {
            trim.files_removed += 1;
            trim.bytes_freed += size;
        }
    }
    trim
}
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        0
    } else if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|rows| rows.flatten().map(|row| size_of(&row.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// Marks a waveform file as used now, so that trimming deletes the ones nobody has asked for.
pub(crate) fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

impl Library {
    /// Applies the cache bounds. Run when the library opens; a failure is not an error because
    /// the next start tries again.
    pub(crate) fn trim_caches(&self) -> CacheTrim {
        let cache = self.data_dir().join("cache");
        let mut total = trim_waveforms(
            &cache.join("waveforms"),
            WAVEFORM_CACHE_LIMIT_BYTES,
            WAVEFORM_CACHE_TARGET_BYTES,
        );
        for scratch in ["fingerprints", "dj-timing", "media"] {
            let part = sweep_scratch(&cache.join(scratch), SCRATCH_MAX_AGE);
            total.files_removed += part.files_removed;
            total.bytes_freed += part.bytes_freed;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: u8) -> String {
        format!("{:02x}{}.json", n, "ab".repeat(31))
    }
    fn write(dir: &Path, file: &str, mib: usize, age_hours: u64) {
        let path = dir.join(file);
        std::fs::write(&path, vec![1u8; mib * 1024 * 1024]).unwrap();
        let modified = SystemTime::now() - Duration::from_secs(age_hours * 3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }
    const MIB: u64 = 1024 * 1024;

    #[test]
    fn the_waveform_cache_stays_under_its_limit_by_dropping_the_least_recently_used() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..6u8 {
            // File 0 is the oldest.
            write(dir.path(), &name(n), 1, 100 - u64::from(n));
        }
        // Older than every cache file, and not named like one: it must stay.
        std::fs::write(dir.path().join("notes.txt"), b"not a cache file").unwrap();
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("notes.txt"))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(500 * 3600))
            .unwrap();
        // Under the limit nothing happens.
        assert_eq!(
            trim_waveforms(dir.path(), 6 * MIB, 3 * MIB),
            CacheTrim::default()
        );
        // Over it, the oldest go until the target is reached.
        let trim = trim_waveforms(dir.path(), 5 * MIB, 3 * MIB);
        assert_eq!(trim.files_removed, 3);
        assert_eq!(trim.bytes_freed, 3 * MIB);
        for n in 0..3u8 {
            assert!(!dir.path().join(name(n)).exists(), "file {n} is oldest");
        }
        for n in 3..6u8 {
            assert!(dir.path().join(name(n)).exists(), "file {n} is recent");
        }
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn a_file_that_was_used_recently_outlives_an_older_one_written_later() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &name(0), 1, 50);
        write(dir.path(), &name(1), 1, 10);
        // The oldest one is played again.
        touch(&dir.path().join(name(0)));
        trim_waveforms(dir.path(), MIB, MIB);
        assert!(dir.path().join(name(0)).exists());
        assert!(!dir.path().join(name(1)).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_waveform_folder_is_never_followed_or_counted() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "keep.bin", 4, 100);
        std::os::unix::fs::symlink(outside.path().join("keep.bin"), dir.path().join(name(9)))
            .unwrap();
        assert_eq!(trim_waveforms(dir.path(), 1, 0), CacheTrim::default());
        assert!(outside.path().join("keep.bin").exists());
    }

    #[test]
    fn scratch_folders_left_by_a_crash_go_but_a_recent_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("scan-old");
        let recent = dir.path().join("scan-recent");
        for folder in [&old, &recent] {
            std::fs::create_dir(folder).unwrap();
            std::fs::write(folder.join("work"), b"scratch").unwrap();
        }
        std::fs::File::open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3 * 24 * 3600))
            .unwrap();
        let trim = sweep_scratch(dir.path(), SCRATCH_MAX_AGE);
        assert_eq!(trim.files_removed, 1);
        assert_eq!(trim.bytes_freed, 7);
        assert!(!old.exists() && recent.join("work").exists());
    }

    #[test]
    fn opening_a_library_trims_its_caches() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app/db");
        drop(Library::open(&db).unwrap());
        let lib = Library::open(&db).unwrap();
        let waves = lib.data_dir().join("cache/waveforms");
        let scratch = lib.data_dir().join("cache/fingerprints/crashed");
        std::fs::create_dir_all(&waves).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::File::open(&scratch)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(48 * 3600))
            .unwrap();
        for n in 0..3u8 {
            std::fs::write(waves.join(name(n)), b"{}").unwrap();
        }
        drop(lib);
        let lib = Library::open(&db).unwrap();
        // Small files are under the limit; the crashed scratch folder is gone.
        assert_eq!(std::fs::read_dir(&waves).unwrap().count(), 3);
        assert!(!lib.data_dir().join("cache/fingerprints/crashed").exists());
    }
}
