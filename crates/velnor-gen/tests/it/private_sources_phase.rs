// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";
const HARNESS: &[u8] = b"[[test]]\nname = \"it\"\nharness = false\n";

fn index(steps: &[serde_json::Value], name: &str) -> Result<usize, String> {
    steps
        .iter()
        .position(|step| step["name"] == name || step["id"] == name)
        .ok_or_else(|| format!("no step {name}: {steps:?}"))
}

fn two_direct_harnesses() -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    two_direct_harnesses_with(&format!("shape = 'groups'\n{SECTION}"))
}

fn two_direct_harnesses_with(
    config: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], config, "0.1.88")?;
    let root = path("Cargo.toml")?;
    let second = RepoPath::new("crates/b/Cargo.toml")?;
    let mut member = repo.rust_metadata[&root].members[0].clone();
    member.name = velnor_model::PackageName::new("package-b")?;
    member.manifest = second.clone();
    for (manifest, name) in [(root.clone(), "package-0"), (second.clone(), "package-b")] {
        let head = format!("[package]\nname = \"{name}\"\n");
        repo.tracked_paths.insert(manifest.clone());
        repo.files
            .insert(manifest, [head.as_bytes(), HARNESS].concat());
    }
    repo.rust_metadata
        .get_mut(&root)
        .ok_or("no metadata")?
        .members
        .push(member);
    Ok(repo)
}

#[test]
fn the_token_and_the_fetch_come_before_a_repository_test_setup_command() -> TestResult {
    let repo = two_direct_harnesses_with(&format!("shape = 'lint-test'\n{SECTION}{SETUP}"))?;
    let mut checked = 0;
    for job in jobs(&workflow(&repo)?)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let Ok(setup) = index(steps, "Prepare the fixture") else {
            continue;
        };
        checked += 1;
        let token = index(steps, "private-token")?;
        let fetch = steps
            .iter()
            .position(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
            })
            .ok_or("no fetch step")?;
        assert!(token < fetch && fetch < setup, "{token} {fetch} {setup}");
        assert!(
            !steps[setup].to_string().contains("private-token"),
            "the setup command must not see the token: {}",
            steps[setup]
        );
    }
    assert_eq!(checked, 1);
    Ok(())
}

#[test]
fn the_phase_runs_when_any_direct_harness_runs_not_only_the_first_one() -> TestResult {
    let text = workflow(&two_direct_harnesses()?)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let direct = steps
            .iter()
            .filter(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo test -p package-"))
            })
            .count();
        if direct < 2 {
            continue;
        }
        checked += 1;
        for step in steps.iter().filter(|step| {
            step["id"] == "private-token"
                || step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
        }) {
            let guard = step["if"].as_str().unwrap_or_default();
            assert!(
                !guard.contains("package-0") && !guard.contains("package-b"),
                "one package's gate cannot decide for the others: {step}"
            );
        }
    }
    assert!(checked > 0, "{text}");
    Ok(())
}

#[test]
fn bot_pull_requests_get_no_token_and_the_fetch_learns_the_server() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], SECTION, "0.1.88")?;
    let mut checked = 0;
    for job in jobs(&workflow(&repo)?)? {
        for step in job["steps"].as_array().into_iter().flatten() {
            if step["id"] == "private-token" {
                checked += 1;
                let guard = step["if"].as_str().unwrap_or_default();
                for bot in [
                    "!endsWith(github.actor, '[bot]')",
                    "!endsWith(github.event.pull_request.user.login, '[bot]')",
                ] {
                    assert!(guard.contains(bot), "{guard}");
                }
            }
            if step["run"]
                .as_str()
                .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
            {
                assert_eq!(step["env"]["VELNOR_SERVER_URL"], "${{ github.server_url }}");
            }
        }
    }
    assert!(checked > 0);
    Ok(())
}
