use anyhow::Result;
use tokio::process::Command;
use std::process::Stdio;
use crate::config::Config;
use crate::state::AppState;
use chrono;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use std::time::{Instant, Duration};

#[derive(Debug, Clone)]
pub enum BuildEvent {
    Task(String),
    Complete(Duration),
    Failed,
}

pub async fn get_devices() -> Result<Vec<String>> {
    let output = Command::new("adb")
        .arg("devices")
        .stdin(Stdio::null())
        .output()
        .await?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let devices: Vec<String> = stdout.lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == "device" {
                Some(parts[0].to_string())
            } else {
                None
            }
        })
        .collect();
    
    Ok(devices)
}

pub async fn build_and_launch(
    config: &Config, 
    state: &AppState, 
    tx_log: mpsc::UnboundedSender<String>,
    tx_build: mpsc::UnboundedSender<BuildEvent>
) -> Result<()> {
    let start_time = Instant::now();
    let _ = tx_log.send("[build] starting...".to_string());
    
    let mut child = Command::new("./gradlew")
        .args([
            ":androidApp:installDebug",
            "--parallel",
            "--configuration-cache",
            "--daemon",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    
    let tx_out = tx_log.clone();
    let tx_err = tx_log.clone();
    let tx_b = tx_build.clone();
    
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if line.starts_with("> Task ") {
                let _ = tx_b.send(BuildEvent::Task(line.clone()));
            }
            let _ = tx_out.send(format!("[build] {}", line));
        }
    });

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = tx_err.send(format!("[build-err] {}", line));
        }
    });

    let status = child.wait().await?;

    if status.success() {
        let duration = start_time.elapsed();
        let _ = tx_build.send(BuildEvent::Complete(duration));
        let _ = tx_log.send(format!("[ok] build successful in {:.2}s", duration.as_secs_f32()));
        
        if state.auto_open {
            if let Some(ref serial) = state.device_serial {
                Command::new("adb")
                    .args(["-s", serial, "shell", "am", "force-stop", &config.app_id])
                    .stdin(Stdio::null())
                    .status()
                    .await?;
                
                Command::new("adb")
                    .args(["-s", serial, "shell", "am", "start", "-n", &config.activity])
                    .stdin(Stdio::null())
                    .status()
                    .await?;
                
                let _ = tx_log.send(format!("[ok] launched on {}", serial));
            }
        }
    } else {
        let _ = tx_build.send(BuildEvent::Failed);
        let _ = tx_log.send("[err] build failed".to_string());
    }
    
    Ok(())
}

pub async fn take_screenshot(state: &AppState, tx_log: mpsc::UnboundedSender<String>) -> Result<()> {
    if let Some(ref serial) = state.device_serial {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let filename = format!("screenshot_{}.png", timestamp);
        let remote_path = format!("/sdcard/{}", filename);
        
        Command::new("adb")
            .args(["-s", serial, "shell", "screencap", "-p", &remote_path])
            .stdin(Stdio::null())
            .status()
            .await?;
        
        Command::new("adb")
            .args(["-s", serial, "pull", &remote_path, "."])
            .stdin(Stdio::null())
            .status()
            .await?;
        
        Command::new("adb")
            .args(["-s", serial, "shell", "rm", &remote_path])
            .stdin(Stdio::null())
            .status()
            .await?;
            
        let _ = tx_log.send(format!("[ok] screenshot saved as {}", filename));
    }
    Ok(())
}

pub async fn clear_app_data(
    config: &Config, 
    state: &AppState, 
    tx_log: mpsc::UnboundedSender<String>
) -> Result<()> {
    if let Some(ref serial) = state.device_serial {
        let _ = tx_log.send(format!("[info] clearing app data for {}...", config.app_id));
        Command::new("adb")
            .args(["-s", serial, "shell", "pm", "clear", &config.app_id])
            .stdin(Stdio::null())
            .status()
            .await?;
        
        let _ = tx_log.send("[ok] data cleared".to_string());
        if state.auto_open {
            Command::new("adb")
                .args(["-s", serial, "shell", "am", "start", "-n", &config.activity])
                .stdin(Stdio::null())
                .status()
                .await?;
        }
    }
    Ok(())
}

pub async fn toggle_layout_bounds(state: &mut AppState) -> Result<()> {
    if let Some(ref serial) = state.device_serial {
        state.show_layout_bounds = !state.show_layout_bounds;
        let val = if state.show_layout_bounds { "true" } else { "false" };
        
        Command::new("adb")
            .args(["-s", serial, "shell", "setprop", "debug.layout", val])
            .status()
            .await?;
            
        Command::new("adb")
            .args(["-s", serial, "shell", "service", "call", "activity", "1599295570"])
            .status()
            .await?;
    }
    Ok(())
}

pub struct Recorder {
    child: Option<tokio::process::Child>,
    filename: String,
}

impl Recorder {
    pub fn new() -> Self {
        Self { child: None, filename: String::new() }
    }

    pub async fn start(&mut self, serial: &str) -> Result<()> {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        self.filename = format!("record_{}.mp4", timestamp);
        let remote_path = format!("/sdcard/{}", self.filename);
        
        let child = Command::new("adb")
            .args(["-s", serial, "shell", "screenrecord", &remote_path])
            .stdin(Stdio::null())
            .spawn()?;
            
        self.child = Some(child);
        Ok(())
    }

    pub async fn stop(&mut self, serial: &str, tx_log: mpsc::UnboundedSender<String>) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
            
            let _ = tx_log.send(format!("[info] pulling recording {}...", self.filename));
            let remote_path = format!("/sdcard/{}", self.filename);
            
            tokio::time::sleep(Duration::from_secs(1)).await; // Wait for file to finalize

            Command::new("adb")
                .args(["-s", serial, "pull", &remote_path, "."])
                .status()
                .await?;
                
            Command::new("adb")
                .args(["-s", serial, "shell", "rm", &remote_path])
                .status()
                .await?;
                
            let _ = tx_log.send(format!("[ok] recording saved as {}", self.filename));
        }
        Ok(())
    }
}
