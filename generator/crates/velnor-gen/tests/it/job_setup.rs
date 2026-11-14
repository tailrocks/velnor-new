// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn the_job_installs_the_pinned_rust_and_cargo_nextest_and_needs_a_real_nextest_pin() -> TestResult {
    let mut pinned = view(&["Cargo.toml"], None)?;
    let other = RepoPath::new(".mise.toml")?;
    pinned.files.remove(&RepoPath::new("mise.toml")?);
    pinned.tracked_paths.remove(&RepoPath::new("mise.toml")?);
    pinned.tracked_paths.insert(other.clone());
    pinned.files.insert(
        other.clone(),
        b"[tools]\n\"core:rust\" = \"1.99.1\"\n\"cargo:cargo-nextest\" = \"0.9.151\"\n".to_vec(),
    );
    assert!(workflow(&pinned)?.contains("install_args: 'core:rust cargo:cargo-nextest'\n"));
    let mut unpinned = pinned;
    unpinned.files.insert(
        other.clone(),
        b"[tools]\nrust = \"1.99.1\"\nnextest-helper = \"1\"\n".to_vec(),
    );
    assert_eq!(
        expected_files(&unpinned),
        Err(GenerateError::Detect(DetectError::NextestRequired {
            path: other
        }))
    );
    Ok(())
}

#[test]
fn a_toolchain_file_that_disagrees_with_the_mise_rust_pin_is_an_error() -> TestResult {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut agreeing = view(&["Cargo.toml"], None)?;
    agreeing.tracked_paths.insert(file.clone());
    agreeing.files.insert(
        file.clone(),
        b"[toolchain]\nchannel = \"1.99.1\"\n".to_vec(),
    );
    assert!(expected_files(&agreeing).is_ok());
    let mut disagreeing = agreeing;
    disagreeing
        .files
        .insert(file, b"[toolchain]\nchannel = \"1.98.0\"\n".to_vec());
    assert_eq!(
        expected_files(&disagreeing),
        Err(GenerateError::Detect(DetectError::ToolchainConflict {
            file: "1.98.0".to_owned(),
            mise: "1.99.1".to_owned(),
        }))
    );
    Ok(())
}

#[test]
fn rustdocflags_in_cargo_config_must_be_an_array_the_doc_step_can_extend() -> TestResult {
    let config = RepoPath::new("lab/.cargo/config.toml")?;
    let mut array = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    array.tracked_paths.insert(config.clone());
    array.files.insert(
        config.clone(),
        b"[build]\nrustdocflags = [\"--cfg\", \"docsrs\"]\n".to_vec(),
    );
    assert!(
        workflow(&array)?.contains("--config ")
            && workflow(&array)?.contains("build.rustdocflags=[")
    );
    let mut string = array;
    string.files.insert(
        config.clone(),
        b"[build]\nrustdocflags = \"--cfg docsrs\"\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&string),
        Err(GenerateError::Detect(DetectError::Invalid { path, .. })) if path == config
    ));
    let mut included = string.clone();
    included
        .files
        .insert(config.clone(), b"include = [\"ci.toml\"]\n".to_vec());
    assert!(matches!(
        expected_files(&included),
        Err(GenerateError::Detect(DetectError::Invalid { path, .. })) if path == config
    ));
    for kept in [".mise/tasks/test", ".config/nextest.toml"] {
        let mut fine = view(&["Cargo.toml"], None)?;
        fine.tracked_paths.insert(RepoPath::new(kept)?);
        assert!(expected_files(&fine).is_ok(), "{kept}");
    }
    let mut capped = string.clone();
    capped.files.insert(
        config.clone(),
        b"[build]\nrustflags = [\"--cap-lints\", \"allow\"]\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&capped),
        Err(GenerateError::Detect(DetectError::Invalid { ref path, .. })) if *path == config
    ));
    let mut per_target = string;
    per_target.files.insert(
        config.clone(),
        b"[target.x86_64-unknown-linux-gnu]\nrustdocflags = [\"--cfg\", \"docsrs\"]\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&per_target),
        Err(GenerateError::Detect(DetectError::Invalid { path, .. })) if path == config
    ));
    Ok(())
}

#[test]
fn a_mise_configuration_may_not_change_what_the_steps_run() -> TestResult {
    let mise = RepoPath::new("mise.toml")?;
    for variable in [
        "RUSTDOCFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS",
        "RUSTUP_TOOLCHAIN",
        "_.file",
        "_.path",
        "RUSTDOC",
        "ANY_NAME",
    ] {
        let mut mise_env = view(&["Cargo.toml"], None)?;
        mise_env.files.insert(
            mise.clone(),
            format!(
                "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[env]\n{variable} = \"x\"\n"
            )
            .into_bytes(),
        );
        assert!(
            matches!(
                expected_files(&mise_env),
                Err(GenerateError::Detect(DetectError::Invalid { ref path, .. })) if *path == mise
            ),
            "{variable}"
        );
    }
    let mut mise_include = view(&["Cargo.toml"], None)?;
    mise_include.files.insert(
        mise.clone(),
        b"include = [\"ci.toml\"]\n\n[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&mise_include),
        Err(GenerateError::Detect(DetectError::Invalid { ref path, .. })) if *path == mise
    ));
    for other in [
        ".mise/config.toml",
        ".config/mise/conf.d/ci.toml",
        ".config/mise/mise.toml",
        "mise.ci.toml",
        ".tool-versions",
    ] {
        let file = RepoPath::new(other)?;
        let mut more = view(&["Cargo.toml"], None)?;
        more.tracked_paths.insert(file.clone());
        assert!(
            matches!(
                expected_files(&more),
                Err(GenerateError::Detect(DetectError::Invalid { ref path, .. })) if *path == file
            ),
            "{other}"
        );
    }
    Ok(())
}

#[test]
fn a_path_toolchain_stands_alone_and_a_nested_toolchain_file_is_an_error() -> TestResult {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mise = RepoPath::new("mise.toml")?;
    let nextest_only = b"[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
    let mut path_toolchain = view(&["Cargo.toml"], None)?;
    path_toolchain.tracked_paths.insert(file.clone());
    path_toolchain
        .files
        .insert(file, b"[toolchain]\npath = \"/opt/rust\"\n".to_vec());
    assert_eq!(
        expected_files(&path_toolchain),
        Err(GenerateError::Detect(DetectError::ToolchainConflict {
            file: "path /opt/rust".to_owned(),
            mise: "1.99.1".to_owned(),
        }))
    );
    path_toolchain
        .files
        .insert(mise.clone(), nextest_only.to_vec());
    assert!(!workflow(&path_toolchain)?.contains("rustup component add"));

    let nested = RepoPath::new("lab/rust-toolchain.toml")?;
    let mut two = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    two.files.insert(mise, nextest_only.to_vec());
    let text = workflow(&two)?;
    assert!(
        text.contains(
            "        working-directory: lab\n        run: rustup component add clippy rustfmt\n"
        ),
        "{text}"
    );
    two.tracked_paths.insert(nested.clone());
    assert_eq!(
        expected_files(&two),
        Err(GenerateError::Detect(DetectError::NestedToolchain {
            file: nested
        }))
    );
    let nested_mise = RepoPath::new("lab/mise.toml")?;
    let mut with_mise = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    with_mise.tracked_paths.insert(nested_mise.clone());
    assert_eq!(
        expected_files(&with_mise),
        Err(GenerateError::Detect(DetectError::NestedToolchain {
            file: nested_mise
        }))
    );
    let ancestor = RepoPath::new("projects/rust-toolchain.toml")?;
    let mut deep = view(&["Cargo.toml", "projects/lab/Cargo.toml"], None)?;
    deep.tracked_paths.insert(ancestor.clone());
    assert_eq!(
        expected_files(&deep),
        Err(GenerateError::Detect(DetectError::NestedToolchain {
            file: ancestor
        }))
    );
    Ok(())
}

#[test]
fn every_job_adds_the_components_and_targets_the_toolchain_file_declares() -> TestResult {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut declared = view(&["Cargo.toml"], Some(b"shape = \"lint-test\"\n"))?;
    declared.tracked_paths.insert(file.clone());
    declared.files.insert(
        file,
        b"[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"llvm-tools\"]\ntargets = [\"wasm32-unknown-unknown\"]\n"
            .to_vec(),
    );
    let text = workflow(&declared)?;
    let runs: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("run: rustup "))
        .collect();
    assert_eq!(
        runs,
        [
            "component add clippy llvm-tools rustfmt",
            "target add wasm32-unknown-unknown",
            "component add llvm-tools",
            "target add wasm32-unknown-unknown",
        ]
    );
    Ok(())
}
#[test]
fn two_roots_that_would_get_the_same_job_id_are_an_error() -> TestResult {
    assert_eq!(
        expected_files(&view(&["a-b/Cargo.toml", "a_b/Cargo.toml"], None)?),
        Err(GenerateError::Detect(DetectError::SameSlug {
            first: RepoPath::new("a-b/Cargo.toml")?,
            second: RepoPath::new("a_b/Cargo.toml")?,
        }))
    );
    Ok(())
}

#[test]
fn a_cargo_config_may_not_weaken_what_the_checks_run() -> TestResult {
    let config = RepoPath::new("lab/.cargo/config.toml")?;
    let rejected = [
        "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"--cap-lints\", \"allow\"]\n",
        "[env]\nCLIPPY_ARGS = { value = \"\", force = true }\n",
        "[env]\nANY_NAME = { value = \"x\", force = true }\n",
        "[env]\nRUSTFLAGS = \"--cap-lints allow\"\n",
        "[env]\nCARGO_BUILD_RUSTDOCFLAGS = \"\"\n",
    ];
    let accepted = [
        "[env]\nANY_NAME = \"x\"\n",
        "[env]\nANY_NAME = { value = \"x\", force = false }\n",
        "[build]\nrustflags = [\"-C\", \"debuginfo=0\"]\n",
    ];
    for (text, ok) in rejected
        .iter()
        .map(|text| (text, false))
        .chain(accepted.iter().map(|text| (text, true)))
    {
        let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
        repo.tracked_paths.insert(config.clone());
        repo.files.insert(config.clone(), text.as_bytes().to_vec());
        let result = expected_files(&repo);
        if ok {
            assert!(result.is_ok(), "{text}: {result:?}");
        } else {
            assert!(
                matches!(
                    &result,
                    Err(GenerateError::Detect(DetectError::Invalid { path, .. })) if *path == config
                ),
                "{text}: {result:?}"
            );
        }
    }
    Ok(())
}
