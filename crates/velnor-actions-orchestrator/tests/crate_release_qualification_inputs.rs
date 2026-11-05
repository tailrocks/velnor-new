// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("/usr/bin/git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run Git");
    assert!(result.status.success());
    String::from_utf8(result.stdout).expect("Git output")
}

fn repository(root: &Path) -> String {
    fs::create_dir_all(root).expect("create checkout");
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("config.toml"), "reviewed config").expect("config");
    fs::write(root.join("manifest.json"), "reviewed manifest").expect("manifest");
    fs::write(root.join(".gitignore"), "ignored\n").expect("ignore");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"]).trim().to_owned()
}

#[test]
fn captures_exact_clean_checkout_inputs_and_rejects_changes() {
    let temp = tempfile::tempdir().expect("temp");
    let control = temp.path().join("control");
    let source = temp.path().join("source");
    let control_sha = repository(&control);
    let source_sha = repository(&source);
    let request = CheckoutInputRequest {
        control: &control,
        control_sha: &control_sha,
        source: &source,
        source_sha: &source_sha,
        config_path: "config.toml",
        manifest_path: "manifest.json",
    };
    let pinned_git = ReleaseGit::host_fixture();
    let captured = capture_checkout_inputs(&request, &pinned_git).expect("capture");
    assert_eq!(captured.config, "reviewed config");
    assert_eq!(captured.manifest, "reviewed manifest");
    let wrong = CheckoutInputRequest {
        source_sha: "0000000000000000000000000000000000000000",
        ..request
    };
    assert!(capture_checkout_inputs(&wrong, &pinned_git).is_err());
    let traversal = CheckoutInputRequest {
        config_path: "../config.toml",
        ..request
    };
    assert!(capture_checkout_inputs(&traversal, &pinned_git).is_err());
    fs::write(control.join("manifest.json"), "tampered").expect("tamper");
    assert!(read(&control, &control_sha, "manifest.json", &pinned_git).is_err());
    assert!(capture_checkout_inputs(&request, &pinned_git).is_err());
    git(&control, &["checkout", "--", "manifest.json"]);
    assert_eq!(
        read(&control, &control_sha, "manifest.json", &pinned_git).expect("restored blob"),
        "reviewed manifest"
    );
    fs::write(source.join("ignored"), "unexpected").expect("ignored file");
    assert!(capture_checkout_inputs(&request, &pinned_git).is_err());
    fs::remove_file(source.join("ignored")).expect("remove ignored");
    fs::write(source.join("config.toml"), "changed").expect("source changed");
    git(&source, &["add", "config.toml"]);
    assert!(capture_checkout_inputs(&request, &pinned_git).is_err());
}
