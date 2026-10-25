use super::CacheRootId;
use std::path::{Path, PathBuf};
use velnor_actions_contract::QualificationCacheLayer;

/// Derive the configured live roots from the private runner environment.
pub(crate) fn expected_paths_for_environment<E>(
    layer: QualificationCacheLayer,
    runner_temp: &Path,
    mut get: E,
) -> Result<Vec<(CacheRootId, PathBuf)>, &'static str>
where
    E: FnMut(&str) -> Option<String>,
{
    super::validate_absolute_path(
        runner_temp
            .to_str()
            .ok_or("qualification_cache_root_environment_invalid")?,
    )?;
    let velnor_temp = runner_temp.join("velnor");
    let expected = match layer {
        QualificationCacheLayer::MbxObjects => {
            let mbx = required_env_path(&mut get, "MBX_CACHE_DIR")?;
            if mbx != velnor_temp.join("mbx") {
                return Err("qualification_cache_root_environment_invalid");
            }
            vec![(CacheRootId::MbxActions, mbx.join("actions"))]
        }
        QualificationCacheLayer::CargoSources => {
            let cargo = cargo_home(&mut get, &velnor_temp, false)?;
            cargo_source_paths(&cargo)
        }
        QualificationCacheLayer::MiseTools => {
            let mise = required_env_path(&mut get, "MISE_DATA_DIR")?;
            if mise != velnor_temp.join("mise") {
                return Err("qualification_cache_root_environment_invalid");
            }
            let cargo = cargo_home(&mut get, &velnor_temp, true)?;
            let rustup = paired_env_path(
                &mut get,
                "RUSTUP_HOME",
                "MISE_RUSTUP_HOME",
                velnor_temp.join("rustup"),
            )?;
            vec![
                (CacheRootId::CargoBin, cargo.join("bin")),
                (CacheRootId::CargoCrates2Json, cargo.join(".crates2.json")),
                (CacheRootId::CargoCratesToml, cargo.join(".crates.toml")),
                (CacheRootId::MiseData, mise),
                (CacheRootId::RustupHome, rustup),
            ]
        }
        QualificationCacheLayer::TofuProviders => {
            let path = required_env_path(&mut get, "TF_PLUGIN_CACHE_DIR")?;
            validate_descendant(&path, &velnor_temp.join("cache/tofu"))?;
            if !valid_tofu_root_name(&path) {
                return Err("qualification_cache_root_environment_invalid");
            }
            vec![(CacheRootId::TofuPlugins, path)]
        }
        QualificationCacheLayer::TaskResult => {
            let parent = required_env_path(&mut get, "MISE_TASK_CACHE_DIR")?;
            validate_descendant(&parent, &velnor_temp.join("tasks"))?;
            vec![(CacheRootId::TaskArtifacts, parent.join("task-artifacts/v2"))]
        }
    };
    Ok(expected)
}

fn cargo_home<E>(
    get: &mut E,
    velnor_temp: &Path,
    require_mise_value: bool,
) -> Result<PathBuf, &'static str>
where
    E: FnMut(&str) -> Option<String>,
{
    let expected = velnor_temp.join("cargo");
    let cargo = required_env_path(get, "CARGO_HOME")?;
    if cargo != expected {
        return Err("qualification_cache_root_environment_invalid");
    }
    match get("MISE_CARGO_HOME") {
        Some(value) if parse_env_path(&value)? == cargo => {}
        None if !require_mise_value => {}
        _ => return Err("qualification_cache_root_environment_invalid"),
    }
    Ok(cargo)
}

fn paired_env_path<E>(
    get: &mut E,
    left: &str,
    right: &str,
    expected: PathBuf,
) -> Result<PathBuf, &'static str>
where
    E: FnMut(&str) -> Option<String>,
{
    let left = required_env_path(get, left)?;
    let right = required_env_path(get, right)?;
    if left != expected || right != expected {
        return Err("qualification_cache_root_environment_invalid");
    }
    Ok(expected)
}

fn cargo_source_paths(cargo: &Path) -> Vec<(CacheRootId, PathBuf)> {
    vec![
        (CacheRootId::CargoGitDb, cargo.join("git/db")),
        (
            CacheRootId::CargoRegistryCache,
            cargo.join("registry/cache"),
        ),
        (
            CacheRootId::CargoRegistryIndex,
            cargo.join("registry/index"),
        ),
    ]
}

fn required_env_path<E>(get: &mut E, name: &str) -> Result<PathBuf, &'static str>
where
    E: FnMut(&str) -> Option<String>,
{
    get(name)
        .ok_or("qualification_cache_root_environment_missing")
        .and_then(|text| parse_env_path(&text))
}

fn parse_env_path(text: &str) -> Result<PathBuf, &'static str> {
    super::validate_absolute_path(text)
        .map_err(|_| "qualification_cache_root_environment_invalid")?;
    Ok(PathBuf::from(text))
}

fn validate_descendant(path: &Path, root: &Path) -> Result<(), &'static str> {
    if path == root || !path.starts_with(root) {
        return Err("qualification_cache_root_environment_invalid");
    }
    Ok(())
}

fn valid_tofu_root_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(digest) = name.strip_prefix("root-") else {
        return false;
    };
    digest.len() == 12 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}
