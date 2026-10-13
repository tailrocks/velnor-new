use super::*;

#[test]
fn source_suites_use_the_pinned_python_and_fixed_discovery_paths() {
    let command = validator_command(&ToolCatalog::pinned()).expect("python source command");
    assert_eq!(command.validator, ValidatorKind::PythonSourceTests);
    assert_eq!(command.name, PYTHON_SOURCE_RUN_NAME);
    assert_eq!(
        command.prepare_argv.join(" "),
        "mise --no-config --no-env --no-hooks install python@3.14.8"
    );
    let script = &command.argv[2];
    for expected in [
        "-s scripts/qualification/mbx-synchronous -v",
        "-s scripts/hosted_phase_measurement/tests -v",
        "-s scripts -p 'test_freshness_probe.py' -v",
        "-s scripts -p 'test_freshness_probe_check.py' -v",
    ] {
        assert!(script.contains(expected), "missing {expected}: {script}");
    }
    assert_eq!(script.matches("python -B -m unittest discover").count(), 4);
    assert!(script.contains("MISE_AUTO_INSTALL=false"));
    assert!(script.contains("MISE_EXEC_AUTO_INSTALL=false"));
    assert!(script.contains("python@3.14.8"));
}
