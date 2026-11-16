use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub pin: String,
    pub service_name: String,
    pub preferred_printer: Option<String>,
    pub max_upload_mb: usize,
    pub mdns_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8989,
            pin: "1234".to_string(),
            service_name: "Office Wireless Printer".to_string(),
            preferred_printer: None,
            max_upload_mb: 100,
            mdns_enabled: true,
        }
    }
}

impl ServerConfig {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: Self = toml::from_str(&content).unwrap_or_else(|e| {
                info!("Failed to parse config.toml ({:?}), falling back to default", e);
                Self::default()
            });
            Ok(config)
        } else {
            let config = Self::default();
            let toml_str = toml::to_string_pretty(&config)?;
            let _ = fs::write(path, toml_str);
            info!("Created default configuration at {:?}", path);
            Ok(config)
        }
    }
}
