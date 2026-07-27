use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use std::thread;
use std::time::Duration;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

/// Preset groupings selected via `symmetri report --preset`. Lives in the
/// metrics domain layer rather than `cli` so chart/report code can depend on
/// it without reaching into the CLI module.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum ReportPreset {
    All,
    Battery,
    Cpu,
    Gpu,
    Memory,
    Network,
    Temperature,
    Disk,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Display, EnumString, EnumIter,
)]
#[strum(serialize_all = "snake_case")]
pub enum MetricKind {
    CpuUsage,
    CpuFrequency,
    GpuUsage,
    GpuFrequency,
    NetworkBytes,
    MemoryUsage,
    DiskUsage,
    Temperature,
    PowerDraw,
    BatteryPercentage,
    BatteryCapacity,
    BatteryHealth,
    BatteryEnergyNow,
    BatteryEnergyFull,
    BatteryEnergyFullDesign,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKind::CpuUsage => "cpu_usage",
            MetricKind::CpuFrequency => "cpu_frequency",
            MetricKind::GpuUsage => "gpu_usage",
            MetricKind::GpuFrequency => "gpu_frequency",
            MetricKind::NetworkBytes => "network_bytes",
            MetricKind::MemoryUsage => "memory_usage",
            MetricKind::DiskUsage => "disk_usage",
            MetricKind::Temperature => "temperature",
            MetricKind::PowerDraw => "power_draw",
            MetricKind::BatteryPercentage => "battery_percentage",
            MetricKind::BatteryCapacity => "battery_capacity",
            MetricKind::BatteryHealth => "battery_health",
            MetricKind::BatteryEnergyNow => "battery_energy_now",
            MetricKind::BatteryEnergyFull => "battery_energy_full",
            MetricKind::BatteryEnergyFullDesign => "battery_energy_full_design",
        }
    }

    #[allow(dead_code)]
    pub fn from_label(raw: &str) -> Option<Self> {
        raw.parse().ok()
    }

    #[allow(dead_code)]
    pub fn all_kinds() -> impl Iterator<Item = MetricKind> {
        <MetricKind as IntoEnumIterator>::iter()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub ts: f64,
    pub kind: MetricKind,
    pub source: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl MetricSample {
    pub fn new<S: Into<String>>(
        ts: f64,
        kind: MetricKind,
        source: S,
        value: Option<f64>,
        unit: Option<&str>,
        details: Value,
    ) -> Self {
        MetricSample {
            ts,
            kind,
            source: source.into(),
            value,
            unit: unit.map(|u| u.to_string()),
            details,
        }
    }
}

#[derive(Clone, Debug)]
struct CpuTimes {
    label: String,
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

fn read_cpu_times() -> Option<Vec<CpuTimes>> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    parse_cpu_times(&content)
}

/// Pure parser for `/proc/stat`. Separated from [`read_cpu_times`] so the
/// parser is testable without staging a fake procfs.
fn parse_cpu_times(content: &str) -> Option<Vec<CpuTimes>> {
    let mut times = Vec::new();
    for line in content.lines() {
        if !line.starts_with("cpu") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let label = parts.next()?.to_string();
        let numbers: Vec<u64> = parts
            .take(8)
            .filter_map(|p| p.parse::<u64>().ok())
            .collect();
        if numbers.len() < 8 {
            continue;
        }
        times.push(CpuTimes {
            label,
            user: numbers[0],
            nice: numbers[1],
            system: numbers[2],
            idle: numbers[3],
            iowait: numbers[4],
            irq: numbers[5],
            softirq: numbers[6],
            steal: numbers[7],
        });
    }
    if times.is_empty() {
        None
    } else {
        Some(times)
    }
}

fn cpu_usage_samples(ts: f64) -> Vec<MetricSample> {
    let first = match read_cpu_times() {
        Some(v) => v,
        None => return Vec::new(),
    };
    // The usage measurement spans the 100ms between the two reads; stamp it at
    // the midpoint so charts don't bias the line 50ms early on every sample.
    const SAMPLE_WINDOW_SECS: f64 = 0.100;
    let midpoint_ts = ts + SAMPLE_WINDOW_SECS / 2.0;
    thread::sleep(Duration::from_millis(100));
    let second = match read_cpu_times() {
        Some(v) => v,
        None => return Vec::new(),
    };
    let mut second_map: BTreeMap<String, CpuTimes> = BTreeMap::new();
    for entry in second {
        second_map.insert(entry.label.clone(), entry);
    }

    let mut samples = Vec::new();
    for prev in first {
        if let Some(next) = second_map.get(&prev.label) {
            let prev_total = prev.user
                + prev.nice
                + prev.system
                + prev.idle
                + prev.iowait
                + prev.irq
                + prev.softirq
                + prev.steal;
            let next_total = next.user
                + next.nice
                + next.system
                + next.idle
                + next.iowait
                + next.irq
                + next.softirq
                + next.steal;
            let prev_idle = prev.idle + prev.iowait;
            let next_idle = next.idle + next.iowait;
            let delta_total = next_total.saturating_sub(prev_total);
            let delta_idle = next_idle.saturating_sub(prev_idle);
            if delta_total == 0 {
                continue;
            }
            let busy = delta_total.saturating_sub(delta_idle);
            let usage = (busy as f64 / delta_total as f64) * 100.0;
            samples.push(MetricSample::new(
                midpoint_ts,
                MetricKind::CpuUsage,
                prev.label.clone(),
                Some(usage),
                Some("%"),
                Value::Null,
            ));
        }
    }
    samples
}

fn read_numeric(path: &Path) -> Option<f64> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<f64>().ok()
}

fn cpu_frequency_samples(ts: f64) -> Vec<MetricSample> {
    let root = Path::new("/sys/devices/system/cpu");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut samples = Vec::new();
    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };
        if !name.starts_with("cpu") || name.len() < 4 {
            continue;
        }
        let freq_path = entry.path().join("cpufreq").join("scaling_cur_freq");
        if let Some(khz) = read_numeric(&freq_path) {
            let mhz = khz / 1000.0;
            samples.push(MetricSample::new(
                ts,
                MetricKind::CpuFrequency,
                name,
                Some(mhz),
                Some("MHz"),
                Value::Null,
            ));
        }
    }
    samples
}

fn parse_meminfo() -> Option<(f64, f64)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = None;
    let mut available_kb = None;
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        match parts.next()? {
            "MemTotal:" => total_kb = parts.next()?.parse::<f64>().ok(),
            "MemAvailable:" => available_kb = parts.next()?.parse::<f64>().ok(),
            _ => continue,
        }
    }
    match (total_kb, available_kb) {
        (Some(total), Some(avail)) => Some((total * 1024.0, avail * 1024.0)),
        _ => None,
    }
}

fn memory_samples(ts: f64) -> Vec<MetricSample> {
    let (total, available) = match parse_meminfo() {
        Some(v) => v,
        None => return Vec::new(),
    };
    let used = (total - available).max(0.0);
    let details = json!({
        "total_bytes": total,
        "available_bytes": available,
        "used_bytes": used
    });
    vec![MetricSample::new(
        ts,
        MetricKind::MemoryUsage,
        "memory",
        Some(used),
        Some("bytes"),
        details,
    )]
}

fn network_samples(ts: f64) -> Vec<MetricSample> {
    let content = match fs::read_to_string("/proc/net/dev") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    parse_network_dev(&content, ts)
}

/// Pure parser for `/proc/net/dev`. See [`parse_cpu_times`] for rationale.
fn parse_network_dev(content: &str, ts: f64) -> Vec<MetricSample> {
    let mut samples = Vec::new();
    for line in content.lines().skip(2) {
        let mut parts = line.split(':');
        let iface = match parts.next() {
            Some(name) => name.trim().to_string(),
            None => continue,
        };
        if iface.is_empty() {
            continue;
        }
        let stats_part = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        let stats: Vec<&str> = stats_part.split_whitespace().collect();
        if stats.len() < 16 {
            continue;
        }
        let rx_bytes = stats[0].parse::<f64>().ok();
        let tx_bytes = stats[8].parse::<f64>().ok();
        let total = match (rx_bytes, tx_bytes) {
            (Some(rx), Some(tx)) => Some(rx + tx),
            _ => None,
        };
        let details = json!({
            "rx_bytes": rx_bytes,
            "tx_bytes": tx_bytes
        });
        samples.push(MetricSample::new(
            ts,
            MetricKind::NetworkBytes,
            iface,
            total,
            Some("bytes"),
            details,
        ));
    }
    samples
}

fn disk_samples(ts: f64) -> Vec<MetricSample> {
    let path = Path::new("/");
    let c_path = match CString::new(path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Vec::new();
    }
    let block_size = stat.f_frsize;
    let total = block_size * stat.f_blocks;
    let free = block_size * stat.f_bfree;
    let available = block_size * stat.f_bavail;
    let used = total.saturating_sub(free);
    let details = json!({
        "total_bytes": total as f64,
        "available_bytes": available as f64,
        "free_bytes": free as f64
    });
    vec![MetricSample::new(
        ts,
        MetricKind::DiskUsage,
        path.display().to_string(),
        Some(used as f64),
        Some("bytes"),
        details,
    )]
}

fn temperature_samples(ts: f64) -> Vec<MetricSample> {
    let mut samples = Vec::new();

    // Read from /sys/class/thermal. We keep track of the hwmon devices that
    // each thermal zone exposes (via the `thermal_zone*/hwmonN` symlinks the
    // kernel publishes) so we can avoid duplicating their reading through the
    // hwmon path below. Without this dedup the temperature chart shows the
    // same physical sensor twice on most systems.
    let mut thermal_hwmon_paths: Vec<PathBuf> = Vec::new();
    let thermal_root = Path::new("/sys/class/thermal");
    if let Ok(entries) = fs::read_dir(thermal_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
            {
                continue;
            }
            // Capture any hwmon symlink that lives under the zone; we skip
            // those during the hwmon sweep.
            if let Ok(hwmon_entries) = fs::read_dir(&path) {
                for sub in hwmon_entries.flatten() {
                    let sub_name = sub.file_name().to_string_lossy().to_string();
                    if sub_name.starts_with("hwmon") {
                        if let Ok(real) = fs::canonicalize(sub.path()) {
                            thermal_hwmon_paths.push(real);
                        }
                    }
                }
            }
            let label = fs::read_to_string(path.join("type"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
            let temp_mc = match fs::read_to_string(path.join("temp"))
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
            {
                Some(v) => v,
                None => continue,
            };
            let temp_c = temp_mc / 1000.0;
            samples.push(MetricSample::new(
                ts,
                MetricKind::Temperature,
                label,
                Some(temp_c),
                Some("C"),
                Value::Null,
            ));
        }
    }

    // Read from /sys/class/hwmon, skipping the ones already exported via
    // /sys/class/thermal above.
    let hwmon_root = Path::new("/sys/class/hwmon");
    if let Ok(entries) = fs::read_dir(hwmon_root) {
        for entry in entries.flatten() {
            let hwmon_path = entry.path();
            let canonical = fs::canonicalize(&hwmon_path).unwrap_or(hwmon_path.clone());
            if thermal_hwmon_paths.iter().any(|p| p == &canonical) {
                continue;
            }
            let name = fs::read_to_string(hwmon_path.join("name"))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
            let sensor_entries = match fs::read_dir(&hwmon_path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for sensor in sensor_entries.flatten() {
                let fname = sensor.file_name().to_string_lossy().to_string();
                if !fname.starts_with("temp") || !fname.ends_with("_input") {
                    continue;
                }
                let temp_mc = match fs::read_to_string(sensor.path())
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                {
                    Some(v) => v,
                    None => continue,
                };
                let temp_c = temp_mc / 1000.0;

                // Try to get a label for this sensor
                let label_file = fname.replace("_input", "_label");
                let label = fs::read_to_string(hwmon_path.join(&label_file))
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| fname.trim_end_matches("_input").to_string());

                let source = format!("{name}:{label}");
                samples.push(MetricSample::new(
                    ts,
                    MetricKind::Temperature,
                    source,
                    Some(temp_c),
                    Some("C"),
                    Value::Null,
                ));
            }
        }
    }

    samples
}

fn parse_pp_dpm_sclk(path: &Path) -> Option<f64> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if !line.contains('*') {
            continue;
        }
        for token in line.split_whitespace() {
            if let Some(raw) = token
                .strip_suffix("Mhz")
                .or_else(|| token.strip_suffix("MHz"))
            {
                if let Ok(value) = raw.parse::<f64>() {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn gpu_samples(ts: f64) -> Vec<MetricSample> {
    let root = Path::new("/sys/class/drm");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut samples = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") {
            continue;
        }
        let device = entry.path().join("device");
        let usage = ["gpu_busy_percent", "busy_percent", "gt_busy_percent"]
            .iter()
            .find_map(|f| read_numeric(&device.join(f)));
        if let Some(value) = usage {
            samples.push(MetricSample::new(
                ts,
                MetricKind::GpuUsage,
                name.clone(),
                Some(value),
                Some("%"),
                Value::Null,
            ));
        }

        let freq = read_numeric(&device.join("gt_cur_freq_mhz"))
            .or_else(|| parse_pp_dpm_sclk(&device.join("pp_dpm_sclk")));
        if let Some(mhz) = freq {
            samples.push(MetricSample::new(
                ts,
                MetricKind::GpuFrequency,
                name.clone(),
                Some(mhz),
                Some("MHz"),
                Value::Null,
            ));
        }
    }
    samples
}

fn power_samples(ts: f64) -> Vec<MetricSample> {
    let root = Path::new("/sys/class/hwmon");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut samples = Vec::new();
    for entry in entries.flatten() {
        let hwmon_path = entry.path();
        let name = fs::read_to_string(hwmon_path.join("name"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
        let sensor_entries = match fs::read_dir(&hwmon_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for sensor in sensor_entries.flatten() {
            let fname = sensor.file_name().to_string_lossy().to_string();
            if !fname.starts_with("power") || !fname.ends_with("_input") {
                continue;
            }
            let raw_value = match fs::read_to_string(sensor.path())
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
            {
                Some(v) => v,
                None => continue,
            };
            let watts = raw_value / 1_000_000.0;
            let source = format!("{name}:{}", fname.trim_end_matches("_input"));
            // Sanity threshold: a typical laptop SoC package draw is well
            // under 200W. Anything north of 1000W almost certainly means the
            // sensor reports in milliwatts rather than microwatts, and a
            // 1000×-scaled reading would silently corrupt the power chart.
            // We keep the value (so the user can audit it) but warn loudly.
            if watts > 1000.0 {
                log::warn!(
                    "power reading {watts:.0} W from {source} exceeds sanity \
                     threshold; sensor may report in mW rather than µW"
                );
            }
            samples.push(MetricSample::new(
                ts,
                MetricKind::PowerDraw,
                source,
                Some(watts),
                Some("W"),
                Value::Null,
            ));
        }
    }
    samples
}

pub fn collect_metrics(ts: f64) -> Vec<MetricSample> {
    let cpu_usage_handle = thread::spawn(move || cpu_usage_samples(ts));

    let mut metrics = Vec::new();
    metrics.extend(cpu_frequency_samples(ts));
    metrics.extend(memory_samples(ts));
    metrics.extend(network_samples(ts));
    metrics.extend(disk_samples(ts));
    metrics.extend(temperature_samples(ts));
    metrics.extend(gpu_samples(ts));
    metrics.extend(power_samples(ts));
    if let Ok(cpu_samples) = cpu_usage_handle.join() {
        metrics.extend(cpu_samples);
    }
    metrics
}

// ponytail: parse_pp_dpm_sclk and the per-source parsers backing gpu_samples/
// power_samples/temperature_samples still read sysfs directly and have no unit
// tests. Add when coverage of these becomes valuable beyond the hottest path.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cpu_times_reads_aggregate_and_per_core() {
        let content = "cpu  100 200 300 400 50 10 5 5 0 0\n\
                       cpu0 10 20 30 40 5 1 0 1 0 0\n\
                       intr 12345\n";
        let times = parse_cpu_times(content).expect("should parse");
        assert_eq!(times.len(), 2);
        assert_eq!(times[0].label, "cpu");
        assert_eq!(times[0].user, 100);
        assert_eq!(times[0].steal, 5);
        assert_eq!(times[1].label, "cpu0");
        assert_eq!(times[1].idle, 40);
    }

    #[test]
    fn parse_cpu_times_handles_missing_columns() {
        // A `cpu` line with fewer than 8 numeric fields is skipped — it is
        // malformed and not safe to compute deltas from.
        let content = "cpu 1 2 3\n";
        assert!(parse_cpu_times(content).is_none());
    }

    #[test]
    fn parse_network_dev_extracts_interfaces() {
        let content = "Inter-|   Receive                                                \
                       |  Transmit\n\
                       face |bytes    packets errs drop fifo frame compressed \
                       multicast|bytes    packets errs drop fifo colls carrier compressed\n\
                       eth0:   1000 50 0 0 0 0 0 0 2000 30 0 0 0 0 0 0\n\
                       lo: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0\n";
        let samples = parse_network_dev(content, 1234.5);
        assert_eq!(samples.len(), 2);
        let eth0 = samples.iter().find(|m| m.source == "eth0").unwrap();
        assert_eq!(eth0.value, Some(3000.0)); // rx + tx
        assert_eq!(
            eth0.details.get("rx_bytes").and_then(|v| v.as_f64()),
            Some(1000.0)
        );
        assert_eq!(
            eth0.details.get("tx_bytes").and_then(|v| v.as_f64()),
            Some(2000.0)
        );
    }
}
