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
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::SECTION;
use crate::release_fixture::{APP, Root, TOOLS, TWO_BINARIES, release_view};
use crate::release_unguarded::expected_release;
use crate::step_refs::{Callees, Scan, dangling_refs, scan_workflow};
use crate::test_groups::{DATABASE, SHELL};

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";
const ARTIFACTS_ON: &str = "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nneeds = ['cargo']\nartifacts = [{ name = 'shell-failures', paths = ['junit.xml'], when = 'failure' }]\n";
const RELEASE_ASSETS_ON: &str = "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\ndest = 'tests/fixtures/dataset'\n";
const PR_ONLY_CHECKS_ON: &str = "pr_only_checks = [\"DCO\"]\n";
const PR_ONLY_CHECKS_MINIMUM: u64 = 82;
const PRIVATE_BASE_MINIMUM: u64 = 111;

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

pub(crate) fn gate_patches() -> Result<BTreeSet<u64>, Box<dyn Error>> {
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

pub(crate) fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    velnor_gen::key_gates()
        .find(|gate| gate.key == key && gate.value.is_none())
        .map(|gate| gate.first.2)
        .ok_or_else(|| format!("the {key} gate is missing").into())
}

fn is_workflow(path: &RepoPath) -> bool {
    path.to_string().starts_with(".github/workflows/")
}

fn sweep_job(
    patch: u64,
    config: Option<&str>,
    seen: &Mutex<HashSet<(Option<String>, u64)>>,
) -> Result<(usize, Vec<String>), Box<dyn Error>> {
    let mut repo = view(ROOTS, config.map(str::as_bytes))?;
    let effective = pinned(&mut repo, patch)?;
    if !seen
        .lock()
        .map_err(|_| "the render set is poisoned")?
        .insert((config.map(str::to_owned), effective))
    {
        return Ok((0, Vec::new()));
    }
    let files = generated(&mut repo)?;
    let callees = callees_of(&files)?;
    let mut scan = Scan::default();
    for (_, text) in files.iter().filter(|(path, _)| is_workflow(path)) {
        scan_workflow(&crate::yaml_tree::document(text)?, &callees, &mut scan);
    }
    let failures = scan
        .dangling
        .into_iter()
        .map(|found| format!("helper 0.1.{effective}, config {config:?}: {found}"))
        .collect();
    Ok((scan.refs, failures))
}

fn callees_of(files: &BTreeMap<RepoPath, String>) -> Result<Callees, Box<dyn Error>> {
    let mut callees = Callees::new();
    for (path, text) in files.iter().filter(|(path, _)| is_workflow(path)) {
        let document = crate::yaml_tree::document(text)?;
        let names: BTreeSet<String> = document["on"]["workflow_call"]["outputs"]
            .as_object()
            .map_or_else(BTreeSet::new, |outputs| outputs.keys().cloned().collect());
        callees.insert(format!("./{path}"), names.clone());
        callees.insert(path.to_string(), names);
    }
    Ok(callees)
}

pub(crate) fn generated(repo: &mut RepoView) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
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

pub(crate) fn pinned(repo: &mut RepoView, patch: u64) -> Result<u64, Box<dyn Error>> {
    let required = velnor_gen::required_helper(repo).map_or(0, |(_, _, patch)| patch);
    let effective = patch.max(required);
    pin_helper(repo, &format!("0.1.{effective}"))?;
    Ok(effective)
}

#[derive(Default)]
pub(crate) struct Sweep {
    scan: Scan,
    problems: Vec<String>,
}

impl Sweep {
    pub(crate) fn add(&mut self, label: &str, files: &BTreeMap<RepoPath, String>) -> TestResult {
        let callees = callees_of(files)?;
        for (_, text) in files.iter().filter(|(path, _)| is_workflow(path)) {
            scan_workflow(&crate::yaml_tree::document(text)?, &callees, &mut self.scan);
        }
        self.problems.extend(
            self.scan
                .dangling
                .drain(..)
                .map(|dangling| format!("{label}: {dangling}")),
        );
        Ok(())
    }

    pub(crate) fn finish(self) {
        assert!(self.problems.is_empty(), "{:#?}", self.problems);
        assert_ne!(self.scan.refs, 0, "no steps reference was checked");
    }
}

#[derive(Clone, Copy)]
struct Case<'a> {
    from: u64,
    config: Option<&'a str>,
    files: &'a [(&'a str, &'a str)],
}

fn rust_ci_sweep(cases: &[Case<'_>]) -> TestResult {
    let mut sweep = Sweep::default();
    let mut rendered = BTreeSet::new();
    for patch in gate_patches()? {
        for (index, case) in cases.iter().enumerate().filter(|(_, c)| patch >= c.from) {
            let mut repo = view(ROOTS, case.config.map(str::as_bytes))?;
            for (path, text) in case.files {
                repo.files
                    .insert(RepoPath::new(*path)?, text.as_bytes().to_vec());
            }
            let effective = pinned(&mut repo, patch)?;
            if rendered.insert((index, effective)) {
                sweep.add(
                    &format!("0.1.{effective}, {:?}", case.config),
                    &generated(&mut repo)?,
                )?;
            }
        }
    }
    sweep.finish();
    Ok(())
}

#[test]
fn every_steps_reference_in_a_feature_lane_or_test_group_names_a_step_of_its_own_job() -> TestResult
{
    let groups = format!("{DATABASE}root = '.'\n{SHELL}root = '.'\n");
    let artifacts = gate_patch("test_groups.artifacts")?;
    rust_ci_sweep(&[
        Case {
            from: 0,
            config: Some(LANE),
            files: &[],
        },
        Case {
            from: 0,
            config: Some(groups.as_str()),
            files: &[],
        },
        Case {
            from: artifacts,
            config: Some(ARTIFACTS_ON),
            files: &[],
        },
    ])
}

#[test]
fn every_steps_reference_in_a_lint_test_setup_or_private_sources_job_names_a_step_of_its_own_job()
-> TestResult {
    let release_assets = gate_patch("release_assets")?;
    let private_assets = format!("{SECTION}{RELEASE_ASSETS_ON}");
    rust_ci_sweep(&[
        Case {
            from: 0,
            config: Some(LINT_LANE),
            files: &[("mise.toml", MISE)],
        },
        Case {
            from: 0,
            config: Some(SETUP),
            files: &[],
        },
        Case {
            from: 0,
            config: Some(SECTION),
            files: &[],
        },
        Case {
            from: release_assets,
            config: Some(private_assets.as_str()),
            files: &[],
        },
        Case {
            from: PRIVATE_BASE_MINIMUM,
            config: Some(SECTION),
            files: &[("Cargo.lock", GIT_LOCK), ("tools/Cargo.lock", GIT_LOCK)],
        },
    ])
}

#[test]
fn every_steps_reference_in_the_release_workflow_names_a_step_of_its_own_job() -> TestResult {
    let cases: [(u64, &[Root], &str); 4] = [
        (0, &[APP], "[[binaries]]\npackage = \"app\"\n"),
        (0, &[APP, TOOLS], TWO_BINARIES),
        (
            0,
            &[APP, TOOLS],
            "roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n",
        ),
        (PR_ONLY_CHECKS_MINIMUM, &[APP], PR_ONLY_CHECKS_ON),
    ];
    let mut sweep = Sweep::default();
    let mut rendered = BTreeSet::new();
    for patch in gate_patches()? {
        for (from, roots, release_toml) in cases {
            if patch < from {
                continue;
            }
            let mut repo = release_view(roots, Some(release_toml))?;
            let effective = pinned(&mut repo, patch)?;
            if rendered.insert((release_toml, effective)) {
                sweep.add(
                    &format!("0.1.{effective}, {release_toml:?}"),
                    &expected_release(&repo)?,
                )?;
            }
        }
    }
    sweep.finish();
    Ok(())
}

#[test]
fn a_needs_output_of_an_outputless_local_reusable_workflow_is_dangling() -> TestResult {
    let mut files = BTreeMap::new();
    files.insert(
        RepoPath::new(".github/workflows/release.yml")?,
        "on:\n  workflow_call: {}\njobs: {}\n".to_owned(),
    );
    files.insert(
        RepoPath::new(".github/workflows/rust-ci.yml")?,
        "on: push\njobs:\n  release:\n    uses: ./.github/workflows/release.yml\n  deploy:\n    needs: [release]\n    steps:\n      - run: echo ${{ needs.release.outputs.nope }}\n".to_owned(),
    );
    let callees = callees_of(&files)?;
    let caller =
        crate::yaml_tree::document(&files[&RepoPath::new(".github/workflows/rust-ci.yml")?])?;
    assert_eq!(dangling_refs(&caller, &callees).1.len(), 1);
    Ok(())
}
