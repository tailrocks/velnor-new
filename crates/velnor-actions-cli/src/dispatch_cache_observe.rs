//! Private operation that admits one cache restore action against the backend.

use std::env;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    BoundQualificationCacheKeys, MAX_CACHE_KEY_BYTES, QualificationCacheBackendObservation,
    QualificationCacheBindingOutputs, QualificationCacheRestoreExpectation,
    bind_qualification_cache_runtime, observe_qualification_cache_backend,
    validate_qualification_cache_backend_observation,
};

use super::cache_bind;

/// Private operation tag for one observed cache restore.
pub(crate) const OP: &str = "observe-qualification-cache-backend-v1";

const WORKSPACE_ENV: &str = "GITHUB_WORKSPACE";
const ACTION_HIT_ENV: &str = "VELNOR_QUALIFICATION_CACHE_HIT";
const ACTION_MATCHED_KEY_ENV: &str = "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY";
const CENSUS_PHASE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_CENSUS_PHASE";
const MAX_ACTION_KEY_BYTES: usize = MAX_CACHE_KEY_BYTES;
const MAX_WORKSPACE_PATH_BYTES: usize = 4_096;

/// Bind the current layer, observe the actual restore, and write only admitted
/// backend evidence.
pub(crate) fn run() -> ExitCode {
    let inputs = match cache_bind::read_inputs(|name| env::var(name)) {
        Ok(inputs) => inputs,
        Err(problem) => return fail(&problem),
    };
    let request = inputs.request();
    let phase = match env::var(CENSUS_PHASE_ENV).as_deref() {
        Ok("restore") => CensusPhase::Restore,
        Ok("save_before") => CensusPhase::SaveBefore,
        Ok("save_after") => CensusPhase::SaveAfter,
        _ => return fail("qualification_cache_census_phase_invalid"),
    };
    let binding = match bind_qualification_cache_runtime(&request) {
        Ok(binding) => binding,
        Err(error) => return fail(&error.to_string()),
    };
    match observe_and_emit(&request, &binding, phase) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => fail(&problem),
    }
}

fn observe_and_emit(
    request: &velnor_actions_orchestrator::QualificationCacheBindingRequest<'_>,
    binding: &QualificationCacheBindingOutputs,
    phase: CensusPhase,
) -> Result<(), String> {
    let keys = match bound_keys(binding) {
        Ok(keys) => keys,
        Err(problem) => return Err(problem.to_owned()),
    };
    let action = match phase {
        CensusPhase::Restore => {
            match read_action_outputs(|name| env::var(name), binding.restore_key.is_some()) {
                Ok(action) => action,
                Err(problem) => return Err(problem.to_owned()),
            }
        }
        CensusPhase::SaveBefore | CensusPhase::SaveAfter => ActionOutputs {
            hit: false,
            matched_key: None,
        },
    };
    let observed_key = match phase {
        CensusPhase::Restore => binding.restore_key.as_deref(),
        CensusPhase::SaveBefore | CensusPhase::SaveAfter => binding.save_key.as_deref(),
    };
    let observed = match observed_key {
        Some(key) => {
            let workspace = match workspace_path(env::var(WORKSPACE_ENV)) {
                Ok(path) => path,
                Err(problem) => return Err(problem.to_owned()),
            };
            match observe_qualification_cache_backend(
                &workspace,
                request.repository,
                key,
                request.git_ref,
            ) {
                Ok(observed) => observed,
                Err(error) => return Err(error.to_string()),
            }
        }
        None => QualificationCacheBackendObservation::NotQueried,
    };
    match phase {
        CensusPhase::Restore => {
            if let Err(error) = validate_qualification_cache_backend_observation(
                &keys,
                action.hit,
                action.matched_key.as_deref(),
                &observed,
            ) {
                return Err(error.to_string());
            }
        }
        CensusPhase::SaveBefore | CensusPhase::SaveAfter => {
            if binding.save_key.is_none()
                && observed != QualificationCacheBackendObservation::NotQueried
            {
                return Err("qualification_cache_save_census_unexpected".to_owned());
            }
        }
    }
    let Some(output_path) =
        env::var_os(cache_bind::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return Err("missing_github_output".to_owned());
    };
    let body = match output_body(binding, phase, &action, &observed) {
        Ok(body) => body,
        Err(problem) => return Err(problem.to_owned()),
    };
    match cache_bind::append_owned_output(&output_path, body.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("append_github_output:{error}")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CensusPhase {
    Restore,
    SaveBefore,
    SaveAfter,
}

impl CensusPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Restore => "restore",
            Self::SaveBefore => "save_before",
            Self::SaveAfter => "save_after",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActionOutputs {
    hit: bool,
    matched_key: Option<String>,
}

fn read_action_outputs<E>(
    mut get: E,
    restore_requested: bool,
) -> Result<ActionOutputs, &'static str>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    let hit_text = get(ACTION_HIT_ENV).ok().filter(|value| !value.is_empty());
    let hit = match hit_text.as_deref() {
        Some("true") => true,
        Some("false") => false,
        None if !restore_requested => false,
        _ => return Err("qualification_cache_action_hit_invalid"),
    };
    let matched_key = get(ACTION_MATCHED_KEY_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    if matched_key.as_ref().is_some_and(|value| {
        value.len() > MAX_ACTION_KEY_BYTES || !value.bytes().all(|byte| byte.is_ascii_graphic())
    }) {
        return Err("qualification_cache_action_key_invalid");
    }
    Ok(ActionOutputs { hit, matched_key })
}

fn bound_keys(
    outputs: &QualificationCacheBindingOutputs,
) -> Result<BoundQualificationCacheKeys, &'static str> {
    if outputs.expected_cache.is_some() && outputs.restore_key.is_none() {
        return Err("qualification_cache_binding_restore_inconsistent");
    }
    Ok(BoundQualificationCacheKeys {
        restore: outputs.restore_key.as_ref().map(|requested_key| {
            QualificationCacheRestoreExpectation {
                requested_key: requested_key.clone(),
                expected_cache: outputs.expected_cache.clone(),
            }
        }),
        save_key: outputs.save_key.clone(),
        save_if_state_changes: outputs.save_if_state_changes,
        expected_prior_state_digest: outputs.expected_prior_state_digest.clone(),
        runtime_identity_digest: outputs.runtime_identity_digest.clone(),
    })
}

fn workspace_path(value: Result<String, env::VarError>) -> Result<PathBuf, &'static str> {
    let text = value.map_err(|_| "missing_github_workspace")?;
    if text.len() > MAX_WORKSPACE_PATH_BYTES
        || !text.starts_with('/')
        || text.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("github_workspace_path_invalid");
    }
    let path = Path::new(&text);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::CurDir | Component::Prefix(_)
        )
    }) {
        return Err("github_workspace_path_invalid");
    }
    Ok(path.to_path_buf())
}

fn output_body(
    binding: &QualificationCacheBindingOutputs,
    phase: CensusPhase,
    action: &ActionOutputs,
    observed: &QualificationCacheBackendObservation,
) -> Result<String, &'static str> {
    let (status, cache) = match observed {
        QualificationCacheBackendObservation::Found(entry) => ("found", Some(entry)),
        QualificationCacheBackendObservation::Absent => ("absent", None),
        QualificationCacheBackendObservation::NotQueried => ("not_queried", None),
        QualificationCacheBackendObservation::Unavailable => {
            return Err("qualification_cache_backend_unavailable");
        }
    };
    let fields = [
        ("campaign", binding.campaign.clone()),
        ("phase", binding.phase.clone()),
        ("plan_id", binding.plan_id.clone()),
        ("run_key", binding.run_key.clone()),
        ("owner_id", binding.owner_id.clone()),
        ("matrix_key", binding.matrix_key.clone()),
        ("layer", binding.layer.clone()),
        ("census_phase", phase.as_str().to_owned()),
        (
            "census_key",
            match phase {
                CensusPhase::Restore => binding.restore_key.clone().unwrap_or_default(),
                CensusPhase::SaveBefore | CensusPhase::SaveAfter => {
                    binding.save_key.clone().unwrap_or_default()
                }
            },
        ),
        (
            "restore_key",
            binding.restore_key.clone().unwrap_or_default(),
        ),
        (
            "runtime_identity_digest",
            binding.runtime_identity_digest.clone().unwrap_or_default(),
        ),
        ("restore_action_hit", bool_text(action.hit).to_owned()),
        (
            "restore_action_matched_key",
            action.matched_key.clone().unwrap_or_default(),
        ),
        ("backend_status", status.to_owned()),
        (
            "backend_cache_id",
            cache.map_or_else(String::new, |entry| entry.id.to_string()),
        ),
        (
            "backend_cache_key",
            cache.map_or_else(String::new, |entry| entry.key.clone()),
        ),
        (
            "backend_cache_ref",
            cache.map_or_else(String::new, |entry| entry.git_ref.clone()),
        ),
        (
            "backend_cache_version",
            cache.map_or_else(String::new, |entry| entry.version.clone()),
        ),
        (
            "backend_cache_size",
            cache.map_or_else(String::new, |entry| entry.size_bytes.to_string()),
        ),
    ];
    let mut body = String::new();
    for (name, value) in fields {
        if value.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("qualification_cache_observation_output_invalid");
        }
        body.push_str(name);
        body.push('=');
        body.push_str(&value);
        body.push('\n');
    }
    // Keep approval last. A failed partial append cannot expose an admission.
    body.push_str("backend_admitted=true\n");
    Ok(body)
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: cache backend observation failed: {problem}");
    ExitCode::from(1)
}
