use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub llm: LlmConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub thinking: ThinkingConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    /// MCP server configurations. Key is the server name.
    #[serde(default)]
    pub mcp: std::collections::HashMap<String, McpServerConfig>,
}

/// Configuration for an MCP server.
/// Transport is auto-detected: `url` → SSE, `command` → stdio.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServerConfig {
    /// SSE transport: URL of the MCP server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Stdio transport: command to spawn
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Stdio transport: arguments for the command
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Environment variables (supports ${VAR} expansion)
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub env: std::collections::HashMap<String, String>,
    /// HTTP headers for SSE transport (supports ${VAR} expansion)
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub headers: std::collections::HashMap<String, String>,
}

impl McpServerConfig {
    /// Whether this is an SSE (HTTP) or stdio (subprocess) server.
    pub fn is_sse(&self) -> bool {
        self.url.is_some()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Custom API base URL (e.g. for LiteLLM proxies). Defaults to provider's official URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Legacy API key field - still supported for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DisplayConfig {
    /// Syntect theme for code block highlighting.
    /// Run `imp themes` to list available themes.
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
        }
    }
}

fn default_theme() -> String {
    "base16-mocha.dark".to_string()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(default = "default_auth_method")]
    pub method: AuthMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<ApiKeyConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    ApiKey,
    OAuth,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OAuthConfig {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiKeyConfig {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThinkingConfig {
    #[serde(default = "default_thinking_enabled")]
    pub enabled: bool,
    #[serde(default = "default_budget_tokens")]
    pub budget_tokens: u32,
}

impl Default for ThinkingConfig {
    fn default() -> Self {
        Self {
            enabled: default_thinking_enabled(),
            budget_tokens: default_budget_tokens(),
        }
    }
}

fn default_thinking_enabled() -> bool {
    true
}

fn default_budget_tokens() -> u32 {
    10000
}

fn default_model() -> String {
    "claude-opus-4-5-20251101".to_string()
}

fn default_max_tokens() -> u32 {
    16384
}

fn default_auth_method() -> AuthMethod {
    AuthMethod::ApiKey
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
                "Config file not found at {}. Run 'imp bootstrap' first.",
                config_path.display()
            )));
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| ImpError::Config(format!("Failed to read config file: {}", e)))?;
        
        let mut config: Config = toml::from_str(&content)
            .map_err(|e| ImpError::Config(format!("Failed to parse config file: {}", e)))?;
        
        // Handle legacy format - migrate old api_key to new auth structure
        if let Some(legacy_key) = &config.llm.api_key {
            if config.auth.api_key.is_none() && config.auth.oauth.is_none() {
                config.auth.method = AuthMethod::ApiKey;
                config.auth.api_key = Some(ApiKeyConfig {
                    key: legacy_key.clone(),
                });
                // Clear the legacy field
                config.llm.api_key = None;
                
                // Save the migrated config
                config.save()?;
            }
        }
        
        // Validate the config
        match &config.auth.method {
            AuthMethod::ApiKey => {
                let api_key_config = config.auth.api_key.as_ref()
                    .ok_or_else(|| ImpError::Config("API key configuration missing. Run 'imp bootstrap' to set it up.".to_string()))?;
                
                if api_key_config.key.trim().is_empty() {
                    return Err(ImpError::Config(
                        "API key is empty. Run 'imp bootstrap' to set it up.".to_string()
                    ));
                }

                // Only validate key format when using the default Anthropic API
                // (custom base URLs like LiteLLM proxies use different token formats)
                if config.llm.base_url.is_none() {
                    if !api_key_config.key.starts_with("sk-ant-") {
                        return Err(ImpError::Config(
                            "API key doesn't look like a valid Anthropic key (should start with 'sk-ant-'). If using a proxy, set base_url in [llm].".to_string()
                        ));
                    }
                    
                    // If user has an OAuth token in the API key field, auto-switch to OAuth
                    if api_key_config.key.starts_with("sk-ant-oat") {
                        return Err(ImpError::Config(
                            "You have an OAuth token but API key auth is configured. Run 'imp login' to switch to OAuth.".to_string()
                        ));
                    }
                }
            }
            AuthMethod::OAuth => {
                if config.auth.oauth.is_none() {
                    return Err(ImpError::Config(
                        "OAuth configuration missing. Run 'imp bootstrap' or 'imp login' to set it up.".to_string()
                    ));
                }
            }
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

    /// Get the current authentication method
    pub fn auth_method(&self) -> &AuthMethod {
        &self.auth.method
    }

    /// Get the API key if using API key authentication
    pub fn api_key(&self) -> Option<&str> {
        self.auth.api_key.as_ref().map(|config| config.key.as_str())
    }

    /// Get OAuth config if using OAuth authentication
    pub fn oauth_config(&self) -> Option<&OAuthConfig> {
        self.auth.oauth.as_ref()
    }

    /// Update OAuth tokens and save to disk
    pub fn update_oauth_tokens(&mut self, access_token: String, refresh_token: String, expires_at: i64) -> Result<()> {
        self.auth.method = AuthMethod::OAuth;
        self.auth.oauth = Some(OAuthConfig {
            access_token,
            refresh_token,
            expires_at,
        });
        self.save()
    }

    /// Check if OAuth token is expired (with 5 minute buffer)
    pub fn is_oauth_token_expired(&self) -> bool {
        if let Some(oauth) = &self.auth.oauth {
            let now = chrono::Utc::now().timestamp();
            oauth.expires_at - 300 < now // 5 minute buffer
        } else {
            true
        }
    }

    /// Detect token type from token string and update config accordingly.
    /// Unknown token formats are stored as plain API keys (for proxy/LiteLLM use).
    pub fn setup_token_auto_detect(&mut self, token: String) -> Result<()> {
        if token.starts_with("sk-ant-oat") {
            // OAuth token - store as OAuth
            self.auth.method = AuthMethod::OAuth;
            self.auth.oauth = Some(OAuthConfig {
                access_token: token,
                refresh_token: String::new(), // setup-token doesn't need refresh
                expires_at: chrono::Utc::now().timestamp() + 365 * 24 * 60 * 60, // 1 year
            });
            self.auth.api_key = None;
        } else {
            // API key (Anthropic or proxy/LiteLLM token) - store as API key
            self.auth.method = AuthMethod::ApiKey;
            self.auth.api_key = Some(ApiKeyConfig { key: token });
            self.auth.oauth = None;
        }
        
        self.save()
    }
}

impl OAuthConfig {
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at - 300 < now // 5 minute buffer
    }
}
