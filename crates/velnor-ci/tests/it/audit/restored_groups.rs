// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, UNIX_EPOCH};

use super::groups::{audit, both_roots};
use super::unit;
use velnor_ci::inputs::restored_units_file;

const TARGET: &str = "target-velnor/tools/debug";

fn make_stale(repo: &crate::io::Repo, package: &str, hash: &str) {
    unit(repo, &format!("tools/{package}/old.txt"), "");
    unit(
        repo,
        &format!("{TARGET}/deps/{package}-{hash}.d"),
        "out.d: src/lib.rs old.txt\n",
    );
    unit(
        repo,
        &format!("{TARGET}/.fingerprint/{package}-{hash}/lib-{package}"),
        "",
    );
    repo.set_mtime(
        &format!("{TARGET}/deps/{package}-{hash}.d"),
        UNIX_EPOCH + Duration::from_secs(3_000_000_001),
    );
    repo.set_mtime(
        &format!("tools/{package}/old.txt"),
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
}

fn head(repo: &crate::io::Repo) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn each_root_of_a_shared_group_trusts_its_own_manifest_in_the_group_directory() {
    let repo = both_roots();
    make_stale(&repo, "a", "f000000000000011");
    make_stale(&repo, "b", "f000000000000013");
    repo.commit();

    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("found stale dep-info"),
        "{refused:?}"
    );

    let header = format!("velnor-restored-units 4 {}", head(&repo));
    for (prefix, hash) in [
        ("tools/a/", "f000000000000011"),
        ("tools/b/", "f000000000000013"),
    ] {
        unit(
            &repo,
            &format!("target-velnor/tools/{}", restored_units_file(prefix)),
            &format!("{header}\nrestored {hash}\n"),
        );
    }
    let trusted = audit(&repo, &["--write"]);
    assert_eq!(trusted.status.code(), Some(0), "{trusted:?}");
}
