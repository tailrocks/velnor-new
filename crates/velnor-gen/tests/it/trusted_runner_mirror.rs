// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const SCALE_SET: &str = "ubuntu-26.04-scale-set";
const POOL: &str = "ubuntu-26.04-scale-set-trusted";
const MIRROR_RUNNER: &str = "mirror-box";
const MIRROR_POOL: &str = "mirror-box-trusted";
const GROUP: &str = "[[test_groups]]\nname = 'database'\nfilter = 'package(package-0) and test(/^db_/)'\nrunner = 'mirror-box'\n";

fn mirror(runner: &str, trusted: Option<&str>) -> String {
    let key = trusted.map_or_else(String::new, |label| format!("trusted_runner = '{label}'\n"));
    format!(
        "[crates_mirror]\nrunner = '{runner}'\n{key}registry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n"
    )
}

fn runs_on(config: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.109")?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    Ok(yaml["jobs"]
        .as_object()
        .ok_or("jobs missing")?
        .values()
        .filter_map(|job| job["runs-on"].as_str().map(str::to_owned))
        .collect())
}

fn refusal(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.109")?;
    Ok(workflow(&repo)
        .err()
        .ok_or_else(|| format!("accepted: {config}"))?
        .to_string())
}

fn on(labels: &[String], needle: &str) -> usize {
    labels
        .iter()
        .filter(|label| label.contains(&format!("'{needle}'")))
        .count()
}

#[test]
fn a_mirror_on_the_runner_inherits_the_top_level_trusted_runner() -> TestResult {
    let base = format!("runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n");
    let plain = runs_on(&base)?;
    let mirrored = runs_on(&format!("{base}{}", mirror(SCALE_SET, None)))?;
    let spelled = runs_on(&format!("{base}{}", mirror(SCALE_SET, Some(POOL))))?;
    assert_eq!(plain, mirrored);
    assert_eq!(plain, spelled);
    assert!(on(&plain, POOL) > 0, "{plain:?}");
    Ok(())
}

#[test]
fn a_mirror_on_its_own_runner_routes_by_its_own_trusted_runner() -> TestResult {
    let base = format!("runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n{GROUP}");
    let with = runs_on(&format!(
        "{base}{}",
        mirror(MIRROR_RUNNER, Some(MIRROR_POOL))
    ))?;
    assert_eq!(on(&with, MIRROR_POOL), 1, "{with:?}");
    assert!(
        with.iter()
            .all(|label| label.contains(POOL) || label.contains(MIRROR_POOL))
    );
    let without = runs_on(&format!("{base}{}", mirror(MIRROR_RUNNER, None)))?;
    assert_eq!(on(&without, MIRROR_POOL), 0, "{without:?}");
    assert_eq!(on(&without, MIRROR_RUNNER), 1, "{without:?}");
    assert!(on(&without, POOL) > 0, "{without:?}");
    Ok(())
}

#[test]
fn a_mirror_without_a_top_level_trusted_runner_keeps_its_own() -> TestResult {
    let config = format!(
        "runner = '{SCALE_SET}'\n{GROUP}{}",
        mirror(MIRROR_RUNNER, Some(MIRROR_POOL))
    );
    let labels = runs_on(&config)?;
    assert_eq!(on(&labels, MIRROR_POOL), 1, "{labels:?}");
    assert_eq!(on(&labels, POOL), 0, "{labels:?}");
    Ok(())
}

#[test]
fn a_mirror_trusted_runner_that_disagrees_with_its_runner_is_refused() -> TestResult {
    for (config, why) in [
        (
            format!(
                "runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n{}",
                mirror(SCALE_SET, Some("other-trusted"))
            ),
            "the mirror shares the runner but names another pool",
        ),
        (
            format!("runner = '{SCALE_SET}'\n{}", mirror(SCALE_SET, Some(POOL))),
            "the mirror shares the runner and the top level has no pool",
        ),
        (
            format!(
                "runner = '{SCALE_SET}'\n{}",
                mirror(MIRROR_RUNNER, Some(MIRROR_RUNNER))
            ),
            "the mirror's own runner",
        ),
        (
            format!(
                "runner = '{SCALE_SET}'\n{}",
                mirror(MIRROR_RUNNER, Some("ubuntu-latest"))
            ),
            "a hosted label",
        ),
        (
            format!(
                "runner = '{SCALE_SET}'\n{}",
                mirror(MIRROR_RUNNER, Some(SCALE_SET))
            ),
            "the top-level runner",
        ),
    ] {
        let error = refusal(&config)?;
        assert!(error.contains("trusted_runner"), "{why}: {error}");
    }
    Ok(())
}
