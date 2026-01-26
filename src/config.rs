use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub scgi_socket: String,
    pub bind_address: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scgi_socket: "/tmp/rtorrent.sock".to_string(),
            bind_address: "0.0.0.0:3000".to_string(),
        }
    }
}

impl Config {
    /// Get config file path
    pub fn config_path() -> PathBuf {
        // Try to find config in current directory first, then home directory
        let local_config = PathBuf::from("vibetorrent.json");
        if local_config.exists() {
            return local_config;
        }

        // Try home directory
        if let Some(home) = dirs_path() {
            let home_config = home.join(".config").join("vibetorrent").join("config.json");
            if home_config.exists() {
                return home_config;
            }
        }

        // Default to local
        local_config
    }

    /// Load config from file
    pub fn load() -> Option<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save config to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;

        Ok(())
    }

    /// Check if config exists
    pub fn exists() -> bool {
        Self::config_path().exists()
    }
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
