// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{DependencyKind, PackageDependency, PackageName, RepoPath};

use crate::facts::TestResult;
use crate::gate_eval::{Context, holds};
use crate::pipeline::{pin_helper, view, workflow};
use crate::yaml_tree::document;

const NAME: &str = "Prepare the fixture";

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

fn compiles(step: &Value) -> bool {
    let run = step["run"].as_str().unwrap_or_default();
    run.contains("cargo")
        && !["cargo-fmt", "cargo-machete", "cargo-deny"]
            .iter()
            .any(|tool| run.contains(tool))
}

#[test]
fn a_coalesced_build_phase_setup_runs_when_any_root_that_needs_it_is_selected() -> TestResult {
    const GROUP: &str =
        "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/b\", \"tools/c\"]\n";
    const ROOTS: [&str; 4] = [
        "Cargo.toml",
        "lib/Cargo.toml",
        "tools/b/Cargo.toml",
        "tools/c/Cargo.toml",
    ];
    for shape in ["one-job", "lint-test"] {
        let config = format!(
            "shape = '{shape}'\n{GROUP}[[test_setup]]\nroot = 'lib'\nname = '{NAME}'\nrun = 'bash scripts/prepare-fixture.sh'\nphase = 'build'\n"
        );
        let mut repo = view(&ROOTS, Some(config.as_bytes()))?;
        pin_helper(&mut repo, "0.1.109")?;
        for dependent in ["tools/b/Cargo.toml", "tools/c/Cargo.toml"] {
            repo.rust_metadata
                .get_mut(&RepoPath::new(dependent)?)
                .ok_or("no metadata")?
                .members[0]
                .dependencies
                .push(PackageDependency {
                    default_features: true,
                    name: PackageName::new("package-1")?,
                    kind: DependencyKind::Normal,
                    platform: None,
                    optional: false,
                    manifest: Some(RepoPath::new("lib/Cargo.toml")?),
                    features: Vec::new(),
                    rename: None,
                });
        }
        let parsed = document(&workflow(&repo)?)?;
        let mut checked = 0;
        for (id, job) in parsed["jobs"].as_object().into_iter().flatten() {
            let grouped = steps(job).iter().any(|step| {
                compiles(step)
                    && step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("tools/b/Cargo.toml"))
            });
            if !grouped {
                continue;
            }
            for step in steps(job).iter().filter(|step| step["name"] == NAME) {
                checked += 1;
                let condition = step["if"]
                    .as_str()
                    .and_then(|text| text.strip_prefix("${{ "))
                    .and_then(|text| text.strip_suffix(" }}"))
                    .ok_or("setup without a condition")?;
                for (selected, wanted) in [
                    ([true, false], true),
                    ([false, true], true),
                    ([true, true], true),
                    ([false, false], false),
                ] {
                    let mut context = Context::new(&[
                        ("steps.plan.outcome", "success"),
                        ("steps.restored.outcome", "success"),
                        ("github.event_name", "pull_request"),
                    ]);
                    for (slug, on) in ["tools_b", "tools_c"].iter().zip(selected) {
                        for source in ["steps.plan", "needs.rust-plan"] {
                            context.set(
                                &format!("{source}.outputs.rust_{slug}_mode"),
                                if on { "full" } else { "none" },
                            );
                        }
                    }
                    assert_eq!(
                        holds(condition, &context)?,
                        wanted,
                        "{shape} {id} {selected:?}: {condition}"
                    );
                }
            }
        }
        assert_ne!(checked, 0, "{shape}: no setup was rendered");
    }
    Ok(())
}
