// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::prepare::{Declared, Run, prepare_declared};

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
