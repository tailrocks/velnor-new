use super::{StepText, step_lists};
use crate::workflow::verifier_actions::{InlineVerifierTemplate, VerifierActionSet};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    render::RenderedFile,
    yaml::{Yaml, quote_scalar, render_yaml, render_yaml_compact},
};

fn verifiers() -> VerifierActionSet {
    VerifierActionSet::build(
        &ToolCatalog::pinned().for_emission(),
        env!("CARGO_PKG_VERSION"),
    )
    .expect("canonical verifier actions")
}

fn mapping(entries: impl IntoIterator<Item = (&'static str, Yaml)>) -> Yaml {
    Yaml::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn step(name: &str, key: &'static str, value: Yaml) -> Yaml {
    mapping([("name", Yaml::str(name)), (key, value)])
}

fn workflow(steps: Vec<Yaml>) -> Yaml {
    mapping([(
        "jobs",
        mapping([("build", mapping([("steps", Yaml::Seq(steps))]))]),
    )])
}

fn canonical_workflow() -> Yaml {
    workflow(vec![
        step(
            "Install pinned Rust",
            "run",
            Yaml::str("mise install rust@1.99.1"),
        ),
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
        step(
            "Build",
            "run",
            Yaml::str("mise exec rust@1.99.1 -- cargo build"),
        ),
    ])
}

fn audit_workflow(text: String, verifiers: &VerifierActionSet) -> String {
    let mut files = vec![RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: text,
    }];
    files.extend(verifiers.files());
    super::super::audit_with_verifiers(&files, verifiers)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

type SemanticStep = (
    String,
    Vec<(String, String)>,
    Vec<(String, String)>,
    Option<String>,
    bool,
);

fn semantic(steps: &[StepText]) -> Vec<SemanticStep> {
    steps
        .iter()
        .map(|step| {
            (
                step.name.clone(),
                step.fields.clone(),
                step.env_fields.clone(),
                step.run_scalar.clone(),
                step.unexpected_nested,
            )
        })
        .collect()
}

#[test]
fn standard_and_compact_renderer_layouts_have_the_same_audited_steps() {
    let verifiers = verifiers();
    let standard = render_yaml(&canonical_workflow());
    let compact = render_yaml_compact(&canonical_workflow());
    assert_ne!(standard, compact);
    assert!(standard.contains("      - name: Install pinned Rust"));
    assert!(compact.contains("   -\n    name: Install pinned Rust"));

    let standard_steps = step_lists(&standard);
    let compact_steps = step_lists(&compact);
    assert_eq!(standard_steps.len(), 1);
    assert_eq!(compact_steps.len(), 1);
    assert_eq!(standard_steps[0].0, "build");
    assert_eq!(
        semantic(&standard_steps[0].1),
        semantic(&compact_steps[0].1)
    );
    assert_eq!(audit_workflow(standard, &verifiers), "");
    assert_eq!(audit_workflow(compact, &verifiers), "");
}

#[derive(Clone, Copy, Default)]
enum VerifierPresence {
    #[default]
    Absent,
    Installed,
    Restored,
}

#[derive(Default)]
struct FlowOptions {
    verifier: VerifierPresence,
    late: bool,
    use_tool: bool,
}

fn flow(options: &FlowOptions) -> Yaml {
    let mut steps = vec![step(
        "Install pinned Rust",
        "run",
        Yaml::str("mise install rust@1.99.1"),
    )];
    let action = if matches!(options.verifier, VerifierPresence::Restored) {
        "./.github/actions/velnor-verify-restored-rust"
    } else {
        "./.github/actions/velnor-verify-rust-manifest"
    };
    let verify = mapping([
        ("name", Yaml::str("Verify Rust channel manifest")),
        (
            "uses",
            Yaml::annotated(action, "zizmor: ignore[self-repository]"),
        ),
    ]);
    let build = step(
        "Build",
        "run",
        Yaml::str("mise exec rust@1.99.1 -- cargo build"),
    );
    if options.late {
        steps.push(build.clone());
    }
    if !matches!(options.verifier, VerifierPresence::Absent) {
        steps.push(verify);
    }
    if !options.late && options.use_tool {
        steps.push(build);
    }
    workflow(steps)
}

#[test]
fn both_widths_reject_missing_restored_late_and_malformed_calls() {
    let verifiers = verifiers();
    for (render, compact_layout) in [
        (render_yaml as fn(&Yaml) -> String, false),
        (render_yaml_compact, true),
    ] {
        let verified_flow = flow(&FlowOptions {
            verifier: VerifierPresence::Installed,
            use_tool: true,
            ..FlowOptions::default()
        });
        let canonical = render(&verified_flow);
        assert_eq!(audit_workflow(canonical, &verifiers), "");

        for (document, message) in [
            (flow(&FlowOptions::default()), "no verification follows"),
            (
                flow(&FlowOptions {
                    verifier: VerifierPresence::Restored,
                    ..FlowOptions::default()
                }),
                "no verification follows",
            ),
            (
                flow(&FlowOptions {
                    verifier: VerifierPresence::Installed,
                    late: true,
                    use_tool: true,
                }),
                "runs the toolchain it installs",
            ),
        ] {
            let found = audit_workflow(render(&document), &verifiers);
            assert!(found.contains(message), "expected {message}: {found}");
        }

        let wrong_ref = render(&verified_flow).replace(
            "velnor-verify-rust-manifest #",
            "velnor-verify-rust-manifest-extra #",
        );
        assert!(audit_workflow(wrong_ref, &verifiers).contains("noncanonical verifier call"));

        let (field_indent, child_indent) = if compact_layout {
            ("    ", "     ")
        } else {
            ("        ", "          ")
        };
        let with_input = render(&verified_flow).replace(
            " # zizmor: ignore[self-repository]\n",
            &format!(
                " # zizmor: ignore[self-repository]\n{field_indent}with:\n{child_indent}phase: installed\n"
            ),
        );
        assert!(audit_workflow(with_input, &verifiers).contains("noncanonical verifier call"));
    }
}

fn inline_step(name: &str, template: &InlineVerifierTemplate, compact: bool) -> String {
    let (item, field_indent, env_indent) = if compact {
        ("   -\n", "    ", "     ")
    } else {
        ("      - ", "        ", "          ")
    };
    let mut lines = vec![item.to_owned()];
    let name_field = format!("name: {}", quote_scalar(name));
    if compact {
        lines.push(format!("{field_indent}{name_field}"));
    } else {
        lines[0].push_str(&name_field);
    }
    lines.push(format!("{field_indent}env:"));
    lines.extend(
        template
            .env
            .iter()
            .map(|(key, value)| format!("{env_indent}{key}: {}", quote_scalar(value))),
    );
    lines.push(format!(
        "{field_indent}shell: {}",
        quote_scalar(template.shell.expect("canonical shell"))
    ));
    lines.push(format!("{field_indent}run: {}", template.run_scalar));
    lines.join("\n") + "\n"
}

fn inline_workflow(step: &str, compact: bool) -> String {
    if compact {
        format!(
            "jobs:\n build:\n  steps:\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n{step}"
        )
    } else {
        format!(
            concat!(
                "jobs:\n",
                "  build:\n",
                "    steps:\n",
                "      - name: Install pinned Rust\n",
                "        run: mise install rust@1.99.1\n",
                "{step}"
            ),
            step = step
        )
    }
}

#[test]
fn compact_and_standard_inline_verifiers_keep_environment_and_shell_exact() {
    let verifiers = verifiers();
    let template = verifiers
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    let env_line = format!(
        "MISE_NO_CONFIG: {}",
        quote_scalar(&template.env["MISE_NO_CONFIG"])
    );
    let shell_line = format!(
        "shell: {}",
        quote_scalar(template.shell.expect("canonical shell"))
    );
    for compact in [false, true] {
        let canonical = inline_step("Verify Rust channel manifest", template, compact);
        assert_eq!(
            audit_workflow(inline_workflow(&canonical, compact), &verifiers),
            ""
        );

        for changed in [
            canonical.replace(&env_line, "MISE_NO_CONFIG: \"0\""),
            canonical.replace(&shell_line, "shell: sh"),
            {
                let mut value = canonical.clone();
                let run = value.find("run: ").expect("inline run field") + "run: ".len();
                let end = value[run..]
                    .find('\n')
                    .map_or(value.len(), |offset| run + offset);
                value.replace_range(run..end, "\"tampered verifier\"");
                value
            },
        ] {
            assert_ne!(changed, canonical);
            let found = audit_workflow(inline_workflow(&changed, compact), &verifiers);
            assert!(
                found.contains("noncanonical verifier call or body"),
                "mutated inline verifier must fail closed: {found}"
            );
        }
    }
}

#[path = "rust_install_audit_parse_anchor_tests.rs"]
mod anchor_tests;

#[path = "rust_install_audit_parse_legacy_alias_tests.rs"]
mod alias_tests;
