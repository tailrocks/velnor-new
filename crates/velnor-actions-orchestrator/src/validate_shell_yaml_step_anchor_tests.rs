use super::{ShellDialect, scan_workflow};

#[test]
fn full_step_alias_is_linted_under_each_consuming_job_shell() -> Result<(), String> {
    let workflow = r#"jobs:
  source:
    runs-on: ubuntu-26.04
    steps:
      - &s1
        name: shared script
        run: "printf '%s' \"$VALUE\""
  target:
    runs-on: ubuntu-26.04
    defaults:
      run:
        shell: sh
    steps:
      - *s1
"#;
    let runs = scan_workflow(workflow).map_err(|error| error.to_string())?;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].body, "printf '%s' \"$VALUE\"");
    assert_eq!(runs[1].body, runs[0].body);
    assert_eq!(runs[0].shell, ShellDialect::Bash);
    assert_eq!(runs[1].shell, ShellDialect::Sh);
    Ok(())
}

#[test]
fn step_mapping_aliases_require_prior_valid_step_definitions() {
    for (workflow, expected_error) in [
        (
            "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - *s1\n",
            "workflow_mapping_alias_unresolved",
        ),
        (
            "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - &bad.name\n        name: shared\n        run: echo safe\n",
            "workflow_mapping_name_invalid",
        ),
        (
            "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - &s1\n        run: echo safe\n        name: shared\n",
            "workflow_mapping_step_name_must_be_first",
        ),
        (
            "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - &s1\n        name: first\n        run: echo safe\n      - &s1\n        name: second\n        run: echo safe\n",
            "workflow_mapping_duplicate_anchor",
        ),
    ] {
        let error = scan_workflow(workflow).expect_err("invalid step anchor must fail");
        assert!(
            error.to_string().contains(expected_error),
            "expected {expected_error}, got {error}"
        );
    }
}
