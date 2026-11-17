// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::view;

fn npm_config(request: &str, options: &str, vars: &str) -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    let config = RepoPath::from_static("mise.toml");
    let locked = RepoPath::from_static("mise.lock");
    let mise = String::from_utf8(repo.files.get(&config).ok_or("no config")?.clone())?;
    let lock = String::from_utf8(repo.files.get(&locked).ok_or("no lock")?.clone())?;
    repo.files.insert(
        config,
        format!("{mise}\"npm:fixture\" = {{ {request} }}\n{vars}").into_bytes(),
    );
    repo.files.insert(locked, format!("{lock}\n[[tools.\"npm:fixture\"]]\nversion = \"24.0.0\"\nbackend = \"npm:fixture\"\nspecifiers = [\"24\"]\n{options}\n[tools.\"npm:fixture\".\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/npm\"\n").into_bytes());
    velnor_rust::check_tools(&velnor_rust::detect(&repo)?)?;
    expected_files(&repo)?;
    Ok(())
}

#[test]
fn npm_lock_identity_canonicalizes_lists() -> TestResult {
    for key in ["allow_builds", "trust_policy_excludes", "allow_exotic_deps"] {
        npm_config(
            &format!("version = \"24\", {key} = [\"z\", \"a\", \"z\", \"\"]"),
            &format!("options = {{ {key} = '[\"a\", \"z\"]' }}"),
            "",
        )?;
        assert!(
            npm_config(
                &format!("version = \"24\", {key} = [\"different\"]"),
                &format!("options = {{ {key} = '[\"a\", \"z\"]' }}"),
                ""
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn npm_lock_identity_omits_default_options() -> TestResult {
    npm_config(
        "version = \"24\", allow_builds = false, trust_policy_excludes = [], allow_exotic_deps = false",
        "",
        "",
    )?;
    assert!(npm_config("version = \"24\", allow_builds = true", "", "").is_err());
    Ok(())
}

#[test]
fn templated_versions_use_the_rendered_lock_specifier() -> TestResult {
    npm_config("version = \"{{ vars.v }}\"", "", "[vars]\nv = \"24\"\n")?;
    npm_config(
        "version = \"{{ vars.v | trim }}\"",
        "",
        "[vars]\nv = \" 24 \"\n",
    )?;
    npm_config(
        "version = \"{{ vars.alias }}\"",
        "",
        "[vars]\nalias = \"{{ vars.v }}\"\nv = \"24\"\n",
    )?;
    assert!(npm_config("version = \"{{ vars.v }}\"", "", "[vars]\nv = \"25\"\n").is_err());
    assert!(
        npm_config(
            "version = \"{{ vars.missing }}\"",
            "",
            "[vars]\nv = \"24\"\n"
        )
        .is_err()
    );
    assert!(
        npm_config(
            "version = \"{{ vars.v }}\"",
            "",
            "[vars]\nv = \"{{ vars.v }}\"\n"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn linux_aarch64_uses_arm_policy_tools_and_prebuilts() -> TestResult {
    let platform = velnor_model::MisePlatform::runner("linux-aarch64").ok_or("no platform")?;
    assert_eq!(platform.lock_key(), "linux-arm64");
    assert!(platform.matches("linux/arm64"));
    assert!(!platform.matches("linux/x64"));
    assert!(
        crate::mise_platform::generate("linux/arm64", "linux-aarch64", true, true)?
            .contains("cargo-deny check")
    );
    assert!(
        crate::mise_platform::generate("linux/x64", "linux-aarch64", true, true)
            .is_err_and(|error| error.contains("not pinned"))
    );
    assert!(
        crate::mise_platform::generate("linux/arm64", "linux-aarch64", false, false)
            .is_err_and(|error| error.contains("prebuilt"))
    );
    Ok(())
}
