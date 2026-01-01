use super::super::{run_shellcheck_bodies, workflow};
use super::{ShellDialect, scan_workflow};

fn bodies(workflow: &str) -> Result<Vec<(String, ShellDialect)>, String> {
    scan_workflow(workflow)
        .map(|runs| runs.into_iter().map(|run| (run.body, run.shell)).collect())
        .map_err(|err| err.to_string())
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
    assert!(bodies(outside_scope).is_err_and(|error| {
        error.contains("workflow_alias_outside_step_run")
            || error.contains("run_scalar_alias_unresolved")
    }));
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
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let first = staging.path().join("first.yml");
    let second = staging.path().join("second.yml");
    std::fs::write(&first, definition).map_err(|error| error.to_string())?;
    std::fs::write(&second, use_in_other_file).map_err(|error| error.to_string())?;
    assert!(
        workflow::staged_runs(
            staging.path(),
            &["first.yml".to_owned(), "second.yml".to_owned()]
        )
        .is_err_and(|error| error.to_string().contains("run_scalar_alias_unresolved"))
    );
    Ok(())
}

#[test]
fn aliases_outside_executable_run_scalars_fail_closed() {
    let env_alias = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: &r1 echo safe\n      - name: alias in env\n        env:\n          COPY: *r1\n        run: echo safe\n";
    let tagged_env_anchor = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: define\n        run: echo safe\n      - name: tagged anchor in env\n        env:\n          COPY: !!str &outside safe\n        run: echo safe\n";
    let tagged_flow_anchor = "jobs:\n  job:\n    env: {COPY: !!str &outside safe, COPY2: *outside}\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let runner_anchor = "jobs:\n  job:\n    runs-on: &runner ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n  second:\n    runs-on: *runner\n    steps:\n      - name: second command\n        run: echo safe\n";
    let flow_alias = "jobs:\n  job:\n    runs-on: [!!str &runner ubuntu-26.04, *runner]\n    steps:\n      - name: command\n        run: echo safe\n";
    let alias_key = "jobs:\n  job:\n    *runner: value\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let merge_key = "jobs:\n  job:\n    <<: {runs-on: ubuntu-26.04}\n    steps:\n      - name: command\n        run: echo safe\n";
    let flow_merge_key = "jobs:\n  job:\n    env: {<<: {COPY: safe}}\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";

    for workflow in [
        env_alias,
        tagged_env_anchor,
        tagged_flow_anchor,
        runner_anchor,
        flow_alias,
        alias_key,
        merge_key,
        flow_merge_key,
    ] {
        assert!(
            bodies(workflow).is_err_and(|error| {
                error.contains("workflow_alias_outside_step_run")
                    || error.contains("workflow_merge_key_unsupported")
            }),
            "accepted out-of-scope alias or merge key: {workflow}"
        );
    }
}

#[test]
fn renderer_mapping_and_step_aliases_preserve_every_run_use_site() -> Result<(), String> {
    let runs = bodies(
        r"jobs:
  hosted:
    runs-on: ubuntu-26.04
    env: &m1
      SHARED: value
    steps:
      - &s1
        name: shared command
        env: *m1
        run: echo $SHARED
      - *s1
      - name: step environment anchor
        env: &m3
          LOCAL: value
        run: echo $LOCAL
      - name: step environment alias
        env: *m3
        run: echo $LOCAL
  scale:
    runs-on: [self-hosted, runner]
    defaults:
      run:
        shell: sh
    steps:
      - *s1
      - name: action inputs
        uses: example/action@0000000000000000000000000000000000000000
        with: &m2
          value: shared
      - name: repeated action inputs
        uses: example/action@0000000000000000000000000000000000000000
        with: *m2
",
    )?;
    assert_eq!(
        runs,
        [
            ("echo $SHARED".to_owned(), ShellDialect::Bash),
            ("echo $SHARED".to_owned(), ShellDialect::Bash),
            ("echo $LOCAL".to_owned(), ShellDialect::Bash),
            ("echo $LOCAL".to_owned(), ShellDialect::Bash),
            ("echo $SHARED".to_owned(), ShellDialect::Sh),
        ]
    );
    Ok(())
}

#[test]
fn inline_empty_mapping_anchors_are_typed_and_resolved() -> Result<(), String> {
    let runs = bodies(
        "jobs:\n  first:\n    runs-on: ubuntu-26.04\n    env: &m1 {}\n    steps:\n      - name: command\n        run: echo safe\n  second:\n    runs-on: ubuntu-26.04\n    env: *m1\n    steps:\n      - name: command\n        run: echo safe\n",
    )?;
    assert_eq!(runs.len(), 2);
    Ok(())
}

#[test]
fn repeated_step_alias_commands_are_shellchecked_at_the_use_site() -> Result<(), String> {
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
      - &s1
        name: define bash array
        run: "paths=(\"$HOME\"); printf '%s' \"${paths[0]}\""
  scale:
    runs-on: [self-hosted, runner]
    defaults:
      run:
        shell: sh
    steps:
      - *s1
"#,
    )
    .map_err(|error| error.to_string())?;
    let error = run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .expect_err("an aliased bash-only step must be checked under the using job shell");
    assert!(error.to_string().contains("SC3030"), "{error}");
    Ok(())
}

#[test]
fn mapping_and_step_aliases_reject_wrong_scope_kind_and_order() {
    let forward_mapping = "jobs:\n  job:\n    env: *m1\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let scalar_as_mapping = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: scalar\n        run: &r1 echo safe\n      - name: wrong kind\n        env: *r1\n        run: echo safe\n";
    let mapping_as_step = "jobs:\n  job:\n    env: &m1\n      KEY: value\n    runs-on: ubuntu-26.04\n    steps:\n      - *m1\n";
    let step_as_mapping = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - &s1\n        name: command\n        run: echo safe\n      - name: wrong kind\n        env: *s1\n        run: echo safe\n";
    let unknown_step = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - *s1\n";
    let malformed_step = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - &bad.name\n        name: malformed\n        run: echo safe\n";
    let malformed_map = "jobs:\n  job:\n    env: &bad.name\n      KEY: value\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let empty_block_map = "jobs:\n  job:\n    env: &m1\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let duplicate_cross_kind = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: scalar\n        run: &x echo safe\n      - &x\n        name: duplicate map\n        run: echo safe\n";
    let cyclic_mapping = "jobs:\n  job:\n    env: &m1\n      COPY: *m1\n    runs-on: ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";
    let duplicate_mapping = "jobs:\n  job:\n    env: &m1\n      KEY: value\n    steps:\n      - name: duplicate\n        env: &m1\n          KEY: value\n        run: echo safe\n";
    let arbitrary_scope = "jobs:\n  job:\n    runs-on: &r1 ubuntu-26.04\n    steps:\n      - name: command\n        run: echo safe\n";

    for workflow in [
        forward_mapping,
        scalar_as_mapping,
        mapping_as_step,
        step_as_mapping,
        unknown_step,
        malformed_step,
        malformed_map,
        empty_block_map,
        duplicate_cross_kind,
        cyclic_mapping,
        duplicate_mapping,
        arbitrary_scope,
    ] {
        assert!(
            bodies(workflow).is_err(),
            "accepted unsupported or unresolved alias structure: {workflow}"
        );
    }
}

#[test]
fn literal_shell_globs_and_comments_are_not_yaml_aliases() -> Result<(), String> {
    let workflow = "jobs:\n  job:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: shell glob\n        env:\n          PATTERN: path[*]\n        run: echo [*r1] [&r1] # &comment\n";
    let scanned = bodies(workflow)?;
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].0, "echo [*r1] [&r1] # &comment");
    Ok(())
}
