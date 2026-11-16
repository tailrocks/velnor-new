// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use velnor_gen::expected_files;
use velnor_model::RepoPath;
use velnor_rust::wanted_files;

use crate::golden::samples;
use crate::release_fixture::{APP, release_view};
use crate::release_symlink::{Scratch, copy_tree, git, run};

type TestResult = Result<(), Box<dyn Error>>;

const NESTED: &str = "crates/inner/rust-toolchain.toml";

#[test]
fn nested_toolchain_files_are_read_so_the_release_check_can_see_them() -> TestResult {
    let tracked: BTreeSet<RepoPath> = [NESTED, "crates/inner/rust-toolchain", "a/other.toml"]
        .into_iter()
        .map(RepoPath::new)
        .collect::<Result<_, _>>()?;
    let wanted = wanted_files(&tracked);
    assert!(wanted.contains(&RepoPath::new(NESTED)?));
    assert!(wanted.contains(&RepoPath::new("crates/inner/rust-toolchain")?));
    assert!(!wanted.contains(&RepoPath::new("a/other.toml")?));
    Ok(())
}

#[test]
fn only_a_root_toolchain_taken_from_a_path_is_refused_when_rendering() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    view.files.insert(
        RepoPath::new(NESTED)?,
        b"[toolchain]\npath = \"/work/tc\"\n".to_vec(),
    );
    assert!(expected_files(&view).is_ok());
    view.files.insert(
        RepoPath::new("rust-toolchain.toml")?,
        b"[toolchain]\npath = \"/work/tc\"\n".to_vec(),
    );
    assert!(expected_files(&view).is_err());
    Ok(())
}

#[test]
fn a_root_toolchain_taken_from_a_path_is_refused_in_a_real_repository() -> TestResult {
    let repo = Scratch(std::env::temp_dir().join(format!(
        "velnor-gen-nested-toolchain-{}",
        std::process::id()
    )));
    fs::remove_dir_all(&repo.0).ok();
    copy_tree(&samples().join("release-two-roots/view"), &repo.0)?;
    fs::write(
        repo.0.join("rust-toolchain.toml"),
        "[toolchain]\npath = \"/work/tc\"\n",
    )?;
    git(&repo.0, &["init", "--quiet"])?;
    git(&repo.0, &["add", "--all"])?;
    let output = run(&repo.0)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("rust-toolchain.toml"), "{stderr}");
    Ok(())
}
