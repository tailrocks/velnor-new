//! Complete first-party input closures per task with explicit unknowns.
//!
//! Declared via `#[path]` from `internal_plan.rs` (no `lib.rs` edit).
//! Every semantic input carries a [`Provenance`]: content-bound, proven
//! absent, guarded externally, or explicitly unknown. Unknown inputs
//! forbid reuse and coverage; `None`/empty is never proven absent.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{ContractError, digest_b3};
use velnor_actions_rust::TaskGroup;

use super::snapshot::normalize_checkout_path;

/// Provenance of one semantic input: knowledge, never assumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Provenance {
    /// Content-bound: the digest commits to observed bytes.
    Known {
        /// Content digest (`b3-` + hex).
        digest: String,
    },
    /// Proven absent: the evidence names how absence was established.
    AbsentProven {
        /// Absence evidence (paths probed, checks run).
        evidence: String,
    },
    /// Guarded outside the closure (e.g. the changed-set guard).
    GuardedExternally {
        /// Guard enforcing the input at the use site.
        guard: String,
    },
    /// Unknown: the reason names what could not be established.
    Unknown {
        /// What is unknown and why.
        reason: String,
    },
}

impl Provenance {
    /// True only for states that permit reuse and coverage.
    fn complete(&self) -> bool {
        !matches!(self, Self::Unknown { .. })
    }
}

/// Complete first-party input closure for one task (P03).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskInputClosure {
    /// Stable task ID.
    pub(crate) task_id: String,
    /// Semantic inputs with provenance (source, manifests, lockfile,
    /// Cargo config, Nextest config, features, target, profile, local
    /// deps, build scripts, declared extras, docs, fixtures, env, VCS).
    pub(crate) inputs: BTreeMap<String, Provenance>,
}

impl TaskInputClosure {
    /// Names of explicitly unknown inputs, sorted.
    ///
    /// Canonical digests over the closure distinguish every state: a
    /// known digest, a proven absence, an external guard, and an
    /// unknown state never collapse.
    pub(crate) fn unknown_inputs(&self) -> Vec<&str> {
        let mut unknown: Vec<&str> = self
            .inputs
            .iter()
            .filter(|(_, provenance)| !provenance.complete())
            .map(|(name, _)| name.as_str())
            .collect();
        unknown.sort_unstable();
        unknown
    }

    /// Refuse reuse and coverage while any input is explicitly unknown.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] naming the unknown inputs.
    pub(crate) fn verify_complete(&self) -> Result<(), ContractError> {
        let unknown = self.unknown_inputs();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(ContractError::identity(
                "input_closure",
                format!("incomplete_inputs:{}", unknown.join(",")),
            ))
        }
    }
}

/// Builder for [`TaskInputClosure`] over one task group.
pub(crate) struct ClosureBuilder {
    /// Task ID under closure.
    task_id: String,
    /// Inputs collected so far.
    inputs: BTreeMap<String, Provenance>,
}

impl ClosureBuilder {
    /// Start a closure for `task_id`.
    pub(crate) fn new(task_id: &str) -> Self {
        Self {
            task_id: task_id.to_owned(),
            inputs: BTreeMap::new(),
        }
    }

    /// Record one semantic input with its provenance.
    pub(crate) fn input(mut self, name: &str, provenance: Provenance) -> Self {
        self.inputs.insert(name.to_owned(), provenance);
        self
    }

    /// Record a value-bound input (features, target, profile, argv).
    pub(crate) fn value(self, name: &str, value: &str) -> Self {
        self.input(
            name,
            Provenance::Known {
                digest: digest_b3(value.as_bytes()),
            },
        )
    }

    /// Finish the closure.
    pub(crate) fn build(self) -> TaskInputClosure {
        TaskInputClosure {
            task_id: self.task_id,
            inputs: self.inputs,
        }
    }
}

/// Resolve one task group's closure against the checkout at `root`.
///
/// Lockfile, Nextest config, manifests, Cargo config, and declared
/// inputs resolve to content digests when present and to proven
/// absence when the probe finds nothing; unreadable files stay
/// explicitly unknown. Source-tree content is guarded externally by
/// the changed-set guard enforced at the coverage site.
pub(crate) fn resolve_closure_at_root(
    root: &Path,
    group: &TaskGroup,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
) -> TaskInputClosure {
    let manifest = super::manifest_for_key(&group.manifest_key);
    let mut closure = ClosureBuilder::new(&group.task_id)
        .input(
            "source_tree",
            Provenance::GuardedExternally {
                guard: "changed_set_guard".to_owned(),
            },
        )
        .input("manifest", probe_file(root, &manifest))
        .input("lockfile", probe_lockfile(root, &manifest))
        .input(
            "nextest_config",
            probe_nextest_config(root, profile_nextest_config),
        )
        .input("cargo_config", probe_cargo_config(root, &manifest))
        .input(
            "local_deps",
            Provenance::Known {
                digest: graph_digest.to_owned(),
            },
        )
        .input(
            "toolchain",
            Provenance::Known {
                digest: toolchain_id.to_owned(),
            },
        )
        .input(
            "platform",
            Provenance::Known {
                digest: platform_id.to_owned(),
            },
        )
        .value("features", &group.features.join(","))
        .value("target", &group.target)
        .value("profile", &group.configuration)
        .value("driver", &group.compile_driver)
        .value("runner", &group.test_runner)
        .value("kind", group.kind.as_str());
    for (index, extra) in group.declared_inputs.iter().enumerate() {
        closure = closure.input(
            &format!("declared_extra:{index}:{extra}"),
            probe_declared(root, extra),
        );
    }
    closure = closure.input("vcs", vcs_provenance(group));
    closure.build()
}

/// Provenance of one file: content digest, proven absence, or unknown.
fn probe_file(root: &Path, path: &str) -> Provenance {
    let Ok(normalized) = normalize_checkout_path(path) else {
        return Provenance::Unknown {
            reason: format!("bad_path:{path}"),
        };
    };
    match std::fs::read(root.join(&normalized)) {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Provenance::AbsentProven {
            evidence: format!("not_found:{normalized}"),
        },
        Err(err) => Provenance::Unknown {
            reason: format!("unreadable:{normalized}:{err}"),
        },
    }
}

/// Lockfile provenance, walking up from the manifest like Cargo does.
fn probe_lockfile(root: &Path, manifest: &str) -> Provenance {
    let mut dir = manifest
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned());
    let mut probed = Vec::new();
    loop {
        let candidate = if dir.is_empty() {
            "Cargo.lock".to_owned()
        } else {
            format!("{dir}/Cargo.lock")
        };
        probed.push(candidate.clone());
        match probe_file(root, &candidate) {
            known @ Provenance::Known { .. } => return known,
            Provenance::Unknown { reason } => return Provenance::Unknown { reason },
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
        let Some((parent, _)) = dir.rsplit_once('/') else {
            if dir.is_empty() {
                break;
            }
            dir.clear();
            continue;
        };
        dir = parent.to_owned();
    }
    Provenance::AbsentProven {
        evidence: format!("not_found:{}", probed.join(",")),
    }
}

/// Nextest-config provenance: profile path plus the conventional path.
fn probe_nextest_config(root: &Path, profile_config: Option<&str>) -> Provenance {
    const CONVENTIONAL: &str = ".config/nextest.toml";
    if let Some(configured) = profile_config {
        match probe_file(root, configured) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
        if configured == CONVENTIONAL {
            return Provenance::AbsentProven {
                evidence: format!("not_found:{CONVENTIONAL}"),
            };
        }
    }
    match probe_file(root, CONVENTIONAL) {
        known @ Provenance::Known { .. } => known,
        unknown @ Provenance::Unknown { .. } => unknown,
        Provenance::AbsentProven { evidence } => Provenance::AbsentProven {
            evidence: format!("profile:{profile_config:?}:{evidence}"),
        },
        guarded @ Provenance::GuardedExternally { .. } => guarded,
    }
}

/// Cargo-config provenance: manifest dir plus the repository root.
fn probe_cargo_config(root: &Path, manifest: &str) -> Provenance {
    let dir = manifest
        .rsplit_once('/')
        .map_or("", |(dir, _)| if dir.is_empty() { "" } else { dir });
    let mut candidates = Vec::new();
    if !dir.is_empty() {
        candidates.push(format!("{dir}/.cargo/config.toml"));
    }
    candidates.push(".cargo/config.toml".to_owned());
    for candidate in &candidates {
        match probe_file(root, candidate) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
    }
    Provenance::AbsentProven {
        evidence: format!("not_found:{}", candidates.join(",")),
    }
}

/// Declared-extra provenance: content digest or explicitly unknown.
///
/// A declared input that cannot be read is unknown, never absent: the
/// task claims to consume it, so absence is a broken declaration.
fn probe_declared(root: &Path, path: &str) -> Provenance {
    match probe_file(root, path) {
        Provenance::AbsentProven { evidence } => Provenance::Unknown {
            reason: format!("declared_but_missing:{evidence}"),
        },
        other => other,
    }
}

/// VCS provenance: tasks with undeclared build-script reads may observe
/// Git state (unknown); offline Cargo tasks never do (proven by kind).
fn vcs_provenance(group: &TaskGroup) -> Provenance {
    if group.undeclared_reads {
        Provenance::Unknown {
            reason: "build_script_may_observe_vcs".to_owned(),
        }
    } else {
        Provenance::AbsentProven {
            evidence: "offline_cargo_no_vcs_reads".to_owned(),
        }
    }
}
