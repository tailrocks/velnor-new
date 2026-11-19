// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_local::io::checkout::{copy_checkout, copy_untracked};
use velnor_local::scenario::overlapping;

#[test]
fn a_state_directory_that_overlaps_the_checkout_is_refused() {
    let checkout = Path::new("/work/lab");
    assert!(overlapping(Path::new("/work/lab/state"), checkout));
    assert!(overlapping(Path::new("/work"), checkout));
    assert!(overlapping(checkout, checkout));
    assert!(!overlapping(Path::new("/work/lab-state"), checkout));
    assert!(!overlapping(Path::new("/tmp/state"), checkout));
}

#[tokio::test]
async fn untracked_files_and_inside_links_are_copied_and_escaping_links_are_refused() {
    let root = crate::support::temp_dir("untracked-links");
    let (checkout, copy, outside) = (
        root.join("checkout"),
        root.join("copy"),
        root.join("outside"),
    );
    for dir in [checkout.join("docs"), copy.clone(), outside.clone()] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let make = |result: std::io::Result<()>| result.unwrap_or_else(|e| unreachable!("{e}"));
    make(std::fs::write(checkout.join("note.txt"), "note"));
    make(std::fs::write(outside.join("secret.txt"), "secret"));
    make(std::os::unix::fs::symlink(
        "../note.txt",
        checkout.join("docs/latest"),
    ));
    make(std::os::unix::fs::symlink(
        "../outside/secret.txt",
        checkout.join("out"),
    ));
    make(std::os::unix::fs::symlink(
        &outside,
        checkout.join("escape"),
    ));
    make(std::os::unix::fs::symlink(
        "escape/secret.txt",
        checkout.join("alias"),
    ));
    make(std::os::unix::fs::symlink(
        "../../nowhere",
        checkout.join("dangling"),
    ));
    let copied = |path: &'static str| copy_untracked(&checkout, &copy, Path::new(path));
    assert!(copied("note.txt").await.is_ok());
    assert_eq!(
        std::fs::read_to_string(copy.join("note.txt"))
            .ok()
            .as_deref(),
        Some("note")
    );
    assert!(copied("docs/latest").await.is_ok());
    let link = std::fs::read_link(copy.join("docs/latest")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(link, Path::new("../note.txt"));
    for escaping in ["out", "alias", "dangling"] {
        let refused = copied(escaping).await.map_err(|e| e.to_string());
        assert!(
            refused.is_err_and(|message| message.contains(escaping)),
            "{escaping}"
        );
        assert!(
            std::fs::symlink_metadata(copy.join(escaping)).is_err(),
            "{escaping}"
        );
    }
}

#[tokio::test]
async fn the_copy_names_the_hosted_repository_as_its_origin() {
    let root = crate::support::temp_dir("copy-origin");
    let (checkout, copy) = (root.join("checkout"), root.join("copy"));
    std::fs::create_dir_all(&checkout).unwrap_or_else(|e| unreachable!("{e}"));
    let git = |dir: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
            .args(args)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&checkout, &["init", "-q", "-b", "main"]);
    std::fs::write(checkout.join("a.txt"), "a").unwrap_or_else(|e| unreachable!("{e}"));
    git(&checkout, &["add", "-A"]);
    git(&checkout, &["commit", "-q", "-m", "first"]);
    let copied = copy_checkout(&checkout, &copy, "main", "t", "tailrocks/velnor-new").await;
    assert!(copied.is_ok(), "{copied:?}");
    assert_eq!(
        git(&copy, &["remote", "get-url", "origin"]),
        "https://github.com/tailrocks/velnor-new"
    );
    assert_eq!(
        git(&copy, &["rev-parse", "HEAD"]),
        git(&checkout, &["rev-parse", "HEAD"])
    );
}
