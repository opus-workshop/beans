//! Watch daemon for beans — monitors `.beans/` and spawns agents for ready beans.
//!
//! Lifecycle:
//! - `start_daemon(beans_dir, config, foreground)` — writes PID file, runs main loop
//! - `stop_daemon()` — reads PID file, sends SIGTERM, waits, cleans up
//! - `is_daemon_running()` — checks PID file and whether process is alive

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::bean::Status;
use crate::config::Config;
use crate::index::Index;

/// Directory for daemon state files (PID, logs).
fn state_dir() -> Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("beans");
    fs::create_dir_all(&dir).context("Failed to create beans state directory")?;
    Ok(dir)
}

/// Path to the PID file.
fn pid_file_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("daemon.pid"))
}

/// Check if the daemon is currently running.
pub fn is_daemon_running() -> Result<bool> {
    let pid_path = pid_file_path()?;
    if !pid_path.exists() {
        return Ok(false);
    }

    let contents = fs::read_to_string(&pid_path).unwrap_or_default();
    let pid: i32 = match contents.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            // Corrupt PID file — clean up
            let _ = fs::remove_file(&pid_path);
            return Ok(false);
        }
    };

    Ok(process_alive(pid))
}

/// Check if a process with the given PID is alive.
fn process_alive(pid: i32) -> bool {
    // kill(pid, 0) checks if the process exists without sending a signal
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Write the current process's PID to the PID file.
fn write_pid_file() -> Result<()> {
    let pid_path = pid_file_path()?;
    let mut f = fs::File::create(&pid_path)
        .with_context(|| format!("Failed to create PID file: {}", pid_path.display()))?;
    write!(f, "{}", std::process::id())?;
    Ok(())
}

/// Remove the PID file.
fn remove_pid_file() -> Result<()> {
    let pid_path = pid_file_path()?;
    if pid_path.exists() {
        fs::remove_file(&pid_path).context("Failed to remove PID file")?;
    }
    Ok(())
}

/// Stop a running daemon by sending SIGTERM and waiting.
pub fn stop_daemon() -> Result<()> {
    let pid_path = pid_file_path()?;
    if !pid_path.exists() {
        anyhow::bail!("No daemon running (PID file not found)");
    }

    let contents = fs::read_to_string(&pid_path)?;
    let pid: i32 = contents
        .trim()
        .parse()
        .context("Invalid PID in daemon.pid")?;

    if !process_alive(pid) {
        remove_pid_file()?;
        eprintln!("✓ Daemon was not running (stale PID file cleaned up)");
        return Ok(());
    }

    // Send SIGTERM
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait up to 5 seconds for the process to exit
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if !process_alive(pid) {
            break;
        }
        if Instant::now() > deadline {
            // Force kill
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    remove_pid_file()?;
    eprintln!("✓ Daemon stopped");
    Ok(())
}

/// Start the daemon. If `foreground` is false, daemonizes into the background.
pub fn start_daemon(beans_dir: &Path, foreground: bool) -> Result<()> {
    if is_daemon_running()? {
        anyhow::bail!("Daemon is already running. Use `bn run --stop` to stop it first.");
    }

    let beans_dir = beans_dir.to_path_buf();

    if foreground {
        write_pid_file()?;
        let result = run_main_loop(&beans_dir);
        remove_pid_file()?;
        result
    } else {
        // Daemonize: fork into background
        let stdout = state_dir()?.join("daemon.log");
        let stderr = state_dir()?.join("daemon.log");

        let daemonize = daemonize::Daemonize::new()
            .working_directory(".")
            .stdout(fs::File::create(&stdout)?)
            .stderr(fs::File::create(&stderr)?);

        match daemonize.start() {
            Ok(()) => {
                // We're now in the child process
                write_pid_file()?;
                let result = run_main_loop(&beans_dir);
                remove_pid_file()?;
                result
            }
            Err(e) => anyhow::bail!("Failed to daemonize: {}", e),
        }
    }
}

/// Print the daemon PID for `--watch` (non-foreground) feedback.
/// Called from the parent process before daemonizing.
pub fn print_started_message() {
    if let Ok(pid_path) = pid_file_path() {
        // Wait briefly for the PID file to appear
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(contents) = fs::read_to_string(&pid_path) {
                if let Ok(pid) = contents.trim().parse::<i32>() {
                    if process_alive(pid) {
                        eprintln!("✓ Daemon started (pid {}, watching .beans/)", pid);
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    eprintln!("✓ Daemon started (watching .beans/)");
}

/// The main daemon loop: poll for ready beans, spawn agents, repeat.
fn run_main_loop(beans_dir: &Path) -> Result<()> {
    let config = Config::load_with_extends(beans_dir)?;
    let poll_interval = Duration::from_secs(config.poll_interval as u64);
    let max_concurrent = config.max_concurrent;

    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    signal_hook::flag::register(signal_hook::consts::SIGTERM, r.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, r)?;

    // Set up file watcher for immediate reaction to changes
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();
    let mut _watcher = setup_file_watcher(beans_dir, notify_tx)?;

    eprintln!(
        "Watching .beans/ (poll_interval={}s, max_concurrent={})",
        config.poll_interval, max_concurrent
    );

    // Track spawned agents by bean ID → child process
    let mut agents: Vec<(String, std::process::Child)> = Vec::new();

    // Run initial poll immediately
    poll_and_spawn(beans_dir, &config, max_concurrent, &mut agents)?;

    while running.load(Ordering::SeqCst) {
        // Wait for either a file change or the poll interval
        let wait_result = notify_rx.recv_timeout(poll_interval);

        if !running.load(Ordering::SeqCst) {
            break;
        }

        // Reap completed agents
        reap_agents(&mut agents);

        match wait_result {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                poll_and_spawn(beans_dir, &config, max_concurrent, &mut agents)?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("File watcher disconnected, falling back to polling");
                // Re-create watcher
                let (tx, rx) = std::sync::mpsc::channel();
                _watcher = setup_file_watcher(beans_dir, tx)?;
                let _ = rx; // suppress unused warning — we'll rebind on next iteration
            }
        }
    }

    // Graceful shutdown: kill remaining agents
    eprintln!("Stopping...");
    for (id, mut child) in agents {
        eprintln!("  Killing agent for {}", id);
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}

/// Set up a file watcher on the beans directory.
/// Sends `()` on the channel whenever a relevant file changes.
fn setup_file_watcher(
    beans_dir: &Path,
    tx: std::sync::mpsc::Sender<()>,
) -> Result<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res {
            // Only trigger on yaml/md file changes (not index.json, not tmp files)
            let dominated_by_beans = event.paths.iter().any(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                ext == "yaml" || ext == "yml" || ext == "md"
            });
            if dominated_by_beans {
                let _ = tx.send(());
            }
        }
    })?;

    watcher.watch(beans_dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// Poll for ready beans and spawn agents up to max_concurrent.
fn poll_and_spawn(
    beans_dir: &Path,
    config: &Config,
    max_concurrent: u32,
    agents: &mut Vec<(String, std::process::Child)>,
) -> Result<()> {
    // Reap completed agents first
    reap_agents(agents);

    let run_template = match &config.run {
        Some(t) => t.clone(),
        None => return Ok(()), // No run command configured — nothing to spawn
    };

    let active_count = agents.len() as u32;
    if active_count >= max_concurrent {
        return Ok(());
    }

    let slots = max_concurrent - active_count;

    // Rebuild index to get fresh state
    let index = Index::load_or_rebuild(beans_dir)?;

    // Collect IDs of beans we're already running agents for
    let running_ids: HashSet<&str> = agents.iter().map(|(id, _)| id.as_str()).collect();

    // Find ready beans (open, has verify, deps resolved) that aren't already claimed/running
    let mut ready: Vec<_> = index
        .beans
        .iter()
        .filter(|e| {
            e.has_verify
                && e.status == Status::Open
                && !running_ids.contains(e.id.as_str())
                && all_deps_closed(e, &index)
        })
        .collect();

    // Sort by priority (P0 first), then ID
    ready.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| crate::util::natural_cmp(&a.id, &b.id))
    });

    for entry in ready.into_iter().take(slots as usize) {
        let cmd = run_template.replace("{id}", &entry.id);
        let now = chrono::Local::now().format("%H:%M:%S");
        eprintln!(
            "[{}] Spawning agent for {} ({})",
            now, entry.id, entry.title
        );

        match std::process::Command::new("sh")
            .args(["-c", &cmd])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                agents.push((entry.id.clone(), child));
            }
            Err(e) => {
                eprintln!("[{}] Failed to spawn agent for {}: {}", now, entry.id, e);
            }
        }
    }

    Ok(())
}

/// Check if all dependencies of a bean are closed.
fn all_deps_closed(entry: &crate::index::IndexEntry, index: &Index) -> bool {
    // Explicit dependencies
    for dep_id in &entry.dependencies {
        match index.beans.iter().find(|e| e.id == *dep_id) {
            Some(dep) if dep.status == Status::Closed => {}
            _ => return false,
        }
    }

    // Smart dependencies (produces/requires among siblings)
    for required in &entry.requires {
        if let Some(producer) = index
            .beans
            .iter()
            .find(|e| e.id != entry.id && e.parent == entry.parent && e.produces.contains(required))
        {
            if producer.status != Status::Closed {
                return false;
            }
        }
    }

    true
}

/// Reap completed child processes and log their completion.
fn reap_agents(agents: &mut Vec<(String, std::process::Child)>) {
    agents.retain_mut(|(id, child)| {
        match child.try_wait() {
            Ok(Some(status)) => {
                let now = chrono::Local::now().format("%H:%M:%S");
                if status.success() {
                    eprintln!("[{}] Agent {} completed", now, id);
                } else {
                    eprintln!(
                        "[{}] Agent {} exited with code {}",
                        now,
                        id,
                        status.code().unwrap_or(-1)
                    );
                }
                false // remove from list
            }
            Ok(None) => true, // still running
            Err(_) => false,  // error checking — remove
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_daemon_running_returns_false_when_no_pid_file() {
        // Clean up any existing PID file for this test
        if let Ok(path) = pid_file_path() {
            let _ = fs::remove_file(&path);
        }
        assert!(!is_daemon_running().unwrap());
    }

    #[test]
    fn pid_file_write_read_roundtrip() {
        write_pid_file().unwrap();
        let path = pid_file_path().unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        let pid: u32 = contents.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
        remove_pid_file().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn process_alive_returns_true_for_current_process() {
        assert!(process_alive(std::process::id() as i32));
    }

    #[test]
    fn process_alive_returns_false_for_nonexistent_pid() {
        // PID 99999999 is extremely unlikely to exist
        assert!(!process_alive(99_999_999));
    }

    #[test]
    fn state_dir_is_created() {
        let dir = state_dir().unwrap();
        assert!(dir.exists());
    }
}
