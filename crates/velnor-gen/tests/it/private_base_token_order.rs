// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const VERDICT_STEPS: [&str; 2] = [
    "Check that the repository is private",
    "Check that the merge queue entry is not from a fork",
];
const CODE_STEPS: [&str; 7] = [
    "Install tools",
    "velnor-ci prepare",
    "velnor-ci check-metadata",
    "Format",
    "Lint",
    "Documentation",
    "Test",
];

fn executes_checkout_code(step: &serde_json::Value) -> bool {
    step["name"]
        .as_str()
        .is_some_and(|name| CODE_STEPS.contains(&name))
        || step["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
        || step["run"]
            .as_str()
            .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" run -- cargo"))
}

#[test]
fn the_token_bearing_steps_run_before_any_step_that_executes_checkout_code() -> TestResult {
    let layouts: [(&[&str], &str); 4] = [
        (&["Cargo.toml"], "one-job"),
        (&["Cargo.toml"], "lint-test"),
        (&["Cargo.toml", "tools/Cargo.toml"], "one-job"),
        (&["Cargo.toml", "tools/Cargo.toml"], "lint-test"),
    ];
    for (roots, shape) in layouts {
        let mut repo = private_repo(roots, &format!("shape = '{shape}'\n{SECTION}"), "0.1.111")?;
        for root in roots {
            let lock = path(root)?.sibling(&path("Cargo.lock")?);
            repo.files.insert(lock, GIT_LOCK.as_bytes().to_vec());
        }
        let mut checked = 0;
        for job in jobs(&workflow(&repo)?)? {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let position = |name: &str| steps.iter().position(|step| step["name"] == name);
            if position(VERDICT_STEPS[0]).is_none() {
                continue;
            }
            checked += 1;
            let code: Vec<usize> = steps
                .iter()
                .enumerate()
                .filter(|(_, step)| executes_checkout_code(step))
                .map(|(index, _)| index)
                .collect();
            assert!(!code.is_empty(), "{roots:?} {shape}: no code step found");
            for token_step in VERDICT_STEPS {
                let at = position(token_step)
                    .ok_or_else(|| format!("{roots:?} {shape}: {token_step} missing"))?;
                for index in &code {
                    assert!(
                        at < *index,
                        "{roots:?} {shape}: {token_step} must run before {}",
                        steps[*index]["name"]
                    );
                }
            }
        }
        assert!(
            checked > 0,
            "{roots:?} {shape}: no job has the verdict step"
        );
    }
    Ok(())
}
