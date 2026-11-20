// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::delta::success;
use crate::delta_freshness::{Fixture, assert_units};

/// A delta applied over changed workspace sources removes the fingerprints of every path package,
/// but a registry package is immutable for its locked version and keeps its fingerprint: only
/// workspace crates recompile.
#[test]
fn changed_workspace_source_keeps_registry_package_fingerprints() {
    let fixture = Fixture::unbuilt("delta-registry-fingerprints", false);
    fixture.repo.write(
        ".cargo/config.toml",
        "[build]\njobs=2\n[source.crates-io]\nreplace-with='vendor'\n[source.vendor]\ndirectory='vendor'\n",
    );
    fixture.repo.write(
        "Cargo.toml",
        "[workspace]\nmembers=['app','peer','spare']\nexclude=['local','stable','vendor']\nresolver='3'\n",
    );
    fixture.repo.write(
        ".velnor/rust-ci.toml",
        "exclude=['local','stable','vendor/regdep']\n",
    );
    fixture.repo.write(
        "vendor/regdep/Cargo.toml",
        "[package]\nname='regdep'\nversion='1.0.0'\nedition='2024'\n",
    );
    fixture
        .repo
        .write("vendor/regdep/src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    fixture
        .repo
        .write("vendor/regdep/.cargo-checksum.json", "{\"files\":{}}\n");
    fixture.repo.write(
        "app/Cargo.toml",
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nlocal={path='../local'}\nstable={path='../stable'}\nregdep='1'\n",
    );
    let mut fixture = fixture;
    fixture.build_baseline(false);
    let lock = fs::read_to_string(fixture.directory.join("Cargo.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(lock.contains("name = \"regdep\""), "{lock}");
    assert!(lock.contains("source = \"registry+"), "{lock}");
    success(&fixture.helper(&["inputs", "save"]).0);
    fixture.delta("stage");
    fixture.change("app/src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["app", "peer", "spare", "local", "stable"],
        &["regdep"],
    );
}
