// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

const CLI: &str = env!("CARGO_BIN_EXE_velnor-actions");

#[test]
fn selects_bin_by_default_and_example_when_requested() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let bin = fixture.executable("target/debug/qualification-diagnostics-bin")?;
    let example = fixture.executable("target/debug/qualification-diagnostics-example")?;
    fixture.write_messages(&[
        Fixture::record(&bin, "qualification-diagnostics", "bin", &fixture.manifest),
        Fixture::record(
            &example,
            "qualification-diagnostics",
            "example",
            &fixture.manifest,
        ),
    ])?;

    let default = fixture.invoke(None)?;
    assert_success_path(&default, &bin)?;
    let selected = fixture.invoke(Some("example"))?;
    assert_success_path(&selected, &example)?;
    Ok(())
}

#[test]
fn rejects_wrong_kind_name_manifest_and_duplicate_artifacts() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let executable = fixture.executable("target/debug/qualification-diagnostics")?;
    let wrong_manifest = fixture.root.join("other/Cargo.toml");
    fs::create_dir_all(wrong_manifest.parent().ok_or("manifest has no parent")?)?;
    fs::write(
        &wrong_manifest,
        "[package]\nname = \"other\"\nversion = \"0.1.0\"\n",
    )?;
    let scenarios = [
        (
            "wrong kind",
            Fixture::record(
                &executable,
                "qualification-diagnostics",
                "example",
                &fixture.manifest,
            ),
            None,
            "qualification-diagnostics",
        ),
        (
            "wrong target name",
            Fixture::record(&executable, "other-example", "bin", &fixture.manifest),
            None,
            "qualification-diagnostics",
        ),
        (
            "wrong manifest",
            Fixture::record(
                &executable,
                "qualification-diagnostics",
                "bin",
                &wrong_manifest,
            ),
            None,
            "qualification-diagnostics",
        ),
    ];
    for (scenario, record, kind, target) in scenarios {
        fixture.write_messages(&[record])?;
        let output = fixture.invoke_for(kind, target)?;
        assert_rejected(&output, scenario, 0);
    }

    let duplicate = Fixture::record(
        &executable,
        "qualification-diagnostics",
        "bin",
        &fixture.manifest,
    );
    fixture.write_messages(&[duplicate.clone(), duplicate])?;
    let output = fixture.invoke(None)?;
    assert_rejected(&output, "duplicate artifacts", 2);
    Ok(())
}

fn assert_success_path(output: &Output, expected: &Path) -> Result<(), Box<dyn Error>> {
    assert!(
        output.status.success(),
        "artifact selection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selected = PathBuf::from(std::str::from_utf8(&output.stdout)?.trim_end());
    assert_eq!(selected, expected.canonicalize()?);
    Ok(())
}

fn assert_rejected(output: &Output, scenario: &str, matches: usize) {
    assert_eq!(output.status.code(), Some(1), "{scenario} was accepted");
    assert!(
        output.stdout.is_empty(),
        "{scenario} printed an artifact path"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("found {matches}")),
        "{scenario} returned unexpected diagnostic: {stderr}"
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    manifest: PathBuf,
    messages: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let manifest = root.join("crates/velnor-actions-cli/Cargo.toml");
        fs::create_dir_all(manifest.parent().ok_or("manifest has no parent")?)?;
        fs::write(&manifest, "[package]\nname = \"velnor-actions-cli\"\n")?;
        let messages = root.join("cargo-messages.jsonl");
        Ok(Self {
            _temp: temp,
            root,
            manifest,
            messages,
        })
    }

    fn executable(&self, relative: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().ok_or("executable has no parent")?)?;
        fs::write(&path, "#!/bin/sh\nexit 0\n")?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
        Ok(path)
    }

    fn record(executable: &Path, name: &str, kind: &str, manifest: &Path) -> Value {
        json!({
            "reason": "compiler-artifact",
            "target": {"name": name, "kind": [kind]},
            "manifest_path": manifest.to_string_lossy(),
            "executable": executable.to_string_lossy(),
        })
    }

    fn write_messages(&self, records: &[Value]) -> Result<(), Box<dyn Error>> {
        let contents = records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        fs::write(&self.messages, format!("{contents}\n"))?;
        Ok(())
    }

    fn invoke(&self, kind: Option<&str>) -> Result<Output, Box<dyn Error>> {
        self.invoke_for(kind, "qualification-diagnostics")
    }

    fn invoke_for(&self, kind: Option<&str>, target: &str) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(CLI);
        command
            .env_clear()
            .env("VELNOR_INTERNAL_OP", "repo-policy-v1")
            .env("VELNOR_REPO_POLICY_ACTION", "artifact-executable")
            .env("VELNOR_REPO_POLICY_ROOT", &self.root)
            .env("VELNOR_REPO_POLICY_MESSAGES", &self.messages)
            .env("VELNOR_REPO_POLICY_MANIFEST", &self.manifest)
            .env("VELNOR_REPO_POLICY_TARGET", target);
        if let Some(kind) = kind {
            command.env("VELNOR_REPO_POLICY_ARTIFACT_KIND", kind);
        }
        Ok(command.output()?)
    }
}
