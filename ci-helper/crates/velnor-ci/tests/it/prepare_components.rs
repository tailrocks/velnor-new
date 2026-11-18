// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare::{
    Declared, Run, Setup, Toolchain, digest, prepare, prepare_declared, reported,
};

fn prepare_with(
    name: &str,
    mise_toml: &str,
    mise_script: &str,
    declared: Option<&Declared<'_>>,
) -> Run {
    let mut setup = Setup::new(String::new());
    mise_toml.clone_into(&mut setup.mise_toml);
    setup.mise_script = Some(mise_script.to_owned());
    setup.toolchain = declared.map(|declared| Toolchain {
        file: declared.file.to_owned(),
        components: declared.components.to_owned(),
        targets: declared.targets.to_owned(),
        rustup: declared.rustup.map(str::to_owned),
    });
    prepare(name, &setup)
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
    let listing = "cargo-x86_64-unknown-linux-gnu (installed)\nrustc-x86_64-unknown-linux-gnu (installed)\nclippy-x86_64-unknown-linux-gnu (installed)\nllvm-tools-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu (installed)";
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
    assert_eq!(digest(&run).len(), 16, "{}", run.outputs);
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

#[test]
fn prepare_names_a_base_component_missing_under_a_minimal_or_default_profile() {
    let listing = "cargo-x86_64-unknown-linux-gnu\nclippy-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu\nrustc-x86_64-unknown-linux-gnu (installed)\nrustfmt-x86_64-unknown-linux-gnu (installed)\nrust-docs-x86_64-unknown-linux-gnu (installed)";
    for (name, file) in [
        (
            "base-minimal",
            "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\n",
        ),
        (
            "base-default",
            "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"default\"\n",
        ),
    ] {
        let run = prepare_declared(
            name,
            &reported("1.99.1", Some("0.9.151")),
            Some(&Declared {
                file,
                components: listing,
                targets: "x86_64-unknown-linux-gnu",
                rustup: None,
            }),
        );
        let stderr = String::from_utf8_lossy(&run.output.stderr);
        assert_eq!(run.output.status.code(), Some(1), "{name}: {stderr}");
        assert!(
            stderr.ends_with("component cargo, component rust-std\n"),
            "{name}: {stderr}"
        );
    }
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
        let run = prepare(name, &Setup::new(json));
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
