// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::ops::RangeInclusive;

use velnor_model::{RepoPath, RepoView};

use crate::facts::{TestResult, lock_pinned_tools};
use crate::lint_lane::{LINT as LINT_LANE, MISE};
use crate::pipeline::{pin_helper, view};
use crate::private_sources::SECTION;
use crate::release_fixture::{APP, Root, TOOLS, TWO_BINARIES, release_view};
use crate::release_unguarded::expected_release;
use crate::step_refs::{Scan, scan_workflow};
use crate::test_groups::{DATABASE, SHELL};

fn helpers() -> RangeInclusive<u64> {
    let newest = velnor_gen::key_gates()
        .map(|gate| gate.first.2)
        .max()
        .unwrap_or(velnor_gen::HELPER_MINIMUM.2);
    velnor_gen::HELPER_MINIMUM.2..=newest
}

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const SHARED_STORE_OFF: &str = "mbx_shared_store = false\n";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

fn pin(repo: &mut RepoView, helper: u64) -> TestResult {
    let required = velnor_gen::required_helper(repo).map_or(0, |(_, _, patch)| patch);
    pin_helper(repo, &format!("0.1.{}", helper.max(required)))
}

fn rust_ci_files(
    config: Option<&str>,
    tools: Option<&str>,
    helper: u64,
) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    let mut repo = view(ROOTS, config.map(str::as_bytes))?;
    if let Some(tools) = tools {
        repo.files
            .insert(RepoPath::new("mise.toml")?, tools.as_bytes().to_vec());
    }
    pin(&mut repo, helper)?;
    lock_pinned_tools(&mut repo)?;
    Ok(velnor_gen::expected_with_notes(&repo)?.0)
}

fn release_files(
    roots: &[Root],
    release_toml: &str,
    helper: u64,
) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    let mut repo = release_view(roots, Some(release_toml))?;
    pin(&mut repo, helper)?;
    Ok(expected_release(&repo)?)
}

#[derive(Default)]
struct Sweep {
    scan: Scan,
    problems: Vec<String>,
}

impl Sweep {
    fn add(&mut self, label: &str, files: &BTreeMap<RepoPath, String>) -> TestResult {
        for (path, text) in files {
            if !path.to_string().starts_with(".github/workflows/") {
                continue;
            }
            self.scan.workflows += 1;
            scan_workflow(&crate::yaml_tree::document(text)?, &mut self.scan);
        }
        self.problems.extend(
            self.scan
                .dangling
                .drain(..)
                .map(|dangling| format!("{label}: {dangling}")),
        );
        Ok(())
    }

    fn finish(self) {
        assert!(self.problems.is_empty(), "{:#?}", self.problems);
        assert!(
            self.scan.workflows > 0 && self.scan.refs > 0,
            "workflows {} refs {}",
            self.scan.workflows,
            self.scan.refs
        );
    }
}

fn rust_ci_sweep(cases: &[(Option<&str>, Option<&str>)]) -> TestResult {
    let mut sweep = Sweep::default();
    for helper in helpers() {
        for (config, tools) in cases {
            sweep.add(
                &format!("0.1.{helper}, {config:?}"),
                &rust_ci_files(*config, *tools, helper)?,
            )?;
        }
    }
    sweep.finish();
    Ok(())
}

#[test]
fn every_steps_reference_in_the_base_rust_ci_names_a_step_of_its_own_job() -> TestResult {
    rust_ci_sweep(&[
        (None, None),
        (Some(SHARED_STORE_ON), None),
        (Some(SHARED_STORE_OFF), None),
    ])
}

#[test]
fn every_steps_reference_in_a_feature_lane_or_test_group_names_a_step_of_its_own_job() -> TestResult
{
    let groups = format!("{DATABASE}root = '.'\n{SHELL}root = '.'\n");
    rust_ci_sweep(&[(Some(LANE), None), (Some(groups.as_str()), None)])
}

#[test]
fn every_steps_reference_in_a_lint_test_setup_or_private_sources_job_names_a_step_of_its_own_job()
-> TestResult {
    rust_ci_sweep(&[
        (Some(LINT_LANE), Some(MISE)),
        (Some(SETUP), None),
        (Some(SECTION), None),
    ])
}

#[test]
fn every_steps_reference_in_the_release_workflow_names_a_step_of_its_own_job() -> TestResult {
    let mut sweep = Sweep::default();
    let cases: [(&[Root], &str); 3] = [
        (&[APP], "[[binaries]]\npackage = \"app\"\n"),
        (&[APP, TOOLS], TWO_BINARIES),
        (
            &[APP, TOOLS],
            "roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n",
        ),
    ];
    for helper in helpers() {
        for (roots, release_toml) in cases {
            sweep.add(
                &format!("0.1.{helper}, {release_toml:?}"),
                &release_files(roots, release_toml, helper)?,
            )?;
        }
    }
    sweep.finish();
    Ok(())
}
