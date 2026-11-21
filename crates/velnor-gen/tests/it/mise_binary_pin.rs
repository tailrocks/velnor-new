// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::facts::TestResult;
use crate::lint_lane::rendered;
use crate::pipeline::{pin_helper, view, workflow};
use crate::release_fixture::two_binaries;
use velnor_render::{MISE_BINARIES, UnknownMiseVersion, Value, mise_sha256_input};

const VERSION: &str = "2026.11.10";
const DIGESTS: [(&str, &str); 4] = [
    (
        "Linux-ARM64",
        "86ef275d541c5243cd20b1c0fcd16810e5f3601113d8db09278d0608617a3833",
    ),
    (
        "Linux-X64",
        "b603a4f48563fc379a794d9ea2cf9f709ed276c9141068604cb0c59fa3e674a9",
    ),
    (
        "macOS-ARM64",
        "3f4ba9d21c6f2170048d0870fd98ab287e04730b76a6fc8e83964d59c80eb704",
    ),
    (
        "macOS-X64",
        "7eab8054d301b75dcbd93440ff184bee6864d95799cfdc4a00ce858313b68783",
    ),
];
fn platform_of(label: &str) -> Option<&'static str> {
    if label.starts_with("macos-") {
        return Some("macOS-ARM64");
    }
    let linux = label.starts_with("ubuntu-");
    let arm = label.ends_with("-arm");
    match (linux, label.starts_with("windows-"), arm) {
        (true, _, true) => Some("Linux-ARM64"),
        (true, _, false) => Some("Linux-X64"),
        (false, true, true) => Some("Windows-ARM64"),
        (false, true, false) => Some("Windows-X64"),
        (false, false, _) => None,
    }
}

fn literal_for(runs_on: &str) -> Option<&'static str> {
    if runs_on.contains("${{") {
        return None;
    }
    let platform = platform_of(runs_on)?;
    DIGESTS
        .iter()
        .find(|(name, _)| *name == platform)
        .map(|(_, digest)| *digest)
}

fn mise_steps(text: &str) -> Result<Vec<(String, serde_json::Value)>, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = serde_saphyr::from_str(text)?;
    let mut found = Vec::new();
    for job in yaml["jobs"].as_object().ok_or("no jobs")?.values() {
        let runs_on = job["runs-on"].as_str().ok_or("no runs-on")?;
        for step in job["steps"].as_array().ok_or("no steps")? {
            if step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
            {
                found.push((runs_on.to_owned(), step["with"].clone()));
            }
        }
    }
    Ok(found)
}

fn assert_selection(label: &str, input: &str) -> TestResult {
    let expected: BTreeMap<&str, &str> = DIGESTS
        .into_iter()
        .filter(|(platform, _)| platform.starts_with("macOS"))
        .collect();
    let chain = input
        .strip_prefix("${{ runner.os != 'Windows' && runner.os != 'Linux' && (")
        .and_then(|rest| rest.strip_suffix(" || 'no-mise-digest-for-this-runner') || '' }}"))
        .ok_or_else(|| format!("{label}: unexpected sha256 expression: {input}"))?;
    let mut table = BTreeMap::new();
    for clause in chain.split(" || ") {
        let (os, rest) = clause
            .strip_prefix("runner.os == '")
            .and_then(|rest| rest.split_once("' && runner.arch == '"))
            .ok_or_else(|| format!("{label}: unexpected clause: {clause}"))?;
        let (arch, digest) = rest
            .split_once("' && '")
            .and_then(|(arch, digest)| Some((arch, digest.strip_suffix('\'')?)))
            .ok_or_else(|| format!("{label}: unexpected clause: {clause}"))?;
        table.insert(format!("{os}-{arch}"), digest.to_owned());
    }
    let table: BTreeMap<&str, &str> = table
        .iter()
        .map(|(platform, digest)| (platform.as_str(), digest.as_str()))
        .collect();
    assert_eq!(table, expected, "{label}: {input}");
    assert!(!input.contains("fromJSON"), "{label}: {input}");
    Ok(())
}

fn assert_pinned(label: &str, text: &str) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let steps = mise_steps(text)?;
    assert!(!steps.is_empty(), "{label}: no mise-action step rendered");
    let (mut literals, mut selections) = (0, 0);
    for (runs_on, with) in steps {
        assert_eq!(with["version"], VERSION, "{label}: {with}");
        let input = with["sha256"]
            .as_str()
            .ok_or_else(|| format!("{label}: a mise-action step has no sha256 input: {with}"))?;
        if let Some(digest) = literal_for(&runs_on) {
            assert_eq!(input, digest, "{label}: {runs_on}");
            literals += 1;
        } else {
            assert_selection(label, input)?;
            selections += 1;
        }
    }
    Ok((literals, selections))
}

#[test]
fn rust_ci_install_tools_pins_the_mise_binary_for_every_runner_platform() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let config = format!("shape = '{shape}'\n");
        let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
        assert_pinned(shape, &workflow(&repo)?)?;
    }
    Ok(())
}

#[test]
fn the_bare_install_mise_step_of_a_toolless_lint_task_is_pinned_too() -> TestResult {
    let text = rendered("[[lint]]\nkind = 'mise-task'\ntask = 'check'\n")?;
    assert_pinned("lint-check", &text)?;
    assert!(
        mise_steps(&text)?
            .iter()
            .any(|(_, with)| with["install"] == false),
        "no `Install mise` step rendered"
    );
    Ok(())
}

#[test]
fn release_workflow_installs_pin_the_mise_binary_for_every_runner_platform() -> TestResult {
    let (text, _) = two_binaries()?;
    let (literals, selections) = assert_pinned("release.yml", &text)?;
    assert!(literals > 0 && selections == 0, "{literals} {selections}");
    Ok(())
}

#[test]
fn a_job_on_an_unknown_runner_label_selects_the_digest_by_runner_when_it_runs() -> TestResult {
    let repo = view(&["Cargo.toml"], Some(b"runner = 'velnor-box'\n"))?;
    let (literals, selections) = assert_pinned("velnor-box", &workflow(&repo)?)?;
    assert!(selections > 0, "{literals} {selections}");
    let repo = view(&["Cargo.toml"], Some(b"runner = 'velnor-linux-x64'\n"))?;
    let (literals, selections) = assert_pinned("velnor-linux-x64", &workflow(&repo)?)?;
    assert!(literals == 0 && selections > 0, "{literals} {selections}");
    Ok(())
}

#[test]
fn a_mise_version_without_a_known_digest_is_refused() {
    for version in ["2026.11.9", "2026.11.100", "latest", ""] {
        assert_eq!(
            mise_sha256_input(version),
            Err(UnknownMiseVersion(version.to_owned())),
            "{version}"
        );
    }
    assert!(matches!(mise_sha256_input(VERSION), Ok(Value::Expr(_))));
}

#[test]
fn the_digest_table_has_the_four_pinned_platforms_with_distinct_sha256_digests() {
    let mut seen = std::collections::BTreeSet::new();
    for binary in MISE_BINARIES {
        let platforms: Vec<&str> = binary
            .sha256
            .iter()
            .map(|(platform, _)| *platform)
            .collect();
        assert_eq!(
            platforms,
            ["Linux-ARM64", "Linux-X64", "macOS-ARM64", "macOS-X64"],
            "{}",
            binary.version
        );
        for (platform, digest) in binary.sha256 {
            assert!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{} {platform}: {digest}",
                binary.version
            );
            assert!(
                seen.insert(digest),
                "{} {platform}: duplicate",
                binary.version
            );
        }
    }
    assert!(MISE_BINARIES.iter().any(|binary| binary.version == VERSION));
}

#[test]
fn the_run_time_selection_leaves_windows_unpinned() -> TestResult {
    let input = match mise_sha256_input(VERSION)? {
        Value::Expr(expr) => expr,
        other => return Err(format!("not a selection: {other:?}").into()),
    };
    assert!(
        input.starts_with("${{ runner.os != 'Windows' && runner.os != 'Linux' && ("),
        "{input}"
    );
    assert!(input.ends_with(") || '' }}"), "{input}");
    for (platform, digest) in DIGESTS {
        if platform.starts_with("Linux") {
            assert!(!input.contains(digest), "{platform}: {input}");
        }
    }
    for digest in [
        "a448e986baedc177173b5f63c81343269eef6fef8e3e916b6151580a80e115aa",
        "60b9518958e796a5bf57e782135b845318c4dc47d21a6c7e4628b6a4c55cecfc",
    ] {
        assert!(!input.contains(digest), "{input}");
    }
    Ok(())
}

#[test]
fn a_self_hosted_runner_under_an_ubuntu_label_gets_no_glibc_digest() -> TestResult {
    for config in [
        "runner = 'ubuntu-musl'\nhosted = false\n",
        "runner = 'ubuntu-26.04'\nhosted = false\n",
    ] {
        let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
        let steps = mise_steps(&workflow(&repo)?)?;
        assert!(!steps.is_empty(), "{config}: no mise-action step rendered");
        for (runs_on, with) in steps {
            assert!(runs_on.contains("${{"), "{config}: {runs_on}");
            let input = with["sha256"].as_str().ok_or("no sha256 input")?;
            assert_selection(config, input)?;
        }
    }
    Ok(())
}

#[test]
fn a_trusted_pool_behind_an_ubuntu_label_gets_no_glibc_digest() -> TestResult {
    let mut repo = view(
        &["Cargo.toml", "tools/Cargo.toml"],
        Some(
            b"runner = 'ubuntu-26.04-scale-set'\nhosted = false\ntrusted_runner = 'ubuntu-26.04-scale-set-musl'\n",
        ),
    )?;
    pin_helper(&mut repo, "0.1.110")?;
    let steps = mise_steps(&workflow(&repo)?)?;
    assert!(!steps.is_empty(), "no mise-action step rendered");
    for (runs_on, with) in steps {
        assert!(runs_on.contains("ubuntu-26.04-scale-set-musl"), "{runs_on}");
        let input = with["sha256"].as_str().ok_or("no sha256 input")?;
        assert_selection("trusted pool", input)?;
    }
    Ok(())
}

fn mixed_case_runner_inputs(helper: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut repo = view(
        &["Cargo.toml", "tools/Cargo.toml"],
        Some(b"runner = 'Ubuntu-Latest'\nhosted = true\n"),
    )?;
    pin_helper(&mut repo, helper)?;
    mise_steps(&workflow(&repo)?)?
        .into_iter()
        .map(|(_, with)| {
            with["sha256"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "no sha256 input".into())
        })
        .collect()
}

#[test]
fn a_mixed_case_ubuntu_label_gets_the_linux_digest_from_the_case_insensitive_helper() -> TestResult {
    let inputs = mixed_case_runner_inputs("0.1.112")?;
    assert!(!inputs.is_empty(), "no mise-action step rendered");
    for input in inputs {
        assert_eq!(input, DIGESTS[1].1);
    }
    Ok(())
}

#[test]
fn a_mixed_case_ubuntu_label_below_the_case_insensitive_helper_selects_the_digest_by_runner()
-> TestResult {
    let inputs = mixed_case_runner_inputs("0.1.111")?;
    assert!(!inputs.is_empty(), "no mise-action step rendered");
    for input in inputs {
        assert_selection("Ubuntu-Latest", &input)?;
    }
    Ok(())
}
