// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::prepare::{Declared, Run, Setup, Toolchain, prepare_declared, prepare_in, scratch};

const BEFORE: &str = "cargo-x86_64-unknown-linux-gnu (installed)\nrustc-x86_64-unknown-linux-gnu (installed)\nclippy-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu (installed)\nrust-docs-x86_64-unknown-linux-gnu\nrust-src";
const AFTER: &str = "cargo-x86_64-unknown-linux-gnu (installed)\nrustc-x86_64-unknown-linux-gnu (installed)\nclippy-x86_64-unknown-linux-gnu (installed)\nrust-std-x86_64-unknown-linux-gnu (installed)\nrust-docs-x86_64-unknown-linux-gnu (installed)\nrust-src (installed)";
const REPORT: &str = "{\"rust\":[{\"version\":\"1.99.1\",\"installed\":true}],\"aqua:nextest-rs/nextest/cargo-nextest\":[{\"version\":\"0.9.151\",\"installed\":true}]}";
const HOST_ONLY: &str = "x86_64-unknown-linux-gnu";
const WITH_WASM: &str = "x86_64-unknown-linux-gnu\nwasm32-unknown-unknown";

fn rustup(after: &str, targets_after: &str) -> String {
    format!(
        "d=$(dirname \"$0\")\necho \"$@\" >> \"$d/calls\"\ncase \"$1 $2\" in\n\"component add\"|\"target add\") touch \"$d/done-$1\";;\n\"component list\") if [ -f \"$d/done-component\" ]; then cat <<'EOF'\n{after}\nEOF\nelse cat <<'EOF'\n{BEFORE}\nEOF\nfi;;\n\"target list\") if [ -f \"$d/done-target\" ]; then cat <<'EOF'\n{targets_after}\nEOF\nelse echo {HOST_ONLY}; fi;;\nesac\n"
    )
}

fn run(name: &str, file: &str, fake: &str) -> Run {
    prepare_declared(
        name,
        REPORT,
        Some(&Declared {
            file,
            components: "",
            targets: "",
            rustup: Some(fake),
        }),
    )
}

fn calls(run: &Run) -> String {
    fs::read_to_string(run.bin.join("calls")).unwrap_or_default()
}

#[test]
fn prepare_installs_the_missing_components_and_targets_then_passes() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\ntargets = [\"wasm32-unknown-unknown\"]\n";
    let run = run("install", file, &rustup(AFTER, WITH_WASM));
    assert!(run.output.status.success(), "{:?}", run.output);
    let calls = calls(&run);
    assert!(
        calls.contains("component add rust-docs rust-src\n"),
        "{calls}"
    );
    assert!(
        calls.contains("target add wasm32-unknown-unknown\n"),
        "{calls}"
    );
}

#[test]
fn prepare_names_a_component_rustup_cannot_install() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"rust-src\", \"rust-docs\"]\n";
    let stuck = AFTER.replace("rust-src (installed)", "rust-src");
    let run = run("stuck", file, &rustup(&stuck, HOST_ONLY));
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.ends_with("component rust-src\n"), "{stderr}");
    assert!(calls(&run).contains("component add rust-docs rust-src\n"));
}

#[test]
fn prepare_installs_nothing_when_nothing_is_missing() {
    let file =
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n";
    let run = run("nothing", file, &rustup(AFTER, WITH_WASM));
    assert!(run.output.status.success(), "{:?}", run.output);
    let calls = calls(&run);
    assert!(!calls.contains(" add "), "{calls}");
}

#[test]
fn prepare_records_each_install_it_ran_for_the_report() {
    let file = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\ntargets = [\"wasm32-unknown-unknown\"]\n";
    let run = run("record", file, &rustup(AFTER, WITH_WASM));
    assert!(run.output.status.success(), "{:?}", run.output);
    let recorded = fs::read_to_string(
        run.bin
            .join("..")
            .join(velnor_ci::waste_evidence::TOOLCHAIN_INSTALLS),
    )
    .unwrap_or_default();
    assert_eq!(
        recorded,
        "component rust-docs\ncomponent rust-src\ntarget wasm32-unknown-unknown\n"
    );
}

#[test]
fn prepare_records_nothing_when_nothing_is_installed() {
    let file =
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n";
    let run = run("record-none", file, &rustup(AFTER, WITH_WASM));
    assert!(run.output.status.success(), "{:?}", run.output);
    assert!(
        !run.bin
            .join("..")
            .join(velnor_ci::waste_evidence::TOOLCHAIN_INSTALLS)
            .exists()
    );
}

/// Runs `prepare` with RUSTUP_HOME at a scratch directory whose `toolchains` holds `installed`.
fn run_with_toolchains(name: &str, file: &str, installed: &[&str]) -> Run {
    let dir = scratch(name);
    let home = dir.join("rustup-home");
    for toolchain in installed {
        fs::create_dir_all(home.join("toolchains").join(toolchain))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut setup = Setup::new(REPORT.to_owned());
    setup.toolchain = Some(Toolchain {
        file: file.to_owned(),
        components: String::new(),
        targets: String::new(),
        rustup: Some(rustup(AFTER, WITH_WASM)),
    });
    setup.rustup_home = Some(home);
    prepare_in(&dir, &setup)
}

fn recorded_installs(run: &Run) -> String {
    fs::read_to_string(
        run.bin
            .join("..")
            .join(velnor_ci::waste_evidence::TOOLCHAIN_INSTALLS),
    )
    .unwrap_or_default()
}

const CLEAN_TOOLCHAIN: &str =
    "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n";

#[test]
fn prepare_records_a_declared_channel_that_is_not_installed_before_the_job() {
    let run = run_with_toolchains("channel-missing", CLEAN_TOOLCHAIN, &[]);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(recorded_installs(&run), "toolchain 1.99.1\n");
}

#[test]
fn prepare_records_nothing_for_a_declared_channel_already_on_disk() {
    let host = "x86_64-unknown-linux-gnu";
    let run = run_with_toolchains(
        "channel-present",
        CLEAN_TOOLCHAIN,
        &[&format!("1.99.1-{host}")],
    );
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(recorded_installs(&run), "");
}

#[test]
fn prepare_does_not_mistake_a_longer_version_for_the_declared_channel() {
    let run = run_with_toolchains(
        "channel-prefix",
        CLEAN_TOOLCHAIN,
        &["1.99.10-x86_64-unknown-linux-gnu"],
    );
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(recorded_installs(&run), "toolchain 1.99.1\n");
}
