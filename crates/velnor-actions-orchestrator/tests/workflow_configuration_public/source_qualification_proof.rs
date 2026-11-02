// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use super::source_qualification_aggregation::{evidence_dir, run_phase};

const PROOF_FILES: &[&str] = &[
    "output-binding manifest.json",
    "output-binding duplicate manifest.json",
    "output-binding normalized manifest.json",
];

struct Fixture {
    _root: tempfile::TempDir,
    runner_temp: PathBuf,
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let runner_temp = fs::canonicalize(root.path())?;
    fs::create_dir_all(evidence_dir(&runner_temp).join("logs"))?;
    fs::create_dir_all(runner_temp.join("velnor-source-qualification-tmp"))?;
    Ok(Fixture {
        _root: root,
        runner_temp,
    })
}

fn record_harness(
    fixture: &Fixture,
    harness: &str,
    status: &str,
    workspace: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let logs = evidence_dir(&fixture.runner_temp).join("logs");
    fs::write(
        logs.join(format!("{harness}.command.exit")),
        format!("{status}\n"),
    )?;
    let workspace_line = workspace.map_or_else(String::new, |path| path.display().to_string());
    fs::write(
        logs.join(format!("{harness}-workspace.txt")),
        format!("{workspace_line}\n"),
    )?;
    Ok(())
}

fn retained_workspace(fixture: &Fixture, files: usize) -> Result<PathBuf, Box<dyn Error>> {
    let workspace = fixture
        .runner_temp
        .join("velnor-source-qualification-tmp/velnor-goldens-bin-test.Ab12Cd");
    fs::create_dir_all(&workspace)?;
    for file in PROOF_FILES.iter().take(files) {
        fs::write(workspace.join(file), "{}\n")?;
    }
    Ok(workspace)
}

fn proof_status(fixture: &Fixture, name: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        evidence_dir(&fixture.runner_temp).join(format!("logs/{name}.exit")),
    )?)
}

#[test]
fn json_proof_for_one_mode_does_not_require_the_other_modes_receipts() -> Result<(), Box<dyn Error>>
{
    let keep = fixture()?;
    let workspace = retained_workspace(&keep, PROOF_FILES.len())?;
    record_harness(&keep, "external-shell-harness-keep", "0", Some(&workspace))?;
    let output = run_phase(
        &keep.runner_temp,
        &["preserve-keep-output-binding-json-proof-files"],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        proof_status(&keep, "external-shell-harness-keep-json-proof")?,
        "0\n"
    );
    let receipt = fs::read_to_string(
        evidence_dir(&keep.runner_temp).join("external-shell-harness-keep-json-proof.receipt.txt"),
    )?;
    assert!(receipt.contains("copied=3\n") && receipt.contains("proof_status=0\n"));

    let ordinary = fixture()?;
    record_harness(&ordinary, "external-shell-harness", "0", None)?;
    let output = run_phase(
        &ordinary.runner_temp,
        &["preserve-ordinary-output-binding-json-proof-files"],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        proof_status(&ordinary, "external-shell-harness-json-proof")?,
        "0\n"
    );
    Ok(())
}

#[test]
fn json_proof_fails_when_its_own_mode_evidence_is_missing() -> Result<(), Box<dyn Error>> {
    let incomplete = fixture()?;
    let workspace = retained_workspace(&incomplete, PROOF_FILES.len() - 1)?;
    record_harness(
        &incomplete,
        "external-shell-harness-keep",
        "0",
        Some(&workspace),
    )?;
    let output = run_phase(
        &incomplete.runner_temp,
        &["preserve-keep-output-binding-json-proof-files"],
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("successful KEEP_WORK run retained 2 of 3 required JSON proof files")
    );
    assert_eq!(
        proof_status(&incomplete, "external-shell-harness-keep-json-proof")?,
        "1\n"
    );

    let unrecorded = fixture()?;
    let output = run_phase(
        &unrecorded.runner_temp,
        &["preserve-ordinary-output-binding-json-proof-files"],
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("invalid or missing harness command status")
    );
    assert_eq!(
        proof_status(&unrecorded, "external-shell-harness-json-proof")?,
        "1\n"
    );
    Ok(())
}
