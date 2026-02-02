use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "claude-3-5-sonnet-20241022".to_string()
}

/// Returns the Imp home directory (~/.imp/ by default, respects IMP_HOME env var).
pub fn imp_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("IMP_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = dirs::home_dir().ok_or_else(|| {
        ImpError::Config("Could not find home directory".to_string())
    })?;
    Ok(home.join(".imp"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Err(ImpError::Config(format!(
                "Config file not found at {}. Run 'imp init' first.",
                config_path.display()
            )));
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| ImpError::Config(format!("Failed to read config file: {}", e)))?;
        
        let config: Config = toml::from_str(&content)
            .map_err(|e| ImpError::Config(format!("Failed to parse config file: {}", e)))?;
        
        // Validate the config
        if config.llm.api_key.trim().is_empty() {
            return Err(ImpError::Config(
                "API key is empty. Run 'imp init' to set it up.".to_string()
            ));
        }

        if !config.llm.api_key.starts_with("sk-ant-") {
            return Err(ImpError::Config(
                "API key doesn't look like a valid Anthropic key (should start with 'sk-ant-')".to_string()
            ));
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content =
            toml::to_string_pretty(self).map_err(|e| ImpError::Config(e.to_string()))?;
        fs::write(config_path, content)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(imp_home()?.join("config.toml"))
    }
}
