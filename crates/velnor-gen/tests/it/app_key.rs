// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::{CONFIG_PATH, LOCK_PATH};

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};
use crate::release_fixture::{APP, LOCK, release_view};
use crate::token_order::{ASSETS, SETUP, runs_checkout_code};

const KEY: &str = "secrets.DEPS_APP_PRIVATE_KEY";

fn mints_with_the_helper(run: &str) -> bool {
    ["velnor-ci app-token ", ". \"$VELNOR_EXEC\" app-token "]
        .iter()
        .any(|prefix| run.starts_with(prefix))
        && !run.contains('\n')
}

fn key_steps(steps: &[Value]) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.to_string().contains(KEY))
        .map(|(index, _)| index)
        .collect()
}

fn assert_key_only_in_the_mint_step(label: &str, text: &str) -> TestResult {
    assert!(
        !text.contains("create-github-app-token") && !text.contains("private-key"),
        "{label}: an action receives the App key"
    );
    let mut mints = 0;
    for job in jobs(text)? {
        let steps = job["steps"].as_array().ok_or("a job without steps")?;
        let code = steps.iter().position(runs_checkout_code);
        for index in key_steps(steps) {
            let step = &steps[index];
            assert!(step["uses"].is_null(), "{label}: {step}");
            let run = step["run"].as_str().unwrap_or_default();
            assert!(mints_with_the_helper(run), "{label}: {step}");
            assert!(
                !run.contains(KEY),
                "{label}: the key is on the command line: {step}"
            );
            assert_eq!(
                step["env"]["VELNOR_APP_PRIVATE_KEY"],
                format!("${{{{ {KEY} }}}}"),
                "{label}: {step}"
            );
            assert!(
                code.is_some_and(|code| index < code),
                "{label}: {step} does not run before checkout code"
            );
            mints += 1;
        }
    }
    assert!(mints > 0, "{label}: no job mints a token");
    Ok(())
}

#[test]
fn at_helper_0_1_112_only_the_helper_mint_step_before_checkout_code_receives_the_app_key()
-> TestResult {
    for shape in ["", "shape = 'groups'\n", "shape = 'lint-test'\n"] {
        for extra in ["", ASSETS, SETUP] {
            let config = format!("{shape}{SECTION}{extra}");
            let repo = private_repo(&["Cargo.toml"], &config, "0.1.112")?;
            assert_key_only_in_the_mint_step(&config, &workflow(&repo)?)?;
        }
    }
    Ok(())
}

#[test]
fn before_helper_0_1_112_the_token_still_comes_from_the_action() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], &format!("{SECTION}{ASSETS}"), "0.1.111")?;
    let text = workflow(&repo)?;
    assert!(text.contains("actions/create-github-app-token@"), "{text}");
    assert!(!text.contains(" app-token "), "{text}");
    Ok(())
}

fn release(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = release_view(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n"),
    )?;
    view.files.insert(
        CONFIG_PATH,
        format!("full_run = \"off\"\n{SECTION}").into_bytes(),
    );
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    Ok(expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?)
}

#[test]
fn a_release_build_at_helper_0_1_112_mints_with_the_helper_before_the_tools_install() -> TestResult
{
    let text = release("0.1.112")?;
    let mut mints = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("a job without steps")?;
        let code = steps.iter().position(runs_checkout_code);
        for index in key_steps(steps) {
            let step = &steps[index];
            assert!(step["uses"].is_null(), "{step}");
            assert!(
                mints_with_the_helper(step["run"].as_str().unwrap_or_default()),
                "{step}"
            );
            assert!(
                code.is_some_and(|code| index < code),
                "{step} runs after checkout code"
            );
            mints += 1;
        }
    }
    assert!(mints > 0, "no release job mints a token");
    let older = release("0.1.111")?;
    let action = jobs(&older)?.into_iter().any(|job| {
        job["steps"].as_array().is_some_and(|steps| {
            steps
                .iter()
                .any(|step| step["with"]["private-key"] == format!("${{{{ {KEY} }}}}"))
        })
    });
    assert!(action, "helper 0.1.111 no longer renders the action");
    Ok(())
}

#[test]
fn a_hyphen_leading_repository_is_rendered_in_the_equals_form_that_the_cli_parses() -> TestResult {
    let section = SECTION.replace("'acme/other-lib'", "'acme/-private'");
    let repo = private_repo(&["Cargo.toml"], &section, "0.1.112")?;
    let text = workflow(&repo)?;
    let mut mints = 0;
    for job in jobs(&text)? {
        for step in job["steps"].as_array().ok_or("a job without steps")? {
            let run = step["run"].as_str().unwrap_or_default();
            if !mints_with_the_helper(run) {
                continue;
            }
            let words: Vec<&str> = run.split_whitespace().collect();
            assert!(
                words.contains(&"--repository=-private") && words.contains(&"--owner=acme"),
                "{run}"
            );
            assert!(
                !words.contains(&"--repository") && !words.contains(&"--owner"),
                "{run}"
            );
            mints += 1;
        }
    }
    assert!(mints > 0, "no job mints a token");
    Ok(())
}
