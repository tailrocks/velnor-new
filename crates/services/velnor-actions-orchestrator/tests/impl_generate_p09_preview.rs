//! P07/P09 regression cases: validated task-step env, preview and
//! generate filesystem guarantees.
//!
//! Owned by the envfs builder; wired into `velnor_orchestrator` by the
//! orchestrator-test owner with one `mod` line.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract_workflow::StepKind;
use velnor_actions_orchestrator::{
    GenerateOptions, GenerateReport, GenerationPreparation, OrchestratorError, ToolSnapshot,
    generate, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, err_of, make_repo, snapshot};

/// Preview-generate helper: `Ok` report or the typed error.
fn preview(
    prep: &GenerationPreparation,
    dir: &std::path::Path,
) -> Result<GenerateReport, OrchestratorError> {
    let opts = GenerateOptions {
        output_dir: Some(dir.to_path_buf()),
    };
    generate(prep, &opts)
}

/// Rendered workflow text window between two job markers.
fn window<'a>(yaml: &'a str, from: &str, to: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let start = yaml.find(from).ok_or("missing window start")?;
    let end = yaml[start..].find(to).map_or(yaml.len(), |at| start + at);
    Ok(&yaml[start..end])
}

#[test]
fn preview_inside_repo_leaves_nothing() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    for dest in [
        root.to_path_buf(),
        root.join("sub"),
        root.join("new-nested/deep"),
    ] {
        let err = err_of(preview(&prep, &dest), "inside refused")?;
        let refused = matches!(err, OrchestratorError::PreviewRefused { .. });
        assert!(refused, "got {err}");
    }
    assert!(!root.join("sub").exists(), "refused preview made sub/");
    assert!(
        !root.join("new-nested").exists(),
        "refused preview made parents"
    );
    Ok(())
}

#[test]
fn preview_dotdot_through_missing_refused_without_litter() -> TestResult {
    // The kernel cannot resolve `..` past missing components, so a
    // lexical pop would be unsound: fail closed and leave nothing.
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let dest = root.join("a").join("..").join("..").join("sibling-preview");
    let err = err_of(preview(&prep, &dest), "dotdot refused")?;
    assert!(err.to_string().contains("traversal"), "got {err}");
    assert!(!root.join("a").exists(), "no litter from dotdot");
    let parent = root.parent().ok_or("no parent")?;
    let preview = parent.join("sibling-preview");
    assert!(!preview.exists(), "nothing outside either");
    Ok(())
}

#[cfg(unix)]
#[test]
fn preview_symlink_chain_refused() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let link = parent.path().join("link");
    std::os::unix::fs::symlink(root, &link)?;
    let dest = link.join("preview");
    let err = err_of(preview(&prep, &dest), "chained link refused")?;
    let refused = matches!(err, OrchestratorError::PreviewRefused { .. });
    assert!(refused, "got {err}");
    assert!(!link.join("preview").exists(), "nothing through the link");
    Ok(())
}

#[test]
fn preview_twice_second_refused_nonempty() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let dest = parent.path().join("preview");
    preview(&prep, &dest)?;
    let err = err_of(preview(&prep, &dest), "second refused")?;
    let refused = matches!(err, OrchestratorError::PreviewRefused { .. });
    assert!(refused, "got {err}");
    Ok(())
}

#[test]
fn rendered_crate_steps_carry_validated_contract() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let task = window(yaml, "  rust-demo:", "  required:")?;
    let run_at = task.find("- name: Clippy").ok_or("first obligation")?;
    let run_block = &task[run_at..];
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:"] {
        assert!(run_block.contains(key), "Clippy misses {key}");
    }
    assert!(
        task.contains("RUSTUP_TOOLCHAIN: 1.98.1"),
        "task misses toolchain"
    );
    for key in [
        "MISE_NO_CONFIG:",
        "MISE_NO_ENV:",
        "MISE_NO_HOOKS:",
        "MISE_LOCKFILE:",
        "MISE_AUTO_INSTALL:",
        "MISE_EXEC_AUTO_INSTALL:",
    ] {
        assert!(task.contains(key), "task misses {key}");
    }
    for key in [
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(
            task.contains(&format!("{key}: \"\"")),
            "task must scrub {key} empty"
        );
    }
    Ok(())
}

#[test]
fn fetch_matches_obligation_contract_by_construction() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    let prep = prepare(root)?;
    let task = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or_else(|| std::io::Error::other("missing crate job"))?;
    let names: Vec<&str> = task.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    let (Some(fetch_at), Some(run_at)) = (at("Fetch Cargo sources"), at("Clippy")) else {
        return Err("crate steps miss fetch/obligation".into());
    };
    let StepKind::Shell { env: fetch_env, .. } = &task.steps[fetch_at].kind else {
        return Err("fetch must be a shell step".into());
    };
    let StepKind::Shell { env: run_env, .. } = &task.steps[run_at].kind else {
        return Err("Clippy must be a shell step".into());
    };
    let mut keys: Vec<&str> = fetch_env.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "MISE_AUTO_INSTALL",
            "MISE_CARGO_HOME",
            "MISE_EXEC_AUTO_INSTALL",
            "MISE_LOCKFILE",
            "MISE_NO_CONFIG",
            "MISE_NO_ENV",
            "MISE_NO_HOOKS",
            "MISE_RUSTUP_HOME",
            "RUSTUP_TOOLCHAIN",
        ],
        "fetch carries exactly the validated contract"
    );
    for (key, value) in fetch_env {
        let got = run_env.get(key);
        assert_eq!(got, Some(value), "fetch key {key} must equal Clippy");
        assert!(!value.is_empty(), "fetch key {key} must be set");
    }
    Ok(())
}

#[test]
fn denylist_pins_mise_strip_set() {
    assert_eq!(
        velnor_actions_workflow_steps::toolchain_env::STEP_CREDENTIAL_DENYLIST,
        velnor_actions_mise::CREDENTIAL_ENV_KEYS,
        "rendered steps and local spawns share one credential contract"
    );
    assert_eq!(
        velnor_actions_workflow_steps::toolchain_env::STEP_ENDPOINT_DENYLIST,
        velnor_actions_mise::ENDPOINT_ENV_KEYS,
        "rendered steps and local spawns share one endpoint contract"
    );
    for denied in velnor_actions_workflow_steps::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .iter()
        .chain(velnor_actions_workflow_steps::toolchain_env::STEP_ENDPOINT_DENYLIST.iter())
    {
        let reserved = velnor_actions_mise::command::is_reserved_env_key(denied);
        assert!(reserved, "{denied} must be reserved in Mise too");
    }
    for denied in velnor_actions_workflow_steps::toolchain_env::STEP_ISOLATION_DENYLIST {
        let reserved = velnor_actions_mise::command::is_reserved_env_key(denied);
        assert!(reserved, "{denied} must be reserved in Mise too");
    }
    for key in [
        "GITHUB_TOKEN",
        "NPM_TOKEN",
        "CARGO_REGISTRIES_ACME_TOKEN",
        "CARGO_REGISTRIES_ENTERPRISE",
        "MY_REGISTRY_TOKEN",
        "MISE_RUSTUP_HOME",
        "TOKEN_COUNT",
        "CARGO_REGISTRIES",
    ] {
        assert_eq!(
            velnor_actions_workflow_steps::toolchain_env::is_denied_credential_key(key),
            velnor_actions_mise::command::is_denied_credential_key(key),
            "credential pattern parity for {key}"
        );
    }
}

#[test]
fn concurrent_generate_lock_refused() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let github = root.join(".github");
    fs::create_dir_all(github.join("workflows"))?;
    fs::write(github.join("workflows/old.yml"), "old: true\n")?;
    fs::create_dir(root.join(".github.velnor-generate.lock"))?;
    let prep = prepare(root)?;
    let opts = GenerateOptions { output_dir: None };
    let err = err_of(generate(&prep, &opts), "locked")?;
    assert!(err.to_string().contains("concurrent_generate"), "got {err}");
    assert_eq!(fs::read(github.join("workflows/old.yml"))?, b"old: true\n");
    fs::remove_dir(root.join(".github.velnor-generate.lock"))?;
    let report = generate(&prep, &opts)?;
    assert_eq!(report.files_written.len(), 5, "lock removal unblocks");
    Ok(())
}

#[test]
fn success_leaves_no_lock_and_no_warnings() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    assert!(report.warnings.is_empty(), "clean commit warns nothing");
    assert!(
        !root.join(".github.velnor-generate.lock").exists(),
        "lock released"
    );
    let parent = TempDir::new()?;
    let report = preview(&prep, &parent.path().join("preview"))?;
    assert!(report.warnings.is_empty(), "clean preview warns nothing");
    Ok(())
}

#[test]
fn generate_leaves_tool_files_untouched() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), "[tools]\n")?;
    fs::write(root.join("mise.lock"), "lock-bytes")?;
    let snap = ToolSnapshot::capture(root);
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    assert!(snap.verify(root).is_ok(), "generate preserves tool files");
    assert_eq!(fs::read(root.join("mise.toml"))?, b"[tools]\n");
    Ok(())
}

#[cfg(unix)]
#[test]
fn tool_snapshot_unreadable_fails_closed() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let tool = root.join("mise.toml");
    fs::write(&tool, "v1")?;
    let snap = ToolSnapshot::capture(root);
    assert!(snap.verify(root).is_ok(), "readable verifies");
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o000))?;
    let readable_despite_bits = fs::read(&tool).is_ok();
    let snap = ToolSnapshot::capture(root);
    let err = snap.verify(root);
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644))?;
    if readable_despite_bits {
        return Ok(()); // Privileged runner: permission bits do not bind.
    }
    let err = err_of(err, "unreadable tool file")?;
    let text = err.to_string();
    assert!(
        text.contains("tool_files_unreadable:mise.toml"),
        "got {text}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn in_place_symlink_target_refused() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let real = root.join("real-github");
    fs::create_dir_all(real.join("workflows"))?;
    fs::write(real.join("workflows/old.yml"), "old: true\n")?;
    std::os::unix::fs::symlink(&real, root.join(".github"))?;
    let prep = prepare(root)?;
    let before = snapshot(root)?;
    let opts = GenerateOptions { output_dir: None };
    let err = err_of(generate(&prep, &opts), "link refused")?;
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert_eq!(before, snapshot(root)?, "refusal writes nothing");
    assert!(fs::symlink_metadata(root.join(".github"))?.is_symlink());
    Ok(())
}

#[test]
fn in_place_file_target_refused() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join(".github"), "not a dir")?;
    let prep = prepare(root)?;
    let opts = GenerateOptions { output_dir: None };
    let err = err_of(generate(&prep, &opts), "file refused")?;
    assert!(err.to_string().contains("not_a_directory"), "{err}");
    assert_eq!(fs::read(root.join(".github"))?, b"not a dir");
    Ok(())
}

#[test]
fn atomic_commit_never_exposes_missing_tree() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let opts = GenerateOptions { output_dir: None };
    generate(&prep, &opts)?;
    let live = root.join(".github/workflows/ci.yml");
    let misses = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..8 {
                generate(&prep, &opts).expect("rewrite commits");
            }
            done.store(1, Ordering::Relaxed);
        });
        for _ in 0..4 {
            scope.spawn(|| {
                while done.load(Ordering::Relaxed) == 0 {
                    if live.symlink_metadata().is_err() {
                        misses.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    assert!(live.is_file(), "final tree live");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert_eq!(
        misses.load(Ordering::Relaxed),
        0,
        "exchange must never expose a gap"
    );
    Ok(())
}
