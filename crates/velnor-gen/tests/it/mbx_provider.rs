// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::mbx_default::{NEXTEST, OPTION, PIN, WRAPPER, pinned, with_mises};

fn refused_provider(
    key: &str,
    visible: bool,
) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let line = format!("\"{key}\" = \"v1.30.0\"\n");
    let (first, hidden) = if visible {
        (
            format!("[tools]\n{OPTION}{PIN}{NEXTEST}{line}{WRAPPER}"),
            None,
        )
    } else {
        (
            format!("[tools]\n{OPTION}{PIN}{NEXTEST}{WRAPPER}"),
            Some(format!("[tools]\n{line}")),
        )
    };
    let repo = with_mises(pinned(None, "0.1.88")?, &first, hidden.as_deref())?;
    let error = expected_files(&repo)
        .err()
        .ok_or(format!("accepted {key}"))?;
    let text = error.to_string();
    let GenerateError::Detect(DetectError::MbxSecondProvider(provider)) = error else {
        return Err(text.into());
    };
    Ok((provider.key, provider.path.to_string(), text))
}

#[test]
fn a_second_mbx_provider_next_to_the_pin_is_refused_in_either_file() -> TestResult {
    for key in [
        "aqua:jdx/mr-boxington",
        "github:evil/mr-boxington",
        "cargo:mr-boxington",
    ] {
        for (visible, file) in [(true, "mise.toml"), (false, ".mise.toml")] {
            let (found, path, text) = refused_provider(key, visible)?;
            assert_eq!((found.as_str(), path.as_str()), (key, file));
            assert!(text.contains("second provider"), "{text}");
            assert!(text.contains("mbx = false"), "{text}");
        }
    }
    Ok(())
}
