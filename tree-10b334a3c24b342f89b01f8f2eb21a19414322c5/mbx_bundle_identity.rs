//! MBX cache identity and one-writer policy for each mutable job closure.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, Step, StepKind};

use super::{
    HOSTED_SUFFIX, MBX_QUALIFICATION_SCOPE_PREFIX, MBX_SCOPE_INPUT, MBX_WRITER_INPUT, SCALE_SUFFIX,
    is_mbx_action,
};
use crate::RenderError;

pub(super) const QUALIFICATION_WRITER_IMPORT_GUARD: &str = r#"set -eu; if [ "$CACHE_HIT" = "true" ] || [ -n "$MATCHED" ]; then echo "qualification writer restore was not cold" >&2; exit 1; fi;"#;
pub(super) const QUALIFICATION_READER_IMPORT_GUARD: &str = r#"set -eu; if [ "$CACHE_HIT" != "true" ]; then echo "qualification reader restore was not an exact cache hit" >&2; exit 1; fi; if [ -z "$MATCHED" ] || [ "$MATCHED" != "$EXPECTED_KEY" ]; then echo "qualification reader key did not match the writer primary" >&2; exit 1; fi;"#;

pub(super) fn import_guard(role: Option<bool>, env: &mut BTreeMap<String, String>) -> &'static str {
    match role {
        Some(writer) => {
            env.insert(
                "CACHE_HIT".to_owned(),
                "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
            );
            if writer {
                QUALIFICATION_WRITER_IMPORT_GUARD
            } else {
                env.insert(
                    "EXPECTED_KEY".to_owned(),
                    "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
                );
                QUALIFICATION_READER_IMPORT_GUARD
            }
        }
        None => "",
    }
}

/// Internal identity used by the explicit restore/import/export lifecycle.
#[derive(Clone)]
pub(super) struct CacheIdentity {
    /// Stable task scope, or an explicit shared qualification scope.
    pub scope: String,
    /// Exact stock MBX version.
    pub version: String,
    /// Versioned object schema namespace.
    pub generation: String,
    /// Exact Mise-selected Rust toolchain name.
    pub rustup_toolchain: String,
    /// Isolated Mise env needed to resolve that toolchain.
    pub rust_env: BTreeMap<String, String>,
    /// Canonical id shared by hosted/scale-set copies.
    logical_job_id: String,
    /// Explicit role only for intentional cross-job shared scopes.
    writer: Option<bool>,
}

impl CacheIdentity {
    /// Qualification roles reserve run-scoped keys for the hosted round trip.
    pub(super) fn qualification_run_scoped(&self) -> bool {
        self.writer.is_some() && self.scope.starts_with(MBX_QUALIFICATION_SCOPE_PREFIX)
    }

    /// Return the explicit qualification writer/reader role, if reserved.
    pub(super) fn qualification_role(&self) -> Option<bool> {
        if self.qualification_run_scoped() {
            self.writer
        } else {
            None
        }
    }
}

/// Build each job identity and elect one writer for every shared identity.
pub(super) fn plan_writers(
    jobs: &BTreeMap<String, Job>,
) -> Result<(BTreeMap<String, CacheIdentity>, BTreeSet<String>), RenderError> {
    let mut identities = BTreeMap::new();
    let mut grouped: BTreeMap<String, Vec<(String, CacheIdentity)>> = BTreeMap::new();
    for (id, job) in jobs {
        let Some(identity) = cache_identity(id, job)? else {
            continue;
        };
        grouped
            .entry(identity.scope.clone())
            .or_default()
            .push((id.clone(), identity.clone()));
        identities.insert(id.clone(), identity);
    }
    let mut writers = BTreeSet::new();
    for owners in grouped.values() {
        writers.extend(elect_owners(owners)?);
    }
    Ok((identities, writers))
}

fn cache_identity(id: &str, job: &Job) -> Result<Option<CacheIdentity>, RenderError> {
    let mut actions = job.steps.iter().filter(|step| is_mbx_action(step));
    let Some(step) = actions.next() else {
        return Ok(None);
    };
    if actions.next().is_some() {
        return Err(identity_error(id, "multiple_mbx_actions"));
    }
    identity_for_action(id, job, step).map(Some)
}

fn identity_for_action(id: &str, job: &Job, step: &Step) -> Result<CacheIdentity, RenderError> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(identity_error(id, "bad_mbx_step"));
    };
    let Some(version) = with.get("version").cloned() else {
        return Err(identity_error(id, "missing_mbx_version"));
    };
    let logical_job_id = canonical_job_id(id).to_owned();
    let scope = with
        .get(MBX_SCOPE_INPUT)
        .cloned()
        .unwrap_or_else(|| logical_job_id.clone());
    if !valid_scope(&scope) {
        return Err(identity_error(id, "bad_cache_scope"));
    }
    let writer = parse_writer_role(id, with.get(MBX_WRITER_INPUT))?;
    if writer.is_some()
        && (!scope.starts_with(MBX_QUALIFICATION_SCOPE_PREFIX)
            || scope.len() == MBX_QUALIFICATION_SCOPE_PREFIX.len())
    {
        return Err(identity_error(
            id,
            "shared_scope_outside_qualification_namespace",
        ));
    }
    let (rustup_toolchain, rust_env) = pinned_rust_env(id, job)?;
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(&version);
    Ok(CacheIdentity {
        scope,
        version,
        generation,
        rustup_toolchain,
        rust_env,
        logical_job_id,
        writer,
    })
}

fn parse_writer_role(id: &str, role: Option<&String>) -> Result<Option<bool>, RenderError> {
    match role.map(String::as_str) {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(identity_error(id, "bad_writer_role")),
    }
}

fn pinned_rust_env(id: &str, job: &Job) -> Result<(String, BTreeMap<String, String>), RenderError> {
    let mut selected: Option<(String, String, String)> = None;
    for step in &job.steps {
        let StepKind::Shell { env, .. } = &step.kind else {
            continue;
        };
        let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN") else {
            continue;
        };
        let Some(rustup_home) = env.get("MISE_RUSTUP_HOME") else {
            return Err(identity_error(id, "missing_mise_rustup_home"));
        };
        let Some(cargo_home) = env.get("MISE_CARGO_HOME") else {
            return Err(identity_error(id, "missing_mise_cargo_home"));
        };
        let candidate = (toolchain.clone(), rustup_home.clone(), cargo_home.clone());
        if selected
            .as_ref()
            .is_some_and(|current| current != &candidate)
        {
            return Err(identity_error(id, "inconsistent_pinned_rust_env"));
        }
        selected = Some(candidate);
    }
    let Some((toolchain, rustup_home, cargo_home)) = selected else {
        return Err(identity_error(id, "missing_pinned_rust_env"));
    };
    if [
        toolchain.as_str(),
        rustup_home.as_str(),
        cargo_home.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(identity_error(id, "empty_pinned_rust_env"));
    }
    let env = BTreeMap::from([
        ("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone()),
        ("MISE_RUSTUP_HOME".to_owned(), rustup_home),
        ("MISE_CARGO_HOME".to_owned(), cargo_home),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
    ]);
    Ok((toolchain, env))
}

fn elect_owners(owners: &[(String, CacheIdentity)]) -> Result<Vec<String>, RenderError> {
    let Some((_, first)) = owners.first() else {
        return Ok(Vec::new());
    };
    if owners.iter().any(|(_, identity)| {
        identity.version != first.version
            || identity.generation != first.generation
            || identity.rustup_toolchain != first.rustup_toolchain
            || identity.rust_env != first.rust_env
    }) {
        return Err(identity_error(
            &first.logical_job_id,
            "incompatible_shared_identity",
        ));
    }
    let one_logical_job = owners
        .iter()
        .all(|(_, identity)| identity.logical_job_id == first.logical_job_id);
    let explicit = owners.iter().any(|(_, identity)| identity.writer.is_some());
    if owners.len() == 1 {
        if explicit && first.writer != Some(true) {
            return Err(identity_error(&first.logical_job_id, "scope_writer_count"));
        }
        return Ok(if writer_enabled(first.writer) {
            owners
                .first()
                .map(|(id, _)| vec![id.clone()])
                .unwrap_or_default()
        } else {
            Vec::new()
        });
    }
    if !one_logical_job && !explicit {
        return Err(identity_error(
            &first.logical_job_id,
            "shared_scope_needs_writer_roles",
        ));
    }
    if explicit {
        return elect_explicit_roles(owners, &first.logical_job_id);
    }
    Ok(owners
        .first()
        .map(|(id, _)| vec![id.clone()])
        .unwrap_or_default())
}

fn elect_explicit_roles(
    owners: &[(String, CacheIdentity)],
    scope: &str,
) -> Result<Vec<String>, RenderError> {
    if owners.iter().any(|(_, identity)| identity.writer.is_none()) {
        return Err(identity_error(scope, "partial_writer_roles"));
    }
    let writers: Vec<String> = owners
        .iter()
        .filter(|(_, identity)| identity.writer == Some(true))
        .map(|(id, _)| id.clone())
        .collect();
    if writers.len() != 1 {
        return Err(identity_error(scope, "scope_writer_count"));
    }
    Ok(writers)
}

fn writer_enabled(policy: Option<bool>) -> bool {
    policy != Some(false)
}

fn canonical_job_id(id: &str) -> &str {
    id.strip_suffix(HOSTED_SUFFIX)
        .or_else(|| id.strip_suffix(SCALE_SUFFIX))
        .unwrap_or(id)
}

fn valid_scope(scope: &str) -> bool {
    !scope.is_empty()
        && scope.len() <= 128
        && scope.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
        })
}

fn identity_error(id: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("mbx_cache_identity:{id}:{problem}"))
}
