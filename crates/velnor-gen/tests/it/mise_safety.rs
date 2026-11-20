// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;
use velnor_rust::{DetectError, detect};

use crate::facts::TestResult;
use crate::pipeline::view;

#[test]
fn detect_rejects_mise_configuration_that_can_change_installation() -> TestResult {
    let path = RepoPath::new("mise.toml")?;
    let mut repo = view(&["Cargo.toml"], None)?;
    let valid = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";
    repo.files.insert(path.clone(), valid.as_bytes().to_vec());
    repo.files.insert(RepoPath::new("mise.lock")?, b"[[tools.rust]]\nversion = \"1.99.1\"\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n".to_vec());
    assert!(detect(&repo).is_ok());
    for text in [
        valid.replace("1.99.1", "{{ exec(command='true') }}1.99.1"),
        format!("{valid}\n[plugins]\nrust = \"https://example.com/plugin\"\n"),
        format!("{valid}\n[vars]\nx = \"{{{{ exec(command='true') }}}}\"\n"),
        format!("unknown = true\n{valid}"),
    ] {
        repo.files.insert(path.clone(), text.as_bytes().to_vec());
        let result = detect(&repo);
        assert!(
            matches!(result, Err(DetectError::Invalid { ref path, .. }) if *path == RepoPath::new("mise.toml")?),
            "accepted {text}: {result:?}"
        );
    }
    Ok(())
}
