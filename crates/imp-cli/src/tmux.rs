//! Tmux integration for session tracking and pane management.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the current tmux pane identifier (e.g., "main:0.1")
pub fn get_current_pane() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}:#{window_index}.#{pane_index}"])
        .output()
        .ok()?;
    
    if output.status.success() {
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !pane.is_empty() && pane.contains(':') {
            return Some(pane);
        }
    }
    None
}

/// Directory for pane registration files
fn panes_dir() -> anyhow::Result<PathBuf> {
    let dir = crate::config::imp_home()?.join("panes");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Register the current process's tmux pane for a session
pub fn register_pane(session_id: &str) -> anyhow::Result<()> {
    let pane = match get_current_pane() {
        Some(p) => p,
        None => return Ok(()), // Not in tmux, skip registration
    };
    
    let pid = std::process::id();
    // Use first 8 chars of UUID for filename (avoid filesystem issues)
    let short_id = &session_id[..8.min(session_id.len())];
    let path = panes_dir()?.join(format!("{}.pane", short_id));
    let content = format!("{}\n{}\n{}\nidle", pid, pane, session_id);
    fs::write(path, content)?;
    Ok(())
}

/// Update the status for a session
pub fn set_status(session_id: &str, status: AgentStatus) -> anyhow::Result<()> {
    let short_id = &session_id[..8.min(session_id.len())];
    let path = panes_dir()?.join(format!("{}.pane", short_id));
    
    if !path.exists() {
        return Ok(()); // Not registered (not in tmux)
    }
    
    let content = fs::read_to_string(&path)?;
    let mut lines: Vec<&str> = content.lines().collect();
    
    // Ensure we have at least 4 lines
    while lines.len() < 4 {
        lines.push("idle");
    }
    
    // Update status (4th line)
    let new_content = format!("{}\n{}\n{}\n{}", 
        lines[0], lines[1], lines[2], status.as_str());
    fs::write(path, new_content)?;
    Ok(())
}

/// Unregister a session's pane (on exit)
pub fn unregister_pane(session_id: &str) -> anyhow::Result<()> {
    let short_id = &session_id[..8.min(session_id.len())];
    let path = panes_dir()?.join(format!("{}.pane", short_id));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Get pane info for a session (pid, pane_id, full_session_id, status)
pub fn get_pane_info_by_short_id(short_id: &str) -> Option<(u32, String, String, AgentStatus)> {
    let path = panes_dir().ok()?.join(format!("{}.pane", short_id));
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    let pid: u32 = lines.next()?.parse().ok()?;
    let pane = lines.next()?.to_string();
    let full_id = lines.next()?.to_string();
    let status = lines.next()
        .map(AgentStatus::from_str)
        .unwrap_or(AgentStatus::Idle);
    Some((pid, pane, full_id, status))
}

/// Check if a process is still running
pub fn is_process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

/// Switch to a tmux pane
pub fn switch_to_pane(pane_id: &str) -> anyhow::Result<()> {
    // pane_id format: "session:window.pane"
    // Extract session name for switch-client
    let session = pane_id.split(':').next().unwrap_or(pane_id);
    
    // Switch client to the target session (needed for cross-session jumps)
    Command::new("tmux")
        .args(["switch-client", "-t", session])
        .output()?;
    
    // Select the specific window and pane
    Command::new("tmux")
        .args(["select-window", "-t", pane_id])
        .output()?;
    
    Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .output()?;
    
    Ok(())
}

/// Agent status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,      // Waiting for user input
    Working,   // Processing a message
}

impl AgentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s {
            "working" => AgentStatus::Working,
            _ => AgentStatus::Idle,
        }
    }
}

/// Pane registration info
pub struct PaneInfo {
    pub session_id: String,
    pub pid: u32,
    pub pane: String,
    pub status: AgentStatus,
}

/// List all registered panes
pub fn list_registered_panes() -> Vec<PaneInfo> {
    let dir = match panes_dir() {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    
    let mut result = vec![];
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "pane").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Some((pid, pane, full_id, status)) = get_pane_info_by_short_id(stem) {
                        result.push(PaneInfo {
                            session_id: full_id,
                            pid,
                            pane,
                            status,
                        });
                    }
                }
            }
        }
    }
    result
}
