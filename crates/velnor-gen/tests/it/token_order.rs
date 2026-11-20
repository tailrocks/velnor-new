// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const ASSETS: &str = "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\ndest = 'tests/fixtures/dataset'\n";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

const PLAIN_ACTIONS: &[&str] = &[
    "actions/checkout@",
    "actions/download-artifact@",
    "actions/upload-artifact@",
    "actions/cache/restore@",
    "actions/cache/save@",
    "tailrocks/velnor-new/actions/setup-velnor@",
];
const HELPER_COMMANDS: &[&str] = &["cache", "gate", "plan", "report", "tools-verify"];

fn holds_a_token(step: &Value) -> bool {
    let rendered = step.to_string();
    ["GITHUB_TOKEN", "private-key", "VELNOR_PRIVATE_TOKEN"]
        .iter()
        .any(|token| rendered.contains(token))
}

fn runs_checkout_code(step: &Value) -> bool {
    let run = step["run"].as_str().unwrap_or_default();
    step["uses"]
        .as_str()
        .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
        || run.contains("\"$VELNOR_EXEC\" run --")
        || run.contains("\"$VELNOR_EXEC\" prepare")
        || run.contains("\"$VELNOR_EXEC\" check-metadata")
        || run.starts_with("rustup ")
        || step["name"] == "Prepare the fixture"
}

fn helper_only(step: &Value) -> bool {
    if let Some(uses) = step["uses"].as_str() {
        return PLAIN_ACTIONS.iter().any(|action| uses.starts_with(action));
    }
    let run = step["run"].as_str().unwrap_or_default().trim_end();
    !run.contains('\n')
        && (run.starts_with("eval \"$VELNOR_HELPER_INSTALL_")
            || HELPER_COMMANDS
                .iter()
                .any(|command| run.starts_with(&format!(". \"$VELNOR_EXEC\" {command} "))))
}

fn assert_tokens_first(label: &str, text: &str) -> TestResult {
    let mut fetching = 0;
    for job in jobs(text)? {
        let steps = job["steps"].as_array().ok_or("a job without steps")?;
        let Some(code) = steps.iter().position(runs_checkout_code) else {
            if let Some(unknown) = steps.iter().find(|step| !helper_only(step)) {
                return Err(format!(
                    "{label}: job {} has no recognised step that runs checkout code, and {unknown} is not a helper-only step",
                    job["name"]
                )
                .into());
            }
            continue;
        };
        if let Some(late) = steps[code..].iter().find(|step| holds_a_token(step)) {
            return Err(format!("{label}: {late} runs after {}", steps[code]).into());
        }
        fetching += usize::from(
            steps[..code]
                .iter()
                .any(|step| step.to_string().contains("private-token.outputs.token")),
        );
    }
    assert!(fetching > 0, "{label}: no job fetches before checkout code");
    Ok(())
}

#[test]
fn no_job_holds_a_token_after_a_step_that_runs_checkout_code() -> TestResult {
    for shape in ["", "shape = 'groups'\n", "shape = 'lint-test'\n"] {
        for extra in ["", ASSETS, SETUP] {
            let config = format!("{shape}{SECTION}{extra}");
            let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], &config, "0.1.111")?;
            assert_tokens_first(&config, &workflow(&repo)?)?;
        }
    }
    let mut consumer = private_repo(&["Cargo.toml"], SECTION, "0.1.111")?;
    consumer
        .files
        .insert(path("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
    assert_tokens_first("private base caches", &workflow(&consumer)?)
}

#[test]
fn nothing_restores_the_tool_directory_before_the_fetch() -> TestResult {
    let mut fetches = 0;
    for shape in ["", "shape = 'groups'\n", "shape = 'lint-test'\n"] {
        let config = format!("{shape}{SECTION}");
        let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], &config, "0.1.111")?;
        for job in jobs(&workflow(&repo)?)? {
            let steps = job["steps"].as_array().ok_or("a job without steps")?;
            let Some(token) = steps.iter().position(|step| step["id"] == "private-token") else {
                continue;
            };
            let fetch = token + 1;
            fetches += 1;
            for step in &steps[..fetch] {
                let rendered = step.to_string();
                assert!(
                    !rendered.contains(".local/share/mise") && !rendered.contains("mise-action"),
                    "{config}: {rendered} precedes the fetch"
                );
            }
        }
    }
    assert!(fetches > 0, "no job fetches");
    Ok(())
}

#[test]
fn a_job_that_runs_cargo_without_a_recognised_checkout_code_step_fails_the_check() {
    let workflow = "jobs:\n  odd:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo build --locked\n";
    let refused = assert_tokens_first("odd", workflow).err().map(|e| e.to_string());
    assert!(
        refused
            .as_deref()
            .is_some_and(|why| why.contains("no recognised step")),
        "{refused:?}"
    );
}
