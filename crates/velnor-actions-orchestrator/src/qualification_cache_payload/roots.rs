//! Closed root observations for staged cache import/export.
//!
//! Root paths are carried separately from qualification runtime identity:
//! their absolute runner-local spelling must never change a cache key. The
//! caller still validates each path against the generated job's live
//! environment before it can be used for filesystem operations.

use serde::Serialize;
use std::path::{Component, Path, PathBuf};

use velnor_actions_contract::{
    QualificationCacheLayer, canonical_json_str, strict_json::parse_strict_json_with_limit,
};

const ROOTS_SCHEMA: &str = "velnor.qualification-cache-roots/v1";
const MAX_ROOTS_JSON_BYTES: usize = 65_536;
const MAX_ROOTS: usize = 8;
const MAX_ROOT_PATH_BYTES: usize = 4_096;

/// Collect the paths configured by the generated owner job for one layer.
/// The result contains process environment values only; payload consumers
/// validate those paths against the same environment before use.
///
/// # Errors
/// Returns an error when the owner, layer, or configured root paths are invalid.
pub fn collect_owner_roots<E>(
    runner_temp: &Path,
    owner_id: &str,
    layer_name: &str,
    mut get: E,
) -> Result<String, &'static str>
where
    E: FnMut(&str) -> Option<String>,
{
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(|_| "qualification_cache_owner_invalid")?;
    let layer = parse_layer(layer_name).ok_or("qualification_cache_layer_invalid")?;
    let roots = expected_paths_for_environment(layer, runner_temp, &mut get)?;
    let observations = roots
        .into_iter()
        .map(|(id, path)| {
            let path = path
                .to_str()
                .ok_or("qualification_cache_root_environment_invalid")?;
            validate_absolute_path(path)?;
            Ok(RootDocument {
                id: id.as_str(),
                path: path.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let document = RootSetDocument {
        schema: ROOTS_SCHEMA,
        owner_id,
        layer: layer.as_str(),
        roots: observations,
    };
    canonical_json_str(&document).map_err(|_| "qualification_cache_roots_invalid")
}

#[derive(Serialize)]
struct RootSetDocument<'a> {
    schema: &'static str,
    owner_id: &'a str,
    layer: &'static str,
    roots: Vec<RootDocument>,
}

#[derive(Serialize)]
struct RootDocument {
    id: &'static str,
    path: String,
}

#[path = "roots_environment.rs"]
mod environment;
pub(crate) use environment::expected_paths_for_environment;

pub(crate) fn parse_layer(text: &str) -> Option<QualificationCacheLayer> {
    Some(match text {
        "mbx_objects" => QualificationCacheLayer::MbxObjects,
        "cargo_sources" => QualificationCacheLayer::CargoSources,
        "mise_tools" => QualificationCacheLayer::MiseTools,
        "tofu_providers" => QualificationCacheLayer::TofuProviders,
        "task_result" => QualificationCacheLayer::TaskResult,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CacheRootId {
    MbxActions,
    CargoGitDb,
    CargoRegistryCache,
    CargoRegistryIndex,
    CargoBin,
    CargoCrates2Json,
    CargoCratesToml,
    MiseData,
    RustupHome,
    TofuPlugins,
    TaskArtifacts,
}

impl CacheRootId {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "mbx_actions" => Self::MbxActions,
            "cargo_git_db" => Self::CargoGitDb,
            "cargo_registry_cache" => Self::CargoRegistryCache,
            "cargo_registry_index" => Self::CargoRegistryIndex,
            "cargo_bin" => Self::CargoBin,
            "cargo_crates2_json" => Self::CargoCrates2Json,
            "cargo_crates_toml" => Self::CargoCratesToml,
            "mise_data" => Self::MiseData,
            "rustup_home" => Self::RustupHome,
            "tofu_plugins" => Self::TofuPlugins,
            "task_artifacts" => Self::TaskArtifacts,
            _ => return None,
        })
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::MbxActions => "mbx_actions",
            Self::CargoGitDb => "cargo_git_db",
            Self::CargoRegistryCache => "cargo_registry_cache",
            Self::CargoRegistryIndex => "cargo_registry_index",
            Self::CargoBin => "cargo_bin",
            Self::CargoCrates2Json => "cargo_crates2_json",
            Self::CargoCratesToml => "cargo_crates_toml",
            Self::MiseData => "mise_data",
            Self::RustupHome => "rustup_home",
            Self::TofuPlugins => "tofu_plugins",
            Self::TaskArtifacts => "task_artifacts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheRootObservation {
    pub(crate) id: CacheRootId,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheRootSet {
    pub(crate) owner_id: String,
    pub(crate) layer: QualificationCacheLayer,
    pub(crate) roots: Vec<CacheRootObservation>,
}

impl CacheRootSet {
    /// Parse a bounded collector result with the required JSON spelling and bind it to this lane.
    pub(crate) fn parse(
        text: &str,
        owner_id: &str,
        layer: QualificationCacheLayer,
    ) -> Result<Self, &'static str> {
        if text.len() > MAX_ROOTS_JSON_BYTES || text.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("qualification_cache_roots_invalid");
        }
        let value = parse_strict_json_with_limit(text, MAX_ROOTS_JSON_BYTES)
            .map_err(|_| "qualification_cache_roots_invalid")?;
        let canonical =
            canonical_json_str(&value).map_err(|_| "qualification_cache_roots_invalid")?;
        if canonical != text {
            return Err("qualification_cache_roots_noncanonical");
        }
        parse_value(&value, owner_id, layer)
    }

    /// Require every path to equal its generated job's current configured root.
    ///
    /// The map is assembled from the private operation's environment, not from
    /// the collector result. This lets the collector report runtime paths
    /// while preventing that report from selecting a different live tree.
    pub(crate) fn validate_expected_paths(
        &self,
        expected: &[(CacheRootId, PathBuf)],
    ) -> Result<(), &'static str> {
        if expected.len() != self.roots.len() {
            return Err("qualification_cache_roots_mismatch");
        }
        for (observed, (expected_id, expected_path)) in self.roots.iter().zip(expected) {
            if observed.id != *expected_id || observed.path != *expected_path {
                return Err("qualification_cache_roots_mismatch");
            }
        }
        Ok(())
    }
}

fn parse_value(
    value: &serde_json::Value,
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<CacheRootSet, &'static str> {
    let object = value
        .as_object()
        .filter(|object| has_exact_keys(object, &["layer", "owner_id", "roots", "schema"]))
        .ok_or("qualification_cache_roots_invalid")?;
    if string_field(object, "schema")? != ROOTS_SCHEMA
        || string_field(object, "owner_id")? != owner_id
        || string_field(object, "layer")? != layer.as_str()
    {
        return Err("qualification_cache_roots_stale");
    }
    let roots_value = object
        .get("roots")
        .and_then(serde_json::Value::as_array)
        .filter(|roots| !roots.is_empty() && roots.len() <= MAX_ROOTS)
        .ok_or("qualification_cache_roots_invalid")?;
    let mut roots = Vec::with_capacity(roots_value.len());
    for root in roots_value {
        roots.push(parse_root(root)?);
    }
    validate_root_ids(layer, &roots)?;
    Ok(CacheRootSet {
        owner_id: owner_id.to_owned(),
        layer,
        roots,
    })
}

fn parse_root(value: &serde_json::Value) -> Result<CacheRootObservation, &'static str> {
    let object = value
        .as_object()
        .filter(|object| has_exact_keys(object, &["id", "path"]))
        .ok_or("qualification_cache_roots_invalid")?;
    let id = CacheRootId::parse(string_field(object, "id")?)
        .ok_or("qualification_cache_roots_invalid")?;
    let text = string_field(object, "path")?;
    validate_absolute_path(text)?;
    Ok(CacheRootObservation {
        id,
        path: PathBuf::from(text),
    })
}

fn validate_root_ids(
    layer: QualificationCacheLayer,
    roots: &[CacheRootObservation],
) -> Result<(), &'static str> {
    let expected = expected_root_ids(layer);
    if roots
        .windows(2)
        .any(|pair| pair[0].id.as_str() >= pair[1].id.as_str())
    {
        return Err("qualification_cache_roots_order_invalid");
    }
    if roots.len() != expected.len() || roots.iter().zip(expected).any(|(root, id)| root.id != *id)
    {
        return Err("qualification_cache_roots_mismatch");
    }
    Ok(())
}

fn expected_root_ids(layer: QualificationCacheLayer) -> &'static [CacheRootId] {
    const MBX: &[CacheRootId] = &[CacheRootId::MbxActions];
    const CARGO: &[CacheRootId] = &[
        CacheRootId::CargoGitDb,
        CacheRootId::CargoRegistryCache,
        CacheRootId::CargoRegistryIndex,
    ];
    const TOOLS: &[CacheRootId] = &[
        CacheRootId::CargoBin,
        CacheRootId::CargoCrates2Json,
        CacheRootId::CargoCratesToml,
        CacheRootId::MiseData,
        CacheRootId::RustupHome,
    ];
    const TOFU: &[CacheRootId] = &[CacheRootId::TofuPlugins];
    const TASK: &[CacheRootId] = &[CacheRootId::TaskArtifacts];
    match layer {
        QualificationCacheLayer::MbxObjects => MBX,
        QualificationCacheLayer::CargoSources => CARGO,
        QualificationCacheLayer::MiseTools => TOOLS,
        QualificationCacheLayer::TofuProviders => TOFU,
        QualificationCacheLayer::TaskResult => TASK,
    }
}

fn validate_absolute_path(text: &str) -> Result<(), &'static str> {
    if text.len() > MAX_ROOT_PATH_BYTES
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || Path::new(text).components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err("qualification_cache_root_path_invalid");
    }
    Ok(())
}

fn has_exact_keys(object: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn string_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, &'static str> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("qualification_cache_roots_invalid")
}

#[cfg(test)]
#[path = "tests/dispatch_cache_roots_tests.rs"]
mod tests;
