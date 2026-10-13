use super::super::run_shellcheck_bodies;
use super::{ShellDialect, scan_workflow};

fn bodies(workflow: &str) -> Result<Vec<(String, ShellDialect)>, String> {
    scan_workflow(workflow)
        .map(|runs| runs.into_iter().map(|run| (run.body, run.shell)).collect())
        .map_err(|err| err.to_string())
}

#[test]
fn step_shell_after_run_overrides_the_hosted_default() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: test\n        run: echo probe\n        shell: sh\n",
    )?;
    assert_eq!(runs, [("echo probe".to_owned(), ShellDialect::Sh)]);
    Ok(())
}

#[test]
fn step_job_and_workflow_shell_precedence_is_order_independent() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  job-default-after-steps:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: job default\n        run: echo job\n    defaults:\n      run:\n        shell: bash -e {0}\n  step-override:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: bash -e {0}\n    steps:\n      - name: step override\n        run: echo step\n        shell: sh\n  workflow-default:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: workflow default\n        run: echo workflow\ndefaults:\n  run:\n    shell: sh\n",
    )?;
    assert_eq!(
        runs,
        [
            ("echo job".to_owned(), ShellDialect::Bash),
            ("echo step".to_owned(), ShellDialect::Sh),
            ("echo workflow".to_owned(), ShellDialect::Sh),
        ]
    );
    Ok(())
}

#[test]
fn container_default_is_posix_and_services_do_not_change_runner_shell() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  scale-container:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: container probe\n        run: test -n \"$HOME\"\n    container: alpine:3.22\n  hosted-container:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: container probe\n        run: test -n \"$HOME\"\n    container:\n      image: private.invalid/unknown:tag\n  hosted-service:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: service probe\n        run: test -n \"$HOME\"\n    services:\n      alpine:\n        image: alpine:3.22\n",
    )?;
    assert_eq!(
        runs,
        [
            ("test -n \"$HOME\"".to_owned(), ShellDialect::Sh),
            ("test -n \"$HOME\"".to_owned(), ShellDialect::Sh),
            ("test -n \"$HOME\"".to_owned(), ShellDialect::Bash),
        ]
    );
    Ok(())
}

#[test]
fn job_and_workflow_defaults_override_container_shell() -> Result<(), String> {
    let runs = bodies(
        "defaults:\n  run:\n    shell: bash\njobs:\n  job-override:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: sh -e {0}\n    steps:\n      - name: job default\n        run: test -n \"$HOME\"\n    container: alpine:3.22\n  workflow-override:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: workflow default\n        run: test -n \"$HOME\"\n    container: alpine:3.22\n",
    )?;
    assert_eq!(
        runs,
        [
            ("test -n \"$HOME\"".to_owned(), ShellDialect::Sh),
            ("test -n \"$HOME\"".to_owned(), ShellDialect::Bash),
        ]
    );
    Ok(())
}

#[test]
fn only_step_run_keys_are_linted() -> Result<(), String> {
    let runs = bodies(
        "defaults:\n  run:\n    shell: bash\njobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: action\n        uses: example/action@0000000000000000000000000000000000000000\n        with:\n          run: action-input\n        env:\n          run: environment-value\n",
    )?;
    assert!(runs.is_empty());
    Ok(())
}

#[test]
fn unknown_runner_shell_and_shellcheck_overrides_fail_closed() {
    let unknown_runner = "jobs:\n  probe:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: test\n        run: echo probe\n";
    assert!(
        bodies(unknown_runner).is_err_and(|err| { err.contains("shell_unresolved_for_runner") })
    );

    let custom_shell = "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: test\n        run: echo probe\n        shell: bash --noprofile {0}\n";
    assert!(bodies(custom_shell).is_err_and(|err| { err.contains("shell_form_unsupported") }));

    for directive in [
        "# shellcheck shell=sh\ntrue",
        "#shellcheck shell=sh\ntrue",
        "#  shellcheck shell=sh\ntrue",
        "# SHELLCHECK disable=SC2086 SHELL=sh\ntrue",
        "# shellcheck shell = sh\ntrue",
    ] {
        let shellcheck_override = format!(
            "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: test\n        run: \"{}\"\n",
            directive.replace('\n', "\\n")
        );
        assert!(
            bodies(&shellcheck_override)
                .is_err_and(|err| { err.contains("shellcheck_shell_directive_unsupported") })
        );
    }
}

#[test]
fn nested_run_values_are_not_confused_with_steps() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  probe:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: action\n        uses: example/action@0000000000000000000000000000000000000000\n        with:\n          run: action-input\n        env:\n          run: environment-value\n",
    )?;
    assert!(runs.is_empty());
    Ok(())
}

#[test]
fn harmless_shellcheck_directives_remain_supported() -> Result<(), String> {
    for directive in ["# shellcheck disable=SC2086", "#shellcheck disable=SC2086"] {
        let workflow = format!(
            "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: test\n        run: \"{}\\necho probe\"\n",
            directive.replace('"', "\\\"")
        );
        assert_eq!(bodies(&workflow)?.len(), 1);
    }
    Ok(())
}

#[test]
fn unsupported_step_mapping_order_fails_closed() {
    let workflow = "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - run: echo probe\n        name: test\n";
    assert!(bodies(workflow).is_err_and(|err| { err.contains("step_name_must_be_first") }));
}

#[test]
fn run_alias_is_linted_with_each_uses_effective_shell() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  hosted:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: &r1 echo \"$HOME\"\n  scale:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: sh\n    steps:\n      - name: reuse\n        run: *r1\n",
    )?;
    assert_eq!(
        runs,
        [
            ("echo \"$HOME\"".to_owned(), ShellDialect::Bash),
            ("echo \"$HOME\"".to_owned(), ShellDialect::Sh),
        ]
    );
    Ok(())
}

#[test]
fn aliases_keep_use_site_environment_and_expression_contexts() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  hosted:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: &r1 printf '%s' \"$VALUE\"\n  scale:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: sh\n    steps:\n      - name: use alias with local expression context\n        env:\n          VALUE: ${{ vars.SCALE_VALUE }}\n        run: *r1\n  other-hosted:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: use alias with another expression context\n        env:\n          VALUE: ${{ vars.HOSTED_VALUE }}\n        run: *r1\n",
    )?;
    assert_eq!(
        runs,
        [
            ("printf '%s' \"$VALUE\"".to_owned(), ShellDialect::Bash),
            ("printf '%s' \"$VALUE\"".to_owned(), ShellDialect::Sh),
            ("printf '%s' \"$VALUE\"".to_owned(), ShellDialect::Bash),
        ]
    );
    Ok(())
}

#[test]
fn run_aliases_reject_forward_duplicate_and_out_of_scope_definitions() {
    let forward = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: reuse\n        run: *r1\n";
    assert!(bodies(forward).is_err_and(|error| error.contains("run_scalar_alias_unresolved")));

    let forward_then_defined = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: forward use\n        run: *r1\n      - name: later definition\n        run: &r1 echo safe\n";
    assert!(
        bodies(forward_then_defined)
            .is_err_and(|error| error.contains("run_scalar_alias_unresolved"))
    );

    let duplicate = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: duplicate\n        run: echo first\n        run: echo second\n";
    assert!(bodies(duplicate).is_err_and(|error| error.contains("duplicate_step_run")));

    let outside_scope = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: action\n        uses: example/action@0000000000000000000000000000000000000000\n        with:\n          run: &r1 echo ignored\n      - name: reuse\n        run: *r1\n";
    assert!(
        bodies(outside_scope).is_err_and(|error| error.contains("run_scalar_alias_unresolved"))
    );
}

#[test]
fn run_aliases_reject_malformed_and_duplicate_definitions() {
    let malformed = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: malformed alias\n        run: *bad.name\n";
    assert!(bodies(malformed).is_err_and(|error| error.contains("run_scalar_alias_malformed")));

    let malformed_anchor = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: malformed anchor\n        run: &bad.name echo ignored\n";
    assert!(
        bodies(malformed_anchor).is_err_and(|error| error.contains("run_scalar_anchor_malformed"))
    );

    let duplicate_definition = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: first definition\n        run: &r1 echo first\n      - name: duplicate definition\n        run: &r1 echo second\n";
    assert!(
        bodies(duplicate_definition)
            .is_err_and(|error| error.contains("run_scalar_anchor_duplicate"))
    );
}

#[test]
fn run_aliases_cannot_cross_workflow_files() -> Result<(), String> {
    let definition = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: definition\n        run: &r1 echo safe\n";
    let use_in_other_file = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: other workflow\n        run: *r1\n";
    assert_eq!(bodies(definition)?.len(), 1);
    assert!(
        bodies(use_in_other_file).is_err_and(|error| error.contains("run_scalar_alias_unresolved"))
    );
    Ok(())
}

#[test]
fn aliased_sc2086_violation_fails_the_real_shellcheck_gates() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workflow_path = ".github/workflows/ci.yml";
    let full_path = staging.path().join(workflow_path);
    std::fs::create_dir_all(full_path.parent().ok_or("workflow has no parent")?)
        .map_err(|error| error.to_string())?;
    std::fs::write(
        &full_path,
        "jobs:\n  hosted:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define unquoted expansion\n        run: &r1 echo $HOME\n  scale:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: sh\n    steps:\n      - name: alias under sh\n        run: *r1\n",
    )
    .map_err(|error| error.to_string())?;
    let error = run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .expect_err("aliased unquoted expansion must fail the targeted SC2086 pass");
    assert!(error.to_string().contains("SC2086"));
    Ok(())
}
