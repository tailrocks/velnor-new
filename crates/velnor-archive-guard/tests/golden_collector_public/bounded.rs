// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

use super::mutation::{copy_mutation_repository, private_directory};
use super::support::{
    Harness, copy_executable, copy_tree, diagnostics_candidate, dogfood_plan_difference,
    retained_workspace, sha256,
};

const OUTPUT_CAP: usize = 1_048_576;
const COMMAND_TIMEOUT_MS: u64 = 5_000;
const RUN_ENTRY_MARKER: &[u8] = b"qualification-diagnostics:run-entry-succeeded\n";

#[test]
fn rejects_zero_and_over_limit_execution_controls() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    for (option, value, expected) in [
        ("--command-timeout-ms", "0", "between 1 and 3600000"),
        ("--command-timeout-ms", "3600001", "between 1 and 3600000"),
        ("--output-cap-bytes", "0", "between 1 and 67108864"),
        ("--output-cap-bytes", "67108865", "between 1 and 67108864"),
    ] {
        let output = harness
            .command(
                "check",
                &harness.root,
                &candidate,
                &harness.archive_guard,
                &harness.guard_sha256,
                None,
            )
            .arg(option)
            .arg(value)
            .output()?;
        assert_eq!(
            output.status.code(),
            Some(2),
            "{option} {value} was accepted"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
    Ok(())
}

#[test]
fn checks_the_real_five_case_collector_and_manifest() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let output = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        None,
    )?;

    if output.status.code() != Some(0) {
        let difference = dogfood_plan_difference(&output, &harness.root)?;
        return Err(format!(
            "collector check failed: {}; dogfood plan {difference}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    for case in [
        "nested",
        "mbx-nextest",
        "empty-suite",
        "minimal-cargo",
        "dogfood",
    ] {
        assert!(
            stdout.contains(&format!("match: {case}")),
            "missing match for {case}"
        );
    }
    assert!(stdout.contains("ALL GOLDENS MATCH"));
    Ok(())
}

#[test]
fn rejects_incomplete_or_changed_case_inventory_without_touching_the_oracle()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let oracle = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle_manifest = fs::read(oracle.join("MANIFEST.sha256"))?;
    let oracle_case = fs::read(oracle.join("cases/nested/plan.txt"))?;
    for fault in [
        ManifestFault::Missing,
        ManifestFault::Truncated,
        ManifestFault::Corrupt,
        ManifestFault::ExtraCaseFile,
    ] {
        let copied = harness.directory.join(format!("oracle-{fault:?}"));
        copy_oracle(&oracle, &copied)?;
        alter_manifest(&copied, fault)?;
        let output = harness.run(
            "check",
            &harness.root,
            &candidate,
            &harness.archive_guard,
            &harness.guard_sha256,
            Some(&copied),
        )?;

        assert_eq!(
            output.status.code(),
            Some(1),
            "{fault:?} returned an unexpected result: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("golden manifest"));
    }
    assert_eq!(fs::read(oracle.join("MANIFEST.sha256"))?, oracle_manifest);
    assert_eq!(fs::read(oracle.join("cases/nested/plan.txt"))?, oracle_case);
    Ok(())
}

#[test]
fn caps_stdout_after_successful_entry_and_retains_the_prefix() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let oracle = harness.directory.join("stdout-overflow-oracle");
    let mut command = fault_command(
        &harness,
        FaultRun {
            candidate: &candidate,
            root: &harness.root,
            guard: &harness.archive_guard,
            guard_sha256: &harness.guard_sha256,
            oracle: &oracle,
            diagnostics_mode: "stdout-overflow",
            diagnostics_root: &harness.root,
            mutation_root: None,
            output_cap: OUTPUT_CAP,
        },
    );
    command_timeout(&mut command, COMMAND_TIMEOUT_MS);
    let output = command.output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("OutputLimitExceeded"),
        "unexpected diagnostic: {stderr}"
    );
    let evidence = plan_evidence(&output)?;
    assert_eq!(evidence.stdout.len(), OUTPUT_CAP);
    assert!(contains(&evidence.stdout, RUN_ENTRY_MARKER));
    assert_eq!(evidence.termination.trim(), "output-limit-exceeded");
    assert_eq!(evidence.status, evidence.exit);
    assert_ne!(evidence.status.trim(), "not-started");
    assert!(!oracle.exists());
    Ok(())
}

#[test]
fn times_out_after_guard_mutation_and_cleans_only_its_owned_group() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let mutation_root = harness.directory.join("guard-mutation");
    private_directory(&mutation_root)?;
    let source = mutation_root.join("source");
    copy_mutation_repository(&harness.root, &source)?;
    let guard_directory = mutation_root.join("guard");
    private_directory(&guard_directory)?;
    let guard = guard_directory.join("archive-guard");
    copy_executable(&harness.archive_guard, &guard)?;
    let guard_sha256 = sha256(&fs::read(&guard)?);
    let oracle = harness.directory.join("guard-mutation-oracle");
    let mut command = fault_command(
        &harness,
        FaultRun {
            candidate: &candidate,
            root: &source,
            guard: &guard,
            guard_sha256: &guard_sha256,
            oracle: &oracle,
            diagnostics_mode: "replace-copied-guard-then-sleep",
            diagnostics_root: &source,
            mutation_root: Some(&mutation_root),
            output_cap: OUTPUT_CAP,
        },
    );
    command_timeout(&mut command, COMMAND_TIMEOUT_MS);
    let mut sentinel = Command::new("/bin/sleep").arg("30").spawn()?;
    let started = Instant::now();
    let result = command.output();
    let sentinel_survived = sentinel.try_wait()?.is_none();
    terminate(&mut sentinel)?;
    let output = result?;

    assert!(started.elapsed() < Duration::from_secs(15));
    assert!(sentinel_survived);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("execution TimedOut"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("archive guard changed during candidate execution"),
        "post-run guard mutation was not reported: {stderr}"
    );
    let evidence = plan_evidence(&output)?;
    assert!(contains(&evidence.stdout, RUN_ENTRY_MARKER));
    assert_eq!(evidence.termination.trim(), "timed-out");
    assert_eq!(evidence.status, evidence.exit);
    assert_ne!(evidence.status.trim(), "not-started");
    let workspace = retained_workspace(&output).ok_or("failed workspace was not retained")?;
    assert!(!workspace.join("raw/nested/plan.cleanup-error").exists());
    assert!(!oracle.exists());
    Ok(())
}

#[test]
fn retains_stderr_prefix_and_reports_source_mutation_with_output_overflow()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.directory.join("source-mutation");
    copy_mutation_repository(&harness.root, &source)?;
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700))?;
    let oracle = harness.directory.join("source-mutation-oracle");
    let mut command = fault_command(
        &harness,
        FaultRun {
            candidate: &candidate,
            root: &source,
            guard: &harness.archive_guard,
            guard_sha256: &harness.guard_sha256,
            oracle: &oracle,
            diagnostics_mode: "change-copied-source-then-stderr-overflow",
            diagnostics_root: &source,
            mutation_root: Some(&source),
            output_cap: OUTPUT_CAP,
        },
    );
    command_timeout(&mut command, COMMAND_TIMEOUT_MS);
    let output = command.output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("execution OutputLimitExceeded"),
        "unexpected diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("archive guard source fingerprint is stale or changed"),
        "post-run source mutation was not reported: {stderr}"
    );
    let evidence = plan_evidence(&output)?;
    assert_eq!(evidence.stderr.len(), OUTPUT_CAP);
    assert!(contains(&evidence.stdout, RUN_ENTRY_MARKER));
    assert_eq!(evidence.termination.trim(), "output-limit-exceeded");
    assert_eq!(evidence.status, evidence.exit);
    assert_ne!(evidence.status.trim(), "not-started");
    assert!(!oracle.exists());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum ManifestFault {
    Missing,
    Truncated,
    Corrupt,
    ExtraCaseFile,
}

#[derive(Clone, Copy)]
struct FaultRun<'a> {
    candidate: &'a Path,
    root: &'a Path,
    guard: &'a Path,
    guard_sha256: &'a str,
    oracle: &'a Path,
    diagnostics_mode: &'a str,
    diagnostics_root: &'a Path,
    mutation_root: Option<&'a Path>,
    output_cap: usize,
}

struct PlanEvidence {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: String,
    termination: String,
    exit: String,
}

fn copy_oracle(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir(destination)?;
    copy_tree(&source.join("cases"), &destination.join("cases"))?;
    fs::copy(
        source.join("MANIFEST.sha256"),
        destination.join("MANIFEST.sha256"),
    )?;
    Ok(())
}

fn alter_manifest(destination: &Path, fault: ManifestFault) -> Result<(), Box<dyn Error>> {
    let manifest = destination.join("MANIFEST.sha256");
    match fault {
        ManifestFault::Missing => fs::remove_file(manifest)?,
        ManifestFault::Truncated => {
            let mut contents = fs::read(&manifest)?;
            contents.truncate(contents.len() / 2);
            fs::write(manifest, contents)?;
        }
        ManifestFault::Corrupt => {
            let mut contents = fs::read(&manifest)?;
            contents[0] ^= 1;
            fs::write(manifest, contents)?;
        }
        ManifestFault::ExtraCaseFile => fs::write(
            destination.join("cases/ignored-extra-case-file.txt"),
            b"unlisted file\n",
        )?,
    }
    Ok(())
}

fn fault_command(harness: &Harness, run: FaultRun<'_>) -> Command {
    let mut command = harness.command(
        "capture",
        run.root,
        run.candidate,
        run.guard,
        run.guard_sha256,
        Some(run.oracle),
    );
    command
        .arg("--output-cap-bytes")
        .arg(run.output_cap.to_string())
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", run.diagnostics_mode)
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", run.diagnostics_root);
    if let Some(root) = run.mutation_root {
        command.env("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT", root);
    }
    command
}

fn command_timeout(command: &mut Command, milliseconds: u64) {
    command
        .arg("--command-timeout-ms")
        .arg(milliseconds.to_string());
}

fn plan_evidence(output: &std::process::Output) -> Result<PlanEvidence, Box<dyn Error>> {
    let workspace = retained_workspace(output).ok_or("failed workspace was not retained")?;
    let raw = workspace.join("raw/nested");
    Ok(PlanEvidence {
        stdout: fs::read(raw.join("plan.raw.txt"))?,
        stderr: fs::read(raw.join("plan.stderr.raw.txt"))?,
        status: fs::read_to_string(raw.join("plan.status"))?,
        termination: fs::read_to_string(raw.join("plan.termination"))?,
        exit: fs::read_to_string(workspace.join("stage/nested/plan.exit"))?,
    })
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn terminate(child: &mut Child) -> Result<(), Box<dyn Error>> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    child.kill()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("sentinel process did not stop within its cleanup budget".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
