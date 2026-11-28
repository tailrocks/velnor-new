// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::mbx_default::pinned;
use crate::pipeline::workflow;
use crate::yaml_tree::document;

const STORE_RUN: &str = "echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx\" >> \"$GITHUB_ENV\"";
const OUT_DIRS_RUN: &str = "echo \"MBX_SHARE_OUT_DIR=0\" >> \"$GITHUB_ENV\"";
const REMOTE: &[u8] = b"[cache]\nmbx_remote = \"github-actions\"\n";

fn compiling_jobs(
    config: Option<&[u8]>,
    version: &str,
) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let yaml = document(&workflow(&pinned(config, version)?)?)?;
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
    let jobs = compiling_jobs(None, "0.1.111")?;
    assert_ne!(jobs.len(), 0);
    for runs in jobs {
        assert_eq!(runs, [STORE_RUN, OUT_DIRS_RUN]);
    }
    Ok(())
}

#[test]
fn a_remote_cache_job_keeps_build_script_outputs_under_target_too() -> TestResult {
    let jobs = compiling_jobs(Some(REMOTE), "0.1.111")?;
    assert_ne!(jobs.len(), 0);
    for runs in jobs {
        assert_eq!(runs, [OUT_DIRS_RUN]);
    }
    Ok(())
}

#[test]
fn an_older_helper_renders_the_private_store_step_byte_for_byte_as_before() -> TestResult {
    for version in ["0.1.88", "0.1.109", "0.1.110"] {
        let jobs = compiling_jobs(None, version)?;
        assert_ne!(jobs.len(), 0, "{version}");
        for runs in jobs {
            assert_eq!(runs, [STORE_RUN], "{version}");
        }
    }
    Ok(())
}

const SHARED: &[u8] = b"mbx_shared_store = true\n";
const SHARED_STORE_RUN: &str = ". \"$VELNOR_EXEC\" mbx-store";

fn shared_store_runs(version: &str) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    shared_runs(SHARED, version)
}

fn shared_runs(
    config: &[u8],
    version: &str,
) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let yaml = document(&workflow(&pinned(Some(config), version)?)?)?;
    let mut jobs = Vec::new();
    for job in yaml["jobs"].as_object().ok_or("no jobs")?.values() {
        let runs: Vec<String> = job["steps"]
            .as_array()
            .ok_or("no steps")?
            .iter()
            .filter(|step| {
                step["name"] == "velnor-ci mbx-store"
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
fn a_shared_store_helper_that_ships_the_setting_keeps_build_script_outputs_under_target()
-> TestResult {
    let jobs = shared_store_runs("0.1.111")?;
    assert_ne!(jobs.len(), 0);
    for runs in jobs {
        assert_eq!(
            runs,
            [SHARED_STORE_RUN, OUT_DIRS_RUN],
            "the setting goes right after the store"
        );
    }
    Ok(())
}

#[test]
fn a_helper_that_appends_the_setting_itself_renders_no_standalone_step_in_the_shared_store()
-> TestResult {
    for version in ["0.1.112", "0.1.113"] {
        let jobs = shared_store_runs(version)?;
        assert_ne!(jobs.len(), 0, "{version}");
        for runs in jobs {
            assert_eq!(runs, [SHARED_STORE_RUN], "{version}");
        }
    }
    Ok(())
}

const SHARED_REMOTE: &[u8] = b"mbx_shared_store = true\n[cache]\nmbx_remote = \"github-actions\"\n";

#[test]
fn a_shared_store_with_a_remote_cache_has_one_writer_of_the_setting_from_0_1_112() -> TestResult {
    for version in ["0.1.112", "0.1.113"] {
        let jobs = shared_runs(SHARED_REMOTE, version)?;
        assert_ne!(jobs.len(), 0, "{version}");
        for runs in jobs {
            assert_eq!(runs, [SHARED_STORE_RUN], "{version}");
        }
    }
    Ok(())
}

#[test]
fn a_shared_store_with_a_remote_cache_renders_two_identical_steps_at_0_1_111_as_on_main()
-> TestResult {
    let jobs = shared_runs(SHARED_REMOTE, "0.1.111")?;
    assert_ne!(jobs.len(), 0);
    for runs in jobs {
        assert_eq!(runs, [SHARED_STORE_RUN, OUT_DIRS_RUN, OUT_DIRS_RUN]);
    }
    Ok(())
}

#[test]
fn the_private_and_remote_paths_keep_the_standalone_step_where_no_helper_command_runs() -> TestResult
{
    for (config, expected) in [
        (None, vec![STORE_RUN, OUT_DIRS_RUN]),
        (Some(REMOTE), vec![OUT_DIRS_RUN]),
    ] {
        let jobs = compiling_jobs(config, "0.1.112")?;
        assert_ne!(jobs.len(), 0);
        for runs in jobs {
            assert_eq!(runs, expected);
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_renders_the_shared_store_byte_for_byte_as_before() -> TestResult {
    for (version, expected) in [
        ("0.1.85", vec![SHARED_STORE_RUN]),
        ("0.1.86", vec![SHARED_STORE_RUN]),
        ("0.1.110", vec![SHARED_STORE_RUN]),
        ("0.1.111", vec![SHARED_STORE_RUN, OUT_DIRS_RUN]),
    ] {
        let jobs = shared_store_runs(version)?;
        assert_ne!(jobs.len(), 0, "{version}");
        for runs in jobs {
            assert_eq!(runs, expected, "{version}");
        }
    }
    Ok(())
}
