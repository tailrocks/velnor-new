// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::path::PathBuf;

use velnor_runner_host::{HostError, resource_probe_release};

fn main() {
    if let Err(error) = run() {
        eprintln!("resource probe release: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), HostError> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next().ok_or(HostError::Identity)?;
    let paths = arguments.map(PathBuf::from).collect::<Vec<_>>();
    match (command.to_str(), paths.as_slice()) {
        (Some("validate-smoke"), [output, root]) => {
            resource_probe_release::validate_smoke(output, root)
        }
        (Some("write-manifest"), [inspect, archive, output]) => {
            resource_probe_release::write_manifest(inspect, archive, output)
        }
        _ => Err(HostError::Identity),
    }
}
