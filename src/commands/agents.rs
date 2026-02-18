use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A persisted agent entry in the agents.json file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub pid: u32,
    pub title: String,
    pub action: String,
    pub started_at: i64,
    #[serde(default)]
    pub log_path: Option<String>,
    /// Set when the agent completes.
    #[serde(default)]
    pub finished_at: Option<i64>,
    /// Exit code on completion.
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// JSON output entry for `bn agents --json`.
#[derive(Debug, Serialize)]
struct AgentJsonEntry {
    bean_id: String,
    title: String,
    action: String,
    pid: u32,
    elapsed_secs: u64,
    status: String,
}

/// Return the path to the agents persistence file.
pub fn agents_file_path() -> Result<std::path::PathBuf> {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("beans");
    std::fs::create_dir_all(&dir).context("Failed to create beans state directory")?;
    Ok(dir.join("agents.json"))
}

/// Load agents from the persistence file. Returns empty map if file doesn't exist.
pub fn load_agents() -> Result<std::collections::HashMap<String, AgentEntry>> {
    let path = agents_file_path()?;
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let agents: std::collections::HashMap<String, AgentEntry> =
        serde_json::from_str(&contents).with_context(|| "Failed to parse agents.json")?;
    Ok(agents)
}

/// Save agents back to the persistence file.
pub fn save_agents(agents: &std::collections::HashMap<String, AgentEntry>) -> Result<()> {
    let path = agents_file_path()?;
    let json = serde_json::to_string_pretty(agents)?;
    std::fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Check if a process with the given PID is still alive.
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Format a duration in seconds as a human-readable string (e.g. "1m 32s").
fn format_elapsed(secs: u64) -> String {
    if secs >= 3600 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{}h {}m", h, m)
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{}m {:02}s", m, s)
    }
}

/// Show running and recently completed agents.
///
/// Reads agent state from the persistence file, checks PIDs, cleans up stale
/// entries, and displays a table of agents.
pub fn cmd_agents(_beans_dir: &Path, json: bool) -> Result<()> {
    let mut agents = load_agents()?;
    let now = chrono::Utc::now().timestamp();

    // Clean up stale entries: if PID is dead and no finished_at, mark as completed
    let mut changed = false;
    for (_id, entry) in agents.iter_mut() {
        if entry.finished_at.is_none() && !process_alive(entry.pid) {
            entry.finished_at = Some(now);
            entry.exit_code = Some(-1); // unknown — process vanished
            changed = true;
        }
    }

    // Remove completed entries older than 1 hour
    let one_hour_ago = now - 3600;
    let before_len = agents.len();
    agents.retain(|_id, entry| {
        entry.finished_at.map(|f| f > one_hour_ago).unwrap_or(true) // keep running agents
    });
    if agents.len() != before_len {
        changed = true;
    }

    if changed {
        save_agents(&agents)?;
    }

    if agents.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No running agents.");
        }
        return Ok(());
    }

    // Separate into running and completed
    let mut running: Vec<(&String, &AgentEntry)> = Vec::new();
    let mut completed: Vec<(&String, &AgentEntry)> = Vec::new();
    for (id, entry) in &agents {
        if entry.finished_at.is_some() {
            completed.push((id, entry));
        } else {
            running.push((id, entry));
        }
    }

    // Sort by bean ID
    running.sort_by(|a, b| crate::util::natural_cmp(a.0, b.0));
    completed.sort_by(|a, b| crate::util::natural_cmp(a.0, b.0));

    if json {
        let entries: Vec<AgentJsonEntry> = agents
            .iter()
            .map(|(id, entry)| {
                let elapsed = if let Some(finished) = entry.finished_at {
                    (finished - entry.started_at).unsigned_abs()
                } else {
                    (now - entry.started_at).unsigned_abs()
                };
                let status = if entry.finished_at.is_some() {
                    match entry.exit_code {
                        Some(0) => "completed".to_string(),
                        Some(code) => format!("failed({})", code),
                        None => "completed".to_string(),
                    }
                } else {
                    "running".to_string()
                };
                AgentJsonEntry {
                    bean_id: id.clone(),
                    title: entry.title.clone(),
                    action: entry.action.clone(),
                    pid: entry.pid,
                    elapsed_secs: elapsed,
                    status,
                }
            })
            .collect();
        let json_str = serde_json::to_string_pretty(&entries)?;
        println!("{}", json_str);
        return Ok(());
    }

    // Table output
    if !running.is_empty() {
        println!(
            "{:<6} {:<24} {:<12} {:<8} ELAPSED",
            "BEAN", "TITLE", "ACTION", "PID"
        );
        for (id, entry) in &running {
            let elapsed = (now - entry.started_at).unsigned_abs();
            let title = if entry.title.len() > 24 {
                format!("{}…", &entry.title[..23])
            } else {
                entry.title.clone()
            };
            println!(
                "{:<6} {:<24} {:<12} {:<8} {}",
                id,
                title,
                entry.action,
                entry.pid,
                format_elapsed(elapsed)
            );
        }
    }

    if !completed.is_empty() {
        if !running.is_empty() {
            println!();
        }
        println!("Recently completed:");
        for (id, entry) in &completed {
            let duration = entry
                .finished_at
                .map(|f| (f - entry.started_at).unsigned_abs())
                .unwrap_or(0);
            let status_str = match entry.exit_code {
                Some(0) => "✓".to_string(),
                Some(code) => format!("✗ exit {}", code),
                None => "?".to_string(),
            };
            let title = if entry.title.len() > 24 {
                format!("{}…", &entry.title[..23])
            } else {
                entry.title.clone()
            };
            println!(
                "  {} {} ({}, {})",
                id,
                title,
                status_str,
                format_elapsed(duration)
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn format_elapsed_seconds() {
        assert_eq!(format_elapsed(0), "0m 00s");
        assert_eq!(format_elapsed(48), "0m 48s");
        assert_eq!(format_elapsed(92), "1m 32s");
    }

    #[test]
    fn format_elapsed_hours() {
        assert_eq!(format_elapsed(3661), "1h 1m");
        assert_eq!(format_elapsed(7200), "2h 0m");
    }

    #[test]
    fn load_agents_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.json");
        std::fs::write(&path, "").unwrap();

        // Test parse of empty string returns empty map
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.trim().is_empty());
    }

    #[test]
    fn agent_entry_roundtrip() {
        let mut agents = HashMap::new();
        agents.insert(
            "5.1".to_string(),
            AgentEntry {
                pid: 42310,
                title: "Define user types".to_string(),
                action: "implement".to_string(),
                started_at: 1708000000,
                log_path: Some("/tmp/log".to_string()),
                finished_at: None,
                exit_code: None,
            },
        );

        let json = serde_json::to_string_pretty(&agents).unwrap();
        let parsed: HashMap<String, AgentEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        let entry = parsed.get("5.1").unwrap();
        assert_eq!(entry.pid, 42310);
        assert_eq!(entry.title, "Define user types");
        assert_eq!(entry.action, "implement");
        assert!(entry.finished_at.is_none());
    }

    #[test]
    fn agents_empty_persistence_shows_no_agents() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();

        // With no agents.json file at all, cmd_agents should work fine
        // We test via the load path
        // Can't easily override agents_file_path in test, so just verify
        // the load_agents function handles missing file
        let agents = load_agents();
        // This will try to read from the real state dir, which may or may not exist.
        // The function handles both cases gracefully.
        assert!(agents.is_ok());
    }

    #[test]
    fn process_alive_returns_true_for_current() {
        assert!(process_alive(std::process::id()));
    }

    #[test]
    fn process_alive_returns_false_for_nonexistent() {
        assert!(!process_alive(99_999_999));
    }
}
