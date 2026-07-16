//! Post-update service restart and gateway notification hooks.
//!
//! Gateway identity, PID-reuse protection, supervisor detection, Windows
//! planned-stop handling, and graceful drain semantics already live in Python's
//! `gateway.status` / `hermes_cli.gateway`. The updater deliberately invokes
//! that canonical bridge from the newly activated slot instead of duplicating
//! the process-control contract in Rust.

use crate::slots;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn restart_gateway(hermes_home: &Path, version: &str) -> Result<()> {
    let slot = slots::slot_path(hermes_home, version);
    let python = if cfg!(windows) {
        slot.join("runtime/venv/Scripts/python.exe")
    } else {
        slot.join("runtime/venv/bin/python")
    };
    let output = std::process::Command::new(&python)
        .args(["-m", "hermes_cli.update_restart"])
        .current_dir(&slot)
        .env("HERMES_HOME", hermes_home)
        .output()
        .with_context(|| format!("cannot run gateway restart bridge via {}", python.display()))?;
    if !output.status.success() {
        bail!(
            "gateway restart bridge failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let message = String::from_utf8_lossy(&output.stdout);
    if !message.trim().is_empty() {
        println!("  {}", message.trim());
    }
    Ok(())
}

/// Write the notification files for gateway `/update` IPC.
pub fn write_notify_files(
    hermes_home: &Path,
    exit_code: i32,
    message: &str,
    notify_file: Option<&str>,
) -> Result<()> {
    if let Some(path) = notify_file {
        std::fs::write(path, format!("{}\n", exit_code))?;
        return Ok(());
    }

    let exit_code_path = hermes_home.join(".update_exit_code");
    let output_path = hermes_home.join(".update_output.txt");
    std::fs::write(&exit_code_path, format!("{}\n", exit_code))
        .with_context(|| format!("cannot write {}", exit_code_path.display()))?;
    std::fs::write(&output_path, message)
        .with_context(|| format!("cannot write {}", output_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_gateway_uses_new_slot_bridge() {
        let tmp = tempfile::tempdir().unwrap();
        let slot = slots::slot_path(tmp.path(), "1.0.0");
        let python = slot.join("runtime/venv/bin/python");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&python, "#!/bin/sh\nprintf 'no gateway running\\n'\n").unwrap();
            std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
            restart_gateway(tmp.path(), "1.0.0").unwrap();
        }
    }

    #[test]
    fn test_write_notify_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_notify_files(tmp.path(), 0, "update complete", None).unwrap();
        let exit_code = std::fs::read_to_string(tmp.path().join(".update_exit_code")).unwrap();
        let output = std::fs::read_to_string(tmp.path().join(".update_output.txt")).unwrap();
        assert_eq!(exit_code.trim(), "0");
        assert_eq!(output, "update complete");
    }

    #[test]
    fn test_write_notify_files_with_custom_path() {
        let tmp = tempfile::tempdir().unwrap();
        let notify_path = tmp.path().join("custom-notify");
        write_notify_files(tmp.path(), 0, "done", Some(notify_path.to_str().unwrap())).unwrap();
        assert_eq!(std::fs::read_to_string(&notify_path).unwrap().trim(), "0");
        assert!(!tmp.path().join(".update_exit_code").exists());
    }
}
