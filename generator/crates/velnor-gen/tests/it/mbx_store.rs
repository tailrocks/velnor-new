// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const EXEC: &str = "\"$VELNOR_EXEC\" mbx-store";

fn repo(config: &str, helper: &str) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    Ok(repo)
}

#[test]
fn the_shared_store_is_selected_once_per_job_before_any_cargo_or_tool_step() -> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let text = workflow(&repo(
            &format!("{shape}mbx_shared_store = true\n"),
            "0.1.85",
        )?)?;
        let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
        let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
        let mut cargo_jobs = 0;
        for (id, job) in jobs {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let run = |step: &serde_json::Value, needle: &str| {
                step["run"].as_str().is_some_and(|run| run.contains(needle))
            };
            let selected: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| run(s, EXEC))
                .collect();
            assert!(selected.len() <= 1, "{id}");
            let first_work = steps.iter().position(|step| {
                run(step, "\"$VELNOR_EXEC\" run ")
                    || step["uses"]
                        .as_str()
                        .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
            });
            if let Some(work) = first_work {
                cargo_jobs += 1;
                let (at, step) = selected
                    .first()
                    .ok_or_else(|| format!("{id}: no mbx-store"))?;
                assert!(*at < work, "{id}");
                assert!(
                    step.get("if").is_none(),
                    "{id}: runs on every event, the helper decides"
                );
            }
        }
        assert!(cargo_jobs > 0);
    }
    Ok(())
}

#[test]
fn the_shared_store_is_off_unless_configured() -> TestResult {
    assert!(!workflow(&repo("", "0.1.84")?)?.contains("mbx-store"));
    assert!(!workflow(&repo("mbx_shared_store = false\n", "0.1.84")?)?.contains("mbx-store"));
    Ok(())
}

#[test]
fn a_helper_that_cannot_select_the_store_is_refused() -> TestResult {
    assert!(
        workflow(&repo("mbx_shared_store = true\n", "0.1.84")?)
            .is_err_and(|error| error.to_string().contains("0.1.85"))
    );
    Ok(())
}
