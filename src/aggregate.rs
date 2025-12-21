use std::collections::BTreeMap;

use crate::metrics::{MetricKind, MetricSample};
use serde_json::json;

fn sum_or_none(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total = 0.0;
    let mut has_value = false;
    for value in values.flatten() {
        total += value;
        has_value = true;
    }
    has_value.then_some(total)
}

fn avg_or_none(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0u64;
    for value in values.flatten() {
        total += value;
        count += 1;
    }
    if count == 0 {
        None
    } else {
        Some(total / count as f64)
    }
}

fn percent(numerator: Option<f64>, denominator: Option<f64>) -> Option<f64> {
    match (numerator, denominator) {
        (Some(num), Some(den)) if den != 0.0 => Some((num / den) * 100.0),
        _ => None,
    }
}

pub fn aggregate_battery_metrics(metrics: &[MetricSample]) -> Vec<MetricSample> {
    if metrics.is_empty() {
        return Vec::new();
    }

    let mut by_timestamp: BTreeMap<ordered_float::OrderedFloat<f64>, Vec<&MetricSample>> =
        BTreeMap::new();
    for metric in metrics {
        by_timestamp
            .entry(ordered_float::OrderedFloat(metric.ts))
            .or_default()
            .push(metric);
    }

    let mut aggregated = Vec::new();
    for (ts_key, group) in by_timestamp {
        let ts = ts_key.0;

        let energy_now: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryEnergyNow)
            .copied()
            .collect();
        let energy_full: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryEnergyFull)
            .copied()
            .collect();
        let energy_full_design: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryEnergyFullDesign)
            .copied()
            .collect();
        let percentages: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryPercentage)
            .copied()
            .collect();
        let capacity: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryCapacity)
            .copied()
            .collect();
        let health: Vec<&MetricSample> = group
            .iter()
            .filter(|m| m.kind == MetricKind::BatteryHealth)
            .copied()
            .collect();

        let sum_energy_now = sum_or_none(energy_now.iter().map(|m| m.value));
        let sum_energy_full = sum_or_none(energy_full.iter().map(|m| m.value));
        let sum_energy_full_design = sum_or_none(energy_full_design.iter().map(|m| m.value));

        let mut sources: Vec<&str> = group.iter().map(|m| m.source.as_str()).collect();
        sources.sort();
        sources.dedup();
        let combined_source = sources.join("+");

        let mut statuses = std::collections::BTreeSet::new();
        for metric in &group {
            if let Some(status) = metric.details.get("status").and_then(|v| v.as_str()) {
                statuses.insert(status);
            }
        }
        let status = if statuses.is_empty() {
            None
        } else if statuses.len() == 1 {
            Some(statuses.into_iter().next().unwrap().to_string())
        } else {
            Some("mixed".to_string())
        };

        let details = json!({ "status": status });

        let mut computed_percentage = percent(sum_energy_now, sum_energy_full);
        if computed_percentage.is_none() {
            computed_percentage = avg_or_none(percentages.iter().map(|m| m.value));
        }

        let mut computed_health = percent(sum_energy_full, sum_energy_full_design);
        if computed_health.is_none() {
            computed_health = avg_or_none(health.iter().map(|m| m.value));
        }

        let avg_capacity = avg_or_none(capacity.iter().map(|m| m.value));

        if let Some(pct) = computed_percentage {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryPercentage,
                &combined_source,
                Some(pct),
                Some("%"),
                details.clone(),
            ));
        }

        if let Some(cap) = avg_capacity {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryCapacity,
                &combined_source,
                Some(cap),
                Some("%"),
                details.clone(),
            ));
        }

        if let Some(h) = computed_health {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryHealth,
                &combined_source,
                Some(h),
                Some("%"),
                details.clone(),
            ));
        }

        if let Some(e) = sum_energy_now {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryEnergyNow,
                &combined_source,
                Some(e),
                Some("Wh"),
                details.clone(),
            ));
        }

        if let Some(e) = sum_energy_full {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryEnergyFull,
                &combined_source,
                Some(e),
                Some("Wh"),
                details.clone(),
            ));
        }

        if let Some(e) = sum_energy_full_design {
            aggregated.push(MetricSample::new(
                ts,
                MetricKind::BatteryEnergyFullDesign,
                &combined_source,
                Some(e),
                Some("Wh"),
                details.clone(),
            ));
        }
    }

    aggregated
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn battery_metric(
        ts: f64,
        kind: MetricKind,
        source: &str,
        value: f64,
        status: &str,
    ) -> MetricSample {
        MetricSample {
            ts,
            kind: kind.clone(),
            source: source.to_string(),
            value: Some(value),
            unit: match kind {
                MetricKind::BatteryPercentage
                | MetricKind::BatteryCapacity
                | MetricKind::BatteryHealth => Some("%".to_string()),
                _ => Some("Wh".to_string()),
            },
            details: json!({"status": status}),
        }
    }

    #[test]
    fn aggregate_battery_metrics_combines_multiple_batteries() {
        let metrics = vec![
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyNow,
                "BAT0",
                10.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFull,
                "BAT0",
                20.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFullDesign,
                "BAT0",
                25.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryCapacity,
                "BAT0",
                90.0,
                "Discharging",
            ),
            battery_metric(1.0, MetricKind::BatteryEnergyNow, "BAT1", 5.0, "Charging"),
            battery_metric(1.0, MetricKind::BatteryEnergyFull, "BAT1", 10.0, "Charging"),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFullDesign,
                "BAT1",
                15.0,
                "Charging",
            ),
            battery_metric(1.0, MetricKind::BatteryCapacity, "BAT1", 95.0, "Charging"),
        ];

        let aggregated = aggregate_battery_metrics(&metrics);

        let energy_now = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryEnergyNow)
            .unwrap();
        assert_eq!(energy_now.value, Some(15.0));
        assert_eq!(energy_now.source, "BAT0+BAT1");

        let energy_full = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryEnergyFull)
            .unwrap();
        assert_eq!(energy_full.value, Some(30.0));

        let energy_design = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryEnergyFullDesign)
            .unwrap();
        assert_eq!(energy_design.value, Some(40.0));

        let percentage = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryPercentage)
            .unwrap();
        assert!((percentage.value.unwrap() - 50.0).abs() < 1e-6);

        let health = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryHealth)
            .unwrap();
        assert!((health.value.unwrap() - 75.0).abs() < 1e-6);

        let capacity = aggregated
            .iter()
            .find(|m| m.kind == MetricKind::BatteryCapacity)
            .unwrap();
        assert_eq!(capacity.value, Some(92.5));

        let status = percentage.details.get("status").unwrap().as_str();
        assert_eq!(status, Some("mixed"));
    }

    #[test]
    fn aggregate_battery_metrics_groups_by_timestamp() {
        let metrics = vec![
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyNow,
                "BAT0",
                1.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFull,
                "BAT0",
                2.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFullDesign,
                "BAT0",
                3.0,
                "Discharging",
            ),
            battery_metric(
                2.0,
                MetricKind::BatteryEnergyNow,
                "BAT0",
                2.0,
                "Discharging",
            ),
            battery_metric(
                2.0,
                MetricKind::BatteryEnergyFull,
                "BAT0",
                4.0,
                "Discharging",
            ),
            battery_metric(
                2.0,
                MetricKind::BatteryEnergyFullDesign,
                "BAT0",
                6.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyNow,
                "BAT1",
                0.5,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFull,
                "BAT1",
                1.0,
                "Discharging",
            ),
            battery_metric(
                1.0,
                MetricKind::BatteryEnergyFullDesign,
                "BAT1",
                1.5,
                "Discharging",
            ),
        ];

        let aggregated = aggregate_battery_metrics(&metrics);

        let ts1_metrics: Vec<_> = aggregated.iter().filter(|m| m.ts == 1.0).collect();
        let ts2_metrics: Vec<_> = aggregated.iter().filter(|m| m.ts == 2.0).collect();

        assert!(ts1_metrics.len() >= 3);
        assert!(ts2_metrics.len() >= 3);

        let ts1_energy_now = ts1_metrics
            .iter()
            .find(|m| m.kind == MetricKind::BatteryEnergyNow)
            .unwrap();
        assert_eq!(ts1_energy_now.value, Some(1.5));

        let ts2_energy_now = ts2_metrics
            .iter()
            .find(|m| m.kind == MetricKind::BatteryEnergyNow)
            .unwrap();
        assert_eq!(ts2_energy_now.value, Some(2.0));
    }
}
