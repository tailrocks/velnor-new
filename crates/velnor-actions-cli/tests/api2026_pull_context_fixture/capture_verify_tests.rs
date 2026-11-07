// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    if !std::process::Command::new("/usr/bin/git")
        .current_dir(root)
        .args(args)
        .status()?
        .success()
    {
        return Err(invalid());
    }
    Ok(())
}

#[test]
fn raw_capture_hydrates_staged_bytes_and_preserves_dirty_source() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("source");
    let capture = temporary.path().join("capture");
    let manifest = temporary.path().join("manifest.json");
    std::fs::create_dir(&root)?;
    std::fs::create_dir(&capture)?;
    git(&root, &["init", "-q"])?;
    git(&root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(&root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
    )?;
    std::fs::create_dir(root.join("docs"))?;
    std::fs::write(root.join("docs/item"), b"head")?;
    std::fs::write(root.join("source"), b"head source")?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-s", "-qm", "fixture"])?;
    git(
        &root,
        &[
            "update-index",
            "--info-only",
            "--add",
            "--cacheinfo",
            "160000,1111111111111111111111111111111111111111,submodule",
        ],
    )?;
    std::fs::create_dir(root.join("submodule"))?;
    std::fs::write(root.join("docs/item"), b"staged")?;
    git(&root, &["add", "docs/item"])?;
    let object = std::process::Command::new("/usr/bin/git")
        .current_dir(&root)
        .args(["rev-parse", ":docs/item"])
        .output()?;
    assert!(object.status.success());
    let oid = std::str::from_utf8(&object.stdout)?.trim();
    let alternate = temporary.path().join("alternate");
    std::fs::create_dir_all(alternate.join(&oid[..2]))?;
    std::fs::rename(
        root.join(".git/objects").join(&oid[..2]).join(&oid[2..]),
        alternate.join(&oid[..2]).join(&oid[2..]),
    )?;
    std::fs::write(
        root.join(".git/objects/info/alternates"),
        format!("{}\n", alternate.display()),
    )?;
    std::fs::remove_file(root.join("docs/item"))?;
    git(&root, &["update-index", "--skip-worktree", "docs/item"])?;
    std::fs::write(root.join("source"), b"dirty source")?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(invalid)?
        .join("crates/velnor-actions-cli/tests/api2026_pull_context_fixture/raw_capture.py");
    let status = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(script)
        .arg(&root)
        .arg(&capture)
        .arg(&manifest)
        .status()?;
    assert!(status.success());
    verify_capture(&capture, &std::fs::read_to_string(&manifest)?)?;
    assert_eq!(std::fs::read(capture.join("docs/item"))?, b"staged");
    assert_eq!(std::fs::read(capture.join("source"))?, b"dirty source");
    std::fs::write(capture.join("submodule/forged"), b"content")?;
    assert!(verify_capture(&capture, &std::fs::read_to_string(&manifest)?).is_err());
    Ok(())
}

#[test]
fn hydrated_blob_is_bound_to_the_frozen_index_and_bytes() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    assert!(
        std::process::Command::new("/usr/bin/git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()?
            .success()
    );
    std::fs::write(root.join("file"), b"hello")?;
    std::fs::set_permissions(
        root.join("file"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )?;
    let oid = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    assert!(
        std::process::Command::new("/usr/bin/git")
            .current_dir(root)
            .args([
                "update-index",
                "--info-only",
                "--add",
                "--cacheinfo",
                &format!("100644,{oid},file")
            ])
            .status()?
            .success()
    );
    let index = format!("100644 {oid} 0\tfile\0");
    let manifest = serde_json::json!({"schema":1,"index_sha256":hex(&Sha256::digest(index.as_bytes())),"entries":[{"path_hex":"66696c65","mode":"100644","oid":oid,"skip":true,"hydrated":true,"physical_mode":420,"size":5,"sha256":hex(&Sha256::digest(b"hello"))}]}).to_string();
    verify_capture(root, &manifest)?;
    let forged = "1111111111111111111111111111111111111111";
    assert!(
        std::process::Command::new("/usr/bin/git")
            .current_dir(root)
            .args([
                "update-index",
                "--info-only",
                "--add",
                "--cacheinfo",
                &format!("100644,{forged},file")
            ])
            .status()?
            .success()
    );
    let forged_index = format!("100644 {forged} 0\tfile\0");
    let forged_manifest = serde_json::json!({"schema":1,"index_sha256":hex(&Sha256::digest(forged_index.as_bytes())),"entries":[{"path_hex":"66696c65","mode":"100644","oid":forged,"skip":true,"hydrated":true,"physical_mode":420,"size":5,"sha256":hex(&Sha256::digest(b"hello"))}]}).to_string();
    assert!(verify_capture(root, &forged_manifest).is_err());
    assert!(
        std::process::Command::new("/usr/bin/git")
            .current_dir(root)
            .args([
                "update-index",
                "--info-only",
                "--add",
                "--cacheinfo",
                &format!("100644,{oid},file")
            ])
            .status()?
            .success()
    );
    std::fs::write(root.join("file"), b"other")?;
    assert!(verify_capture(root, &manifest).is_err());
    Ok(())
}
