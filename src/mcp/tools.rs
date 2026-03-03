//! MCP tool definitions and handlers.
//!
//! Each tool maps to a beans operation. Handlers work directly with
//! Bean/Index types to avoid stdout pollution from CLI commands.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Value};

use crate::bean::{Bean, Status};
use crate::blocking::check_blocked;
use crate::config::Config;
use crate::discovery::find_bean_file;
use crate::index::{Index, IndexEntry};
use crate::mcp::protocol::ToolDefinition;
use crate::util::{natural_cmp, title_to_slug};

/// Return all MCP tool definitions.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_beans".to_string(),
            description: "List beans with optional status and priority filters".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["open", "in_progress", "closed"],
                        "description": "Filter by status"
                    },
                    "priority": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 4,
                        "description": "Filter by priority (0-4, where P0 is highest)"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Filter by parent bean ID"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "show_bean".to_string(),
            description: "Get full bean details including description, acceptance criteria, verify command, and history".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Bean ID"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "ready_beans".to_string(),
            description: "Get beans ready to work on (open, has verify command, all dependencies resolved)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "create_bean".to_string(),
            description: "Create a new bean (task/spec for agents)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Bean title"
                    },
                    "description": {
                        "type": "string",
                        "description": "Full description / agent context (markdown)"
                    },
                    "verify": {
                        "type": "string",
                        "description": "Shell command that must exit 0 to close the bean"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Parent bean ID (creates a child bean)"
                    },
                    "priority": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 4,
                        "description": "Priority 0-4 (P0 highest, default P2)"
                    },
                    "acceptance": {
                        "type": "string",
                        "description": "Acceptance criteria"
                    },
                    "deps": {
                        "type": "string",
                        "description": "Comma-separated dependency bean IDs"
                    }
                },
                "required": ["title"]
            }),
        },
        ToolDefinition {
            name: "claim_bean".to_string(),
            description: "Claim a bean for work (sets status to in_progress)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Bean ID to claim"
                    },
                    "by": {
                        "type": "string",
                        "description": "Who is claiming (agent name or user)"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "close_bean".to_string(),
            description: "Close a bean (runs verify gate first if configured). Returns error if verify fails.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Bean ID to close"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Skip verify command (force close)",
                        "default": false
                    },
                    "reason": {
                        "type": "string",
                        "description": "Close reason"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "verify_bean".to_string(),
            description: "Run a bean's verify command without closing it. Returns pass/fail and output.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Bean ID to verify"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "context_bean".to_string(),
            description: "Get assembled context for a bean (reads files referenced in description)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Bean ID"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "status".to_string(),
            description: "Project status overview: claimed, ready, goals, and blocked beans".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "tree".to_string(),
            description: "Hierarchical bean tree showing parent-child relationships and status".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Root bean ID (shows full tree if omitted)"
                    }
                }
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Tool Handlers
// ---------------------------------------------------------------------------

/// Dispatch a tool call to the appropriate handler.
pub fn handle_tool_call(name: &str, args: &Value, beans_dir: &Path) -> Value {
    let result = match name {
        "list_beans" => handle_list_beans(args, beans_dir),
        "show_bean" => handle_show_bean(args, beans_dir),
        "ready_beans" => handle_ready_beans(beans_dir),
        "create_bean" => handle_create_bean(args, beans_dir),
        "claim_bean" => handle_claim_bean(args, beans_dir),
        "close_bean" => handle_close_bean(args, beans_dir),
        "verify_bean" => handle_verify_bean(args, beans_dir),
        "context_bean" => handle_context_bean(args, beans_dir),
        "status" => handle_status(beans_dir),
        "tree" => handle_tree(args, beans_dir),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    };

    match result {
        Ok(text) => json!({
            "content": [{ "type": "text", "text": text }]
        }),
        Err(e) => json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

// ---------------------------------------------------------------------------
// Individual Handlers
// ---------------------------------------------------------------------------

fn handle_list_beans(args: &Value, beans_dir: &Path) -> Result<String> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let status_filter = args
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(crate::util::parse_status);

    let priority_filter = args
        .get("priority")
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);

    let parent_filter = args.get("parent").and_then(|v| v.as_str());

    let filtered: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| {
            if let Some(status) = status_filter {
                if entry.status != status {
                    return false;
                }
            } else if entry.status == Status::Closed {
                // Exclude closed by default
                return false;
            }
            if let Some(priority) = priority_filter {
                if entry.priority != priority {
                    return false;
                }
            }
            if let Some(parent) = parent_filter {
                if entry.parent.as_deref() != Some(parent) {
                    return false;
                }
            }
            true
        })
        .collect();

    let entries: Vec<Value> = filtered
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "title": e.title,
                "status": format!("{}", e.status),
                "priority": format!("P{}", e.priority),
                "parent": e.parent,
                "has_verify": e.has_verify,
                "claimed_by": e.claimed_by,
            })
        })
        .collect();

    serde_json::to_string_pretty(&json!({ "beans": entries, "count": entries.len() }))
        .context("Failed to serialize bean list")
}

fn handle_show_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?;

    crate::util::validate_bean_id(id)?;
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    serde_json::to_string_pretty(&bean).context("Failed to serialize bean")
}

fn handle_ready_beans(beans_dir: &Path) -> Result<String> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let mut ready: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| {
            entry.has_verify
                && entry.status == Status::Open
                && check_blocked(entry, &index).is_none()
        })
        .collect();

    ready.sort_by(|a, b| match a.priority.cmp(&b.priority) {
        std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
        other => other,
    });

    let entries: Vec<Value> = ready
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "title": e.title,
                "priority": format!("P{}", e.priority),
            })
        })
        .collect();

    serde_json::to_string_pretty(&json!({ "ready": entries, "count": entries.len() }))
        .context("Failed to serialize ready beans")
}

fn handle_create_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: title"))?;

    let description = args.get("description").and_then(|v| v.as_str());
    let verify = args.get("verify").and_then(|v| v.as_str());
    let parent = args.get("parent").and_then(|v| v.as_str());
    let priority = args
        .get("priority")
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);
    let acceptance = args.get("acceptance").and_then(|v| v.as_str());
    let deps = args.get("deps").and_then(|v| v.as_str());

    if let Some(p) = priority {
        crate::bean::validate_priority(p)?;
    }

    // Determine bean ID
    let mut config = Config::load(beans_dir)?;
    let bean_id = if let Some(parent_id) = parent {
        crate::util::validate_bean_id(parent_id)?;
        crate::commands::create::assign_child_id(beans_dir, parent_id)?
    } else {
        let id = config.increment_id();
        config.save(beans_dir)?;
        id.to_string()
    };

    let slug = title_to_slug(title);
    let mut bean = Bean::try_new(&bean_id, title)?;
    bean.slug = Some(slug.clone());

    if let Some(desc) = description {
        bean.description = Some(desc.to_string());
    }
    if let Some(v) = verify {
        bean.verify = Some(v.to_string());
    }
    if let Some(p) = parent {
        bean.parent = Some(p.to_string());
    }
    if let Some(p) = priority {
        bean.priority = p;
    }
    if let Some(a) = acceptance {
        bean.acceptance = Some(a.to_string());
    }
    if let Some(d) = deps {
        bean.dependencies = d.split(',').map(|s| s.trim().to_string()).collect();
    }

    // Write bean file
    let bean_path = beans_dir.join(format!("{}-{}.md", bean_id, slug));
    bean.to_file(&bean_path)?;

    // Rebuild index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    Ok(format!("Created bean {}: {}", bean_id, title))
}

fn handle_claim_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?;
    let by = args.get("by").and_then(|v| v.as_str());

    crate::util::validate_bean_id(id)?;
    let bean_path = find_bean_file(beans_dir, id)?;
    let mut bean = Bean::from_file(&bean_path)?;

    if bean.status != Status::Open {
        anyhow::bail!(
            "Bean {} is {} — only open beans can be claimed",
            id,
            bean.status
        );
    }

    let now = Utc::now();
    bean.status = Status::InProgress;
    bean.claimed_by = by.map(|s| s.to_string());
    bean.claimed_at = Some(now);
    bean.updated_at = now;

    bean.to_file(&bean_path)?;

    // Rebuild index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    let claimer = by.unwrap_or("anonymous");
    Ok(format!(
        "Claimed bean {}: {} (by {})",
        id, bean.title, claimer
    ))
}

fn handle_close_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?;
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    let reason = args.get("reason").and_then(|v| v.as_str());

    crate::util::validate_bean_id(id)?;
    let bean_path = find_bean_file(beans_dir, id)?;
    let mut bean = Bean::from_file(&bean_path)?;

    // Run verify if configured and not forced
    if let Some(ref verify_cmd) = bean.verify {
        if !force {
            let project_root = beans_dir
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Cannot determine project root"))?;

            let output = std::process::Command::new("sh")
                .args(["-c", verify_cmd])
                .current_dir(project_root)
                .output()
                .with_context(|| format!("Failed to execute verify: {}", verify_cmd))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let combined = format!("{}{}", stdout, stderr);
                let snippet = if combined.len() > 2000 {
                    format!("...{}", &combined[combined.len() - 2000..])
                } else {
                    combined.to_string()
                };

                bean.attempts += 1;
                bean.updated_at = Utc::now();
                bean.to_file(&bean_path)?;

                // Rebuild index to reflect attempt count
                let index = Index::build(beans_dir)?;
                index.save(beans_dir)?;

                anyhow::bail!(
                    "Verify failed for bean {} (attempt {})\nCommand: {}\nOutput:\n{}",
                    id,
                    bean.attempts,
                    verify_cmd,
                    snippet.trim()
                );
            }
        }
    }

    // Close the bean
    let now = Utc::now();
    bean.status = Status::Closed;
    bean.closed_at = Some(now);
    bean.close_reason = reason.map(|s| s.to_string());
    bean.updated_at = now;

    bean.to_file(&bean_path)?;

    // Archive the bean
    let slug = bean
        .slug
        .clone()
        .unwrap_or_else(|| title_to_slug(&bean.title));
    let ext = bean_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("md");
    let today = chrono::Local::now().naive_local().date();
    let archive_path = crate::discovery::archive_path_for_bean(beans_dir, id, &slug, ext, today);

    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&bean_path, &archive_path)?;

    bean.is_archived = true;
    bean.to_file(&archive_path)?;

    // Rebuild index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    // Check auto-close parent
    if let Some(parent_id) = &bean.parent {
        let auto_close = Config::load(beans_dir)
            .map(|c| c.auto_close_parent)
            .unwrap_or(true);
        if auto_close {
            if let Ok(true) = all_children_closed(beans_dir, parent_id) {
                let _ = auto_close_parent(beans_dir, parent_id);
            }
        }
    }

    Ok(format!("Closed bean {}: {}", id, bean.title))
}

fn handle_verify_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?;

    crate::util::validate_bean_id(id)?;
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    let verify_cmd = match &bean.verify {
        Some(cmd) => cmd.clone(),
        None => return Ok(format!("Bean {} has no verify command", id)),
    };

    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine project root"))?;

    let output = std::process::Command::new("sh")
        .args(["-c", &verify_cmd])
        .current_dir(project_root)
        .output()
        .with_context(|| format!("Failed to execute verify: {}", verify_cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let passed = output.status.success();

    Ok(serde_json::to_string_pretty(&json!({
        "id": id,
        "passed": passed,
        "command": verify_cmd,
        "exit_code": output.status.code(),
        "stdout": truncate_str(&stdout, 2000),
        "stderr": truncate_str(&stderr, 2000),
    }))?)
}

fn handle_context_bean(args: &Value, beans_dir: &Path) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?;

    crate::util::validate_bean_id(id)?;
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    let project_dir = beans_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine project root"))?;

    let description = bean.description.as_deref().unwrap_or("");
    let paths = crate::ctx_assembler::extract_paths(description);

    if paths.is_empty() {
        return Ok(format!("Bean {}: no file paths found in description", id));
    }

    let context = crate::ctx_assembler::assemble_context(paths, project_dir)
        .context("Failed to assemble context")?;

    Ok(context)
}

fn handle_status(beans_dir: &Path) -> Result<String> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let mut claimed = Vec::new();
    let mut ready = Vec::new();
    let mut goals = Vec::new();
    let mut blocked: Vec<(&IndexEntry, String)> = Vec::new();

    for entry in &index.beans {
        match entry.status {
            Status::InProgress => claimed.push(entry),
            Status::Open => {
                if let Some(reason) = check_blocked(entry, &index) {
                    blocked.push((entry, reason.to_string()));
                } else if entry.has_verify {
                    ready.push(entry);
                } else {
                    goals.push(entry);
                }
            }
            Status::Closed => {}
        }
    }

    let format_entries = |entries: &[&IndexEntry]| -> Vec<Value> {
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "priority": format!("P{}", e.priority),
                    "claimed_by": e.claimed_by,
                })
            })
            .collect()
    };

    let blocked_entries: Vec<Value> = blocked
        .iter()
        .map(|(e, reason)| {
            json!({
                "id": e.id,
                "title": e.title,
                "priority": format!("P{}", e.priority),
                "claimed_by": e.claimed_by,
                "block_reason": reason,
            })
        })
        .collect();

    serde_json::to_string_pretty(&json!({
        "claimed": format_entries(&claimed),
        "ready": format_entries(&ready),
        "goals": format_entries(&goals),
        "blocked": blocked_entries,
        "summary": format!(
            "{} claimed, {} ready, {} goals, {} blocked",
            claimed.len(), ready.len(), goals.len(), blocked.len()
        )
    }))
    .context("Failed to serialize status")
}

fn handle_tree(args: &Value, beans_dir: &Path) -> Result<String> {
    let index = Index::load_or_rebuild(beans_dir)?;
    let root_id = args.get("id").and_then(|v| v.as_str());

    let mut output = String::new();

    if let Some(root) = root_id {
        render_subtree(&index, root, "", true, &mut output);
    } else {
        // Find root beans (no parent)
        let roots: Vec<&IndexEntry> = index.beans.iter().filter(|e| e.parent.is_none()).collect();

        for (i, root) in roots.iter().enumerate() {
            let is_last = i == roots.len() - 1;
            let status_icon = status_icon(root.status);
            output.push_str(&format!("{} {} {}\n", status_icon, root.id, root.title));
            render_children(&index, &root.id, "  ", &mut output);
            if !is_last {
                output.push('\n');
            }
        }
    }

    if output.is_empty() {
        Ok("No beans found.".to_string())
    } else {
        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Helper Functions
// ---------------------------------------------------------------------------

/// Check if all children of a parent bean are closed.
fn all_children_closed(beans_dir: &Path, parent_id: &str) -> Result<bool> {
    let index = Index::load_or_rebuild(beans_dir)?;
    let children: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|e| e.parent.as_deref() == Some(parent_id))
        .collect();

    if children.is_empty() {
        return Ok(false);
    }

    Ok(children.iter().all(|c| c.status == Status::Closed))
}

/// Auto-close a parent bean when all children are closed.
fn auto_close_parent(beans_dir: &Path, parent_id: &str) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, parent_id)?;
    let mut bean = Bean::from_file(&bean_path)?;

    if bean.status == Status::Closed {
        return Ok(());
    }

    let now = Utc::now();
    bean.status = Status::Closed;
    bean.closed_at = Some(now);
    bean.close_reason = Some("All children closed".to_string());
    bean.updated_at = now;
    bean.to_file(&bean_path)?;

    // Archive
    let slug = bean
        .slug
        .clone()
        .unwrap_or_else(|| title_to_slug(&bean.title));
    let ext = bean_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("md");
    let today = chrono::Local::now().naive_local().date();
    let archive_path =
        crate::discovery::archive_path_for_bean(beans_dir, parent_id, &slug, ext, today);
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&bean_path, &archive_path)?;
    bean.is_archived = true;
    bean.to_file(&archive_path)?;

    // Rebuild index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    Ok(())
}

fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Open => "[ ]",
        Status::InProgress => "[-]",
        Status::Closed => "[x]",
    }
}

fn render_subtree(index: &Index, id: &str, prefix: &str, _is_last: bool, output: &mut String) {
    if let Some(entry) = index.beans.iter().find(|e| e.id == id) {
        let icon = status_icon(entry.status);
        output.push_str(&format!(
            "{}{} {} {}\n",
            prefix, icon, entry.id, entry.title
        ));
        render_children(index, id, &format!("{}  ", prefix), output);
    }
}

fn render_children(index: &Index, parent_id: &str, prefix: &str, output: &mut String) {
    let children: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|e| e.parent.as_deref() == Some(parent_id))
        .collect();

    for child in &children {
        let icon = status_icon(child.status);
        output.push_str(&format!(
            "{}{} {} {}\n",
            prefix, icon, child.id, child.title
        ));
        render_children(index, &child.id, &format!("{}  ", prefix), output);
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("...{}", &s[s.len() - max..])
    } else {
        s.to_string()
    }
}
