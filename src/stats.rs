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
        
        let stats = get_stats(&serial, &app_id).await.unwrap_or(None);
        let battery = get_battery(&serial).await.unwrap_or(None);
        
        if let Some(mut s) = stats {
            s.battery = battery;
            let _ = tx.send(s);
        } else if let Some(b) = battery {
             let _ = tx.send(StatsUpdate { cpu: 0.0, mem: 0.0, battery: Some(b) });
        }
    }
}

async fn get_battery(serial: &str) -> Result<Option<u8>> {
    let output = Command::new("adb")
        .args(["-s", serial, "shell", "dumpsys", "battery"])
        .stdin(Stdio::null())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("level:") {
            if let Some(lvl_str) = line.split(':').last() {
                if let Ok(lvl) = lvl_str.trim().parse::<u8>() {
                    return Ok(Some(lvl));
                }
            }
        }
    }
    Ok(None)
}

async fn get_stats(serial: &str, app_id: &str) -> Result<Option<StatsUpdate>> {
    // Get CPU and Mem from top
    let output = Command::new("adb")
        .args(["-s", serial, "shell", "top", "-n", "1", "-b", "-q"])
        .stdin(Stdio::null())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(app_id) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // top columns: PID USER PR NI VIRT RES SHR S %CPU %MEM TIME+ ARGS
            // Usually %CPU is at index 8 and %MEM at index 9 for busybox top, 
            // but Android top might differ.
            // In Android 10+ top:
            // PID USER PR NI VIRT RES SHR S [%CPU] [%MEM] TIME+ ARGS
            if parts.len() >= 10 {
                let cpu: f64 = parts[8].parse().unwrap_or(0.0);
                let mem: f64 = parts[9].parse().unwrap_or(0.0);
                return Ok(Some(StatsUpdate { cpu, mem, battery: None }));
            }
        }
    }
    
    Ok(None)
}
