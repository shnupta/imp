use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImpError {
    #[error("Config error: {0}")]
    ConfigParsing(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Dialogue error: {0}")]
    Dialogue(#[from] dialoguer::Error),
    
    #[error("Header error: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),
    
    #[error("Tool error: {0}")]
    Tool(String),
    
    #[error("Context error: {0}")]
    Context(String),
    
    #[error("Agent error: {0}")]
    Agent(String),
    
    #[error("Config error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),
}

impl From<toml::de::Error> for ImpError {
    fn from(err: toml::de::Error) -> Self {
        ImpError::ConfigParsing(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ImpError>;