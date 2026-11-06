// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use crate::{
    RenderError, marker,
    tree::RenderedFile,
    yaml::{Yaml, render_yaml},
};

const BOOTSTRAP: &str = include_str!("native_helper/source_preview_bootstrap.py");
const EXECUTOR: &str = include_str!("native_helper/source_preview_exec.py");
const SETUP: &str = include_str!("native_helper/source_preview_setup.py");
const LOADER: &str = include_str!("native_helper/source_preview_loader.py");

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
        "target": preview.receipt.payload.target,
        "sha256": preview.receipt.payload.sha256,
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
        hex(Sha256::digest(EXECUTOR.as_bytes()).as_ref()),
        identity(preview),
    ]
}

pub(crate) fn action_file(
    version: &str,
    preview: &velnor_actions_contract::source_preview::SourcePreviewLock,
) -> Result<RenderedFile, RenderError> {
    let run = format!(
        "/usr/bin/python3 -I -c \"exec(bytes.fromhex('{}'))\" {} {} {}",
        hex(SETUP.as_bytes()),
        hex(BOOTSTRAP.as_bytes()),
        hex(EXECUTOR.as_bytes()),
        hex(identity(preview).as_bytes()),
    );
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
    let step = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Stage protected preview launcher".to_owned()),
        ),
        ("shell".to_owned(), Yaml::str("bash".to_owned())),
        ("env".to_owned(), Yaml::Map(environment)),
        ("run".to_owned(), Yaml::str(run)),
    ]);
    let action = crate::composite::composite_yaml("velnor-preview-setup", vec![step])?;
    Ok(RenderedFile {
        path: ".github/actions/velnor-preview-setup/action.yml".to_owned(),
        bytes: marker::with_marker(version, &render_yaml(&action))?,
    })
}
