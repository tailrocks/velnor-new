// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, io::Write as _};
use velnor_actions_contract::Step;

pub fn render_entry(timeout_seconds: u32) -> Result<String, RenderError> {
    let bundle = crate::release_privileged_modules::supervisor_bundle();
    let encoded =
        STANDARD
            .encode(serde_json::to_vec(&bundle).map_err(|_| {
                RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned())
            })?);
    Ok(render_helpers(timeout_seconds)?
        .replace("SUPERVISOR = 'e30='", &format!("SUPERVISOR = '{encoded}'")))
}

pub(crate) fn render_helpers(timeout_seconds: u32) -> Result<String, RenderError> {
    render_source(timeout_seconds, &STANDARD.encode(b"{}"))
}

fn render_source(timeout_seconds: u32, encoded: &str) -> Result<String, RenderError> {
    if !(61..=21_600).contains(&timeout_seconds) {
        return Err(RenderError::InvalidWorkflow(
            "publisher_timeout_invalid".to_owned(),
        ));
    }
    let contract = STANDARD.encode(
        serde_json::to_vec(&crate::release_privileged_modules::export_contract())
            .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?,
    );
    let mut entry = include_str!("release_publish_entry.py")
        .replace("__VELNOR_EXPORT_CONTRACT_PAYLOAD__", &contract)
        .replace("__VELNOR_SUPERVISOR_PAYLOAD__", encoded)
        .replace(
            "__VELNOR_JOB_TIMEOUT_SECONDS__",
            &timeout_seconds.to_string(),
        );
    for source in crate::release_privileged_modules::bootstrap_sources() {
        let marker = format!("__VELNOR_{}_PAYLOAD__", source.marker);
        if entry.matches(&marker).count() != 1 {
            return Err(RenderError::InvalidWorkflow(
                "publisher_module_marker_invalid".to_owned(),
            ));
        }
        entry = entry.replace(&marker, &STANDARD.encode(source.source.as_bytes()));
    }
    if entry.contains("__VELNOR_") {
        return Err(RenderError::InvalidWorkflow(
            "publisher_bundle_invalid".to_owned(),
        ));
    }
    Ok(entry)
}

pub fn publisher_step(
    bootstrap: bool,
    version: &str,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<Step, RenderError> {
    crate::release_generator_bootstrap::acquire_step(
        version,
        crate::release_tree::RELEASE_HOSTED_RUNNER,
        source_sha,
        manifest_sha256,
    )?;
    let encoded = publisher_bundle()?;
    let script = bootstrap_payload_script(&encoded)?;
    let preserve = concat!(
        "GH_TOKEN,GITHUB_REPOSITORY,GITHUB_REPOSITORY_ID,GITHUB_REPOSITORY_OWNER_ID,",
        "GITHUB_RUN_ID,GITHUB_RUN_ATTEMPT,GITHUB_JOB,GITHUB_SHA,GITHUB_REF,",
        "GITHUB_EVENT_NAME,GITHUB_WORKFLOW_REF,GITHUB_WORKFLOW_SHA,GITHUB_SERVER_URL,",
        "GITHUB_API_URL,RUNNER_ENVIRONMENT,RUNNER_OS,RUNNER_ARCH,",
        "VELNOR_RELEASE_BUDGET,VELNOR_PREFLIGHT_ELIGIBLE,VELNOR_PREFLIGHT_SOURCE_SHA,",
        "VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID,VELNOR_PREFLIGHT_RUST_ARTIFACT_ID"
    );
    let credentials = if bootstrap {
        "CARGO_REGISTRY_TOKEN"
    } else {
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN,ACTIONS_ID_TOKEN_REQUEST_URL"
    };
    let mode = if bootstrap {
        "bootstrap"
    } else {
        "trusted-publishing"
    };
    let argv = vec![
        "/usr/bin/sudo".to_owned(),
        "-n".to_owned(),
        format!("--preserve-env={preserve},{credentials},VELNOR_RELEASE_BOOTSTRAP_PAYLOAD"),
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        script,
        mode.to_owned(),
        version.to_owned(),
        source_sha.to_owned(),
        manifest_sha256.to_owned(),
    ];
    if crate::commands::join_argv_for_run(&argv)?.len() > 21_000 {
        return Err(RenderError::InvalidWorkflow(
            "publisher_step_too_large".to_owned(),
        ));
    }
    let mut env = publisher_environment(bootstrap);
    env.insert("VELNOR_RELEASE_BOOTSTRAP_PAYLOAD".to_owned(), encoded);
    crate::ambient_shell_step("Publish approved Rust crate", argv, env)
}

pub(crate) fn bootstrap_payload_script(encoded: &str) -> Result<String, RenderError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if encoded.is_empty() || encoded.len() > 32 * 1024 {
        return Err(RenderError::InvalidWorkflow(
            "publisher_bundle_invalid".to_owned(),
        ));
    }
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(encoded.as_bytes()) {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let script =
        include_str!("release_bootstrap_payload.py").replace("__VELNOR_PAYLOAD_SHA256__", &digest);
    Ok(format!("exec({script:?})"))
}

fn publisher_bundle() -> Result<String, RenderError> {
    let mut bundle = serde_json::Map::from_iter([
        (
            "stage".to_owned(),
            serde_json::json!(include_str!("release_publish_first_stage.py")),
        ),
        (
            "entry".to_owned(),
            serde_json::json!(include_str!("release_publish_entry.py")),
        ),
        (
            "export_contract".to_owned(),
            crate::release_privileged_modules::export_contract(),
        ),
    ]);
    for source in crate::release_privileged_modules::bootstrap_sources() {
        if bundle
            .insert(source.key.to_owned(), serde_json::json!(source.source))
            .is_some()
        {
            return Err(RenderError::InvalidWorkflow(
                "publisher_module_key_duplicate".to_owned(),
            ));
        }
    }
    let raw = serde_json::to_vec(&bundle)
        .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?;
    if raw.len() > 512 * 1024 {
        return Err(RenderError::InvalidWorkflow(
            "publisher_bundle_invalid".to_owned(),
        ));
    }
    let mut options = lzma_rust2::XzOptions::with_preset(6);
    options.lzma_options.nice_len = 273;
    options.lzma_options.pb = 0;
    let mut compressor = lzma_rust2::XzWriter::new(Vec::new(), options)
        .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?;
    compressor
        .write_all(&raw)
        .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?;
    Ok(STANDARD.encode(
        compressor
            .finish()
            .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?,
    ))
}

fn publisher_environment(bootstrap: bool) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            "VELNOR_RELEASE_BUDGET".to_owned(),
            "${{ steps.release-budget.outputs.budget }}".to_owned(),
        ),
        (
            "VELNOR_PREFLIGHT_ELIGIBLE".to_owned(),
            "${{ needs.release-preflight.outputs.eligible }}".to_owned(),
        ),
        (
            "VELNOR_PREFLIGHT_SOURCE_SHA".to_owned(),
            "${{ needs.release-preflight.outputs.source_sha }}".to_owned(),
        ),
        (
            "VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID".to_owned(),
            "${{ needs.release-preflight.outputs.receipt_artifact_id }}".to_owned(),
        ),
        (
            "VELNOR_PREFLIGHT_RUST_ARTIFACT_ID".to_owned(),
            "${{ needs.release-preflight.outputs.rust_artifact_id }}".to_owned(),
        ),
        ("LD_PRELOAD".to_owned(), String::new()),
        ("LD_AUDIT".to_owned(), String::new()),
        ("GLIBC_TUNABLES".to_owned(), String::new()),
        ("LD_LIBRARY_PATH".to_owned(), String::new()),
        ("BASH_ENV".to_owned(), String::new()),
        ("ENV".to_owned(), String::new()),
    ]);
    if bootstrap {
        env.insert(
            "CARGO_REGISTRY_TOKEN".to_owned(),
            "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        );
    }
    env
}

#[cfg(test)]
mod tests {
    use super::render_entry;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    #[test]
    fn rendered_entry_is_executable_python() -> Result<(), Box<dyn std::error::Error>> {
        let rendered = render_entry(1800)?;
        let mut child = Command::new("python3")
            .args([
                "-I",
                "-S",
                "-c",
                "import sys; compile(sys.stdin.read(), '<entry>', 'exec')",
            ])
            .stdin(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("missing stdin")?
            .write_all(rendered.as_bytes())?;
        assert!(child.wait()?.success());
        Ok(())
    }
}
