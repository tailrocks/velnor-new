use super::super::{run_shellcheck_bodies, workflow};
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
fn hosted_macos_26_uses_the_typed_bash_default() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  macos-26:\n    runs-on: macos-26\n    steps:\n      - name: test\n        run: echo probe\n  macos-15:\n    runs-on: macos-15\n    steps:\n      - name: test\n        run: echo probe\n",
    )?;
    assert_eq!(
        runs,
        [
            ("echo probe".to_owned(), ShellDialect::Bash),
            ("echo probe".to_owned(), ShellDialect::Bash),
        ]
    );
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

    let unknown_macos = "jobs:\n  probe:\n    runs-on: macos-27\n    steps:\n      - name: test\n        run: echo probe\n";
    assert!(
        bodies(unknown_macos).is_err_and(|err| { err.contains("shell_unresolved_for_runner") })
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
fn aliases_outside_run_fields_fail_the_complete_staged_validation_chain() {
    for workflow in [
        "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: &r1 echo safe\n      - name: alias in env\n        env:\n          COPY: *r1\n        run: echo safe\n",
        "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: echo safe\n      - name: tagged anchor in env\n        env:\n          COPY: !!str &outside safe\n          COPY2: *outside\n        run: echo safe\n",
        "jobs:\n  job:\n    env: {COPY: !!str &outside safe, COPY2: *outside}\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n",
        "jobs:\n  job:\n    runs-on: &runner ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n  second:\n    runs-on: *runner\n    steps:\n      - name: second command\n        run: echo safe\n",
    ] {
        let error = validate_generated_workflow(workflow)
            .expect_err("out-of-scope aliases must fail staged validation");
        assert!(error.contains("workflow_alias_outside_step_run"), "{error}");
    }
}

#[test]
fn tagged_run_scalars_fail_closed_before_shell_validation() {
    let tagged_anchor = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: tagged anchor\n        run: &r1 !!str \"echo $HOME\"\n      - name: alias\n        run: *r1\n";
    assert!(
        bodies(tagged_anchor).is_err_and(|error| error.contains("run_scalar_anchor_malformed"))
    );

    let tagged_alias = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: tagged alias\n        run: !!str *r1\n";
    assert!(bodies(tagged_alias).is_err_and(|error| error.contains("run_scalar_tag_unsupported")));
}

#[test]
fn run_aliases_are_checked_under_each_effective_shell() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workflow_path = ".github/workflows/ci.yml";
    let full_path = staging.path().join(workflow_path);
    std::fs::create_dir_all(full_path.parent().ok_or("workflow has no parent")?)
        .map_err(|error| error.to_string())?;
    std::fs::write(
        &full_path,
        r#"jobs:
  hosted:
    runs-on: ubuntu-26.04
    steps:
      - name: define bash array
        run: &r1 "paths=(\"$HOME\"); printf '%s' \"${paths[0]}\""
  scale:
    runs-on: [self-hosted, runner]
    defaults:
      run:
        shell: sh
    steps:
      - name: alias under sh
        run: *r1
"#,
    )
    .map_err(|error| error.to_string())?;
    let error = run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .expect_err("a bash-only run alias must fail at its sh use site");
    assert!(error.to_string().contains("SC3030"), "{error}");
    Ok(())
}

#[test]
fn run_aliases_pass_when_each_use_has_the_bash_shell() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workflow_path = ".github/workflows/ci.yml";
    let full_path = staging.path().join(workflow_path);
    std::fs::create_dir_all(full_path.parent().ok_or("workflow has no parent")?)
        .map_err(|error| error.to_string())?;
    std::fs::write(
        &full_path,
        r#"jobs:
  hosted:
    runs-on: ubuntu-26.04
    steps:
      - name: define bash array
        run: &r1 "paths=(\"$HOME\"); printf '%s' \"${paths[0]}\""
  second-hosted:
    runs-on: ubuntu-26.04
    steps:
      - name: reuse under bash
        run: *r1
"#,
    )
    .map_err(|error| error.to_string())?;
    run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_generated_workflow(body: &str) -> Result<(), String> {
    use velnor_actions_workflow_renderer::{
        marker::with_marker,
        render::{RenderedFile, RenderedTree},
    };

    let version = env!("CARGO_PKG_VERSION");
    let actionlint = with_marker(
        version,
        "config-variables: []\n\nself-hosted-runner:\n  labels:\n    - ubuntu-26.04\n",
    )
    .map_err(|error| error.to_string())?;
    let workflow_body = format!("on:\n  push:\npermissions:\n  contents: read\n{body}");
    let workflow = with_marker(version, &workflow_body).map_err(|error| error.to_string())?;
    let tree = RenderedTree {
        files: vec![
            RenderedFile {
                path: ".github/actionlint.yaml".to_owned(),
                bytes: actionlint,
            },
            RenderedFile {
                path: ".github/workflows/ci.yml".to_owned(),
                bytes: workflow,
            },
        ],
        symlinks: Vec::new(),
    };
    crate::validate::validate_staged(&tree)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
