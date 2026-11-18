// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::{audit, own_units, unit, workspace_below_the_root};

const FILE_LIMIT: usize = 1 << 20;

#[test]
fn dep_info_budget_is_per_cargo_root() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, ".gitignore", "target/\ntarget-tools/\n");
    unit(
        &repo,
        "tools/Cargo.toml",
        "[package]\nname = \"tools\"\nedition = \"2024\"\n",
    );
    unit(&repo, "tools/Cargo.lock", "version = 4\n");
    unit(&repo, "tools/src/main.rs", "fn main() {}\n");
    unit(
        &repo,
        "tools/.cargo/config.toml",
        "[build]\ntarget-dir = \"../target-tools\"\n",
    );
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    unit(
        &repo,
        "target-tools/debug/deps/tools-0000000000000001.d",
        "out.d: src/main.rs\n",
    );
    unit(
        &repo,
        "target-tools/debug/.fingerprint/tools-0000000000000001/bin-tools",
        "",
    );
    unit(
        &repo,
        "target-tools/debug/deps/tools-0000000000000002.d",
        "out.d: src/main.rs\n",
    );
    unit(
        &repo,
        "target-tools/debug/.fingerprint/tools-0000000000000002/test-bin-tools",
        "",
    );
    repo.git(&["add", "--all"]);

    let mut index = 0;
    // 7 MiB in each root: 14 MiB in total, above the old 8 MiB aggregate.
    write_dep_info_files(&repo, "rust/target/debug", "app/src/lib.rs", 7, &mut index);
    write_dep_info_files(&repo, "target-tools/debug", "src/main.rs", 7, &mut index);
    let under_limit = audit(&repo, &["--write"]);
    assert!(under_limit.status.success(), "{under_limit:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let before = fs::read(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(before, b"shared/greeting.txt\tapp\n");

    // 9 MiB in one root is over its own limit even though the other holds 7.
    write_dep_info_files(&repo, "rust/target/debug", "app/src/lib.rs", 2, &mut index);
    let over_limit = audit(&repo, &["--write"]);
    assert_eq!(over_limit.status.code(), Some(1), "{over_limit:?}");
    let error = String::from_utf8_lossy(&over_limit.stderr);
    assert!(
        error.contains("target data of one Cargo root exceeds 8388608 bytes"),
        "{error}"
    );
    assert_eq!(
        fs::read(lock).unwrap_or_else(|e| unreachable!("{e}")),
        before
    );
}

pub(super) fn write_dep_info_files(
    repo: &crate::io::Repo,
    target: &str,
    source: &str,
    count: usize,
    index: &mut usize,
) {
    for _ in 0..count {
        let mut text = format!("out.d: {source}\n");
        text.push_str(&" ".repeat(FILE_LIMIT - text.len()));
        unit(
            repo,
            &format!("{target}/deps/extra-{:016x}.d", *index + 1),
            &text,
        );
        *index += 1;
    }
}
