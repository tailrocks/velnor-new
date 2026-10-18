//! Fixed isolated paths for authenticated cache payloads.

use std::path::{Path, PathBuf};

/// Derive the isolated stage path for one exact qualification key.
///
/// # Errors
///
/// Gives a static error code when the runner temp, campaign, layer, or key is
/// invalid.
pub fn qualification_cache_stage_path(
    runner_temp: &Path,
    campaign: &str,
    layer: &str,
    key: &str,
) -> Result<PathBuf, &'static str> {
    let runner_temp = clean_path_text(runner_temp, "cache_binding_stage_path_invalid")?;
    if [campaign, layer, key]
        .iter()
        .any(|value| value.chars().any(|ch| matches!(ch, '\n' | '\r')))
    {
        return Err("cache_binding_output_invalid");
    }
    let valid_campaign = !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let valid_layer = matches!(
        layer,
        "mbx_objects" | "cargo_sources" | "mise_tools" | "tofu_providers" | "task_result"
    );
    let valid_key = !key.is_empty()
        && key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        && key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
        })
        && key.starts_with(&format!(
            "velnor-qcache-v{}-{campaign}-{layer}-",
            velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
        ));
    if !valid_campaign || !valid_layer || !valid_key {
        return Err("cache_binding_stage_path_invalid");
    }
    let key_digest = blake3::hash(key.as_bytes()).to_hex();
    let path = format!(
        "{runner_temp}/velnor/qualification-cache-v{}/{campaign}/{layer}/{key_digest}/payload",
        velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
    );
    if path.len() > 4_096 {
        return Err("cache_binding_stage_path_invalid");
    }
    Ok(PathBuf::from(path))
}

/// Derive the isolated stage path for one normal tools cache key.
///
/// # Errors
///
/// Gives a static error code when the runner temp or full cache key is invalid.
pub fn normal_tools_cache_stage_path(
    runner_temp: &Path,
    cache_key: &str,
) -> Result<PathBuf, &'static str> {
    let runner_temp = clean_path_text(runner_temp, "normal_cache_stage_path_invalid")?;
    let Some(identity) = cache_key.strip_prefix("mise-tools-v2-payload-v3-") else {
        return Err("normal_cache_key_invalid");
    };
    if identity.len() != 64
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_cache_key_invalid");
    }
    let key_digest = blake3::hash(cache_key.as_bytes()).to_hex();
    let path = format!("{runner_temp}/velnor/normal-cache-v3/mise_tools/{key_digest}/payload");
    if path.len() > 4_096 {
        return Err("normal_cache_stage_path_invalid");
    }
    Ok(PathBuf::from(path))
}

fn clean_path_text<'a>(path: &'a Path, error: &'static str) -> Result<&'a str, &'static str> {
    let text = path.to_str().ok_or(error)?;
    if text.len() > 4_096
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(error);
    }
    Ok(text)
}
