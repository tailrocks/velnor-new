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
    let integrity_check = script
        .find("python_binary_digest_mismatch")
        .expect("Python binary digest guard");
    let first_suite = script
        .find("\"$verified_python\" -B -m unittest discover")
        .expect("source suite command");
    assert!(
        integrity_check < first_suite,
        "guard must precede execution of the verified executable"
    );
    assert!(script.contains("python_path=$(command -v python3.14)"));
    assert!(script.contains("verified_python=\"$python_path\""));
    assert!(script.contains("sha256sum --check --status"));
    assert!(script.contains(PYTHON_BINARY_SHA256_LINUX_X64));
    for expected in [
        "-s 'scripts/qualification/mbx-synchronous' -v",
        "-s 'scripts/hosted_phase_measurement/tests' -v",
        "-s scripts -p 'test_freshness_probe.py' -v",
        "-s scripts -p 'test_freshness_probe_check.py' -v",
    ] {
        assert!(script.contains(expected), "missing {expected}: {script}");
    }
    assert_eq!(
        script
            .matches("\"$verified_python\" -B -m unittest discover")
            .count(),
        4
    );
    assert_eq!(script.matches("mise --no-config --no-env --no-hooks exec").count(), 1);
    assert!(!script.contains("-- python -B -m unittest discover"));
    assert!(script.contains("MISE_AUTO_INSTALL=false"));
    assert!(script.contains("MISE_EXEC_AUTO_INSTALL=false"));
    assert!(script.contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Python)));
}
