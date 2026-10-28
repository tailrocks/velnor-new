// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::{
    NATIVE_IMAGE_CANDIDATE_INPUTS, parse_image_candidate_source_closure,
};

struct CheckJobFixture {
    directory: TempDir,
    root: PathBuf,
    source: String,
    closure: std::path::PathBuf,
}

impl CheckJobFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let directory = TempDir::new()?;
        let base = directory.path().canonicalize()?;
        let root = base.join("checkout");
        fs::create_dir_all(root.join(".velnor"))?;
        for relative in NATIVE_IMAGE_CANDIDATE_INPUTS {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().ok_or("source input has no parent")?)?;
            if relative == ".velnor/runner.lock" {
                fs::write(
                    path,
                    b"runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\n",
                )?;
            } else {
                fs::write(path, format!("fixture source A: {relative}\n"))?;
            }
        }
        fs::write(root.join("tracked.txt"), b"fixture source A\n")?;
        git(&root, &["init", "--quiet"])?;
        git(&root, &["config", "user.name", "Check source fixture"])?;
        git(
            &root,
            &["config", "user.email", "check-source@example.invalid"],
        )?;
        git(&root, &["config", "commit.gpgsign", "false"])?;
        git(&root, &["add", "--all"])?;
        git(&root, &["commit", "--quiet", "-m", "source A"])?;
        let source = String::from_utf8(git(&root, &["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned();
        let closure = base.join("source-closure.json");
        Ok(Self {
            directory,
            root,
            source,
            closure,
        })
    }

    fn add_replacement_ref(&self, checkout_replacement: bool) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join("Cargo.toml"), b"fixture source B\n")?;
        fs::write(self.root.join("tracked.txt"), b"fixture source B\n")?;
        let lock_path = self.root.join(".velnor/runner.lock");
        let mut lock = fs::read(&lock_path)?;
        lock.extend_from_slice(b"fixture_marker = \"replacement-b\"\n");
        fs::write(lock_path, lock)?;
        git(&self.root, &["add", "--all"])?;
        let tree = String::from_utf8(git(&self.root, &["write-tree"])?.stdout)?
            .trim()
            .to_owned();
        let replacement = String::from_utf8(
            git(
                &self.root,
                &["commit-tree", &tree, "-m", "unrelated replacement source"],
            )?
            .stdout,
        )?
        .trim()
        .to_owned();
        git(
            &self.root,
            &["checkout", "--quiet", &self.source, "--", "."],
        )?;
        if checkout_replacement {
            git(
                &self.root,
                &["checkout", "--quiet", "--detach", &replacement],
            )?;
            git(
                &self.root,
                &["update-ref", "--no-deref", "HEAD", &self.source],
            )?;
        }
        git(&self.root, &["replace", &self.source, &replacement])?;
        Ok(())
    }

    fn same_commit_decoy(&self) -> Result<PathBuf, Box<dyn Error>> {
        let decoy = self.directory.path().join("decoy");
        let path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let output = Command::new("git")
            .env_clear()
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["clone", "--quiet", "--no-hardlinks"])
            .arg(&self.root)
            .arg(&decoy)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "git clone decoy failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let head = String::from_utf8(git(&decoy, &["rev-parse", "HEAD"])?.stdout)?;
        if head.trim() != self.source
            || fs::read(decoy.join(".velnor/runner.lock"))?
                != fs::read(self.root.join(".velnor/runner.lock"))?
        {
            return Err("decoy does not match selected source".into());
        }
        Ok(decoy)
    }

    fn invoke(&self) -> Result<Output, Box<dyn Error>> {
        self.invoke_with_git_routing(None)
    }

    fn invoke_routed(&self, decoy: &std::path::Path) -> Result<Output, Box<dyn Error>> {
        self.invoke_with_git_routing(Some(decoy))
    }

    fn invoke_with_git_routing(
        &self,
        decoy: Option<&std::path::Path>,
    ) -> Result<Output, Box<dyn Error>> {
        let path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.directory.path().join("home"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GITHUB_SHA", &self.source)
            .env("GITHUB_WORKFLOW_SHA", &self.source)
            .args(["image-maintenance", "check-job-source", "--root"])
            .arg(&self.root)
            .args(["--source-commit", &self.source, "--source-closure-path"])
            .arg(&self.closure);
        if let Some(decoy) = decoy {
            command
                .env("GIT_DIR", decoy.join(".git"))
                .env("GIT_WORK_TREE", decoy)
                .env("GIT_INDEX_FILE", decoy.join(".git/index"));
        }
        Ok(command.output()?)
    }

    fn source_input_hashes(&self) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        NATIVE_IMAGE_CANDIDATE_INPUTS
            .into_iter()
            .map(|relative| {
                let bytes = fs::read(self.root.join(relative))?;
                Ok((relative.to_owned(), sha256(&bytes)))
            })
            .collect()
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
    let output = Command::new("git")
        .env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

#[test]
fn replacement_ref_preserves_source_a_closure_and_input_hashes() -> Result<(), Box<dyn Error>> {
    let fixture = CheckJobFixture::new()?;
    let expected = fixture.source_input_hashes()?;
    fixture.add_replacement_ref(false)?;

    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "check-job-source failed: {output:?}"
    );
    let closure = parse_image_candidate_source_closure(&fs::read(&fixture.closure)?)?;
    assert_eq!(closure.source_commit, fixture.source);
    assert_eq!(closure.inputs.len(), 75);
    assert_eq!(closure.inputs, expected);
    Ok(())
}

#[test]
fn replacement_ref_rejects_physical_source_b_without_closure() -> Result<(), Box<dyn Error>> {
    let fixture = CheckJobFixture::new()?;
    fixture.add_replacement_ref(true)?;

    let output = fixture.invoke()?;
    assert!(
        !output.status.success(),
        "accepted physical source B as A: {output:?}"
    );
    assert!(!fixture.closure.exists());
    Ok(())
}

#[test]
fn ambient_git_routing_cannot_select_clean_decoy_for_dirty_source() -> Result<(), Box<dyn Error>> {
    let fixture = CheckJobFixture::new()?;
    let decoy = fixture.same_commit_decoy()?;
    let dockerfile = "images/job/ubuntu-26.04/Dockerfile";
    let mut changed = fs::read(fixture.root.join(dockerfile))?;
    changed.extend_from_slice(b"\nambient Git routing fixture change\n");
    fs::write(fixture.root.join(dockerfile), &changed)?;

    let output = fixture.invoke_routed(&decoy)?;
    let emitted_hash = if fixture.closure.exists() {
        Some(
            parse_image_candidate_source_closure(&fs::read(&fixture.closure)?)?
                .inputs
                .get(dockerfile)
                .cloned()
                .ok_or("Runner Dockerfile hash is absent from closure")?,
        )
    } else {
        None
    };
    if let Some(emitted_hash) = &emitted_hash {
        assert_eq!(*emitted_hash, sha256(&changed));
    }
    assert!(
        !output.status.success() && emitted_hash.is_none(),
        "accepted dirty source through a clean Git decoy: {output:?}; emitted Dockerfile hash: {emitted_hash:?}"
    );
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
