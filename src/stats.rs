use tokio::process::Command;
use std::process::Stdio;
use tokio::sync::mpsc;
use anyhow::Result;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct StatsUpdate {
    pub cpu: f64,
    pub mem: f64,
    pub battery: Option<u8>,
}

pub async fn start_stats_polling(
    serial: String,
    app_id: String,
    tx: mpsc::UnboundedSender<StatsUpdate>
) {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    loop {
        interval.tick().await;
        
        let update = get_combined_stats(&serial, &app_id).await.unwrap_or(None);
        if let Some(stats) = update {
            let _ = tx.send(stats);
        }
    }
}

async fn get_combined_stats(serial: &str, app_id: &str) -> Result<Option<StatsUpdate>> {
    // Run top and battery in a single shell command to reduce latency
    let cmd = format!("top -n 1 -b -q | grep {}; dumpsys battery | grep level", app_id);
    let output = Command::new("adb")
        .args(["-s", serial, "shell", &cmd])
        .stdin(Stdio::null())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut cpu = 0.0;
    let mut mem = 0.0;
    let mut battery = None;
    let mut found_app = false;

    for line in stdout.lines() {
        if line.contains(app_id) && !found_app {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                cpu = parts[8].parse().unwrap_or(0.0);
                mem = parts[9].parse().unwrap_or(0.0);
                found_app = true;
            }
        } else if line.contains("level:") {
            if let Some(lvl_str) = line.split(':').last() {
                battery = lvl_str.trim().parse::<u8>().ok();
            }
        }
    }
    
    if found_app || battery.is_some() {
        Ok(Some(StatsUpdate { cpu, mem, battery }))
    } else {
        Ok(None)
    }
}
