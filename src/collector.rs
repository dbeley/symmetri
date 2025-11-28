use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use log::{info, warn};

use crate::db::{self, Sample};
use crate::sysfs::{find_battery_paths, read_battery};

pub fn default_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".local")
        .join("share")
        .join("battery-monitor")
        .join("battery.db")
}

pub fn resolve_db_path(db_path: Option<&Path>) -> PathBuf {
    if let Some(path) = db_path {
        return path.to_path_buf();
    }
    if let Ok(env_path) = std::env::var("BATTERY_MONITOR_DB") {
        if let Some(stripped) = env_path.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(stripped);
            }
        }
        return PathBuf::from(env_path);
    }
    default_db_path()
}

pub fn collect_once(db_path: Option<&Path>, sysfs_root: Option<&Path>) -> Result<i32> {
    let resolved_db = resolve_db_path(db_path);
    db::init_db(&resolved_db)?;

    let root = sysfs_root.unwrap_or_else(|| Path::new("/sys/class/power_supply"));
    let battery_paths = find_battery_paths(root);
    if battery_paths.is_empty() {
        warn!("No batteries found in sysfs");
        return Ok(1);
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let mut samples: Vec<Sample> = Vec::new();
    for path in battery_paths {
        let reading = read_battery(&path);
        samples.push(db::create_sample_from_reading(&reading, Some(ts)));
    }

    db::insert_samples(&resolved_db, &samples)?;

    for sample in samples {
        info!(
            "Logged record for {}: percent={:.2} health={:.2}",
            Path::new(&sample.source_path)
                .file_name()
                .map(|p| p.to_string_lossy())
                .unwrap_or_else(|| sample.source_path.clone().into()),
            sample.percentage.unwrap_or(0.0),
            sample.health_pct.unwrap_or(0.0)
        );
    }
    Ok(0)
}

pub fn collect_loop(
    interval_seconds: u64,
    db_path: Option<&Path>,
    sysfs_root: Option<&Path>,
) -> Result<()> {
    loop {
        let _ = collect_once(db_path, sysfs_root)?;
        thread::sleep(Duration::from_secs(interval_seconds));
    }
}
