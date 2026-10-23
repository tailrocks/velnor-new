use super::super::step_lists;
use super::{files, problem, verifiers};

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

fn map_alias_workflow(compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    format!(
        "{prefix}{item} &checkout\n{field}name: Checkout\n{field}uses: actions/checkout@v4\n{field}with: &m11\n{child}persist-credentials: \"false\"\n{child}ref: main\n{item} *checkout\n{item}\n{field}name: Cache\n{field}uses: actions/cache@v4\n{field}with: *m11\n"
    )
}

fn source_with_alias(compact: bool, source: &str, alias: &str) -> String {
    let (prefix, item, field, _) = layout(compact);
    format!(
        "{prefix}{source}{item}\n{field}name: Checkout\n{field}uses: actions/checkout@v4\n{field}with: {alias}\n"
    )
}

fn assert_invalid_alias(compact: bool, source: &str, alias: &str, label: &str) {
    let yaml = source_with_alias(compact, source, alias);
    let parsed = step_lists(&yaml);
    let step = parsed[0].1.last().expect("with alias step");
    assert!(step.invalid_anchor, "{label}, {compact}: {}", step.text);
}

fn duplicate_map_workflow(compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    format!(
        "{prefix}{item}\n{field}name: First\n{field}uses: actions/checkout@v4\n{field}with: &same\n{child}ref: main\n{item}\n{field}name: Second\n{field}uses: actions/checkout@v4\n{field}with: &same\n{child}ref: other\n"
    )
}

fn malformed_map_workflow(compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    format!(
        "{prefix}{item}\n{field}name: Checkout\n{field}uses: actions/checkout@v4\n{field}with: &inputs trailing\n{child}ref: value\n"
    )
}

#[test]
fn with_map_anchors_survive_step_and_mapping_aliases_at_both_widths() {
    for compact in [false, true] {
        let yaml = map_alias_workflow(compact);
        let steps = &step_lists(&yaml)[0].1;
        assert_eq!(steps.len(), 3, "{compact}");
        let checkout = &steps[0];
        let step_alias = &steps[1];
        let map_alias = &steps[2];
        for step in [checkout, step_alias, map_alias] {
            assert!(!step.invalid_anchor, "{compact}: {}", step.text);
            assert!(step.unexpected_nested, "{compact}: {}", step.text);
            assert!(
                step.fields
                    .iter()
                    .any(|(key, value)| { key == "persist-credentials" && value == "\"false\"" })
            );
            assert!(
                step.fields
                    .iter()
                    .any(|(key, value)| key == "ref" && value == "main")
            );
        }
        assert_eq!(checkout.text, step_alias.text);
        assert_eq!(checkout.fields, step_alias.fields);
        assert!(
            map_alias
                .fields
                .iter()
                .any(|(key, value)| { key == "with" && value == "*m11" })
        );
    }
}

#[test]
fn with_map_aliases_reject_unknown_wrong_kind_duplicate_and_descendant() {
    for compact in [false, true] {
        let (_, item, field, child) = layout(compact);
        let wrong_kinds = [
            (
                "environment map",
                format!("{item}\n{field}name: Source\n{field}env: &shared\n{child}KEY: value\n"),
            ),
            (
                "run scalar",
                format!("{item}\n{field}name: Source\n{field}run: &shared echo ordinary\n"),
            ),
            (
                "whole step",
                format!("{item} &shared\n{field}name: Source\n{field}uses: actions/checkout@v4\n"),
            ),
        ];
        assert_invalid_alias(compact, "", "*missing", "unknown alias");
        for (kind, source) in wrong_kinds {
            assert_invalid_alias(compact, &source, "*shared", kind);
        }

        let duplicate_lists = step_lists(&duplicate_map_workflow(compact));
        let duplicate = &duplicate_lists[0].1;
        assert!(duplicate.iter().all(|step| step.invalid_anchor));

        let definition = format!(
            "{item}\n{field}name: Source\n{field}uses: actions/checkout@v4\n{field}with: &inputs\n{child}ref: main\n"
        );
        let with_child = format!(
            "{}{child}ref: forged\n",
            source_with_alias(compact, &definition, "*inputs")
        );
        let with_child_lists = step_lists(&with_child);
        let alias = with_child_lists[0].1.last().expect("with alias child");
        assert!(alias.invalid_anchor, "{compact}: {}", alias.text);

        let malformed_lists = step_lists(&malformed_map_workflow(compact));
        let malformed = &malformed_lists[0].1[0];
        assert!(malformed.invalid_anchor, "{compact}: {}", malformed.text);
    }
}

#[test]
fn canonical_verifier_rejects_an_anchored_extra_input_map() {
    let verifier_set = verifiers();
    for compact in [false, true] {
        let (prefix, item, field, child) = layout(compact);
        let yaml = format!(
            "{prefix}{item}\n{field}name: Verify Rust channel manifest\n{field}uses: {} # zizmor: ignore[self-repository]\n{field}with: &inputs\n{child}phase: installed\n",
            verifier_set.installed_rust.uses
        );
        let step = &step_lists(&yaml)[0].1[0];
        assert!(!step.invalid_anchor, "{compact}: {}", step.text);
        assert!(step.unexpected_nested, "{compact}: {}", step.text);
        assert!(
            step.fields
                .iter()
                .any(|(key, value)| { key == "phase" && value == "installed" })
        );
        let found = problem(&files(".github/workflows/ci.yml", &yaml));
        assert!(
            found.contains("noncanonical verifier call or body"),
            "a valid with map still cannot extend the canonical verifier ({compact}): {found}"
        );
    }
}
