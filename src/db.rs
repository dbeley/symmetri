use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Result;
use rusqlite::{params, Connection, Row};

use crate::metrics::{MetricKind, MetricSample};

const SCHEMA: &str = r#"

CREATE TABLE IF NOT EXISTS metric_samples (
    ts REAL NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    value REAL,
    unit TEXT,
    details TEXT
);
CREATE INDEX IF NOT EXISTS idx_metric_samples_ts ON metric_samples (ts);
CREATE INDEX IF NOT EXISTS idx_metric_samples_kind_ts ON metric_samples (kind, ts);
"#;

/// Current schema version this binary writes. Existing databases at a lower
/// version are migrated forward in [`migrate`]. Bumping this requires adding
/// a branch to `migrate` covering every prior version.
const SCHEMA_VERSION: i64 = 1;

pub fn init_db_connection(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    // WAL lets a concurrent reader (report) coexist with the writer (collector)
    // instead of erroring with "database is locked". `busy_timeout` covers the
    // brief window where WAL is checkpointing.
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(SCHEMA)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Forward-only schema migrations via `PRAGMA user_version`. A database
/// created by an older binary reports `user_version = 0` and is assumed to
/// already match the historical schema; we only stamp the version forward.
/// Future schema changes add `else if current < N` arms running `ALTER TABLE`
/// statements inside a transaction, then bumping the pragma.
fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    // No structural migrations yet (v1 is the historical schema). Just stamp
    // existing databases so future migrations can target a known baseline.
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

#[allow(dead_code)]
pub fn init_db(db_path: &Path) -> Result<()> {
    init_db_connection(db_path).map(|_| ())
}

fn serialize_details(details: &serde_json::Value) -> Option<String> {
    if details.is_null() {
        None
    } else {
        Some(details.to_string())
    }
}

pub fn insert_metric_samples_with_conn(
    conn: &mut Connection,
    samples: &[MetricSample],
) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            r#"
            INSERT INTO metric_samples (
                ts, kind, source, value, unit, details
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )?;
        for sample in samples {
            stmt.execute(params![
                sample.ts,
                sample.kind.as_str(),
                sample.source,
                sample.value,
                sample.unit,
                serialize_details(&sample.details),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[allow(dead_code)]
pub fn insert_metric_samples(db_path: &Path, samples: &[MetricSample]) -> Result<()> {
    let mut conn = Connection::open(db_path)?;
    insert_metric_samples_with_conn(&mut conn, samples)
}

#[allow(dead_code)]
pub fn count_metric_samples(db_path: &Path, since_ts: Option<f64>) -> Result<usize> {
    let conn = Connection::open(db_path)?;
    count_metric_samples_with_conn(&conn, since_ts)
}

pub fn count_metric_samples_with_conn(conn: &Connection, since_ts: Option<f64>) -> Result<usize> {
    let count: i64 = match since_ts {
        Some(ts) => conn.query_row(
            "SELECT COUNT(*) FROM metric_samples WHERE ts >= ?",
            params![ts],
            |row| row.get(0),
        )?,
        None => conn.query_row("SELECT COUNT(*) FROM metric_samples", [], |row| row.get(0))?,
    };
    Ok(count as usize)
}

/// Deletes every metric sample with `ts < now_secs - prune_days*86400`.
/// Returns the number of removed rows.
pub fn prune_older_than_days_with_conn(conn: &Connection, prune_days: u64) -> Result<usize> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let cutoff = now_secs - (prune_days as f64) * 86400.0;
    let removed = conn.execute("DELETE FROM metric_samples WHERE ts < ?", params![cutoff])?;
    Ok(removed)
}

/// One row mapped to a sample. Rows whose `kind` doesn't parse (legacy or
/// corrupt entries after an upgrade) are dropped with a warning rather than
/// aborting the whole report — see [`map_row_skipping_unknown`].
fn metric_from_row(row: &Row) -> rusqlite::Result<Option<MetricSample>> {
    let kind_raw: String = row.get("kind")?;
    let Some(kind) = MetricKind::from_str(&kind_raw).ok() else {
        log::warn!("skipping metric row with unknown kind `{kind_raw}`");
        return Ok(None);
    };
    let details_raw: Option<String> = row.get("details")?;
    let details = match details_raw {
        Some(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    };

    Ok(Some(MetricSample {
        ts: row.get("ts")?,
        kind,
        source: row.get::<_, String>("source")?,
        value: row.get("value")?,
        unit: row.get::<_, Option<String>>("unit")?,
        details,
    }))
}

#[allow(dead_code)]
pub fn fetch_metric_samples(
    db_path: &Path,
    since_ts: Option<f64>,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let conn = Connection::open(db_path)?;
    fetch_metric_samples_with_conn(&conn, since_ts, kinds)
}

pub fn fetch_metric_samples_with_conn(
    conn: &Connection,
    since_ts: Option<f64>,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let kind_placeholders = kinds.map(|k| k.iter().map(|_| "?").collect::<Vec<_>>().join(", "));

    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        match (since_ts, &kind_placeholders) {
            (Some(_), Some(ph)) => (
                format!(
                    "SELECT * FROM metric_samples WHERE ts >= ? AND kind IN ({ph}) ORDER BY ts"
                ),
                {
                    let mut v: Vec<Box<dyn rusqlite::types::ToSql>> =
                        vec![Box::new(since_ts.unwrap())];
                    for k in kinds.unwrap() {
                        v.push(Box::new(k.as_str().to_string()));
                    }
                    v
                },
            ),
            (Some(ts), None) => (
                "SELECT * FROM metric_samples WHERE ts >= ? ORDER BY ts".to_string(),
                vec![Box::new(ts)],
            ),
            (None, Some(ph)) => (
                format!("SELECT * FROM metric_samples WHERE kind IN ({ph}) ORDER BY ts"),
                {
                    let mut v: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                    for k in kinds.unwrap() {
                        v.push(Box::new(k.as_str().to_string()));
                    }
                    v
                },
            ),
            (None, None) => (
                "SELECT * FROM metric_samples ORDER BY ts".to_string(),
                Vec::new(),
            ),
        };

    let to_sql_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(to_sql_refs.iter()),
        metric_from_row,
    )?;
    let mut samples = Vec::new();
    for row in rows {
        if let Some(sample) = row? {
            samples.push(sample);
        }
    }
    Ok(samples)
}

#[allow(dead_code)]
pub fn fetch_latest_metric_samples(
    db_path: &Path,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let conn = Connection::open(db_path)?;
    fetch_latest_metric_samples_with_conn(&conn, kinds)
}

#[allow(dead_code)]
pub fn fetch_latest_metric_samples_with_conn(
    conn: &Connection,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let kind_filter = match kinds {
        Some(k) => {
            let placeholders = k.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            format!(" AND kind IN ({placeholders})")
        }
        None => String::new(),
    };

    let sql = format!(
        "SELECT m.* FROM metric_samples m \
         INNER JOIN ( \
             SELECT kind, source, MAX(ts) as max_ts \
             FROM metric_samples \
             WHERE 1=1{kind_filter} \
             GROUP BY kind, source \
         ) latest ON m.kind = latest.kind AND m.source = latest.source AND m.ts = latest.max_ts \
         ORDER BY m.ts"
    );

    let params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = match kinds {
        Some(k) => k
            .iter()
            .map(|kind| Box::new(kind.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>)
            .collect(),
        None => Vec::new(),
    };
    let to_sql_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(to_sql_refs.iter()),
        metric_from_row,
    )?;
    let mut samples = Vec::new();
    for row in rows {
        if let Some(sample) = row? {
            samples.push(sample);
        }
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{MetricKind, MetricSample};
    use serde_json::json;

    #[test]
    fn metric_samples_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("metrics.db");
        init_db(&db_path).unwrap();

        let metrics = vec![
            MetricSample {
                ts: 1.0,
                kind: MetricKind::CpuUsage,
                source: "cpu".to_string(),
                value: Some(42.0),
                unit: Some("%".to_string()),
                details: json!({"note": "first"}),
            },
            MetricSample {
                ts: 2.0,
                kind: MetricKind::CpuUsage,
                source: "cpu".to_string(),
                value: Some(50.0),
                unit: Some("%".to_string()),
                details: serde_json::Value::Null,
            },
            MetricSample {
                ts: 2.0,
                kind: MetricKind::NetworkBytes,
                source: "eth0".to_string(),
                value: Some(1000.0),
                unit: Some("bytes".to_string()),
                details: json!({"rx_bytes": 750, "tx_bytes": 250}),
            },
        ];

        insert_metric_samples(&db_path, &metrics).unwrap();

        let all = fetch_metric_samples(&db_path, None, None).unwrap();
        assert_eq!(all.len(), 3);

        let filtered =
            fetch_metric_samples(&db_path, Some(1.5), Some(&[MetricKind::CpuUsage])).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].value, Some(50.0));

        let latest = fetch_latest_metric_samples(&db_path, None).unwrap();
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].source, "cpu");
        assert_eq!(latest[0].value, Some(50.0));
    }

    #[test]
    fn battery_metrics_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("battery.db");
        init_db(&db_path).unwrap();

        let metrics = vec![
            MetricSample {
                ts: 10.0,
                kind: MetricKind::BatteryPercentage,
                source: "BAT0".to_string(),
                value: Some(75.0),
                unit: Some("%".to_string()),
                details: json!({"status": "Discharging"}),
            },
            MetricSample {
                ts: 10.0,
                kind: MetricKind::BatteryHealth,
                source: "BAT0".to_string(),
                value: Some(87.5),
                unit: Some("%".to_string()),
                details: json!({"status": "Discharging"}),
            },
        ];

        insert_metric_samples(&db_path, &metrics).unwrap();

        let rows = fetch_metric_samples(&db_path, None, None).unwrap();
        assert_eq!(rows.len(), 2);
        let stored = &rows[0];
        assert_eq!(stored.ts, 10.0);
        assert_eq!(stored.value, Some(75.0));
        assert_eq!(stored.kind, MetricKind::BatteryPercentage);
    }

    #[test]
    fn fetch_skips_unknown_kind_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("unknown.db");
        init_db(&db_path).unwrap();
        let valid = MetricSample {
            ts: 1.0,
            kind: MetricKind::CpuUsage,
            source: "cpu".to_string(),
            value: Some(10.0),
            unit: Some("%".to_string()),
            details: serde_json::Value::Null,
        };
        insert_metric_samples(&db_path, std::slice::from_ref(&valid)).unwrap();

        // Manually inject a row with a kind string the current binary doesn't know.
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO metric_samples (ts, kind, source, value, unit, details) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                2.0,
                "kind_from_the_future",
                "x",
                1.0,
                "",
                Option::<String>::None
            ],
        )
        .unwrap();

        let rows = fetch_metric_samples(&db_path, None, None).unwrap();
        assert_eq!(
            rows.len(),
            1,
            "unknown-kind row should be dropped, not abort the fetch"
        );
        assert_eq!(rows[0].kind, MetricKind::CpuUsage);
    }

    #[test]
    fn stamping_user_version_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("version.db");
        init_db(&db_path).unwrap();
        let v: i64 = Connection::open(&db_path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // Reopening must not error and must leave the version unchanged.
        init_db(&db_path).unwrap();
        let v2: i64 = Connection::open(&db_path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v2, SCHEMA_VERSION);
    }

    #[test]
    fn concurrent_read_during_write_does_not_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("concurrent.db");
        init_db(&db_path).unwrap();
        let to_insert = vec![MetricSample {
            ts: 1.0,
            kind: MetricKind::CpuUsage,
            source: "cpu".to_string(),
            value: Some(1.0),
            unit: Some("%".to_string()),
            details: serde_json::Value::Null,
        }];
        insert_metric_samples(&db_path, &to_insert).unwrap();

        let path = db_path.clone();
        let writer = std::thread::spawn(move || {
            let mut w = Connection::open(&path).unwrap();
            w.busy_timeout(Duration::from_secs(5)).unwrap();
            let tx = w.transaction().unwrap();
            // hold write lock for ~100ms
            std::thread::sleep(std::time::Duration::from_millis(100));
            tx.commit().unwrap();
            drop(w);
        });

        // reader waits briefly so the writer grabs the lock first
        std::thread::sleep(std::time::Duration::from_millis(20));
        let r = Connection::open(&db_path).unwrap();
        r.busy_timeout(Duration::from_secs(5)).unwrap();
        let result = count_metric_samples_with_conn(&r, None);
        assert!(
            result.is_ok(),
            "read should not fail under WAL: {:?}",
            result
        );
        writer.join().unwrap();
    }

    #[test]
    fn prune_deletes_old_samples_only() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("prune.db");
        init_db(&db_path).unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let old_ts = now - 60.0 * 86400.0; // 60 days ago
        let recent_ts = now - 60.0; // 1 minute ago
        let samples = vec![
            MetricSample {
                ts: old_ts,
                kind: MetricKind::CpuUsage,
                source: "cpu".to_string(),
                value: Some(1.0),
                unit: None,
                details: serde_json::Value::Null,
            },
            MetricSample {
                ts: recent_ts,
                kind: MetricKind::CpuUsage,
                source: "cpu".to_string(),
                value: Some(2.0),
                unit: None,
                details: serde_json::Value::Null,
            },
        ];
        insert_metric_samples(&db_path, &samples).unwrap();
        assert_eq!(count_metric_samples(&db_path, None).unwrap(), 2);

        let removed =
            prune_older_than_days_with_conn(&Connection::open(&db_path).unwrap(), 30).unwrap();
        assert_eq!(removed, 1, "the 60-day-old sample should be pruned");
        let remaining = fetch_metric_samples(&db_path, None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert!((remaining[0].ts - recent_ts).abs() < 1.0);
    }
}
