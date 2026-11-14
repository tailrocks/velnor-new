// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{io::Write, sync::OnceLock};

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};

use crate::{RenderError, yaml::Yaml};

const BOOTSTRAP: &str = velnor_actions_mise::source_preview::BOOTSTRAP;
const EXECUTOR_TEMPLATE: &str = include_str!("native_helper/source_preview_exec.py");
const SETUP: &str = include_str!("native_helper/source_preview_setup.py");
const LOADER: &str = include_str!("native_helper/source_preview_loader.py");

fn executor() -> &'static str {
    static EXECUTOR: OnceLock<String> = OnceLock::new();
    EXECUTOR.get_or_init(|| {
        let keys = velnor_actions_contract::config::project_tool_resolver::PROJECT_TOOL_KEYS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        EXECUTOR_TEMPLATE.replace(
            "\"__VELNOR_PROJECT_TOOL_KEYS__\"",
            &serde_json::json!(keys).to_string(),
        )
    })
}

pub(crate) const LOADER_SCRUB_KEYS: &[&str] = &[
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "ACTIONS_RUNTIME_TOKEN",
    "BASH_ENV",
    "ENV",
    "GITHUB_TOKEN",
    "LD_AUDIT",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONSTARTUP",
];

pub(crate) fn scrub_loader_env(environment: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(entries) = environment else {
        return Err(RenderError::InvalidWorkflow(
            "preview_loader_environment_invalid".to_owned(),
        ));
    };
    for name in LOADER_SCRUB_KEYS {
        entries.retain(|(key, _)| key != *name);
        entries.push(((*name).to_owned(), Yaml::str(String::new())));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

fn identity(preview: &velnor_actions_contract::source_preview::SourcePreviewLock) -> String {
    serde_json::json!({
        "channel": "source-preview",
        "version": preview.receipt.version,
        "target": preview.receipt.targets.linux.payload.target,
        "sha256": preview.receipt.targets.linux.payload.sha256,
        "source_sha": preview.receipt.source_sha,
        "receipt_sha256": preview.receipt_sha256,
    })
    .to_string()
}

pub(crate) fn loader_command(
    preview: &velnor_actions_contract::source_preview::SourcePreviewLock,
) -> Vec<String> {
    vec![
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        format!("exec(bytes.fromhex(\"{}\"))", hex(LOADER.as_bytes())),
        hex(Sha256::digest(BOOTSTRAP.as_bytes()).as_ref()),
        hex(Sha256::digest(executor().as_bytes()).as_ref()),
        identity(preview),
    ]
}

pub(crate) fn acquire_step(
    preview: &velnor_actions_contract::source_preview::SourcePreviewLock,
) -> Result<Yaml, RenderError> {
    static PAYLOAD: OnceLock<Result<String, String>> = OnceLock::new();
    let compressed = PAYLOAD
        .get_or_init(|| {
            let payload = serde_json::json!([SETUP, BOOTSTRAP, executor()]).to_string();
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
            encoder
                .write_all(payload.as_bytes())
                .map_err(|error| error.to_string())?;
            encoder
                .finish()
                .map(|bytes| STANDARD.encode(bytes))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| RenderError::InvalidWorkflow(error.clone()))?;
    let run = format!(
        "/usr/bin/python3 -I -c 'import base64,json,sys,zlib; p=json.loads(zlib.decompress(base64.b64decode(\"{}\"))); sys.argv=[\"preview-setup\"]+[s.encode().hex() for s in p[1:]]+[\"{}\"]; exec(p[0])'",
        compressed,
        hex(identity(preview).as_bytes()),
    );
    if run.len() > 21_000 {
        return Err(RenderError::InvalidWorkflow(
            "preview_setup_too_large".to_owned(),
        ));
    }
    let environment = [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "BASH_ENV",
        "ENV",
        "GITHUB_TOKEN",
        "LD_AUDIT",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONSTARTUP",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), Yaml::str(String::new())))
    .collect();
    let mut environment: Vec<_> = environment;
    environment.push((
        "GH_TOKEN".to_owned(),
        Yaml::str("${{ github.token }}".to_owned()),
    ));
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Stage protected preview launcher".to_owned()),
        ),
        ("shell".to_owned(), Yaml::str("bash".to_owned())),
        ("env".to_owned(), Yaml::Map(environment)),
        ("run".to_owned(), Yaml::str(run)),
    ]))
}
