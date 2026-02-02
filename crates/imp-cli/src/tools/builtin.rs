use crate::error::{ImpError, Result};
use serde_json::Value;
use std::fs;
use std::process::Command;

pub async fn execute_builtin(tool_name: &str, arguments: &Value) -> Result<String> {
    match tool_name {
        "exec" => exec_command(arguments).await,
        "file_read" => file_read(arguments).await,
        "file_write" => file_write(arguments).await,
        "file_edit" => file_edit(arguments).await,
        "search_code" => search_code(arguments).await,
        "list_files" => list_files(arguments).await,
        _ => Err(ImpError::Tool(format!("Unknown builtin tool: {}", tool_name))),
    }
}

async fn exec_command(arguments: &Value) -> Result<String> {
    let command = arguments.get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'command' parameter".to_string()))?;

    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(format!("Exit code: 0\nStdout:\n{}\nStderr:\n{}", stdout, stderr))
    } else {
        Ok(format!("Exit code: {}\nStdout:\n{}\nStderr:\n{}", 
                   output.status.code().unwrap_or(-1), stdout, stderr))
    }
}

async fn file_read(arguments: &Value) -> Result<String> {
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'path' parameter".to_string()))?;

    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) => Ok(format!("Error reading file '{}': {}", path, e)),
    }
}

async fn file_write(arguments: &Value) -> Result<String> {
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'path' parameter".to_string()))?;
    
    let content = arguments.get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'content' parameter".to_string()))?;

    match fs::write(path, content) {
        Ok(()) => Ok(format!("Successfully wrote {} bytes to '{}'", content.len(), path)),
        Err(e) => Ok(format!("Error writing file '{}': {}", path, e)),
    }
}

async fn file_edit(arguments: &Value) -> Result<String> {
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'path' parameter".to_string()))?;
    
    let old_text = arguments.get("old_text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'old_text' parameter".to_string()))?;
    
    let new_text = arguments.get("new_text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'new_text' parameter".to_string()))?;

    match fs::read_to_string(path) {
        Ok(content) => {
            if content.contains(old_text) {
                let updated_content = content.replace(old_text, new_text);
                match fs::write(path, &updated_content) {
                    Ok(()) => Ok(format!("Successfully replaced text in '{}'", path)),
                    Err(e) => Ok(format!("Error writing file '{}': {}", path, e)),
                }
            } else {
                Ok(format!("Text not found in file '{}'", path))
            }
        }
        Err(e) => Ok(format!("Error reading file '{}': {}", path, e)),
    }
}

async fn search_code(arguments: &Value) -> Result<String> {
    let query = arguments.get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'query' parameter".to_string()))?;
    
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    // Try ripgrep first, fall back to grep
    let output = Command::new("rg")
        .args(&["-n", "--color", "never", query, path])
        .output();

    let result = if let Ok(output) = output {
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            format!("No matches found for '{}' in '{}'", query, path)
        }
    } else {
        // Fall back to grep
        let output = Command::new("grep")
            .args(&["-rn", query, path])
            .output()?;
            
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            format!("No matches found for '{}' in '{}'", query, path)
        }
    };

    Ok(result)
}

async fn list_files(arguments: &Value) -> Result<String> {
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let output = Command::new("ls")
        .args(&["-la", path])
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(format!("Error listing files in '{}': {}", path, 
                   String::from_utf8_lossy(&output.stderr)))
    }
}