// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{collections::BTreeMap, io::Write as _};
use velnor_actions_contract::Step;

pub fn render_entry(timeout_seconds: u32) -> Result<String, RenderError> {
    if !(61..=21_600).contains(&timeout_seconds) {
        return Err(RenderError::InvalidWorkflow(
            "publisher_timeout_invalid".to_owned(),
        ));
    }
    let bundle = serde_json::json!({
        "coordinator": STANDARD.encode(include_bytes!("release_coordinator_exec.py")),
        "watch": STANDARD.encode(include_bytes!("release_launcher_watch.py")),
        "claim": STANDARD.encode(include_bytes!("release_supervisor_claim.py")),
        "retained": STANDARD.encode(include_bytes!("release_retained_parent.py")),
        "source": STANDARD.encode(include_bytes!("release_source_executor.py")),
        "channel": STANDARD.encode(include_bytes!("release_source_channel.py")),
        "upload": STANDARD.encode(include_bytes!("release_upload_operation.py")),
        "broker": STANDARD.encode(include_bytes!("release_forge_broker.py")),
    });
    let encoded =
        STANDARD
            .encode(serde_json::to_vec(&bundle).map_err(|_| {
                RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned())
            })?);
    let entry = include_str!("release_publish_entry.py")
        .replace(
            "__VELNOR_BOOTSTRAP_PAYLOAD__",
            &STANDARD.encode(include_bytes!("release_generator_bootstrap.sh")),
        )
        .replace(
            "__VELNOR_MANAGER_PAYLOAD__",
            &STANDARD.encode(include_bytes!("release_manager_bus.py")),
        )
        .replace(
            "__VELNOR_LIFECYCLE_PAYLOAD__",
            &STANDARD.encode(include_bytes!("release_manager_lifecycle.py")),
        )
        .replace(
            "__VELNOR_HELPER_PAYLOAD__",
            &STANDARD.encode(include_bytes!("release_supervisor_exec.py")),
        )
        .replace("__VELNOR_SUPERVISOR_PAYLOAD__", &encoded)
        .replace(
            "__VELNOR_JOB_TIMEOUT_SECONDS__",
            &timeout_seconds.to_string(),
        );
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
    let script = format!(
        "import base64,json,zlib;bundle=json.loads(zlib.decompress(base64.b64decode('{encoded}',validate=True)));exec(compile(bundle['stage'],'<publisher>','exec'),{{'bundle':bundle,'__name__':'__main__'}})"
    );
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
        format!("--preserve-env={preserve},{credentials}"),
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
    let env = publisher_environment(bootstrap);
    crate::ambient_shell_step("Publish approved Rust crate", argv, env)
}

fn publisher_bundle() -> Result<String, RenderError> {
    let bundle = serde_json::json!({
        "stage": include_str!("release_publish_first_stage.py"),
        "entry": include_str!("release_publish_entry.py"),
        "bootstrap": include_str!("release_generator_bootstrap.sh"),
        "manager": include_str!("release_manager_bus.py"),
        "lifecycle": include_str!("release_manager_lifecycle.py"),
        "helper": include_str!("release_supervisor_exec.py"),
    });
    let raw = serde_json::to_vec(&bundle)
        .map_err(|_| RenderError::InvalidWorkflow("publisher_bundle_invalid".to_owned()))?;
    let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
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
