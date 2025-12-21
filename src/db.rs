use std::fs;
use std::path::Path;
use std::{collections::HashSet, str::FromStr};

use anyhow::Result;
use rusqlite::{params, Connection, Row};

use crate::metrics::{MetricKind, MetricSample};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS metric_samples (
    ts REAL NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    value REAL,
    unit TEXT,
    details TEXT
);
CREATE INDEX IF NOT EXISTS idx_metric_samples_ts ON metric_samples (ts);
CREATE INDEX IF NOT EXISTS idx_metric_samples_kind_ts ON metric_samples (kind, ts);
"#;

pub fn init_db_connection(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

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

pub fn insert_metric_samples(db_path: &Path, samples: &[MetricSample]) -> Result<()> {
    let mut conn = Connection::open(db_path)?;
    insert_metric_samples_with_conn(&mut conn, samples)
}

pub fn count_metric_samples(db_path: &Path, since_ts: Option<f64>) -> Result<usize> {
    let conn = Connection::open(db_path)?;
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

fn metric_from_row(row: &Row) -> rusqlite::Result<MetricSample> {
    let kind_raw: String = row.get("kind")?;
    let kind = MetricKind::from_str(&kind_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::fmt::Error),
        )
    })?;
    let details_raw: Option<String> = row.get("details")?;
    let details = match details_raw {
        Some(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    };

    Ok(MetricSample {
        ts: row.get("ts")?,
        kind,
        source: row.get::<_, String>("source")?,
        value: row.get("value")?,
        unit: row.get::<_, Option<String>>("unit")?,
        details,
    })
}

pub fn fetch_metric_samples(
    db_path: &Path,
    since_ts: Option<f64>,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(if since_ts.is_some() {
        "SELECT * FROM metric_samples WHERE ts >= ? ORDER BY ts"
    } else {
        "SELECT * FROM metric_samples ORDER BY ts"
    })?;
    let rows = match since_ts {
        Some(ts) => stmt.query_map(params![ts], metric_from_row)?,
        None => stmt.query_map([], metric_from_row)?,
    };
    let mut samples = Vec::new();
    for row in rows {
        let sample = row?;
        if let Some(filter) = kinds {
            if !filter.contains(&sample.kind) {
                continue;
            }
        }
        samples.push(sample);
    }
    Ok(samples)
}

pub fn fetch_latest_metric_samples(
    db_path: &Path,
    kinds: Option<&[MetricKind]>,
) -> Result<Vec<MetricSample>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT * FROM metric_samples ORDER BY ts DESC")?;
    let rows = stmt.query_map([], metric_from_row)?;
    let mut seen: HashSet<(MetricKind, String)> = HashSet::new();
    let mut samples = Vec::new();
    for row in rows {
        let sample = row?;
        if let Some(filter) = kinds {
            if !filter.contains(&sample.kind) {
                continue;
            }
        }
        let key = (sample.kind.clone(), sample.source.clone());
        if seen.insert(key) {
            samples.push(sample);
        }
    }
    samples.reverse();
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
}
