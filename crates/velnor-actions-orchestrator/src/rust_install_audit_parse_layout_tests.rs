use super::super::{StepText, step_lists};
use super::{files, inline_step, problem, verifiers};
use velnor_actions_workflow_renderer::yaml::{Yaml, quote_scalar, render_yaml};

fn mapping(entries: impl IntoIterator<Item = (&'static str, Yaml)>) -> Yaml {
    Yaml::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn run_step(name: &str, run: &str) -> Yaml {
    mapping([("name", Yaml::str(name)), ("run", Yaml::str(run))])
}

fn canonical_action_workflow() -> Yaml {
    mapping([(
        "jobs",
        mapping([(
            "build",
            mapping([(
                "steps",
                Yaml::Seq(vec![
                    run_step("Install pinned Rust", "mise install rust@1.99.1"),
                    mapping([
                        ("name", Yaml::str("Verify Rust channel manifest")),
                        (
                            "uses",
                            Yaml::annotated(
                                "./.github/actions/velnor-verify-rust-manifest",
                                "zizmor: ignore[self-repository]",
                            ),
                        ),
                    ]),
                    run_step("Build", "mise exec rust@1.99.1 -- cargo build"),
                ]),
            )]),
        )]),
    )])
}

fn parsed_semantics(steps: &[StepText]) -> Vec<ParsedStep> {
    steps
        .iter()
        .map(|step| ParsedStep {
            name: step.name.clone(),
            fields: step.fields.clone(),
            top_fields: step.top_fields.clone(),
            env_fields: step.env_fields.clone(),
            run_scalar: step.run_scalar.clone(),
            unexpected_nested: step.unexpected_nested,
            invalid_anchor: step.invalid_anchor,
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedStep {
    name: String,
    fields: Vec<(String, String)>,
    top_fields: Vec<(String, String)>,
    env_fields: Vec<(String, String)>,
    run_scalar: Option<String>,
    unexpected_nested: bool,
    invalid_anchor: bool,
}

#[test]
fn standard_and_compact_layouts_have_equal_parsed_and_audited_steps() {
    let standard = render_yaml(&canonical_action_workflow());
    let compact = concat!(
        "jobs:\n",
        " build:\n",
        "  steps:\n",
        "   -\n",
        "    name: Install pinned Rust\n",
        "    run: mise install rust@1.99.1\n",
        "   -\n",
        "    name: Verify Rust channel manifest\n",
        "    uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n",
        "   -\n",
        "    name: Build\n",
        "    run: mise exec rust@1.99.1 -- cargo build\n",
    );
    assert_ne!(standard, compact);
    assert!(standard.contains("      - name: Install pinned Rust"));
    assert!(compact.contains("   -\n    name: Install pinned Rust"));

    let standard_steps = step_lists(&standard);
    let compact_steps = step_lists(compact);
    assert_eq!(standard_steps.len(), 1);
    assert_eq!(compact_steps.len(), 1);
    assert_eq!(standard_steps[0].0, compact_steps[0].0);
    assert_eq!(
        parsed_semantics(&standard_steps[0].1),
        parsed_semantics(&compact_steps[0].1),
        "top-level fields and verifier env fields stay distinct at both widths"
    );
    assert_eq!(problem(&files(".github/workflows/ci.yml", &standard)), "");
    assert_eq!(problem(&files(".github/workflows/ci.yml", compact)), "");
}

fn compact_inline_step(
    name: &str,
    template: &crate::workflow::verifier_actions::InlineVerifierTemplate,
) -> String {
    let fields = "    ";
    let env = "     ";
    let mut lines = vec![
        "   -".to_owned(),
        format!("{fields}name: {}", quote_scalar(name)),
        format!("{fields}env:"),
    ];
    lines.extend(
        template
            .env
            .iter()
            .map(|(key, value)| format!("{env}{key}: {}", quote_scalar(value))),
    );
    lines.push(format!(
        "{fields}shell: {}",
        quote_scalar(template.shell.expect("canonical verifier shell"))
    ));
    lines.push(format!("{fields}run: {}", template.run_scalar));
    lines.join("\n") + "\n"
}

fn inline_workflow(step: &str, compact: bool) -> String {
    if compact {
        format!(
            "jobs:\n build:\n  steps:\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n{step}"
        )
    } else {
        format!(
            "jobs:\n  build:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{step}"
        )
    }
}

#[test]
fn inline_verifier_mutations_fail_closed_at_both_renderer_widths() {
    let verifier_set = verifiers();
    let template = verifier_set
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    let standard = inline_step("Verify Rust channel manifest", template);
    let compact = compact_inline_step("Verify Rust channel manifest", template);
    for (step, compact_layout) in [(standard, false), (compact, true)] {
        let canonical = inline_workflow(&step, compact_layout);
        assert_eq!(
            problem(&files(".github/workflows/release.yml", &canonical)),
            ""
        );

        let env_indent = if compact_layout {
            "     "
        } else {
            "          "
        };
        let nested_indent = if compact_layout {
            "     "
        } else {
            "          "
        };
        let field_indent = if compact_layout { "    " } else { "        " };
        let wrong_env = step.replace(
            &format!(
                "{env_indent}MISE_NO_CONFIG: {}",
                quote_scalar(&template.env["MISE_NO_CONFIG"])
            ),
            &format!("{env_indent}MISE_NO_CONFIG: \"0\""),
        );
        let wrong_shell = step.replace(
            &format!(
                "{field_indent}shell: {}",
                quote_scalar(template.shell.expect("canonical shell"))
            ),
            &format!("{field_indent}shell: sh"),
        );
        let wrong_run = step.replace(&template.run_scalar, "\"tampered verifier\"");
        let wrong_width = step.replace(
            &format!("{field_indent}shell:"),
            &format!("{nested_indent}shell:"),
        );
        for changed in [wrong_env, wrong_shell, wrong_run, wrong_width] {
            assert_ne!(changed, step);
            let found = problem(&files(
                ".github/workflows/release.yml",
                &inline_workflow(&changed, compact_layout),
            ));
            assert!(
                found.contains("noncanonical verifier call or body"),
                "altered inline verifier or mixed field widths must fail: {found}"
            );
        }
    }
}

#[test]
fn literal_run_anchors_and_aliases_resolve_at_both_renderer_widths() {
    for (compact, yaml) in [
        (
            false,
            "jobs:\n  build:\n    steps:\n      -\n        name: Script body\n        run: &script |\n          printf '%s\\n' 'key: value'\n      -\n        name: Script alias\n        run: *script\n",
        ),
        (
            true,
            "jobs:\n build:\n  steps:\n   -\n    name: Script body\n    run: &script |\n     printf '%s\\n' 'key: value'\n   -\n    name: Script alias\n    run: *script\n",
        ),
    ] {
        let lists = step_lists(yaml);
        assert_eq!(lists.len(), 1);
        let steps = &lists[0].1;
        assert_eq!(steps.len(), 2);
        let expected = "printf '%s\\n' 'key: value'\n";
        for step in steps {
            assert!(!step.unexpected_nested, "{compact}: {}", step.text);
            assert_eq!(step.run_scalar.as_deref(), Some(expected));
            assert_eq!(
                step.top_fields
                    .iter()
                    .find(|(key, _)| key == "run")
                    .map(|(_, value)| value.as_str()),
                Some(expected)
            );
            assert_eq!(step.fields.len(), 2, "script body is not parsed as fields");
        }
    }
}

#[test]
fn missing_environment_and_run_anchors_remain_unexpected_at_both_widths() {
    for yaml in [
        "jobs:\n  build:\n    steps:\n      -\n        name: Missing anchors\n        env: *missing_env\n        run: *missing_run\n",
        "jobs:\n build:\n  steps:\n   -\n    name: Missing anchors\n    env: *missing_env\n    run: *missing_run\n",
    ] {
        let lists = step_lists(yaml);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].1.len(), 1);
        assert!(lists[0].1[0].unexpected_nested);
        assert_eq!(lists[0].1[0].run_scalar, None);
    }
}

#[test]
fn literal_run_blocks_reject_inconsistent_implicit_indentation() {
    for yaml in [
        "jobs:\n  build:\n    steps:\n      -\n        name: Malformed block\n        run: |\n          first\n         second\n",
        "jobs:\n build:\n  steps:\n   -\n    name: Malformed block\n    run: |\n      first\n     second\n",
    ] {
        let lists = step_lists(yaml);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].1.len(), 1);
        assert!(lists[0].1[0].unexpected_nested);
    }
}

fn action_alias_workflow(compact: bool, installed: bool, continuation: &str) -> String {
    if compact {
        let install = if installed {
            "   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n"
        } else {
            ""
        };
        format!(
            "jobs:\n template:\n  steps:\n   - &verify\n    name: Neutral verifier label\n    uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n build:\n  steps:\n{install}   - *verify\n{continuation}"
        )
    } else {
        let install = if installed {
            "      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n"
        } else {
            ""
        };
        format!(
            "jobs:\n  template:\n    steps:\n      - &verify\n        name: Neutral verifier label\n        uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n  build:\n    steps:\n{install}      - *verify\n{continuation}"
        )
    }
}

#[test]
fn whole_step_aliases_are_leaves_for_neutral_labels_at_both_widths() {
    for compact in [false, true] {
        for installed in [false, true] {
            let valid = action_alias_workflow(compact, installed, "");
            assert_eq!(
                problem(&files(".github/workflows/ci.yml", &valid)),
                "",
                "a canonical aliased action keeps its neutral display label"
            );

            let (field_indent, child_indent) = if compact {
                ("    ", "     ")
            } else {
                ("        ", "          ")
            };
            let id = format!("{field_indent}id: forged\n");
            let with = format!("{field_indent}with:\n{child_indent}phase: installed\n");
            for continuation in [id, with] {
                let forged = action_alias_workflow(compact, installed, &continuation);
                let found = problem(&files(".github/workflows/ci.yml", &forged));
                let alias = step_lists(&forged)
                    .into_iter()
                    .flat_map(|(_, steps)| steps)
                    .last()
                    .expect("the forged whole-step alias");
                let invalid_alias = alias.invalid_anchor && alias.unexpected_nested;
                assert!(invalid_alias, "invalid alias flags");
                assert!(
                    found.contains("unresolved or malformed YAML anchor"),
                    "invalid whole-step aliases fail at anchor validation: {found}"
                );
            }
        }
    }
}
