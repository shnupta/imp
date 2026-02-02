use crate::error::{ImpError, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    #[serde(default = "default_repos_dir")]
    pub repos_dir: String,
}

fn default_model() -> String {
    "claude-3-sonnet-20240229".to_string()
}

fn default_repos_dir() -> String {
    "~/code".to_string()
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        
        if !config_path.exists() {
            return Err(ImpError::Config(
                format!("Config file not found at {}. Run 'imp init' first.", config_path.display())
            ));
        }

        let content = fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        
        // Create config directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| ImpError::Config(e.to_string()))?;
        fs::write(config_path, content)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let home = home_dir().ok_or_else(|| {
            ImpError::Config("Could not find home directory".to_string())
        })?;
        Ok(home.join(".imp").join("config.toml"))
    }

    pub fn config_dir() -> Result<PathBuf> {
        let home = home_dir().ok_or_else(|| {
            ImpError::Config("Could not find home directory".to_string())
        })?;
        Ok(home.join(".imp"))
    }
}