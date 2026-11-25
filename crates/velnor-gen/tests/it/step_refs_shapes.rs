// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use velnor_gen::GenerateError;
use velnor_model::RepoPath;
use velnor_render::{DELTA_CLEANUP_HELPER, DELTA_CLEANUP_PATH};

use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::view;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::SECTION;
use crate::public_sources::{DECLARED as PUBLIC_SOURCES, SHA, SOURCE};
use crate::step_refs_sweep::{ROOTS, Sweep, gate_patch, gate_patches, generated, pinned};

const TEST_GROUP: &str =
    "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\n";
const RELEASE_ASSETS: &str = "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\ndest = 'tests/fixtures/dataset'\n";

pub(crate) type Keys = &'static [&'static str];

pub(crate) fn first_patch(keys: Keys) -> Result<u64, Box<dyn Error>> {
    let mut first = 0;
    for key in keys {
        first = first.max(gate_patch(key)?);
    }
    Ok(first)
}

pub(crate) type Rendered = (u64, BTreeMap<RepoPath, String>);

pub(crate) trait Shape: Sync {
    fn label(&self) -> &'static str;
    fn keys(&self) -> Keys;
    fn render(&self, patch: u64) -> Result<Option<Rendered>, Box<dyn Error>>;
}

pub(crate) fn sweep_shapes<S: Shape>(shapes: &[S]) -> TestResult {
    let patches = gate_patches()?;
    let mut work = Vec::new();
    for (index, shape) in shapes.iter().enumerate() {
        let first = first_patch(shape.keys())?;
        work.extend(
            patches
                .iter()
                .filter(|patch| **patch >= first)
                .map(|patch| (index, *patch)),
        );
    }
    let seen = Mutex::new(HashSet::new());
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4);
    let results: Vec<Result<Sweep, String>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut sweep = Sweep::default();
                    while let Some(&(index, patch)) = work.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let shape = &shapes[index];
                        let rendered = shape.render(patch).map_err(|error| error.to_string())?;
                        let Some((effective, files)) = rendered else {
                            continue;
                        };
                        let fresh = seen
                            .lock()
                            .map_err(|_| "the render set is poisoned".to_owned())?
                            .insert((index, effective));
                        if fresh {
                            sweep
                                .add(&format!("0.1.{effective}, {}", shape.label()), &files)
                                .map_err(|error| error.to_string())?;
                        }
                    }
                    Ok(sweep)
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
    let mut sweep = Sweep::default();
    for result in results {
        sweep.absorb(result?);
    }
    sweep.finish();
    Ok(())
}

struct RustCi {
    label: &'static str,
    keys: Keys,
    config: Option<String>,
    files: Vec<(&'static str, String)>,
    cleanup: bool,
    roots: &'static [&'static str],
}

fn rust_ci(label: &'static str, keys: Keys, config: Option<String>) -> RustCi {
    RustCi {
        label,
        keys,
        config,
        files: Vec::new(),
        cleanup: false,
        roots: ROOTS,
    }
}

impl RustCi {
    fn single(mut self) -> Self {
        self.roots = &["Cargo.toml"];
        self
    }

    fn with(mut self, path: &'static str, text: impl Into<String>) -> Self {
        self.files.push((path, text.into()));
        self
    }
}

impl Shape for RustCi {
    fn label(&self) -> &'static str {
        self.label
    }

    fn keys(&self) -> Keys {
        self.keys
    }

    fn render(&self, pin: u64) -> Result<Option<Rendered>, Box<dyn Error>> {
        let mut repo = view(self.roots, self.config.as_deref().map(str::as_bytes))?;
        for (name, text) in &self.files {
            let path = RepoPath::new(*name)?;
            repo.tracked_paths.insert(path.clone());
            repo.files.insert(path, text.as_bytes().to_vec());
        }
        let effective = pinned(&mut repo, pin)?;
        let files = match generated(&mut repo) {
            Ok(files) => files,
            Err(error)
                if matches!(
                    error.downcast_ref::<GenerateError>(),
                    Some(GenerateError::MergeGroupHelperTooOld(_))
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if self.cleanup
            && effective >= DELTA_CLEANUP_HELPER.2
            && !files.contains_key(&DELTA_CLEANUP_PATH)
        {
            return Err(format!(
                "0.1.{effective}, {}: no cache-cleanup workflow was rendered",
                self.label
            )
            .into());
        }
        Ok(Some((effective, files)))
    }
}

fn public_lock() -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep0\"\nversion = \"1.0.0\"\nsource = \"git+{SOURCE}?rev=v1#{SHA}\"\n"
    )
}

pub(crate) fn mixed_lock() -> String {
    format!(
        "{GIT_LOCK}\n[[package]]\nname = \"dep1\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/public-lib?rev=v1#{SHA}\"\n"
    )
}

fn rust_ci_shapes() -> Vec<RustCi> {
    let shape = |label, keys, extra: &str| rust_ci(label, keys, Some(extra.to_owned()));
    vec![
        RustCi {
            cleanup: true,
            ..rust_ci("default", &[], None)
        },
        shape(
            "artifacts",
            &["test_groups.artifacts"],
            &format!(
                "{TEST_GROUP}needs = ['cargo']\nartifacts = [{{ name = 'shell-failures', paths = ['junit.xml'], when = 'failure' }}]\n"
            ),
        ),
        shape(
            "partitions",
            &["test_groups.partitions"],
            "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\npartitions = 2\n",
        ),
        shape(
            "restore_cache",
            &["test_groups.restore_cache"],
            &format!("{TEST_GROUP}needs = ['cargo']\nrestore_cache = true\n"),
        ),
        shape(
            "hosted",
            &["test_groups.hosted"],
            &format!("{TEST_GROUP}runner = 'ubuntu-26.04'\nhosted = true\n"),
        ),
        shape(
            "no_tests",
            &["test_groups.no_tests"],
            &format!("{TEST_GROUP}no_tests = 'warn'\n"),
        ),
        shape(
            "workspace group",
            &["workspace_groups"],
            "[[workspace_groups]]\nname = 'pair'\nroots = ['.', 'tools']\n",
        ),
        shape(
            "parallel test lane",
            &["test_lane.job"],
            "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\njob = 'parallel'\n",
        ),
        shape("lint-test", &["shape"], "shape = 'lint-test'\n"),
        shape(
            "mbx remote",
            &["cache.mbx_remote"],
            "[cache]\nmbx_remote = \"github-actions\"\n",
        ),
        shape(
            "merge-group skip",
            &["ci.skip_push_after_merge_group"],
            "[ci]\nskip_push_after_merge_group = true\n",
        ),
        shape(
            "private release assets",
            &["private_sources", "release_assets"],
            &format!("{SECTION}{RELEASE_ASSETS}"),
        ),
        shape("private base caches", &["private_sources"], SECTION)
            .with("Cargo.lock", GIT_LOCK)
            .with("tools/Cargo.lock", GIT_LOCK),
        shape("public sources", &["public_sources"], PUBLIC_SOURCES)
            .with("Cargo.lock", public_lock())
            .with("tools/Cargo.lock", public_lock()),
        shape(
            "public and private sources",
            &["private_sources", "public_sources"],
            &format!("{SECTION}[public_sources]\ngit = ['https://github.com/acme/public-lib']\n"),
        )
        .with("Cargo.lock", mixed_lock())
        .single(),
        shape(
            "dependency policy",
            &["policy.deny"],
            "[policy]\ndeny = true\n",
        )
        .with("deny.toml", "")
        .with("mise.toml", DENY_MISE),
    ]
}

#[test]
fn every_steps_reference_in_a_rust_ci_shape_names_a_step_of_its_own_job() -> TestResult {
    sweep_shapes(&rust_ci_shapes())
}
