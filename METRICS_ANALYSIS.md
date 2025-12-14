# Symmetri Metrics Analysis

## Executive Summary

This document provides a comprehensive analysis of the Symmetri system metrics implementation, with emphasis on power consumption metrics, time bucketing, and computational accuracy.

## Overall Architecture

**Strengths:**
- Clean separation between collection (`metrics.rs`), storage (`db.rs`), aggregation (`aggregate.rs`), and reporting (`cli.rs`)
- SQLite-based persistent storage with appropriate indexes
- Flexible timeframe system supporting hours, days, and months

## Power Consumption Metrics - Critical Analysis

### 1. Battery Power Calculation (cli_helpers.rs)

**Current Implementation:**
```rust
// Lines 101-133 in cli_helpers.rs
pub fn average_rates<'a>(samples: impl IntoIterator<Item = &'a Sample>) -> AverageRates {
    const MAX_GAP_HOURS: f64 = 5.0 / 60.0;  // 5 minutes
    
    for current in iter {
        let dt_hours = (current.ts - previous.ts) / 3600.0;
        if dt_hours > 0.0 && dt_hours <= MAX_GAP_HOURS {
            let delta = current.energy_now_wh.unwrap() - previous.energy_now_wh.unwrap();
            if delta > 0.0 && is_charging(previous) && is_charging(current) {
                charge.record(delta, dt_hours);
            } else if delta < 0.0 && is_discharging(previous) && is_discharging(current) {
                discharge.record(-delta, dt_hours);
            }
        }
    }
}
```

**✅ CORRECT:** This implementation properly:
- Computes power (W) = energy_delta (Wh) / time_delta (h)
- Filters out gaps > 5 minutes to avoid skewed averages from missing samples
- Respects charge/discharge state to avoid mixing incompatible data
- Accumulates total energy and time, then divides for weighted average

**Issue #1: No handling of data quality edge cases**
- **Problem:** If energy readings have noise or temporary spikes, they can skew the average
- **Impact:** Minor - battery meters are generally stable
- **Severity:** LOW

**Issue #2: MAX_GAP_HOURS hardcoded**
- **Problem:** 5-minute gap is reasonable for default 5-minute collection, but won't adapt if collection interval changes
- **Impact:** Could incorrectly exclude valid data or include stale data
- **Severity:** MEDIUM
- **Recommendation:** Make gap threshold configurable or derive from actual collection interval

### 2. Hardware Monitor Power Draw (metrics.rs)

**Current Implementation:**
```rust
// Lines 455-497 in metrics.rs
fn power_samples(ts: f64) -> Vec<MetricSample> {
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
        let watts = raw_value / 1_000_000.0;  // Convert from microwatts
```

**✅ CORRECT:** Proper conversion from µW to W

**Issue #3: Point-in-time sampling**
- **Problem:** hwmon power readings are instantaneous snapshots at collection time
- **Implication:** Power can vary significantly between samples (e.g., CPU burst)
- **Current handling:** Samples are collected once per interval and averaged in reports
- **Severity:** MEDIUM
- **Assessment:** This is standard practice for system monitoring tools. The 5-minute collection interval helps average out transient spikes.

**Issue #4: No validation of power values**
- **Problem:** No bounds checking on power readings (e.g., > 300W might be invalid for laptop)
- **Impact:** Could store obviously incorrect values
- **Severity:** LOW
- **Recommendation:** Add optional sanity bounds based on device type

### 3. Power Draw Aggregation in Reports (cli.rs)

**Current Implementation:**
```rust
// Lines 283-290 in cli.rs
let power_draw_stats = average_for_kind(metrics, MetricKind::PowerDraw);
let avg_discharge_w = power_draw_stats.average().or(battery_rates.discharge_w);

// Lines 860-863 in cli.rs
let discharge_power = discharge_rates
    .get(&bucket_start)
    .and_then(NumberStats::average)
    .or_else(|| power_draw.get(&bucket_start).and_then(NumberStats::average))
    .or(rates.discharge_w);
```

**✅ EXCELLENT:** Multi-source power calculation with proper fallback:
1. First tries hwmon power_draw average
2. Falls back to battery energy delta rates
3. This provides redundancy and better coverage

**Issue #5: Mixing power sources**
- **Problem:** hwmon power draw may measure CPU/GPU only, while battery measures total system power
- **Impact:** Comparing or combining these could be misleading
- **Assessment:** Current code doesn't combine, just uses as alternatives - this is CORRECT
- **Severity:** N/A - not an issue

## Time Bucketing Strategy

### 1. Bucket Size Selection (cli_helpers.rs)

**Current Implementation:**
```rust
// Lines 46-62 in cli_helpers.rs
pub fn bucket_span_seconds(timeframe: &Timeframe, data_span_seconds: Option<f64>) -> i64 {
    let window = timeframe.seconds.or(data_span_seconds).unwrap_or(7.0 * 24.0 * 3600.0);
    
    match window {
        w if w <= 2.0 * 3600.0 => 5 * 60,          // ≤2h: 5min buckets
        w if w <= 6.0 * 3600.0 => 10 * 60,         // ≤6h: 10min buckets
        w if w <= 24.0 * 3600.0 => 3600,           // ≤1d: 1h buckets
        w if w <= 3.0 * 24.0 * 3600.0 => 2 * 3600, // ≤3d: 2h buckets
        w if w <= 7.0 * 24.0 * 3600.0 => 6 * 3600, // ≤7d: 6h buckets
        w if w <= 30.0 * 24.0 * 3600.0 => 24 * 3600, // ≤30d: 1d buckets
        w if w <= 90.0 * 24.0 * 3600.0 => 3 * 24 * 3600, // ≤90d: 3d buckets
        _ => 7 * 24 * 3600,                        // >90d: 7d buckets
    }
}
```

**✅ EXCELLENT:** Well-designed adaptive bucketing
- Smaller buckets for shorter timeframes maintain detail
- Larger buckets for longer timeframes keep reports manageable
- Reasonable balance between resolution and readability

**Issue #6: No explicit documentation of tradeoffs**
- **Problem:** Users don't know what resolution they get
- **Impact:** Could be surprising when detail is lost
- **Severity:** LOW
- **Recommendation:** Document in README or CLI help

### 2. Bucket Alignment (cli_helpers.rs)

**Current Implementation:**
```rust
// Lines 64-72 in cli_helpers.rs
pub fn bucket_start(ts: f64, bucket_seconds: i64) -> DateTime<Local> {
    let local_dt = Local.timestamp_opt(ts as i64, 0).unwrap();
    let offset_seconds = -local_dt.offset().utc_minus_local();
    let bucket_epoch = (((ts + offset_seconds as f64) / bucket_seconds as f64).floor()
        * bucket_seconds as f64)
        - offset_seconds as f64;
    let aligned = bucket_epoch.max(0.0) as i64;
    Local.timestamp_opt(aligned, 0).unwrap()
}
```

**✅ CORRECT:** Timezone-aware bucket alignment
- Properly handles local timezone offsets
- Floor division ensures consistent bucket boundaries
- Buckets align to natural boundaries (e.g., hour starts at XX:00:00)

**Potential Issue #7: DST transitions**
- **Problem:** During daylight saving time transitions, hour buckets could be ambiguous
- **Impact:** One-hour windows could appear duplicated or missing
- **Assessment:** chrono handles this correctly with `timestamp_opt`, which returns proper disambiguation
- **Severity:** N/A - handled correctly

## Metric Quality and Aggregation

### 1. Battery Rate Buckets (cli.rs)

**Current Implementation:**
```rust
// Lines 776-824 in cli.rs
fn battery_rate_buckets(samples: &[Sample], bucket_seconds: i64) -> (...) {
    const MAX_GAP_HOURS: f64 = 5.0 / 60.0;
    
    for current in iter {
        let dt_hours = (current.ts - previous.ts) / 3600.0;
        if dt_hours <= 0.0 || dt_hours > MAX_GAP_HOURS {
            previous = current;
            continue;
        }
        let bucket = bucket_start(current.ts, bucket_seconds);
        if curr_now > prev_now && is_charging(previous) && is_charging(current) {
            charge.entry(bucket).or_default().record((curr_now - prev_now) / dt_hours);
        }
    }
}
```

**✅ CORRECT:** Per-bucket power calculations
- Computes instantaneous rate between consecutive samples
- Assigns rate to bucket based on the CURRENT timestamp (correct choice)
- Filters by charging state to prevent mixing

**Issue #8: Duplicate MAX_GAP_HOURS constant**
- **Problem:** Same constant defined in two places (cli_helpers.rs:102 and cli.rs:783)
- **Impact:** Could diverge if changed in only one place
- **Severity:** LOW
- **Recommendation:** Define once in a common location

### 2. Power Draw Bucketing (cli.rs)

**Current Implementation:**
```rust
// Lines 597-610 in cli.rs
fn bucket_stats_for_kind(...) -> BTreeMap<DateTime<Local>, NumberStats> {
    for sample in metrics.iter().filter(|s| s.kind == kind) {
        if let Some(value) = sample.value {
            let bucket = bucket_start(sample.ts, bucket_seconds);
            buckets.entry(bucket).or_default().record(value);
        }
    }
}

impl NumberStats {
    fn average(&self) -> Option<f64> {
        (self.count > 0).then_some(self.total / self.count as f64)
    }
}
```

**✅ CORRECT:** Simple arithmetic mean of samples in bucket
- For instantaneous power readings, arithmetic mean is appropriate
- Min/max also tracked for variance analysis

**Issue #9: No weighted averaging**
- **Problem:** If samples are unevenly spaced within bucket, all treated equally
- **Example:** 5 samples in first minute, 1 sample in last minute of 10-min bucket
- **Impact:** Could bias toward periods with more samples
- **Assessment:** Given regular 5-minute collection interval, this is unlikely to be an issue
- **Severity:** LOW

### 3. Network Counter Handling (cli.rs)

**Current Implementation:**
```rust
// Lines 692-697 in cli.rs
fn compute_counter_delta(prev: Option<f64>, next: Option<f64>) -> f64 {
    match (prev, next) {
        (Some(prev_val), Some(next_val)) if next_val >= prev_val => next_val - prev_val,
        _ => 0.0,
    }
}
```

**✅ CORRECT:** Handles counter resets/rollovers
- Returns 0 if counter wraps (next < prev)
- This is standard practice for monotonic counters

**Issue #10: Counter wraps at 2^64 not detected**
- **Problem:** If network counter wraps from MAX to 0, delta is lost
- **Impact:** Very rare (would take years at GB/s speeds)
- **Severity:** NEGLIGIBLE
- **Recommendation:** Could detect and handle wraps, but not critical

## Data Collection Process

### 1. Collection Frequency (systemd/symmetri.timer)

**Default: 5 minutes**
- Reasonable for battery and system metrics
- Nyquist theorem: can capture changes with 10-minute period

**Issue #11: No adaptive sampling**
- **Problem:** Same 5-minute interval regardless of system state
- **Opportunity:** Could sample more frequently during active use, less when idle
- **Impact:** Could improve detail during interesting periods
- **Severity:** ENHANCEMENT
- **Recommendation:** Consider adaptive sampling in future version

### 2. Timestamp Assignment (collector.rs)

**Current Implementation:**
```rust
// Lines 45-48 in collector.rs
let ts = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs_f64();
```

**✅ CORRECT:** Single timestamp for entire collection
- All battery + metric samples get same timestamp
- Enables proper aggregation by timestamp
- Sub-second precision preserved

**Issue #12: Collection duration not accounted for**
- **Problem:** Collection takes ~100ms for CPU (plus filesystem reads)
- **Impact:** All samples timestamped at start, but CPU sample actually measured over 100ms interval
- **Assessment:** For 5-minute intervals, 100ms error is negligible (0.03%)
- **Severity:** NEGLIGIBLE

## Edge Cases and Error Handling

### ✅ Well Handled:
1. Missing battery (warning logged, continues with other metrics)
2. Failed file reads (gracefully skipped)
3. No samples in timeframe (clear error message)
4. Multiple batteries (properly aggregated)
5. Timezone changes (handled by chrono)

### Potential Issues:

**Issue #13: No handling of clock jumps**
- **Problem:** If system clock adjusted backward, could create negative time deltas
- **Current:** Code checks `dt_hours > 0.0` but doesn't distinguish clock jump from reordering
- **Impact:** Could skip valid data after clock correction
- **Severity:** LOW
- **Recommendation:** Add monotonic timestamp in addition to wall-clock time

**Issue #14: No data retention policy**
- **Problem:** Database grows indefinitely
- **Impact:** Could fill disk over years of operation
- **Severity:** MEDIUM
- **Recommendation:** Add optional data retention policy (e.g., `--keep-days 90`)

## Comparison with Industry Standards

### Similar Tools:
- **upower**: Polls every 30s by default (symmetri's 5min is more conservative)
- **powerstat**: Samples at 10s intervals (much finer)
- **powertop**: Real-time monitoring (different use case)

**Assessment:** Symmetri's approach is appropriate for long-term trending rather than real-time monitoring.

## Summary of Issues

| Issue | Severity | Component | Impact |
|-------|----------|-----------|--------|
| #1 | LOW | Battery rates | Energy reading noise could skew averages |
| #2 | MEDIUM | Battery rates | Hardcoded gap threshold not adaptive |
| #3 | MEDIUM | Power sampling | Point-in-time sampling misses transients |
| #4 | LOW | Power validation | No bounds checking on values |
| #5 | N/A | Power aggregation | Not an issue - handled correctly |
| #6 | LOW | Documentation | Bucket resolution not documented |
| #7 | N/A | Bucket alignment | DST handled correctly |
| #8 | LOW | Code quality | Duplicate constant definition |
| #9 | LOW | Averaging | No weighted averaging (minor impact) |
| #10 | NEGLIGIBLE | Network counters | Counter wrap not detected |
| #11 | ENHANCEMENT | Collection | No adaptive sampling |
| #12 | NEGLIGIBLE | Timestamps | Collection duration not accounted |
| #13 | LOW | Time handling | Clock jumps not distinguished |
| #14 | MEDIUM | Database | No data retention policy |

## Recommendations

### High Priority:
1. **Make gap threshold adaptive** (Issue #2)
2. **Add data retention policy** (Issue #14)
3. **Document bucket resolution behavior** (Issue #6)

### Medium Priority:
4. **Remove duplicate MAX_GAP_HOURS constant** (Issue #8)
5. **Add optional sanity bounds for power values** (Issue #4)

### Low Priority:
6. **Consider monotonic timestamps** (Issue #13)
7. **Improve transient power capture** (Issue #3) - may require architectural changes

## Conclusion

**Overall Assessment: EXCELLENT**

The Symmetri implementation demonstrates solid engineering:
- ✅ Correct power calculations (W = ΔWh / Δh)
- ✅ Appropriate time bucketing with adaptive resolution
- ✅ Proper timezone and DST handling
- ✅ Robust error handling and graceful degradation
- ✅ Clear separation of concerns

The identified issues are mostly minor and don't affect the core correctness of the metrics. The most significant recommendations are:
1. Adaptive gap threshold (correctness)
2. Data retention policy (operational)
3. Better documentation (usability)

The implementation is production-ready and provides accurate system metrics for the intended use case of long-term trending and reporting.
