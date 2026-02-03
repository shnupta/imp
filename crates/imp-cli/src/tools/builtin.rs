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
        // spawn_agent and check_agents are intercepted by Agent before reaching here.
        // If they somehow arrive, return an informative error.
        "spawn_agent" | "check_agents" => {
            Err(ImpError::Tool(format!(
                "'{}' must be handled by the Agent, not the builtin executor",
                tool_name
            )))
        }
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

    let content = fs::read_to_string(path)
        .map_err(|e| ImpError::Tool(format!("Failed to read file '{}': {}", path, e)))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Parse optional offset (1-indexed line number) and limit
    let offset = arguments
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|v| v.max(1) as usize)
        .unwrap_or(1);

    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let start_idx = (offset - 1).min(total_lines);
    let end_idx = match limit {
        Some(lim) => (start_idx + lim).min(total_lines),
        None => total_lines,
    };

    let selected = &lines[start_idx..end_idx];

    // Format with line numbers
    let mut output = String::new();
    for (i, line) in selected.iter().enumerate() {
        let line_num = start_idx + i + 1;
        output.push_str(&format!("{:>4} | {}\n", line_num, line));
    }

    // Add metadata header
    let range_info = if start_idx == 0 && end_idx == total_lines {
        format!("{} ({} lines)", path, total_lines)
    } else {
        format!(
            "{} (lines {}-{} of {})",
            path,
            start_idx + 1,
            end_idx,
            total_lines
        )
    };

    Ok(format!("{}\n{}", range_info, output))
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
        // Help the model debug: show nearby lines if we can find a partial match
        let first_line = old_text.lines().next().unwrap_or(old_text);
        let hint = if first_line.len() > 10 {
            let partial = &first_line[..first_line.len().min(40)];
            let partial_matches: Vec<usize> = content
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(partial))
                .map(|(i, _)| i + 1)
                .collect();
            if partial_matches.is_empty() {
                String::new()
            } else {
                format!(
                    " Partial match on first line found at line(s): {:?}. Use file_read with offset to check exact content.",
                    &partial_matches[..partial_matches.len().min(5)]
                )
            }
        } else {
            String::new()
        };
        return Err(ImpError::Tool(format!(
            "old_text not found in '{}'. Ensure it matches exactly (including whitespace/indentation).{}",
            path, hint
        )));
    }

    if occurrences > 1 {
        // Find line numbers of each occurrence to help the model be more specific
        let mut positions = Vec::new();
        let mut search_from = 0;
        for _ in 0..occurrences {
            if let Some(pos) = content[search_from..].find(old_text) {
                let abs_pos = search_from + pos;
                let line_num = content[..abs_pos].matches('\n').count() + 1;
                positions.push(line_num);
                search_from = abs_pos + old_text.len();
            }
        }
        return Err(ImpError::Tool(format!(
            "old_text matches {} locations in '{}' (lines {:?}). Include more surrounding context in old_text to match exactly one location.",
            occurrences, path, positions
        )));
    }

    // Exactly one match — safe to replace
    let match_pos = content.find(old_text).unwrap();
    let start_line = content[..match_pos].matches('\n').count() + 1;
    let old_line_count = old_text.matches('\n').count() + 1;
    let new_line_count = new_text.matches('\n').count() + 1;

    let updated_content = content.replacen(old_text, new_text, 1);
    fs::write(path, &updated_content)
        .map_err(|e| ImpError::Tool(format!("Failed to write file '{}': {}", path, e)))?;

    let total_lines = updated_content.lines().count();
    Ok(format!(
        "Replaced lines {}-{} ({} lines → {}) in '{}' ({} total lines)",
        start_line,
        start_line + old_line_count - 1,
        old_line_count,
        new_line_count,
        path,
        total_lines
    ))
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