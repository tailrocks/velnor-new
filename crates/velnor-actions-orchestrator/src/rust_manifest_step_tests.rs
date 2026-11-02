// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use super::{ToolchainPhase, toolchain_script};

const REAL_COMPONENTS: &str = "cargo-x86_64-unknown-linux-gnu\nclippy-preview-x86_64-unknown-linux-gnu\nrust-std-x86_64-unknown-linux-gnu\nrustc-x86_64-unknown-linux-gnu\nrustfmt-preview-x86_64-unknown-linux-gnu\n";

const DEAD_PROXY: &str = "http://127.0.0.1:9";

#[test]
fn the_upgrades_verifier_checks_the_default_home_file_by_file_without_pins() {
    let version = ToolCatalog::pinned().version(PinnedTool::Rust).to_owned();
    let home = tempfile::tempdir().expect("tempdir");
    let rustup = home.path().join("rustup");
    let lib = rustup
        .join("toolchains")
        .join(format!("{version}-x86_64-unknown-linux-gnu"))
        .join("lib/rustlib");
    fs::create_dir_all(&lib).expect("toolchain dir");
    fs::write(lib.join("components"), REAL_COMPONENTS).expect("components");
    let script = toolchain_script(
        "${VELNOR_TOOL_RUST}",
        ToolchainPhase::Installed,
        RUST_CHANNEL_MANIFESTS,
    );
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", "/usr/bin")
        .env("HOME", home.path())
        .env("RUNNER_TEMP", home.path().join("runner"))
        .env("RUSTUP_HOME", &rustup)
        .env("CARGO_HOME", home.path().join("cargo"))
        .env("VELNOR_TOOL_RUST", &version)
        .env("HTTPS_PROXY", DEAD_PROXY)
        .env("https_proxy", DEAD_PROXY)
        .env("ALL_PROXY", DEAD_PROXY)
        .env("all_proxy", DEAD_PROXY)
        .output()
        .expect("spawn sh");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("rust_tree_digest_mismatch")
            && !combined.contains("pin_mismatch")
            && !combined.contains("matches pinned")
            && stderr.contains("curl: (7)"),
        "{output:?}"
    );
}
