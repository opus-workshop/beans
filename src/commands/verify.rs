use std::io::Read;
use std::path::Path;
use std::process::{Command as ShellCommand, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::bean::Bean;
use crate::config::Config;
use crate::discovery::find_bean_file;
use crate::output::Output;

/// Run the verify command for a bean without closing it.
///
/// Returns `Ok(true)` if the command exits 0, `Ok(false)` if non-zero or timed out.
/// If no verify command is set, prints a message and returns `Ok(true)`.
/// Respects `verify_timeout` from the bean or project config.
pub fn cmd_verify(beans_dir: &Path, id: &str, out: &Output) -> Result<bool> {
    let bean_path = find_bean_file(beans_dir, id).map_err(|_| anyhow!("Bean not found: {}", id))?;

    let bean =
        Bean::from_file(&bean_path).with_context(|| format!("Failed to load bean: {}", id))?;

    let verify_cmd = match &bean.verify {
        Some(cmd) => cmd.clone(),
        None => {
            out.info(&format!("no verify command set for bean {}", id));
            return Ok(true);
        }
    };

    // Determine effective timeout: bean overrides config.
    let config = Config::load(beans_dir).ok();
    let timeout_secs =
        bean.effective_verify_timeout(config.as_ref().and_then(|c| c.verify_timeout));

    // Run in the project root (parent of .beans/)
    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    out.info(&format!("Running: {}", verify_cmd));
    if let Some(secs) = timeout_secs {
        out.info(&format!("Timeout: {}s", secs));
    }

    let mut child = ShellCommand::new("sh")
        .args(["-c", &verify_cmd])
        .current_dir(project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn verify command: {}", verify_cmd))?;

    // Drain output in background threads to prevent pipe deadlock.
    let stdout_thread = {
        let stdout = child.stdout.take().expect("stdout is piped");
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = std::io::BufReader::new(stdout);
            let _ = reader.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).to_string()
        })
    };
    let stderr_thread = {
        let stderr = child.stderr.take().expect("stderr is piped");
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = std::io::BufReader::new(stderr);
            let _ = reader.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).to_string()
        })
    };

    let timeout = timeout_secs.map(Duration::from_secs);
    let start = Instant::now();

    let (timed_out, exit_status) = loop {
        match child
            .try_wait()
            .with_context(|| "Failed to poll verify process")?
        {
            Some(status) => break (false, Some(status)),
            None => {
                if let Some(limit) = timeout {
                    if start.elapsed() >= limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        break (true, None);
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout_str = stdout_thread.join().unwrap_or_default();
    let stderr_str = stderr_thread.join().unwrap_or_default();

    // Print captured subprocess output so the user can see what happened.
    // These relay raw process output and bypass the Output abstraction.
    if !stdout_str.trim().is_empty() {
        print!("{}", stdout_str);
    }
    if !stderr_str.trim().is_empty() {
        eprint!("{}", stderr_str);
    }

    if timed_out {
        out.warn(&format!(
            "Verify timed out after {}s for bean {}",
            timeout_secs.unwrap_or(0),
            id
        ));
        return Ok(false);
    }

    let status = exit_status.expect("exit_status is Some when not timed_out");
    if status.success() {
        out.success(id, "Verify passed");
        Ok(true)
    } else {
        out.error(&format!("Verify failed for bean {}", id));
        Ok(false)
    }
}
