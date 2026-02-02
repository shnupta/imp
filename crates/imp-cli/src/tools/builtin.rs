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

    fs::read_to_string(path)
        .map_err(|e| ImpError::Tool(format!("Failed to read file '{}': {}", path, e)))
}

async fn file_write(arguments: &Value) -> Result<String> {
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'path' parameter".to_string()))?;
    
    let content = arguments.get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'content' parameter".to_string()))?;

    // Create parent directories if they don't exist
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ImpError::Tool(format!("Failed to create parent directories for '{}': {}", path, e)))?;
    }

    fs::write(path, content)
        .map_err(|e| ImpError::Tool(format!("Failed to write file '{}': {}", path, e)))?;
    
    Ok(format!("Successfully wrote {} bytes to '{}'", content.len(), path))
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

    let content = fs::read_to_string(path)
        .map_err(|e| ImpError::Tool(format!("Failed to read file '{}': {}", path, e)))?;

    let occurrences = content.matches(old_text).count();
    if occurrences == 0 {
        return Err(ImpError::Tool(format!("Text not found in file '{}'", path)));
    }

    let updated_content = content.replace(old_text, new_text);
    fs::write(path, &updated_content)
        .map_err(|e| ImpError::Tool(format!("Failed to write updated file '{}': {}", path, e)))?;

    Ok(format!("Successfully replaced {} occurrence(s) in '{}'", occurrences, path))
}

async fn search_code(arguments: &Value) -> Result<String> {
    let query = arguments.get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImpError::Tool("Missing 'query' parameter".to_string()))?;
    
    let path = arguments.get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    // Try ripgrep first (better performance and features)
    if let Ok(output) = Command::new("rg")
        .args(&[
            "-n",           // Show line numbers
            "--color", "never",  // No color output
            "--type-add", "code:*.{rs,py,js,ts,go,java,cpp,c,h,hpp}", // Define code file types
            "--type", "code",    // Search only code files
            "--context", "2",    // Show 2 lines of context
            "--max-count", "50", // Limit to 50 matches per file
            query,
            path
        ])
        .output() 
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                Ok(format!("No matches found for '{}' in code files under '{}'", query, path))
            } else {
                Ok(format!("Search results for '{}' in '{}':\n\n{}", query, path, stdout))
            }
        } else {
            Ok(format!("No matches found for '{}' in code files under '{}'", query, path))
        }
    } else {
        // Fallback to basic grep
        let output = Command::new("grep")
            .args(&["-rn", "--include=*.rs", "--include=*.py", "--include=*.js", 
                   "--include=*.ts", "--include=*.go", "--include=*.java", query, path])
            .output()
            .map_err(|e| ImpError::Tool(format!("Search command failed: {}", e)))?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                Ok(format!("No matches found for '{}' in code files under '{}'", query, path))
            } else {
                Ok(format!("Search results for '{}' in '{}':\n\n{}", query, path, stdout))
            }
        } else {
            Ok(format!("No matches found for '{}' in code files under '{}'", query, path))
        }
    }
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