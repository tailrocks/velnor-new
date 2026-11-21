// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde_json::Value;
use velnor_model::WORKFLOWS_CONFIG_PATH;

use crate::facts::{TestResult, lock_pinned_tools, path};
use crate::pipeline::{pin_helper, view};
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};
use crate::release_family_fixture;
use crate::release_fixture;
use crate::release_fixture::APP;
use crate::step_refs_parse::dangling_refs;
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";
const PR_ONLY_CHECKS_ON: &str = "pr_only_checks = [\"DCO\"]\n";
const RELEASE_ASSETS_ON: &str = "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\ndest = 'tests/fixtures/dataset'\n";
const RELEASE_BINARIES: &str = "[[binaries]]\npackage = \"app\"\n";
const ARTIFACTS_ON: &str = "[[test_groups]]\nname = 'shell'\nfilter = 'binary(it) and test(~cargo_)'\nneeds = ['cargo']\nartifacts = [{ name = 'shell-failures', paths = ['junit.xml'], when = 'failure' }]\n";
const TWO_GROUPS: &str = "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\nneeds = ['docker']\ntest_threads = 2\ntimeout_minutes = 45\nrunner = 'velnor-box'\n[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nneeds = ['cargo']\n";
const PR_ONLY_CHECKS_MINIMUM: u64 = 82;
const PRIVATE_BASE_MINIMUM: u64 = 111;
const FAMILY_KEYS: [&str; 5] = [
    "roots",
    "family_tag",
    "binaries.package",
    "binaries.smoke",
    "binaries.platforms",
];
const DECLARED: &str = "[[workflow]]\nfile = \"declared\"\nname = \"Declared\"\n\n[[workflow.job]]\nid = \"check\"\n\n[[workflow.job.step]]\nid = \"warm\"\nname = \"Warm the cache\"\nrun = \"echo ok\"\n\n[[workflow.job.step]]\nname = \"Use the warm step\"\nrun = \"echo $WARM\"\nenv = { WARM = \"${{ steps.warm.outputs.ready }}\" }\n";

#[derive(Clone, Copy, Debug)]
enum Shape {
    Rust(Option<&'static str>),
    Groups(&'static str),
    Private,
    PrivateBase,
    PrivateAssets,
    Declared,
    Release(&'static str),
    Family,
}

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
    for krate in [
        "velnor-gen",
        "velnor-rust",
        "velnor-render",
        "velnor-model",
        "velnor-ci",
        "velnor-release",
        "velnor-workflows",
    ] {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(krate)
            .join("src");
        rust_sources(&src, &mut files)?;
    }
    let mut patches: BTreeSet<u64> = velnor_gen::key_gates().map(|gate| gate.first.2).collect();
    for file in files {
        let text = fs::read_to_string(file)?;
        patches.extend(patches_in(&text));
    }
    patches.retain(|patch| *patch >= velnor_gen::HELPER_MINIMUM.2);
    Ok(patches)
}

fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    velnor_gen::key_gates()
        .find(|gate| gate.key == key && gate.value.is_none())
        .map(|gate| gate.first.2)
        .ok_or_else(|| format!("the {key} gate is missing").into())
}

fn family_first() -> Result<u64, Box<dyn Error>> {
    let mut first = 0;
    for key in FAMILY_KEYS {
        first = first.max(gate_patch(key)?);
    }
    Ok(first)
}

fn documents(shape: Shape, helper: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut repo = match shape {
        Shape::Family => {
            let (text, _) =
                release_family_fixture::generate(helper, &release_family_fixture::tools_like(""))?;
            return Ok(vec![document(&text)?]);
        }
        Shape::Rust(config) => {
            let mut repo = view(
                &["Cargo.toml", "tools/Cargo.toml"],
                config.map(str::as_bytes),
            )?;
            pin_helper(&mut repo, helper)?;
            repo
        }
        Shape::Groups(config) => view_with(&["Cargo.toml"], config, helper)?,
        Shape::Private => private_repo(&["Cargo.toml"], SECTION, helper)?,
        Shape::PrivateAssets => {
            let config = format!("{SECTION}{RELEASE_ASSETS_ON}");
            private_repo(&["Cargo.toml"], &config, helper)?
        }
        Shape::PrivateBase => {
            let mut repo = private_repo(&["Cargo.toml"], SECTION, helper)?;
            repo.files
                .insert(path("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
            repo
        }
        Shape::Declared => {
            let mut repo = release_fixture::release_view(&[], None)?;
            repo.files
                .insert(WORKFLOWS_CONFIG_PATH, DECLARED.as_bytes().to_vec());
            pin_helper(&mut repo, helper)?;
            repo
        }
        Shape::Release(config) => {
            let (text, _) = release_fixture::render_pinned(&[APP], Some(config), helper)?;
            return Ok(vec![document(&text)?]);
        }
    };
    lock_pinned_tools(&mut repo)?;
    let mut docs = Vec::new();
    for (path, text) in velnor_gen::expected_files(&repo)? {
        if Path::new(path.as_str())
            .extension()
            .is_some_and(|ext| ext == "yml")
        {
            docs.push(document(&text)?);
        }
    }
    Ok(docs)
}

fn sweep_job(
    patch: u64,
    shape: Shape,
    seen: &Mutex<HashSet<String>>,
) -> Result<(usize, Vec<String>), Box<dyn Error>> {
    let helper = format!("0.1.{patch}");
    let mut checked = 0;
    let mut failures = Vec::new();
    for doc in documents(shape, &helper)? {
        let key = doc.to_string().replace(&helper, "HELPER");
        if !seen
            .lock()
            .map_err(|_| "the render set is poisoned")?
            .insert(key)
        {
            continue;
        }
        let (found_refs, dangling) = dangling_refs(&doc);
        checked += found_refs;
        failures.extend(
            dangling
                .into_iter()
                .map(|found| format!("helper {helper}, shape {shape:?}: {found}")),
        );
    }
    Ok((checked, failures))
}

fn run_sweep(work: &[(u64, Shape)]) -> Result<(usize, Vec<String>), Box<dyn Error>> {
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
                    while let Some(&(patch, shape)) = work.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let (checked, found) =
                            sweep_job(patch, shape, &seen).map_err(|error| error.to_string())?;
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
    Ok((refs, failures))
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let scanned = gate_patches()?;
    let newest = *scanned.last().ok_or("the gate scan found no helper")?;
    let shared_store = gate_patch("mbx_shared_store")?;
    let cache_debug = gate_patch("cache_debug")?;
    let test_groups = gate_patch("test_groups")?;
    let artifacts = gate_patch("test_groups.artifacts")?;
    let private_sources = gate_patch("private_sources")?;
    let release_assets = gate_patch("release_assets")?;
    let family = family_first()?;
    for gate in [
        velnor_gen::HELPER_MINIMUM.2,
        shared_store,
        cache_debug,
        test_groups,
        artifacts,
        private_sources,
        release_assets,
        PRIVATE_BASE_MINIMUM,
        PR_ONLY_CHECKS_MINIMUM,
    ] {
        assert!(
            scanned.contains(&gate),
            "gate scan found {scanned:?}, which misses the gate at 0.1.{gate}"
        );
    }
    let mut work: Vec<(u64, Shape)> = Vec::new();
    for &patch in &scanned {
        work.push((patch, Shape::Rust(None)));
        if patch >= shared_store {
            work.push((patch, Shape::Rust(Some(SHARED_STORE_ON))));
        }
        if patch >= cache_debug {
            work.push((patch, Shape::Rust(Some(CACHE_DEBUG_ON))));
        }
        if patch >= test_groups {
            work.push((patch, Shape::Groups(TWO_GROUPS)));
        }
        if patch >= artifacts {
            work.push((patch, Shape::Groups(ARTIFACTS_ON)));
        }
        if patch >= private_sources {
            work.push((patch, Shape::Private));
        }
        if patch >= release_assets {
            work.push((patch, Shape::PrivateAssets));
        }
        if patch >= PRIVATE_BASE_MINIMUM {
            work.push((patch, Shape::PrivateBase));
        }
        if patch >= velnor_gen::HELPER_MINIMUM.2 {
            work.push((patch, Shape::Declared));
        }
        if patch >= PR_ONLY_CHECKS_MINIMUM {
            work.push((patch, Shape::Release(PR_ONLY_CHECKS_ON)));
        }
        if patch >= velnor_gen::HELPER_MINIMUM.2 {
            work.push((patch, Shape::Release(RELEASE_BINARIES)));
        }
    }
    for patch in family..=newest {
        work.push((patch, Shape::Family));
    }
    let (refs, mut failures) = run_sweep(&work)?;
    failures.sort();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}
