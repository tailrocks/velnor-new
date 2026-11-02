// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::toolfiles::lockfile::subject_for_install_spec;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn install_spec_subject_accepts_only_the_canonical_rust_profile_option() {
    let catalog = ToolCatalog::pinned();
    let rust = catalog.version(PinnedTool::Rust);
    let shellcheck = catalog.version(PinnedTool::Shellcheck);
    let plain = subject_for_install_spec(&format!("rust@{rust}"), &catalog);
    assert!(plain.is_some(), "the bare Rust spec lost its subject");
    let minimal = subject_for_install_spec(&format!("rust[profile=minimal]@{rust}"), &catalog);
    assert_eq!(minimal, plain);
    let rejected = [
        format!("rust[profile=complete]@{rust}"),
        format!("rust[profile=default]@{rust}"),
        format!("rust[targets=wasm32-unknown-unknown]@{rust}"),
        format!("rust[profile=minimal,components=rust-docs]@{rust}"),
        format!("rust[profile=minimal@{rust}"),
        format!("shellcheck[profile=minimal]@{shellcheck}"),
        "rust[profile=minimal]@0.0.1".to_owned(),
    ];
    for spec in rejected {
        assert_eq!(
            subject_for_install_spec(&spec, &catalog),
            None,
            "{spec} was accepted"
        );
    }
}
