use super::*;
use velnor_actions_contract::StepKind;

fn shell_parts(kind: &StepKind) -> Option<(&Vec<String>, &BTreeMap<String, String>)> {
    match kind {
        StepKind::Shell { run, env } => Some((run, env)),
        _ => None,
    }
}

#[test]
fn root_step_probes_before_fetch_with_miss_record() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
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
fn crate_fetch_carries_full_validated_contract() {
    let catalog = ToolCatalog::pinned();
    let steps = fetch_steps_for_crate(&catalog, &[String::new()]).expect("fetch steps");
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
    let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
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
    let steps = fetch_steps_for_plan(&catalog, &["nested".to_owned()]).expect("fetch steps");
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
    let crates = fetch_steps_for_crate(&catalog, &[]).expect("crate fetch steps");
    let plan = fetch_steps_for_plan(&catalog, &[]).expect("plan fetch steps");
    assert!(crates.is_empty() && plan.is_empty());
}
