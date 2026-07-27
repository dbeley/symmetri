use std::borrow::Cow;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, TimeZone, Utc};

use crate::metrics::{MetricKind, MetricSample};
use crate::timeframe::Timeframe;

/// Maximum gap (in hours) between two consecutive samples that may still be
/// interpolated. Beyond this length we assume the machine was off or the
/// collector missed a tick, and the segment between the two samples is
/// dropped (rates not averaged, line plots break). Tuned for the default
/// 5-minute systemd cadence with a 2× margin for jitter.
pub const MAX_GAP_HOURS: f64 = 10.0 / 60.0;

/// Maximum gap expressed in seconds, used by the plotting layer where we
/// already work in epoch seconds.
#[allow(dead_code)]
pub const MAX_GAP_SECONDS: f64 = MAX_GAP_HOURS * 3600.0;

fn sanitize_component(value: &str) -> Cow<'_, str> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Cow::Borrowed(value);
    }
    let replaced: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    Cow::Owned(replaced)
}

pub fn default_graph_path(
    timeframe: &str,
    base_dir: Option<&Path>,
    now: Option<DateTime<Local>>,
) -> PathBuf {
    let current = now.unwrap_or_else(Local::now);
    let tz_label = current.format("%Z").to_string();
    let tz_name = sanitize_component(&tz_label);
    let timeframe_label = timeframe.replace('-', "_");
    let timestamp = current.format("%Y-%m-%d_%H-%M-%S");
    let filename = format!("symmetri_{}_{}_{}.png", timeframe_label, timestamp, tz_name);
    base_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(filename)
}

pub fn bucket_span_seconds(timeframe: &Timeframe, data_span_seconds: Option<f64>) -> i64 {
    // Note: tightened at the short end of the table so 5-minute-collected
    // data produces dense polygon lines even on `--days 1`/`--hours 6` charts
    // (previously those ranges drew only a handful of 10-min or 1h buckets).
    let window = timeframe
        .seconds
        .or(data_span_seconds)
        .unwrap_or(7.0 * 24.0 * 3600.0);

    match window {
        w if w <= 2.0 * 3600.0 => 5 * 60,
        w if w <= 6.0 * 3600.0 => 5 * 60,
        w if w <= 24.0 * 3600.0 => 15 * 60,
        w if w <= 3.0 * 24.0 * 3600.0 => 3600,
        w if w <= 7.0 * 24.0 * 3600.0 => 6 * 3600,
        w if w <= 30.0 * 24.0 * 3600.0 => 24 * 3600,
        w if w <= 90.0 * 24.0 * 3600.0 => 3 * 24 * 3600,
        _ => 7 * 24 * 3600,
    }
}

/// Aligns an epoch-seconds timestamp to a bucket boundary in local time.
///
/// DST-safe: samples that fall in a spring-forward gap (`None` from
/// `timestamp_opt`) are aligned using a UTC snapshot of the offset bracketing
/// the gap, so we never panic. DST folds pick the earlier of the two
/// ambiguous local times for consistent bucketing.
pub fn bucket_start(ts: f64, bucket_seconds: i64) -> DateTime<Local> {
    local_bucket_start(ts as i64, bucket_seconds, 0)
}

fn local_bucket_start(ts_secs: i64, bucket_seconds: i64, nanos: u32) -> DateTime<Local> {
    let local_dt = match Local.timestamp_opt(ts_secs, nanos).earliest() {
        Some(dt) => dt,
        None => {
            // Spring-forward gap (or missing tzdata). Fall back to UTC-as-local:
            // the wall-clock label may be off by up to an hour, but the instant
            // stays correct and bucket alignment degrades gracefully.
            let utc = Utc
                .timestamp_opt(ts_secs, nanos)
                .single()
                .expect("epoch seconds are always representable as UTC");
            DateTime::<Local>::from(utc)
        }
    };
    let offset_seconds = -local_dt.offset().utc_minus_local();
    let bucket_epoch = (((ts_secs as f64 + offset_seconds as f64) / bucket_seconds as f64).floor()
        * bucket_seconds as f64)
        - offset_seconds as f64;
    let aligned = bucket_epoch.max(0.0) as i64;
    match Local.timestamp_opt(aligned, 0).earliest() {
        Some(dt) => dt,
        None => DateTime::<Local>::from(
            Utc.timestamp_opt(aligned, 0)
                .single()
                .expect("aligned epoch is always representable as UTC"),
        ),
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct AverageRates {
    pub discharge_w: Option<f64>,
    pub charge_w: Option<f64>,
}

#[derive(Default)]
struct RateAccumulator {
    delta: f64,
    hours: f64,
}

impl RateAccumulator {
    fn record(&mut self, delta_wh: f64, dt_hours: f64) {
        self.delta += delta_wh;
        self.hours += dt_hours;
    }

    fn average(&self) -> Option<f64> {
        if self.hours == 0.0 || self.delta == 0.0 {
            None
        } else {
            Some(self.delta / self.hours)
        }
    }
}

pub fn average_rates<'a>(
    battery_metrics: impl IntoIterator<Item = &'a MetricSample>,
) -> AverageRates {
    let mut discharge = RateAccumulator::default();
    let mut charge = RateAccumulator::default();

    let energy_now_samples: Vec<_> = battery_metrics
        .into_iter()
        .filter(|m| m.kind == MetricKind::BatteryEnergyNow && m.value.is_some())
        .collect();

    let mut iter = energy_now_samples.iter();
    let mut previous = match iter.next() {
        Some(sample) => sample,
        None => return AverageRates::default(),
    };

    for current in iter {
        if current.ts < previous.ts {
            previous = current;
            continue;
        }
        let dt_hours = (current.ts - previous.ts) / 3600.0;
        if dt_hours > 0.0 && dt_hours <= MAX_GAP_HOURS {
            let delta = current.value.unwrap() - previous.value.unwrap();
            if delta > 0.0 && is_charging(previous) && is_charging(current) {
                charge.record(delta, dt_hours);
            } else if delta < 0.0 && is_discharging(previous) && is_discharging(current) {
                discharge.record(-delta, dt_hours);
            }
        }
        previous = current;
    }

    AverageRates {
        discharge_w: discharge.average(),
        charge_w: charge.average(),
    }
}

pub(crate) fn is_discharging(sample: &MetricSample) -> bool {
    sample
        .details
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case("discharging"))
        .unwrap_or(true)
}

pub(crate) fn is_charging(sample: &MetricSample) -> bool {
    sample
        .details
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case("charging"))
        .unwrap_or(false)
}

#[allow(dead_code)]
pub fn average_discharge_w(battery_metrics: &[MetricSample]) -> Option<f64> {
    average_rates(battery_metrics).discharge_w
}

#[allow(dead_code)]
pub fn average_charge_w(battery_metrics: &[MetricSample]) -> Option<f64> {
    average_rates(battery_metrics).charge_w
}

pub fn estimate_runtime_hours(
    avg_discharge_w: Option<f64>,
    battery_metrics: &[MetricSample],
) -> Option<f64> {
    let avg = avg_discharge_w?;
    if avg <= 0.0 {
        return None;
    }
    let capacity_wh = battery_metrics
        .iter()
        .find(|m| m.kind == MetricKind::BatteryEnergyFull && m.value.is_some())
        .or_else(|| {
            battery_metrics
                .iter()
                .find(|m| m.kind == MetricKind::BatteryEnergyFullDesign && m.value.is_some())
        })
        .and_then(|m| m.value)?;
    if capacity_wh <= 0.0 {
        return None;
    }
    Some(capacity_wh / avg)
}

pub fn format_runtime(hours: Option<f64>) -> String {
    match hours {
        None => "--".to_string(),
        Some(value) if value.is_sign_negative() || !value.is_finite() => "--".to_string(),
        Some(value) => {
            let minutes = (value * 60.0).floor() as i64;
            let hrs = minutes / 60;
            let mins = minutes % 60;
            format!("{hrs}h{mins:02}m")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use serde_json::json;

    fn battery_metric(
        ts: f64,
        kind: MetricKind,
        energy: f64,
        status: Option<&str>,
    ) -> MetricSample {
        MetricSample {
            ts,
            kind,
            source: "BAT0".to_string(),
            value: Some(energy),
            unit: Some("Wh".to_string()),
            details: json!({"status": status}),
        }
    }

    #[test]
    fn default_graph_path_has_timeframe_and_timestamp() {
        let now = Local.with_ymd_and_hms(2025, 11, 28, 1, 30, 42).unwrap();
        let path = default_graph_path("last_3_hours", Some(Path::new("/tmp")), Some(now));
        let tz_label = now.format("%Z").to_string();
        let tz = sanitize_component(&tz_label);
        let expected = PathBuf::from(format!(
            "/tmp/symmetri_last_3_hours_2025-11-28_01-30-42_{}.png",
            tz
        ));
        assert_eq!(path, expected);
    }

    #[test]
    fn average_discharge_and_runtime_estimates() {
        let metrics = vec![
            battery_metric(0.0, MetricKind::BatteryEnergyNow, 60.0, None),
            battery_metric(0.0, MetricKind::BatteryEnergyFull, 60.0, None),
            battery_metric(0.0, MetricKind::BatteryEnergyFullDesign, 70.0, None),
            battery_metric(300.0, MetricKind::BatteryEnergyNow, 59.6, None),
            battery_metric(600.0, MetricKind::BatteryEnergyNow, 59.2, None),
        ];
        let avg = average_discharge_w(&metrics).unwrap();
        let runtime_hours = estimate_runtime_hours(Some(avg), &metrics).unwrap();
        assert!((avg - 4.8).abs() < 0.01);
        assert!((runtime_hours - 12.5).abs() < 0.01);
        assert_eq!(format_runtime(Some(runtime_hours)), "12h30m");
    }

    #[test]
    fn average_discharge_ignores_large_gaps() {
        let metrics = vec![
            battery_metric(0.0, MetricKind::BatteryEnergyNow, 60.0, None),
            battery_metric(300.0, MetricKind::BatteryEnergyNow, 59.5, None),
            battery_metric(1800.0, MetricKind::BatteryEnergyNow, 59.4, None),
        ];
        let avg = average_discharge_w(&metrics).unwrap();
        assert!((avg - 6.0).abs() < 0.01);
    }

    #[test]
    fn average_discharge_ignores_charging_segments() {
        let metrics = vec![
            battery_metric(0.0, MetricKind::BatteryEnergyNow, 60.0, Some("Discharging")),
            battery_metric(
                300.0,
                MetricKind::BatteryEnergyNow,
                59.0,
                Some("Discharging"),
            ),
            battery_metric(600.0, MetricKind::BatteryEnergyNow, 60.0, Some("Charging")),
            battery_metric(
                900.0,
                MetricKind::BatteryEnergyNow,
                59.5,
                Some("Discharging"),
            ),
            battery_metric(
                1200.0,
                MetricKind::BatteryEnergyNow,
                59.0,
                Some("Discharging"),
            ),
        ];
        let avg = average_discharge_w(&metrics).unwrap();
        assert!((avg - 9.0).abs() < 0.01);
    }

    #[test]
    fn average_charge_tracks_charging_only() {
        let metrics = vec![
            battery_metric(0.0, MetricKind::BatteryEnergyNow, 50.0, Some("Charging")),
            battery_metric(300.0, MetricKind::BatteryEnergyNow, 52.0, Some("Charging")),
            battery_metric(600.0, MetricKind::BatteryEnergyNow, 52.5, Some("Charging")),
            battery_metric(
                900.0,
                MetricKind::BatteryEnergyNow,
                52.2,
                Some("Discharging"),
            ),
            battery_metric(1200.0, MetricKind::BatteryEnergyNow, 53.0, Some("Charging")),
            battery_metric(1500.0, MetricKind::BatteryEnergyNow, 54.5, Some("Charging")),
        ];
        let avg = average_charge_w(&metrics).unwrap();
        assert!((avg - 16.0).abs() < 0.01);
    }

    #[test]
    fn average_rates_compute_charge_and_discharge_together() {
        let metrics = vec![
            battery_metric(0.0, MetricKind::BatteryEnergyNow, 50.0, Some("Charging")),
            battery_metric(300.0, MetricKind::BatteryEnergyNow, 51.0, Some("Charging")),
            battery_metric(600.0, MetricKind::BatteryEnergyNow, 52.0, Some("Charging")),
            battery_metric(
                900.0,
                MetricKind::BatteryEnergyNow,
                51.5,
                Some("Discharging"),
            ),
            battery_metric(
                1200.0,
                MetricKind::BatteryEnergyNow,
                51.0,
                Some("Discharging"),
            ),
        ];

        let rates = average_rates(&metrics);
        assert!((rates.charge_w.unwrap() - 12.0).abs() < 0.01);
        assert!((rates.discharge_w.unwrap() - 6.0).abs() < 0.01);
    }

    #[test]
    fn bucket_alignment_matches_expected_windows() {
        use crate::timeframe::build_timeframe;
        let timeframe = build_timeframe(6, 0, 0, false).unwrap();
        let span = bucket_span_seconds(&timeframe, None);
        let sample_dt = Local::now()
            .with_minute(37)
            .unwrap()
            .with_second(42)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let bucket = bucket_start(sample_dt.timestamp() as f64, span);

        assert_eq!(span, 5 * 60);
        assert_eq!(bucket.minute() % 5, 0);
        assert_eq!(bucket.second(), 0);

        let one_day = build_timeframe(0, 1, 0, false).unwrap();
        let span_day = bucket_span_seconds(&one_day, None);
        let bucket_day = bucket_start(sample_dt.timestamp() as f64, span_day);
        assert_eq!(span_day, 15 * 60);
        assert_eq!(bucket_day.minute() % 15, 0);
        assert_eq!(bucket_day.second(), 0);
    }

    #[test]
    fn short_timeframes_use_five_minute_buckets() {
        use crate::timeframe::build_timeframe;
        let timeframe = build_timeframe(1, 0, 0, false).unwrap();
        let span = bucket_span_seconds(&timeframe, None);
        let sample_dt = Local::now()
            .with_minute(12)
            .unwrap()
            .with_second(55)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let bucket = bucket_start(sample_dt.timestamp() as f64, span);

        assert_eq!(span, 5 * 60);
        assert_eq!(bucket.minute() % 5, 0);
        assert_eq!(bucket.second(), 0);
    }

    #[test]
    fn all_time_uses_data_span_for_buckets() {
        use crate::timeframe::build_timeframe;
        let timeframe = build_timeframe(6, 0, 0, true).unwrap();
        let span = bucket_span_seconds(&timeframe, Some(6.0 * 3600.0));
        assert_eq!(span, 5 * 60);

        let weekly = bucket_span_seconds(&timeframe, Some(200.0 * 24.0 * 3600.0));
        assert_eq!(weekly, 7 * 24 * 3600);
    }
}
