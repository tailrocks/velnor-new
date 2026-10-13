#[test]
fn python_source_install_resolves_through_the_catalog_pin() {
    let catalog = ToolCatalog::pinned();
    let command = crate::python_source_units::validator_command(&catalog)
        .expect("pinned source-suite command");
    let python = catalog.tool_spec(PinnedTool::Python);
    assert!(matches!(
        validator_command_specs(&command),
        CommandInstalls::Specs(specs) if specs == vec![python.clone()]
    ));
    assert_eq!(
        command.argv[2]
            .matches("mise --no-config --no-env --no-hooks exec")
            .count(),
        1,
        "all Python suites share one verified Mise execution environment"
    );
    let mut subjects = Vec::new();
    let mut blocking = Vec::new();
    audit_validator_command(&command, &catalog, &mut subjects, &mut blocking);
    assert_eq!(blocking, Vec::<String>::new());
    assert_eq!(subjects.len(), 1, "only Python install is lock-audited");
    assert_eq!(
        validator_subject(&python, &catalog).map(|subject| subject.display),
        Some("python@3.14.8".to_owned())
    );
    assert!(
        validator_subject("python@3.14.8", &catalog).is_none(),
        "bare registry shorthand must not bypass the pinned PBS artifact"
    );
}

#[test]
fn python_source_audit_rejects_unclassifiable_exec_after_install() {
    let catalog = ToolCatalog::pinned();
    let mut command = crate::python_source_units::validator_command(&catalog)
        .expect("pinned source-suite command");
    let python = catalog.tool_spec(PinnedTool::Python);
    command.argv[2] =
        format!("mise --no-config --no-env --no-hooks exec '{python}' -- true && mise unknown");
    assert!(matches!(
        validator_command_specs(&command),
        CommandInstalls::Unclassifiable(detail) if detail == "unexpected_subcommand:unknown"
    ));
}

#[test]
fn validator_prepare_must_be_install_even_when_run_contains_install() {
    let mut command = deny_command("mise --no-config install cargo-deny@0.20.2");
    command.prepare_argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "exec".to_owned(),
        "python@3.14.8".to_owned(),
        "--".to_owned(),
        "true".to_owned(),
    ];
    assert!(matches!(
        validator_command_specs(&command),
        CommandInstalls::Unclassifiable(detail) if detail == "prepare_without_install"
    ));
}
