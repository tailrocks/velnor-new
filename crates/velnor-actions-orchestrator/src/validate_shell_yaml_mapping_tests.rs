use super::super::super::run_shellcheck_bodies;
use super::super::{ShellDialect, scan_workflow};
use super::*;

const PREFIX: &str = "jobs:\n  first:\n    runs-on: ubuntu-26.04\n    steps:\n";

#[test]
fn mappings_and_whole_steps_expand_without_changing_use_site_shell() {
    let text = concat!(
        "jobs:\n  first:\n    runs-on: ubuntu-26.04\n    env: &m1\n      VALUE: '${{ github.job }}'\n",
        "    steps:\n      - &s1\n        name: shared\n        env: *m1\n        run: &r1 echo safe\n",
        "      - name: action\n        uses: example/action@0000000000000000000000000000000000000000\n",
        "        with: &m2\n          input: value\n",
        "  second:\n    runs-on: ubuntu-26.04\n    env: *m1\n    defaults:\n      run:\n        shell: sh\n",
        "    steps:\n      - *s1\n      - name: inputs\n        with: *m2\n        run: *r1\n"
    );
    let runs = scan_workflow(text).expect("closed aliases");
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].shell, ShellDialect::Bash);
    assert!(runs[1..].iter().all(|run| run.shell == ShellDialect::Sh));
    assert!(runs.iter().all(|run| run.body == "echo safe"));
    let normalized = normalize(text).expect("normalize");
    assert_eq!(normalized.matches("VALUE: '${{ github.job }}'").count(), 4);
    assert!(!normalized.contains("&m1"));
    assert_eq!(normalized, normalize(&normalized).expect("idempotent"));
}

#[test]
fn aliased_step_shell_is_still_admitted_by_the_existing_scanner() {
    let unsupported = format!(
        "{PREFIX}      - &s1\n        name: shared\n        run: echo safe\n        shell: python\n      - *s1\n"
    );
    assert!(
        scan_workflow(&unsupported)
            .is_err_and(|error| error.to_string().contains("shell_form_unsupported"))
    );
    let supported = unsupported.replace("shell: python", "shell: sh");
    let runs = scan_workflow(&supported).expect("explicit shared shell");
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| run.shell == ShellDialect::Sh));
}

#[test]
fn literal_run_blocks_preserve_body_indentation_spaces_and_clip_newline() {
    let text = format!(
        "{PREFIX}      - name: multiline\n        run: |\n          printf '%s' \"$VALUE\"  \n            printf 'done\\n'\n          \n      - name: following\n        run: echo next\n"
    );
    let expected = [
        (
            "printf '%s' \"$VALUE\"  \n  printf 'done\\n'\n",
            ShellDialect::Bash,
        ),
        ("echo next", ShellDialect::Bash),
    ];
    let runs = scan_workflow(&text).expect("The parser rejected a supported run block.");
    assert_eq!(
        runs.iter()
            .map(|run| (run.body.as_str(), run.shell))
            .collect::<Vec<_>>(),
        expected
    );

    let normalized = normalize(&text).expect("The test could not normalize the run block.");
    assert_eq!(
        scan_workflow(&normalized)
            .expect("The parser rejected the normalized run block.")
            .iter()
            .map(|run| run.body.clone())
            .collect::<Vec<_>>(),
        [expected[0].0.to_owned(), expected[1].0.to_owned()]
    );
    assert_eq!(
        normalized,
        normalize(&normalized).expect("The test could not normalize the text again.")
    );
}

#[test]
fn literal_run_blocks_preserve_indented_blank_content_and_clip_controls() {
    let text = format!(
        "{PREFIX}      - name: heredoc\n        run: |\n        \n          cat <<EOF\n            \n          EOF\n          \n      - name: following\n        run: echo next\n"
    );
    let runs = scan_workflow(&text).expect("The parser rejected the run block with blank lines.");
    assert_eq!(runs[0].body, "\ncat <<EOF\n  \nEOF\n");
    assert_eq!(runs[1].body, "echo next");

    let normalized = normalize(&text).expect("The test could not normalize the workflow.");
    assert_eq!(
        scan_workflow(&normalized).expect("The parser rejected the normalized workflow.")[0].body,
        runs[0].body
    );
    assert_eq!(
        normalized,
        normalize(&normalized).expect("The test could not normalize the workflow again.")
    );

    let trailing_content = format!(
        "{PREFIX}      - name: trailing whitespace\n        run: |\n          echo safe\n            \n"
    );
    assert_eq!(
        scan_workflow(&trailing_content)
            .expect("The parser rejected trailing content with additional indentation.")[0]
            .body,
        "echo safe\n  \n"
    );
}

#[test]
fn unsupported_literal_chomping_indentation_and_folded_blocks_remain_rejected() {
    for indicator in ["|-", "|+", "|2", ">", ">-"] {
        let text =
            format!("{PREFIX}      - name: block\n        run: {indicator}\n          echo safe\n");
        assert!(
            scan_workflow(&text)
                .is_err_and(|error| error.to_string().contains("run_block_scalar_unlintable")),
            "{indicator}"
        );
    }
}

#[test]
fn aliases_reject_forward_self_duplicate_cross_kind_and_cross_file_names() {
    for body in [
        "      - *s1\n      - &s1\n        name: later\n        run: true\n",
        "      - &s1\n        name: self\n        env: *s1\n        run: true\n",
        "      - &s1\n        name: first\n        run: true\n      - &s1\n        name: second\n        run: true\n",
        "      - name: first\n        run: &same echo safe\n      - name: second\n        env: &same\n          VALUE: safe\n        run: true\n",
        "      - name: first\n        env: &same\n          VALUE: safe\n        run: &same echo safe\n",
        "      - name: first\n        env: &m1\n          VALUE: safe\n      - *m1\n",
        "      - name: first\n        run: *missing\n",
    ] {
        assert!(scan_workflow(&format!("{PREFIX}{body}")).is_err(), "{body}");
    }
    normalize(&format!(
        "{PREFIX}      - &s1\n        name: defined\n        run: true\n"
    ))
    .expect("definition");
    assert!(normalize(&format!("{PREFIX}      - *s1\n")).is_err());
}

#[test]
fn mapping_aliases_reject_unsupported_scope_shape_properties_and_children() {
    for body in [
        "      - &s1\n        run: true\n        name: wrong order\n",
        "      - &s1\n        name: first\n        name: duplicate\n        run: true\n",
        "      - &s1\n        name: first\n        parallel: value\n",
        "      - &s1\n        name: !!str tagged\n        run: true\n",
        "      - name: first\n        env: &m1\n          VALUE:\n            nested: value\n",
        "      - name: first\n        env: &m1\n          VALUE: safe\n          VALUE: duplicate\n",
        "      - name: first\n        env: &m1\n          VALUE: !!str safe\n",
        "      - name: first\n        env: &m1\n          <<: {VALUE: safe}\n",
        "      - name: first\n        env: &m1\n          VALUE: safe\n      - name: second\n        env: *m1\n          EXTRA: child\n",
        "      - name: first\n        env: &m1\n          VALUE: safe\n        shell: *m1\n",
        "      - !!map &s1\n        name: tagged\n        run: true\n",
    ] {
        assert!(scan_workflow(&format!("{PREFIX}{body}")).is_err(), "{body}");
    }
    for key in [
        "runs-on",
        "needs",
        "permissions",
        "outputs",
        "defaults",
        "steps",
    ] {
        let text = format!("jobs:\n  job:\n    {key}: &m1\n      VALUE: safe\n");
        assert!(scan_workflow(&text).is_err(), "{key}");
    }
}

#[test]
fn input_depth_nodes_and_expansion_are_bounded() {
    assert!(normalize(&"x".repeat(MAX_INPUT + 1)).is_err());
    assert!(normalize(&format!("{}value: safe\n", " ".repeat(66))).is_err());
    assert!(normalize(&"\n".repeat(MAX_NODES + 1)).is_err());
    let mut text = format!(
        "{PREFIX}      - &s1\n        name: large\n        run: {}\n",
        "x".repeat(120_000)
    );
    text.push_str(&"      - *s1\n".repeat(150));
    assert!(text.len() < MAX_INPUT);
    assert!(normalize(&text).is_err_and(|error| error.to_string().contains("expansion_limit")));
}

#[cfg(all(test, feature = "test-render-capture"))]
#[test]
fn equivalence_scanner_has_a_separate_hard_input_ceiling() {
    let text = format!("value: \"{}\"\n", "x".repeat(MAX_EQUIVALENCE_INPUT));
    assert!(normalize_for_equivalence(&text).is_err());
}

#[test]
fn whole_step_aliases_cannot_hide_bash_code_from_posix_shellcheck() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workflow_path = "aliased-step.yml";
    let text = concat!(
        "jobs:\n  hosted:\n    runs-on: ubuntu-26.04\n    steps:\n",
        "      - &s1\n        name: shared bash array\n",
        "        run: &r1 \"paths=(safe); printf '%s' \\\"${paths[0]}\\\"\"\n",
        "  container:\n    runs-on: ubuntu-26.04\n    container: alpine:3.24.2\n",
        "    steps:\n      - *s1\n"
    );
    std::fs::write(staging.path().join(workflow_path), text).map_err(|error| error.to_string())?;
    let error = run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .expect_err("the aliased executable must be checked under its actual sh shell");
    assert!(error.to_string().contains("SC3030"), "{error}");
    Ok(())
}

#[test]
fn literal_run_blocks_are_decoded_and_linted_as_shell_scripts() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workflow_path = "literal-run-block.yml";
    let clean = format!(
        "{PREFIX}      - name: multiline shell\n        run: |\n          python3 - <<'PY'\n            print('safe')\n          PY\n"
    );
    std::fs::write(staging.path().join(workflow_path), &clean)
        .map_err(|error| error.to_string())?;
    let runs = super::super::staged_runs(staging.path(), &[workflow_path.to_owned()])
        .map_err(|error| error.to_string())?;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].body, "python3 - <<'PY'\n  print('safe')\nPY\n");
    run_shellcheck_bodies(
        &velnor_actions_mise::ToolCatalog::pinned(),
        staging.path(),
        &[workflow_path.to_owned()],
    )
    .map_err(|error| error.to_string())?;

    let bad = format!(
        "{PREFIX}      - name: unquoted expansion\n        run: |\n          echo $VALUE\n"
    );
    std::fs::write(staging.path().join(workflow_path), bad).map_err(|error| error.to_string())?;
    assert!(
        run_shellcheck_bodies(
            &velnor_actions_mise::ToolCatalog::pinned(),
            staging.path(),
            &[workflow_path.to_owned()],
        )
        .is_err_and(|error| error.to_string().contains("SC2086"))
    );
    Ok(())
}
