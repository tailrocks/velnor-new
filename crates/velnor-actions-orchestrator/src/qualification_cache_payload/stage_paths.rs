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
    crate::cache_action_stage::StageContext::qualification_action_path(
        Path::new(runner_temp),
        campaign,
        layer,
        key,
    )
    .map_err(|_| "cache_binding_stage_path_invalid")
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
    let path = crate::cache_action_stage::StageContext::normal_action_path(
        Path::new(runner_temp),
        cache_key,
    )
    .map_err(|error| match error {
        "cache_stage_context_invalid" => "normal_cache_key_invalid",
        _ => "normal_cache_stage_path_invalid",
    })?;
    if path.as_os_str().len() > 4_096 {
        return Err("normal_cache_stage_path_invalid");
    }
    Ok(path)
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

#[cfg(test)]
#[path = "tests/stage_paths_tests.rs"]
mod tests;
