use std::fmt::Write as _;

use super::super::{parse::duplicate_anchor_names, step_lists};
use super::{files, problem, verifiers};
use crate::workflow::verifier_actions::InlineVerifierTemplate;
use velnor_actions_workflow_renderer::yaml::quote_scalar;

fn layout(compact: bool) -> (&'static str, &'static str, &'static str) {
    if compact {
        ("jobs:\n build:\n  steps:\n", "   -", "    ")
    } else {
        ("jobs:\n  build:\n    steps:\n", "      -", "        ")
    }
}

fn anchored_inline_workflow(template: &InlineVerifierTemplate, compact: bool) -> String {
    let (prefix, item, field, env) = if compact {
        ("jobs:\n build:\n  steps:\n", "   -", "    ", "     ")
    } else {
        (
            "jobs:\n  build:\n    steps:\n",
            "      -",
            "        ",
            "          ",
        )
    };
    let mut yaml = prefix.to_owned();
    writeln!(
        yaml,
        "{item}\n{field}name: {}\n{field}env: &verify_env",
        quote_scalar("Neutral verifier")
    )
    .unwrap_or_default();
    for (key, value) in &template.env {
        writeln!(yaml, "{env}{key}: {}", quote_scalar(value)).unwrap_or_default();
    }
    writeln!(
        yaml,
        "{field}shell: {}\n{field}run: &verify_run {}",
        quote_scalar(template.shell.expect("canonical verifier shell")),
        template.run_scalar
    )
    .unwrap_or_default();
    writeln!(
        yaml,
        "{item}\n{field}name: {}\n{field}env: *verify_env\n{field}shell: {}\n{field}run: *verify_run",
        quote_scalar("Another neutral label"),
        quote_scalar(template.shell.expect("canonical verifier shell"))
    )
    .unwrap_or_default();
    yaml
}

#[test]
fn inline_run_and_environment_anchors_resolve_at_both_renderer_widths() {
    let verifier_set = verifiers();
    let template = verifier_set
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let anchored = anchored_inline_workflow(template, compact);
        let prefix = if compact {
            "jobs:\n build:\n  steps:\n"
        } else {
            "jobs:\n  build:\n    steps:\n"
        };
        let install = if compact {
            "   -\n    name: Install pinned Rust\n    run: mise install rust@1.99.1\n"
        } else {
            "      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n"
        };
        let anchor_steps = anchored
            .strip_prefix(prefix)
            .expect("anchored workflow prefix");
        let workflow = format!("{prefix}{install}{anchor_steps}");
        assert_eq!(
            problem(&files(".github/workflows/release.yml", &workflow)),
            "",
            "shared run/env anchors preserve the canonical verifier at either width"
        );

        let (field, child) = if compact {
            ("    ", "     ")
        } else {
            ("        ", "          ")
        };
        let forged = anchored.replace(
            &format!("{field}env: *verify_env\n"),
            &format!("{field}env: *verify_env\n{child}INJECTED: value\n"),
        );
        assert_ne!(forged, anchored);
        let found = problem(&files(".github/workflows/release.yml", &forged));
        assert!(
            found.contains("unresolved or malformed YAML anchor"),
            "a map alias remains a leaf: {found}"
        );
    }
}

#[test]
fn null_anchors_in_flow_values_poison_later_aliases() {
    for (kind, redefinition) in [
        ("sequence", "metadata: [&shared, other]\n"),
        ("sequence close", "metadata: [&shared]\n"),
        ("mapping", "metadata: { first: &shared, second: other }\n"),
        ("mapping close", "metadata: { first: &shared}\n"),
        (
            "sequence target after null",
            "metadata: [&innocent, &shared { KEY: hostile }]\n",
        ),
        (
            "sequence null target before close",
            "metadata: [&innocent, &shared]\n",
        ),
        (
            "mapping target after null",
            "metadata: { first: &innocent, second: &shared { KEY: hostile } }\n",
        ),
        (
            "mapping null target before close",
            "metadata: { first: &innocent, second: &shared}\n",
        ),
        (
            "bare tag target after null",
            "metadata: [!!str, &shared { KEY: hostile }]\n",
        ),
        (
            "bare tag null target before close",
            "metadata: [!!str, &shared]\n",
        ),
        (
            "tagged nested sequence",
            "metadata: [!<tag:yaml.org,2002:seq> [&shared { KEY: hostile }, other]]\n",
        ),
        (
            "tagged nested mapping",
            "metadata: [!<tag:yaml.org,2002:map> { first: &shared { KEY: hostile }, second: other }]\n",
        ),
    ] {
        for compact in [false, true] {
            let (prefix, item, field) = layout(compact);
            let yaml = format!(
                "trusted: &shared {{ KEY: safe }}\n{redefinition}{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
            );
            assert_eq!(
                duplicate_anchor_names(&yaml),
                ["shared".to_owned()].into_iter().collect(),
                "{kind} null anchor ({compact})"
            );
            let lists = step_lists(&yaml);
            let step = &lists[0].1[0];
            assert!(step.invalid_anchor, "{kind}, {compact}: {}", step.text);

            let found = problem(&files(".github/workflows/release.yml", &yaml));
            assert!(
                found.contains("duplicate YAML anchor in document"),
                "{kind}, {compact}: {found}"
            );
        }
    }
}

#[test]
fn plain_scalar_continuations_do_not_declare_anchors_or_hide_siblings() {
    let outside =
        "trusted: &shared safe\nmessage: ordinary\n  &shared literal\nselected: *shared\n";
    assert!(duplicate_anchor_names(outside).is_empty());
    let document_start = "---\n  trusted: &shared safe\n  metadata:\n    nested: &shared hostile\n";
    assert_eq!(
        duplicate_anchor_names(document_start),
        ["shared".to_owned()].into_iter().collect()
    );
    let scalar_marker =
        "trusted: &shared safe\nmessage: ---\n  &shared literal\nselected: *shared\n";
    assert!(duplicate_anchor_names(scalar_marker).is_empty());

    let expected_duplicate = ["shared".to_owned()].into_iter().collect();
    for yaml in [
        "trusted: &shared safe\nmessage: ordinary\nnext: &shared hostile\n",
        "trusted: &shared safe\nmetadata:\n  nested: &shared hostile\n",
        "trusted: &shared safe\nmetadata:\n  - &shared hostile\n",
        "trusted: &shared safe\njobs:\n  build:\n    steps:\n      - name: Plain map\n        id: ordinary\n          &shared literal\n        env: &shared { KEY: hostile }\n",
    ] {
        assert_eq!(duplicate_anchor_names(yaml), expected_duplicate, "{yaml}");
    }

    for compact in [false, true] {
        let (prefix, item, field) = layout(compact);
        let child = if compact { " " } else { "  " };
        let yaml = format!(
            "{prefix}{item}\n{field}name: Seed\n{field}env: &shared\n{field}{child}KEY: safe\n{item}\n{field}name: Plain continuation\n{field}id: ordinary\n{field}{child}&shared literal\n{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert!(
            duplicate_anchor_names(&yaml).is_empty(),
            "{compact}: {yaml}"
        );
        let lists = step_lists(&yaml);
        let alias = &lists[0].1[2];
        assert!(!alias.invalid_anchor, "{compact}: {}", alias.text);
        assert_eq!(
            alias.env_fields,
            vec![("KEY".to_owned(), "safe".to_owned())]
        );

        let sibling_anchor = format!(
            "{prefix}{item}\n{field}name: Seed\n{field}env: &shared\n{field}{child}KEY: safe\n{item}\n{field}name: Plain continuation\n{field}id: ordinary\n{field}{child}&shared literal\n{field}env: &shared {{ KEY: hostile }}\n"
        );
        assert_eq!(
            duplicate_anchor_names(&sibling_anchor),
            expected_duplicate,
            "same-column sibling after scalar continuation ({compact}): {sibling_anchor}"
        );
    }
}

#[test]
fn partial_flow_poisoning_uses_the_same_anchor_name_boundary() {
    let yaml = "trusted: &shared safe\nmetadata: [&shared, other\n";
    assert_eq!(
        duplicate_anchor_names(yaml),
        ["shared".to_owned()].into_iter().collect()
    );
}

#[test]
fn verbatim_tag_uri_commas_preserve_unique_and_duplicate_anchors() {
    let unique = "first: !<tag:yaml.org,2002:str> &shared value\nselected: *shared\n";
    assert!(duplicate_anchor_names(unique).is_empty());

    let yaml = "trusted: &shared { KEY: safe }\nmetadata: [!<tag:yaml.org,2002:str> &shared hostile]\nselected: *shared\n";
    assert_eq!(
        duplicate_anchor_names(yaml),
        ["shared".to_owned()].into_iter().collect()
    );
}

#[test]
fn unterminated_verbatim_tag_cannot_hide_a_duplicate_anchor() {
    for compact in [false, true] {
        let (prefix, item, field) = layout(compact);
        let yaml = format!(
            "trusted: &shared {{ KEY: safe }}\nmetadata: [!<tag:example.org,2026:t &shared hostile]\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "unterminated tag ({compact})"
        );
        let lists = step_lists(&yaml);
        assert!(lists[0].1[0].invalid_anchor, "{compact}");
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{compact}: {found}"
        );
    }
}
