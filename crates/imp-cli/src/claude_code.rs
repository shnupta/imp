use crate::config::OAuthConfig;
use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct ClaudeCodeCredentials {
    #[serde(rename = "claudeAiOauth")]
    pub claude_ai_oauth: ClaudeAiOAuth,
}

#[derive(Debug, Deserialize)]
pub struct ClaudeAiOAuth {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    pub scopes: Vec<String>,
    #[serde(rename = "subscriptionType")]
    pub subscription_type: String,
}

#[derive(Debug, Serialize)]
struct TokenRefreshRequest {
    grant_type: String,
    refresh_token: String,
    client_id: String,
}

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

/// Get the Claude Code credentials file path
pub fn claude_code_credentials_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| ImpError::Config("Could not find home directory".to_string()))?;
    Ok(home.join(".claude").join(".credentials.json"))
}

/// Check if Claude Code credentials exist and are readable
pub fn claude_code_credentials_exist() -> bool {
    claude_code_credentials_path()
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Read Claude Code credentials from ~/.claude/.credentials.json
pub fn read_claude_code_credentials() -> Result<ClaudeCodeCredentials> {
    let creds_path = claude_code_credentials_path()?;
    
    if !creds_path.exists() {
        return Err(ImpError::Config(format!(
            "Claude Code credentials not found at {}",
            creds_path.display()
        )));
    }

    let content = fs::read_to_string(&creds_path)
        .map_err(|e| ImpError::Config(format!("Failed to read Claude Code credentials: {}", e)))?;
    
    let credentials: ClaudeCodeCredentials = serde_json::from_str(&content)
        .map_err(|e| ImpError::Config(format!("Failed to parse Claude Code credentials: {}", e)))?;
    
    Ok(credentials)
}

/// Convert Claude Code credentials to imp's OAuthConfig format
pub fn to_oauth_config(credentials: &ClaudeCodeCredentials) -> OAuthConfig {
    OAuthConfig {
        access_token: credentials.claude_ai_oauth.access_token.clone(),
        refresh_token: credentials.claude_ai_oauth.refresh_token.clone(),
        expires_at: credentials.claude_ai_oauth.expires_at as i64,
    }
}

/// Refresh OAuth tokens using Anthropic's token endpoint
pub async fn refresh_oauth_tokens(refresh_token: &str) -> Result<OAuthConfig> {
    let client = reqwest::Client::new();
    
    let request_body = TokenRefreshRequest {
        grant_type: "refresh_token".to_string(),
        refresh_token: refresh_token.to_string(),
        client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".to_string(),
    };

    let response = client
        .post("https://console.anthropic.com/v1/oauth/token")
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(ImpError::Agent(format!("Token refresh failed: {}", error_text)));
    }

    let token_response: TokenRefreshResponse = response.json().await?;
    
    // Calculate expiration timestamp
    let expires_at = chrono::Utc::now().timestamp() + token_response.expires_in as i64;
    
    Ok(OAuthConfig {
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
    })
}