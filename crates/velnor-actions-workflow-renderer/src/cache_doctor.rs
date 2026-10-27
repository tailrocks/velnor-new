// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, SOURCES_KEY_PLAN_EXPR,
    SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR, SOURCES_RESTORE_KEYS_STEP_EXPR,
    SOURCES_SAVE_CONDITION, TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_SAVE_CONDITION,
    is_plan_cache_output,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR, MBX_BUNDLE_SAVE_CONDITION,
    is_mbx_bundle_key_needs_expr, is_mbx_bundle_restore_keys_needs_expr,
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

pub(crate) const MBX_OBJECTS_MODE_DOCTOR_EXEMPTION: &str = "jdx/mr-boxington-action@";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Runtime(CacheFamily),
    Tools,
}

struct CacheUse<'a> {
    family: Family,
    save: bool,
    path: Option<&'a str>,
}

pub(crate) fn check(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut owners: BTreeMap<String, Family> = BTreeMap::new();
    for (id, job) in jobs {
        for step in &job.steps {
            check_tools_identity(id, step)?;
            stack::check_family_keys_env(id, job, step).map_err(|problem| doctor(id, &problem))?;
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

type CacheInputs<'a> = (&'a str, Option<&'a str>, bool);

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
    if key == MBX_BUNDLE_KEY_EXPR || is_mbx_bundle_key_needs_expr(key) {
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
    CacheFamily::for_key(key)
        .filter(|family| !STACK_FAMILIES.contains(family))
        .map(Family::Runtime)
}

fn is_tofu_plan_key(key: &str) -> bool {
    key.strip_prefix("${{needs.plan.outputs.")
        .and_then(|rest| rest.strip_suffix("}}"))
        .is_some_and(|name| name.starts_with("tofu_providers_key_") && is_plan_cache_output(name))
}

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

const BANNED_KEY_PARTS: [&str; 5] = [
    "github.job",
    "github.workflow",
    "github.run_",
    "ImageVersion",
    "hashFiles(",
];

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
            // The stale `velnor/mbx-bundle` path is rejected; the state transport
            // payload (or the state root) is the only accepted bundle path.
            (path == COMPLETE_STATE_TRANSPORT_EXPR || path == COMPLETE_STATE_ROOT_EXPR)
                && mbx_bundle_restore_keys_ok(step, cache.save)
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

fn mbx_bundle_restore_keys_ok(step: &Step, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    if save {
        !with.contains_key("restore-keys")
    } else {
        with.get("restore-keys")
            .map(String::as_str)
            .is_some_and(|keys| {
                keys == MBX_BUNDLE_RESTORE_KEYS_EXPR || is_mbx_bundle_restore_keys_needs_expr(keys)
            })
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
