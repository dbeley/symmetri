use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Local, TimeDelta};
use log::{info, warn};
use plotters::coord::Shift;
use plotters::prelude::*;
use plotters::series::LineSeries;

use crate::cli_helpers::{bucket_span_seconds, bucket_start, MAX_GAP_SECONDS};
use crate::metrics::{MetricKind, MetricSample, ReportPreset};
use crate::timeframe::Timeframe;

struct MetricSeries {
    label: String,
    points: SeriesPoints,
}

type SeriesPoints = Vec<(DateTime<Local>, f64)>;

struct ChartSpec {
    title: String,
    y_desc: String,
    series: Vec<MetricSeries>,
}

pub fn render_plot(
    metrics: &[MetricSample],
    presets: &[ReportPreset],
    timeframe: &Timeframe,
    output: &Path,
) -> Result<()> {
    let bucket_seconds = bucket_span_seconds(timeframe, data_span_seconds(metrics));
    let charts = build_charts(metrics, presets, timeframe, bucket_seconds);
    if charts.is_empty() {
        warn!("No values available to plot for selected presets");
        return Ok(());
    }

    let rows = charts.len().max(1);
    let height = (rows as u32 * 260).max(260);
    let root = BitMapBackend::new(output, (1280, height)).into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((rows, 1));

    for (area, chart) in areas.into_iter().zip(charts.iter()) {
        plot_chart(area, chart, bucket_seconds)?;
    }

    root.present()?;
    info!("Saved plot to {}", output.display());
    Ok(())
}

fn build_charts(
    metrics: &[MetricSample],
    presets: &[ReportPreset],
    timeframe: &Timeframe,
    bucket_seconds: i64,
) -> Vec<ChartSpec> {
    let mut charts = Vec::new();
    let label = timeframe.label.replace('_', " ");

    if presets.contains(&ReportPreset::Battery) {
        let mut series = Vec::new();
        let percent_points = bucket_mean_series(
            metrics,
            MetricKind::BatteryPercentage,
            bucket_seconds,
            |v, _| v,
        );
        if !percent_points.is_empty() {
            series.push(MetricSeries {
                label: "Charge %".to_string(),
                points: percent_points,
            });
        }
        let health_points =
            bucket_mean_series(metrics, MetricKind::BatteryHealth, bucket_seconds, |v, _| v);
        if !health_points.is_empty() {
            series.push(MetricSeries {
                label: "Health %".to_string(),
                points: health_points,
            });
        }
        if !series.is_empty() {
            charts.push(ChartSpec {
                title: format!("Battery ({label})"),
                y_desc: "Percent".to_string(),
                series,
            });
        }

        let power_draw =
            bucket_mean_series(metrics, MetricKind::PowerDraw, bucket_seconds, |v, _| v);
        if !power_draw.is_empty() {
            charts.push(ChartSpec {
                title: format!("Power draw ({label})"),
                y_desc: "Watts".to_string(),
                series: vec![MetricSeries {
                    label: "Discharge".to_string(),
                    points: power_draw,
                }],
            });
        }
    }

    if presets.contains(&ReportPreset::Cpu) {
        let usage =
            bucket_mean_series_by_source(metrics, MetricKind::CpuUsage, bucket_seconds, |v, _| v);
        if !usage.is_empty() {
            charts.push(ChartSpec {
                title: format!("CPU usage ({label})"),
                y_desc: "Percent".to_string(),
                series: usage,
            });
        }
        let freq = bucket_mean_series_by_source(
            metrics,
            MetricKind::CpuFrequency,
            bucket_seconds,
            |v, _| v,
        );
        if !freq.is_empty() {
            charts.push(ChartSpec {
                title: format!("CPU frequency ({label})"),
                y_desc: "MHz".to_string(),
                series: freq,
            });
        }
    }

    if presets.contains(&ReportPreset::Gpu) {
        let usage =
            bucket_mean_series_by_source(metrics, MetricKind::GpuUsage, bucket_seconds, |v, _| v);
        if !usage.is_empty() {
            charts.push(ChartSpec {
                title: format!("GPU usage ({label})"),
                y_desc: "Percent".to_string(),
                series: usage,
            });
        }
        let freq = bucket_mean_series_by_source(
            metrics,
            MetricKind::GpuFrequency,
            bucket_seconds,
            |v, _| v,
        );
        if !freq.is_empty() {
            charts.push(ChartSpec {
                title: format!("GPU frequency ({label})"),
                y_desc: "MHz".to_string(),
                series: freq,
            });
        }
    }

    if presets.contains(&ReportPreset::Memory) {
        let memory = bucket_mean_series(
            metrics,
            MetricKind::MemoryUsage,
            bucket_seconds,
            |used, _| bytes_to_gib(used),
        );
        if !memory.is_empty() {
            charts.push(ChartSpec {
                title: format!("Memory usage ({label})"),
                y_desc: "GiB".to_string(),
                series: vec![MetricSeries {
                    label: "Used".to_string(),
                    points: memory,
                }],
            });
        }
    }

    if presets.contains(&ReportPreset::Disk) {
        let disk = bucket_mean_series(metrics, MetricKind::DiskUsage, bucket_seconds, |used, _| {
            bytes_to_gib(used)
        });
        if !disk.is_empty() {
            charts.push(ChartSpec {
                title: format!("Disk usage ({label})"),
                y_desc: "GiB".to_string(),
                series: vec![MetricSeries {
                    label: "Used".to_string(),
                    points: disk,
                }],
            });
        }
    }

    if presets.contains(&ReportPreset::Network) {
        let (rx, tx) = network_bucket_series(metrics, bucket_seconds);
        let mut series = Vec::new();
        if !rx.is_empty() {
            series.push(MetricSeries {
                label: "Download".to_string(),
                points: rx,
            });
        }
        if !tx.is_empty() {
            series.push(MetricSeries {
                label: "Upload".to_string(),
                points: tx,
            });
        }
        if !series.is_empty() {
            charts.push(ChartSpec {
                title: format!("Network data transferred ({label})"),
                y_desc: "MiB".to_string(),
                series,
            });
        }
    }

    if presets.contains(&ReportPreset::Temperature) {
        let temps = bucket_mean_series_by_source(
            metrics,
            MetricKind::Temperature,
            bucket_seconds,
            |v, _| v,
        );
        if !temps.is_empty() {
            charts.push(ChartSpec {
                title: format!("Temperature ({label})"),
                y_desc: "Celsius".to_string(),
                series: temps,
            });
        }
    }

    charts
}

fn plot_chart(
    area: DrawingArea<BitMapBackend, Shift>,
    chart: &ChartSpec,
    bucket_seconds: i64,
) -> Result<()> {
    let mut all_points: SeriesPoints = Vec::new();
    for series in &chart.series {
        all_points.extend_from_slice(&series.points);
    }

    let Some(min_ts) = all_points.iter().map(|(ts, _)| *ts).min() else {
        return Ok(());
    };
    let Some(max_ts) = all_points.iter().map(|(ts, _)| *ts).max() else {
        return Ok(());
    };

    // Guard against degenerate x-range (single bucket). plotters' coordinate
    // builder does not handle zero-width ranges; pad by ±60s.
    let (min_ts, max_ts) = if max_ts == min_ts {
        let pad = TimeDelta::seconds(60);
        (min_ts - pad, max_ts + pad)
    } else {
        (min_ts, max_ts)
    };

    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (_, value) in &all_points {
        min_y = min_y.min(*value);
        max_y = max_y.max(*value);
    }
    if (max_y - min_y).abs() < 1e-6 {
        min_y -= 1.0;
        max_y += 1.0;
    }
    let padding = (max_y - min_y) * 0.05;
    let y_min = min_y - padding;
    let y_max = max_y + padding;

    let mut chart_ctx = ChartBuilder::on(&area)
        .caption(&chart.title, ("sans-serif", 20).into_font())
        .margin(12)
        .x_label_area_size(36)
        .y_label_area_size(60)
        .build_cartesian_2d(min_ts..max_ts, y_min..y_max)?;

    chart_ctx
        .configure_mesh()
        .x_labels(5)
        .y_labels(6)
        .x_desc("Time (local)")
        .y_desc(chart.y_desc.as_str())
        .light_line_style(WHITE.mix(0.15))
        .draw()?;

    for (idx, series) in chart.series.iter().enumerate() {
        let color = Palette99::pick(idx).to_rgba();
        // Break the line wherever two consecutive points are more than
        // MAX_GAP_SECONDS apart (machine off, sleep, missed collector ticks).
        // Each run is a separate LineSeries, but only the first carries the
        // legend entry so duplicates don't appear.
        let mut segments = split_by_gaps(&series.points, bucket_seconds).into_iter();
        if let Some(first) = segments.next() {
            chart_ctx
                .draw_series(LineSeries::new(first, &color))?
                .label(series.label.clone())
                .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 10, y)], color));
            for rest in segments {
                chart_ctx.draw_series(LineSeries::new(rest, &color))?;
            }
        }
    }

    chart_ctx
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    Ok(())
}

/// Splits a sorted point list into contiguous runs separated by gaps longer
/// than the bucket width can bridge. Each returned Vec is suitable as input
/// to one `LineSeries` so the line does not interpolate across the gap.
/// Input is expected to be sorted ascending by timestamp.
///
/// The threshold is `max(MAX_GAP_SECONDS, 2 * bucket_seconds)`: a single
/// missing bucket stays connected (its neighbours are exactly one bucket
/// apart, well under `2 * bucket_seconds`), but a stretch covering more
/// than one bucket of downtime splits the line. Without scaling to the
/// bucket width, larger buckets (15m / 6h) would always exceed the raw
/// 10-minute cadence threshold and every run would collapse to a single
/// point, producing an empty plot.
fn split_by_gaps(points: &[(DateTime<Local>, f64)], bucket_seconds: i64) -> Vec<SeriesPoints> {
    let threshold_secs = (MAX_GAP_SECONDS as i64).max(bucket_seconds.saturating_mul(2));
    let max_gap = TimeDelta::seconds(threshold_secs);
    let mut out = Vec::new();
    let mut current: SeriesPoints = Vec::new();
    for point in points {
        if let Some(last) = current.last() {
            let dt = point.0.signed_duration_since(last.0);
            if dt > max_gap {
                // `current.last()` returning Some means current is non-empty.
                out.push(std::mem::take(&mut current));
            }
        }
        current.push(*point);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Averages all samples of `kind` into fixed-width local-time buckets. Empty
/// buckets are absent from the output — that absence is what produces a gap
/// when [`split_by_gaps`] walks the series.
fn bucket_mean_series<F>(
    metrics: &[MetricSample],
    kind: MetricKind,
    bucket_seconds: i64,
    mut map_value: F,
) -> SeriesPoints
where
    F: FnMut(f64, &MetricSample) -> f64,
{
    let mut grouped: BTreeMap<DateTime<Local>, Vec<f64>> = BTreeMap::new();
    for sample in metrics.iter().filter(|m| m.kind == kind) {
        if let Some(value) = sample.value {
            let bucket = bucket_start(sample.ts, bucket_seconds);
            grouped
                .entry(bucket)
                .or_default()
                .push(map_value(value, sample));
        }
    }
    grouped
        .into_iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(dt, values)| {
            let avg = values.iter().sum::<f64>() / values.len() as f64;
            (dt, avg)
        })
        .collect()
}

/// Same as [`bucket_mean_series`] but keeps one series per `source` (e.g. one
/// line per CPU core / GPU / thermal zone).
fn bucket_mean_series_by_source<F>(
    metrics: &[MetricSample],
    kind: MetricKind,
    bucket_seconds: i64,
    mut map_value: F,
) -> Vec<MetricSeries>
where
    F: FnMut(f64, &MetricSample) -> f64,
{
    let mut grouped: BTreeMap<String, BTreeMap<DateTime<Local>, Vec<f64>>> = BTreeMap::new();
    for sample in metrics.iter().filter(|m| m.kind == kind) {
        if let Some(value) = sample.value {
            let bucket = bucket_start(sample.ts, bucket_seconds);
            grouped
                .entry(sample.source.clone())
                .or_default()
                .entry(bucket)
                .or_default()
                .push(map_value(value, sample));
        }
    }
    let mut series = Vec::new();
    for (source, buckets) in grouped {
        let mut points = Vec::new();
        for (dt, values) in buckets {
            if values.is_empty() {
                continue;
            }
            let avg = values.iter().sum::<f64>() / values.len() as f64;
            points.push((dt, avg));
        }
        if !points.is_empty() {
            series.push(MetricSeries {
                label: source,
                points,
            });
        }
    }
    series
}

fn network_bucket_series(
    metrics: &[MetricSample],
    bucket_seconds: i64,
) -> (SeriesPoints, SeriesPoints) {
    let mut by_iface: BTreeMap<&str, Vec<&MetricSample>> = BTreeMap::new();
    for sample in metrics
        .iter()
        .filter(|s| s.kind == MetricKind::NetworkBytes)
    {
        by_iface.entry(&sample.source).or_default().push(sample);
    }

    let mut rx_buckets: BTreeMap<DateTime<Local>, f64> = BTreeMap::new();
    let mut tx_buckets: BTreeMap<DateTime<Local>, f64> = BTreeMap::new();

    for (_iface, mut samples) in by_iface {
        samples.sort_by(|a, b| a.ts.total_cmp(&b.ts));

        for window in samples.windows(2) {
            let prev = window[0];
            let next = window[1];
            let dt = next.ts - prev.ts;
            if dt <= 0.0 {
                continue;
            }

            // Skip intervals spanning a gap larger than the cadence: the
            // counters were not sampled while the machine was off, so the
            // per-bucket total across that window is meaningless.
            if dt > MAX_GAP_SECONDS {
                continue;
            }

            let rx_delta = counter_delta(
                detail_number(prev, "rx_bytes"),
                detail_number(next, "rx_bytes"),
            );
            let tx_delta = counter_delta(
                detail_number(prev, "tx_bytes"),
                detail_number(next, "tx_bytes"),
            );

            if rx_delta > 0.0 || tx_delta > 0.0 {
                let bucket = bucket_start(next.ts, bucket_seconds);
                *rx_buckets.entry(bucket).or_insert(0.0) += rx_delta;
                *tx_buckets.entry(bucket).or_insert(0.0) += tx_delta;
            }
        }
    }

    let rx_series = rx_buckets
        .into_iter()
        .map(|(bucket, total)| (bucket, total / 1_048_576.0))
        .collect();
    let tx_series = tx_buckets
        .into_iter()
        .map(|(bucket, total)| (bucket, total / 1_048_576.0))
        .collect();

    (rx_series, tx_series)
}

fn counter_delta(previous: Option<f64>, current: Option<f64>) -> f64 {
    match (previous, current) {
        (Some(prev), Some(next)) if next >= prev => next - prev,
        _ => 0.0,
    }
}

fn detail_number(sample: &MetricSample, key: &str) -> Option<f64> {
    sample
        .details
        .get(key)
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
}

fn bytes_to_gib(used: f64) -> f64 {
    used / (1024.0 * 1024.0 * 1024.0)
}

/// Span (max−min ts) of the provided samples, used to choose a bucket width
/// consistent with the printed report tables.
fn data_span_seconds(metrics: &[MetricSample]) -> Option<f64> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut any = false;
    for m in metrics {
        if m.ts < min {
            min = m.ts;
            any = true;
        }
        if m.ts > max {
            max = m.ts;
        }
    }
    if !any {
        return None;
    }
    Some(max - min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn metric_sample(source: &str, ts: f64, value: f64, kind: MetricKind) -> MetricSample {
        MetricSample {
            ts,
            kind,
            source: source.to_string(),
            value: Some(value),
            unit: None,
            details: serde_json::Value::Null,
        }
    }

    #[test]
    fn bucket_mean_series_by_source_keeps_per_source_lines() {
        // 5-min bucket: 0 and 60 land in the same bucket for cpu0 → averaged.
        let metrics = vec![
            metric_sample("cpu0", 0.0, 10.0, MetricKind::CpuUsage),
            metric_sample("cpu1", 0.0, 20.0, MetricKind::CpuUsage),
            metric_sample("cpu0", 60.0, 30.0, MetricKind::CpuUsage),
        ];
        let series = bucket_mean_series_by_source(&metrics, MetricKind::CpuUsage, 5 * 60, |v, _| v);
        assert_eq!(series.len(), 2);
        let cpu0 = series.iter().find(|s| s.label == "cpu0").unwrap();
        let cpu1 = series.iter().find(|s| s.label == "cpu1").unwrap();
        // cpu0: (10 + 30) / 2 = 20 averaged into one bucket
        assert_eq!(cpu0.points.len(), 1);
        assert!((cpu0.points[0].1 - 20.0).abs() < 1e-9);
        assert_eq!(cpu1.points.len(), 1);
        assert!((cpu1.points[0].1 - 20.0).abs() < 1e-9);
    }

    #[test]
    fn split_by_gaps_breaks_at_downtime() {
        let base = chrono::Local.timestamp_opt(0, 0).single().unwrap();
        let a = base + TimeDelta::seconds(0);
        let b = base + TimeDelta::seconds(60);
        // Gap of 30 minutes (well above threshold = 2*5min = 600s).
        let c = base + TimeDelta::seconds(60 + 1800);
        let d = base + TimeDelta::seconds(60 + 1860);

        let points: SeriesPoints = vec![(a, 1.0), (b, 2.0), (c, 3.0), (d, 4.0)];
        let runs = split_by_gaps(&points, 5 * 60);
        assert_eq!(runs.len(), 2, "expected a break across the 30-min gap");
        assert_eq!(runs[0].len(), 2);
        assert_eq!(runs[1].len(), 2);
    }

    #[test]
    fn split_by_gaps_keeps_contiguous_run() {
        let base = chrono::Local.timestamp_opt(0, 0).single().unwrap();
        let points: SeriesPoints = vec![
            (base, 1.0),
            (base + TimeDelta::seconds(60), 2.0),
            (base + TimeDelta::seconds(120), 3.0),
        ];
        let runs = split_by_gaps(&points, 5 * 60);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 3);
    }

    #[test]
    fn split_by_gaps_breaks_exactly_at_threshold() {
        let base = chrono::Local.timestamp_opt(0, 0).single().unwrap();
        // For 5-min buckets, threshold is 2*300 = 600s. A gap of exactly
        // 600s stays connected (the comparison is `>`, not `>=`), 601s splits.
        let at_threshold: SeriesPoints =
            vec![(base, 1.0), (base + TimeDelta::seconds(2 * 5 * 60), 2.0)];
        assert_eq!(split_by_gaps(&at_threshold, 5 * 60).len(), 1);
        let over_threshold: SeriesPoints = vec![
            (base, 1.0),
            (base + TimeDelta::seconds(2 * 5 * 60 + 1), 2.0),
        ];
        assert_eq!(split_by_gaps(&over_threshold, 5 * 60).len(), 2);
    }

    #[test]
    fn split_by_gaps_keeps_run_for_wide_buckets() {
        // Regression: with buckets larger than MAX_GAP_SECONDS, adjacent
        // buckets stayed spaced far enough apart that the old fixed
        // 10-minute threshold treated every pair as downtime and collapsed
        // the line into single-point runs (rendering blank plots).
        let base = chrono::Local.timestamp_opt(0, 0).single().unwrap();
        // 6-hour buckets (used for --days 6): adjacent points live 6h apart.
        let points: SeriesPoints = vec![
            (base, 1.0),
            (base + TimeDelta::seconds(6 * 3600), 2.0),
            (base + TimeDelta::seconds(2 * 6 * 3600), 3.0),
        ];
        let runs = split_by_gaps(&points, 6 * 3600);
        assert_eq!(runs.len(), 1, "adjacent 6h buckets must stay connected");
        assert_eq!(runs[0].len(), 3);
    }

    #[test]
    fn split_by_gaps_splits_when_a_wide_bucket_is_missed() {
        let base = chrono::Local.timestamp_opt(0, 0).single().unwrap();
        // Two adjacent 6h buckets skipped (gap of 3 buckets = 18h).
        let points: SeriesPoints =
            vec![(base, 1.0), (base + TimeDelta::seconds(3 * 6 * 3600), 2.0)];
        let runs = split_by_gaps(&points, 6 * 3600);
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn split_by_gaps_empty_input() {
        assert!(split_by_gaps(&[], 5 * 60).is_empty());
    }

    #[test]
    fn network_bucket_series_uses_counter_deltas() {
        let mk = |ts: f64, rx: f64, tx: f64| MetricSample {
            ts,
            kind: MetricKind::NetworkBytes,
            source: "eth0".to_string(),
            value: None,
            unit: None,
            details: json!({"rx_bytes": rx, "tx_bytes": tx}),
        };
        // 60s apart, within MAX_GAP, 5-min bucket
        let metrics = vec![
            mk(0.0, 1000.0, 100.0),
            mk(60.0, 2500.0, 350.0),
            mk(120.0, 3000.0, 600.0),
        ];
        let (rx, tx) = network_bucket_series(&metrics, 5 * 60);
        assert_eq!(rx.len(), 1, "all three samples fall in one 5-min bucket");
        // Deep into dt1=1500, dt2=500 → 2000 bytes / 2^20 MiB
        let expected_rx = 2000.0 / 1_048_576.0;
        let expected_tx = 500.0 / 1_048_576.0;
        assert!((rx[0].1 - expected_rx).abs() < 1e-9);
        assert!((tx[0].1 - expected_tx).abs() < 1e-9);
    }

    #[test]
    fn network_bucket_series_skips_long_gap() {
        let mk = |ts: f64, rx: f64| MetricSample {
            ts,
            kind: MetricKind::NetworkBytes,
            source: "eth0".to_string(),
            value: None,
            unit: None,
            details: json!({"rx_bytes": rx, "tx_bytes": 0.0}),
        };
        // Gap of 1 hour > MAX_GAP_SECONDS so the delta is dropped.
        let metrics = vec![mk(0.0, 0.0), mk(3600.0, 1_000_000.0)];
        let (rx, _tx) = network_bucket_series(&metrics, 5 * 60);
        assert!(rx.is_empty(), "no bucket produced across a long gap");
    }
}
