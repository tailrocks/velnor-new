// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_release::ReleaseError;

use crate::facts::TestResult;
use crate::release_fixture::{APP, release_view};

const HOOKS: [&str; 5] = [
    "[registry]\ncredential-provider = \"./steal.sh\"\n",
    "[registry]\nglobal-credential-providers = [\"cargo:token\", \"./steal.sh\"]\n",
    "[registry]\ncredential-provider = \"cargo:token-from-stdout ./steal.sh\"\n",
    "[registries.mine]\ncredential-provider = [\"./steal.sh\", \"--flag\"]\n",
    "[credential-alias]\nsteal = \"./steal.sh\"\n",
];

#[test]
fn a_release_root_refuses_a_cargo_credential_hook() -> TestResult {
    let config = RepoPath::new(".cargo/config.toml")?;
    for text in HOOKS {
        let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
        view.tracked_paths.insert(config.clone());
        view.files.insert(config.clone(), text.as_bytes().to_vec());
        let result = expected_files(&view);
        assert!(
            matches!(
                &result,
                Err(GenerateError::Release(ReleaseError::CargoCredentialProvider { file }))
                    if file == config.as_str()
            ),
            "{text}: {result:?}"
        );
    }
    Ok(())
}

#[test]
fn a_repository_without_a_release_root_may_use_any_credential_provider() -> TestResult {
    let config = RepoPath::new(".cargo/config.toml")?;
    for text in HOOKS {
        let mut view = release_view(&[APP], None)?;
        view.tracked_paths.insert(config.clone());
        view.files.insert(config.clone(), text.as_bytes().to_vec());
        let result = expected_files(&view);
        assert!(result.is_ok(), "{text}: {result:?}");
    }
    Ok(())
}

fn path_toolchain_view(
    release: bool,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let release_toml = release.then_some("[[binaries]]\npackage = \"app\"\n");
    let mut view = release_view(&[APP], release_toml)?;
    let toolchain = RepoPath::new("rust-toolchain.toml")?;
    view.files.insert(
        RepoPath::new("mise.toml")?,
        b"[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n".to_vec(),
    );
    view.tracked_paths.insert(toolchain.clone());
    view.files.insert(
        toolchain,
        b"[toolchain]\npath = \"/tmp/fake-toolchain\"\n".to_vec(),
    );
    Ok(view)
}

#[test]
fn a_release_root_refuses_a_rust_toolchain_that_names_a_path() -> TestResult {
    let result = expected_files(&path_toolchain_view(true)?);
    assert!(
        matches!(result, Err(GenerateError::ReleaseToolchainPath)),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn a_repository_without_a_release_root_may_use_a_path_toolchain() -> TestResult {
    let result = expected_files(&path_toolchain_view(false)?);
    assert!(result.is_ok(), "{result:?}");
    Ok(())
}
