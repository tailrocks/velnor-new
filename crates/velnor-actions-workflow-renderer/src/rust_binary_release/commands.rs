//! Exact Mise/Cargo argv validation for generic binary releases.

use velnor_actions_contract::RustBinaryReleaseConfig;

use crate::RenderError;

use super::RustBinaryReleaseCommands;

pub(super) fn validate_commands(
    config: &RustBinaryReleaseConfig,
    commands: &RustBinaryReleaseCommands,
) -> Result<(), RenderError> {
    if !is_exact_version(&commands.rust_version) || !is_exact_version(&commands.gh_version) {
        return Err(RenderError::BadCommand(
            "bad_binary_release_tool_version".to_owned(),
        ));
    }
    let rust_spec = format!("rust@{}", commands.rust_version);
    let gh_spec = format!("gh@{}", commands.gh_version);
    let expected_install_rust = mise_install(&rust_spec);
    let expected_install_gh = mise_install(&gh_spec);
    let expected_metadata = mise_exec(
        &rust_spec,
        "cargo",
        &[
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--manifest-path",
            &config.manifest_path,
        ],
    );
    let expected_rustc = mise_exec(&rust_spec, "rustc", &["-vV"]);
    let expected_linux = build_argv(config, &rust_spec, "x86_64-unknown-linux-gnu");
    let expected_macos = build_argv(config, &rust_spec, "aarch64-apple-darwin");
    let expected_gh = mise_exec(&gh_spec, "gh", &[]);
    if commands.install_rust != expected_install_rust
        || commands.install_gh != expected_install_gh
        || commands.metadata != expected_metadata
        || commands.rustc_version != expected_rustc
        || commands.build_linux_x86_64 != expected_linux
        || commands.build_macos_arm64 != expected_macos
        || commands.gh_prefix != expected_gh
    {
        return Err(RenderError::BadCommand(
            "binary_release_command_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn mise_install(tool_spec: &str) -> Vec<String> {
    vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
        tool_spec.to_owned(),
    ]
}

fn mise_exec(tool_spec: &str, program: &str, args: &[&str]) -> Vec<String> {
    let mut command = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        tool_spec.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    command.extend(args.iter().map(ToString::to_string));
    command
}

fn build_argv(config: &RustBinaryReleaseConfig, rust_spec: &str, target: &str) -> Vec<String> {
    mise_exec(
        rust_spec,
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--message-format=json",
            "--manifest-path",
            &config.manifest_path,
            "--package",
            &config.package,
            "--bin",
            config.binary_name(),
            "--target",
            target,
        ],
    )
}

fn is_exact_version(value: &str) -> bool {
    let mut parts = value.split('.');
    parts.next().is_some_and(decimal)
        && parts.next().is_some_and(decimal)
        && parts.next().is_some_and(decimal)
        && parts.next().is_none()
}

fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}
