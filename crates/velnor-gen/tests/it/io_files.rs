// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_gen::io::{Error as IoError, add_manifests, lock_root, manifest_file, write_lock};
use velnor_model::{RepoPath, RepoView};
use velnor_rust::LOCK_PATH;

use crate::io::{Scratch, TestResult, write};

#[test]
fn cargo_metadata_is_run_only_on_a_real_manifest_inside_the_repository() -> TestResult {
    let manifest = RepoPath::new("crates/a/Cargo.toml")?;
    let real = Scratch::new("manifest")?;
    write(&real.repo().join(manifest.as_str()), "[package]\n")?;
    assert_eq!(
        manifest_file(&real.repo(), &manifest)?,
        real.repo().join(manifest.as_str())
    );

    let file_link = Scratch::new("manifest-file-link")?;
    write(&file_link.outside().join("Cargo.toml"), "[package]\n")?;
    file_link.link(manifest.as_str(), &file_link.outside().join("Cargo.toml"))?;
    let directory_link = Scratch::new("manifest-directory-link")?;
    write(&directory_link.outside().join("Cargo.toml"), "[package]\n")?;
    directory_link.link("crates/a", &directory_link.outside())?;
    let lock_link = Scratch::new("manifest-lock-link")?;
    write(&lock_link.repo().join(manifest.as_str()), "[package]\n")?;
    write(&lock_link.outside().join("Cargo.lock"), "version = 4\n")?;
    lock_link.link(
        "crates/a/Cargo.lock",
        &lock_link.outside().join("Cargo.lock"),
    )?;
    let missing = Scratch::new("manifest-missing")?;
    for scratch in [&file_link, &directory_link, &lock_link, &missing] {
        let result = manifest_file(&scratch.repo(), &manifest);
        assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    }
    Ok(())
}

#[test]
fn pin_helper_writes_the_lock_inside_the_repository_and_never_through_a_symbolic_link() -> TestResult
{
    let plain = Scratch::new("lock-plain")?;
    write_lock(&plain.repo(), "version = \"0.1.81\"\n")?;
    write_lock(&plain.repo(), "version = \"0.1.80\"\n")?;
    assert_eq!(
        fs::read_to_string(plain.repo().join(".velnor/rust-ci.lock"))?,
        "version = \"0.1.80\"\n"
    );

    let directory_link = Scratch::new("lock-directory-link")?;
    directory_link.link(".velnor", &directory_link.outside())?;
    let result = write_lock(&directory_link.repo(), "pinned\n");
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert!(!directory_link.outside().join("rust-ci.lock").exists());

    let file_link = Scratch::new("lock-file-link")?;
    let target = file_link.outside().join("victim.lock");
    write(&target, "theirs\n")?;
    file_link.link(".velnor/rust-ci.lock", &target)?;
    let result = write_lock(&file_link.repo(), "pinned\n");
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert_eq!(fs::read_to_string(&target)?, "theirs\n");
    Ok(())
}

#[test]
fn pin_helper_writes_the_lock_under_the_root_it_is_given_even_inside_another_repository()
-> TestResult {
    let outer = Scratch::new("pin-root-outer")?;
    fs::create_dir_all(outer.repo().join(".git"))?;
    let sample = outer.repo().join("generator/samples/sample/view");
    fs::create_dir_all(&sample)?;
    let root = lock_root(&sample)?;
    assert_eq!(root, fs::canonicalize(&sample)?);
    write_lock(&root, "version = \"0.1.95\"\n")?;
    assert_eq!(
        fs::read_to_string(sample.join(LOCK_PATH.as_str()))?,
        "version = \"0.1.95\"\n"
    );
    assert!(!outer.repo().join(LOCK_PATH.as_str()).exists());
    Ok(())
}

#[test]
fn a_tracked_manifest_that_cannot_be_read_fails_the_view() -> TestResult {
    let manifest = RepoPath::new("b/Cargo.toml")?;
    let mut view = RepoView::default();
    view.tracked_paths.insert(manifest.clone());
    let result = add_manifests(&mut view, |_| Err(IoError::NotRegular(manifest.clone())));
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    let mut read = RepoView::default();
    read.tracked_paths.insert(manifest.clone());
    add_manifests(&mut read, |paths| {
        Ok(paths.iter().map(|p| (p.clone(), b"x".to_vec())).collect())
    })?;
    assert!(read.files.contains_key(&manifest));
    Ok(())
}
