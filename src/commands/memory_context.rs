use std::path::Path;

use anyhow::Result;
use chrono::{Duration, Utc};

use crate::bean::{AttemptOutcome, Bean, Status};
use crate::discovery::{find_archived_bean, find_bean_file};
use crate::index::Index;
use crate::relevance::relevance_score;

/// Default token budget for context output (~4000 tokens ≈ ~16000 chars).
const DEFAULT_MAX_CHARS: usize = 16000;

/// Output memory context for session-start injection.
///
/// When `bn context` is called without a bean ID, it returns relevant memories:
/// 1. WARNINGS — stale facts, past failures (never truncated)
/// 2. WORKING ON — claimed beans with attempt history
/// 3. RELEVANT FACTS — scored by path overlap, dependencies
/// 4. RECENT WORK — closed beans from last 7 days
pub fn cmd_memory_context(beans_dir: &Path, json: bool) -> Result<()> {
    let now = Utc::now();
    let index = Index::load_or_rebuild(beans_dir)?;
    let archived = Index::collect_archived(beans_dir).unwrap_or_default();

    // Collect working paths and deps from claimed beans for relevance scoring
    let mut working_paths: Vec<String> = Vec::new();
    let mut working_deps: Vec<String> = Vec::new();

    // =========================================================================
    // Section 1: WARNINGS (stale facts, failing facts)
    // =========================================================================
    let mut warnings: Vec<String> = Vec::new();

    // =========================================================================
    // Section 2: WORKING ON (claimed beans with attempt history)
    // =========================================================================
    let mut working_on: Vec<String> = Vec::new();

    for entry in &index.beans {
        if entry.status != Status::InProgress {
            continue;
        }

        let bean_path = match find_bean_file(beans_dir, &entry.id) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        // Collect paths and deps for relevance scoring
        working_paths.extend(bean.paths.clone());
        working_deps.extend(bean.requires.clone());
        working_deps.extend(bean.produces.clone());

        let mut line = format!("[{}] {}", bean.id, bean.title);

        // Show attempt history
        let failed_attempts: Vec<_> = bean
            .attempt_log
            .iter()
            .filter(|a| a.outcome == AttemptOutcome::Failed)
            .collect();

        if !failed_attempts.is_empty() {
            line.push_str(&format!(
                "\n│   Attempt #{} (previous failures: {})",
                failed_attempts.len() + 1,
                failed_attempts.len()
            ));
            // Show last failure notes
            if let Some(last) = failed_attempts.last() {
                if let Some(ref notes) = last.notes {
                    let preview: String = notes.chars().take(100).collect();
                    line.push_str(&format!("\n│   Last failure: {}", preview));

                    // Add to warnings
                    warnings.push(format!(
                        "PAST FAILURE [{}]: \"{}\"",
                        bean.id,
                        notes.chars().take(80).collect::<String>()
                    ));
                }
            }
        }

        working_on.push(line);
    }

    // Check all facts for staleness
    for entry in index.beans.iter().chain(archived.iter()) {
        let bean_path = match find_bean_file(beans_dir, &entry.id)
            .or_else(|_| find_archived_bean(beans_dir, &entry.id))
        {
            Ok(p) => p,
            Err(_) => continue,
        };

        let bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bean.bean_type != "fact" {
            continue;
        }

        // Check staleness
        if let Some(stale_after) = bean.stale_after {
            if now > stale_after {
                let days_stale = (now - stale_after).num_days();
                warnings.push(format!(
                    "STALE: \"{}\" — not verified in {}d",
                    bean.title, days_stale
                ));
            }
        }
    }

    // =========================================================================
    // Section 3: RELEVANT FACTS (scored by path overlap, dependencies)
    // =========================================================================
    let mut relevant_facts: Vec<(Bean, u32)> = Vec::new();

    for entry in index.beans.iter().chain(archived.iter()) {
        let bean_path = match find_bean_file(beans_dir, &entry.id)
            .or_else(|_| find_archived_bean(beans_dir, &entry.id))
        {
            Ok(p) => p,
            Err(_) => continue,
        };

        let bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bean.bean_type != "fact" {
            continue;
        }

        let score = relevance_score(&bean, &working_paths, &working_deps);
        if score > 0 {
            relevant_facts.push((bean, score));
        }
    }

    relevant_facts.sort_by(|a, b| b.1.cmp(&a.1));

    // =========================================================================
    // Section 4: RECENT WORK (closed beans from last 7 days)
    // =========================================================================
    let mut recent_work: Vec<Bean> = Vec::new();
    let seven_days_ago = now - Duration::days(7);

    for entry in &archived {
        if entry.status != Status::Closed {
            continue;
        }

        let bean_path = match find_archived_bean(beans_dir, &entry.id) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bean.bean_type == "fact" {
            continue; // facts shown separately
        }

        if let Some(closed_at) = bean.closed_at {
            if closed_at > seven_days_ago {
                recent_work.push(bean);
            }
        }
    }

    recent_work.sort_by(|a, b| b.closed_at.unwrap_or(now).cmp(&a.closed_at.unwrap_or(now)));

    // =========================================================================
    // Output
    // =========================================================================

    if json {
        let output = serde_json::json!({
            "warnings": warnings,
            "working_on": working_on.iter().map(|w| {
                // Parse out the bean ID for structured output
                w.split(']').next().unwrap_or("").trim_start_matches('[').to_string()
            }).collect::<Vec<_>>(),
            "relevant_facts": relevant_facts.iter().map(|(b, s)| {
                serde_json::json!({
                    "id": b.id,
                    "title": b.title,
                    "score": s,
                    "verified": b.last_verified,
                })
            }).collect::<Vec<_>>(),
            "recent_work": recent_work.iter().map(|b| {
                serde_json::json!({
                    "id": b.id,
                    "title": b.title,
                    "closed_at": b.closed_at,
                    "close_reason": b.close_reason,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Check if there's any content to show
    let has_content = !warnings.is_empty()
        || !working_on.is_empty()
        || !relevant_facts.is_empty()
        || !recent_work.is_empty();

    if !has_content {
        println!("No memory context available.");
        return Ok(());
    }

    let mut output = String::new();
    #[allow(unused_assignments)]
    let mut chars_used = 0;

    output.push_str("═══ BEANS CONTEXT ═══════════════════════════════════════════\n\n");

    // Warnings (never truncated)
    if !warnings.is_empty() {
        output.push_str("⚠ WARNINGS\n");
        for w in &warnings {
            output.push_str(&format!("│ {}\n", w));
        }
        output.push('\n');
    }

    // Working on
    if !working_on.is_empty() {
        output.push_str("► WORKING ON\n");
        for w in &working_on {
            output.push_str(&format!("│ {}\n", w));
        }
        output.push('\n');
    }

    chars_used = output.len();

    // Relevant facts (truncate if over budget)
    if !relevant_facts.is_empty() && chars_used < DEFAULT_MAX_CHARS {
        output.push_str("✓ RELEVANT FACTS\n");
        for (bean, _score) in &relevant_facts {
            if chars_used > DEFAULT_MAX_CHARS {
                break;
            }
            let verified_ago = bean
                .last_verified
                .map(|lv| {
                    let ago = now - lv;
                    if ago.num_days() > 0 {
                        format!("✓ {}d ago", ago.num_days())
                    } else if ago.num_hours() > 0 {
                        format!("✓ {}h ago", ago.num_hours())
                    } else {
                        "✓ just now".to_string()
                    }
                })
                .unwrap_or_else(|| "unverified".to_string());

            let line = format!("│ \"{}\" {}\n", bean.title, verified_ago);
            chars_used += line.len();
            output.push_str(&line);
        }
        output.push('\n');
    }

    // Recent work (truncate from bottom first)
    if !recent_work.is_empty() && chars_used < DEFAULT_MAX_CHARS {
        output.push_str("◷ RECENT WORK\n");
        for bean in &recent_work {
            if chars_used > DEFAULT_MAX_CHARS {
                break;
            }
            let closed_ago = bean
                .closed_at
                .map(|ca| {
                    let ago = now - ca;
                    if ago.num_days() > 0 {
                        format!("{}d ago", ago.num_days())
                    } else if ago.num_hours() > 0 {
                        format!("{}h ago", ago.num_hours())
                    } else {
                        "just now".to_string()
                    }
                })
                .unwrap_or_else(|| "recently".to_string());

            let mut line = format!("│ [{}] {} (closed {})\n", bean.id, bean.title, closed_ago);

            if let Some(ref reason) = bean.close_reason {
                line.push_str(&format!(
                    "│   \"{}\"\n",
                    reason.chars().take(80).collect::<String>()
                ));
            }

            chars_used += line.len();
            output.push_str(&line);
        }
        output.push('\n');
    }

    print!("{}", output);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_beans_dir_with_config() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = crate::config::Config {
            project: "test".to_string(),
            next_id: 10,
            auto_close_parent: true,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
            on_close: None,
            on_fail: None,
            post_plan: None,
            verify_timeout: None,
            review: None,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn memory_context_empty() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Should not error with no beans
        let result = cmd_memory_context(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn memory_context_shows_claimed_beans() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create a claimed bean
        let mut bean = Bean::new("1", "Working on auth");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = crate::util::title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        let result = cmd_memory_context(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn memory_context_shows_stale_facts() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create a stale fact
        let mut bean = Bean::new("1", "Auth uses RS256");
        bean.bean_type = "fact".to_string();
        bean.stale_after = Some(Utc::now() - Duration::days(5)); // 5 days past stale
        bean.verify = Some("true".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        let result = cmd_memory_context(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn memory_context_json_output() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let result = cmd_memory_context(&beans_dir, true);
        assert!(result.is_ok());
    }
}
