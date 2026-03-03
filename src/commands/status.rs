use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde::Serialize;

use crate::bean::Status;
use crate::blocking::{check_blocked, check_scope_warning, BlockReason};
use crate::index::{Index, IndexEntry};
use crate::util::natural_cmp;

/// Agent status parsed from claimed_by field
#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub pid: u32,
    pub alive: bool,
}

/// Parse claimed_by field for agent info (e.g., "spro:12345" -> Some(AgentStatus))
fn parse_agent_claim(claimed_by: &Option<String>) -> Option<AgentStatus> {
    let claim = claimed_by.as_ref()?;
    if !claim.starts_with("spro:") {
        return None;
    }
    let pid_str = claim.strip_prefix("spro:")?;
    let pid: u32 = pid_str.parse().ok()?;
    let alive = is_pid_alive(pid);
    Some(AgentStatus { pid, alive })
}

/// Check if a process with the given PID is alive
fn is_pid_alive(pid: u32) -> bool {
    // Use kill -0 to check if process exists (doesn't send a signal)
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Format agent status for display
fn format_agent_status(entry: &IndexEntry) -> String {
    match parse_agent_claim(&entry.claimed_by) {
        Some(agent) if agent.alive => format!("spro:{} ●", agent.pid),
        Some(agent) => format!("spro:{} ✗", agent.pid),
        None => entry.claimed_by.clone().unwrap_or_else(|| "-".to_string()),
    }
}

/// Entry with agent status for JSON output
#[derive(Serialize)]
struct StatusEntry {
    #[serde(flatten)]
    entry: IndexEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<AgentStatus>,
}

impl StatusEntry {
    fn from_entry(entry: IndexEntry) -> Self {
        let agent = parse_agent_claim(&entry.claimed_by);
        Self { entry, agent }
    }
}

/// Blocked entry with reason for JSON output
#[derive(Serialize)]
struct BlockedEntry {
    #[serde(flatten)]
    entry: IndexEntry,
    block_reason: String,
}

/// JSON output structure for status command
#[derive(Serialize)]
struct StatusOutput {
    claimed: Vec<StatusEntry>,
    ready: Vec<IndexEntry>,
    goals: Vec<IndexEntry>,
    blocked: Vec<BlockedEntry>,
}

/// Show complete work picture: claimed, ready, goals (need decomposition), and blocked beans
pub fn cmd_status(json: bool, beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    // Separate beans into categories
    let mut claimed: Vec<&IndexEntry> = Vec::new();
    let mut ready: Vec<&IndexEntry> = Vec::new();
    let mut goals: Vec<&IndexEntry> = Vec::new();
    let mut blocked: Vec<(&IndexEntry, BlockReason)> = Vec::new();

    for entry in &index.beans {
        match entry.status {
            Status::InProgress => {
                claimed.push(entry);
            }
            Status::Open => {
                if let Some(reason) = check_blocked(entry, &index) {
                    blocked.push((entry, reason));
                } else if entry.has_verify {
                    ready.push(entry);
                } else {
                    goals.push(entry);
                }
            }
            Status::Closed => {}
        }
    }

    sort_beans(&mut claimed);
    sort_beans(&mut ready);
    sort_beans(&mut goals);
    blocked.sort_by(|(a, _), (b, _)| match a.priority.cmp(&b.priority) {
        std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
        other => other,
    });

    if json {
        let output = StatusOutput {
            claimed: claimed
                .into_iter()
                .cloned()
                .map(StatusEntry::from_entry)
                .collect(),
            ready: ready.into_iter().cloned().collect(),
            goals: goals.into_iter().cloned().collect(),
            blocked: blocked
                .iter()
                .map(|(e, reason)| BlockedEntry {
                    entry: (*e).clone(),
                    block_reason: reason.to_string(),
                })
                .collect(),
        };
        let json_str = serde_json::to_string_pretty(&output)?;
        println!("{}", json_str);
    } else {
        println!("## Claimed ({})", claimed.len());
        if claimed.is_empty() {
            println!("  (none)");
        } else {
            for entry in claimed {
                let agent_str = format_agent_status(entry);
                println!("  {} [-] {} ({})", entry.id, entry.title, agent_str);
            }
        }
        println!();

        println!("## Ready ({})", ready.len());
        if ready.is_empty() {
            println!("  (none)");
        } else {
            for entry in ready {
                let warning = check_scope_warning(entry)
                    .map(|w| format!("  (⚠ {})", w))
                    .unwrap_or_default();
                println!("  {} [ ] {}{}", entry.id, entry.title, warning);
            }
        }
        println!();

        println!("## Goals (need decomposition) ({})", goals.len());
        if goals.is_empty() {
            println!("  (none)");
        } else {
            for entry in goals {
                println!("  {} [?] {}", entry.id, entry.title);
            }
        }
        println!();

        println!("## Blocked ({})", blocked.len());
        if blocked.is_empty() {
            println!("  (none)");
        } else {
            for (entry, reason) in &blocked {
                println!("  {} [!] {}  ({})", entry.id, entry.title, reason);
            }
        }
    }

    Ok(())
}

fn sort_beans(beans: &mut Vec<&IndexEntry>) {
    beans.sort_by(|a, b| match a.priority.cmp(&b.priority) {
        std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
        other => other,
    });
}
