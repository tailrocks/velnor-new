// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const ASSETS: &str = "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\ndest = 'tests/fixtures/dataset'\n";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

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

fn assert_tokens_first(label: &str, text: &str) -> TestResult {
    let mut fetching = 0;
    for job in jobs(text)? {
        let steps = job["steps"].as_array().ok_or("a job without steps")?;
        let Some(code) = steps.iter().position(runs_checkout_code) else {
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
