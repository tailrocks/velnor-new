// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::mbx_default::pinned;
use crate::pipeline::workflow;
use crate::yaml_tree::document;

const STORE_RUN: &str = "echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx\" >> \"$GITHUB_ENV\"";
const OUT_DIRS_RUN: &str = "echo \"MBX_SHARE_OUT_DIR=0\" >> \"$GITHUB_ENV\"";

fn compiling_jobs(version: &str) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let yaml = document(&workflow(&pinned(None, version)?)?)?;
    let mut jobs = Vec::new();
    for job in yaml["jobs"].as_object().ok_or("no jobs")?.values() {
        let runs: Vec<String> = job["steps"]
            .as_array()
            .ok_or("no steps")?
            .iter()
            .filter(|step| {
                step["name"] == "Private mbx store"
                    || step["name"] == "Keep mbx build-script outputs in target"
            })
            .map(|step| step["run"].as_str().map(str::to_owned).ok_or("no run"))
            .collect::<Result<_, _>>()?;
        if !runs.is_empty() {
            jobs.push(runs);
        }
    }
    Ok(jobs)
}

#[test]
fn a_helper_that_ships_the_setting_keeps_build_script_outputs_under_target() -> TestResult {
    let jobs = compiling_jobs("0.1.111")?;
    assert_ne!(jobs.len(), 0);
    for runs in jobs {
        assert_eq!(runs, [STORE_RUN, OUT_DIRS_RUN]);
    }
    Ok(())
}

#[test]
fn an_older_helper_renders_the_private_store_step_byte_for_byte_as_before() -> TestResult {
    for version in ["0.1.88", "0.1.109", "0.1.110"] {
        let jobs = compiling_jobs(version)?;
        assert_ne!(jobs.len(), 0, "{version}");
        for runs in jobs {
            assert_eq!(runs, [STORE_RUN], "{version}");
        }
    }
    Ok(())
}
