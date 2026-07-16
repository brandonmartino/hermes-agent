//! Adopt verb (hop 3, Rust side) — migrate a legacy git-checkout install
//! to managed slots.
//!
//! See docs/updater-world.md §2.13 and
//! docs/plans/updater-rework/03-phase2-compat-and-adoption.md task 2.6.

use crate::apply::{self, ApplyRequest};
use crate::release::ReleaseSource;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Perform the adoption: download a bundle, create a slot, flip, re-point
/// the PATH symlink. The checkout is left completely untouched.
pub fn adopt(
    hermes_home: &Path,
    from_checkout: &Path,
    source: Option<&str>,
    undo: bool,
    trusted_pubkey: &str,
) -> Result<()> {
    if undo {
        return adopt_undo(hermes_home);
    }

    let git_sha = read_checkout_sha(from_checkout)?;
    let checkout_state = read_checkout_state(from_checkout)?;
    println!(
        "==> Adopting from checkout: {} ({})",
        from_checkout.display(),
        &git_sha[..8]
    );

    let source_url =
        source.unwrap_or("https://github.com/NousResearch/hermes-agent/releases/download");
    let release_source = ReleaseSource::parse(source_url)?;
    let manifest = apply::apply_release(ApplyRequest {
        hermes_home,
        source: &release_source,
        version: None,
        channel: "stable",
        trusted_pubkey,
    })?;
    let version = manifest.version;
    let slot = hermes_home.join("versions").join(&version);
    apply::activate_stable_launchers(hermes_home, &version)?;
    if let Err(error) = apply::apply_feature_ledger(hermes_home, &version) {
        eprintln!("warning: feature ledger application failed: {error:#}");
    }
    if let Err(error) = crate::services::restart_gateway(hermes_home, &version) {
        eprintln!("warning: gateway restart failed: {error:#}");
    }

    let launcher = hermes_home.join("bin").join(if cfg!(windows) {
        "hermes.exe"
    } else {
        "hermes"
    });
    let link_dir = find_command_link_dir()?;
    let symlink_path = link_dir.join("hermes");

    // Record the old target for undo
    let pre_adopt_path = hermes_home.join(".pre-adopt-target");
    if symlink_path.exists() || symlink_path.is_symlink() {
        if let Ok(target) = std::fs::read_link(&symlink_path) {
            std::fs::write(&pre_adopt_path, target.to_string_lossy().as_bytes())
                .context("cannot write .pre-adopt-target")?;
        }
    }

    // Re-point the symlink
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&symlink_path);
        std::os::unix::fs::symlink(&launcher, &symlink_path).with_context(|| {
            format!(
                "cannot symlink {} → {}",
                symlink_path.display(),
                launcher.display()
            )
        })?;
    }

    println!(
        "==> Symlink: {} → {}",
        symlink_path.display(),
        launcher.display()
    );

    let new_sha = read_checkout_sha(from_checkout)?;
    if new_sha != git_sha || read_checkout_state(from_checkout)? != checkout_state {
        bail!(
            "checkout was modified during adoption (HEAD expected {}, got {})",
            git_sha,
            new_sha
        );
    }
    println!("==> Checkout untouched");

    println!();
    println!("✓ Adoption complete!");
    println!("  Version:  {}", version);
    println!("  Slot:    {}", slot.display());
    println!("  Symlink: {}", symlink_path.display());
    println!();
    println!("  Undo with: hermes-updater adopt --undo");

    Ok(())
}

/// Undo a previous adoption: re-point the symlink at the old target.
fn adopt_undo(hermes_home: &Path) -> Result<()> {
    let pre_adopt_path = hermes_home.join(".pre-adopt-target");
    if !pre_adopt_path.exists() {
        bail!("no .pre-adopt-target found — nothing to undo");
    }

    let old_target = std::fs::read_to_string(&pre_adopt_path)?;
    let old_target = old_target.trim();

    let link_dir = find_command_link_dir()?;
    let symlink_path = link_dir.join("hermes");

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&symlink_path);
        std::os::unix::fs::symlink(old_target, &symlink_path)?;
    }

    let _ = std::fs::remove_file(&pre_adopt_path);

    println!("✓ Adoption undone");
    println!("  Symlink: {} → {}", symlink_path.display(), old_target);

    Ok(())
}

/// Read the git SHA of a checkout.
fn read_checkout_sha(checkout: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(checkout)
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_checkout_state(checkout: &Path) -> Result<Vec<u8>> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(checkout)
        .output()
        .context("failed to run git status")?;
    if !output.status.success() {
        bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

/// Find the command link directory (~/.local/bin, /usr/local/bin, etc.)
fn find_command_link_dir() -> Result<PathBuf> {
    // Check common locations
    let home = dirs::home_dir().context("cannot find home directory")?;

    // Try ~/.local/bin first
    let local_bin = home.join(".local").join("bin");
    if local_bin.exists() {
        return Ok(local_bin);
    }

    // Try /usr/local/bin
    let usr_local = PathBuf::from("/usr/local/bin");
    if usr_local.exists() && usr_local.is_dir() {
        return Ok(usr_local);
    }

    // Fallback: create ~/.local/bin
    std::fs::create_dir_all(&local_bin)?;
    Ok(local_bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_checkout_sha_invalid_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_checkout_sha(tmp.path());
        // Not a git repo — should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_find_command_link_dir() {
        let dir = find_command_link_dir().unwrap();
        assert!(dir.is_dir() || dir.parent().is_some());
    }

    #[test]
    fn test_adopt_undo_fails_without_pre_adopt() {
        let tmp = tempfile::tempdir().unwrap();
        let result = adopt_undo(tmp.path());
        assert!(result.is_err());
    }
}
