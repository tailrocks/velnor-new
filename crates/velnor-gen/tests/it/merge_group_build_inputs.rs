// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NAME: &str = "Prepare the fixture";

const GROUP: &str = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";

fn config(shape: &str, root: &str, ci: &str) -> String {
    let group = if shape == "groups" { "" } else { GROUP };
    format!(
        "shape = '{shape}'\n{ci}{group}{SECTION}[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '{SHA}'\ndest = 'tests/fixtures/dataset'\n[[test_setup]]\nroot = '{root}'\nname = '{NAME}'\nrun = 'bash scripts/prepare-fixture.sh'\nphase = 'build'\n[ci]\nskip_push_after_merge_group = true\n"
    )
}

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

fn skipped(step: &Value) -> bool {
    step["if"]
        .as_str()
        .is_some_and(|condition| condition.contains("skip"))
}

fn compiles(step: &Value) -> bool {
    step["run"].as_str().is_some_and(|run| {
        run.starts_with(". \"$VELNOR_EXEC\" run -- ")
            && !["cargo-fmt", "cargo-deny", "cargo-machete"]
                .iter()
                .any(|tool| run.contains(tool))
    })
}

fn feeds_build(step: &Value) -> bool {
    step["name"] == NAME
        || step["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("release-asset"))
}

fn check(ci: &str) -> TestResult {
    let mut dropped = 0;
    let roots = ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];
    for (shape, root) in [
        ("one-job", "tools/a"),
        ("lint-test", "tools/a"),
        ("groups", "tools/a"),
        ("one-job", "."),
    ] {
        let repo = private_repo(&roots, &config(shape, root, ci), "0.1.109")?;
        for job in jobs(&workflow(&repo)?)? {
            let inputs: Vec<&Value> = steps(&job).iter().filter(|s| feeds_build(s)).collect();
            if inputs.is_empty() {
                continue;
            }
            let survives = steps(&job).iter().any(|s| compiles(s) && !skipped(s));
            for input in inputs {
                assert_eq!(
                    !skipped(input),
                    survives,
                    "{shape} {}: {input}",
                    job["name"]
                );
            }
            dropped += usize::from(!survives);
        }
    }
    assert!(dropped > 0);
    Ok(())
}

#[test]
fn the_build_inputs_run_after_a_merge_group_exactly_when_a_compiling_step_of_the_job_does()
-> TestResult {
    check("")
}

#[test]
fn the_build_inputs_stay_for_a_compiling_step_that_runs_only_after_the_merge() -> TestResult {
    check("docs = \"post_merge\"\ncheck_alone = \"post_merge\"\n")
}
