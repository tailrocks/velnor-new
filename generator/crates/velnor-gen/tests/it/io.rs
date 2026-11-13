// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use velnor_gen::Drift;
use velnor_gen::io::{Error as IoError, apply, manifest_file, read_files};
use velnor_model::RepoPath;

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const CONFIG: &str = ".velnor/rust-ci.toml";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let base =
            std::env::temp_dir().join(format!("velnor-gen-io-{}-{name}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base)?;
        }
        fs::create_dir_all(base.join("repo"))?;
        fs::create_dir_all(base.join("outside"))?;
        Ok(Self(fs::canonicalize(base)?))
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn outside(&self) -> PathBuf {
        self.0.join("outside")
    }

    fn link(&self, from: &str, to: &Path) -> TestResult {
        let link = self.repo().join(from);
        fs::create_dir_all(link.parent().ok_or("no parent")?)?;
        symlink(to, link)?;
        Ok(())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn write(path: &Path, text: &str) -> TestResult {
    fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    fs::write(path, text)?;
    Ok(())
}

fn workflow() -> Result<RepoPath, Box<dyn Error>> {
    Ok(RepoPath::new(WORKFLOW)?)
}

fn expected() -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    Ok(BTreeMap::from([(workflow()?, "generated\n".to_owned())]))
}

#[test]
fn generate_writes_and_removes_only_real_files_inside_the_repository() -> TestResult {
    let scratch = Scratch::new("real")?;
    apply(
        &scratch.repo(),
        &expected()?,
        &[Drift::Missing(workflow()?)],
    )?;
    assert_eq!(
        fs::read_to_string(scratch.repo().join(WORKFLOW))?,
        "generated\n"
    );
    apply(
        &scratch.repo(),
        &BTreeMap::new(),
        &[Drift::Stale(workflow()?)],
    )?;
    assert!(!scratch.repo().join(WORKFLOW).exists());
    assert!(scratch.repo().join(".github/workflows").is_dir());
    Ok(())
}

#[test]
fn generate_never_writes_or_removes_through_a_symbolic_link() -> TestResult {
    let file_link = Scratch::new("file-link")?;
    let target = file_link.outside().join("victim.yml");
    write(&target, "theirs\n")?;
    file_link.link(WORKFLOW, &target)?;
    for (expected, change) in [
        (expected()?, Drift::Changed(workflow()?)),
        (BTreeMap::new(), Drift::Stale(workflow()?)),
    ] {
        let result = apply(&file_link.repo(), &expected, &[change]);
        assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
        assert_eq!(fs::read_to_string(&target)?, "theirs\n");
    }

    let directory_link = Scratch::new("directory-link")?;
    directory_link.link(".github/workflows", &directory_link.outside())?;
    let result = apply(
        &directory_link.repo(),
        &expected()?,
        &[Drift::Missing(workflow()?)],
    );
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert!(!directory_link.outside().join("rust-ci.yml").exists());
    Ok(())
}

#[test]
fn inputs_are_read_only_from_real_files_inside_the_repository() -> TestResult {
    let config = RepoPath::new(CONFIG)?;
    let scratch = Scratch::new("inputs")?;
    assert_eq!(
        read_files(&scratch.repo(), std::slice::from_ref(&config))?,
        BTreeMap::new()
    );
    write(&scratch.repo().join(CONFIG), "runner = \"a\"\n")?;
    assert_eq!(
        read_files(&scratch.repo(), std::slice::from_ref(&config))?,
        BTreeMap::from([(config.clone(), b"runner = \"a\"\n".to_vec())])
    );

    let file_link = Scratch::new("input-file-link")?;
    write(&file_link.outside().join("secret.toml"), "runner = \"b\"\n")?;
    file_link.link(CONFIG, &file_link.outside().join("secret.toml"))?;
    let directory_link = Scratch::new("input-directory-link")?;
    write(
        &directory_link.outside().join("rust-ci.toml"),
        "runner = \"c\"\n",
    )?;
    directory_link.link(".velnor", &directory_link.outside())?;
    for scratch in [&file_link, &directory_link] {
        let result = read_files(&scratch.repo(), std::slice::from_ref(&config));
        assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    }
    Ok(())
}

#[test]
fn a_leftover_or_planted_temporary_file_never_redirects_the_write() -> TestResult {
    let scratch = Scratch::new("temporary")?;
    let target = scratch.outside().join("victim.yml");
    write(&target, "theirs\n")?;
    scratch.link(".github/workflows/.rust-ci.yml.velnor-gen", &target)?;
    apply(
        &scratch.repo(),
        &expected()?,
        &[Drift::Missing(workflow()?)],
    )?;
    assert_eq!(fs::read_to_string(&target)?, "theirs\n");
    assert_eq!(
        fs::read_to_string(scratch.repo().join(WORKFLOW))?,
        "generated\n"
    );
    let mut left = fs::read_dir(scratch.repo().join(".github/workflows"))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    left.sort();
    assert_eq!(left, vec!["rust-ci.yml"]);
    Ok(())
}

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
    let missing = Scratch::new("manifest-missing")?;
    for scratch in [&file_link, &directory_link, &missing] {
        let result = manifest_file(&scratch.repo(), &manifest);
        assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    }
    Ok(())
}
