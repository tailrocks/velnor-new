//! Exact native source transport and role bindings.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CacheSnapshotDomain, SourceBoundOperation, SourceProducer, SourceProducerRole, Step, StepKind,
    ToolCacheDomain,
};

pub(super) fn allowed_step(step: &Step, meta: &SourceProducer) -> bool {
    match &step.kind {
        StepKind::SourceBoundHelper { invocation, env } => {
            let operation = invocation.descriptor().operation();
            if !meta.role.permits(operation) {
                return false;
            }
            if operation == SourceBoundOperation::SourceProducerReport {
                return step.id.as_ref() == Some(&meta.report_step);
            }
            if is_verification(meta.role, operation) {
                return step.id.as_ref() == Some(&meta.verification_step);
            }
            if operation == SourceBoundOperation::CacheSnapshot {
                return snapshot_allowed(meta, invocation, env, step);
            }
            step.condition.is_none()
        }
        StepKind::Action { uses, with, env } => allowed_transport(uses, with, env, step, meta),
        _ => false,
    }
}

fn snapshot_allowed(
    meta: &SourceProducer,
    invocation: &velnor_actions_contract::HelperInvocation,
    env: &BTreeMap<String, String>,
    step: &Step,
) -> bool {
    let layer = match meta.role {
        SourceProducerRole::Cargo => "sources",
        SourceProducerRole::Npm => "npm_downloads",
        SourceProducerRole::Bun => "bun_downloads",
        SourceProducerRole::Gradle => "gradle_dependencies",
        SourceProducerRole::Tofu => return false,
    };
    let Some(phase) = invocation.args().get(1) else {
        return false;
    };
    if invocation.args().len() != 2
        || invocation
            .args()
            .first()
            .is_none_or(|argument| argument != layer)
        || env
            .get("VELNOR_SNAPSHOT_LAYER")
            .is_none_or(|value| value != layer)
        || CacheSnapshotDomain::ALL
            .into_iter()
            .find(|domain| domain.name() == layer)
            .is_none_or(|domain| *env != domain.environment(phase == "before"))
    {
        return false;
    }
    match phase.as_str() {
        "before" => step.condition.is_none(),
        "after" if meta.role == SourceProducerRole::Cargo => step.condition.is_none(),
        "after" => {
            let expected = format!(
                "steps.{}.outputs.verified == 'true'",
                meta.verification_step.as_str()
            );
            step.condition.as_deref() == Some(expected.as_str())
        }
        _ => false,
    }
}

pub(super) fn is_verification(role: SourceProducerRole, operation: SourceBoundOperation) -> bool {
    match role {
        SourceProducerRole::Cargo => operation == SourceBoundOperation::RustSourceProducer,
        SourceProducerRole::Npm => operation == SourceBoundOperation::NpmPublicSourceProducer,
        SourceProducerRole::Bun => operation == SourceBoundOperation::BunSourceProducer,
        SourceProducerRole::Tofu => operation == SourceBoundOperation::TofuProviderExport,
        SourceProducerRole::Gradle => operation == SourceBoundOperation::GradleSourceProducer,
    }
}

fn allowed_transport(
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    step: &Step,
    meta: &SourceProducer,
) -> bool {
    if !env.is_empty() {
        return false;
    }
    match step.id.as_ref() {
        Some(id) if id == &meta.restore_step => {
            step.condition.is_none() && restore_transport(uses, with, meta)
        }
        Some(id) if id == &meta.save_step => save_transport(uses, with, step, meta),
        Some(id) if id == &meta.publication_step => publication_transport(uses, with, step, meta),
        _ => false,
    }
}

fn restore_transport(uses: &str, with: &BTreeMap<String, String>, meta: &SourceProducer) -> bool {
    uses == crate::steps::TOOLS_RESTORE_USES
        && with
            .keys()
            .all(|key| matches!(key.as_str(), "path" | "key" | "restore-keys"))
        && with
            .get("path")
            .is_some_and(|path| source_paths(meta.role, path, true))
        && with.get("key").is_some_and(|key| key == &restore_key(meta))
        && with
            .get("restore-keys")
            .is_some_and(|keys| restore_keys(meta, keys))
}

fn save_transport(
    uses: &str,
    with: &BTreeMap<String, String>,
    step: &Step,
    meta: &SourceProducer,
) -> bool {
    uses == crate::steps::TOOLS_SAVE_USES
        && step.condition.as_deref() == Some(meta.save_condition().as_str())
        && with
            .keys()
            .all(|key| matches!(key.as_str(), "path" | "key"))
        && with
            .get("path")
            .is_some_and(|path| source_paths(meta.role, path, false))
        && with.get("key") == Some(&meta.save_key())
}

fn publication_transport(
    uses: &str,
    with: &BTreeMap<String, String>,
    step: &Step,
    meta: &SourceProducer,
) -> bool {
    uses == crate::steps::TOOLS_RESTORE_USES
        && step.condition.as_deref() == Some(meta.publication_condition().as_str())
        && with
            .keys()
            .all(|key| matches!(key.as_str(), "path" | "key" | "lookup-only"))
        && with.get("key") == Some(&meta.save_key())
        && with.get("lookup-only") == Some(&"true".to_owned())
        && with
            .get("path")
            .is_some_and(|path| source_paths(meta.role, path, false))
}

fn restore_key(meta: &SourceProducer) -> String {
    match meta.role {
        SourceProducerRole::Cargo => format!(
            "{}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}",
            meta.source_identity
        ),
        SourceProducerRole::Npm | SourceProducerRole::Bun => {
            format!("{}-lookup", meta.source_identity)
        }
        SourceProducerRole::Tofu | SourceProducerRole::Gradle => meta.source_identity.clone(),
    }
}

fn restore_keys(meta: &SourceProducer, value: &str) -> bool {
    match meta.role {
        SourceProducerRole::Cargo => value == format!("{}-snapshot-", meta.source_identity),
        SourceProducerRole::Npm | SourceProducerRole::Bun => {
            let current = format!("{}-snapshot-", meta.source_identity);
            value == current
                || compatible_prefix(meta)
                    .is_some_and(|prefix| value == format!("{current}\n{prefix}"))
        }
        SourceProducerRole::Tofu | SourceProducerRole::Gradle => value.is_empty(),
    }
}

fn compatible_prefix(meta: &SourceProducer) -> Option<String> {
    let suffix = meta
        .source_identity
        .get(meta.source_identity.len().checked_sub(67)?..)?;
    let digest = suffix.strip_prefix("b3-")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(meta.source_identity[..meta.source_identity.len() - 67].to_owned())
}

fn source_paths(role: SourceProducerRole, paths: &str, restore: bool) -> bool {
    let actual: Vec<_> = paths.split('\n').collect();
    if actual.iter().any(|path| {
        path.is_empty() || path.contains("..") || crate::cache_steps::is_never_archive_path(path)
    }) {
        return false;
    }
    match role {
        SourceProducerRole::Cargo => actual == cargo_paths(),
        SourceProducerRole::Npm => actual == npm_paths(),
        SourceProducerRole::Bun => {
            actual == vec!["${{ runner.temp }}/velnor/native/bun/install/cache"]
        }
        SourceProducerRole::Gradle => {
            actual == vec!["${{ runner.temp }}/velnor/native/gradle/caches/modules-2"]
        }
        SourceProducerRole::Tofu => {
            actual.len() == 1
                && (crate::tofu_cache::tofu_providers_path_ok(actual[0])
                    || (restore
                        && actual[0]
                            .strip_prefix("${{ runner.temp }}/velnor/tofu-provider-candidate/")
                            .is_some_and(|slug| {
                                !slug.is_empty()
                                    && slug.bytes().all(|byte| {
                                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                                    })
                            })))
        }
    }
}

fn cargo_paths() -> Vec<&'static str> {
    vec![
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/registry/index",
    ]
}

fn npm_paths() -> Vec<&'static str> {
    vec![
        "${{ runner.temp }}/velnor/native/npm/_cacache/content-v2",
        "${{ runner.temp }}/velnor/native/npm/public-proof-v1.json",
    ]
}

pub(super) const fn source_domain(role: SourceProducerRole) -> ToolCacheDomain {
    match role {
        SourceProducerRole::Cargo => ToolCacheDomain::Full,
        SourceProducerRole::Npm => ToolCacheDomain::NpmBootstrap,
        SourceProducerRole::Bun => ToolCacheDomain::BunBootstrap,
        SourceProducerRole::Tofu => ToolCacheDomain::TofuBootstrap,
        SourceProducerRole::Gradle => ToolCacheDomain::GradleBootstrap,
    }
}
