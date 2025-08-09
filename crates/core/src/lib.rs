use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Polling interval in seconds
    pub poll_interval_secs: u64,
    /// Optional battery name to monitor
    pub battery: Option<String>,
    /// Path to the sled database file
    pub database_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            battery: None,
            database_path: String::from("battery.db"),
        }
    }
}

/// Load configuration from a TOML file. If the file doesn't exist, default
/// values are returned.
pub fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<Config> {
    let path = path.unwrap_or_else(|| std::path::Path::new("config.toml"));
    if path.exists() {
        let contents = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&contents)?;
        Ok(cfg)
    } else {
        Ok(Config::default())
    }
}
