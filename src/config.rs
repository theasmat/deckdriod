use dotenvy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Config {
    pub app_id: String,
    pub activity: String,
    pub watch_latency: f64,
    pub rebuild_gap: f64,
    pub log_tag: String,
    pub project_path: String,
    pub output_path: String,
    pub mcp_port: u16,
    pub build_variant: String,
    pub custom_commands: HashMap<char, String>,
}

impl Config {
    fn get_config_path() -> PathBuf {
        if Path::new(".deckdriodconfig").exists() {
            return PathBuf::from(".deckdriodconfig");
        }
        if let Some(mut home) = dirs::home_dir() {
            home.push(".deckdriodconfig");
            if home.exists() {
                return home;
            }
        }
        PathBuf::from(".deckdriodconfig")
    }

    pub fn load() -> Self {
        let mut map = HashMap::new();
        let mut custom_commands = HashMap::new();
        
        map.insert("APP_ID".to_string(), "com.mfc.manager.kotlin.dev".to_string());
        map.insert("WATCH_LATENCY".to_string(), "1.0".to_string());
        map.insert("REBUILD_GAP".to_string(), "2.0".to_string());
        map.insert("LOG_TAG".to_string(), "".to_string());
        map.insert("PROJECT_PATH".to_string(), ".".to_string());
        map.insert("OUTPUT_PATH".to_string(), ".".to_string());
        map.insert("MCP_PORT".to_string(), "3000".to_string());

        let config_path = Self::get_config_path();

        if config_path.exists() {
            if let Ok(iter) = dotenvy::from_path_iter(&config_path) {
                for item in iter {
                    if let Ok((key, value)) = item {
                        if key.starts_with("DECKDRIOD_CMD_") {
                            if let Some(c) = key.chars().last().map(|c| c.to_ascii_lowercase()) {
                                custom_commands.insert(c, value);
                            }
                        } else {
                            map.insert(key, value);
                        }
                    }
                }
            }
        }

        let app_id = map.get("APP_ID").cloned().unwrap_or_else(|| "com.example.app".to_string());
        let activity = map.get("ACTIVITY").cloned().unwrap_or_else(|| format!("{}/.MainActivity", app_id));
        let watch_latency = map.get("WATCH_LATENCY").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let rebuild_gap = map.get("REBUILD_GAP").and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let log_tag = map.get("LOG_TAG").cloned().unwrap_or_default();
        let project_path = map.get("PROJECT_PATH").cloned().unwrap_or_else(|| ".".to_string());
        let output_path = map.get("OUTPUT_PATH").cloned().unwrap_or_else(|| ".".to_string());
        let mcp_port = map.get("MCP_PORT").and_then(|s| s.parse().ok()).unwrap_or(3000);

        Config {
            app_id,
            activity,
            watch_latency,
            rebuild_gap,
            log_tag,
            project_path,
            output_path,
            mcp_port,
            build_variant: map.get("BUILD_VARIANT").cloned().unwrap_or_else(|| "debug".to_string()),
            custom_commands,
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut content = format!(
            "APP_ID={}\nACTIVITY={}\nWATCH_LATENCY={:.1}\nREBUILD_GAP={:.1}\nLOG_TAG={}\nPROJECT_PATH={}\nOUTPUT_PATH={}\nMCP_PORT={}\nBUILD_VARIANT={}\n",
            self.app_id, self.activity, self.watch_latency, self.rebuild_gap, self.log_tag, self.project_path, self.output_path, self.mcp_port, self.build_variant
        );

        for (key, val) in &self.custom_commands {
            content.push_str(&format!("DECKDRIOD_CMD_{}={}\n", key.to_uppercase(), val));
        }

        let config_path = Self::get_config_path();
        std::fs::write(config_path, content)
    }
}
