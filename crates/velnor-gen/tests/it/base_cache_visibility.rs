// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_cache_rules::{CRATES_IO, block, rooted_at, text_of, with_lock_file};
use crate::base_save::{jobs, pinned, step};
use crate::facts::TestResult;
use crate::pipeline::workflow;

#[test]
fn a_repository_without_private_dependencies_renders_no_visibility_condition() -> TestResult {
    let text = text_of(&with_lock_file(CRATES_IO)?)?;
    assert!(!text.contains("repository.private"), "{text}");
    Ok(())
}

#[test]
fn the_trim_job_also_prunes_the_sources_family_of_the_lane_that_saves_it() -> TestResult {
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let lane = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, b)| b.as_str())
    };
    let test = lane("rust-test").ok_or("no test lane")?;
    assert!(
        test.contains(
            "      sources_prefix: 'velnor-src-${{ runner.os }}-${{ runner.arch }}-all.roots-'\n"
        ),
        "{test}"
    );
    assert!(
        !lane("rust-lint")
            .ok_or("no lint lane")?
            .contains("sources_prefix")
    );
    let trim = lane("rust-trim").ok_or("no trim job")?;
    let prune =
        step(trim, "Delete the older rust-test sources entries").ok_or("no sources trim")?;
    assert!(
        prune.contains("CACHE_PREFIX: '${{ needs.rust-test.outputs.sources_prefix }}'"),
        "{prune}"
    );
    assert!(
        prune.contains("run: 'velnor-ci cache trim --prefix \"$CACHE_PREFIX\"'"),
        "{prune}"
    );
    assert!(step(trim, "Delete the older rust-lint sources entries").is_none());
    Ok(())
}

#[test]
fn the_test_prebuild_runs_only_on_a_run_that_can_save() -> TestResult {
    let text = workflow(&pinned("0.1.81", "")?)?;
    let lane = block(&text, "rust").ok_or("no lane")?;
    let build = step(&lane, "Build the tests").ok_or("no prebuild")?;
    for needle in [
        "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)",
        "github.event_name == ''push''",
    ] {
        assert!(build.contains(needle), "{build}");
    }
    Ok(())
}

#[test]
fn input_timestamps_save_and_restore_in_the_configured_target_directory() -> TestResult {
    for (manifest, config, configured, saved, restored, root) in [
        ("Cargo.toml", ".cargo/config.toml", "out", "out", "out", "."),
        (
            "app/Cargo.toml",
            "app/.cargo/config.toml",
            "../out/app",
            "../out/app",
            "out/app",
            "app",
        ),
    ] {
        let mut view = rooted_at("0.1.97", manifest, &[])?;
        crate::cache_key_defects::with_config(
            &mut view,
            config,
            &format!("[build]\ntarget-dir = \"{configured}\"\n"),
        )?;
        let text = text_of(&view)?;
        let yaml: serde_json::Value = serde_saphyr::from_str(&text)?;
        let jobs = yaml["jobs"].as_object().ok_or("no jobs")?;
        for job in jobs.values() {
            let steps = job["steps"].as_array().ok_or("no steps")?;
            for saved in steps
                .iter()
                .filter(|step| step["name"] == "velnor-ci inputs save")
            {
                assert_eq!(saved["id"], "inputs_save");
            }
            for decide in steps
                .iter()
                .filter(|step| step["name"] == "Decide whether to save the build directory")
            {
                assert!(
                    decide["if"]
                        .as_str()
                        .is_some_and(|condition| condition
                            .contains("steps.inputs_save.outcome == 'success'")),
                    "{decide}"
                );
            }
        }
        let runs: Vec<_> = jobs
            .values()
            .filter_map(|job| job["steps"].as_array())
            .flatten()
            .filter_map(|step| step["run"].as_str())
            .map(|run| match run.strip_prefix(". \"$VELNOR_EXEC\" ") {
                Some(rest) => format!("velnor-ci {rest}"),
                None => run.to_owned(),
            })
            .collect();
        for command in [
            format!(
                "velnor-ci inputs save --target-dir {saved} --scope-cache \"${{RUNNER_TEMP}}/velnor-input-scopes-${{GITHUB_JOB}}.json\""
            ),
            format!(
                "velnor-ci inputs restore-all --root-target \"${{GITHUB_WORKSPACE}}\"/{root} \"${{GITHUB_WORKSPACE}}\"/{restored} --scope-cache \"${{RUNNER_TEMP}}/velnor-input-scopes-${{GITHUB_JOB}}.json\""
            ),
        ] {
            assert!(runs.contains(&command), "{command} missing in {runs:?}");
        }
    }
    Ok(())
}
