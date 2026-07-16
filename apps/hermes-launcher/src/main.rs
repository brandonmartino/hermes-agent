mod adopt;
mod apply;
mod cli;
mod cwd_guard;
mod launch;
mod release;
mod selfupdate;
mod services;
mod slots;
mod tree;

use anyhow::Context;
use cli::Command;
use std::path::PathBuf;

fn hermes_home() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("HERMES_HOME") {
        return Ok(PathBuf::from(path));
    }
    Ok(dirs::home_dir()
        .context("cannot find home directory")?
        .join(".hermes"))
}

fn main() -> anyhow::Result<()> {
    let args = apply_cwd_guard()?;
    let cli = cli::parse_from(args);

    match cli.command {
        Some(Command::Launch { args }) => launch(args),
        Some(Command::Install { source, channel }) => install(source, channel),
        Some(Command::Apply {
            source,
            target_version,
            notify_file,
            relaunch_app,
            report,
        }) => apply(source, target_version, notify_file, relaunch_app, report),
        Some(Command::Rollback) => rollback(),
        Some(Command::Status { check, json }) => status(check, json),
        Some(Command::Adopt {
            from_checkout,
            source,
            undo,
        }) => adopt(from_checkout, source, undo),
        Some(Command::SelfRestage) => self_restage(),
        None => {
            // Should not happen — parse() fills in a default.
            unreachable!("cli::parse() should always set a command")
        }
    }
}

fn apply_cwd_guard() -> anyhow::Result<Vec<String>> {
    let mut argv: Vec<String> = std::env::args().collect();
    if cli::invoked_as_updater() {
        return Ok(argv);
    }
    let has_dev = argv.iter().any(|arg| arg == "--dev");
    let has_global = argv.iter().any(|arg| arg == "--global");
    if has_dev && has_global {
        eprintln!("hermes: --dev and --global are contradictory — pick one.");
        std::process::exit(2);
    }
    if has_global {
        argv.retain(|arg| arg != "--global");
        return Ok(argv);
    }
    let cwd = std::env::current_dir().context("cannot resolve current directory")?;
    let executable = std::env::current_exe().context("cannot resolve launcher executable")?;
    let launcher_tree = tree::resolve_tree_root(&executable)?;
    match cwd_guard::cwd_guard(&launcher_tree.root, &cwd, &argv) {
        cwd_guard::GuardDecision::Refuse(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
        cwd_guard::GuardDecision::Run => {
            argv.retain(|arg| arg != "--dev" && arg != "--global");
            Ok(argv)
        }
        cwd_guard::GuardDecision::ReExec(path) => reexec_checkout(path, &argv[1..]),
    }
}

fn reexec_checkout(path: PathBuf, args: &[String]) -> anyhow::Result<Vec<String>> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = std::process::Command::new(&path).args(args).exec();
        Err(error).with_context(|| format!("cannot exec {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        let status = std::process::Command::new(&path)
            .args(args)
            .status()
            .with_context(|| format!("cannot launch {}", path.display()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn launch(args: Vec<String>) -> anyhow::Result<()> {
    launch::launch(args)
}

fn trusted_release_pubkey() -> anyhow::Result<&'static str> {
    option_env!("HERMES_RELEASE_PUBLIC_KEY")
        .filter(|key| !key.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| {
            anyhow::anyhow!("this updater was built without the Hermes release public key")
        })
}

fn release_source(source: Option<String>) -> anyhow::Result<release::ReleaseSource> {
    release::ReleaseSource::parse(
        source
            .as_deref()
            .unwrap_or("https://github.com/NousResearch/hermes-agent/releases/download"),
    )
}

fn install(source: Option<String>, channel: String) -> anyhow::Result<()> {
    let home = hermes_home()?;
    let source = release_source(source)?;
    let manifest = apply::apply_release(apply::ApplyRequest {
        hermes_home: &home,
        source: &source,
        version: None,
        channel: &channel,
        trusted_pubkey: trusted_release_pubkey()?,
    })?;
    let _marker = apply::UpdateMarker::acquire(&home)?;
    apply::activate_stable_launchers(&home, &manifest.version)?;
    println!("Installed Hermes {}", manifest.version);
    Ok(())
}

fn apply(
    source: Option<String>,
    version: Option<String>,
    notify_file: Option<String>,
    relaunch_app: Option<String>,
    _report: String,
) -> anyhow::Result<()> {
    let home = hermes_home()?;
    let source = release_source(source)?;
    let manifest = apply::apply_release(apply::ApplyRequest {
        hermes_home: &home,
        source: &source,
        version: version.as_deref(),
        channel: "stable",
        trusted_pubkey: trusted_release_pubkey()?,
    })?;
    let _marker = apply::UpdateMarker::acquire(&home)?;
    apply::activate_stable_launchers(&home, &manifest.version)?;
    if let Err(error) = apply::apply_feature_ledger(&home, &manifest.version) {
        eprintln!("warning: feature ledger application failed: {error:#}");
    }
    if let Err(error) = services::restart_gateway(&home, &manifest.version) {
        eprintln!("warning: gateway restart failed: {error:#}");
    }
    services::write_notify_files(
        &home,
        0,
        &format!("Updated Hermes to {}", manifest.version),
        notify_file.as_deref(),
    )?;
    if let Some(executable) = relaunch_app {
        std::process::Command::new(executable).spawn()?;
    }
    println!("Updated Hermes to {}", manifest.version);
    Ok(())
}

fn rollback() -> anyhow::Result<()> {
    let hermes_home = hermes_home()?;
    let version = slots::rollback(&hermes_home)?;
    println!("Rolled back to {}", version);
    Ok(())
}

#[derive(serde::Serialize)]
struct StatusChangelog {
    version: String,
    summary: String,
    author: String,
    at: i64,
    sha: String,
}

#[derive(serde::Serialize)]
struct StatusReport {
    current_version: Option<String>,
    previous_version: Option<String>,
    channel: String,
    staged_leftovers: Vec<String>,
    latest_version: Option<String>,
    update_available: bool,
    behind: usize,
    changelog: Vec<StatusChangelog>,
    current_sha: Option<String>,
    target_sha: Option<String>,
    error: Option<String>,
}

fn status_report(hermes_home: &std::path::Path, check: bool) -> StatusReport {
    let current = slots::resolve_current(hermes_home).unwrap_or(None);
    let previous = slots::resolve_previous(hermes_home).unwrap_or(None);
    let versions = hermes_home.join("versions");
    let mut staged_leftovers: Vec<String> = std::fs::read_dir(&versions)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".staging").map(str::to_owned)
        })
        .collect();
    staged_leftovers.sort();
    let current_manifest = current
        .as_deref()
        .map(|version| versions.join(version).join("manifest.json"))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<release::Manifest>(&bytes).ok());
    let channel = current_manifest
        .as_ref()
        .map(|manifest| manifest.channel.clone())
        .unwrap_or_else(|| "stable".to_owned());
    let mut report = StatusReport {
        current_version: current.clone(),
        previous_version: previous,
        channel: channel.clone(),
        staged_leftovers,
        latest_version: None,
        update_available: false,
        behind: 0,
        changelog: Vec::new(),
        current_sha: current_manifest.map(|manifest| manifest.git_sha),
        target_sha: None,
        error: None,
    };
    if check {
        match release_source(None).and_then(|source| source.history(&channel)) {
            Ok(history) => {
                report.latest_version = history.first().map(|item| item.version.clone());
                report.target_sha = history.first().and_then(|item| item.target_sha.clone());
                let newer: Vec<_> = history
                    .iter()
                    .take_while(|item| Some(&item.version) != current.as_ref())
                    .collect();
                report.update_available =
                    !newer.is_empty() && report.latest_version.as_ref() != current.as_ref();
                report.behind = if report.update_available {
                    newer.len()
                } else {
                    0
                };
                report.changelog = newer
                    .into_iter()
                    .map(|item| StatusChangelog {
                        version: item.version.clone(),
                        summary: item.summary.clone(),
                        author: String::new(),
                        at: item
                            .published_at
                            .as_deref()
                            .and_then(|value| {
                                time::OffsetDateTime::parse(
                                    value,
                                    &time::format_description::well_known::Rfc3339,
                                )
                                .ok()
                            })
                            .map(|value| value.unix_timestamp())
                            .unwrap_or(0),
                        sha: item.target_sha.clone().unwrap_or_default(),
                    })
                    .collect();
            }
            Err(error) => report.error = Some(error.to_string()),
        }
    }
    report
}

fn status(check: bool, json: bool) -> anyhow::Result<()> {
    let report = status_report(&hermes_home()?, check);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("hermes-updater 0.1.0");
    println!(
        "  current:  {}",
        report.current_version.as_deref().unwrap_or("(none)")
    );
    println!(
        "  previous: {}",
        report.previous_version.as_deref().unwrap_or("(none)")
    );
    println!("  channel:  {}", report.channel);
    println!(
        "  staged:   {}",
        if report.staged_leftovers.is_empty() {
            "(none)".to_owned()
        } else {
            report.staged_leftovers.join(", ")
        }
    );
    if check {
        if let Some(error) = report.error {
            println!("  check:    failed: {}", error);
        } else {
            println!(
                "  latest:   {} ({} release(s) behind)",
                report.latest_version.as_deref().unwrap_or("(none)"),
                report.behind
            );
        }
    }
    Ok(())
}

fn adopt(from_checkout: Option<String>, source: Option<String>, undo: bool) -> anyhow::Result<()> {
    let hermes_home = hermes_home()?;

    let checkout = match from_checkout {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            // Default to the current checkout (PROJECT_ROOT)
            std::path::PathBuf::from(".")
        }
    };

    let trusted_pubkey = if undo { "" } else { trusted_release_pubkey()? };
    adopt::adopt(
        &hermes_home,
        &checkout,
        source.as_deref(),
        undo,
        trusted_pubkey,
    )
}

fn self_restage() -> anyhow::Result<()> {
    let home = hermes_home()?;
    let version =
        slots::resolve_current(&home)?.ok_or_else(|| anyhow::anyhow!("no current managed slot"))?;
    apply::activate_stable_launchers(&home, &version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvRestore {
        key: &'static str,
        value: Option<OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn hermes_home_honors_environment_override() {
        let _restore = EnvRestore {
            key: "HERMES_HOME",
            value: std::env::var_os("HERMES_HOME"),
        };
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HERMES_HOME", temp.path());

        assert_eq!(hermes_home().unwrap(), temp.path());
    }

    #[test]
    fn status_report_includes_slot_metadata_and_staging() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("versions/1.0.0")).unwrap();
        std::fs::create_dir_all(temp.path().join("versions/2.0.0.staging")).unwrap();
        std::fs::write(temp.path().join("current.txt"), "1.0.0\n").unwrap();
        std::fs::write(temp.path().join("previous.txt"), "0.9.0\n").unwrap();
        std::fs::write(
            temp.path().join("versions/1.0.0/manifest.json"),
            serde_json::json!({
                "schema": 1,
                "version": "1.0.0",
                "channel": "nightly",
                "git_sha": "abc",
                "platform": "linux-x64",
                "min_updater_version": "0.1.0",
                "desktop": false,
                "files": {}
            })
            .to_string(),
        )
        .unwrap();

        let report = status_report(temp.path(), false);
        assert_eq!(report.current_version.as_deref(), Some("1.0.0"));
        assert_eq!(report.previous_version.as_deref(), Some("0.9.0"));
        assert_eq!(report.channel, "nightly");
        assert_eq!(report.current_sha.as_deref(), Some("abc"));
        assert_eq!(report.staged_leftovers, ["2.0.0"]);
    }

    #[test]
    fn test_status_works() {
        // status is the one verb that isn't a stub — it prints a version line.
        assert!(status(false, false).is_ok());
    }
}
