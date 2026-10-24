use super::{audit_workflow, inline_step, inline_workflow, step_lists, verifiers};
use crate::workflow::verifier_actions::InlineVerifierTemplate;
use velnor_actions_workflow_renderer::yaml::quote_scalar;

#[test]
fn inline_single_line_run_rejects_hidden_nested_fields_in_both_layouts() {
    let verifiers = verifiers();
    for compact in [false, true] {
        let template = verifiers
            .inline_installed_rust_templates
            .last()
            .expect("installed Rust inline verifier");
        let canonical = inline_step("Verify Rust channel manifest", template, compact);
        assert_eq!(
            audit_workflow(inline_workflow(&canonical, compact), &verifiers),
            ""
        );

        let continuation_indent = if compact { "     " } else { "          " };
        let forged = format!("{canonical}{continuation_indent}id: forged\n");
        let found = audit_workflow(inline_workflow(&forged, compact), &verifiers);
        assert!(
            found.contains("noncanonical verifier call or body"),
            "single-line run must not absorb a hidden field: {found}"
        );
    }
}

#[test]
fn block_scalar_run_body_is_collected_but_plain_run_cannot_hide_fields() {
    let block = concat!(
        "jobs:\n a:\n  steps:\n   -\n",
        "    name: Block body\n    run: |\n",
        "     echo first\n     echo second\n",
    );
    let steps = step_lists(block);
    assert_eq!(
        steps[0].1[0].run_scalar.as_deref(),
        Some("echo first\necho second\n")
    );
    assert!(!steps[0].1[0].unexpected_nested);

    let plain = concat!(
        "jobs:\n a:\n  steps:\n   -\n",
        "    name: Plain scalar\n    run: echo first\n",
        "     id: forged\n",
    );
    assert!(step_lists(plain)[0].1[0].unexpected_nested);
}

fn inline_env_alias_workflow(
    template: &InlineVerifierTemplate,
    compact: bool,
    extra_child: bool,
) -> String {
    let child_indent = if compact { "     " } else { "          " };
    let field_indent = if compact { "    " } else { "        " };
    let env_fields = template
        .env
        .iter()
        .map(|(key, value)| format!("{child_indent}{key}: {}", quote_scalar(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let extra = if extra_child {
        format!("{child_indent}FORGED: true\n")
    } else {
        String::new()
    };
    let shell = quote_scalar(template.shell.expect("canonical shell"));
    if compact {
        format!(
            "jobs:\n template:\n  steps:\n   -\n    name: Environment source\n    env: &expected_env\n{env_fields}\n build:\n  steps:\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n   -\n    name: Verify Rust channel manifest\n    env: *expected_env\n{extra}{field_indent}shell: {shell}\n{field_indent}run: {}\n   -\n    name: Build\n    run: mise exec rust@1.99.1 -- cargo build\n",
            template.run_scalar
        )
    } else {
        format!(
            "jobs:\n  template:\n    steps:\n      - name: Environment source\n        env: &expected_env\n{env_fields}\n  build:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n      - name: Verify Rust channel manifest\n        env: *expected_env\n{extra}{field_indent}shell: {shell}\n{field_indent}run: {}\n      - name: Build\n        run: mise exec rust@1.99.1 -- cargo build\n",
            template.run_scalar
        )
    }
}

#[test]
fn env_aliases_reject_additional_children_in_both_layouts() {
    let verifiers = verifiers();
    let template = verifiers
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let canonical = inline_env_alias_workflow(template, compact, false);
        assert_eq!(audit_workflow(canonical, &verifiers), "");

        let forged = inline_env_alias_workflow(template, compact, true);
        let steps = step_lists(&forged);
        let verifier = &steps[1].1[1];
        assert!(verifier.invalid_anchor);
        assert!(verifier.unexpected_nested);
        let found = audit_workflow(forged, &verifiers);
        assert!(
            found.contains("rust_install_unverified:.github/workflows/ci.yml:build:Verify Rust channel manifest:invalid YAML anchor declaration or alias"),
            "an alias must not hide additional environment keys: {found}"
        );
    }
}

fn action_alias_workflow(compact: bool, extra: &str) -> String {
    if compact {
        format!(
            "jobs:\n template:\n  steps:\n   - &verify\n    name: Verify Rust channel manifest\n    uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n build:\n  steps:\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n   - *verify\n{extra}   -\n    name: Build\n    run: mise exec rust@1.99.1 -- cargo build\n"
        )
    } else {
        format!(
            "jobs:\n  template:\n    steps:\n      - &verify\n        name: Verify Rust channel manifest\n        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n  build:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n      - *verify\n{extra}      - name: Build\n        run: mise exec rust@1.99.1 -- cargo build\n"
        )
    }
}

#[test]
fn whole_step_aliases_reject_hidden_fields_in_both_layouts() {
    let verifiers = verifiers();
    for compact in [false, true] {
        assert_eq!(
            audit_workflow(action_alias_workflow(compact, ""), &verifiers),
            ""
        );
        let trailing =
            action_alias_workflow(compact, "").replace("- *verify\n", "- *verify trailing\n");
        assert!(
            audit_workflow(trailing, &verifiers)
                .contains("invalid YAML anchor declaration or alias")
        );
        let (id, with) = if compact {
            ("    id: forged\n", "    with:\n     phase: installed\n")
        } else {
            (
                "        id: forged\n",
                "        with:\n          phase: installed\n",
            )
        };
        for extra in [id, with] {
            let found = audit_workflow(action_alias_workflow(compact, extra), &verifiers);
            assert!(
                found.contains("invalid YAML anchor declaration or alias"),
                "a whole-step alias must not hide extra fields: {found}"
            );
        }
    }
}

fn anchor_alias_cases(compact: bool) -> [&'static str; 2] {
    if compact {
        [
            "jobs:\n build:\n  steps:\n   -\n    name: Map source\n    env: &shared\n     SAFE: yes\n   -\n    name: Scalar collision\n    run: &shared echo harmless\n   -\n    name: Verify Rust channel manifest\n    env: *shared\n",
            "jobs:\n build:\n  steps:\n   -\n    name: Map source\n    env: &map_only\n     SAFE: yes\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n   -\n    name: Verify Rust channel manifest\n    run: *map_only\n",
        ]
    } else {
        [
            "jobs:\n  build:\n    steps:\n      - name: Map source\n        env: &shared\n          SAFE: yes\n      - name: Scalar collision\n        run: &shared echo harmless\n      - name: Verify Rust channel manifest\n        env: *shared\n",
            "jobs:\n  build:\n    steps:\n      - name: Map source\n        env: &map_only\n          SAFE: yes\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n      - name: Verify Rust channel manifest\n        run: *map_only\n",
        ]
    }
}

#[test]
fn cross_kind_duplicates_and_wrong_kind_aliases_are_tainted_in_both_layouts() {
    let verifiers = verifiers();
    for compact in [false, true] {
        for workflow in anchor_alias_cases(compact) {
            let steps = step_lists(workflow);
            assert!(
                steps
                    .iter()
                    .flat_map(|(_, steps)| steps)
                    .any(|step| step.invalid_anchor)
            );
            let problem = audit_workflow(workflow.to_owned(), &verifiers);
            assert!(
                problem.contains("invalid YAML anchor declaration or alias"),
                "anchor ambiguity must fail before install classification: {problem}"
            );
        }
    }
}

#[test]
fn duplicate_anchor_outside_steps_and_unknown_aliases_fail_closed() {
    let verifiers = verifiers();
    for outside_duplicate in [
        concat!(
            "defaults: &shared value\n",
            "jobs:\n build:\n  steps:\n   -\n",
            "    name: Duplicate declaration\n    run: &shared harmless\n",
        ),
        concat!(
            "defaults: &shared value\n",
            "jobs:\n  build:\n    steps:\n      - name: Duplicate declaration\n",
            "        run: &shared harmless\n",
        ),
    ] {
        assert!(
            audit_workflow(outside_duplicate.to_owned(), &verifiers)
                .contains("invalid YAML anchor declaration or alias")
        );
    }

    let unknown = concat!(
        "jobs:\n build:\n  steps:\n   -\n",
        "    name: Verify Rust channel manifest\n    env: *missing\n",
    );
    assert!(
        audit_workflow(unknown.to_owned(), &verifiers)
            .contains("invalid YAML anchor declaration or alias")
    );

    let invalid_name = concat!(
        "jobs:\n build:\n  steps:\n   -\n",
        "    name: Verify Rust channel manifest\n    run: *bad.name\n",
    );
    assert!(
        audit_workflow(invalid_name.to_owned(), &verifiers)
            .contains("invalid YAML anchor declaration or alias")
    );

    let wrong_kind_env = concat!(
        "jobs:\n build:\n  steps:\n   -\n",
        "    name: Scalar source\n    run: &scalar echo harmless\n",
        "   -\n    name: Verify Rust channel manifest\n    env: *scalar\n",
    );
    assert!(
        audit_workflow(wrong_kind_env.to_owned(), &verifiers)
            .contains("invalid YAML anchor declaration or alias")
    );

    let wrong_kind_step = concat!(
        "jobs:\n build:\n  steps:\n   -\n",
        "    name: Map source\n    env: &map_only\n     SAFE: yes\n",
        "   - *map_only\n",
    );
    assert!(
        audit_workflow(wrong_kind_step.to_owned(), &verifiers)
            .contains("invalid YAML anchor declaration or alias")
    );
}

#[test]
fn inline_sequence_block_body_does_not_hide_sibling_anchor_declarations() {
    let verifiers = verifiers();
    let compact = concat!(
        "defaults:\n source: &shared value\n",
        "jobs:\n build:\n  steps:\n",
        "   - run: |\n       echo harmless\n",
        "     name: &shared sibling\n",
        "   -\n    name: Verify Rust channel manifest\n",
        "    env: *shared\n",
    );
    let standard = concat!(
        "defaults:\n  source: &shared value\n",
        "jobs:\n  build:\n    steps:\n",
        "      - run: |\n          echo harmless\n",
        "        name: &shared sibling\n",
        "      -\n        name: Verify Rust channel manifest\n",
        "        env: *shared\n",
    );
    for workflow in [compact, standard] {
        let steps = step_lists(workflow);
        assert!(
            steps
                .iter()
                .flat_map(|(_, steps)| steps)
                .all(|step| step.invalid_anchor)
        );
        assert!(
            audit_workflow(workflow.to_owned(), &verifiers)
                .contains("invalid YAML anchor declaration or alias")
        );
    }

    let block_body = concat!(
        "jobs:\n build:\n  steps:\n",
        "   - run: |\n       &shared harmless block text\n",
        "     name: Block body\n",
        "   - &shared\n    name: Valid anchor\n",
    );
    let steps = step_lists(block_body);
    assert!(steps[0].1.iter().all(|step| !step.invalid_anchor));
    assert_eq!(audit_workflow(block_body.to_owned(), &verifiers), "");
}

#[test]
fn flow_and_doubled_quote_declarations_share_the_document_anchor_namespace() {
    for compact in [false, true] {
        let step = if compact {
            "jobs:\n build:\n  steps:\n   - &shared\n    name: Safe\n"
        } else {
            "jobs:\n  build:\n    steps:\n      - &shared\n        name: Safe\n"
        };
        let flow = format!("extra: [{{key: &shared value}}]\n{step}");
        assert!(step_lists(&flow)[0].1[0].invalid_anchor);

        let quoted = format!("extra: \"{{key: &shared value}}\"\n{step}");
        assert!(!step_lists(&quoted)[0].1[0].invalid_anchor);
    }

    let doubled_quote = concat!(
        "defaults:\n",
        "  'a''b # c': &shared first\n",
        "jobs:\n  build:\n    steps:\n",
        "      - &shared\n        name: Safe\n",
    );
    assert!(step_lists(doubled_quote)[0].1[0].invalid_anchor);
}

#[test]
fn tainted_whole_step_alias_cannot_hide_an_install() {
    let verifiers = verifiers();
    for compact in [false, true] {
        let externally_anchored_install = if compact {
            concat!(
                "templates:\n install: &install\n",
                "  name: Install pinned Rust\n  run: mise install rust@1.99.1\n",
                "jobs:\n build:\n  steps:\n   - *install\n",
            )
        } else {
            concat!(
                "templates:\n  install: &install\n",
                "    name: Install pinned Rust\n    run: mise install rust@1.99.1\n",
                "jobs:\n  build:\n    steps:\n      - *install\n",
            )
        };
        let aliased = step_lists(externally_anchored_install);
        assert!(aliased[0].1[0].invalid_anchor);
        assert!(
            audit_workflow(externally_anchored_install.to_owned(), &verifiers)
                .contains("invalid YAML anchor declaration or alias")
        );

        let workflow = if compact {
            concat!(
                "jobs:\n build:\n  steps:\n   - &install\n",
                "    name: Install pinned Rust\n    run: mise install rust@1.99.1\n",
                "   - &install\n    name: Ordinary replacement\n    run: echo safe\n",
                "   - *install\n",
            )
        } else {
            concat!(
                "jobs:\n  build:\n    steps:\n      - &install\n",
                "        name: Install pinned Rust\n        run: mise install rust@1.99.1\n",
                "      - &install\n        name: Ordinary replacement\n        run: echo safe\n",
                "      - *install\n",
            )
        };
        let steps = step_lists(workflow);
        assert!(steps[0].1[0].invalid_anchor);
        assert!(steps[0].1[2].invalid_anchor);
        let problem = audit_workflow(workflow.to_owned(), &verifiers);
        assert!(
            problem.contains("invalid YAML anchor declaration or alias"),
            "tainted install alias cannot disappear as an ordinary step: {problem}"
        );
    }
}

#[test]
fn quoted_comment_and_block_text_do_not_create_anchor_duplicates() {
    let workflow = concat!(
        "jobs:\n  build:\n    steps:\n",
        "      - &safe\n        name: Safe anchor\n",
        "        env: &environment\n          TEXT: '&safe'\n",
        "        shell: \"bash\" # &safe\n        run: |\n",
        "          echo '&safe'\n          echo \"&environment\"\n",
        "          echo &safe\n      - *safe\n",
    );
    let steps = step_lists(workflow);
    assert_eq!(steps[0].1.len(), 2);
    assert!(steps[0].1.iter().all(|step| !step.invalid_anchor));
    assert!(steps[0].1.iter().all(|step| !step.unexpected_nested));
}

#[path = "rust_install_audit_parse_multiline_tests.rs"]
mod multiline_tests;

#[path = "rust_install_audit_parse_legacy_env_alias_tests.rs"]
mod env_alias_tests;
