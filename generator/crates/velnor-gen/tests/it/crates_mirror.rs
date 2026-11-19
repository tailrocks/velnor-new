// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const MIRROR: &str = "[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

const MIRROR_GUARD: &str = "${{ runner.environment == 'self-hosted' && (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) }}";

fn mirrored_repo(
    runner: &str,
    mirror_runner: &str,
    shape: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .get_mut(&path("mise.toml")?)
        .ok_or("mise.toml missing")?
        .extend_from_slice(b"cargo-machete = '0.9.1'\n");
    repo.files.insert(
        path(".velnor/rust-ci.toml")?,
        format!(
            "runner = '{runner}'\n{shape}{}",
            MIRROR.replace("mirror-box", mirror_runner)
        )
        .into_bytes(),
    );
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", "0.1.82").into_bytes());
    Ok(repo)
}

#[test]
fn mirror_is_explicit_scoped_and_precedes_every_job_tools() -> TestResult {
    for (runner, mirror_runner, enabled) in [
        ("mirror-box", "mirror-box", true),
        ("ubuntu-26.04-scale-set", "ubuntu-26.04-scale-set", true),
        ("ubuntu-999.99", "ubuntu-999.99", true),
        ("xcode-27", "xcode-27", true),
        ("ubuntu-future", "ubuntu-future", true),
        ("ubuntu-latest", "mirror-box", false),
    ] {
        for shape in ["", "shape = 'groups'\n"] {
            let mut repo = mirrored_repo(runner, mirror_runner, shape)?;
            let lock = path(".velnor/rust-ci.lock")?;
            let old = String::from_utf8(repo.files[&lock].clone())?;
            let text = workflow(&repo)?;
            assert_eq!(
                text.contains("\"$VELNOR_EXEC\" crates-mirror"),
                enabled,
                "{text}"
            );
            if enabled {
                let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
                let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
                assert!(jobs.contains_key("policy"));
                if !shape.is_empty() {
                    assert!(text.contains("install_args: 'aqua:nextest-rs/nextest/cargo-nextest'"));
                    assert!(jobs.contains_key("rust-test-default-root"));
                }
                let mut saw_mise = false;
                for (job_id, job) in jobs {
                    let steps = job["steps"].as_array().ok_or("steps missing")?;
                    let mirror = steps.iter().position(|step| {
                        step["run"]
                            .as_str()
                            .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" crates-mirror"))
                    });
                    assert_cargo_scope(steps, mirror, runner, job_id);
                    for (tools, _) in steps.iter().enumerate().filter(|(_, step)| {
                        step["uses"]
                            .as_str()
                            .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
                    }) {
                        saw_mise = true;
                        assert!(mirror.is_some_and(|index| index < tools));
                        let repeated = steps.get(tools + 1).ok_or("mirror missing after Mise")?;
                        let run = repeated["run"].as_str().ok_or("mirror command missing")?;
                        assert!(
                            run.contains("\"$VELNOR_EXEC\" crates-mirror"),
                            "{runner}: {repeated}"
                        );
                        assert_helper_guard(
                            repeated["if"].as_str(),
                            MIRROR_GUARD,
                            &format!("{runner}: {repeated}"),
                        );
                    }
                }
                assert!(saw_mise);
            }
            repo.files
                .insert(lock, old.replace("0.1.82", "0.1.81").into_bytes());
            assert!(
                workflow(&repo).is_err_and(|error| error.to_string().contains("pin-helper 0.1.82"))
            );
            repo.files.insert(
                path(".velnor/rust-ci.toml")?,
                b"full_run = \"off\"\n".to_vec(),
            );
            assert!(!workflow(&repo)?.contains("crates-mirror"));
        }
    }
    Ok(())
}

fn assert_cargo_scope(steps: &[serde_json::Value], mirror: Option<usize>, runner: &str, job: &str) {
    let has_cargo = steps.iter().any(|step| {
        step["run"]
            .as_str()
            .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" run "))
    });
    assert_eq!(mirror.is_some(), has_cargo, "{runner}: {job}");
    assert_mirror_before_tools(steps, mirror);
    if let Some(index) = mirror {
        assert_helper_guard(
            steps[index]["if"].as_str(),
            MIRROR_GUARD,
            &format!("{runner}: {}", steps[index]),
        );
    }
}

fn assert_helper_guard(actual: Option<&str>, original: &str, context: &str) {
    assert_eq!(actual, Some(original), "{context}");
}

fn assert_mirror_before_tools(steps: &[serde_json::Value], mirror: Option<usize>) {
    if let Some(restore) = steps
        .iter()
        .position(|step| step["name"] == "Restore the tools cache")
    {
        assert!(mirror.is_some_and(|index| index < restore));
    }
}

#[test]
fn machete_runs_through_cargo_with_and_without_the_mirror() -> TestResult {
    for (runner, mirror_runner) in [
        ("mirror-box", "mirror-box"),
        ("ubuntu-latest", "mirror-box"),
    ] {
        let text = workflow(&mirrored_repo(runner, mirror_runner, "")?)?;
        let mirrored = text.contains("\"$VELNOR_EXEC\" crates-mirror");
        assert_eq!(mirrored, runner == "mirror-box", "{text}");
        assert!(text.contains("run -- cargo machete'"), "{mirrored}: {text}");
        assert!(
            !text.contains("cargo-machete machete"),
            "{mirrored}: {text}"
        );
    }
    Ok(())
}

#[test]
fn in_job_planning_selects_work_before_configuring_the_mirror() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let config = format!("runner = 'mirror-box'\nshape = '{shape}'\n{MIRROR}");
        let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        repo.files
            .get_mut(&path("mise.toml")?)
            .ok_or("mise.toml missing")?
            .extend_from_slice(b"cargo-machete = '0.9.1'\n");
        let lock = repo
            .files
            .get_mut(&path(".velnor/rust-ci.lock")?)
            .ok_or("helper lock missing")?;
        *lock = String::from_utf8(lock.clone())?
            .replace("0.1.81", "0.1.82")
            .into_bytes();
        let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
        let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
        assert!(jobs.contains_key("policy"));
        if shape == "one-job" {
            assert!(jobs.contains_key("rust"));
        }
        for (job_id, job) in jobs {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let plan = steps.iter().position(|step| step["id"] == "plan");
            let (selection, owns_plan) = if shape == "one-job" && job_id == "rust" {
                (
                    Some(
                        "steps.plan.outcome == 'success' && steps.plan.outputs.rust_root_mode != 'none'",
                    ),
                    true,
                )
            } else {
                (None, false)
            };
            let policy_lane = shape == "one-job" && job_id == "policy";
            let expected = selection.as_ref().map_or_else(
                || MIRROR_GUARD.to_owned(),
                |selected| {
                    format!(
                        "${{{{ ({}) && ({selected}) }}}}",
                        MIRROR_GUARD
                            .trim_start_matches("${{ ")
                            .trim_end_matches(" }}")
                    )
                },
            );
            let mut saw_mirror = false;
            assert_mirror_before_tools(
                steps,
                steps.iter().position(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" crates-mirror"))
                }),
            );
            for (index, step) in steps.iter().enumerate().filter(|(_, step)| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" crates-mirror"))
            }) {
                saw_mirror = true;
                if let Some(plan) = plan {
                    assert!(index > plan, "{shape}: {job_id}: mirror precedes plan");
                }
                assert_helper_guard(
                    step["if"].as_str(),
                    &expected,
                    &format!("{shape}: {job_id}: {step}"),
                );
            }
            if selection.is_some() || policy_lane {
                if owns_plan {
                    assert!(plan.is_some(), "{shape}: {job_id}: plan missing");
                }
                assert!(saw_mirror, "{shape}: {job_id}: mirror missing");
            }
        }
    }
    Ok(())
}

#[test]
fn invalid_mirror_settings_fail_generation() -> TestResult {
    for (config, diagnostic) in [
        (
            MIRROR.replace("mirror-box", "ubuntu-latest"),
            "crates_mirror.runner",
        ),
        (
            MIRROR.replace("sparse+https://", "http://"),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace("crates-mirror.velnor-system.svc.cluster.local/index", "%"),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace("crates-mirror.velnor-system.svc.cluster.local/index", ""),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace("crates-mirror.velnor-system.svc.cluster.local", ":443"),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace("crates-mirror.velnor-system.svc.cluster.local", "[broken]"),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace(
                "crates-mirror.velnor-system.svc.cluster.local",
                "localhost:70000",
            ),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace(
                "crates-mirror.velnor-system.svc.cluster.local",
                "\\localhost",
            ),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace(
                "crates-mirror.velnor-system.svc.cluster.local",
                "/localhost",
            ),
            "crates_mirror.registry",
        ),
        (
            MIRROR.replace("/opt/velnor", "relative/velnor"),
            "crates_mirror.cainfo",
        ),
        (format!("{MIRROR}unknown = true\n"), "unknown field"),
    ] {
        let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        let lock = path(".velnor/rust-ci.lock")?;
        let old = String::from_utf8(repo.files[&lock].clone())?;
        repo.files
            .insert(lock, old.replace("0.1.81", "0.1.82").into_bytes());
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("invalid mirror accepted: {config}"))?
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
    Ok(())
}

#[test]
fn valid_mirror_registry_hosts_preserve_the_configured_url() -> TestResult {
    for host in [
        "mirror.example",
        "localhost:8443",
        "127.0.0.1:8443",
        "[::1]:8443",
    ] {
        let config = format!(
            "runner = 'mirror-box'\n{}",
            MIRROR.replace("crates-mirror.velnor-system.svc.cluster.local", host)
        );
        let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        let lock = path(".velnor/rust-ci.lock")?;
        let old = String::from_utf8(repo.files[&lock].clone())?;
        repo.files
            .insert(lock, old.replace("0.1.81", "0.1.82").into_bytes());
        let text = workflow(&repo)?;
        assert!(
            text.contains(&format!("sparse+https://{host}/index/")),
            "{text}"
        );
    }
    Ok(())
}
