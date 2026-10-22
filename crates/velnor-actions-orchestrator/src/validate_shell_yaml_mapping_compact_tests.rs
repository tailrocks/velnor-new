use super::super::super::super::run_shellcheck_bodies;
use super::super::super::scan_workflow;
use super::super::normalize;
use velnor_actions_mise::ToolCatalog;

const CANONICAL: &str = concat!(
    "jobs:\n",
    "  hosted:\n",
    "    runs-on: ubuntu-26.04\n",
    "    env: &jobenv\n",
    "      JOB: safe\n",
    "    steps:\n",
    "      - &shared\n",
    "        name: shared script\n",
    "        env: &stepenv\n",
    "          STEP: safe\n",
    "        run: &script \"set -eu\\nprintf safe\"\n",
    "      - name: action one\n",
    "        uses: example/action@0000000000000000000000000000000000000000\n",
    "        with: &inputs\n",
    "          value: safe\n",
    "  container:\n",
    "    runs-on: ubuntu-26.04\n",
    "    container: alpine:3.24.2\n",
    "    env: *jobenv\n",
    "    defaults:\n",
    "      run:\n",
    "        shell: sh\n",
    "    steps:\n",
    "      - *shared\n",
    "      - name: action two\n",
    "        uses: example/action@0000000000000000000000000000000000000000\n",
    "        with: *inputs\n",
);

const COMPACT: &str = concat!(
    "jobs:\n",
    " hosted:\n",
    "  runs-on: ubuntu-26.04\n",
    "  env: &jobenv\n",
    "   JOB: safe\n",
    "  steps:\n",
    "   - &shared\n",
    "    name: shared script\n",
    "    env: &stepenv\n",
    "     STEP: safe\n",
    "    run: &script \"set -eu\\nprintf safe\"\n",
    "   -\n",
    "    name: action one\n",
    "    uses: example/action@0000000000000000000000000000000000000000\n",
    "    with: &inputs\n",
    "     value: safe\n",
    " container:\n",
    "  runs-on: ubuntu-26.04\n",
    "  container: alpine:3.24.2\n",
    "  env: *jobenv\n",
    "  defaults:\n",
    "   run:\n",
    "    shell: sh\n",
    "  steps:\n",
    "   - *shared\n",
    "   -\n",
    "    name: action two\n",
    "    uses: example/action@0000000000000000000000000000000000000000\n",
    "    with: *inputs\n",
);

#[test]
fn compact_full_aliases_preserve_ordered_run_bodies_and_use_site_shells() {
    let expected = scan_workflow(CANONICAL).expect("canonical document");
    let compact = scan_workflow(COMPACT).expect("compact document");
    assert_eq!(compact, expected);
    assert_eq!(compact.len(), 2);
    assert_eq!(compact[0].body, "set -eu\nprintf safe");
    assert_eq!(compact[0].shell, super::super::super::ShellDialect::Bash);
    assert_eq!(compact[1].body, "set -eu\nprintf safe");
    assert_eq!(compact[1].shell, super::super::super::ShellDialect::Sh);
}

#[test]
fn compact_normalization_emits_canonical_step_indentation_and_is_idempotent() {
    let normalized = normalize(COMPACT).expect("compact input normalizes");
    assert!(normalized.contains("      - name: action one\n"));
    assert!(!normalized.contains("\n   -\n"));
    assert_eq!(
        normalize(&normalized).expect("canonical input normalizes"),
        normalized
    );
}

#[test]
fn compact_literal_run_body_keeps_relative_indentation_and_script_bytes() {
    let canonical = concat!(
        "jobs:\n  linux:\n    runs-on: ubuntu-26.04\n    steps:\n",
        "      - name: block\n        run: |\n",
        "          echo safe\n            printf 'done\\n'\n"
    );
    let compact = concat!(
        "jobs:\n linux:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   -\n    name: block\n    run: |\n",
        "      echo safe\n        printf 'done\\n'\n"
    );
    let expected = scan_workflow(canonical).expect("canonical literal block");
    let actual = scan_workflow(compact).expect("compact literal block");
    assert_eq!(actual, expected);
    assert_eq!(actual[0].body, "echo safe\n  printf 'done\\n'\n");
}

#[test]
fn compact_mixed_step_ancestry_fails_closed() {
    let mixed = concat!(
        "jobs:\n job:\n  runs-on: ubuntu-26.04\n   steps:\n",
        "    -\n     name: run\n     run: echo safe\n"
    );
    assert!(normalize(mixed).is_err());
}

#[test]
fn compact_aliases_still_reject_forward_cycles_and_duplicate_names() {
    let forward = concat!(
        "jobs:\n job:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   - *later\n   - &later\n    name: later\n    run: echo safe\n"
    );
    let cycle = concat!(
        "jobs:\n job:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   - &self\n    name: self\n    env: *self\n    run: echo safe\n"
    );
    let duplicate = concat!(
        "jobs:\n job:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   - &same\n    name: first\n    run: echo safe\n",
        "   - &same\n    name: second\n    run: echo safe\n"
    );
    let cross_kind = concat!(
        "jobs:\n job:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   -\n    name: first\n    run: &same echo safe\n",
        "   -\n    name: second\n    env: &same\n     VALUE: safe\n",
        "    run: true\n"
    );
    for text in [forward, cycle, duplicate, cross_kind] {
        assert!(scan_workflow(text).is_err());
    }
}

#[test]
fn compact_whole_step_alias_keeps_the_use_site_shell_for_shellcheck() -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = "compact-step.yml";
    let text = concat!(
        "jobs:\n hosted:\n  runs-on: ubuntu-26.04\n  steps:\n",
        "   - &shared\n    name: bash array\n",
        "    run: \"paths=(safe); printf '%s' \\\"${paths[0]}\\\"\"\n",
        " container:\n  runs-on: ubuntu-26.04\n  container: alpine:3.24.2\n",
        "  steps:\n   - *shared\n"
    );
    std::fs::write(staging.path().join(path), text).map_err(|error| error.to_string())?;
    let error = run_shellcheck_bodies(&ToolCatalog::pinned(), staging.path(), &[path.to_owned()])
        .expect_err("the shared Bash step must be linted under the container's `sh`");
    assert!(error.to_string().contains("SC3030"), "{error}");
    Ok(())
}

#[test]
fn compact_expansion_keeps_the_existing_byte_limit() {
    let mut text = String::from("jobs:\n job:\n  runs-on: ubuntu-26.04\n  steps:\n   - &shared\n");
    text.push_str("    name: large\n    run: ");
    text.push_str(&"x".repeat(120_000));
    text.push('\n');
    text.push_str(&"   - *shared\n".repeat(150));
    assert!(text.len() < super::super::MAX_INPUT);
    assert!(normalize(&text).is_err_and(|error| {
        error
            .to_string()
            .contains("workflow_mapping_expansion_limit")
    }));
}

#[test]
fn compact_indentation_depth_remains_bounded() {
    let mut text = String::from("jobs:\n job:\n  env:\n");
    for depth in 3..=super::MAX_DEPTH + 1 {
        text.push_str(&" ".repeat(depth));
        text.push_str("nested:\n");
    }
    assert!(normalize(&text).is_err());
}
