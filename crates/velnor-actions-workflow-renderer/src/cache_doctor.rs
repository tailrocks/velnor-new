//! Generation-time cache doctor over every finalized cache step.
//!
//! Runs after writer election and every closure, over each
//! `actions/cache/*` step, the V2 tools composite, the provider-cache
//! composite, and any other action carrying a `cache-key` input:
//!
//! - I1: keys never carry `github.job`, `github.workflow`, `github.run_*`,
//!   a 40-hex action SHA, a literal semver (tool or Velnor version),
//!   `ImageVersion`, or `hashFiles(`.
//! - I2: every restore and save of one family uses the family's one path list.
//! - I3: no path is owned by two families; sources paths stay in sources.
//! - I7: every save is trust-gated; no save can run on forks or merge groups.
//! - The V2 tools identity's `d` input, which the tools key binds, is the
//!   exact static-digest-plus-resolved-versions shape and never hashes a
//!   repository file.
//! - Keys fit GitHub's 512-byte cap.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR,
    SOURCES_RESTORE_KEYS_STEP_EXPR, SOURCES_SAVE_CONDITION, TOFU_PROVIDER_CACHE_BASE_EXPR,
    TOFU_PROVIDERS_SAVE_CONDITION, is_plan_cache_output,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_PATH_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR,
    MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::workflow::cache_family_steps::{
    STACK_FAMILIES, family_key_expr, family_save_condition,
};
use velnor_actions_contract::workflow::step_identity::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDERS_KEY_OUTPUT_EXPR, TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
};
use velnor_actions_contract::{Job, PullRequestCachePolicy, Step, StepKind};

use crate::RenderError;

#[path = "cache_doctor_stack.rs"]
mod stack;

/// The install-only MBX action carries no cache input, so the doctor sees
/// only the objects-mode action (qualification), whose run-bound generation
/// keys are checked by the qualification contract instead.
pub(crate) const MBX_OBJECTS_MODE_DOCTOR_EXEMPTION: &str = "jdx/mr-boxington-action@";

/// Families the doctor knows: the runtime-keyed families plus V2 tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Runtime(CacheFamily),
    Tools,
}

/// One cache-bearing step, classified.
struct CacheUse<'a> {
    family: Family,
    save: bool,
    path: Option<&'a str>,
}

/// Check every cache step in `jobs`.
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] naming the job and broken rule.
pub(crate) fn check(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut owners: BTreeMap<String, Family> = BTreeMap::new();
    for (id, job) in jobs {
        for step in &job.steps {
            check_tools_identity(id, step)?;
            let Some((key, path, save)) = classify(id, step)? else {
                continue;
            };
            check_key(id, key)?;
            let family =
                family_for_key(key).ok_or_else(|| doctor(id, &format!("unknown_family:{key}")))?;
            let cache = CacheUse { family, save, path };
            check_paths(id, step, &cache)?;
            if cache.save {
                check_save(id, step, cache.family)?;
            }
            if let Some(path) = cache.path {
                for line in path.lines() {
                    if let Some(previous) = owners.insert(line.to_owned(), cache.family)
                        && previous != cache.family
                    {
                        return Err(doctor(id, &format!("i3_path_two_families:{line}")));
                    }
                }
            }
        }
    }
    check_sources_isolation(&owners)
}

fn doctor(job: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("cache_doctor:{job}:{problem}"))
}

/// One cache-bearing step's `(key, path, save)`, or `None` for other steps.
type CacheInputs<'a> = (&'a str, Option<&'a str>, bool);

/// Classify one step, or `None` for steps that touch no cache.
fn classify<'a>(job: &str, step: &'a Step) -> Result<Option<CacheInputs<'a>>, RenderError> {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return Ok(None);
    };
    if uses.starts_with(MBX_OBJECTS_MODE_DOCTOR_EXEMPTION) {
        return Ok(None);
    }
    let get = |name: &str| with.get(name).map(String::as_str);
    let (key, path, save) = if uses.starts_with("actions/cache/restore@") {
        (get("key"), get("path"), false)
    } else if uses.starts_with("actions/cache/save@") {
        (get("key"), get("path"), true)
    } else if uses.starts_with("actions/cache@") {
        return Err(doctor(job, "i7_combined_cache_action_saves_ungated"));
    } else if uses == crate::cache_steps::TOOLS_RESTORE_USES {
        (get("key"), None, false)
    } else if uses == TOFU_PROVIDER_ADMISSION_USES || with.contains_key("cache-key") {
        (get("cache-key"), get("cache-path"), false)
    } else {
        return Ok(None);
    };
    let key = key.ok_or_else(|| doctor(job, "cache_step_without_key"))?;
    Ok(Some((key, path, save)))
}

fn family_for_key(key: &str) -> Option<Family> {
    if key == SOURCES_KEY_STEP_EXPR || key == SOURCES_KEY_PLAN_EXPR {
        return Some(Family::Runtime(CacheFamily::Sources));
    }
    if key == MBX_BUNDLE_KEY_EXPR {
        return Some(Family::Runtime(CacheFamily::MbxBundle));
    }
    if let Some(family) = STACK_FAMILIES
        .into_iter()
        .find(|family| key == family_key_expr(*family))
    {
        return Some(Family::Runtime(family));
    }
    if key == TOFU_PROVIDERS_KEY_OUTPUT_EXPR || is_tofu_plan_key(key) {
        return Some(Family::Runtime(CacheFamily::TofuProviders));
    }
    if crate::cache_p08::is_v2_cache_key_expression(key) {
        return Some(Family::Tools);
    }
    // A stack family key is only the generated expression; a literal key that
    // merely shares its prefix would bypass the per-job key step.
    CacheFamily::for_key(key)
        .filter(|family| !STACK_FAMILIES.contains(family))
        .map(Family::Runtime)
}

fn is_tofu_plan_key(key: &str) -> bool {
    key.strip_prefix("${{needs.plan.outputs.")
        .and_then(|rest| rest.strip_suffix("}}"))
        .is_some_and(|name| name.starts_with("tofu_providers_key_") && is_plan_cache_output(name))
}

/// The tools key path: the identity `d` input binds only the static digest
/// and the resolver's `env` exports, never `hashFiles` or run metadata.
fn check_tools_identity(job: &str, step: &Step) -> Result<(), RenderError> {
    use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
    if step.role != Some(velnor_actions_contract::StepRole::ToolsCacheIdentity) {
        return Ok(());
    }
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(doctor(job, "tools_identity_not_action"));
    };
    let input = with
        .get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .map_or("", String::as_str);
    for banned in BANNED_KEY_PARTS {
        if input.contains(banned) {
            return Err(doctor(job, &format!("i1_tools_identity_contains:{banned}")));
        }
    }
    if !velnor_actions_contract::config::project_tool_refs::is_tools_identity_input(input) {
        return Err(doctor(job, "i1_tools_identity_shape"));
    }
    Ok(())
}

/// Key fragments that make a cache identity unstable or file-bound.
const BANNED_KEY_PARTS: [&str; 5] = [
    "github.job",
    "github.workflow",
    "github.run_",
    "ImageVersion",
    "hashFiles(",
];

/// I1 plus the key length bound.
fn check_key(job: &str, key: &str) -> Result<(), RenderError> {
    for banned in BANNED_KEY_PARTS {
        if key.contains(banned) {
            return Err(doctor(job, &format!("i1_key_contains:{banned}")));
        }
    }
    if has_hex_run(key, 40) {
        return Err(doctor(job, "i1_key_contains_action_sha"));
    }
    if has_semver(key) {
        return Err(doctor(job, "i1_key_contains_semver"));
    }
    if !key.contains("${{") && key.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES {
        return Err(doctor(job, "key_too_long"));
    }
    if let Some(Family::Runtime(family)) = family_for_key(key) {
        family
            .validate_key_bound()
            .map_err(|err| doctor(job, &err.to_string()))?;
    }
    Ok(())
}

/// True when `text` has a run of exactly `len` or more lowercase hex chars.
fn has_hex_run(text: &str, len: usize) -> bool {
    let mut run = 0;
    for byte in text.bytes() {
        run = if byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) {
            run + 1
        } else {
            0
        };
        if run >= len {
            return true;
        }
    }
    false
}

/// True for a `\d+\.\d+\.\d+` run anywhere in `text`.
fn has_semver(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|start| {
        let mut at = start;
        for part in 0..3 {
            let digits = bytes[at..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits == 0 {
                return false;
            }
            at += digits;
            if part < 2 {
                if bytes.get(at) != Some(&b'.') {
                    return false;
                }
                at += 1;
            }
        }
        true
    })
}

/// I2: the family's one path list (and restore keys for sources).
fn check_paths(job: &str, step: &Step, cache: &CacheUse<'_>) -> Result<(), RenderError> {
    let ok = match (cache.family, cache.path) {
        (Family::Tools, None) => !cache.save,
        (Family::Tools, Some(path)) => path == crate::cache_steps::TOOLS_CACHE_PATHS.join("\n"),
        (Family::Runtime(CacheFamily::Sources), Some(path)) => {
            path == CacheFamily::Sources.spec().path_input(None)
                && sources_restore_keys_ok(step, cache.save)
        }
        (Family::Runtime(CacheFamily::TofuProviders), Some(path)) => {
            path == TOFU_PROVIDERS_PATH_OUTPUT_EXPR && cache.save
                || tofu_slug(path).is_some_and(|slug| {
                    path == CacheFamily::TofuProviders.spec().path_input(Some(slug))
                })
        }
        (Family::Runtime(CacheFamily::MbxBundle), Some(path)) => {
            path == MBX_BUNDLE_PATH_EXPR && mbx_bundle_restore_keys_ok(step, cache.save)
        }
        (
            Family::Runtime(
                family @ (CacheFamily::BunInstall
                | CacheFamily::PlaywrightBrowsers
                | CacheFamily::DbtPackages
                | CacheFamily::PythonVenv
                | CacheFamily::AnsibleCollections),
            ),
            Some(path),
        ) => stack::path_ok(family, path) && stack::restore_keys_ok(step, family, cache.save),
        (Family::Runtime(_), None) => false,
    };
    if ok {
        Ok(())
    } else {
        Err(doctor(job, &format!("i2_path_not_from_spec:{}", step.name)))
    }
}

fn sources_restore_keys_ok(step: &Step, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    match (save, with.get("key").map(String::as_str)) {
        (true, _) => !with.contains_key("restore-keys"),
        (false, Some(SOURCES_KEY_STEP_EXPR)) => {
            with.get("restore-keys").map(String::as_str) == Some(SOURCES_RESTORE_KEYS_STEP_EXPR)
        }
        (false, _) => {
            with.get("restore-keys").map(String::as_str) == Some(SOURCES_RESTORE_KEYS_PLAN_EXPR)
        }
    }
}

/// A bundle restore reads the same-compatibility prefix; its save names no restore keys.
fn mbx_bundle_restore_keys_ok(step: &Step, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    if save {
        !with.contains_key("restore-keys")
    } else {
        with.get("restore-keys").map(String::as_str) == Some(MBX_BUNDLE_RESTORE_KEYS_EXPR)
    }
}

fn tofu_slug(path: &str) -> Option<&str> {
    path.strip_prefix(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/"))
        .filter(|slug| {
            !slug.is_empty()
                && slug
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

/// I7: every save carries its family's trust gate.
fn check_save(job: &str, step: &Step, family: Family) -> Result<(), RenderError> {
    let condition = step.condition.as_deref().unwrap_or_default();
    let ok = match family {
        Family::Runtime(CacheFamily::Sources) => {
            condition == SOURCES_SAVE_CONDITION && job == crate::render::PLAN_JOB_ID
        }
        Family::Runtime(CacheFamily::TofuProviders) => condition == TOFU_PROVIDERS_SAVE_CONDITION,
        Family::Runtime(CacheFamily::MbxBundle) => condition == MBX_BUNDLE_SAVE_CONDITION,
        Family::Runtime(
            family @ (CacheFamily::BunInstall
            | CacheFamily::PlaywrightBrowsers
            | CacheFamily::DbtPackages
            | CacheFamily::PythonVenv
            | CacheFamily::AnsibleCollections),
        ) => condition == family_save_condition(family),
        Family::Tools => [
            PullRequestCachePolicy::ReadOnly,
            PullRequestCachePolicy::SameRepositoryScoped,
        ]
        .into_iter()
        .any(|policy| condition == crate::cache_p08::tools_cache_save_condition(policy)),
    };
    if ok {
        Ok(())
    } else {
        Err(doctor(
            job,
            &format!("i7_save_not_trust_gated:{}", step.name),
        ))
    }
}

/// I3: no other family archives or nests a sources path.
fn check_sources_isolation(owners: &BTreeMap<String, Family>) -> Result<(), RenderError> {
    let sources = CacheFamily::Sources.spec().resolved_paths(None);
    for (path, family) in owners {
        if *family == Family::Runtime(CacheFamily::Sources) {
            continue;
        }
        let overlaps = sources.iter().any(|source| {
            path == source
                || path.starts_with(&format!("{source}/"))
                || source.starts_with(&format!("{path}/"))
        });
        if overlaps {
            return Err(doctor(
                "workflow",
                &format!("i3_sources_path_shared:{path}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cache_doctor_tests.rs"]
mod tests;
