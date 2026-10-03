use super::*;
use std::os::unix::fs::PermissionsExt;

fn probe_script(body: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("probe");
    std::fs::write(
        &program,
        format!("#!/bin/sh\nprintf x >> \"$0.calls\"\n{body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    (directory, program)
}

fn assert_one_spawn(program: &Path) {
    assert_eq!(
        std::fs::read(program.with_extension("calls")).unwrap(),
        b"x"
    );
}

#[test]
fn measured_driver_probe_preserves_success_output_and_one_spawn() {
    let (_directory, program) = probe_script("printf '  driver version 1\\n  '");
    assert_eq!(run(&program, &["--version"]).unwrap(), "driver version 1");
    assert_one_spawn(&program);
}

#[test]
fn measured_driver_probe_preserves_failure_stderr_and_one_spawn() {
    let (_directory, program) = probe_script("printf 'driver failed' >&2; exit 7");
    let error = run(&program, &["--version"]).unwrap_err().to_string();
    assert!(error.contains("failed: driver failed"), "{error}");
    assert_one_spawn(&program);
}

#[test]
fn measured_status_tolerant_probe_preserves_both_streams() {
    let (_directory, program) =
        probe_script("printf 'stdout version'; printf ' stderr version' >&2; exit 7");
    assert_eq!(
        run_allowing_status(&program, &["--version"]).unwrap(),
        "stdout version stderr version"
    );
    assert_one_spawn(&program);
}

#[test]
fn measured_linker_probe_keeps_stderr_version_despite_failure_status() {
    let (_directory, program) = probe_script("printf 'linker version 1\\nextra\\n' >&2; exit 7");
    let located = Located {
        path: program.clone(),
        pins: Pins::default(),
    };
    let (version, _) = linker_version(&program, Some(&located), None).unwrap();
    assert_eq!(version, format!("{}: linker version 1", program.display()));
    assert_one_spawn(&program);
}

#[test]
fn measured_search_probe_preserves_locale_and_rejects_failed_status() {
    let (_directory, program) = probe_script(
        "test \"$LC_ALL\" = C && test \"$LANGUAGE\" = C || exit 1\nprintf 'programs: =/bin\\nlibraries: =/lib\\n'",
    );
    assert_eq!(
        search_dirs(&program),
        Some(SearchDirs {
            programs: vec![PathBuf::from("/bin")],
            libraries: vec![PathBuf::from("/lib")],
        })
    );
    assert_one_spawn(&program);
    let (_failed_directory, failed_program) =
        probe_script("printf 'programs: =/bin\\nlibraries: =/lib\\n'; exit 7");
    assert_eq!(search_dirs(&failed_program), None);
    assert_one_spawn(&failed_program);
}

#[test]
fn measured_probe_spawn_errors_preserve_caller_context() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing-driver");
    let error = run(&missing, &["--version"]).unwrap_err().to_string();
    assert_eq!(error, format!("failed to run {}", missing.display()));
    assert!(search_dirs(&missing).is_none());
}
