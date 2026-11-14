// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use crate::io_plan::{plan_command, selected};

#[test]
fn plan_follows_the_edges_of_unowned_packages_and_explains_a_package_it_cannot_read() {
    let repo = Repo::new("unowned");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(".velnor/rust-ci.toml", "exclude = [\"vendor\"]\n");
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\", \"c\", \"d\", \"e\"]\nexclude = [\"vendor\"]\nresolver = \"3\"\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    for (member, dependencies) in [
        ("a", "mid = { path = \"../vendor/mid\" }\n"),
        ("b", "remote = { path = \"../vendor/remote\" }\n"),
        ("c", ""),
        ("d", ""),
        ("e", ""),
    ] {
        repo.write(
            &format!("{member}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{member}\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}"
            ),
        );
        repo.write(&format!("{member}/src/lib.rs"), "");
    }
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\n",
    );
    repo.write(
        "vendor/remote/Cargo.toml",
        "[package]\nname = \"remote\"\nedition = \"2024\"\n\n[dependencies]\nfar = { git = \"https://example.com/far\" }\n",
    );
    for package in ["mid", "remote"] {
        repo.write(&format!("vendor/{package}/src/lib.rs"), "");
    }
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", "c/src/lib.rs"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  b depends-on-unread:remote\n  c changed:c/src/lib.rs\nunread package: vendor/remote/Cargo.toml\n  it has the git dependency far, so Velnor cannot follow its dependencies and a change to any package selects every package that depends on it\n"
    );
}
