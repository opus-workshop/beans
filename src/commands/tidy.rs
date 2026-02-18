use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::bean::{Bean, Status};
use crate::discovery::{archive_path_for_bean, find_bean_file};
use crate::index::Index;
use crate::util::title_to_slug;

/// A record of one bean that was (or would be) archived during tidy.
/// We collect these so we can print a summary at the end.
struct TidiedBean {
    id: String,
    title: String,
    archive_path: String,
}

/// A record of one bean that was (or would be) released during tidy.
struct ReleasedBean {
    id: String,
    title: String,
    reason: String,
}

/// Format a chrono Duration as a human-readable string like "3 days ago"
/// or "2 hours ago".
fn format_duration(duration: chrono::Duration) -> String {
    let secs = duration.num_seconds();
    if secs < 0 {
        return "just now".to_string();
    }
    let minutes = secs / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("claimed {} day(s) ago", days)
    } else if hours > 0 {
        format!("claimed {} hour(s) ago", hours)
    } else if minutes > 0 {
        format!("claimed {} minute(s) ago", minutes)
    } else {
        "claimed just now".to_string()
    }
}

/// Check if any agent processes are currently running.
///
/// Looks for `pi` processes that might be working on beans. This is used
/// to avoid releasing in-progress beans that are actively being worked on.
/// Returns true if we detect running agent processes.
fn has_running_agents() -> bool {
    // Check for running `pi` processes (the coding agent that works on beans).
    // We look for `pi` processes that are NOT the current process.
    let current_pid = std::process::id();

    // Try pgrep first (macOS/Linux)
    if let Ok(output) = std::process::Command::new("pgrep")
        .args(["-f", "pi -p beans"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    if pid != current_pid {
                        return true;
                    }
                }
            }
        }
    }

    // Also check for deli spawn processes
    if let Ok(output) = std::process::Command::new("pgrep")
        .args(["-f", "deli spawn"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    if pid != current_pid {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Tidy the beans directory: archive closed beans, release stale in-progress
/// beans, and rebuild the index.
///
/// Delegates to `cmd_tidy_inner` with the real agent-detection function.
pub fn cmd_tidy(beans_dir: &Path, dry_run: bool) -> Result<()> {
    cmd_tidy_inner(beans_dir, dry_run, has_running_agents)
}

/// Inner implementation of tidy, with an injectable agent-check function
/// for testability.
///
/// This is a housekeeping command that catches state inconsistencies:
///
/// - **Closed beans not archived:** beans whose status was set to "closed"
///   via `bn update --status closed` (which bypasses the close command's
///   archiving logic), beans closed before archiving was added, or files
///   edited by hand.
///
/// - **Stale in-progress beans:** beans whose status is "in_progress" but
///   no agent is actually working on them. This happens when an agent
///   crashes without releasing its claim, when `deli spawn` is killed, or
///   when files are edited by hand. These are released back to "open".
///
/// The steps are:
/// 1. Build a fresh index from disk so we see every bean, even if the
///    cached index is stale.
/// 2. Walk through the index looking for beans with status == Closed
///    that are still sitting in the main .beans/ directory (is_archived
///    is false).
/// 3. For each one, compute its archive path (using closed_at if available,
///    otherwise today's date) and move it there.
/// 4. Check for in-progress beans that appear stale (no running agent
///    processes detected) and release them back to open.
/// 5. Rebuild and save the index one final time so it reflects the new
///    state.
///
/// With `dry_run = true` we report what would change without touching
/// any files.
fn cmd_tidy_inner(beans_dir: &Path, dry_run: bool, check_agents: fn() -> bool) -> Result<()> {
    // Step 1 — Build a fresh index so we're working from the truth on disk,
    // not a potentially stale cache.
    let index = Index::build(beans_dir)
        .context("Failed to build index")?;

    // Step 2 — Find every closed bean that's still in the main directory.
    // We filter on two things:
    //   • status == Closed  (the bean is done)
    //   • find_bean_file succeeds (the file is still in .beans/, not archive/)
    //
    // We also skip beans that have open children — archiving them would
    // orphan the children's parent reference without the parent being
    // findable in the main directory.
    let closed: Vec<&crate::index::IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| entry.status == Status::Closed)
        .collect();

    let mut tidied: Vec<TidiedBean> = Vec::new();
    let mut skipped_parent_ids: Vec<String> = Vec::new();

    for entry in &closed {
        // Double-check the file actually exists in the main directory.
        // If find_bean_file fails, it's either already archived or
        // something weird — either way, nothing for us to do.
        let bean_path = match find_bean_file(beans_dir, &entry.id) {
            Ok(path) => path,
            Err(_) => continue,
        };

        // Load the full bean so we can read closed_at, slug, etc.
        let mut bean = Bean::from_file(&bean_path)
            .with_context(|| format!("Failed to load bean: {}", entry.id))?;

        // Safety check: if this bean is already marked archived, skip it.
        // (Shouldn't happen since it's in the main dir, but be defensive.)
        if bean.is_archived {
            continue;
        }

        // Guard: don't archive a parent whose children are still open.
        // We check by looking for any bean in the index that lists this
        // bean as its parent and isn't closed yet.
        let has_open_children = index.beans.iter().any(|b| {
            b.parent.as_deref() == Some(entry.id.as_str())
                && b.status != Status::Closed
        });

        if has_open_children {
            skipped_parent_ids.push(entry.id.clone());
            continue;
        }

        // Pick the date for the archive subdirectory.
        // Prefer closed_at (when the bean was actually finished) because
        // that groups archived beans by *completion* month.  Fall back to
        // updated_at (always present) if closed_at was never set — this
        // happens for beans that were closed via `bn update --status closed`
        // which doesn't set closed_at.
        let archive_date = bean
            .closed_at
            .unwrap_or(bean.updated_at)
            .with_timezone(&chrono::Local)
            .date_naive();

        // Build the target path under .beans/archive/YYYY/MM/<id>-<slug>.<ext>
        let slug = bean
            .slug
            .clone()
            .unwrap_or_else(|| title_to_slug(&bean.title));
        let ext = bean_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("md");
        let archive_path =
            archive_path_for_bean(beans_dir, &entry.id, &slug, ext, archive_date);

        // Record what we're about to do (for the summary).
        // We store the archive path relative to .beans/ to keep output tidy.
        let relative = archive_path
            .strip_prefix(beans_dir)
            .unwrap_or(&archive_path);
        tidied.push(TidiedBean {
            id: entry.id.clone(),
            title: entry.title.clone(),
            archive_path: relative.display().to_string(),
        });

        // In dry-run mode we stop here — no file moves.
        if dry_run {
            continue;
        }

        // Step 3 — Actually move the bean.
        // Create the archive directory tree (archive/YYYY/MM) if needed.
        if let Some(parent) = archive_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!(
                    "Failed to create archive directory for bean {}",
                    entry.id
                ))?;
        }

        // Move the file from .beans/<id>-<slug>.md → .beans/archive/YYYY/MM/…
        std::fs::rename(&bean_path, &archive_path)
            .with_context(|| format!(
                "Failed to move bean {} to archive",
                entry.id
            ))?;

        // Mark the bean as archived and persist. This sets is_archived = true
        // in the YAML front-matter so other commands (unarchive, list --all)
        // know this bean lives in the archive.
        bean.is_archived = true;
        bean.to_file(&archive_path)
            .with_context(|| format!(
                "Failed to save archived bean: {}",
                entry.id
            ))?;
    }

    // Step 4 — Release stale in-progress beans.
    //
    // An in-progress bean is "stale" if no agent process is currently
    // running that could be working on it. We check for running `pi`
    // and `deli spawn` processes. If none are found, all in-progress
    // beans are considered stale and released back to open.
    //
    // If agents ARE running, we skip this step entirely because we
    // can't reliably determine which beans they're working on.
    let in_progress: Vec<&crate::index::IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| entry.status == Status::InProgress)
        .collect();

    let mut released: Vec<ReleasedBean> = Vec::new();

    if !in_progress.is_empty() {
        let agents_running = check_agents();

        if agents_running {
            // Agents are running — we can't safely release in-progress
            // beans because one of them might be actively being worked on.
            // Just report them.
            eprintln!(
                "Note: {} in-progress bean(s) found, but agent processes are running — skipping release.",
                in_progress.len()
            );
        } else {
            // No agents running — all in-progress beans are stale.
            for entry in &in_progress {
                let bean_path = match find_bean_file(beans_dir, &entry.id) {
                    Ok(path) => path,
                    Err(_) => continue,
                };

                let mut bean = match Bean::from_file(&bean_path) {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                // Build a human-readable reason for the release.
                let reason = if let Some(claimed_at) = bean.claimed_at {
                    let age = Utc::now().signed_duration_since(claimed_at);
                    format_duration(age)
                } else {
                    "never properly claimed".to_string()
                };

                released.push(ReleasedBean {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    reason,
                });

                if dry_run {
                    continue;
                }

                // Release the bean: set status to Open, clear claim fields.
                let now = Utc::now();
                bean.status = Status::Open;
                bean.claimed_by = None;
                bean.claimed_at = None;
                bean.updated_at = now;

                bean.to_file(&bean_path)
                    .with_context(|| format!(
                        "Failed to release stale bean: {}",
                        entry.id
                    ))?;
            }
        }
    }

    // Step 5 — Rebuild the index one final time.
    // After moving files around and releasing stale beans, the old index
    // is stale, so we rebuild from disk. In dry-run mode nothing changed,
    // but we still rebuild because the user asked to "update the index."
    let final_index = Index::build(beans_dir)
        .context("Failed to rebuild index after tidy")?;
    final_index
        .save(beans_dir)
        .context("Failed to save index")?;

    // ── Print results ────────────────────────────────────────────────

    let archive_verb = if dry_run { "Would archive" } else { "Archived" };
    let release_verb = if dry_run { "Would release" } else { "Released" };

    if tidied.is_empty() && skipped_parent_ids.is_empty() && released.is_empty() {
        println!("Nothing to tidy — all beans look good.");
    }

    if !tidied.is_empty() {
        println!("{} {} bean(s):", archive_verb, tidied.len());
        for t in &tidied {
            println!("  → {}. {} → {}", t.id, t.title, t.archive_path);
        }
    }

    if !released.is_empty() {
        println!("{} {} stale in-progress bean(s):", release_verb, released.len());
        for r in &released {
            println!("  → {}. {} ({})", r.id, r.title, r.reason);
        }
    }

    if !skipped_parent_ids.is_empty() {
        println!(
            "Skipped {} closed parent(s) with open children: {}",
            skipped_parent_ids.len(),
            skipped_parent_ids.join(", ")
        );
    }

    println!(
        "Index rebuilt: {} bean(s) indexed.",
        final_index.beans.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use crate::util::title_to_slug;
    use std::fs;
    use tempfile::TempDir;

    /// Create a .beans/ directory and return (TempDir guard, path).
    fn setup() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    /// Mock: no agents running (for testing stale-release behavior).
    fn no_agents() -> bool { false }

    /// Mock: agents are running (for testing skip behavior).
    fn agents_running() -> bool { true }

    /// Helper: write a bean to the main .beans/ directory.
    fn write_bean(beans_dir: &Path, bean: &Bean) {
        let slug = title_to_slug(&bean.title);
        let path = beans_dir.join(format!("{}-{}.md", bean.id, slug));
        bean.to_file(path).unwrap();
    }

    // ── Basic behaviour ────────────────────────────────────────────

    #[test]
    fn tidy_archives_closed_beans() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "Done task");
        bean.status = Status::Closed;
        bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Should no longer be in main directory
        assert!(find_bean_file(&beans_dir, "1").is_err());
        // Should be in archive
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived.is_ok());
        let archived_bean = Bean::from_file(&archived.unwrap()).unwrap();
        assert!(archived_bean.is_archived);
    }

    #[test]
    fn tidy_leaves_open_beans_alone() {
        let (_dir, beans_dir) = setup();

        let bean = Bean::new("1", "Open task");
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Should still be in main directory
        assert!(find_bean_file(&beans_dir, "1").is_ok());
    }

    #[test]
    fn tidy_idempotent() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "Done task");
        bean.status = Status::Closed;
        bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        // First tidy archives it
        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();
        // Second tidy should be a no-op (no panic, no error)
        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived.is_ok());
    }

    // ── Dry-run ────────────────────────────────────────────────────

    #[test]
    fn tidy_dry_run_does_not_move_files() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "Done task");
        bean.status = Status::Closed;
        bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, true, no_agents).unwrap();

        // File should still be in main directory (dry-run)
        assert!(find_bean_file(&beans_dir, "1").is_ok());
    }

    // ── Skips parents with open children ───────────────────────────

    #[test]
    fn tidy_skips_closed_parent_with_open_children() {
        let (_dir, beans_dir) = setup();

        // Parent is closed
        let mut parent = Bean::new("1", "Parent");
        parent.status = Status::Closed;
        parent.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &parent);

        // Child is still open
        let mut child = Bean::new("1.1", "Child");
        child.parent = Some("1".to_string());
        write_bean(&beans_dir, &child);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Parent should NOT be archived because child is still open
        assert!(find_bean_file(&beans_dir, "1").is_ok());
        // Child should still be in main dir
        assert!(find_bean_file(&beans_dir, "1.1").is_ok());
    }

    #[test]
    fn tidy_archives_parent_when_all_children_closed() {
        let (_dir, beans_dir) = setup();

        // Parent is closed
        let mut parent = Bean::new("1", "Parent");
        parent.status = Status::Closed;
        parent.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &parent);

        // Child is also closed
        let mut child = Bean::new("1.1", "Child");
        child.parent = Some("1".to_string());
        child.status = Status::Closed;
        child.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &child);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Both should be archived
        assert!(find_bean_file(&beans_dir, "1").is_err());
        assert!(find_bean_file(&beans_dir, "1.1").is_err());
        assert!(crate::discovery::find_archived_bean(&beans_dir, "1").is_ok());
        assert!(crate::discovery::find_archived_bean(&beans_dir, "1.1").is_ok());
    }

    // ── Uses closed_at for archive path ────────────────────────────

    #[test]
    fn tidy_uses_closed_at_for_archive_date() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "January task");
        bean.status = Status::Closed;
        // Force a specific closed_at date
        bean.closed_at = Some(
            chrono::DateTime::parse_from_rfc3339("2025-06-15T12:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        // The archive path should contain 2025/06 (from closed_at)
        let path_str = archived.display().to_string();
        assert!(
            path_str.contains("2025") && path_str.contains("06"),
            "Expected archive under 2025/06, got: {}",
            path_str
        );
    }

    // ── Mixed open and closed ──────────────────────────────────────

    #[test]
    fn tidy_handles_mix_of_open_closed_and_in_progress() {
        let (_dir, beans_dir) = setup();

        let open_bean = Bean::new("1", "Still open");
        write_bean(&beans_dir, &open_bean);

        let mut closed_bean = Bean::new("2", "Already done");
        closed_bean.status = Status::Closed;
        closed_bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &closed_bean);

        let mut in_progress = Bean::new("3", "Working on it");
        in_progress.status = Status::InProgress;
        write_bean(&beans_dir, &in_progress);

        // With no agents running, in_progress beans get released
        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Open bean untouched
        let b1 = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(b1.status, Status::Open);

        // Closed bean archived
        assert!(find_bean_file(&beans_dir, "2").is_err());
        assert!(crate::discovery::find_archived_bean(&beans_dir, "2").is_ok());

        // In-progress bean released (no agents running)
        let b3 = Bean::from_file(find_bean_file(&beans_dir, "3").unwrap()).unwrap();
        assert_eq!(b3.status, Status::Open);
    }

    #[test]
    fn tidy_skips_in_progress_when_agents_running() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "Active WIP");
        bean.status = Status::InProgress;
        bean.claimed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        // With agents running, in_progress beans are NOT released
        cmd_tidy_inner(&beans_dir, false, agents_running).unwrap();

        let updated = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert!(updated.claimed_at.is_some());
    }

    // ── Stale in-progress beans ──────────────────────────────────

    #[test]
    fn tidy_releases_stale_in_progress_beans() {
        let (_dir, beans_dir) = setup();

        // Create an in-progress bean with a stale claim (old claimed_at, no running agent)
        let mut bean = Bean::new("1", "Stale WIP");
        bean.status = Status::InProgress;
        bean.claimed_at = Some(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Bean should be released back to open
        let updated = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert!(updated.claimed_by.is_none());
        assert!(updated.claimed_at.is_none());
    }

    #[test]
    fn tidy_releases_in_progress_bean_without_claimed_at() {
        let (_dir, beans_dir) = setup();

        // Create a bean that was manually set to in_progress without proper claiming
        let mut bean = Bean::new("1", "Manually set WIP");
        bean.status = Status::InProgress;
        // No claimed_at, no claimed_by — definitely stale
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        let updated = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
    }

    #[test]
    fn tidy_dry_run_does_not_release_stale_beans() {
        let (_dir, beans_dir) = setup();

        let mut bean = Bean::new("1", "Stale WIP");
        bean.status = Status::InProgress;
        bean.claimed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, true, no_agents).unwrap();

        // Bean should still be in_progress (dry-run)
        let updated = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn tidy_handles_mix_of_stale_and_closed() {
        let (_dir, beans_dir) = setup();

        // An open bean — untouched
        let open_bean = Bean::new("1", "Open");
        write_bean(&beans_dir, &open_bean);

        // A closed bean — archived
        let mut closed_bean = Bean::new("2", "Closed");
        closed_bean.status = Status::Closed;
        closed_bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &closed_bean);

        // A stale in-progress bean — released
        let mut stale_bean = Bean::new("3", "Stale WIP");
        stale_bean.status = Status::InProgress;
        stale_bean.claimed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &stale_bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Open bean untouched
        let b1 = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(b1.status, Status::Open);

        // Closed bean archived
        assert!(find_bean_file(&beans_dir, "2").is_err());
        assert!(crate::discovery::find_archived_bean(&beans_dir, "2").is_ok());

        // Stale in-progress bean released
        let b3 = Bean::from_file(find_bean_file(&beans_dir, "3").unwrap()).unwrap();
        assert_eq!(b3.status, Status::Open);
        assert!(b3.claimed_at.is_none());
        assert!(b3.claimed_by.is_none());
    }

    #[test]
    fn tidy_releases_in_progress_with_claimed_by() {
        let (_dir, beans_dir) = setup();

        // Bean was claimed by an agent that no longer exists
        let mut bean = Bean::new("1", "Agent crashed");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("agent-42".to_string());
        bean.claimed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        let updated = Bean::from_file(find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert!(updated.claimed_by.is_none());
        assert!(updated.claimed_at.is_none());
    }

    // ── Empty project ──────────────────────────────────────────────

    #[test]
    fn tidy_empty_project() {
        let (_dir, beans_dir) = setup();
        // Should succeed with nothing to do
        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();
    }

    // ── Index is rebuilt ───────────────────────────────────────────

    #[test]
    fn tidy_rebuilds_index() {
        let (_dir, beans_dir) = setup();

        let open_bean = Bean::new("1", "Open");
        write_bean(&beans_dir, &open_bean);

        let mut closed_bean = Bean::new("2", "Closed");
        closed_bean.status = Status::Closed;
        closed_bean.closed_at = Some(chrono::Utc::now());
        write_bean(&beans_dir, &closed_bean);

        cmd_tidy_inner(&beans_dir, false, no_agents).unwrap();

        // Index should only contain the open bean (closed was archived)
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "1");
    }
}
