//! Bind fixed pre-seed MBX vectors to the native action executable.

use crate::{RenderError, cache_steps::ACTION_MBX_BIN};

pub(super) fn probe_argv_for_action(probe: &[String]) -> Result<Vec<String>, RenderError> {
    super::check_probe_shape(probe)?;
    let offset = crate::toolchain_env::unset_prefix_len(probe);
    let mut runtime = probe.to_vec();
    ACTION_MBX_BIN.clone_into(&mut runtime[offset + 7]);
    Ok(runtime)
}

pub(super) fn build_argv_for_action(build: &[String]) -> Result<Vec<String>, RenderError> {
    let offset = crate::toolchain_env::unset_prefix_len(build);
    let payload = build
        .get(offset..)
        .ok_or_else(|| RenderError::BadCommand("preseed_bad_mbx_build".to_owned()))?;
    let malformed = || RenderError::BadCommand("preseed_bad_mbx_build".to_owned());
    if payload.len() < 10
        || payload.iter().take(5).map(String::as_str).ne([
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
        ])
        || payload.get(6).map(String::as_str) != Some("--")
        || payload.get(7).map(String::as_str) != Some("mbx")
        || payload.get(9).map(String::as_str) != Some("build")
    {
        return Err(malformed());
    }
    let rust = payload[5].strip_prefix("rust@").ok_or_else(malformed)?;
    let rust_selector = format!("+{rust}");
    if !super::is_exact_version(rust) || payload.get(8) != Some(&rust_selector) {
        return Err(malformed());
    }
    let mut runtime = build.to_vec();
    ACTION_MBX_BIN.clone_into(&mut runtime[offset + 7]);
    Ok(runtime)
}
