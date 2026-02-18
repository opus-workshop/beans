use std::path::Path;

use anyhow::{Context, Result};

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

/// Tidy the beans directory: archive all closed beans and rebuild the index.
///
/// This is a housekeeping command that catches closed beans which weren't
/// archived automatically — for example, beans whose status was set to
/// "closed" via `bn update --status closed` (which bypasses the close
/// command's archiving logic), beans closed before archiving was added,
/// or files edited by hand.
///
/// The steps are:
/// 1. Build a fresh index from disk so we see every bean, even if the
///    cached index is stale.
/// 2. Walk through the index looking for beans with status == Closed
///    that are still sitting in the main .beans/ directory (is_archived
///    is false).
/// 3. For each one, compute its archive path (using closed_at if available,
///    otherwise today's date) and move it there.
/// 4. Rebuild and save the index one final time so it reflects the new
///    file locations.
///
/// With `dry_run = true` we do steps 1-2 but only *report* what would
/// move, without touching any files.
pub fn cmd_tidy(beans_dir: &Path, dry_run: bool) -> Result<()> {
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

    // Step 4 — Rebuild the index one final time.
    // After moving files around the old index is stale, so we rebuild
    // from disk. In dry-run mode nothing moved, but we still rebuild
    // because the user asked to "update the index."
    let final_index = Index::build(beans_dir)
        .context("Failed to rebuild index after tidy")?;
    final_index
        .save(beans_dir)
        .context("Failed to save index")?;

    // ── Print results ────────────────────────────────────────────────

    let verb = if dry_run { "Would archive" } else { "Archived" };

    if tidied.is_empty() && skipped_parent_ids.is_empty() {
        println!("Nothing to tidy — no unarchived closed beans found.");
    } else if !tidied.is_empty() {
        println!("{} {} bean(s):", verb, tidied.len());
        for t in &tidied {
            println!("  → {}. {} → {}", t.id, t.title, t.archive_path);
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

        cmd_tidy(&beans_dir, false).unwrap();

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

        cmd_tidy(&beans_dir, false).unwrap();

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
        cmd_tidy(&beans_dir, false).unwrap();
        // Second tidy should be a no-op (no panic, no error)
        cmd_tidy(&beans_dir, false).unwrap();

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

        cmd_tidy(&beans_dir, true).unwrap();

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

        cmd_tidy(&beans_dir, false).unwrap();

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

        cmd_tidy(&beans_dir, false).unwrap();

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

        cmd_tidy(&beans_dir, false).unwrap();

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
    fn tidy_handles_mix_of_open_and_closed() {
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

        cmd_tidy(&beans_dir, false).unwrap();

        // Only the closed bean should be archived
        assert!(find_bean_file(&beans_dir, "1").is_ok());
        assert!(find_bean_file(&beans_dir, "2").is_err());
        assert!(find_bean_file(&beans_dir, "3").is_ok());
        assert!(crate::discovery::find_archived_bean(&beans_dir, "2").is_ok());
    }

    // ── Empty project ──────────────────────────────────────────────

    #[test]
    fn tidy_empty_project() {
        let (_dir, beans_dir) = setup();
        // Should succeed with nothing to do
        cmd_tidy(&beans_dir, false).unwrap();
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

        cmd_tidy(&beans_dir, false).unwrap();

        // Index should only contain the open bean (closed was archived)
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "1");
    }
}
