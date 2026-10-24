use std::fmt::Write as _;

use super::super::step_lists;
use super::{files, problem, verifiers};
use crate::workflow::verifier_actions::InlineVerifierTemplate;
use velnor_actions_workflow_renderer::yaml::quote_scalar;

fn layout(compact: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    if compact {
        ("jobs:\n build:\n  steps:\n", "   -", "    ", "     ")
    } else {
        (
            "jobs:\n  build:\n    steps:\n",
            "      -",
            "        ",
            "          ",
        )
    }
}

fn checkout_with_alias(compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    let expression = "${{ github.sha }}";
    format!(
        "{prefix}{item} &checkout\n{field}name: Checkout\n{field}uses: actions/checkout@v4\n{field}with:\n{child}persist-credentials: \"false\"\n{child}ref: '{expression}'\n{child}repository: example/project\n{item} *checkout\n"
    )
}

fn inline_step_with_extra_input(
    template: &InlineVerifierTemplate,
    compact: bool,
    name: &str,
) -> String {
    let (prefix, item, field, child) = layout(compact);
    let mut yaml = format!(
        "{prefix}{item}\n{field}name: {}\n{field}env:\n",
        quote_scalar(name)
    );
    for (key, value) in &template.env {
        writeln!(yaml, "{child}{key}: {}", quote_scalar(value)).unwrap_or_default();
    }
    writeln!(
        yaml,
        "{field}shell: {}\n{field}run: {}\n{field}with:\n{child}phase: installed",
        quote_scalar(template.shell.expect("canonical verifier shell")),
        template.run_scalar
    )
    .unwrap_or_default();
    yaml
}

fn install_before(compact: bool, steps: &str) -> String {
    if compact {
        format!(
            "jobs:\n build:\n  steps:\n   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n{steps}"
        )
    } else {
        format!(
            "jobs:\n  build:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{steps}"
        )
    }
}

fn anchored_rust_install_across_jobs(
    compact: bool,
    installed_rust_uses: &str,
    post_install: &str,
) -> String {
    let job_indent = if compact { " " } else { "  " };
    let steps_indent = if compact { "  " } else { "    " };
    let (_, item, field, child) = layout(compact);
    format!(
        "jobs:\n{job_indent}template:\n{steps_indent}steps:\n{item} &rust_install\n{field}name: Install pinned Rust\n{field}run: mise install rust@1.99.1\n{field}with:\n{child}install-mode: \"pinned\"\n{item}\n{field}name: Verify template installation\n{field}uses: {installed_rust_uses} # zizmor: ignore[self-repository]\n{job_indent}build:\n{steps_indent}steps:\n{item} *rust_install\n{post_install}"
    )
}

#[test]
fn ordinary_checkout_with_maps_survive_whole_step_aliases() {
    for compact in [false, true] {
        let checkout = checkout_with_alias(compact);
        let steps = &step_lists(&checkout)[0].1;
        assert_eq!(steps.len(), 2);
        for step in steps {
            assert!(!step.invalid_anchor, "{compact}: {}", step.text);
            assert!(step.unexpected_nested, "{compact}: {}", step.text);
            assert_eq!(step.uses_value(), Some("actions/checkout@v4"));
            assert!(
                step.fields
                    .iter()
                    .any(|(key, value)| { key == "persist-credentials" && value == "\"false\"" })
            );
            assert!(
                step.fields
                    .iter()
                    .any(|(key, value)| { key == "ref" && value == "'${{ github.sha }}'" })
            );
        }
        assert_eq!(steps[0].text, steps[1].text);
        assert_eq!(steps[0].fields, steps[1].fields);
        assert_eq!(steps[0].top_fields, steps[1].top_fields);
        assert_eq!(steps[0].env_fields, steps[1].env_fields);
        assert_eq!(steps[0].run_scalar, steps[1].run_scalar);
        assert_eq!(
            problem(&files(".github/workflows/ci.yml", &checkout)),
            "",
            "the aliased Checkout preserves its input map ({compact})"
        );
    }
}

#[test]
fn verifier_action_with_inputs_is_not_authoritative_through_a_step_alias() {
    let verifiers = verifiers();
    for compact in [false, true] {
        let (prefix, item, field, child) = layout(compact);
        let action = &verifiers.installed_rust;
        let yaml = format!(
            "{prefix}{item} &verifier\n{field}name: Neutral verifier\n{field}uses: {} # zizmor: ignore[self-repository]\n{field}with:\n{child}phase: installed\n{item} *verifier\n",
            action.uses
        );
        let steps = &step_lists(&yaml)[0].1;
        assert_eq!(steps.len(), 2);
        for step in steps {
            assert!(!step.invalid_anchor, "{compact}: {}", step.text);
            assert!(step.unexpected_nested, "{compact}: {}", step.text);
            assert_eq!(step.fields, steps[0].fields);
            assert_eq!(step.text, steps[0].text);
        }
        let found = problem(&files(".github/workflows/ci.yml", &yaml));
        assert!(
            found.contains("noncanonical verifier call or body"),
            "typed role and neutral label cannot authorize extra inputs: {found}"
        );
    }
}

#[test]
fn aliased_rust_install_with_inputs_still_requires_installed_verification() {
    let verifiers = verifiers();
    let uses = verifiers.installed_rust.uses;
    for compact in [false, true] {
        let missing = anchored_rust_install_across_jobs(compact, uses, "");
        let lists = step_lists(&missing);
        let install = &lists[0].1[0];
        let alias = &lists[1].1[0];
        for step in [install, alias] {
            assert!(!step.invalid_anchor, "{compact}: {}", step.text);
            assert!(step.unexpected_nested, "{compact}: {}", step.text);
            assert!(
                step.run_scalar
                    .as_deref()
                    .is_some_and(|run| run.contains("mise install rust@1.99.1")),
                "the aliased installation run remains visible"
            );
            assert!(
                step.fields
                    .iter()
                    .any(|(key, value)| key == "install-mode" && value == "\"pinned\""),
                "the aliased input map remains visible"
            );
        }
        assert_eq!(install.text, alias.text);
        let found = problem(&files(".github/workflows/ci.yml", &missing));
        assert!(
            found.contains("no verification follows the install"),
            "the aliased install still requires proof ({compact}): {found}"
        );

        let (_, item, field, _) = layout(compact);
        let late_build = format!(
            "{item}\n{field}name: Build before verification\n{field}run: mise exec rust@1.99.1 -- cargo build\n{item}\n{field}name: Verify build installation\n{field}uses: {uses} # zizmor: ignore[self-repository]\n",
        );
        let late = anchored_rust_install_across_jobs(compact, uses, &late_build);
        let found = problem(&files(".github/workflows/ci.yml", &late));
        assert!(
            found.contains("runs the toolchain it installs before the verification"),
            "the aliased install still rejects Rust use before proof ({compact}): {found}"
        );
    }
}

#[test]
fn inline_verifier_with_inputs_is_rejected_and_cannot_discharge_install() {
    let verifiers = verifiers();
    let template = verifiers
        .inline_installed_rust_templates
        .last()
        .expect("the installed Rust inline verifier template");
    for compact in [false, true] {
        let reserved =
            inline_step_with_extra_input(template, compact, "Verify Rust channel manifest");
        let found = problem(&files(".github/workflows/release.yml", &reserved));
        assert!(
            found.contains("noncanonical verifier call or body"),
            "a reserved inline label cannot authorize extra inputs: {found}"
        );

        let neutral = inline_step_with_extra_input(template, compact, "Neutral verifier");
        let found = problem(&files(
            ".github/workflows/release.yml",
            &install_before(compact, &neutral),
        ));
        assert!(
            found.contains("runs the toolchain it installs before the verification")
                || found.contains("no verification follows the install"),
            "a neutral inline step with extra inputs cannot satisfy Rust verification: {found}"
        );
    }
}

#[test]
fn malformed_layout_and_run_remain_fatal_for_anchored_steps() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let malformed_layout = format!(
            "{prefix}{item} &bad_layout\n{field}name: Ordinary step\n {field}uses: actions/checkout@v4\n{item} *bad_layout\n"
        );
        let layout_steps = &step_lists(&malformed_layout)[0].1;
        assert!(
            layout_steps[0].invalid_anchor,
            "{compact}: malformed layout"
        );
        assert!(layout_steps[1].invalid_anchor, "{compact}: malformed alias");

        let malformed_run = format!(
            "{prefix}{item} &bad_run\n{field}name: Ordinary step\n{field}run: echo first\n{field}run: echo second\n{item} *bad_run\n"
        );
        let run_steps = &step_lists(&malformed_run)[0].1;
        assert!(run_steps[0].invalid_anchor, "{compact}: malformed run");
        assert!(
            run_steps[1].invalid_anchor,
            "{compact}: malformed run alias"
        );
    }
}
