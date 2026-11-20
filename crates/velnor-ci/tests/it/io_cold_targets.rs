// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};
use velnor_test_support::real_cargo;

use velnor_ci::io::{break_time_ties_with, cold_on_failure, every_root_target};

use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn stale_target(repo: &Repo, relative: &str) {
    repo.write_unstaged(
        &format!("{relative}/debug/.fingerprint/stale/lib-stale"),
        "x",
    );
}

fn configured_roots() -> Repo {
    let repo = Repo::new("cold-configured");
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (".gitignore", "/cache dir/\n**/target/\n"),
        (
            "app/Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("app/src/lib.rs", "pub fn app() {}\n"),
        (
            "app/.cargo/config.toml",
            "[build]\ntarget-dir='../cache dir'\n",
        ),
        (
            "tools/Cargo.toml",
            "[package]\nname='tools'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("tools/src/lib.rs", "pub fn tools() {}\n"),
    ]);
    for root in ["app", "tools"] {
        let output = Command::new(real_cargo())
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.root().join(root))
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
    }
    repo.git(&["add", "--all"]);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    stale_target(&repo, "cache dir");
    stale_target(&repo, "tools/target");
    repo
}

fn assert_configured_checkout(repo: &Repo) {
    for (path, expected) in [
        ("app/src/lib.rs", "pub fn app() {}\n"),
        ("tools/src/lib.rs", "pub fn tools() {}\n"),
        (
            "app/.cargo/config.toml",
            "[build]\ntarget-dir='../cache dir'\n",
        ),
        (
            "app/Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
    ] {
        assert_eq!(
            std::fs::read_to_string(repo.root().join(path)).unwrap_or_default(),
            expected
        );
    }
    repo.git(&[
        "ls-files",
        "--error-unmatch",
        "app/src/lib.rs",
        "tools/src/lib.rs",
    ]);
}

#[test]
fn rounding_after_restore_removes_configured_and_default_caches() {
    let repo = configured_roots();
    let tied = UNIX_EPOCH + Duration::new(1_700_000_000, 193_657_627);
    for path in ["app/src/lib.rs", "tools/src/lib.rs"] {
        repo.set_mtime(path, tied);
    }
    let report = cold_on_failure(
        &every_root_target(repo.root()).unwrap_or_else(|e| unreachable!("{e}")),
        "restored",
        || break_time_ties_with(repo.root(), &|_, _| Ok(())),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        report.contains("still share a modification time"),
        "{report}"
    );
    assert!(report.contains("building cold"), "{report}");
    assert!(!repo.root().join("cache dir").exists());
    assert!(!repo.root().join("tools/target").exists());
    assert_configured_checkout(&repo);
}

#[test]
fn restored_refuses_unreadable_recorded_targets_without_claiming_a_cold_build() {
    let repo = configured_roots();
    repo.write_unstaged(".velnor/rust-metadata.json", "invalid\n");
    let output = helper(&repo, &["restored"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("rust-metadata.json"),
        "{output:?}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("building cold"));
    assert!(repo.root().join("cache dir").is_dir());
    assert!(repo.root().join("tools/target").is_dir());
    assert_configured_checkout(&repo);
}

#[test]
fn restored_validates_every_configured_target_before_removing_any_cache() {
    for target in ["../app/src", "../.git"] {
        let repo = configured_roots();
        repo.write(
            "tools/.cargo/config.toml",
            &format!("[build]\ntarget-dir='{target}'\n"),
        );
        let output = helper(&repo, &["restored"]);
        assert!(!output.status.success(), "{target}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("overlaps tracked checkout content or repository control paths"),
            "{target}: {output:?}"
        );
        assert!(repo.root().join("cache dir").is_dir());
        assert!(repo.root().join("tools/target").is_dir());
        assert_configured_checkout(&repo);
    }
}

#[test]
fn restored_preserves_tracked_files_in_a_default_target_without_metadata() {
    let repo = Repo::new("cold-default-tracked");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/tracked", "checkout data\n");
    repo.git(&["add", "--force", "target/tracked"]);
    repo.git(&[
        "ls-files",
        "--error-unmatch",
        "src/lib.rs",
        "target/tracked",
    ]);
    repo.set_mtime("src/lib.rs", UNIX_EPOCH - Duration::from_secs(1));
    stale_target(&repo, "target");
    assert!(!repo.root().join(".velnor/rust-metadata.json").exists());
    let output = helper(&repo, &["restored"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("contains checkout files"),
        "{output:?}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("building cold"));
    for (path, expected) in [
        ("src/lib.rs", "pub fn one() {}\n"),
        ("target/tracked", "checkout data\n"),
        ("target/debug/.fingerprint/stale/lib-stale", "x"),
    ] {
        assert_eq!(
            std::fs::read(repo.root().join(path)).unwrap_or_default(),
            expected.as_bytes()
        );
    }
    assert!(repo.root().join(".git").is_dir());
    repo.git(&[
        "ls-files",
        "--error-unmatch",
        "src/lib.rs",
        "target/tracked",
    ]);
}
