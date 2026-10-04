use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{export_script, import_script};
use crate::mbx_bundle::EXPORT_SCRIPT;

#[test]
fn export_and_gc_capture_receipts_phases_and_nonfatal_samples() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let script = export_script(true, EXPORT_SCRIPT);
    let result = run_script(
        &scratch,
        "export-gc",
        &script,
        &[
            ("MBX_QUALIFICATION_PHASE_FILE", scratch.0.join("phase.tsv")),
            (
                "MBX_QUALIFICATION_EXPORT_RECEIPT",
                scratch.0.join("export.txt"),
            ),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("SAMPLER_STATUS", "17".into()),
            ("EXPORT_STATUS", "0".into()),
            ("GC_STATUS", "0".into()),
        ],
        false,
    )?;
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let phases =
        fs::read_to_string(scratch.0.join("phase.tsv")).map_err(|error| display_error(&error))?;
    let rows: Vec<_> = phases.lines().collect();
    assert_eq!(rows.len(), 4, "{phases}");
    assert_eq!(
        rows.iter()
            .map(|row| row.split('\t').nth(1))
            .collect::<Vec<_>>(),
        [
            Some("export-start"),
            Some("export-end"),
            Some("gc-start"),
            Some("gc-end")
        ]
    );
    for row in rows {
        let timestamp = row.split('\t').next().unwrap_or_default();
        assert!(timestamp.contains('T') && timestamp.ends_with('Z'), "{row}");
    }
    let receipt =
        fs::read_to_string(scratch.0.join("export.txt")).map_err(|error| display_error(&error))?;
    assert!(receipt.contains("command=export\nstdout_bytes=11\nout-export\nstderr_bytes=11\nerr-export\n\nexit_status=0"), "{receipt}");
    assert!(
        receipt.contains("command=gc\nstdout_bytes=7\ngc-out\nstderr_bytes=7\ngc-err"),
        "{receipt}"
    );
    assert!(
        receipt.contains("command=snapshot-export-complete"),
        "{receipt}"
    );
    assert!(
        receipt.contains("command=snapshot-gc-complete"),
        "{receipt}"
    );
    assert_eq!(receipt.matches("exit_status=17").count(), 2, "{receipt}");
    let output = fs::read_to_string(scratch.0.join("export-gc-github-output"))
        .map_err(|error| display_error(&error))?;
    assert!(output.contains("export_status=0\n"), "{output}");
    assert!(output.contains("gc_status=0\n"), "{output}");
    assert!(output.contains("ready=true\n"), "{output}");
    Ok(())
}

#[test]
fn gc_failure_is_recorded_without_changing_nonfatal_export_status() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let script = export_script(true, EXPORT_SCRIPT);
    let failure_receipt = scratch.0.join("gc-failure.txt");
    let failure = run_script(
        &scratch,
        "gc-failure",
        &script,
        &[
            (
                "MBX_QUALIFICATION_PHASE_FILE",
                scratch.0.join("gc-failure.tsv"),
            ),
            ("MBX_QUALIFICATION_EXPORT_RECEIPT", failure_receipt.clone()),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("SAMPLER_STATUS", "0".into()),
            ("EXPORT_STATUS", "0".into()),
            ("GC_STATUS", "44".into()),
        ],
        false,
    )?;
    assert!(
        failure.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&failure.stdout),
        String::from_utf8_lossy(&failure.stderr)
    );
    let failure_body =
        fs::read_to_string(failure_receipt).map_err(|error| display_error(&error))?;
    assert!(
        failure_body.contains("command=snapshot-gc-complete"),
        "{failure_body}"
    );
    assert!(
        failure_body.contains(
            "command=gc\nstdout_bytes=7\ngc-out\nstderr_bytes=7\ngc-err\n\nexit_status=44"
        ),
        "{failure_body}"
    );
    let failure_output = fs::read_to_string(scratch.0.join("gc-failure-github-output"))
        .map_err(|error| display_error(&error))?;
    assert!(
        failure_output.contains("export_status=0\n"),
        "{failure_output}"
    );
    assert!(
        failure_output.contains("gc_status=44\n"),
        "{failure_output}"
    );
    assert!(failure_output.contains("ready=false\n"), "{failure_output}");
    Ok(())
}

#[test]
fn no_build_export_is_nonfatal_and_does_not_run_gc() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let script = export_script(true, EXPORT_SCRIPT);
    let no_build = run_script(
        &scratch,
        "no-build",
        &script,
        &[
            (
                "MBX_QUALIFICATION_PHASE_FILE",
                scratch.0.join("no-build.tsv"),
            ),
            (
                "MBX_QUALIFICATION_EXPORT_RECEIPT",
                scratch.0.join("no-build.txt"),
            ),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("SAMPLER_STATUS", "0".into()),
            ("EXPORT_NO_BUILD", "true".into()),
        ],
        false,
    )?;
    assert!(
        no_build.status.success(),
        "{}",
        String::from_utf8_lossy(&no_build.stderr)
    );
    let no_build_receipt = fs::read_to_string(scratch.0.join("no-build.txt"))
        .map_err(|error| display_error(&error))?;
    assert!(
        no_build_receipt.contains("no completed mbx builds are recorded for export group"),
        "{no_build_receipt}"
    );
    assert!(
        no_build_receipt.contains("exit_status=1"),
        "{no_build_receipt}"
    );
    let no_build_output = fs::read_to_string(scratch.0.join("no-build-github-output"))
        .map_err(|error| display_error(&error))?;
    assert!(
        no_build_output.contains("export_status=1\n"),
        "{no_build_output}"
    );
    assert!(
        no_build_output.contains("ready=false\n"),
        "{no_build_output}"
    );
    assert!(!no_build_output.contains("gc_status="), "{no_build_output}");
    Ok(())
}

#[test]
fn enospc_export_failure_never_marks_bundle_ready_or_runs_gc() -> Result<(), String> {
    let scratch = Scratch::new()?;
    setup_evidence(&scratch)?;
    let script = export_script(true, EXPORT_SCRIPT);
    let receipt = scratch.0.join("enospc-export.txt");
    let failure = run_script(
        &scratch,
        "enospc-export",
        &script,
        &[
            (
                "MBX_QUALIFICATION_PHASE_FILE",
                scratch.0.join("enospc-export.tsv"),
            ),
            ("MBX_QUALIFICATION_EXPORT_RECEIPT", receipt.clone()),
            ("MBX_QUALIFICATION_SAMPLE_INTERVAL", "5".into()),
            ("EXPORT_ENOSPC", "true".into()),
        ],
        false,
    )?;
    assert!(
        !failure.status.success(),
        "export failure unexpectedly succeeded"
    );
    let body = fs::read_to_string(receipt).map_err(|error| display_error(&error))?;
    assert!(
        body.contains("No space left on device (os error 28)"),
        "{body}"
    );
    assert!(body.contains("exit_status=1"), "{body}");
    let output = fs::read_to_string(scratch.0.join("enospc-export-github-output"))
        .map_err(|error| display_error(&error))?;
    assert!(output.contains("export_status=1\n"), "{output}");
    assert!(!output.contains("ready=true"), "{output}");
    assert!(!output.contains("gc_status="), "{output}");
    assert!(
        !body.contains("command=gc\n"),
        "GC ran after export failure: {body}"
    );
    Ok(())
}

fn setup_evidence(scratch: &Scratch) -> Result<(), String> {
    fs::create_dir_all(scratch.0.join("mbx-cache-evidence")).map_err(|error| display_error(&error))
}

#[test]
fn importer_receipt_preserves_raw_streams_and_exit_status() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let receipt = scratch.0.join("import.txt");
    let script = import_script(
        true,
        "",
        "if mbx cache import bundle; then echo imported; else echo fallback; fi\n",
    );
    let result = run_script(
        &scratch,
        "import",
        &script,
        &[
            ("MBX_QUALIFICATION_IMPORT_RECEIPT", receipt.clone()),
            ("IMPORT_STATUS", "42".into()),
        ],
        true,
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("import-out"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("fallback"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("import-err"));
    let body = fs::read_to_string(&receipt).map_err(|error| display_error(&error))?;
    assert!(
        body.contains("stdout_bytes=11\nimport-out\nstderr_bytes=11\nimport-err\n\nexit_status=42"),
        "{body}"
    );
    assert!(!scratch.0.join("import.txt.tmp").exists());
    Ok(())
}

fn run_script(
    scratch: &Scratch,
    name: &str,
    script: &str,
    extra_env: &[(&str, std::path::PathBuf)],
    importing: bool,
) -> Result<std::process::Output, String> {
    let bin = scratch.0.join(format!("{name}-bin"));
    fs::create_dir_all(&bin).map_err(|error| display_error(&error))?;
    executable(
        &bin.join("mbx"),
        "#!/bin/sh\ncase \"$1:$2\" in cache:export) if [ \"${EXPORT_NO_BUILD:-false}\" = true ]; then printf 'no completed mbx builds are recorded for export group\\n' >&2; exit 1; fi; if [ \"${EXPORT_ENOSPC:-false}\" = true ]; then printf 'mbx[error]: No space left on device (os error 28)\\n' >&2; exit 1; fi; mkdir -p \"$RUNNER_TEMP/mbx-single-bundle\"; printf 'out-export\\n'; printf 'err-export\\n' >&2; exit \"${EXPORT_STATUS:-0}\" ;; gc:--max-size) printf 'gc-out\\n'; printf 'gc-err\\n' >&2; exit \"${GC_STATUS:-0}\" ;; cache:import) printf 'import-out\\n'; printf 'import-err\\n' >&2; exit \"${IMPORT_STATUS:-0}\" ;; esac\nexit 0\n",
    )?;
    executable(
        &bin.join("bash"),
        "#!/bin/sh\nprintf 'sample-out\\n'; printf 'sample-err\\n' >&2; exit \"${SAMPLER_STATUS:-0}\"\n",
    )?;
    executable(
        &bin.join("df"),
        "#!/bin/sh\nprintf 'Filesystem 1024-blocks Used Available Capacity Mounted on\\n'\n",
    )?;
    let output = scratch.0.join(format!("{name}-github-output"));
    let github_env = scratch.0.join(format!("{name}-github-env"));
    fs::write(&output, "").map_err(|error| display_error(&error))?;
    fs::write(&github_env, "").map_err(|error| display_error(&error))?;
    let runner_temp = scratch.0.join(format!("{name}-runner-temp"));
    fs::create_dir_all(runner_temp.join("mbx-cache-evidence"))
        .map_err(|error| display_error(&error))?;
    let mut command = Command::new("/bin/bash");
    command
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_OUTPUT", output)
        .env("GITHUB_ENV", github_env)
        .env("MBX_CACHE_EXPORT_GROUP", "group");
    for (key, value) in extra_env {
        if let Some(value) = value.to_str() {
            command.env(key, value);
        } else {
            return Err("non-UTF8 test environment value".to_owned());
        }
    }
    if importing {
        command.env("MATCHED", "true");
    }
    command.output().map_err(|error| display_error(&error))
}

fn executable(path: &std::path::Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| display_error(&error))?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| display_error(&error))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|error| display_error(&error))
}

fn display_error(error: &std::io::Error) -> String {
    error.to_string()
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-qualification-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| display_error(&error))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not clean MBX qualification test directory: {error}");
        }
    }
}
