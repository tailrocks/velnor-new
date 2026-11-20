// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::gh_expr::{Value, evaluate, has_status_function};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

fn is_fetch(step: &serde_json::Value) -> bool {
    step["run"]
        .as_str()
        .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
}

fn is_phase(step: &serde_json::Value) -> bool {
    step["id"] == "private-token" || step["id"] == "private-check" || is_fetch(step)
}

/// A PR run on a same-repository branch by a person, after the setup steps passed and the
/// sources cache missed (the action leaves `cache-hit` empty on a miss).
fn pull_request(path: &str) -> Value {
    let text = |value: &str| Value::Str(value.to_owned());
    match path {
        "github.event_name" => text("pull_request"),
        "github.repository" | "github.event.pull_request.head.repo.full_name" => text("o/r"),
        "github.actor" | "github.event.pull_request.user.login" => text("alice"),
        "steps.plan.outputs.rust_root_mode" => text("all"),
        "needs.rust.outputs.rust_deny" => text("run"),
        _ if path.ends_with(".outcome") => text("success"),
        _ => Value::Null,
    }
}

#[test]
fn a_failed_format_step_does_not_skip_the_private_token_and_the_fetch() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], SECTION, "0.1.88")?;
    let mut checked = 0;
    for job in jobs(&workflow(&repo)?)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let compile = steps
            .iter()
            .find(|step| {
                step["name"] == "Lint"
                    || step["run"]
                        .as_str()
                        .is_some_and(|r| r.contains("cargo-clippy"))
            })
            .and_then(|step| step["if"].as_str());
        let Some(compile) = compile else { continue };
        assert!(has_status_function(compile), "{compile}");
        for step in steps.iter().filter(|step| is_phase(step)) {
            checked += 1;
            let guard = step["if"]
                .as_str()
                .ok_or_else(|| format!("no if: {step}"))?;
            assert!(
                has_status_function(guard),
                "a failed earlier step (Format) would skip this one while the compile steps still run offline: {guard}"
            );
            if step["id"] != "private-check" {
                assert!(
                    evaluate(guard, &|path| Ok(pull_request(path)))?.truthy(),
                    "{guard}"
                );
            }
        }
    }
    assert!(checked >= 3, "{checked}");
    Ok(())
}
