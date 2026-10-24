//! Bounds on the saved scans, reviews and previews.
//!
//! A duplicate scan lists every library track with its path and fingerprint, a review embeds a
//! full copy of its scan, and a merge plan embeds the review. Before this bound each scan added
//! three copies of the library to the database, for ever. Only the newest few of each kind are
//! useful: the view reopens the scan or review it last used, and anything older is replaced by
//! a new scan.
//!
//! A row is never removed while something that can still be used needs it:
//! - a review that a kept merge plan was made from (applying the plan checks that review's
//!   revision), and a scan that a kept removal plan was made from;
//! - any of the unused-file kinds while an unused-file removal is unfinished.
use anyhow::Result;
use rusqlite::Connection;

/// How many of each kind stay.
pub const KEEP_NEWEST: usize = 3;
/// How many settled unused-file removals the history list keeps. A removal is only a list of
/// paths and hashes; the files themselves are in the Trash.
pub const KEEP_REMOVAL_HISTORY: usize = 20;

/// The kinds of saved row that are bounded. A kind comes after the kinds it is made from, so a
/// pass can free a review or a scan by removing the plan that held it.
const BOUNDED: [&str; 6] = [
    "duplicates",
    "duplicate_review",
    "duplicate_merge_plan",
    "broken",
    "unused",
    "unused_removal_plan",
];

impl crate::Library {
    /// Applies the bounds to a library that earlier versions let grow: run when it opens.
    pub(crate) fn prune_saved_reports(&self) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let removed = prune(&tx)?;
        tx.commit()?;
        Ok(removed)
    }
}

/// Removes the saved rows beyond the newest [`KEEP_NEWEST`] of their kind that nothing needs.
/// Call it in the transaction that saves a new one, and once when a library opens. Returns how
/// many rows it removed.
pub(crate) fn prune(conn: &Connection) -> Result<usize> {
    let unfinished: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM maintenance_scans WHERE kind='unused_removal_operation'
                        AND json_extract(payload_json,'$.state') IN ('prepared','conflict'))",
        [],
        |r| r.get(0),
    )?;
    let mut removed = 0;
    // Plans first: removing an old plan can free the review or scan it was made from.
    for kind in BOUNDED.iter().rev() {
        if unfinished && matches!(*kind, "unused" | "unused_removal_plan") {
            continue;
        }
        let older: Vec<String> = conn
            .prepare(
                "SELECT id FROM maintenance_scans WHERE kind=?1
                 ORDER BY created DESC,rowid DESC LIMIT -1 OFFSET ?2",
            )?
            .query_map(rusqlite::params![kind, KEEP_NEWEST as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in older {
            // A plan names the review or scan it was made from by id.
            let needed: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM maintenance_scans
                                WHERE kind IN ('duplicate_merge_plan','unused_removal_plan')
                                  AND id!=?1 AND instr(payload_json,?1)>0)",
                [&id],
                |r| r.get(0),
            )?;
            if needed {
                continue;
            }
            removed += conn.execute(
                "DELETE FROM maintenance_scans WHERE id=?1 AND kind=?2",
                rusqlite::params![id, kind],
            )?;
        }
    }
    // The history of unused-file removals: only those that have settled and hold no copy that
    // an earlier version kept.
    removed += conn.execute(
        "DELETE FROM maintenance_scans WHERE kind='unused_removal_operation'
           AND id IN (SELECT id FROM maintenance_scans WHERE kind='unused_removal_operation'
                       ORDER BY created DESC,rowid DESC LIMIT -1 OFFSET ?1)
           AND json_extract(payload_json,'$.cleanup_complete')=1
           AND json_extract(payload_json,'$.state') IN ('committed','rolled_back','undone')
           AND (json_extract(payload_json,'$.schema')='unused-removal-v3'
                OR COALESCE(json_extract(payload_json,'$.backups_disposed'),0)=1)",
        [KEEP_REMOVAL_HISTORY as i64],
    )?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE maintenance_scans (id TEXT PRIMARY KEY,kind TEXT NOT NULL,payload_json TEXT NOT NULL,created INTEGER NOT NULL)",
        )
        .unwrap();
        conn
    }
    fn add(conn: &Connection, kind: &str, id: &str, created: i64, payload: &str) {
        conn.execute(
            "INSERT INTO maintenance_scans VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, kind, payload, created],
        )
        .unwrap();
    }
    fn ids(conn: &Connection, kind: &str) -> Vec<String> {
        conn.prepare("SELECT id FROM maintenance_scans WHERE kind=?1 ORDER BY created")
            .unwrap()
            .query_map([kind], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn only_the_newest_of_each_kind_stay() {
        let conn = conn();
        for n in 0..6 {
            add(&conn, "duplicates", &format!("scan{n}"), n, "{}");
            add(&conn, "broken", &format!("broken{n}"), n, "{}");
            add(&conn, "missing_repair", &format!("repair{n}"), n, "{}");
        }
        assert_eq!(prune(&conn).unwrap(), 6);
        assert_eq!(ids(&conn, "duplicates"), ["scan3", "scan4", "scan5"]);
        assert_eq!(ids(&conn, "broken"), ["broken3", "broken4", "broken5"]);
        assert_eq!(
            ids(&conn, "missing_repair").len(),
            6,
            "history is not a saved scan"
        );
    }

    #[test]
    fn a_review_a_kept_plan_was_made_from_stays() {
        let conn = conn();
        for n in 0..6 {
            add(&conn, "duplicate_review", &format!("review{n}"), n, "{}");
        }
        // The newest plan was made from the oldest review.
        add(
            &conn,
            "duplicate_merge_plan",
            "plan",
            9,
            "{\"review_id\":\"review0\"}",
        );
        prune(&conn).unwrap();
        assert_eq!(
            ids(&conn, "duplicate_review"),
            ["review0", "review3", "review4", "review5"]
        );
        // Once the plan itself is old enough to go, so does the review.
        for n in 0..3 {
            add(
                &conn,
                "duplicate_merge_plan",
                &format!("later{n}"),
                10 + n,
                "{}",
            );
        }
        prune(&conn).unwrap();
        assert_eq!(ids(&conn, "duplicate_merge_plan").len(), 3);
        assert_eq!(
            ids(&conn, "duplicate_review"),
            ["review3", "review4", "review5"]
        );
    }

    #[test]
    fn unused_file_scans_stay_while_a_removal_is_unfinished() {
        let conn = conn();
        for n in 0..6 {
            add(&conn, "unused", &format!("unused{n}"), n, "{}");
            add(&conn, "unused_removal_plan", &format!("plan{n}"), n, "{}");
        }
        add(
            &conn,
            "unused_removal_operation",
            "operation",
            9,
            "{\"state\":\"prepared\"}",
        );
        assert_eq!(prune(&conn).unwrap(), 0);
        conn.execute(
            "UPDATE maintenance_scans SET payload_json='{\"state\":\"committed\"}' WHERE id='operation'",
            [],
        )
        .unwrap();
        assert_eq!(prune(&conn).unwrap(), 6);
        assert_eq!(ids(&conn, "unused"), ["unused3", "unused4", "unused5"]);
    }

    #[test]
    fn the_removal_history_keeps_the_newest_settled_ones_and_everything_unfinished() {
        let conn = conn();
        let settled =
            "{\"schema\":\"unused-removal-v3\",\"state\":\"committed\",\"cleanup_complete\":true}";
        for n in 0..30 {
            add(
                &conn,
                "unused_removal_operation",
                &format!("op{n:02}"),
                n,
                settled,
            );
        }
        // Older than all of them: one that is not finished, one that still holds a copy.
        add(
            &conn,
            "unused_removal_operation",
            "unfinished",
            -2,
            "{\"schema\":\"unused-removal-v3\",\"state\":\"committed\",\"cleanup_complete\":false}",
        );
        add(
            &conn,
            "unused_removal_operation",
            "holds-a-copy",
            -1,
            "{\"schema\":\"unused-removal-v2\",\"state\":\"committed\",\"cleanup_complete\":true,\"backups_disposed\":false}",
        );
        add(
            &conn,
            "unused_removal_operation",
            "copy-moved",
            -3,
            "{\"schema\":\"unused-removal-v2\",\"state\":\"committed\",\"cleanup_complete\":true,\"backups_disposed\":true}",
        );
        assert_eq!(prune(&conn).unwrap(), 10 + 1);
        let kept = ids(&conn, "unused_removal_operation");
        assert_eq!(kept.len(), 20 + 2);
        assert!(kept.contains(&"unfinished".to_owned()));
        assert!(kept.contains(&"holds-a-copy".to_owned()));
        assert!(!kept.contains(&"copy-moved".to_owned()));
        assert!(!kept.contains(&"op00".to_owned()) && kept.contains(&"op29".to_owned()));
    }

    #[test]
    fn a_scan_a_kept_removal_plan_was_made_from_stays() {
        let conn = conn();
        for n in 0..5 {
            add(&conn, "unused", &format!("unused{n}"), n, "{}");
        }
        add(
            &conn,
            "unused_removal_plan",
            "plan",
            9,
            "{\"scan_id\":\"unused0\"}",
        );
        prune(&conn).unwrap();
        assert_eq!(
            ids(&conn, "unused"),
            ["unused0", "unused2", "unused3", "unused4"]
        );
    }
}
