use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn preview_fetch_removes_full_baseline_without_changing_probe_or_owned_homes() {
    let catalog = ToolCatalog::pinned();
    let roots = [String::new(), "nested".to_owned()];
    let plan = fetch_steps_for_plan(&catalog, &roots, false).expect("plan preparation");
    let (_, env) = shell_parts(&plan[0].kind).expect("shell");
    let preview = fetch_steps_for_preview(&catalog, &roots, env).expect("preview preparation");
    let credentials = velnor_actions_workflow_renderer::toolchain_env::credential_scrub()
        .into_keys()
        .collect::<Vec<_>>()
        .join(" ");
    for (ordinary, isolated) in plan.iter().zip(&preview) {
        let (ordinary_run, ordinary_env) = shell_parts(&ordinary.kind).expect("ordinary shell");
        let (isolated_run, isolated_env) = shell_parts(&isolated.kind).expect("isolated shell");
        assert_eq!(isolated_env, ordinary_env);
        assert_eq!(&isolated_run[..2], &ordinary_run[..2]);
        assert_eq!(
            isolated_run[2],
            format!("unset {credentials}; {}", ordinary_run[2])
        );
        assert_eq!(ordinary.name, isolated.name);
        assert_eq!(ordinary.role, isolated.role);
    }
}

fn shell_parts(kind: &StepKind) -> Option<(&Vec<String>, &BTreeMap<String, String>)> {
    match kind {
        StepKind::Shell { run, env } => Some((run, env)),
        _ => None,
    }
}

#[test]
fn root_step_probes_before_fetch_with_miss_record() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_plan(&catalog, &[String::new()], false).expect("fetch steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].name, FETCH_SOURCES_STEP);
    let (run, env) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
    assert_eq!(&run[..2], ["sh", "-c"]);
    let spec = catalog.tool_spec(PinnedTool::Rust);
    for need in [
        "mkdir -p \"$RUNNER_TEMP/velnor/cargo-clean\"".to_owned(),
        "cd \"$RUNNER_TEMP/velnor/cargo-clean\"".to_owned(),
        format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo"),
        "metadata --locked --offline".to_owned(),
        "cargo fetch --locked".to_owned(),
        "--manifest-path \"$GITHUB_WORKSPACE/Cargo.toml\"".to_owned(),
        "sources hit, skipping fetch".to_owned(),
        "sources miss (source_missing)".to_owned(),
    ] {
        assert!(run[2].contains(&need), "script misses {need}: {}", run[2]);
    }
    assert!(
        env.get("MISE_CARGO_HOME").is_some_and(|v| !v.is_empty()),
        "writer uses owned homes"
    );
}

#[test]
fn all_features_applies_only_to_project_plan_and_crate_probes() {
    let catalog = ToolCatalog::pinned();
    let roots = [String::new()];
    let plan = fetch_steps_for_plan(&catalog, &roots, true).expect("all-features plan fetch");
    let crates = fetch_steps_for_crate(&catalog, &roots, true).expect("all-features crate fetch");
    for steps in [&plan, &crates] {
        let (run, _) = shell_parts(&steps[0].kind).expect("fetch shell");
        assert!(run[2].contains("metadata --locked --offline --all-features"));
        assert!(run[2].contains("cargo fetch --locked"));
        assert!(
            run[2].matches("--all-features").count() == 1,
            "feature selection belongs to metadata resolution, not Cargo fetch"
        );
    }

    let (_, env) = shell_parts(&plan[0].kind).expect("plan fetch shell");
    let preview = fetch_steps_for_preview(&catalog, &roots, env).expect("preview fetch");
    let (run, _) = shell_parts(&preview[0].kind).expect("preview fetch shell");
    assert!(
        !run[2].contains("--all-features"),
        "preview keeps its existing probe"
    );
}

#[test]
fn crate_source_preparation_preserves_the_callers_validated_environment() {
    let catalog = ToolCatalog::pinned();
    let env = BTreeMap::from([
        ("CARGO_HOME".to_owned(), "/tmp/task-cargo".to_owned()),
        ("MISE_CARGO_HOME".to_owned(), "/tmp/task-cargo".to_owned()),
        ("MISE_RUSTUP_HOME".to_owned(), "/tmp/task-rustup".to_owned()),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.1".to_owned()),
    ]);
    let steps = fetch_steps_for_crate_with_env(&catalog, &[String::new()], &env, true)
        .expect("crate source preparation");
    let (_, actual) = shell_parts(&steps[0].kind).expect("fetch shell");
    assert_eq!(actual, &env);
    let (run, _) = shell_parts(&steps[0].kind).expect("fetch shell");
    assert!(run[2].contains("metadata --locked --offline --all-features"));
    assert!(run[2].contains("cargo fetch --locked"));
    assert_eq!(run[2].matches("--all-features").count(), 1);
}

#[test]
fn crate_fetch_carries_full_validated_contract() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_crate(&catalog, &[String::new()], false).expect("fetch steps");
    let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ] {
        assert_eq!(
            got.get(key).map(String::as_str),
            Some(value),
            "crate fetch must carry the validated policy pair {key}"
        );
    }
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(
            got.get(key).is_some_and(|value| !value.is_empty()),
            "crate fetch must carry a non-empty {key}"
        );
    }
    for key in [
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
    ] {
        assert!(
            !got.contains_key(key),
            "crate fetch must never carry a credential {key}"
        );
    }
    let shared = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)
        .expect("shared crate env");
    assert_eq!(
        got, &shared,
        "fetch must match obligation steps by construction"
    );
}

#[test]
fn plan_fetch_uses_owned_homes_for_shared_snapshot() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_plan(&catalog, &[String::new()], false).expect("fetch steps");
    let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
    let shared =
        crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true).expect("shared env");
    assert_eq!(
        got, &shared,
        "writer and readers share one Cargo home expression"
    );
}

#[test]
fn nested_step_names_its_manifest() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_plan(&catalog, &["nested".to_owned()], false).expect("fetch steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].name, "Fetch Cargo sources (nested/Cargo.toml)");
    let (run, _) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
    assert!(
        run[2].contains("--manifest-path \"$GITHUB_WORKSPACE/nested/Cargo.toml\""),
        "script names absolute manifest: {}",
        run[2]
    );
}

#[test]
fn lockless_roots_emit_no_steps() {
    let catalog = ToolCatalog::pinned();
    let crates = fetch_steps_for_crate(&catalog, &[], false).expect("crate fetch steps");
    let plan = fetch_steps_for_plan(&catalog, &[], false).expect("plan fetch steps");
    assert!(crates.is_empty() && plan.is_empty());
}

#[test]
fn crate_fetch_roots_scopes_to_matching_workspace_unless_all_needed() {
    let roots = vec![String::new(), "crates/velnor-runner".to_owned()];
    assert_eq!(
        crate_fetch_roots("crates/velnor-actions-contract/Cargo.toml", &roots, false),
        vec![String::new()]
    );
    assert_eq!(
        crate_fetch_roots(
            "crates/velnor-runner/crates/runner-core/Cargo.toml",
            &roots,
            false
        ),
        vec!["crates/velnor-runner".to_owned()]
    );
    assert_eq!(
        crate_fetch_roots(
            "crates/velnor-actions-orchestrator/Cargo.toml",
            &roots,
            true
        ),
        roots
    );
}

/// Run one root's fetch script against a fake `mise` and return (status, stdout).
fn run_fetch(mode: &str) -> (Option<i32>, String) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("velnor-fetch-{mode}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let mise = bin.join("mise");
    std::fs::write(
        &mise,
        "#!/bin/sh\nfor a; do case \"$a\" in metadata) exit 1 ;; fetch) printf '%s\\n' '  Downloaded serde v1.0.0' '  Downloaded foo-bar v0.1.0' '  Downloaded 2 crates (10 KB) in 0.1s' >&2; [ \"$FETCH_MODE\" = ok ] && exit 0; exit 101 ;; esac; done; exit 9\n",
    )
    .expect("fake mise");
    std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let script = fetch_script(&ToolCatalog::pinned(), "", false);
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("RUNNER_TEMP", &dir)
        .env("GITHUB_WORKSPACE", &dir)
        .env("FETCH_MODE", mode)
        .output()
        .expect("sh");
    std::fs::remove_dir_all(&dir).ok();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn fetch_fallback_counts_downloaded_crates_and_keeps_the_fetch_status() {
    let (status, stdout) = run_fetch("ok");
    assert_eq!(status, Some(0), "{stdout}");
    assert!(stdout.contains("velnor: sources miss (source_missing), fetching"));
    assert!(stdout.contains("velnor: sources downloaded=2"), "{stdout}");
    let (status, stdout) = run_fetch("fail");
    assert_eq!(status, Some(101), "a failed fetch fails the step: {stdout}");
    assert!(stdout.contains("velnor: sources downloaded=2"), "{stdout}");
}
