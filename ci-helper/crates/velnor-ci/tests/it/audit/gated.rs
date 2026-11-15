// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{audit, own_units_in, workspace_below_the_root};
use crate::io::Repo;

const DEBUG: &str = "rust/target/debug";
const TRIPLE: &str = "rust/target/x86_64-unknown-linux-gnu/debug";

fn build_dependency_under(repo: &Repo, platform: &str) {
    repo.write(
        "rust/app/Cargo.toml",
        &format!(
            "[package]\nname = \"app\"\nedition = \"2024\"\n\n[target.'{platform}'.build-dependencies]\nlib = {{ path = \"../lib\" }}\n"
        ),
    );
    repo.git(&["add", "--all"]);
    own_units_in(repo, DEBUG, TRIPLE, &[]);
}

#[test]
fn a_member_that_is_a_build_dependency_only_under_an_inactive_cfg_needs_no_host_units() {
    for platform in [
        "cfg(target_os = \"no-such-os\")",
        "cfg(all(unix, not(unix)))",
        "x86_64-no-such-vendor-none",
    ] {
        let repo = workspace_below_the_root();
        build_dependency_under(&repo, platform);
        let output = audit(&repo, &["--write"]);
        assert!(output.status.success(), "{platform}: {output:?}");
    }
}

#[test]
fn a_member_that_is_a_build_dependency_under_an_active_cfg_still_needs_its_host_units() {
    for platform in [
        "cfg(not(target_os = \"no-such-os\"))",
        "cfg(any(unix, windows))",
    ] {
        let repo = workspace_below_the_root();
        build_dependency_under(&repo, platform);
        let output = audit(&repo, &["--write"]);
        assert_eq!(output.status.code(), Some(1), "{platform}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("for lib (rust/lib/Cargo.toml)"),
            "{platform}: {output:?}"
        );
    }
}
