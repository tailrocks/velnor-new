//! Anchor resolution and malformed-layout regression tests for workflow parsing.

use super::{semantic, step_lists};

fn anchored_workflow(compact: bool) -> String {
    if compact {
        concat!(
            "jobs:\n",
            " a:\n",
            "  steps:\n",
            "   - &install\n",
            "    name: Install pinned Rust\n",
            "    run: &install_run mise install rust@1.99.1\n",
            "   -\n",
            "    name: Verify Rust channel manifest\n",
            "    uses: ./.github/actions/velnor-verify-rust-manifest ",
            "# zizmor: ignore[self-repository]\n",
            "   - *install\n",
            "   -\n",
            "    name: Shared environment\n",
            "    env: &shared_env\n",
            "     SAFE: \"1\"\n",
            "    run: &shared_run echo ready\n",
            "   -\n",
            "    name: Shared environment alias\n",
            "    env: *shared_env\n",
            "    run: *shared_run\n",
        )
        .to_owned()
    } else {
        concat!(
            "jobs:\n",
            "  a:\n",
            "    steps:\n",
            "      - &install\n",
            "        name: Install pinned Rust\n",
            "        run: &install_run mise install rust@1.99.1\n",
            "      - name: Verify Rust channel manifest\n",
            "        uses: ./.github/actions/velnor-verify-rust-manifest ",
            "# zizmor: ignore[self-repository]\n",
            "      - *install\n",
            "      - name: Shared environment\n",
            "        env: &shared_env\n",
            "          SAFE: \"1\"\n",
            "        run: &shared_run echo ready\n",
            "      - name: Shared environment alias\n",
            "        env: *shared_env\n",
            "        run: *shared_run\n",
        )
        .to_owned()
    }
}

#[test]
fn shared_step_environment_and_run_anchors_resolve_in_both_layouts() {
    let standard = step_lists(&anchored_workflow(false));
    let compact = step_lists(&anchored_workflow(true));
    assert_eq!(standard.len(), 1);
    assert_eq!(compact.len(), 1);
    assert_eq!(semantic(&standard[0].1), semantic(&compact[0].1));

    let steps = &compact[0].1;
    assert_eq!(steps.len(), 5);
    assert_eq!(steps[2].name, "Install pinned Rust");
    assert_eq!(
        steps[2].run_scalar.as_deref(),
        Some("mise install rust@1.99.1")
    );
    assert_eq!(
        steps[3].env_fields,
        [("SAFE".to_owned(), "\"1\"".to_owned())]
    );
    assert_eq!(steps[4].env_fields, steps[3].env_fields);
    assert_eq!(steps[4].run_scalar.as_deref(), Some("echo ready"));
    assert!(steps.iter().all(|step| !step.unexpected_nested));
}

#[test]
fn compact_duplicate_fields_and_unexpected_nesting_remain_visible() {
    let duplicate = concat!(
        "jobs:\n a:\n  steps:\n   -\n",
        "    name: First\n    name: Second\n    env:\n",
        "     SAFE: one\n     SAFE: two\n",
    );
    let steps = step_lists(duplicate);
    let step = &steps[0].1[0];
    assert_eq!(
        step.fields.iter().filter(|(key, _)| key == "name").count(),
        2
    );
    assert_eq!(step.env_fields.len(), 2);

    let malformed = concat!(
        "jobs:\n a:\n  steps:\n   -\n",
        "      name: Bad indentation\n",
    );
    let steps = step_lists(malformed);
    assert!(steps[0].1[0].unexpected_nested);
    assert_eq!(steps[0].1[0].fields, Vec::<(String, String)>::new());

    let mixed_width = concat!(
        "jobs:\n a:\n  steps:\n   - &step\n",
        "     name: Anchored\n    run: echo invalid-width\n",
    );
    let steps = step_lists(mixed_width);
    assert_eq!(steps[0].1[0].name, "Anchored");
    assert!(steps[0].1[0].unexpected_nested);
}
