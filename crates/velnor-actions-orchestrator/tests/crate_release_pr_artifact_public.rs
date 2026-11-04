// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use velnor_actions_orchestrator::crate_release_pr_artifact::{
    PreparationArtifact, PreparationBinding, PreparationCheckout, PreparedFile,
    verify_preparation_artifact,
};

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

fn candidate() -> (PreparationArtifact, BTreeSet<String>) {
    let bytes = b"[package]\nversion = \"0.1.1\"\n".to_vec();
    (
        PreparationArtifact {
            schema: 1,
            binding: PreparationBinding {
                repository: "owner/project".into(),
                workflow_sha: "a".repeat(40),
                source_sha: "b".repeat(40),
                run_id: 12,
                run_attempt: 1,
            },
            files: vec![PreparedFile {
                path: "Cargo.toml".into(),
                sha256: digest(&bytes),
                bytes,
            }],
        },
        BTreeSet::from(["Cargo.toml".into()]),
    )
}

#[test]
fn accepts_exact_reviewed_run_and_allowlisted_file() -> Result<(), Box<dyn std::error::Error>> {
    let (artifact, allowed) = candidate();
    let bytes = serde_json::to_vec(&artifact)?;
    assert_eq!(
        verify_preparation_artifact(&bytes, &digest(&bytes), &artifact.binding, &allowed)?,
        artifact
    );
    Ok(())
}

#[test]
fn rejects_changed_run_digest_content_and_paths() -> Result<(), Box<dyn std::error::Error>> {
    let (artifact, allowed) = candidate();
    let bytes = serde_json::to_vec(&artifact)?;
    let mut other_run = artifact.binding.clone();
    other_run.run_attempt += 1;
    assert!(verify_preparation_artifact(&bytes, &digest(&bytes), &other_run, &allowed).is_err());
    let mut other_source = artifact.binding.clone();
    other_source.source_sha = "c".repeat(40);
    assert!(verify_preparation_artifact(&bytes, &digest(&bytes), &other_source, &allowed).is_err());
    assert!(
        verify_preparation_artifact(&bytes, &"0".repeat(64), &artifact.binding, &allowed).is_err()
    );
    let mut changed = artifact.clone();
    changed.files[0].bytes.push(b'X');
    let changed_bytes = serde_json::to_vec(&changed)?;
    assert!(
        verify_preparation_artifact(
            &changed_bytes,
            &digest(&changed_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    let mut escaped = artifact.clone();
    escaped.files[0].path = "../Cargo.toml".into();
    let escaped_bytes = serde_json::to_vec(&escaped)?;
    assert!(
        verify_preparation_artifact(
            &escaped_bytes,
            &digest(&escaped_bytes),
            &artifact.binding,
            &BTreeSet::from(["../Cargo.toml".into()])
        )
        .is_err()
    );
    let mut unexpected = artifact.clone();
    unexpected.files[0].path = "README.md".into();
    let unexpected_bytes = serde_json::to_vec(&unexpected)?;
    assert!(
        verify_preparation_artifact(
            &unexpected_bytes,
            &digest(&unexpected_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    let mut duplicated = artifact.clone();
    duplicated.files.push(duplicated.files[0].clone());
    let duplicate_bytes = serde_json::to_vec(&duplicated)?;
    assert!(
        verify_preparation_artifact(
            &duplicate_bytes,
            &digest(&duplicate_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    Ok(())
}

fn git(root: &std::path::Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err("fixture git failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn make_checkout()
-> Result<(tempfile::TempDir, PreparationBinding, BTreeSet<String>), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    git(root.path(), &["init", "-q"])?;
    std::fs::write(root.path().join("Cargo.toml"), "version = \"0.1.0\"\n")?;
    std::fs::write(root.path().join("README.md"), "reviewed\n")?;
    git(root.path(), &["add", "."])?;
    git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "initial",
        ],
    )?;
    let mut binding = candidate().0.binding;
    binding.source_sha = git(root.path(), &["rev-parse", "HEAD"])?;
    let allowed = BTreeSet::from(["Cargo.toml".into(), "CHANGELOG.md".into()]);
    Ok((root, binding, allowed))
}

#[test]
fn captures_only_allowed_regular_updates_from_clean_source()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding.clone(), allowed.clone())?;
    std::fs::write(root.path().join("Cargo.toml"), "version = \"0.1.1\"\n")?;
    std::fs::write(root.path().join("CHANGELOG.md"), "release notes\n")?;
    let artifact = checkout.collect()?;
    assert_eq!(
        artifact
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["CHANGELOG.md", "Cargo.toml"]
    );
    let encoded = serde_json::to_vec(&artifact)?;
    assert_eq!(
        verify_preparation_artifact(&encoded, &digest(&encoded), &binding, &allowed)?,
        artifact
    );
    Ok(())
}

#[test]
fn refuses_dirty_source_and_unapproved_updates() -> Result<(), Box<dyn std::error::Error>> {
    let (root, binding, allowed) = make_checkout()?;
    std::fs::write(root.path().join("README.md"), "dirty\n")?;
    assert!(PreparationCheckout::open(root.path(), binding, allowed).is_err());
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::write(root.path().join(".hidden"), "unexpected\n")?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_deleted_files_mode_changes_and_symlinks() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::remove_file(root.path().join("Cargo.toml"))?;
    assert!(checkout.collect().is_err());
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::set_permissions(
        root.path().join("Cargo.toml"),
        std::fs::Permissions::from_mode(0o755),
    )?;
    assert!(checkout.collect().is_err());
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    symlink("README.md", root.path().join("CHANGELOG.md"))?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[test]
fn refuses_git_control_and_head_changes() -> Result<(), Box<dyn std::error::Error>> {
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    let config = root.path().join(".git/config");
    let mut contents = std::fs::read(&config)?;
    contents.extend_from_slice(b"\n");
    std::fs::write(config, contents)?;
    assert!(checkout.collect().is_err());
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    let index = root.path().join(".git/index");
    let mut contents = std::fs::read(&index)?;
    contents.push(0);
    std::fs::write(index, contents)?;
    assert!(checkout.collect().is_err());
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::write(root.path().join("Cargo.toml"), "new head\n")?;
    git(root.path(), &["add", "Cargo.toml"])?;
    git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "new",
        ],
    )?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[test]
fn refuses_ignored_untracked_files() -> Result<(), Box<dyn std::error::Error>> {
    let (root, mut binding, allowed) = make_checkout()?;
    std::fs::write(root.path().join(".gitignore"), ".hidden\n")?;
    git(root.path(), &["add", ".gitignore"])?;
    git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "ignore",
        ],
    )?;
    binding.source_sha = git(root.path(), &["rev-parse", "HEAD"])?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::write(root.path().join(".hidden"), "ignored\n")?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[test]
fn refuses_oversized_release_output() -> Result<(), Box<dyn std::error::Error>> {
    let (root, binding, allowed) = make_checkout()?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::write(
        root.path().join("CHANGELOG.md"),
        vec![b'x'; 8 * 1024 * 1024 + 1],
    )?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_replaced_parent_directory_symlink() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let (root, mut binding, allowed) = make_checkout()?;
    for dir in ["one", "two"] {
        std::fs::create_dir(root.path().join(dir))?;
        std::fs::write(root.path().join(dir).join("data"), "same\n")?;
    }
    git(root.path(), &["add", "."])?;
    git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "directories",
        ],
    )?;
    binding.source_sha = git(root.path(), &["rev-parse", "HEAD"])?;
    let checkout = PreparationCheckout::open(root.path(), binding, allowed)?;
    std::fs::remove_file(root.path().join("one/data"))?;
    std::fs::remove_dir(root.path().join("one"))?;
    symlink("two", root.path().join("one"))?;
    assert!(checkout.collect().is_err());
    Ok(())
}
