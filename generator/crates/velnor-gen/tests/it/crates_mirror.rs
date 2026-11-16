// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const MIRROR: &str = "[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

#[test]
fn mirror_is_explicit_scoped_and_precedes_every_job_tools() -> TestResult {
    for (runner, mirror_runner, enabled) in [
        ("mirror-box", "mirror-box", true),
        ("ubuntu-26.04-scale-set", "ubuntu-26.04-scale-set", true),
        ("ubuntu-slim", "ubuntu-slim", true),
        ("xcode-27", "xcode-27", true),
        ("ubuntu-future", "ubuntu-future", true),
        ("ubuntu-latest", "mirror-box", false),
    ] {
        let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
        repo.files.insert(
            path(".velnor/rust-ci.toml")?,
            format!(
                "runner = '{runner}'\n{}",
                MIRROR.replace("mirror-box", mirror_runner)
            )
            .into_bytes(),
        );
        let lock = path(".velnor/rust-ci.lock")?;
        let old = String::from_utf8(repo.files[&lock].clone())?;
        repo.files
            .insert(lock.clone(), old.replace("0.1.79", "0.1.81").into_bytes());
        let text = workflow(&repo)?;
        assert_eq!(text.contains("velnor-ci crates-mirror"), enabled, "{text}");
        if enabled {
            let yaml: serde_json::Value = serde_saphyr::from_str(&text)?;
            let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
            for (_, job) in jobs {
                let steps = job["steps"].as_array().ok_or("steps missing")?;
                let mirror = steps.iter().position(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.starts_with("velnor-ci crates-mirror"))
                });
                if let Some(tools) = steps.iter().position(|step| {
                    step["uses"]
                        .as_str()
                        .is_some_and(|uses| uses.starts_with("jdx/mise-action"))
                }) {
                    assert!(mirror.is_some_and(|index| index < tools));
                }
                if let Some(index) = mirror {
                    assert_eq!(
                        steps[index]["if"].as_str(),
                        Some(
                            "${{ runner.environment == 'self-hosted' && (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) }}"
                        ),
                        "{runner}: {}",
                        steps[index]
                    );
                }
            }
        }
        repo.files
            .insert(lock, old.replace("0.1.79", "0.1.80").into_bytes());
        assert!(workflow(&repo).is_err_and(|error| error.to_string().contains("0.1.81")));
        repo.files.remove(&path(".velnor/rust-ci.toml")?);
        assert!(!workflow(&repo)?.contains("crates-mirror"));
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
            MIRROR.replace("/opt/velnor", "relative/velnor"),
            "crates_mirror.cainfo",
        ),
        (format!("{MIRROR}unknown = true\n"), "unknown field"),
    ] {
        let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        let lock = path(".velnor/rust-ci.lock")?;
        let old = String::from_utf8(repo.files[&lock].clone())?;
        repo.files
            .insert(lock, old.replace("0.1.79", "0.1.81").into_bytes());
        let error = workflow(&repo)
            .expect_err("invalid mirror accepted")
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
    Ok(())
}
