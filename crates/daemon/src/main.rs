use battery::Manager;
use battery_monitor_core::load_config;
use chrono::Utc;
use tokio::{
    signal,
    time::{interval, Duration},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config(None)?;
    let db = sled::open(&config.database_path)?;
    let manager = Manager::new()?;

    let mut batteries = manager.batteries()?;
    let mut battery = batteries
        .next()
        .ok_or_else(|| anyhow::anyhow!("no batteries found"))??;

    let mut tick = interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                battery.refresh()?;
                let capacity = battery.state_of_charge().value * 100.0;
                let timestamp = Utc::now().timestamp().to_be_bytes();
                db.insert(timestamp, capacity.to_be_bytes().to_vec())?;
                db.flush()?;
            }
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    Ok(())
}
