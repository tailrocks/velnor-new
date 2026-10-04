//! Canonical tool closure transport and restore ordering.

use super::*;

#[test]
fn complete_tool_payload_restores_before_install_and_excludes_source_credentials() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(repo.path())?;
    let prep = prepare(repo.path())?;
    for (id, job) in &prep.workflow.ir.jobs {
        let Some(restore_at) = job
            .steps
            .iter()
            .position(|s| s.name == "Restore Mise tools")
        else {
            continue;
        };
        let StepKind::Action {
            uses,
            with: restored,
            ..
        } = &job.steps[restore_at].kind
        else {
            return Err(format!("{id}: restore not action").into());
        };
        assert_eq!(
            uses,
            "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
        );
        let expected = [
            "${{ runner.temp }}/velnor/mise",
            "${{ runner.temp }}/velnor/rustup",
            "${{ runner.temp }}/velnor/cargo/bin",
            "${{ runner.temp }}/velnor/cargo/.crates.toml",
            "${{ runner.temp }}/velnor/cargo/.crates2.json",
        ]
        .join("\n");
        assert_eq!(
            restored.get("path"),
            Some(&expected),
            "{id}: full Rust closure"
        );
        for banned in ["registry", "git/db", "credentials", "~", ".."] {
            assert!(
                !expected.contains(banned),
                "{id}: forbidden payload {banned}"
            );
        }
        for (at, step) in job.steps.iter().enumerate() {
            if let StepKind::Shell { run, env } = &step.kind {
                if run
                    .iter()
                    .any(|arg| arg == "mise" || arg.contains("mise install"))
                {
                    assert!(
                        restore_at < at,
                        "{id}: restore precedes installation/verification"
                    );
                    assert_eq!(
                        env.get("MISE_DATA_DIR").map(String::as_str),
                        Some("${{ runner.temp }}/velnor/mise")
                    );
                }
            }
            if step.name == "Save Mise tools" {
                assert_tool_save(id, step, restored)?;
            }
        }
    }
    Ok(())
}

#[test]
fn source_transport_owns_only_source_subset_and_fills_non_exact_hits() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(repo.path())?;
    let prep = prepare(repo.path())?;
    let expected = [
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ]
    .join("\n");
    let mut savers = 0;
    for (id, job) in &prep.workflow.ir.jobs {
        for step in &job.steps {
            if !matches!(
                step.name.as_str(),
                "Restore Cargo sources" | "Save Cargo sources"
            ) {
                continue;
            }
            let StepKind::Action { with, .. } = &step.kind else {
                return Err("source cache not action".into());
            };
            assert_eq!(
                with.get("path"),
                Some(&expected),
                "{id}: sources cannot own tools or credentials"
            );
            if step.name == "Save Cargo sources" {
                savers += 1;
                assert_eq!(id, "plan", "only source producer saves");
                let gate = step.condition.as_deref().ok_or("ungated source save")?;
                assert!(gate.contains("success() && github.event_name == 'push'"));
                assert!(gate.contains("github.event.repository.default_branch"));
                assert!(gate.contains("steps.velnor-sources-cache.outputs.cache-hit != 'true'"));
            } else {
                assert!(
                    step.condition.is_none(),
                    "prefix/exact source restores remain available"
                );
            }
        }
    }
    assert_eq!(savers, 1);
    Ok(())
}

fn assert_tool_save(
    id: &str,
    step: &velnor_actions_contract::Step,
    restored: &std::collections::BTreeMap<String, String>,
) -> TestResult {
    let StepKind::Action {
        uses, with: saved, ..
    } = &step.kind
    else {
        return Err(format!("{id}: save not action").into());
    };
    assert_eq!(
        uses,
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
    );
    assert_eq!(
        saved.get("path"),
        restored.get("path"),
        "{id}: hidden-version input"
    );
    assert_eq!(saved.get("key"), restored.get("key"), "{id}: visible key");
    let gate = step.condition.as_deref().ok_or("ungated tools save")?;
    assert!(gate.contains("github.event_name == 'push'"));
    assert!(gate.contains("github.event.repository.default_branch"));
    assert!(gate.contains("steps.velnor-tools-cache.outputs.cache-hit != 'true'"));
    Ok(())
}
