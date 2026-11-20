// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::io_plan::plan_repo;
use super::io_plan_policy::policy_lines;

#[test]
fn plan_runs_deny_for_a_deny_config_change_alone() {
    let repo = plan_repo("policy-deny");
    repo.write("deny.toml", "");
    repo.write(
        "mise.toml",
        "[tools]\nrust = \"1.99.1\"\ncargo-deny = \"0.20.2\"\n",
    );
    repo.write(
        ".velnor/rust-ci.toml",
        "[[inputs]]\nglob = \"deny.toml\"\npackages = [\"a\"]\n",
    );
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["deny.toml"]),
        "rust_deny=run rust_machete=skip "
    );
}

#[test]
fn plan_runs_deny_for_a_cargo_config_change_that_selects_only_a_package() {
    let repo = plan_repo("policy-cargo-config");
    repo.write("deny.toml", "");
    repo.write(
        "mise.toml",
        "[tools]\nrust = \"1.99.1\"\ncargo-deny = \"0.20.2\"\n",
    );
    repo.write(".cargo/config.toml", "[net]\nretry = 2\n");
    repo.write(
        ".velnor/rust-ci.toml",
        "[[inputs]]\nglob = \".cargo/config.toml\"\npackages = [\"a\"]\n",
    );
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &[".cargo/config.toml"]),
        "rust_deny=run rust_machete=skip "
    );
}
