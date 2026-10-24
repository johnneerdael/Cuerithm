use super::*;
use rusqlite::{
    params_from_iter,
    types::{Value as SqlValue, ValueRef},
};
/// The native tables a sync can change: (table, column naming the target, sort order).
/// Rows are captured in key order, so the ledger and its digest do not depend on where SQLite
/// keeps a row in the file.
const TABLES: &[(&str, &str, &str)] = &[
    ("native_targets", "id", "id"),
    ("native_records", "target_id", "table_name,record_id"),
    ("native_track_links", "target_id", "content_id"),
    ("native_sync_track_ownership", "target_id", "local_track_id"),
    ("native_reconciliation_plans", "target_id", "id"),
    ("native_playlist_bindings", "target_id", "kind,native_id"),
    ("native_playlist_acknowledgements", "target_id", "plan_hash"),
];
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TableRows {
    name: String,
    columns: Vec<String>,
    rows: Vec<Vec<TargetValue>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NativeLedger {
    target: String,
    tables: Vec<TableRows>,
}
impl NativeLedger {
    /// True for the empty ledger of a sync that can no longer be restored.
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}
/// The native state of one target in its lean form: a record outside the tables sync reads back
/// is its key and hash only, and a DJ-software track with no library track has no observation.
/// A database that still holds the earlier full copies captures the same ledger as one that has
/// been converted, so a digest of it does not depend on which one it is.
pub(super) fn capture(connection: &Connection, target: &str) -> Result<NativeLedger> {
    let mut ledger = read(connection, target, false)?;
    normalise(&mut ledger);
    Ok(ledger)
}
/// What the earlier versions captured: every row as stored, in the order SQLite keeps them.
pub(super) fn capture_as_before_schema_30(
    connection: &Connection,
    target: &str,
) -> Result<NativeLedger> {
    read(connection, target, true)
}
/// Drops the content a ledger no longer carries.
pub(super) fn normalise(ledger: &mut NativeLedger) {
    for table in &mut ledger.tables {
        let column = |name: &str| table.columns.iter().position(|c| c == name);
        match table.name.as_str() {
            "native_records" => {
                if let (Some(name), Some(json)) = (column("table_name"), column("json")) {
                    for row in &mut table.rows {
                        let kept = matches!(&row[name], TargetValue::Text(t)
                            if crate::native_sync::RECORD_CONTENT_TABLES.contains(&t.as_str()));
                        if !kept && matches!(row[json], TargetValue::Text(_)) {
                            row[json] = TargetValue::Text(String::new());
                        }
                    }
                }
            }
            "native_track_links" => {
                if let (Some(local), Some(observation), Some(hash)) = (
                    column("local_track_id"),
                    column("observation_json"),
                    column("observation_hash"),
                ) {
                    for row in &mut table.rows {
                        if matches!(row[local], TargetValue::Null)
                            && !matches!(row[hash], TargetValue::Null)
                        {
                            row[observation] = TargetValue::Null;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
fn read(connection: &Connection, target: &str, legacy: bool) -> Result<NativeLedger> {
    let mut tables = vec![];
    for (name, key, order) in TABLES {
        let order = if legacy { "rowid" } else { order };
        let mut stmt = connection.prepare(&format!(
            "SELECT * FROM {name} WHERE {key}=?1 ORDER BY {order}"
        ))?;
        let columns = stmt
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let count = columns.len();
        let rows = stmt
            .query_map([target], |r| {
                (0..count)
                    .map(|i| {
                        Ok(match r.get_ref(i)? {
                            ValueRef::Null => TargetValue::Null,
                            ValueRef::Integer(v) => TargetValue::Integer(v),
                            ValueRef::Real(v) => TargetValue::Real(v),
                            ValueRef::Text(v) => TargetValue::Text(
                                std::str::from_utf8(v)
                                    .map_err(|e| {
                                        rusqlite::Error::FromSqlConversionFailure(
                                            i,
                                            rusqlite::types::Type::Text,
                                            Box::new(e),
                                        )
                                    })?
                                    .to_owned(),
                            ),
                            ValueRef::Blob(v) => TargetValue::Blob(v.to_vec()),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tables.push(TableRows {
            name: (*name).into(),
            columns,
            rows,
        });
    }
    Ok(NativeLedger {
        target: target.into(),
        tables,
    })
}
pub(super) fn restore(connection: &Connection, ledger: &NativeLedger) -> Result<()> {
    for (name, key, _) in TABLES.iter().rev() {
        connection.execute(
            &format!("DELETE FROM {name} WHERE {key}=?1"),
            [&ledger.target],
        )?;
    }
    for table in &ledger.tables {
        ensure!(
            TABLES.iter().any(|(name, _, _)| *name == table.name),
            "Unsupported native ledger table"
        );
        let columns = table
            .columns
            .iter()
            .map(|c| {
                ensure!(
                    c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                    "Invalid ledger column"
                );
                Ok(format!("\"{c}\""))
            })
            .collect::<Result<Vec<_>>>()?
            .join(",");
        let values = (0..table.columns.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        for row in &table.rows {
            let row = row
                .iter()
                .map(|v| match v {
                    TargetValue::Null => SqlValue::Null,
                    TargetValue::Integer(v) => SqlValue::Integer(*v),
                    TargetValue::Real(v) => SqlValue::Real(*v),
                    TargetValue::Text(v) => SqlValue::Text(v.clone()),
                    TargetValue::Blob(v) => SqlValue::Blob(v.clone()),
                })
                .collect::<Vec<_>>();
            connection.execute(
                &format!("INSERT INTO {} ({columns}) VALUES ({values})", table.name),
                params_from_iter(row),
            )?;
        }
    }
    Ok(())
}

/// Brings the operations saved by an earlier version to the lean ledger, in the transaction
/// that makes the stored records lean (schema 31). It must run first, while the records still
/// hold their full copies, because that is the only time a recorded digest can be checked
/// against the way it was computed.
///
/// - Every ledger loses the content a lean ledger does not carry.
/// - A ledger digest that still describes the current native state is recomputed for the lean
///   form, so that the sync can still be restored. One that no longer describes it keeps its
///   value; restoring that sync was already refused.
/// - A sync that was restored or rolled back can never be restored again and drops its ledger.
///
/// An operation this cannot read is left as it is: the sync service cannot read it either.
pub(crate) fn upgrade_operations(connection: &Connection) -> Result<()> {
    let operations = connection
        .prepare("SELECT id,status,journal_json FROM rekordbox_sync_operations")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, status, journal) in operations {
        let Ok(mut value) = serde_json::from_str::<Value>(&journal) else {
            continue;
        };
        let Some(operation) = value.as_object_mut() else {
            continue;
        };
        let (Some(target), Some(before)) = (
            operation
                .get("native_target_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            operation
                .get("ledger_before")
                .cloned()
                .and_then(|v| serde_json::from_value::<NativeLedger>(v).ok()),
        ) else {
            continue;
        };
        let mut before = before;
        normalise(&mut before);
        if matches!(status.as_str(), "restored" | "rolled_back" | "discarded") {
            before.tables.clear();
        }
        if let Some(recorded) = operation
            .get("ledger_after_hash")
            .and_then(Value::as_str)
            .map(str::to_owned)
            && digest(&capture_as_before_schema_30(connection, &target)?)? == recorded
        {
            operation.insert(
                "ledger_after_hash".into(),
                json!(digest(&capture(connection, &target)?)?),
            );
        }
        operation.insert("ledger_before".into(), serde_json::to_value(&before)?);
        connection.execute(
            "UPDATE rekordbox_sync_operations SET journal_json=?1 WHERE id=?2",
            params![serde_json::to_string(&value)?, id],
        )?;
    }
    Ok(())
}
