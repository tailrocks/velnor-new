// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-ci-prepare-{}-{name}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn reported(rust: &str, nextest: Option<&str>) -> String {
    let entry = |version: &str| {
        format!(
            "[{{\"version\":\"{version}\",\"requested_version\":\"{version}\",\"installed\":true,\"active\":true}}]"
        )
    };
    let mut tools = vec![format!("\"rust\":{}", entry(rust))];
    if let Some(version) = nextest {
        tools.push(format!(
            "\"aqua:nextest-rs/nextest/cargo-nextest\":{}",
            entry(version)
        ));
    }
    format!("{{{}}}", tools.join(","))
}

pub(crate) struct Run {
    pub bin: PathBuf,
    pub output: Output,
    env_file: String,
    outputs: String,
}

pub(crate) struct Declared<'a> {
    pub file: &'a str,
    pub components: &'a str,
    pub targets: &'a str,
    pub rustup: Option<&'a str>,
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn prepare(name: &str, json: &str) -> Run {
    prepare_declared(name, json, None)
}

pub(crate) fn prepare_declared(name: &str, json: &str, declared: Option<&Declared<'_>>) -> Run {
    prepare_with(name, MISE, &format!("cat <<'EOF'\n{json}\nEOF\n"), declared)
}

fn prepare_with(
    name: &str,
    mise_toml: &str,
    mise_script: &str,
    declared: Option<&Declared<'_>>,
) -> Run {
    let dir = scratch(name);
    let checkout = dir.join("checkout");
    fs::create_dir_all(&checkout).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(checkout.join("mise.toml"), mise_toml).unwrap_or_else(|e| unreachable!("{e}"));
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let mise = bin.join("mise");
    fs::write(&mise, format!("#!/bin/sh\n{mise_script}")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    if let Some(declared) = declared {
        fs::write(checkout.join("rust-toolchain.toml"), declared.file)
            .unwrap_or_else(|e| unreachable!("{e}"));
        script(
            &bin.join("rustc"),
            "echo 'host: x86_64-unknown-linux-gnu'\n",
        );
        let fixed = format!(
            "if [ \"$1\" = component ]; then cat <<'EOF'\n{}\nEOF\nelse cat <<'EOF'\n{}\nEOF\nfi\n",
            declared.components, declared.targets
        );
        script(&bin.join("rustup"), declared.rustup.unwrap_or(&fixed));
    }
    let (env_file, outputs) = (dir.join("env"), dir.join("output"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("prepare")
        .current_dir(&checkout)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("GITHUB_ENV", &env_file)
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let read = |file: &Path| fs::read_to_string(file).unwrap_or_default();
    Run {
        bin,
        output,
        env_file: read(&env_file),
        outputs: read(&outputs),
    }
}

#[test]
fn prepare_sets_the_build_profile_and_an_empty_compat_when_every_tool_is_at_its_pin() {
    let run = prepare("ok", &reported("1.99.1", Some("0.9.151")));
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(
        run.env_file,
        "RUSTUP_AUTO_INSTALL=0\nCARGO_INCREMENTAL=0\nCARGO_TERM_COLOR=always\nSOURCE_DATE_EPOCH=0\n"
    );
    assert_eq!(run.outputs, "compat=\n");
}

#[test]
fn prepare_names_the_tool_that_is_not_at_its_pinned_version_and_writes_nothing() {
    for (name, json, tool) in [
        (
            "wrong",
            reported("1.98.0", Some("0.9.151")),
            "rust: pinned 1.99.1, mise reports 1.98.0",
        ),
        (
            "absent",
            reported("1.99.1", None),
            "aqua:nextest-rs/nextest/cargo-nextest: pinned 0.9.151, mise reports nothing installed",
        ),
    ] {
        let run = prepare(name, &json);
        assert_eq!(run.output.status.code(), Some(1), "{name}");
        assert!(
            String::from_utf8_lossy(&run.output.stderr).contains(tool),
            "{name}: {:?}",
            run.output
        );
        assert_eq!(
            (run.env_file.as_str(), run.outputs.as_str()),
            ("", ""),
            "{name}"
        );
    }
}

#[test]
fn restored_exits_zero_and_writes_nothing() {
    let dir = scratch("restored");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("restored")
        .current_dir(&dir)
        .env("GITHUB_ENV", dir.join("env"))
        .env("GITHUB_OUTPUT", dir.join("output"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(!dir.join("env").exists() && !dir.join("output").exists());
}

const LISTING: &str = "cargo-x86_64-unknown-linux-gnu (installed)\nclippy-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu (installed)\nrust-std-wasm32-unknown-unknown\nrustc-x86_64-unknown-linux-gnu (installed)\nrustfmt-x86_64-unknown-linux-gnu (installed)\nrust-docs-x86_64-unknown-linux-gnu\nrust-src\nrustc-dev-x86_64-unknown-linux-gnu\nrustc-dev-aarch64-apple-darwin";

fn declared(file: &'static str) -> Declared<'static> {
    Declared {
        file,
        components: LISTING,
        targets: "x86_64-unknown-linux-gnu",
        rustup: None,
    }
}

#[test]
fn prepare_names_every_component_a_complete_profile_implies_that_is_not_installed() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\n";
    let run = prepare_declared(
        "complete",
        &reported("1.99.1", Some("0.9.151")),
        Some(&declared(file)),
    );
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("component rust-docs, component rust-src, component rustc-dev"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("aarch64") && !stderr.contains("clippy"),
        "{stderr}"
    );
    assert_eq!((run.env_file.as_str(), run.outputs.as_str()), ("", ""));
}

#[test]
fn prepare_passes_a_complete_profile_whose_components_are_all_installed() {
    let all = LISTING
        .lines()
        .map(|line| {
            if line.contains("(installed)") {
                line.to_owned()
            } else {
                format!("{line} (installed)")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\n";
    let run = prepare_declared(
        "complete-ok",
        &reported("1.99.1", Some("0.9.151")),
        Some(&Declared {
            file,
            components: &all,
            targets: "x86_64-unknown-linux-gnu",
            rustup: None,
        }),
    );
    assert!(run.output.status.success(), "{:?}", run.output);
}

fn assert_lacks(name: &str, file: &'static str, missing: &str) {
    let run = prepare_declared(
        name,
        &reported("1.99.1", Some("0.9.151")),
        Some(&declared(file)),
    );
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.ends_with(&format!("{missing}\n")), "{stderr}");
}

#[test]
fn prepare_checks_the_components_the_default_profile_implies() {
    assert_lacks(
        "default",
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"default\"\n",
        "component rust-docs",
    );
}

#[test]
fn prepare_checks_the_listed_components() {
    assert_lacks(
        "listed",
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"rust-src\"]\n",
        "component rust-src",
    );
}

#[test]
fn prepare_checks_the_listed_targets() {
    assert_lacks(
        "target",
        "[toolchain]\nchannel = \"1.99.1\"\ntargets = [\"wasm32-unknown-unknown\"]\n",
        "target wasm32-unknown-unknown",
    );
}

#[test]
fn prepare_matches_a_legacy_preview_component_name_to_its_renamed_component() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"llvm-tools-preview\", \"clippy-preview\"]\n";
    let listing = "clippy-x86_64-unknown-linux-gnu (installed)\nllvm-tools-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu (installed)";
    let run = prepare_declared(
        "preview",
        &reported("1.99.1", Some("0.9.151")),
        Some(&Declared {
            file,
            components: listing,
            targets: "x86_64-unknown-linux-gnu",
            rustup: None,
        }),
    );
    assert!(run.output.status.success(), "{:?}", run.output);
}

#[test]
fn prepare_passes_a_minimal_profile_with_nothing_more_declared() {
    let file =
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n";
    let run = prepare_declared(
        "minimal",
        &reported("1.99.1", Some("0.9.151")),
        Some(&declared(file)),
    );
    assert!(run.output.status.success(), "{:?}", run.output);
}

#[test]
fn prepare_does_not_run_rustup_for_a_path_toolchain() {
    let file = "[toolchain]\npath = \"/opt/custom-rust\"\n";
    let run = prepare_declared(
        "path",
        &reported("1.99.1", Some("0.9.151")),
        Some(&Declared {
            file,
            components: "",
            targets: "",
            rustup: Some("echo rustup must not run >&2\nexit 1\n"),
        }),
    );
    assert!(run.output.status.success(), "{:?}", run.output);
}

#[test]
fn prepare_does_not_need_mise_when_no_tool_is_pinned() {
    let run = prepare_with(
        "empty",
        "[tools]\n",
        "echo mise must not run >&2\nexit 1\n",
        None,
    );
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(run.outputs, "compat=\n");
}

#[test]
fn prepare_still_verifies_the_declared_toolchain_when_no_tool_is_pinned() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\n";
    let run = prepare_with(
        "empty-declared",
        "[tools]\n",
        "echo mise must not run >&2\nexit 1\n",
        Some(&declared(file)),
    );
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("component rust-docs"), "{stderr}");
}
