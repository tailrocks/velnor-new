// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;
use velnor_rust::{DetectError, Reason, detect};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn cargo_response_file_arguments_are_refused_in_string_and_array_flags() -> TestResult {
    let config = RepoPath::new(".cargo/config.toml")?;
    let scopes = [
        ("build", "rustflags"),
        ("target.x86_64-unknown-linux-gnu", "rustflags"),
        ("build", "rustdocflags"),
    ];
    for (section, key) in scopes {
        for flags in [
            "\"@hidden.args\"",
            "\" @hidden.args\"",
            "\"--cfg x @hidden.args\"",
            "\"--cfg x\\t@hidden.args\"",
            "[\"@hidden.args\"]",
            "[\"--cfg\", \"x\", \"@hidden.args\"]",
            "\"--cap-lints allow\"",
            "\"--force-warn unused-variables\"",
            "[\"--cap-lints\", \"allow\"]",
            "[\"--force-warn\", \"unused-variables\"]",
        ] {
            let mut repo = view(&["Cargo.toml"], None)?;
            let text = format!("[{section}]\n{key} = {flags}\n");
            repo.tracked_paths.insert(config.clone());
            repo.files.insert(config.clone(), text.as_bytes().to_vec());
            let reason = if key == "rustdocflags" && flags.starts_with('"') {
                Reason::RustdocflagsString
            } else {
                Reason::CapLints
            };
            assert_eq!(
                detect(&repo),
                Err(DetectError::Invalid {
                    path: config.clone(),
                    reason,
                }),
                "{text}"
            );
        }
    }
    for flags in ["\"--cfg x @hidden.args\"", "[\"@hidden.args\"]"] {
        let mut repo = view(&["Cargo.toml"], None)?;
        repo.tracked_paths.insert(config.clone());
        repo.files.insert(
            config.clone(),
            format!("[target.x86_64-unknown-linux-gnu]\nrustdocflags = {flags}\n").into_bytes(),
        );
        assert_eq!(
            detect(&repo),
            Err(DetectError::Invalid {
                path: config.clone(),
                reason: Reason::RustdocflagsString,
            })
        );
    }
    Ok(())
}

#[test]
fn an_at_sign_inside_a_flag_token_still_generates() -> TestResult {
    for (section, key, flags) in [
        ("build", "rustflags", "\"--cfg feat@ure\""),
        ("build", "rustflags", "[\"--cfg\", \"feat@ure\"]"),
        (
            "target.x86_64-unknown-linux-gnu",
            "rustflags",
            "\"--cfg feat@ure\"",
        ),
        ("build", "rustdocflags", "[\"--cfg\", \"feat@ure\"]"),
    ] {
        let mut repo = view(&["Cargo.toml"], None)?;
        let config = RepoPath::new(".cargo/config.toml")?;
        repo.tracked_paths.insert(config.clone());
        repo.files.insert(
            config,
            format!("[{section}]\n{key} = {flags}\n").into_bytes(),
        );
        assert_ne!(workflow(&repo)?, "");
    }
    Ok(())
}
