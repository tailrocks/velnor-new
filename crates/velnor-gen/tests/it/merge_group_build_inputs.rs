// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NAME: &str = "Prepare the fixture";
const ROOTS: [&str; 3] = ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];
const HELPERS: [&str; 2] = ["0.1.109", "0.1.112"];

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

fn condition(step: &Value) -> &str {
    step["if"].as_str().unwrap_or_default()
}

fn skipped(step: &Value) -> bool {
    condition(step).contains(".skip != 'true'") || condition(step).contains(".skip_push != 'true'")
}

fn post_merge_only(step: &Value) -> bool {
    condition(step).contains("github.event_name == 'push'")
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

fn check(ci: &str, helper: &str) -> TestResult {
    let mut dropped = 0;
    for (shape, root) in [
        ("one-job", "tools/a"),
        ("lint-test", "tools/a"),
        ("groups", "tools/a"),
        ("one-job", "."),
    ] {
        let repo = private_repo(&ROOTS, &config(shape, root, ci), helper)?;
        for job in jobs(&workflow(&repo)?)? {
            let inputs: Vec<&Value> = steps(&job).iter().filter(|s| feeds_build(s)).collect();
            if inputs.is_empty() {
                continue;
            }
            let survives = steps(&job).iter().any(|s| compiles(s) && !skipped(s));
            let before_merge = steps(&job)
                .iter()
                .any(|s| compiles(s) && !post_merge_only(s));
            for input in inputs {
                assert_eq!(
                    !skipped(input),
                    survives,
                    "{helper} {shape} {}: {input}",
                    job["name"]
                );
                assert!(
                    !before_merge || !post_merge_only(input),
                    "{helper} {shape} {}: {input}",
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
    for helper in HELPERS {
        check("", helper)?;
    }
    Ok(())
}

#[test]
fn the_build_inputs_stay_for_a_compiling_step_that_runs_only_after_the_merge() -> TestResult {
    for helper in HELPERS {
        check(
            "docs = \"post_merge\"\ncheck_alone = \"post_merge\"\n",
            helper,
        )?;
    }
    Ok(())
}

fn with_lib(mut repo: RepoView, manifest: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let metadata = repo
        .rust_metadata
        .get_mut(&RepoPath::new(manifest)?)
        .ok_or("no metadata")?;
    for member in &mut metadata.members {
        member.targets.push(Target {
            name: TargetName::new("lib")?,
            kinds: vec![TargetKind::Lib],
            tests: false,
            doctest: false,
            source: None,
            required_features: Vec::new(),
            doc: true,
            bench: true,
        });
    }
    Ok(repo)
}

fn setup_skipped(helper: &str, lib: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let repo = with_lib(
        private_repo(
            &ROOTS,
            &config("one-job", "tools/a", "check_alone = \"post_merge\"\n"),
            helper,
        )?,
        lib,
    )?;
    let job = jobs(&workflow(&repo)?)?
        .into_iter()
        .find(|job| steps(job).iter().any(|step| step["name"] == NAME))
        .ok_or("no job holds the setup")?;
    let check_alone = steps(&job)
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo check"))
        })
        .ok_or("no check-alone step")?;
    assert!(
        post_merge_only(check_alone) && !skipped(check_alone),
        "{check_alone}"
    );
    let setup = steps(&job)
        .iter()
        .find(|step| step["name"] == NAME)
        .ok_or("no setup step")?;
    Ok(skipped(setup))
}

#[test]
fn a_setup_waits_for_a_post_merge_compile_of_its_own_root_from_helper_0_1_112() -> TestResult {
    assert!(!setup_skipped("0.1.109", "tools/b/Cargo.toml")?);
    assert!(!setup_skipped("0.1.111", "tools/b/Cargo.toml")?);
    assert!(setup_skipped("0.1.112", "tools/b/Cargo.toml")?);
    assert!(!setup_skipped("0.1.112", "tools/a/Cargo.toml")?);
    Ok(())
}
