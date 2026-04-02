use std::fs;
use std::path::Path;
use std::str::FromStr;

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

pub fn init_db_connection(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
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

fn metric_from_row(row: &Row) -> rusqlite::Result<MetricSample> {
    let kind_raw: String = row.get("kind")?;
    let kind = MetricKind::from_str(&kind_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
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
        samples.push(row?);
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
        samples.push(row?);
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
}
