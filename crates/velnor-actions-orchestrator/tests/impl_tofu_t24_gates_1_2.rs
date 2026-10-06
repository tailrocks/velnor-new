use super::*;

/// Gate 1: "A pure-tofu consumer using a prebuilt generator
/// installs/runs no Rust toolchain, Cargo metadata, MBX, Nextest,
/// rustfmt, or Clippy for its stack or Plan job."
#[test]
fn gate1_pure_tofu_installs_no_rust_toolchain() -> TestResult {
    let dir = tofu_repo(2)?;
    write_public_provider_locks(dir.path(), &tofu_perf_fixtures_t24::tofu_root_names(2))?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let catalog = ToolCatalog::pinned();
    let rust = catalog
        .tool_spec(PinnedTool::Rust)
        .expect("qualified selector");
    let mbx = catalog
        .tool_spec(PinnedTool::MrBoxington)
        .expect("qualified selector");
    let opentofu = catalog
        .native_tool_spec(
            velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
            PinnedTool::Opentofu,
        )
        .expect("qualified selector");
    let mut checked = 0;
    for (id, job) in &jobs {
        if (id != PLAN_JOB_ID && !is_crate_job(id)) || job.source_producer.is_some() {
            continue;
        }
        checked += 1;
        let steps = names(job);
        assert!(
            !steps.contains(&"Prepare Rust components"),
            "{id} has no components step: {steps:?}"
        );
        assert!(
            !steps.iter().any(|name| name.starts_with("Fetch Cargo")),
            "{id} has no cargo fetch: {steps:?}"
        );
        for name in &steps {
            for banned in [
                "Rust", "Cargo", "rustfmt", "Clippy", "Nextest", "MBX", "metadata",
            ] {
                assert!(!name.contains(banned), "{id} step {name} names {banned}");
            }
        }
        let (run, env) = shell_of(job, "Prepare pinned tools")?;
        assert!(run.contains(&opentofu), "{id} installs opentofu: {run:?}");
        assert!(!run.contains(&rust), "{id} installs no Rust: {run:?}");
        assert!(!run.contains(&mbx), "{id} installs no MBX: {run:?}");
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert!(!env.contains_key(key), "{id} prepare carries no {key}");
        }
    }
    assert_eq!(checked, 3, "plan plus two stack jobs");
    Ok(())
}

/// The tofu adapter spawns no processes and emits argv, never shell.
fn assert_tofu_zero_spawn(tofu: &Path) -> TestResult {
    for token in [
        "Command::new",
        "process::Command",
        ".spawn(",
        ".output(",
        "tokio::process",
        "std::process",
    ] {
        assert!(
            token_hits(tofu, token)?.is_empty(),
            "{token}: {:?}",
            token_hits(tofu, token)?
        );
    }
    assert!(
        token_hits(tofu, "\"sh\"")?.is_empty(),
        "tofu emits argv, never shell"
    );
    Ok(())
}

/// Discovery/plan files download nothing: merge-time retrieval
/// (`retrieve_*`) may retry downloads, but discovery never does.
fn assert_discovery_downloads_nothing(orch: &Path, tofu: &Path) -> TestResult {
    let plan_path = [
        "discover.rs",
        "discover_index.rs",
        "discover_tofu.rs",
        "select.rs",
        "select_tofu.rs",
        "internal.rs",
        "internal_plan.rs",
        "internal_request.rs",
    ];
    for token in ["download_with_retry", "reqwest", "ureq"] {
        let hits: Vec<String> = token_hits(orch, token)?
            .into_iter()
            .filter(|hit| {
                plan_path
                    .iter()
                    .any(|name| hit.starts_with(&format!("{name}:")))
            })
            .collect();
        assert!(hits.is_empty(), "{token}: {hits:?}");
    }
    for token in ["download", "fetch("] {
        assert!(
            token_hits(tofu, token)?.is_empty(),
            "{token}: {:?}",
            token_hits(tofu, token)?
        );
    }
    Ok(())
}

/// Gate 2: "`plan`/`generate` perform zero tofu init/validate/apply
/// operations, no provider downloads for discovery, and no
/// modifications to source/tool/lock files."
#[test]
fn gate2_plan_and_generate_run_zero_tofu_operations() -> TestResult {
    let tofu = crate_src("../velnor-actions-tofu");
    assert_tofu_zero_spawn(&tofu)?;
    assert_discovery_downloads_nothing(&crate_src(""), &tofu)?;
    let dir = tofu_repo_with_lock()?;
    let root = dir.path();
    let (base, head) = commit_two_tofu(root, "stacks/a/main.tf", "variable \"bump\" {}\n")?;
    let before = snapshot(root)?;
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": base, "head": head,
        "event": "pull_request", "root": root.display().to_string(),
    });
    plan_internal(&request.to_string())?;
    let prep = prepare(root)?;
    let out = tempfile::TempDir::new()?;
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(out.path().join("gate2")),
        },
    )?;
    let after = snapshot(root)?;
    let bytes = |snap: Snapshot| {
        snap.into_iter()
            .filter(|(path, _)| !path.starts_with(".git/"))
            .map(|(path, (body, _))| (path, body))
            .collect::<BTreeMap<_, _>>()
    };
    let (before, after) = (bytes(before), bytes(after));
    let mut changed = Vec::new();
    for path in before.keys().chain(after.keys()) {
        if before.get(path) != after.get(path) {
            changed.push(path.clone());
        }
    }
    changed.sort();
    changed.dedup();
    assert!(changed.is_empty(), "plan+generate modified: {changed:?}");
    Ok(())
}
