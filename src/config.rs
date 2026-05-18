use dotenvy;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone)]
pub struct Config {
    pub app_id: String,
    pub activity: String,
    pub watch_latency: f64,
    pub rebuild_gap: f64,
    pub log_tag: String,
    pub custom_commands: HashMap<char, String>,
}

impl Config {
    pub fn load() -> Self {
        let mut map = HashMap::new();
        let mut custom_commands = HashMap::new();
        
        // Defaults from the original project
        map.insert("APP_ID".to_string(), "com.mfc.manager.kotlin.dev".to_string());
        map.insert("WATCH_LATENCY".to_string(), "1.0".to_string());
        map.insert("REBUILD_GAP".to_string(), "2.0".to_string());
        map.insert("LOG_TAG".to_string(), "".to_string());

        // Load .deckdriodconfig if it exists
        if Path::new(".deckdriodconfig").exists() {
            if let Ok(iter) = dotenvy::from_path_iter(".deckdriodconfig") {
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

        let app_id = map.get("APP_ID").cloned().unwrap();
        
        // Use the activity from map or build the default one
        let activity = map.get("ACTIVITY")
            .cloned()
            .unwrap_or_else(|| format!("{}/com.mfc.manager.android.MainActivity", app_id));
            
        let watch_latency = map.get("WATCH_LATENCY")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);

        let rebuild_gap = map.get("REBUILD_GAP")
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0);

        let log_tag = map.get("LOG_TAG").cloned().unwrap_or_default();

        Config {
            app_id,
            activity,
            watch_latency,
            rebuild_gap,
            log_tag,
            custom_commands,
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut content = format!(
            "APP_ID={}\nACTIVITY={}\nWATCH_LATENCY={:.1}\nREBUILD_GAP={:.1}\nLOG_TAG={}\n",
            self.app_id, self.activity, self.watch_latency, self.rebuild_gap, self.log_tag
        );

        for (key, val) in &self.custom_commands {
            content.push_str(&format!("DECKDRIOD_CMD_{}={}\n", key.to_uppercase(), val));
        }

        std::fs::write(".deckdriodconfig", content)
    }
}
