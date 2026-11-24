// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Stdio};

use crate::error::CiError;
use crate::image_deploy::promote::Registry;

const STDERR_SHOWN: usize = 1000;

pub(super) struct Docker;

fn failure(reference: &str, stderr: &[u8]) -> CiError {
    let text = String::from_utf8_lossy(stderr);
    let shown: String = text.trim().chars().take(STDERR_SHOWN).collect();
    CiError::new(format!(
        "docker buildx imagetools failed for {reference}: {shown}"
    ))
}

fn digest_text(text: &str) -> Option<&str> {
    let hex = text.strip_prefix("sha256:")?;
    (hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(text)
}

impl Registry for Docker {
    fn digest(&self, reference: &str) -> Result<Option<String>, CiError> {
        let output = Command::new("docker")
            .args([
                "buildx",
                "imagetools",
                "inspect",
                reference,
                "--format",
                "{{.Manifest.Digest}}",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| CiError::new(format!("cannot run docker: {error}")))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            return digest_text(text.trim())
                .map(|digest| Some(digest.to_owned()))
                .ok_or_else(|| {
                    CiError::new(format!("docker answered {reference} with no digest"))
                });
        }
        if String::from_utf8_lossy(&output.stderr)
            .to_ascii_lowercase()
            .contains("not found")
        {
            return Ok(None);
        }
        Err(failure(reference, &output.stderr))
    }

    fn tag(&self, source: &str, target: &str) -> Result<(), CiError> {
        let output = Command::new("docker")
            .args(["buildx", "imagetools", "create", "--tag", target, source])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| CiError::new(format!("cannot run docker: {error}")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(failure(target, &output.stderr))
        }
    }
}
