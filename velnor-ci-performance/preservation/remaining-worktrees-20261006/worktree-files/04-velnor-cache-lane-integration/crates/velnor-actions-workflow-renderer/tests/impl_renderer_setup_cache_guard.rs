//! Cache-guard invariants for P08 setup generation.
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Output};
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::BTreeMap, error::Error};

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::{
    RenderError,
    cache_p08::{RESTORE_SOURCES_NAME, RUST_CACHE_NAME, ensure_setup_p08},
    steps::{WORKSPACE_CACHE_GUARD_NAME, workspace_cache_guard_step},
};

use crate::impl_renderer_fixtures::{checkout_pin, job, mise, mise_argv, scrubbed_shell_step};

#[path = "impl_renderer_cache_report_wrapper.rs"]
mod report_wrapper_tests;

#[cfg(unix)]
struct TempRoot(PathBuf);

#[cfg(unix)]
impl TempRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root =
            std::env::temp_dir().join(format!("velnor-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }
}

#[cfg(unix)]
impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove test root {}: {error}", self.0.display());
        }
    }
}

#[cfg(unix)]
fn run_guard(step: &Step, workspace: &Path) -> Result<Output, Box<dyn Error>> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err("workspace cache guard must be a shell step".into());
    };
    Ok(Command::new(&run[0])
        .args(&run[1..])
        .env("GITHUB_WORKSPACE", workspace)
        .output()?)
}

#[test]
#[cfg(unix)]
fn no_checkout_guard_allows_missing_git_and_rejects_cache_links() -> Result<(), Box<dyn Error>> {
    let guard = workspace_cache_guard_step(false)?;
    assert_eq!(guard.name, WORKSPACE_CACHE_GUARD_NAME);
    let temp = TempRoot::new("cache-guard-no-checkout")?;
    let workspace = temp.0.join("repo");
    fs::create_dir(&workspace)?;
    let outside = temp.0.join("outside");
    fs::create_dir(&outside)?;

    let clean = run_guard(&guard, &workspace)?;
    assert!(clean.status.success(), "a checkout-free job has no .git");
    assert_eq!(
        fs::read_dir(&workspace)?.count(),
        0,
        "guard creates no metadata"
    );
    let git = workspace.join(".git");
    for kind in ["file", "directory", "symlink"] {
        match kind {
            "file" => fs::write(&git, b"git metadata")?,
            "directory" => fs::create_dir(&git)?,
            "symlink" => std::os::unix::fs::symlink(&outside, &git)?,
            _ => return Err("unknown Git fixture".into()),
        }
        let result = run_guard(&guard, &workspace)?;
        assert!(
            !result.status.success()
                && String::from_utf8_lossy(&result.stderr)
                    .contains("velnor_git_directory_unexpected_without_checkout"),
            "no-checkout guard rejected .git {kind}"
        );
        if kind == "directory" {
            fs::remove_dir(&git)?;
        } else {
            fs::remove_file(&git)?;
        }
    }

    for relative in [
        ".velnor/cache/rustup",
        ".velnor/cache/cargo/registry",
        ".velnor/cache/cargo/registry/cache",
        ".velnor/cache/cargo/registry/index",
        ".velnor/cache/cargo/registry/src",
        ".velnor/cache/cargo/git/db",
        ".velnor/cache/cargo/git/checkouts",
    ] {
        let link = workspace.join(relative);
        fs::create_dir_all(link.parent().ok_or("cache link parent")?)?;
        std::os::unix::fs::symlink(&outside, &link)?;
        let result = run_guard(&guard, &workspace)?;
        assert!(
            !result.status.success()
                && String::from_utf8_lossy(&result.stderr)
                    .contains(&format!("velnor_cache_symlink:{relative}")),
            "no-checkout guard rejects {relative} before restore"
        );
        fs::remove_file(link)?;
        fs::remove_dir_all(workspace.join(".velnor"))?;
    }

    let file = workspace.join(".velnor/cache/rustup");
    fs::create_dir_all(file.parent().ok_or("cache file parent")?)?;
    fs::write(&file, b"not a cache directory")?;
    let result = run_guard(&guard, &workspace)?;
    assert!(
        !result.status.success()
            && String::from_utf8_lossy(&result.stderr)
                .contains("velnor_cache_not_directory:.velnor/cache/rustup"),
        "regular files cannot occupy cache roots"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn checkout_guard_rejects_git_path_symlinks_and_non_directories() -> Result<(), Box<dyn Error>> {
    let guard = workspace_cache_guard_step(true)?;
    let temp = TempRoot::new("cache-guard-checkout")?;
    let outside = temp.0.join("outside");
    fs::create_dir_all(outside.join("info"))?;
    fs::write(
        outside.join("info/exclude"),
        b"outside info stays unchanged",
    )?;
    fs::write(outside.join("exclude"), b"outside exclude stays unchanged")?;
    for (case, marker) in [
        ("git-file", "velnor_git_directory_invalid"),
        ("git-symlink", "velnor_git_directory_invalid"),
        ("info-file", "velnor_git_info_invalid"),
        ("info-symlink", "velnor_git_info_invalid"),
        ("exclude-directory", "velnor_git_exclude_invalid"),
        ("exclude-symlink", "velnor_git_exclude_invalid"),
    ] {
        let workspace = temp.0.join(case);
        fs::create_dir(&workspace)?;
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&workspace)
                .status()?
                .success()
        );
        match case {
            "git-file" => {
                fs::remove_dir_all(workspace.join(".git"))?;
                fs::write(workspace.join(".git"), b"git file")?;
            }
            "git-symlink" => {
                fs::remove_dir_all(workspace.join(".git"))?;
                std::os::unix::fs::symlink(&outside, workspace.join(".git"))?;
            }
            "info-file" => {
                fs::remove_dir_all(workspace.join(".git/info"))?;
                fs::write(workspace.join(".git/info"), b"info file")?;
            }
            "info-symlink" => {
                fs::remove_dir_all(workspace.join(".git/info"))?;
                std::os::unix::fs::symlink(outside.join("info"), workspace.join(".git/info"))?;
            }
            "exclude-directory" => {
                fs::remove_file(workspace.join(".git/info/exclude"))?;
                fs::create_dir(workspace.join(".git/info/exclude"))?;
            }
            "exclude-symlink" => {
                fs::remove_file(workspace.join(".git/info/exclude"))?;
                std::os::unix::fs::symlink(
                    outside.join("exclude"),
                    workspace.join(".git/info/exclude"),
                )?;
            }
            _ => return Err("unknown checkout fixture".into()),
        }
        let result = run_guard(&guard, &workspace)?;
        assert!(
            !result.status.success() && String::from_utf8_lossy(&result.stderr).contains(marker),
            "guard rejects {case} with {marker}"
        );
        assert_eq!(
            fs::read(outside.join("info/exclude"))?,
            b"outside info stays unchanged"
        );
        assert_eq!(
            fs::read(outside.join("exclude"))?,
            b"outside exclude stays unchanged"
        );
    }
    Ok(())
}

#[test]
fn guard_precedes_existing_source_and_swatinem_restores() -> Result<(), RenderError> {
    let sha = "a".repeat(40);
    for (restore_name, uses, path) in [
        (
            RESTORE_SOURCES_NAME,
            format!("actions/cache/restore@{sha}"),
            Some(".velnor/cache/cargo/registry\n.velnor/cache/cargo/git/db"),
        ),
        (RUST_CACHE_NAME, format!("Swatinem/rust-cache@{sha}"), None),
    ] {
        let mut with = BTreeMap::new();
        if let Some(path) = path {
            with.insert("key".to_owned(), "sources-fixture".to_owned());
            with.insert("path".to_owned(), path.to_owned());
        }
        let restore = Step {
            name: restore_name.to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses,
                with,
                env: BTreeMap::new(),
            },
        };
        let setup = velnor_actions_workflow_renderer::mise_setup_step(&mise())?;
        let command = scrubbed_shell_step(
            "Run actionlint",
            mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
        )?;
        let (job_id, mut workflow_job) = job(
            "cache-reader",
            "Cache reader",
            Vec::new(),
            vec![
                velnor_actions_workflow_renderer::checkout_step(&checkout_pin())?,
                restore,
                setup,
                command,
            ],
        );
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu",
        )?;
        let names: Vec<&str> = workflow_job
            .steps
            .iter()
            .map(|step| step.name.as_str())
            .collect();
        let guard_at = names
            .iter()
            .position(|name| *name == WORKSPACE_CACHE_GUARD_NAME);
        let restore_at = names.iter().position(|name| *name == restore_name);
        assert!(
            guard_at.is_some_and(|guard| guard > 0 && Some(guard) < restore_at),
            "guard must precede {restore_name}: {names:?}"
        );
    }
    Ok(())
}

#[test]
fn duplicate_workspace_guards_are_rejected() -> Result<(), RenderError> {
    let guard = workspace_cache_guard_step(true)?;
    let setup = velnor_actions_workflow_renderer::mise_setup_step(&mise())?;
    let command = scrubbed_shell_step(
        "Run actionlint",
        mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
    )?;
    let (job_id, mut workflow_job) = job(
        "duplicate-guard",
        "Duplicate guard",
        Vec::new(),
        vec![
            velnor_actions_workflow_renderer::checkout_step(&checkout_pin())?,
            guard.clone(),
            guard,
            setup,
            command,
        ],
    );
    assert!(
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu",
        )
        .is_err_and(|error| format!("{error:?}").contains("duplicate_workspace_cache_guard"))
    );
    Ok(())
}

#[test]
fn repeated_always_bootstrap_setup_is_idempotent() -> Result<(), RenderError> {
    let echo = scrubbed_shell_step(
        "Run echo",
        vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
    )?;
    let (job_id, mut workflow_job) = job("bootstrap", "Bootstrap", Vec::new(), vec![echo]);
    for _ in 0..2 {
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            true,
            "x86_64-unknown-linux-gnu",
        )?;
    }
    for expected in [
        WORKSPACE_CACHE_GUARD_NAME,
        "Restore Mise tools",
        "Setup Mise",
    ] {
        assert_eq!(
            workflow_job
                .steps
                .iter()
                .filter(|step| step.name == expected)
                .count(),
            1,
            "one {expected}: {:?}",
            workflow_job
                .steps
                .iter()
                .map(|step| &step.name)
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}
