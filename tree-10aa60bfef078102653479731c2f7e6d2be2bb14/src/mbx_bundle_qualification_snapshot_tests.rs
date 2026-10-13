use std::fs;

use super::{Scratch, export_script, run_script, setup_evidence};
use crate::mbx_bundle::EXPORT_SCRIPT;

#[test]
fn export_snapshot_failure_fails_qualification_before_gc() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let result = run_script(
        &scratch,
        "export-snapshot-failure",
        &export_script(true, EXPORT_SCRIPT),
        &[
            (
                "MBX_QUALIFICATION_PHASE_FILE",
                scratch.0.join("export-snapshot-failure.tsv"),
            ),
            (
                "MBX_QUALIFICATION_EXPORT_RECEIPT",
                scratch.0.join("export-snapshot-failure.txt"),
            ),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("SAMPLER_FAIL_LABEL", "export-complete".into()),
            ("SAMPLER_STATUS", "17".into()),
            ("EXPORT_STATUS", "0".into()),
            ("GC_STATUS", "0".into()),
        ],
        false,
    )?;
    assert!(
        !result.status.success(),
        "qualification unexpectedly passed"
    );
    let receipt = fs::read_to_string(scratch.0.join("export-snapshot-failure.txt"))
        .map_err(|error| error.to_string())?;
    assert!(
        receipt.contains("command=snapshot-export-complete") && receipt.contains("exit_status=17"),
        "{receipt}"
    );
    assert!(!receipt.contains("command=gc\n"), "{receipt}");
    let output = fs::read_to_string(scratch.0.join("export-snapshot-failure-github-output"))
        .map_err(|error| error.to_string())?;
    assert!(output.contains("export_status=0\n"), "{output}");
    assert!(!output.contains("gc_status="), "{output}");
    Ok(())
}

#[test]
fn gc_snapshot_failure_fails_qualification_after_gc() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let result = run_script(
        &scratch,
        "gc-snapshot-failure",
        &export_script(true, EXPORT_SCRIPT),
        &[
            (
                "MBX_QUALIFICATION_PHASE_FILE",
                scratch.0.join("gc-snapshot-failure.tsv"),
            ),
            (
                "MBX_QUALIFICATION_EXPORT_RECEIPT",
                scratch.0.join("gc-snapshot-failure.txt"),
            ),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("SAMPLER_FAIL_LABEL", "gc-complete".into()),
            ("SAMPLER_STATUS", "23".into()),
            ("EXPORT_STATUS", "0".into()),
            ("GC_STATUS", "0".into()),
        ],
        false,
    )?;
    assert!(
        !result.status.success(),
        "qualification unexpectedly passed"
    );
    let receipt = fs::read_to_string(scratch.0.join("gc-snapshot-failure.txt"))
        .map_err(|error| error.to_string())?;
    assert!(
        receipt.contains("command=snapshot-export-complete")
            && receipt.contains("command=snapshot-gc-complete")
            && receipt.contains("exit_status=23"),
        "{receipt}"
    );
    let output = fs::read_to_string(scratch.0.join("gc-snapshot-failure-github-output"))
        .map_err(|error| error.to_string())?;
    assert!(output.contains("gc_status=0\n"), "{output}");
    assert!(output.contains("ready=false\n"), "{output}");
    let phases = fs::read_to_string(scratch.0.join("gc-snapshot-failure.tsv"))
        .map_err(|error| error.to_string())?;
    assert!(
        phases.lines().any(|row| row.ends_with("\tgc-end")),
        "{phases}"
    );
    Ok(())
}
