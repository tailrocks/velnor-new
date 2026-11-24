// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{SHARED_TARGET, inputs_build_in, placed, unit};

fn workspace_with_member_outside_the_root_directory() -> Repo {
    let repo = Repo::new("audit-invocation-descendant-of-outside-member");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        "root/Cargo.toml",
        "[workspace]\nmembers = [\"../member\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "root/Cargo.lock", "version = 4\n");
    unit(
        &repo,
        "member/Cargo.toml",
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\nworkspace = \"../root\"\n",
    );
    unit(&repo, "member/src/lib.rs", "");
    unit(&repo, "member/src/deep/nested/mod.rs", "");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_failed_build_from_a_descendant_of_a_member_outside_the_root_directory_is_blamed_in_the_member_build()
 {
    for descendant in ["member/src", "member/src/deep/nested"] {
        let repo = workspace_with_member_outside_the_root_directory();
        let failed = inputs_build_in(
            &repo,
            descendant,
            &["cargo", "build", "--workspace", "--no-such-flag"],
        );
        assert!(!failed.status.success(), "{descendant}: {failed:?}");
        placed(&repo, &format!("{SHARED_TARGET}/debug"));
        unit(
            &repo,
            &format!("{SHARED_TARGET}/debug/build/member-f000000000000005/output"),
            "cargo:rerun-if-changed=src/lib.rs\n",
        );
        let stderr = String::from_utf8_lossy(
            &inputs_build_in(&repo, "member", &["cargo", "build", "--no-such-flag"]).stderr,
        )
        .into_owned();
        assert!(
            stderr.contains("An earlier inputs build failed"),
            "{descendant}: {stderr}"
        );
    }
}
