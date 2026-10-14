use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;
use velnor_actions_orchestrator::finalized_jobs;
use velnor_actions_workflow_renderer::SETUP_MISE_NAME;

/// R22: every Rust job reinstalls clippy/rustfmt after cache restore.
///
/// Cold-install-always policy: the fixed `rustup component add` step
/// is unconditional (no `if:`), so it runs on cold AND warm runs alike;
/// ordered after `Setup Mise`, a component-less restored toolchain
/// (upstream jdx/mise-action#215) is repaired before any obligation.
/// Pinned at IR level (unconditionality) and YAML level (final order).
#[test]
fn rust_components_install_unconditionally_after_restore() -> TestResult {
    for mbx in [false, true] {
        let repo = make_workspace(&lock_for(&["a", "b"]), mbx)?;
        let prep = prepare(repo.path())?;
        let jobs = finalized_jobs(&prep)?;
        let mut rust_jobs = 0;
        for (id, job) in &jobs {
            if !id.starts_with("rust-") {
                continue;
            }
            rust_jobs += usize::from(id.starts_with("rust-"));
            let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
            let missing =
                |what: &str| std::io::Error::other(format!("{id} misses {what} (mbx={mbx})"));
            let setup = names
                .iter()
                .position(|name| *name == SETUP_MISE_NAME)
                .ok_or_else(|| missing("setup"))?;
            let components = names
                .iter()
                .position(|name| *name == PREPARE_RUST_COMPONENTS_STEP)
                .ok_or_else(|| missing("components"))?;
            assert!(
                setup < components,
                "{id}: components install after restore (mbx={mbx})"
            );
            let step = &job.steps[components];
            assert!(
                step.condition.is_none(),
                "{id}: unconditional install runs cold AND warm (mbx={mbx})"
            );
            let StepKind::Shell { run, .. } = &step.kind else {
                return Err(missing("shell components payload").into());
            };
            for token in [
                "rustup",
                "component",
                "add",
                "--toolchain",
                "clippy",
                "rustfmt",
            ] {
                assert!(
                    run.join(" ").contains(token),
                    "{id} payload misses {token} (mbx={mbx})"
                );
            }
        }
        assert_eq!(rust_jobs, 2, "both fixture crates watched (mbx={mbx})");
        let tree = render_staged_tree(&prep)?;
        let yaml = tree
            .get(WORKFLOW_PATH)
            .ok_or_else(|| std::io::Error::other("missing workflow"))?;
        let emitted = yaml_steps(yaml);
        for id in ["rust-a", "rust-b"] {
            let names = emitted
                .get(id)
                .ok_or_else(|| std::io::Error::other(format!("missing {id}")))?;
            let at = |want: &str| {
                names
                    .iter()
                    .position(|name| name == want)
                    .ok_or_else(|| std::io::Error::other(format!("{id} misses {want}")))
            };
            assert!(
                at(SETUP_MISE_NAME)? < at(PREPARE_RUST_COMPONENTS_STEP)?,
                "{id}: emitted components follow restore (mbx={mbx})"
            );
            if id.starts_with("rust-") {
                assert!(
                    at(PREPARE_RUST_COMPONENTS_STEP)? < at("Clippy")?,
                    "{id}: components precede obligations (mbx={mbx})"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn lockfile_delta_changes_cache_identity() -> TestResult {
    let lock = lock_for(&["a", "b"]);
    // Compiler driver does not change the exact Cargo sources archive.
    let mbx_keys = cache_keys(&yaml_for(&lock, true)?);
    let cargo_keys = cache_keys(&yaml_for(&lock, false)?);
    assert!(
        mbx_keys
            .iter()
            .any(|key| key.contains("velnor-v1-sources-")),
        "mbx snapshot: {mbx_keys:?}"
    );
    assert!(
        cargo_keys
            .iter()
            .any(|key| key.contains("velnor-v1-sources-")),
        "cargo sources key: {cargo_keys:?}"
    );
    assert_eq!(
        mbx_keys
            .iter()
            .find(|key| key.contains("velnor-v1-sources-")),
        cargo_keys
            .iter()
            .find(|key| key.contains("velnor-v1-sources-")),
        "same source identity in both driver modes"
    );
    // Lock CONTENT deltas re-key at runtime through `hashFiles`: the
    // template is stable across renders, the resolved key is not.
    for key in &mbx_keys {
        if key.contains("velnor-v1-sources-") {
            assert!(key.contains("hashFiles("), "content-pinned: {key}");
        }
    }
    // Lockless emits no sources cache at all: removing the lock removes reuse.
    let repo = make_workspace(&lock, true)?;
    fs::remove_file(repo.path().join("Cargo.lock"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    assert!(
        !yaml.contains("Restore Cargo sources"),
        "lockless restores nothing"
    );
    assert!(
        !yaml.contains("Save Cargo sources"),
        "lockless saves nothing"
    );
    Ok(())
}
