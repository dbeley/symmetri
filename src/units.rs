//! Unit conversion helpers for consistent formatting across the application

/// Format bytes to human-readable format (B, KiB, MiB, GiB, TiB)
pub fn format_bytes(value: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = value;
    let mut unit = "B";
    for next in &UNITS {
        unit = next;
        if val.abs() < 1024.0 || *next == "TiB" {
            break;
        }
        val /= 1024.0;
    }
    if unit == "B" {
        format!("{val:.0}{unit}")
    } else {
        format!("{val:.1}{unit}")
    }
}

/// Format bytes with optional value
pub fn format_opt_bytes(value: Option<f64>) -> String {
    value.map(format_bytes).unwrap_or_else(|| "--".to_string())
}

/// Format power value in watts
pub fn format_power(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2}W"),
        None => "--".to_string(),
    }
}

/// Format percentage value
pub fn format_percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1}%"))
        .unwrap_or_else(|| "--".to_string())
}

/// Format frequency in MHz
pub fn format_freq(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.0}MHz"))
        .unwrap_or_else(|| "--".to_string())
}

/// Format network rate (bytes per second)
pub fn format_rate(value: Option<f64>) -> String {
    value
        .map(|v| format!("{}/s", format_bytes(v)))
        .unwrap_or_else(|| "--".to_string())
}

/// Format temperature in Celsius
pub fn format_temp(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1}°C"))
        .unwrap_or_else(|| "--".to_string())
}

/// Convert microseconds to seconds
pub fn usecs_to_secs(usecs: f64) -> f64 {
    usecs / 1_000_000.0
}

/// Convert milliseconds to seconds
pub fn msecs_to_secs(msecs: f64) -> f64 {
    msecs / 1000.0
}

/// Convert seconds to hours
pub fn secs_to_hours(secs: f64) -> f64 {
    secs / 3600.0
}

/// Convert watts to kilowatts
pub fn watts_to_kilowatts(watts: f64) -> f64 {
    watts / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_handles_various_sizes() {
        assert_eq!(format_bytes(512.0), "512B");
        assert_eq!(format_bytes(1024.0), "1.0KiB");
        assert_eq!(format_bytes(1536.0), "1.5KiB");
        assert_eq!(format_bytes(1_048_576.0), "1.0MiB");
        assert_eq!(format_bytes(1_073_741_824.0), "1.0GiB");
    }

    #[test]
    fn format_power_displays_watts() {
        assert_eq!(format_power(Some(15.5)), "15.50W");
        assert_eq!(format_power(None), "--");
    }

    #[test]
    fn format_percent_displays_correctly() {
        assert_eq!(format_percent(Some(75.5)), "75.5%");
        assert_eq!(format_percent(None), "--");
    }

    #[test]
    fn format_freq_displays_mhz() {
        assert_eq!(format_freq(Some(2400.0)), "2400MHz");
        assert_eq!(format_freq(None), "--");
    }

    #[test]
    fn format_temp_displays_celsius() {
        assert_eq!(format_temp(Some(45.5)), "45.5°C");
        assert_eq!(format_temp(None), "--");
    }

    #[test]
    fn time_conversions_are_correct() {
        assert!((usecs_to_secs(1_000_000.0) - 1.0).abs() < 1e-6);
        assert!((msecs_to_secs(1000.0) - 1.0).abs() < 1e-6);
        assert!((secs_to_hours(3600.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn power_conversions_are_correct() {
        assert!((watts_to_kilowatts(1000.0) - 1.0).abs() < 1e-6);
    }
}
