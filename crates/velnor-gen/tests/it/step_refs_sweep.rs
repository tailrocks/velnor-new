// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use velnor_model::{RepoPath, RepoView};

use crate::facts::{TestResult, lock_pinned_tools};
use crate::lint_lane::{LINT as LINT_LANE, MISE};
use crate::pipeline::{pin_helper, view};
use crate::private_sources::SECTION;
use crate::release_fixture::{APP, Root, TOOLS, TWO_BINARIES, release_view};
use crate::release_unguarded::expected_release;
use crate::step_refs::{Scan, scan_workflow};
use crate::test_groups::{DATABASE, SHELL};

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_sources(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn patches_in(text: &str) -> Vec<u64> {
    text.split("(0, 1, ")
        .skip(1)
        .filter_map(|rest| rest.split_once(')')?.0.parse().ok())
        .collect()
}

fn gate_patches() -> Result<BTreeSet<u64>, Box<dyn Error>> {
    let mut files = Vec::new();
    for krate in ["velnor-gen", "velnor-rust", "velnor-render", "velnor-model"] {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(krate)
            .join("src");
        rust_sources(&src, &mut files)?;
    }
    let mut patches = BTreeSet::new();
    for file in files {
        let text = fs::read_to_string(file)?;
        patches.extend(
            patches_in(&text)
                .into_iter()
                .filter(|patch| *patch >= velnor_gen::HELPER_MINIMUM.2),
        );
    }
    Ok(patches)
}

fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    velnor_gen::key_gates()
        .find(|gate| gate.key == key && gate.value.is_none())
        .map(|gate| gate.first.2)
        .ok_or_else(|| format!("the {key} gate is missing").into())
}

fn is_workflow(path: &RepoPath) -> bool {
    path.to_string().starts_with(".github/workflows/")
}

fn shape(text: &str, helper: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with("# velnor-gen-sha256:"))
        .map(|line| line.replace(helper, "HELPER"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sweep_job(
    patch: u64,
    config: Option<&str>,
    seen: &Mutex<HashSet<String>>,
) -> Result<(usize, Vec<String>), Box<dyn Error>> {
    let helper = format!("0.1.{patch}");
    let mut repo = view(ROOTS, config.map(str::as_bytes))?;
    pin_helper(&mut repo, &helper)?;
    let files = generated(&mut repo)?;
    let shaped: String = files
        .iter()
        .filter(|(path, _)| is_workflow(path))
        .map(|(_, text)| shape(text, &helper))
        .collect();
    if !seen
        .lock()
        .map_err(|_| "the render set is poisoned")?
        .insert(shaped)
    {
        return Ok((0, Vec::new()));
    }
    let mut scan = Scan::default();
    for (_, text) in files.iter().filter(|(path, _)| is_workflow(path)) {
        scan_workflow(&crate::yaml_tree::document(text)?, &mut scan);
    }
    let failures = scan
        .dangling
        .into_iter()
        .map(|found| format!("helper {helper}, config {config:?}: {found}"))
        .collect();
    Ok((scan.refs, failures))
}

fn generated(repo: &mut RepoView) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    lock_pinned_tools(repo)?;
    Ok(velnor_gen::expected_with_notes(repo)?.0)
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let patches = gate_patches()?;
    let shared_store = gate_patch("mbx_shared_store")?;
    let cache_debug = gate_patch("cache_debug")?;
    assert!(
        patches.contains(&velnor_gen::HELPER_MINIMUM.2)
            && patches.contains(&shared_store)
            && patches.contains(&cache_debug),
        "gate scan found {patches:?}, which misses the minimum, shared-store or cache_debug gate"
    );
    let mut work: Vec<(u64, Option<&str>)> = Vec::new();
    for patch in patches {
        work.push((patch, None));
        if patch >= shared_store {
            work.push((patch, Some(SHARED_STORE_ON)));
        }
        if patch >= cache_debug {
            work.push((patch, Some(CACHE_DEBUG_ON)));
        }
    }
    let seen = Mutex::new(HashSet::new());
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4);
    let results: Vec<Result<(usize, Vec<String>), String>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut refs = 0;
                    let mut failures = Vec::new();
                    while let Some(&(patch, config)) =
                        work.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let (checked, found) =
                            sweep_job(patch, config, &seen).map_err(|error| error.to_string())?;
                        refs += checked;
                        failures.extend(found);
                    }
                    Ok((refs, failures))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("a sweep worker panicked".to_owned()))
            })
            .collect()
    });
    let mut refs = 0;
    let mut failures = Vec::new();
    for result in results {
        let (checked, found) = result?;
        refs += checked;
        failures.extend(found);
    }
    failures.sort();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}

fn pinned(repo: &mut RepoView, patch: u64) -> TestResult {
    let required = velnor_gen::required_helper(repo).map_or(0, |(_, _, patch)| patch);
    pin_helper(repo, &format!("0.1.{}", patch.max(required)))
}

#[derive(Default)]
struct Sweep {
    scan: Scan,
    problems: Vec<String>,
}

impl Sweep {
    fn add(&mut self, label: &str, files: &BTreeMap<RepoPath, String>) -> TestResult {
        for (_, text) in files.iter().filter(|(path, _)| is_workflow(path)) {
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
        assert_ne!(self.scan.refs, 0, "no steps reference was checked");
    }
}

fn rust_ci_sweep(cases: &[(Option<&str>, Option<&str>)]) -> TestResult {
    let mut sweep = Sweep::default();
    for patch in gate_patches()? {
        for (config, tools) in cases {
            let mut repo = view(ROOTS, config.map(str::as_bytes))?;
            if let Some(tools) = tools {
                repo.files
                    .insert(RepoPath::new("mise.toml")?, tools.as_bytes().to_vec());
            }
            pinned(&mut repo, patch)?;
            sweep.add(&format!("0.1.{patch}, {config:?}"), &generated(&mut repo)?)?;
        }
    }
    sweep.finish();
    Ok(())
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
    let cases: [(&[Root], &str); 3] = [
        (&[APP], "[[binaries]]\npackage = \"app\"\n"),
        (&[APP, TOOLS], TWO_BINARIES),
        (
            &[APP, TOOLS],
            "roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n",
        ),
    ];
    let mut sweep = Sweep::default();
    for patch in gate_patches()? {
        for (roots, release_toml) in cases {
            let mut repo = release_view(roots, Some(release_toml))?;
            pinned(&mut repo, patch)?;
            sweep.add(
                &format!("0.1.{patch}, {release_toml:?}"),
                &expected_release(&repo)?,
            )?;
        }
    }
    sweep.finish();
    Ok(())
}
