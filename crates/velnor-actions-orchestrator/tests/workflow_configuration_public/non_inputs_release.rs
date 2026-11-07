// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::non_input_checkout::NON_INPUT_GUARD_STEP;

use super::non_inputs_checkout_support::exclusion;
use super::scoped_workspace::{TestResult, put};
use super::workflow_configuration_fixture::{git, make_repo};

const WORKFLOWS: [&str; 2] = ["binary-preview.yml", "binary-release.yml"];

const EXCLUDED: [(&str, &str); 4] = [
    ("binary-preview.yml", "preview-payload"),
    ("binary-preview.yml", "preview-assemble"),
    ("binary-release.yml", "stable-payload-000"),
    ("binary-release.yml", "stable-assemble"),
];

const FULL_TREE: [(&str, &str); 3] = [
    ("binary-preview.yml", "preview-eligibility"),
    ("binary-preview.yml", "preview-tap-prepare"),
    ("binary-release.yml", "stable-prepare"),
];

const STEP_START: &str = "\n      - ";

pub(super) const CONFIG: &str = r#"schema = 2
[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "main"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
[stacks.rust]
[stacks.rust.binary_release]
manifest_path = "Cargo.toml"
version_package = "demo"
version_env = "PROBE_VERSION_OVERRIDE"
[stacks.rust.binary_release.preview]
enabled = true
[stacks.rust.binary_release.stable]
enabled = true
[stacks.rust.release]
enabled = true
registry = "git-tag"
packages = ["demo"]
manifest_path = "Cargo.toml"
environment = "release"
tag_name = "v{{ version }}"
[stacks.rust.binary_release.producer]
task = "release:package"
source = { mise_config = "mise.toml", working_directory = "." }
inputs = ["Cargo.toml", "Cargo.lock", "mise.toml"]
tools = ["rust"]
timeout_minutes = 1
output_directory = "dist/release"
manifest = "dist/release/output.json"
[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"
[[stacks.rust.binary_release.products]]
id = "demo"
package = "demo"
bins = ["demo"]
targets = ["x86_64-unknown-linux-gnu"]
archive = "tar.gz"
"#;

const TAP: &str = r#"[distribution.homebrew_tap]
enabled = true
repository = "example/homebrew-probe"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
[distribution.homebrew_tap.destinations.preview]
formula = "Formula/probe.rb"
[distribution.homebrew_tap.updater]
task = "tap:update"
source = { mise_config = "mise.toml", working_directory = "." }
runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }
inputs = ["mise.toml", "Formula/probe.rb"]
tools = ["ruby"]
timeout_minutes = 1
[distribution.homebrew_tap.protection_observer]
app_id_variable = "TAP_OBSERVER_APP_ID"
app_private_key_secret = "TAP_OBSERVER_APP_PRIVATE_KEY"
[distribution.homebrew_tap.review_completion]
check_name = "Review completion"
app_id_variable = "REVIEW_APP_ID"
[[distribution.homebrew_tap.review_completion.expected_reviewers]]
app_id_variable = "CODEX_REVIEWER_APP_ID"
"#;

pub(super) const QUALIFIED_RUST: &str = r#"[[qualified_tools]]
id = "rust"
backend = { kind = "core", tool = "rust" }
version = "1.99.1"
options = { kind = "rust", components = [], targets = [] }
depends_on = []
[[qualified_tools.platforms]]
platform = "linux_x64"
artifacts = [
  { url = "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "53f3521949404d595efb06a55c12c5658d4ed1c75566f33d3d514eaebe70bd8a" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rust-std-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "109b074225bf5b9ea3820890876bfdf4ed161adbfaf94b2b4b5984c85ee5d958" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rustc-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "2ff05ff9c08a90861e7f6e4065b40a8b87463b4576186fd613871c3e37f2091f" },
]
dependency_artifacts = []
install_tree_sha256 = "baa8089f830b6c49980b5c2274a0863d963d2eae8ef7f8ece56b90dfae8a8625"
[[qualified_tools.platforms.executables]]
name = "cargo"
path = "bin/cargo"
sha256 = "a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af"
probe = { kind = "version", expected = "cargo 1.99.1 (5f94df478 2026-08-27)" }
[[qualified_tools.platforms.executables]]
name = "rustc"
path = "bin/rustc"
sha256 = "b53b780f77f952062f8ca8a3b70aef2390b1e45ad91939be94afdc9543026853"
probe = { kind = "rustc_verbose", expected = "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\ncommit-date: 2026-10-13\nhost: x86_64-unknown-linux-gnu\nrelease: 1.99.1\nLLVM version: 23.1.1" }
"#;

fn release_workflows(non_inputs: Option<&str>) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let rust = non_inputs.map_or_else(String::new, |glob| format!("non_inputs = [\"{glob}\"]\n"));
    let config = CONFIG.replace("[stacks.rust]\n", &format!("[stacks.rust]\n{rust}"));
    let repo = make_repo(&format!("{config}\n{TAP}\n{QUALIFIED_RUST}"))?;
    let root = repo.path();
    fs::remove_dir_all(root.join("src"))?;
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"2\"\n",
    )?;
    put(
        root,
        "crates/demo/Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    put(root, "crates/demo/src/main.rs", "fn main() {}\n")?;
    put(
        root,
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    put(
        root,
        "mise.toml",
        "[tasks.\"release:package\"]\nrun = \"true\"\n\n[tasks.\"tap:update\"]\nrun = \"true\"\n",
    )?;
    put(root, "Formula/probe.rb", "class Probe < Formula; end\n")?;
    put(root, "docs/a.md", "a\n")?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/probe.git",
        ],
        root,
    )?;
    git(&["add", "-A"], root)?;
    git(&["commit", "-m", "release fixture"], root)?;
    let tree = render_staged_tree(&prepare(root)?)?;
    WORKFLOWS
        .iter()
        .map(|name| {
            let path = format!(".github/workflows/{name}");
            let text = tree
                .get(&path)
                .ok_or_else(|| format!("{path} is missing"))?;
            Ok(((*name).to_owned(), text.to_owned()))
        })
        .collect()
}

fn jobs_of(workflow: &str) -> Vec<(String, Vec<String>)> {
    let Some((_, jobs)) = workflow.split_once("\njobs:\n") else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = Vec::new();
    for line in jobs.lines() {
        if let Some(id) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|id| !id.is_empty() && !id.starts_with(' '))
        {
            found.push((id.to_owned(), String::new()));
        } else if let Some((_, text)) = found.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    found
        .into_iter()
        .map(|(id, text)| {
            let steps = text
                .split(STEP_START)
                .skip(1)
                .map(|step| format!("{STEP_START}{step}"))
                .collect();
            (id, steps)
        })
        .collect()
}

fn is_checkout(step: &str) -> bool {
    step.contains("uses: actions/checkout@")
}

fn carries_exclusion(step: &str, glob: &str) -> bool {
    step.contains(&format!(
        "sparse-checkout: \"{}\"",
        exclusion(glob).replace('\n', "\\n")
    )) && step.contains("sparse-checkout-cone-mode: false")
        && !step.contains("\n        if:")
}

#[test]
fn release_producer_checkouts_exclude_non_inputs_and_carry_the_guard() -> TestResult {
    let glob = "docs/**";
    let mut seen = Vec::new();
    for (file, workflow) in release_workflows(Some(glob))? {
        for (job, steps) in jobs_of(&workflow) {
            for (at, step) in steps
                .iter()
                .enumerate()
                .filter(|(_, step)| is_checkout(step))
            {
                let key = (file.as_str(), job.as_str());
                seen.push((file.clone(), job.clone()));
                let foreign =
                    step.contains("\n          repository:") || step.contains("\n          path:");
                let next = steps.get(at + 1);
                if EXCLUDED.contains(&key) {
                    assert!(
                        carries_exclusion(step, glob),
                        "{file}/{job}: checkout lacks the exclusion"
                    );
                    let guard =
                        next.ok_or_else(|| format!("{file}/{job}: no step after the checkout"))?;
                    assert!(
                        guard.contains(NON_INPUT_GUARD_STEP)
                            && guard.contains(&format!(":(glob){glob}"))
                            && !guard.contains("\n        if:"),
                        "{file}/{job}: guard does not follow the checkout"
                    );
                } else if FULL_TREE.contains(&key) {
                    assert!(
                        !step.contains("sparse-checkout"),
                        "{file}/{job}: excludes paths"
                    );
                    assert!(
                        !steps
                            .iter()
                            .any(|other| other.contains(NON_INPUT_GUARD_STEP)),
                        "{file}/{job}: carries the guard"
                    );
                } else {
                    return Err(format!(
                        "{file}/{job}: checkout job has no row in the release table"
                    )
                    .into());
                }
                assert!(
                    !foreign || !step.contains("sparse-checkout"),
                    "{file}/{job}: foreign checkout excludes paths"
                );
            }
        }
    }
    for (file, workflow) in release_workflows(None)? {
        assert!(!workflow.contains("sparse-checkout"), "{file}");
        assert!(!workflow.contains(NON_INPUT_GUARD_STEP), "{file}");
    }
    for key in EXCLUDED.iter().chain(&FULL_TREE) {
        assert!(
            seen.iter()
                .any(|(file, job)| (file.as_str(), job.as_str()) == *key),
            "the fixture never rendered {key:?}"
        );
    }
    Ok(())
}

#[test]
fn stable_select_step_carries_release_status_token() -> TestResult {
    let workflows = release_workflows(None)?;
    let (_, workflow) = workflows
        .iter()
        .find(|(name, _)| name.as_str() == "binary-release.yml")
        .ok_or("binary-release.yml is missing")?;
    let (_, steps) = jobs_of(workflow)
        .into_iter()
        .find(|(job, _)| job == "stable-eligibility")
        .ok_or("stable-eligibility is missing")?;
    let step = steps
        .iter()
        .find(|step| step.contains("select-stable-release-source-v1"))
        .ok_or("stable select step is missing")?;
    assert!(
        step.contains("VELNOR_RELEASE_STATUS_TOKEN: ${{ secrets.RELEASE_STATUS_READ_TOKEN }}"),
        "stable select step lacks the status token binding"
    );
    assert!(
        step.contains("GH_TOKEN: ${{ github.token }}"),
        "stable select step lost the write token binding"
    );
    Ok(())
}

#[test]
fn stable_prepare_step_does_not_cap_file_size_during_tool_exec() -> TestResult {
    let workflows = release_workflows(None)?;
    let (_, workflow) = workflows
        .iter()
        .find(|(name, _)| name.as_str() == "binary-release.yml")
        .ok_or("binary-release.yml is missing")?;
    let (_, steps) = jobs_of(workflow)
        .into_iter()
        .find(|(job, _)| job == "stable-prepare")
        .ok_or("stable-prepare is missing")?;
    let step = steps
        .iter()
        .find(|step| step.contains("Prepare the version PR with qualified release-plz"))
        .ok_or("stable prepare release-plz step is missing")?;
    assert!(
        !step.contains("ulimit -f"),
        "stable prepare step must not set ulimit -f around mise exec"
    );
    Ok(())
}
