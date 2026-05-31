use anyhow::Result;
use tokio::process::Command;
use std::process::Stdio;
use crate::config::Config;
use chrono;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use std::time::{Instant, Duration};
use std::path::Path;
use futures::future::join_all;

#[derive(Debug, Clone)]
pub enum BuildEvent {
    Task(String),
    Complete(Duration),
    Failed,
}

pub async fn get_device_serials() -> Result<Vec<String>> {
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

pub async fn get_avds() -> Result<Vec<String>> {
    let output = Command::new("emulator")
        .arg("-list-avds")
        .stdin(Stdio::null())
        .output()
        .await?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let avds: Vec<String> = stdout.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    
    Ok(avds)
}

pub async fn get_devices() -> Vec<(String, String)> {
    let output = Command::new("adb").args(["devices", "-l"]).stdin(Stdio::null()).output().await;
    if let Ok(out) = output {
        let list = String::from_utf8_lossy(&out.stdout);
        list.lines()
            .skip(1)
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == "device" {
                    let serial = parts[0].to_string();
                    let model = parts.iter()
                        .find(|p| p.starts_with("model:"))
                        .and_then(|p| p.strip_prefix("model:"))
                        .unwrap_or("Unknown")
                        .to_string();
                    Some((serial, model))
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

pub async fn launch_emulator(avd_name: &str) -> Result<()> {
    Command::new("emulator")
        .arg("-avd")
        .arg(avd_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub async fn build_and_launch(
    config: &Config, 
    serials: Vec<String>,
    auto_open: bool,
    force_rebuild: bool,
    tx_log: mpsc::UnboundedSender<String>,
    tx_build: mpsc::UnboundedSender<BuildEvent>
) -> Result<()> {
    let start_time = Instant::now();
    let _ = tx_log.send(if force_rebuild { "[build] starting (force clean)..." } else { "[build] starting..." }.to_string());
    
    let project_path = Path::new(&config.project_path);
    if !project_path.exists() {
        let _ = tx_log.send(format!("[err] project path not found: {}", config.project_path));
        let _ = tx_build.send(BuildEvent::Failed);
        return Ok(());
    }

    // Check gradlew exists
    let gradlew = project_path.join("gradlew");
    if !gradlew.exists() {
        let _ = tx_log.send(format!("[err] gradlew not found in: {}", config.project_path));
        let _ = tx_build.send(BuildEvent::Failed);
        return Ok(());
    }

    let mut args: Vec<String> = vec![
        format!(":androidApp:install{}", config.build_variant.chars().next().unwrap().to_uppercase().to_string() + &config.build_variant[1..]),
        "--parallel".to_string(),
        "--console=plain".to_string()
    ];
    if force_rebuild {
        args.insert(0, "clean".to_string());
        args.push("--no-build-cache".to_string());
    }

    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let mut child = match Command::new("./gradlew")
        .args(&args_str)
        .current_dir(project_path)
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx_log.send(format!("[err] failed to start gradlew: {}", e));
            let _ = tx_build.send(BuildEvent::Failed);
            return Ok(());
        }
    };

    let stdout = child.stdout.take().expect("stdout not captured");
    let stderr = child.stderr.take().expect("stderr not captured");
    
    let tx_out = tx_log.clone();
    let tx_err = tx_log.clone();
    let tx_b = tx_build.clone();
    
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if line.starts_with("> Task ") {
                let _ = tx_b.send(BuildEvent::Task(line.trim_start_matches("> Task ").to_string()));
            }
            let _ = tx_out.send(format!("[build] {}", line));
        }
    });

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                let _ = tx_err.send(format!("[build-err] {}", line));
            }
        }
    });

    // 10 minute timeout — prevents infinite "building..." state
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        child.wait()
    ).await;

    match status {
        Err(_) => {
            let _ = child.start_kill();
            let _ = tx_log.send("[err] build timed out after 10 minutes".to_string());
            let _ = tx_build.send(BuildEvent::Failed);
            return Ok(());
        }
        Ok(Err(e)) => {
            let _ = tx_log.send(format!("[err] build process error: {}", e));
            let _ = tx_build.send(BuildEvent::Failed);
            return Ok(());
        }
        Ok(Ok(exit_status)) => {
            if exit_status.success() {
                let duration = start_time.elapsed();
                let _ = tx_build.send(BuildEvent::Complete(duration));
                let _ = tx_log.send(format!("[ok] build successful in {:.2}s", duration.as_secs_f32()));
                if auto_open && !serials.is_empty() {
                    let mut tasks = Vec::new();
                    for serial in serials {
                        let s = serial.clone();
                        let cfg = config.clone();
                        let log_tx = tx_log.clone();
                        tasks.push(tokio::spawn(async move {
                            let _ = Command::new("adb").args(["-s", &s, "shell", "am", "force-stop", &cfg.app_id]).stdin(Stdio::null()).status().await;
                            let _ = Command::new("adb").args(["-s", &s, "shell", "am", "start", "-n", &cfg.activity]).stdin(Stdio::null()).status().await;
                            let _ = log_tx.send(format!("[ok] launched on {}", s));
                        }));
                    }
                    join_all(tasks).await;
                }
            } else {
                let _ = tx_build.send(BuildEvent::Failed);
                let _ = tx_log.send("[err] build FAILED — check [3] Build tab for details".to_string());
            }
        }
    }

    Ok(())
}

pub async fn launch_app(config: &Config, serials: Vec<String>, tx_log: mpsc::UnboundedSender<String>) -> Result<()> {
    let mut tasks = Vec::new();
    for serial in serials {
        let s = serial.clone();
        let cfg = config.clone();
        let log_tx = tx_log.clone();
        tasks.push(tokio::spawn(async move {
            let _ = log_tx.send(format!("[info] launching {} on {}...", cfg.app_id, s));
            let _ = Command::new("adb")
                .args(["-s", &s, "shell", "am", "force-stop", &cfg.app_id])
                .stdin(Stdio::null())
                .status()
                .await;
            
            let _ = Command::new("adb")
                .args(["-s", &s, "shell", "am", "start", "-n", &cfg.activity])
                .stdin(Stdio::null())
                .status()
                .await;
            
            let _ = log_tx.send(format!("[ok] launched on {}", s));
        }));
    }
    join_all(tasks).await;
    Ok(())
}

pub async fn take_screenshot(config: &Config, serials: Vec<String>, tx_log: mpsc::UnboundedSender<String>) -> Result<()> {
    let mut tasks = Vec::new();
    for serial in serials {
        let s = serial.clone();
        let cfg = config.clone();
        let log_tx = tx_log.clone();
        tasks.push(tokio::spawn(async move {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            let filename = format!("screenshot_{}_{}.png", s.replace(":", "_"), timestamp);
            let remote_path = format!("/sdcard/{}", filename);
            
            let _ = Command::new("adb")
                .args(["-s", &s, "shell", "screencap", "-p", &remote_path])
                .stdin(Stdio::null())
                .status()
                .await;
            
            let local_path = Path::new(&cfg.output_path).join(&filename);
            if let Some(path_str) = local_path.to_str() {
                let _ = Command::new("adb")
                    .args(["-s", &s, "pull", &remote_path, path_str])
                    .stdin(Stdio::null())
                    .status()
                    .await;
            }
            
            let _ = Command::new("adb")
                .args(["-s", &s, "shell", "rm", &remote_path])
                .stdin(Stdio::null())
                .status()
                .await;
                
            let _ = log_tx.send(format!("[ok] screenshot saved: {}/{}", cfg.output_path, filename));
        }));
    }
    join_all(tasks).await;
    Ok(())
}

pub async fn clear_app_data(
    config: &Config, 
    serials: Vec<String>,
    auto_open: bool,
    tx_log: mpsc::UnboundedSender<String>
) -> Result<()> {
    let mut tasks = Vec::new();
    for serial in serials {
        let s = serial.clone();
        let cfg = config.clone();
        let log_tx = tx_log.clone();
        tasks.push(tokio::spawn(async move {
            let _ = log_tx.send(format!("[info] clearing data for {} on {}...", cfg.app_id, s));
            let _ = Command::new("adb")
                .args(["-s", &s, "shell", "pm", "clear", &cfg.app_id])
                .stdin(Stdio::null())
                .status()
                .await;
            
            let _ = log_tx.send(format!("[ok] data cleared on {}", s));
            if auto_open {
                let _ = Command::new("adb")
                    .args(["-s", &s, "shell", "am", "start", "-n", &cfg.activity])
                    .stdin(Stdio::null())
                    .status()
                    .await;
            }
        }));
    }
    join_all(tasks).await;
    Ok(())
}

pub async fn toggle_layout_bounds(serials: Vec<String>, show: bool) -> Result<()> {
    let mut tasks = Vec::new();
    for serial in serials {
        let s = serial.clone();
        tasks.push(tokio::spawn(async move {
            let val = if show { "true" } else { "false" };
            let _ = Command::new("adb").args(["-s", &s, "shell", "setprop", "debug.layout", val]).status().await;
            let _ = Command::new("adb").args(["-s", &s, "shell", "service", "call", "activity", "1599295570"]).status().await;
        }));
    }
    join_all(tasks).await;
    Ok(())
}

pub struct Recorder {
    children: Vec<(String, tokio::process::Child, String)>,
}

impl Recorder {
    pub fn new() -> Self {
        Self { children: Vec::new() }
    }

    pub async fn start(&mut self, serials: Vec<String>) -> Result<()> {
        for serial in serials {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            let filename = format!("record_{}_{}.mp4", serial.replace(":", "_"), timestamp);
            let remote_path = format!("/sdcard/{}", filename);
            
            let child = Command::new("adb")
                .args(["-s", &serial, "shell", "screenrecord", &remote_path])
                .stdin(Stdio::null())
                .spawn()?;
                
            self.children.push((serial, child, filename));
        }
        Ok(())
    }

    pub async fn stop(&mut self, config: &Config, tx_log: mpsc::UnboundedSender<String>) -> Result<()> {
        let mut tasks = Vec::new();
        let children = std::mem::take(&mut self.children);
        
        for (serial, mut child, filename) in children {
            let cfg = config.clone();
            let log_tx = tx_log.clone();
            tasks.push(tokio::spawn(async move {
                let _ = child.start_kill();
                let _ = child.wait().await;
                
                let remote_path = format!("/sdcard/{}", filename);
                tokio::time::sleep(Duration::from_secs(1)).await;

                let local_path = Path::new(&cfg.output_path).join(&filename);
                if let Some(path_str) = local_path.to_str() {
                    let _ = Command::new("adb").args(["-s", &serial, "pull", &remote_path, path_str]).status().await;
                }
                let _ = Command::new("adb").args(["-s", &serial, "shell", "rm", &remote_path]).status().await;
                
                let _ = log_tx.send(format!("[ok] recording saved: {}/{}", cfg.output_path, filename));
            }));
        }
        join_all(tasks).await;
        Ok(())
    }
}
