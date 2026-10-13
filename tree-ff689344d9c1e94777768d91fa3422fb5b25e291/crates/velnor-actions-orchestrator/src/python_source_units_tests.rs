use super::*;

#[test]
fn source_suites_use_the_pinned_python_and_fixed_discovery_paths() {
    let command = validator_command(&ToolCatalog::pinned()).expect("python source command");
    assert_eq!(command.validator, ValidatorKind::PythonSourceTests);
    assert_eq!(command.name, PYTHON_SOURCE_RUN_NAME);
    assert_eq!(
        command.prepare_argv.join(" "),
        format!(
            "mise --no-config --no-env --no-hooks install {}",
            ToolCatalog::pinned().tool_spec(PinnedTool::Python)
        )
    );
    let script = &command.argv[2];
    let inner = python_suite_script();
    let integrity_check = inner
        .find("python_binary_digest_mismatch")
        .expect("Python binary digest guard");
    let first_suite = inner
        .find("\"$verified_python\" -B -m unittest discover")
        .expect("source suite command");
    assert!(
        integrity_check < first_suite,
        "guard must precede execution of the verified executable"
    );
    assert!(inner.contains("python_path=$(command -v python3.14)"));
    assert!(inner.contains("verified_python=\"$python_path\""));
    assert!(inner.contains("sha256sum --check --status"));
    assert!(inner.contains(PYTHON_BINARY_SHA256_LINUX_X64));
    for expected in [
        "-s \"scripts/qualification/mbx-synchronous\" -v",
        "-s \"scripts/hosted_phase_measurement/tests\" -v",
        "-s \"scripts\" -p \"test_freshness_probe.py\" -v",
        "-s \"scripts\" -p \"test_freshness_probe_check.py\" -v",
    ] {
        assert!(inner.contains(expected), "missing {expected}: {inner}");
    }
    assert_eq!(
        inner
            .matches("\"$verified_python\" -B -m unittest discover")
            .count(),
        4
    );
    assert!(!inner.contains("python -B -m unittest discover"));
    let python = ToolCatalog::pinned().tool_spec(PinnedTool::Python);
    let expected_outer = format!(
        concat!(
            "set -eu; export MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false ",
            "MISE_LOCKFILE=0 MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1; ",
            "mise --no-config --no-env --no-hooks exec {} -- sh -eu -c {}"
        ),
        shell_quote(&python),
        shell_quote(&inner)
    );
    assert_eq!(script, &expected_outer);
}
