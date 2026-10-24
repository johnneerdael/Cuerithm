//! The one bound on how many duplicate merges and removals can still be undone.
//!
//! Every removal of library records keeps a copy of those records for Undo, whether it
//! came from a duplicate merge that deletes files or from Missing & broken, and every merge
//! keeps the library state it changed. The newest `REMOVAL_UNDO_LIMIT` of them, counted
//! together, keep their copy; older ones are final:
//! the stored records are dropped, their history line stays without them, and Undo is no
//! longer offered. That keeps the library from carrying the records of every track it ever
//! lost. The cached covers a dropped removal was holding are returned so the caller can free
//! them with `purge::free_covers` once its transaction has committed.
use anyhow::Result;
use rusqlite::Connection;

/// How many removals keep their records for Undo, duplicate merges and Missing & broken
/// removals counted together.
pub const REMOVAL_UNDO_LIMIT: usize = 10;

/// Drops the stored records of every removal beyond the newest `REMOVAL_UNDO_LIMIT`, in
/// whichever history holds it, and returns the covers those removals were holding. Call it in
/// the transaction that records a new removal.
pub(super) fn enforce(conn: &Connection) -> Result<Vec<String>> {
    // Newest first. A removal that was undone or already dropped no longer counts.
    let expired: Vec<(String, String)> = conn
        .prepare(
            "SELECT store,id FROM (
               SELECT 'missing' AS store,id,created,rowid AS seq FROM maintenance_scans
                WHERE kind='missing_repair'
                  AND json_extract(payload_json,'$.entry.kind')='remove'
                  AND json_extract(payload_json,'$.entry.undo_of') IS NULL
                  AND json_extract(payload_json,'$.undone_by') IS NULL
                  AND COALESCE(json_extract(payload_json,'$.undo_discarded'),0)=0
               UNION ALL
               SELECT 'merge',id,created,rowid FROM maintenance_writes
                WHERE kind='apply' AND state='committed'
                  AND COALESCE(json_extract(payload_json,'$.undo_expired'),0)=0
             ) ORDER BY created DESC,seq DESC LIMIT -1 OFFSET ?1",
        )?
        .query_map([REMOVAL_UNDO_LIMIT as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut covers = Vec::new();
    for (store, id) in expired {
        match store.as_str() {
            "missing" => covers.extend(super::missing::drop_saved_records(conn, &id)?),
            #[cfg(feature = "autotag")]
            "merge" => covers.extend(super::donors::drop_saved_records(conn, &id)?),
            _ => {}
        }
    }
    Ok(covers)
}
