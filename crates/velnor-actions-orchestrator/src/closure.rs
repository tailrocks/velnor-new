//! Complete first-party input closures with explicit unknowns.
//! Declared via `#[path]` from `internal_plan.rs`; unknown inputs forbid reuse and coverage.
use super::snapshot::normalize_checkout_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use velnor_actions_contract::{ContractError, digest_b3};
use velnor_actions_rust::{TaskGroup, TaskKind};
/// Provenance of one semantic input: knowledge, never assumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Provenance {
    Known { digest: String },
    AbsentProven { evidence: String },
    GuardedExternally { guard: String },
    Unknown { reason: String },
}
fn known(digest: String) -> Provenance {
    Provenance::Known { digest }
}
fn absent(evidence: String) -> Provenance {
    Provenance::AbsentProven { evidence }
}
fn unknown(reason: String) -> Provenance {
    Provenance::Unknown { reason }
}
/// Complete first-party input closure for one task (P03).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskInputClosure {
    pub(crate) task_id: String,
    pub(crate) inputs: BTreeMap<String, Provenance>,
}
impl TaskInputClosure {
    /// Names of explicitly unknown inputs, sorted (`BTreeMap` order).
    pub(crate) fn unknown_inputs(&self) -> Vec<&str> {
        self.inputs
            .iter()
            .filter(|(_, p)| matches!(p, Provenance::Unknown { .. }))
            .map(|(n, _)| n.as_str())
            .collect()
    }
    /// Refuse reuse and coverage while any input is explicitly unknown.
    pub(crate) fn verify_complete(&self) -> Result<(), ContractError> {
        let unknown = self.unknown_inputs();
        if !unknown.is_empty() {
            return Err(ContractError::identity(
                "input_closure",
                format!("incomplete_inputs:{}", unknown.join(",")),
            ));
        }
        Ok(())
    }
}
/// Builder for [`TaskInputClosure`] over one task group.
pub(crate) struct ClosureBuilder {
    inputs: BTreeMap<String, Provenance>,
}
impl ClosureBuilder {
    pub(crate) fn new() -> Self {
        Self {
            inputs: BTreeMap::new(),
        }
    }
    pub(crate) fn input(mut self, name: &str, provenance: Provenance) -> Self {
        self.inputs.insert(name.to_owned(), provenance);
        self
    }
    /// Record a value-bound input (features, target, profile, argv).
    pub(crate) fn value(self, name: &str, value: &str) -> Self {
        self.input(name, known(digest_b3(value.as_bytes())))
    }
    /// Record a precomputed digest input (graph, toolchain, platform).
    fn digest(self, name: &str, digest: &str) -> Self {
        self.input(name, known(digest.to_owned()))
    }
    pub(crate) fn build(self, task_id: &str) -> TaskInputClosure {
        TaskInputClosure {
            task_id: task_id.to_owned(),
            inputs: self.inputs,
        }
    }
}

/// Resolve one task group's closure against the checkout at `root`.
pub(crate) fn resolve_closure_at_root(
    root: &Path,
    group: &TaskGroup,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
) -> TaskInputClosure {
    let manifest = super::manifest_for_key(&group.manifest_key);
    let nextest = probe_nextest_config(root, profile_nextest_config);
    let docs = class_provenance(root, group, &manifest, Class::Docs);
    let fixtures = class_provenance(root, group, &manifest, Class::Fixtures);
    let schemas = class_provenance(root, group, &manifest, Class::Schemas);
    let mut closure = ClosureBuilder::new()
        .input("source_tree", source_tree_provenance(root, &manifest))
        .input("manifest", probe_file(root, &manifest))
        .input("lockfile", probe_lockfile(root, &manifest))
        .input("nextest_config", nextest)
        .input("cargo_config", probe_cargo_config(root, &manifest))
        .input("docs", docs)
        .input("fixtures", fixtures)
        .input("schemas", schemas)
        .digest("local_deps", graph_digest)
        .digest("toolchain", toolchain_id)
        .digest("platform", platform_id)
        .value("features", &group.features.join(","))
        .value("target", &group.target)
        .value("profile", &group.configuration)
        .value("driver", group.compile_driver.as_str())
        .value("runner", group.test_runner.as_str())
        .value("kind", group.kind.as_str());
    for (index, extra) in group.declared_inputs.iter().enumerate() {
        let name = format!("declared_extra:{index}:{extra}");
        closure = closure.input(&name, probe_declared(root, extra));
    }
    let closure = closure.input("vcs", vcs_provenance(group));
    closure.build(&group.task_id)
}
/// Provenance of one file: content digest, proven absence, or unknown.
fn probe_file(root: &Path, path: &str) -> Provenance {
    let Ok(normalized) = normalize_checkout_path(path) else {
        return unknown(format!("bad_path:{path}"));
    };
    match std::fs::read(root.join(&normalized)) {
        Ok(bytes) => known(digest_b3(&bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            absent(format!("not_found:{normalized}"))
        }
        Err(err) => unknown(format!("unreadable:{normalized}:{err}")),
    }
}
/// Decisive probes win; absence and guards fall through to the next probe.
macro_rules! triage {
    ($root:expr, $candidate:expr) => {
        match probe_file($root, $candidate) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
    };
}
/// Lockfile provenance, walking up from the manifest like Cargo does.
pub(crate) fn probe_lockfile(root: &Path, manifest: &str) -> Provenance {
    let mut dir = package_dir(manifest).to_owned();
    let mut probed = Vec::new();
    loop {
        let candidate = if dir.is_empty() {
            "Cargo.lock".to_owned()
        } else {
            format!("{dir}/Cargo.lock")
        };
        triage!(root, &candidate);
        probed.push(candidate);
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_owned(),
            None if dir.is_empty() => break,
            None => dir.clear(),
        }
    }
    absent(format!("not_found:{}", probed.join(",")))
}
/// Nextest-config provenance: profile path plus the conventional path.
pub(crate) fn probe_nextest_config(root: &Path, profile_config: Option<&str>) -> Provenance {
    const CONVENTIONAL: &str = ".config/nextest.toml";
    if let Some(configured) = profile_config {
        triage!(root, configured);
        if configured == CONVENTIONAL {
            return absent(format!("not_found:{CONVENTIONAL}"));
        }
    }
    match probe_file(root, CONVENTIONAL) {
        known @ Provenance::Known { .. } => known,
        unknown @ Provenance::Unknown { .. } => unknown,
        Provenance::AbsentProven { evidence } => {
            absent(format!("profile:{profile_config:?}:{evidence}"))
        }
        guarded @ Provenance::GuardedExternally { .. } => guarded,
    }
}
/// Cargo-config provenance: manifest dir plus the repository root.
fn probe_cargo_config(root: &Path, manifest: &str) -> Provenance {
    let dir = package_dir(manifest);
    let mut candidates = Vec::new();
    if !dir.is_empty() {
        candidates.push(format!("{dir}/.cargo/config.toml"));
    }
    candidates.push(".cargo/config.toml".to_owned());
    for candidate in &candidates {
        triage!(root, candidate);
    }
    absent(format!("not_found:{}", candidates.join(",")))
}
/// Declared-extra provenance: missing means unknown, never absent.
fn probe_declared(root: &Path, path: &str) -> Provenance {
    match probe_file(root, path) {
        Provenance::AbsentProven { evidence } => {
            unknown(format!("declared_but_missing:{evidence}"))
        }
        other => other,
    }
}
/// VCS provenance: unknown with build scripts, proven absent without.
fn vcs_provenance(group: &TaskGroup) -> Provenance {
    if group.undeclared_reads {
        unknown("build_script_may_observe_vcs".to_owned())
    } else {
        absent("offline_cargo_no_vcs_reads".to_owned())
    }
}
fn package_dir(manifest: &str) -> &str {
    manifest.rsplit_once('/').map_or("", |(dir, _)| dir)
}
/// Digest over sorted `(path, digest)` file pairs.
fn files_digest(files: &[(String, String)]) -> String {
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, digest)| (path.as_str(), digest.as_str()))
        .collect();
    super::snapshot::canonical_digest(&pairs).unwrap_or_else(|_| digest_b3(b"files_error"))
}
/// Walk the package dir skipping `target`/`.git`, collecting `(path, digest)`
/// pairs accepted by `keep`. Strict fails on symlinks/unreadable entries; lenient skips.
fn walk_package_files(
    root: &Path,
    manifest: &str,
    keep: &dyn Fn(&Path, &str) -> bool,
    cap: usize,
    cap_reason: &'static str,
    strict: bool,
) -> Result<Vec<(String, String)>, String> {
    let dir = package_dir(manifest);
    let mut base = root.to_path_buf();
    if !dir.is_empty() {
        base.push(dir);
    }
    let mut files = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(err) if strict => {
                return Err(format!("unreadable_dir:{}:{err}", current.display()));
            }
            Err(_) => return Err(format!("unreadable_dir:{}", current.display())),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) if !strict => continue,
                Err(err) => return Err(format!("unreadable_entry:{}:{err}", current.display())),
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) if !strict => return Err("unreadable_type".to_owned()),
                Err(err) => return Err(format!("unreadable_type:{}:{err}", current.display())),
            };
            if file_type.is_symlink() {
                if strict {
                    return Err(format!("symlink:{}", entry.path().display()));
                }
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                if name != "target" && name != ".git" {
                    stack.push(entry.path());
                }
                continue;
            }
            let full = entry.path();
            let Ok(rel) = full.strip_prefix(root) else {
                if strict {
                    return Err("outside_root".to_owned());
                }
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !keep(&full, &rel) {
                continue;
            }
            match std::fs::read(&full) {
                Ok(bytes) => files.push((rel, digest_b3(&bytes))),
                Err(_) if !strict => {}
                Err(err) => return Err(format!("unreadable:{}:{err}", full.display())),
            }
            if files.len() > cap {
                return Err(cap_reason.to_owned());
            }
        }
    }
    files.sort();
    Ok(files)
}
/// Source-tree provenance: digest over package `*.rs` plus the manifest.
/// A source edit flips the closure digest; an empty tree is unknown.
fn source_tree_provenance(root: &Path, manifest: &str) -> Provenance {
    let is_source = |path: &Path, _: &str| path.extension().is_some_and(|ext| ext == "rs");
    let mut files = match walk_package_files(
        root,
        manifest,
        &is_source,
        2000,
        "too_many_source_files",
        true,
    ) {
        Ok(files) => files,
        Err(reason) => return unknown(reason),
    };
    if let Ok(normalized) = normalize_checkout_path(manifest)
        && let Ok(bytes) = std::fs::read(root.join(&normalized))
    {
        files.push((normalized, digest_b3(&bytes)));
        files.sort();
    }
    if files.is_empty() {
        return unknown("no_source_files".to_owned());
    }
    known(files_digest(&files))
}
/// Schema-definition extensions collected for compiling kinds.
const SCHEMA_EXTS: [&str; 3] = ["proto", "graphql", "avsc"];
/// Conventional input class with kind-gated collection.
#[derive(Clone, Copy)]
enum Class {
    Docs,
    Fixtures,
    Schemas,
}
impl Class {
    /// Exclusion evidence when `kind` never consumes this class.
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
    /// True for paths this class collects.
    fn keep(self, path: &str) -> bool {
        match self {
            Self::Docs => ext_is(path, "md") || path.contains("/docs/") || path.ends_with("README"),
            Self::Fixtures => {
                path.contains("fixtures/") || path.contains("tests/") && ext_is(path, "json")
            }
            Self::Schemas => {
                path.contains("schemas/") || SCHEMA_EXTS.iter().any(|w| ext_is(path, w))
            }
        }
    }
}
/// Class provenance: exclusion for non-consuming kinds, else digest or absence.
fn class_provenance(root: &Path, group: &TaskGroup, manifest: &str, class: Class) -> Provenance {
    if let Some(evidence) = class.excluded(group.kind) {
        return absent(evidence.to_owned());
    }
    let keep = |_: &Path, path: &str| class.keep(path);
    match walk_package_files(root, manifest, &keep, 500, "too_many_class_files", false) {
        Err(reason) => unknown(reason),
        Ok(files) if files.is_empty() => absent("probed_no_class_files".to_owned()),
        Ok(files) => known(files_digest(&files)),
    }
}
fn ext_is(path: &str, want: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
}
