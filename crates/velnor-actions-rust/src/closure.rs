// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::{
    ClosureBuilder, ContractError, ProposedTask, Provenance, TaskInputClosure, digest_b3,
};

use super::closure_probes::{
    probe_cargo_config, probe_declared, probe_file, probe_lockfile, probe_nextest_config,
    source_tree_files,
};
use crate::identity::DependencyCoverage;
use crate::task_identity::DigestSlot;
use crate::tasks::TaskKind;

#[derive(Debug, Clone, Default)]
pub struct RustClosureCache {
    source_trees: BTreeMap<String, Provenance>,
    files: BTreeMap<String, Provenance>,
    lockfiles: BTreeMap<String, Provenance>,
    nextest_configs: BTreeMap<Option<String>, Provenance>,
    cargo_configs: BTreeMap<String, Provenance>,
    classes: BTreeMap<(String, u8), Provenance>,
}

impl RustClosureCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
) -> Result<TaskInputClosure, ContractError> {
    let mut cache = RustClosureCache::new();
    resolve_closure_at_root_cached(
        root,
        task,
        profile_nextest_config,
        graph_digest,
        toolchain_id,
        platform_id,
        &mut cache,
    )
}

pub fn resolve_closure_at_root_cached(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
    cache: &mut RustClosureCache,
) -> Result<TaskInputClosure, ContractError> {
    let kind = TaskKind::parse(&task.task_kind)?;
    let manifest = task.identity.unit_path.as_str();
    let nextest = cache
        .nextest_configs
        .entry(profile_nextest_config.map(str::to_owned))
        .or_insert_with(|| probe_nextest_config(root, profile_nextest_config))
        .clone();
    let docs = class_provenance_cached(root, kind, manifest, Class::Docs, cache);
    let fixtures = class_provenance_cached(root, kind, manifest, Class::Fixtures, cache);
    let schemas = class_provenance_cached(root, kind, manifest, Class::Schemas, cache);
    let source_tree = cache
        .source_trees
        .entry(manifest.to_owned())
        .or_insert_with(|| source_tree_provenance(root, manifest))
        .clone();
    let manifest_prov = cache
        .files
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_file(root, manifest))
        .clone();
    let lockfile = cache
        .lockfiles
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_lockfile(root, manifest))
        .clone();
    let cargo_config = cache
        .cargo_configs
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_cargo_config(root, manifest))
        .clone();
    let mut closure = ClosureBuilder::new()
        .input("source_tree", source_tree)
        .input("manifest", manifest_prov)
        .input("lockfile", lockfile)
        .input("nextest_config", nextest)
        .input("cargo_config", cargo_config)
        .input("docs", docs)
        .input("fixtures", fixtures)
        .input("schemas", schemas)
        .digest("local_deps", graph_digest)
        .digest("toolchain", toolchain_id)
        .digest("platform", platform_id)
        .value("features", &task.identity.features.join(","))
        .value("target", &task.identity.target)
        .value("profile", &task.configuration)
        .value("driver", &task.identity.compile_driver)
        .value("runner", &task.identity.test_runner)
        .value("kind", &task.task_kind);
    for (index, extra) in task.identity.declared_inputs.iter().enumerate() {
        let name = format!("declared_extra:{index}:{extra}");
        closure = closure.input(&name, probe_declared(root, extra));
    }
    let closure = closure.input("vcs", vcs_provenance(task.identity.undeclared_reads));
    Ok(closure.build(&task.task_id))
}

pub fn resolve_dependency_closure(
    task_id: &str,
    coverage: &DependencyCoverage,
) -> Result<TaskInputClosure, ContractError> {
    if task_id.is_empty() {
        return Err(ContractError::identity(
            "dependency_closure.task_id",
            "unknown_task",
        ));
    }
    for digest in [
        &coverage.dependency_digest,
        &coverage.workspace_digest,
        &coverage.native_digest,
        &coverage.coverage_digest,
    ] {
        Digest::parse(digest).map_err(|_| {
            ContractError::identity("dependency_closure.coverage", "malformed_b3_digest")
        })?;
    }
    Ok(ClosureBuilder::new()
        .digest("dependencies", &coverage.dependency_digest)
        .digest("workspace_deps", &coverage.workspace_digest)
        .digest("native_inputs", &coverage.native_digest)
        .digest("coverage", &coverage.coverage_digest)
        .build(task_id))
}

fn digest_slot(provenance: Provenance) -> DigestSlot {
    match provenance {
        Provenance::Known { digest } => DigestSlot::Known(digest),
        Provenance::AbsentProven { evidence } => DigestSlot::AbsentProven(evidence),
        Provenance::Unknown { reason } => DigestSlot::Unknown(reason),
        Provenance::GuardedExternally { guard } => {
            DigestSlot::Unknown(format!("guarded_externally:{guard}"))
        }
    }
}

#[must_use]
pub fn lock_digest_at_root(root: &Path, manifest: &str) -> DigestSlot {
    digest_slot(probe_lockfile(root, manifest))
}

#[must_use]
pub fn lock_digest_cached(root: &Path, manifest: &str, cache: &mut RustClosureCache) -> DigestSlot {
    let prov = cache
        .lockfiles
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_lockfile(root, manifest))
        .clone();
    digest_slot(prov)
}

#[must_use]
pub fn nextest_digest_at_root(root: &Path, profile_config: Option<&str>) -> DigestSlot {
    digest_slot(probe_nextest_config(root, profile_config))
}

#[must_use]
pub fn nextest_digest_cached(
    root: &Path,
    profile_config: Option<&str>,
    cache: &mut RustClosureCache,
) -> DigestSlot {
    let prov = cache
        .nextest_configs
        .entry(profile_config.map(str::to_owned))
        .or_insert_with(|| probe_nextest_config(root, profile_config))
        .clone();
    digest_slot(prov)
}

fn vcs_provenance(undeclared_reads: bool) -> Provenance {
    if undeclared_reads {
        Provenance::Unknown {
            reason: "build_script_may_observe_vcs".to_owned(),
        }
    } else {
        Provenance::AbsentProven {
            evidence: "offline_cargo_no_vcs_reads".to_owned(),
        }
    }
}

fn source_tree_provenance(root: &Path, manifest: &str) -> Provenance {
    let mut files = match source_tree_files(root, manifest) {
        Ok(files) => files,
        Err(reason) => {
            return Provenance::Unknown { reason };
        }
    };
    if let Ok((normalized, bytes)) = read_manifest_bytes(root, manifest) {
        files.push((normalized, digest_b3(&bytes)));
        files.sort();
    }
    if files.is_empty() {
        return Provenance::Unknown {
            reason: "no_source_files".to_owned(),
        };
    }
    Provenance::Known {
        digest: files_digest(&files),
    }
}

fn read_manifest_bytes(root: &Path, manifest: &str) -> Result<(String, Vec<u8>), String> {
    let normalized =
        crate::identity::normalize_identity_path(manifest).map_err(|err| err.to_string())?;
    let bytes = std::fs::read(root.join(&normalized)).map_err(|err| err.to_string())?;
    Ok((normalized, bytes))
}

fn files_digest(files: &[(String, String)]) -> String {
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, digest)| (path.as_str(), digest.as_str()))
        .collect();
    canonical_digest(&pairs).unwrap_or_else(|_| digest_b3(b"files_error"))
}

fn canonical_digest<T: serde::Serialize>(value: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&velnor_actions_contract::canonical_json_bytes(
        value,
    )?))
}

#[derive(Clone, Copy)]
enum Class {
    Docs,
    Fixtures,
    Schemas,
}

impl Class {
    fn excluded(self, kind: TaskKind) -> Option<&'static str> {
        match self {
            Self::Docs if !matches!(kind, TaskKind::Doc | TaskKind::Doctest) => {
                Some("excluded:kind_does_not_render_docs")
            }
            Self::Fixtures
                if !matches!(kind, TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest) =>
            {
                Some("excluded:kind_does_not_execute_tests")
            }
            Self::Schemas if kind == TaskKind::Fmt => Some("excluded:kind_does_not_compile"),
            _ => None,
        }
    }

    fn keep(self, path: &str) -> bool {
        match self {
            Self::Docs => {
                super::closure_probes::ext_is(path, "md")
                    || path.contains("/docs/")
                    || path.ends_with("README")
            }
            Self::Fixtures => {
                path.contains("fixtures/")
                    || path.contains("tests/") && super::closure_probes::ext_is(path, "json")
            }
            Self::Schemas => {
                path.contains("schemas/")
                    || super::closure_probes::SCHEMA_EXTS
                        .iter()
                        .any(|want| super::closure_probes::ext_is(path, want))
            }
        }
    }
}

fn class_provenance_cached(
    root: &Path,
    kind: TaskKind,
    manifest: &str,
    class: Class,
    cache: &mut RustClosureCache,
) -> Provenance {
    if let Some(evidence) = class.excluded(kind) {
        return Provenance::AbsentProven {
            evidence: evidence.to_owned(),
        };
    }
    cache
        .classes
        .entry((manifest.to_owned(), class as u8))
        .or_insert_with(|| {
            let keep = |_: &Path, path: &str| class.keep(path);
            match super::closure_probes::walk_package_files(root, manifest, &keep) {
                Err(reason) => Provenance::Unknown { reason },
                Ok(files) if files.is_empty() => Provenance::AbsentProven {
                    evidence: "probed_no_class_files".to_owned(),
                },
                Ok(files) => Provenance::Known {
                    digest: files_digest(&files),
                },
            }
        })
        .clone()
}
