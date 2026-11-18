// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::fixture::{files, refusal};

#[test]
fn a_cargo_feature_that_is_stable_does_not_need_nightly() {
    let mut stable = files();
    stable.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "cargo-features = [\"edition2024\"]\n[package]\nname = \"app\"\n".to_owned(),
    );
    assert_eq!(refusal(&stable), None);
}
