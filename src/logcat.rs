use tokio::process::{Command, Child};
use std::process::Stdio;
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

pub struct LogcatManager {
    child: Option<Child>,
}

impl LogcatManager {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub async fn start(
        &mut self, 
        serial: &str, 
        app_id: &str, 
        tx_log: mpsc::UnboundedSender<String>
    ) -> Result<()> {
        self.stop();
        
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat", "-c"])
            .stdin(Stdio::null())
            .status()
            .await;

        let mut child = Command::new("adb")
            .args([
                "-s", serial,
                "logcat",
                "-v", "color",
                "-s", "Timber:V", "AndroidRuntime:E", &format!("{}:V", app_id),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
            
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        
        let tx_out = tx_log.clone();
        let tx_err = tx_log.clone();
        
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx_out.send(line);
            }
        });

        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx_err.send(format!("[log-err] {}", line));
            }
        });

        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
    
    pub async fn check_status(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(None) => true,
                _ => {
                    self.child = None;
                    false
                }
            }
        } else {
            false
        }
    }
}
