//! Keeping the library database small: a bounded write-ahead log and reclaimable space.
//!
//! The library is a lean index of tags, file locations and artwork. This module holds the
//! parts of that policy that concern the SQLite file itself. Nothing here changes what a
//! row means.
//!
//! - **Write-ahead log.** SQLite appends every change to `library.sqlite-wal` and folds it
//!   back into the database at a checkpoint. A single large transaction (a DJ-library
//!   import, a removal) can leave the log as large as the change, and SQLite keeps the file
//!   that size until it is told otherwise. [`WAL_SIZE_LIMIT_BYTES`] makes SQLite shrink the
//!   file after a checkpoint, and [`Library::checkpoint_wal`] truncates it explicitly after
//!   large operations and on a clean shutdown.
//! - **Free space.** Deleting rows leaves free pages inside the database file, and SQLite never
//!   returns them to the system by itself. A library created by this version uses incremental
//!   auto-vacuum, and [`Library::reclaim_storage`] hands free pages back in bounded steps. An
//!   older library is converted once, by a single `VACUUM`, which rewrites the live data: it
//!   runs only when the data is small enough for that to take seconds and the disk has room for
//!   the rewrite.
use crate::Library;
use anyhow::{Context, Result};
use rusqlite::Connection;

/// After a checkpoint empties the log, SQLite truncates the file to at most this many bytes.
pub const WAL_SIZE_LIMIT_BYTES: i64 = 16 * 1024 * 1024;
/// A checkpoint is attempted automatically once the log holds this many pages (4 MiB).
pub const WAL_AUTOCHECKPOINT_PAGES: i64 = 1000;

/// Free pages handed back to the system in one step of [`Library::reclaim_storage`] (100 MiB).
pub const RECLAIM_STEP_PAGES: i64 = 25_600;
/// The one-time conversion rewrites the live data; the explicit reclaim does it for libraries up
/// to this size.
const CONVERSION_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// While the library opens, the conversion runs before the window can appear, so it is held to
/// a size that costs about a second. Measured on a fast SSD with the rewrite warm in the cache:
/// 0.7 s for 256 MiB of live data and 2.4 s for 1 GiB, linear, so 2 GiB would be 5 s or more and
/// longer on a slower disk. A larger library is converted by the explicit
/// [`Library::reclaim_storage`].
pub const OPEN_CONVERSION_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
#[cfg(test)]
thread_local! {
    /// A test cannot fill half a gigabyte; it stands in a smaller limit for the opening one.
    static OPEN_LIMIT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}
fn open_conversion_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = OPEN_LIMIT.with(|limit| limit.get()) {
        return limit;
    }
    OPEN_CONVERSION_LIMIT_BYTES
}
/// A library whose free pages fall below this size is left alone by the automatic step.
const WORTH_RECLAIMING_BYTES: u64 = 8 * 1024 * 1024;

/// The outcome of [`Library::checkpoint_wal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct WalCheckpoint {
    /// True when a reader or writer outside this connection kept the log from being emptied.
    pub busy: bool,
    /// Pages the log held when the checkpoint began (`-1` when the database is not in WAL
    /// mode).
    pub log_pages: i64,
    /// Pages moved into the main database file.
    pub checkpointed_pages: i64,
}

/// Applies the log bounds to a freshly opened connection.
pub(crate) fn configure_wal(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES)?;
    Ok(())
}

pub(crate) fn checkpoint(conn: &Connection) -> Result<WalCheckpoint> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        Ok(WalCheckpoint {
            busy: r.get::<_, i64>(0)? != 0,
            log_pages: r.get(1)?,
            checkpointed_pages: r.get(2)?,
        })
    })
    .context("Checkpoint the library write-ahead log")
}

impl Library {
    /// Folds the write-ahead log into the database and truncates its file. Run it after a
    /// large operation and on a clean shutdown.
    ///
    /// It takes the library's one connection, not the operation gate, so it works while an
    /// exclusive operation holds its lease on another thread: a checkpoint changes no row.
    /// A transaction that is open on this connection makes it report `busy` instead of
    /// failing.
    pub fn checkpoint_wal(&self) -> Result<WalCheckpoint> {
        let conn = self
            .inner
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Library connection interrupted"))?;
        checkpoint(&conn)
    }

    /// [`Library::checkpoint_wal`] for use after an operation that has already succeeded:
    /// it never waits for the connection and never reports a failure, because a log that
    /// stays a little longer is harmless and is truncated by the next call.
    pub(crate) fn checkpoint_wal_when_idle(&self) {
        if let Ok(conn) = self.inner.conn.try_lock() {
            let _ = checkpoint(&conn);
        }
    }
}

/// How big the library's files are and how much of the database is free space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct StorageReport {
    pub database_bytes: u64,
    /// Bytes of the database file that hold no data and could be given back.
    pub free_bytes: u64,
    pub wal_bytes: u64,
    /// True once the database hands free pages back incrementally.
    pub incremental_vacuum: bool,
}

/// What [`Library::reclaim_storage`] did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StorageReclaim {
    pub before: StorageReport,
    pub after: StorageReport,
    /// True when this call rewrote the database once to enable incremental auto-vacuum.
    pub converted: bool,
    /// Why the one-time conversion did not run, when it did not.
    pub skipped: Option<String>,
}

fn pragma_i64(conn: &Connection, name: &str) -> Result<i64> {
    Ok(conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))?)
}
fn file_len(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Marks a database that has no tables yet so that it frees pages incrementally.
pub(crate) fn enable_incremental_vacuum(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "auto_vacuum", 2)?;
    Ok(())
}

fn report(conn: &Connection, database: &std::path::Path) -> Result<StorageReport> {
    let page = pragma_i64(conn, "page_size")? as u64;
    Ok(StorageReport {
        database_bytes: pragma_i64(conn, "page_count")? as u64 * page,
        free_bytes: pragma_i64(conn, "freelist_count")? as u64 * page,
        wal_bytes: file_len(&database.with_file_name(format!(
            "{}-wal",
            database.file_name().and_then(|n| n.to_str()).unwrap_or("library.sqlite")
        ))),
        incremental_vacuum: pragma_i64(conn, "auto_vacuum")? == 2,
    })
}

impl Library {
    fn database_path(&self, conn: &Connection) -> std::path::PathBuf {
        conn.path()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.inner.root.join("library.sqlite"))
    }

    pub fn storage_report(&self) -> Result<StorageReport> {
        let conn = self.conn()?;
        let path = self.database_path(&conn);
        report(&conn, &path)
    }

    /// Gives free space back to the system.
    ///
    /// A library that already uses incremental auto-vacuum frees up to
    /// [`RECLAIM_STEP_PAGES`] free pages per call. An older library is first converted by one
    /// `VACUUM`, if it is small enough that this takes seconds and the disk has room for the
    /// rewrite (the live data three times over, once in the temporary copy, once in the log and
    /// once in the file). The log is truncated afterwards. Nothing in the library changes
    /// meaning: only where pages sit in the file.
    pub fn reclaim_storage(&self) -> Result<StorageReclaim> {
        let mut conn = self.conn()?;
        let path = self.database_path(&conn);
        reclaim(&mut conn, &path, &Limits::system(&path))
    }

    /// After an operation that rewrote or removed a lot of rows: give free space back if there
    /// is a worthwhile amount, and keep the log short. The connection must not be held.
    pub(crate) fn tidy_after_bulk_change(&self) {
        // Merges that just left the Undo window no longer need their retained originals.
        let _ = self.sweep_legacy_originals();
        self.reclaim_storage_when_worthwhile();
        self.checkpoint_wal_when_idle();
    }

    /// The automatic step at the end of opening and after a purge: skipped for a library with
    /// nothing worth giving back, never an error.
    pub(crate) fn reclaim_storage_when_worthwhile(&self) {
        let Ok(mut conn) = self.inner.conn.try_lock() else {
            return;
        };
        let path = self.database_path(&conn);
        let Ok(before) = report(&conn, &path) else {
            return;
        };
        if before.incremental_vacuum && before.free_bytes < WORTH_RECLAIMING_BYTES {
            return;
        }
        let mut limits = Limits::system(&path);
        limits.conversion_bytes = open_conversion_limit();
        let _ = reclaim(&mut conn, &path, &limits);
    }
}

/// What a reclaim may do: how many free pages one step returns, the largest library the
/// one-time conversion rewrites, and the disk space it can count on.
struct Limits {
    step_pages: i64,
    conversion_bytes: u64,
    room_bytes: u64,
}
impl Limits {
    fn system(database: &std::path::Path) -> Self {
        Self {
            step_pages: RECLAIM_STEP_PAGES,
            conversion_bytes: CONVERSION_LIMIT_BYTES,
            room_bytes: database
                .parent()
                .and_then(|dir| fs2::available_space(dir).ok())
                .unwrap_or(0),
        }
    }
}

fn reclaim(
    conn: &mut Connection,
    database: &std::path::Path,
    limits: &Limits,
) -> Result<StorageReclaim> {
    checkpoint(conn)?;
    let before = report(conn, database)?;
    let mut converted = false;
    let mut skipped = None;
    if before.incremental_vacuum {
        // The pragma frees one page per row it returns: step it to the end.
        let mut step =
            conn.prepare(&format!("PRAGMA incremental_vacuum({})", limits.step_pages))?;
        let mut freed = step.query([])?;
        while freed.next()?.is_some() {}
    } else {
        let live = before.database_bytes - before.free_bytes;
        if live > limits.conversion_bytes {
            skipped = Some(format!(
                "The library holds {} MiB, more than a start-up rewrite should handle",
                live >> 20
            ));
        } else if limits.room_bytes < live.saturating_mul(3) + (256 << 20) {
            skipped = Some("Not enough free disk space for a safe rewrite".to_owned());
        } else {
            conn.pragma_update(None, "auto_vacuum", 2)?;
            conn.execute_batch("VACUUM")?;
            converted = true;
        }
    }
    checkpoint(conn)?;
    Ok(StorageReclaim {
        before,
        after: report(conn, database)?,
        converted,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn wal_bytes(lib: &Library) -> u64 {
        std::fs::metadata(lib.data_dir().join("library.sqlite-wal"))
            .map(|m| m.len())
            .unwrap_or(0)
    }
    /// One transaction that writes about `megabytes` MiB.
    fn write_in_one_transaction(lib: &Library, megabytes: usize) {
        let mut conn = lib.conn().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lean_probe(id INTEGER PRIMARY KEY, blob BLOB NOT NULL)",
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        let chunk = vec![7u8; 1024 * 1024];
        for _ in 0..megabytes {
            tx.execute("INSERT INTO lean_probe(blob) VALUES(?1)", params![chunk])
                .unwrap();
        }
        tx.commit().unwrap();
    }

    /// A library as the earlier versions left it: no auto-vacuum, `junk_mib` of free pages and
    /// some rows in a table that has no INTEGER PRIMARY KEY.
    fn old_library_with_free_space(dir: &std::path::Path, junk_mib: usize) -> std::path::PathBuf {
        let db = dir.join("library.sqlite");
        drop(Library::open(&db).unwrap());
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA auto_vacuum=0; VACUUM").unwrap();
        assert_eq!(pragma_i64(&conn, "auto_vacuum").unwrap(), 0);
        conn.execute_batch(
            "CREATE TABLE junk(id INTEGER PRIMARY KEY, blob BLOB);
             INSERT INTO maintenance_scans VALUES('first','broken','{}',1),('gone','broken','{}',2),('second','broken','{}',3),('third','broken','{}',4);",
        )
        .unwrap();
        for _ in 0..junk_mib {
            conn.execute(
                "INSERT INTO junk(blob) VALUES(?1)",
                params![vec![1u8; 1 << 20]],
            )
            .unwrap();
        }
        conn.execute_batch("DELETE FROM junk; DELETE FROM maintenance_scans WHERE id='gone'; PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        assert!(
            pragma_i64(&conn, "freelist_count").unwrap() as u64 * 4096 >= (junk_mib as u64) << 20
        );
        db
    }
    fn scan_order(lib: &Library) -> Vec<String> {
        let conn = lib.conn().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM maintenance_scans ORDER BY created,rowid")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn a_new_library_hands_free_pages_back_incrementally() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        assert!(lib.storage_report().unwrap().incremental_vacuum);
        {
            let conn = lib.conn().unwrap();
            conn.execute_batch("CREATE TABLE junk(id INTEGER PRIMARY KEY, blob BLOB)")
                .unwrap();
            for _ in 0..12 {
                conn.execute(
                    "INSERT INTO junk(blob) VALUES(?1)",
                    params![vec![1u8; 1 << 20]],
                )
                .unwrap();
            }
            conn.execute_batch("DELETE FROM junk").unwrap();
        }
        let before = lib.storage_report().unwrap();
        assert!(before.free_bytes >= 12 << 20);
        let done = lib.reclaim_storage().unwrap();
        assert!(!done.converted && done.skipped.is_none());
        assert!(done.after.free_bytes < 1 << 20, "{:?}", done.after);
        assert!(done.after.database_bytes + (10 << 20) < before.database_bytes);
    }

    #[test]
    fn opening_an_older_library_converts_it_once_and_keeps_its_rows_in_order() {
        let d = tempfile::tempdir().unwrap();
        let db = old_library_with_free_space(d.path(), 12);
        let size_before = std::fs::metadata(&db).unwrap().len();
        let lib = Library::open(&db).unwrap();
        let report = lib.storage_report().unwrap();
        assert!(report.incremental_vacuum);
        assert!(report.free_bytes < 1 << 20);
        assert!(report.database_bytes + (10 << 20) < size_before);
        assert_eq!(wal_bytes(&lib), 0);
        assert_eq!(scan_order(&lib), ["first", "second", "third"]);
        let integrity: String = lib
            .conn()
            .unwrap()
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        drop(lib);
        // A second open has nothing left to do.
        let lib = Library::open(&db).unwrap();
        assert!(lib.storage_report().unwrap().incremental_vacuum);
        assert!(!lib.reclaim_storage().unwrap().converted);
    }

    #[test]
    fn the_conversion_waits_when_the_disk_or_the_library_size_does_not_allow_it() {
        let d = tempfile::tempdir().unwrap();
        let db = old_library_with_free_space(d.path(), 10);
        let mut conn = Connection::open(&db).unwrap();
        let size = std::fs::metadata(&db).unwrap().len();
        let tight = Limits {
            step_pages: RECLAIM_STEP_PAGES,
            conversion_bytes: CONVERSION_LIMIT_BYTES,
            room_bytes: 1 << 20,
        };
        let done = reclaim(&mut conn, &db, &tight).unwrap();
        assert!(!done.converted);
        assert!(done.skipped.unwrap().contains("disk space"));
        let huge = Limits {
            conversion_bytes: 1 << 10,
            room_bytes: u64::MAX / 4,
            ..tight
        };
        let done = reclaim(&mut conn, &db, &huge).unwrap();
        assert!(!done.converted);
        assert!(done.skipped.unwrap().contains("start-up rewrite"));
        assert_eq!(
            std::fs::metadata(&db).unwrap().len(),
            size,
            "nothing was rewritten"
        );
        assert_eq!(pragma_i64(&conn, "auto_vacuum").unwrap(), 0);
    }

    #[test]
    fn opening_leaves_a_big_old_library_unconverted_and_the_explicit_reclaim_converts_it() {
        let d = tempfile::tempdir().unwrap();
        let db = old_library_with_free_space(d.path(), 10);
        let size = std::fs::metadata(&db).unwrap().len();
        // The live data is over what opening may rewrite (a small limit stands in for the real
        // one, which a test cannot afford to fill).
        OPEN_LIMIT.with(|limit| limit.set(Some(1 << 10)));
        let lib = Library::open(&db).unwrap();
        OPEN_LIMIT.with(|limit| limit.set(None));
        assert!(!lib.storage_report().unwrap().incremental_vacuum);
        assert_eq!(std::fs::metadata(&db).unwrap().len(), size);
        // The explicit reclaim is not held to the opening limit.
        let done = lib.reclaim_storage().unwrap();
        assert!(done.converted && lib.storage_report().unwrap().incremental_vacuum);
        const { assert!(OPEN_CONVERSION_LIMIT_BYTES < CONVERSION_LIMIT_BYTES) };
        assert_eq!(open_conversion_limit(), OPEN_CONVERSION_LIMIT_BYTES);
    }

    #[test]
    fn one_step_returns_only_the_pages_it_is_allowed_to() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("library.sqlite");
        drop(Library::open(&db).unwrap());
        let mut conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE junk(id INTEGER PRIMARY KEY, blob BLOB)")
            .unwrap();
        for _ in 0..8 {
            conn.execute(
                "INSERT INTO junk(blob) VALUES(?1)",
                params![vec![1u8; 1 << 20]],
            )
            .unwrap();
        }
        conn.execute_batch("DELETE FROM junk").unwrap();
        let free = pragma_i64(&conn, "freelist_count").unwrap();
        assert!(free > 2000);
        let limits = Limits {
            step_pages: 500,
            conversion_bytes: CONVERSION_LIMIT_BYTES,
            room_bytes: u64::MAX / 4,
        };
        let done = reclaim(&mut conn, &db, &limits).unwrap();
        assert_eq!(pragma_i64(&conn, "freelist_count").unwrap(), free - 500);
        assert!(done.after.free_bytes > 0);
    }

    #[test]
    fn the_connection_carries_the_log_bounds() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let conn = lib.conn().unwrap();
        let limit: i64 = conn
            .query_row("PRAGMA journal_size_limit", [], |r| r.get(0))
            .unwrap();
        let auto: i64 = conn
            .query_row("PRAGMA wal_autocheckpoint", [], |r| r.get(0))
            .unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            (limit, auto, mode.as_str()),
            (WAL_SIZE_LIMIT_BYTES, WAL_AUTOCHECKPOINT_PAGES, "wal")
        );
    }

    #[test]
    fn a_large_transaction_does_not_leave_a_large_log_behind() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        let limit = WAL_SIZE_LIMIT_BYTES as u64;
        write_in_one_transaction(&lib, 3 * (limit / (1024 * 1024)) as usize);
        assert!(
            wal_bytes(&lib) > limit,
            "the probe must make the log bigger than the limit"
        );
        // The next write restarts the emptied log, and SQLite cuts the file to the limit.
        write_in_one_transaction(&lib, 1);
        assert!(
            wal_bytes(&lib) <= limit + 1024 * 1024,
            "log stayed at {} bytes",
            wal_bytes(&lib)
        );
    }

    #[test]
    fn an_explicit_checkpoint_empties_the_log_file() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        write_in_one_transaction(&lib, 2);
        assert!(wal_bytes(&lib) > 0);
        let done = lib.checkpoint_wal().unwrap();
        assert!(!done.busy);
        assert_eq!(wal_bytes(&lib), 0);
    }

    #[test]
    fn checkpointing_works_while_an_exclusive_operation_holds_its_lease() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.sqlite")).unwrap();
        write_in_one_transaction(&lib, 2);
        let lease = lib.exclusive_operation().unwrap();
        // On the lease's own thread and from another thread (the shutdown path).
        assert!(!lib.checkpoint_wal().unwrap().busy);
        write_in_one_transaction(&lib, 2);
        let other = lib.clone();
        std::thread::spawn(move || other.checkpoint_wal().unwrap())
            .join()
            .unwrap();
        assert_eq!(wal_bytes(&lib), 0);
        // The lease is still the only way in: background work stays busy until it ends.
        let background = lib.clone();
        let refused = std::thread::spawn(move || background.counts().is_err())
            .join()
            .unwrap();
        assert!(refused, "the lease must still lock out other threads");
        drop(lease);
        assert!(lib.counts().is_ok());
        // A second process is still locked out of the library.
        assert!(Library::open(&d.path().join("library.sqlite")).is_err());
    }

    #[test]
    fn opening_truncates_a_log_left_by_a_killed_process() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("library.sqlite");
        let lib = Library::open(&db).unwrap();
        // Keep the log from being folded in, then leave without closing the connection:
        // copy the files as a killed process would leave them.
        lib.conn()
            .unwrap()
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        write_in_one_transaction(&lib, 4);
        let copy = d.path().join("killed");
        std::fs::create_dir(&copy).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let name = format!("library.sqlite{suffix}");
            std::fs::copy(d.path().join(&name), copy.join(&name)).unwrap();
        }
        assert!(copy.join("library.sqlite-wal").metadata().unwrap().len() > 3 << 20);
        let reopened = Library::open(&copy.join("library.sqlite")).unwrap();
        assert_eq!(wal_bytes(&reopened), 0);
    }
}
