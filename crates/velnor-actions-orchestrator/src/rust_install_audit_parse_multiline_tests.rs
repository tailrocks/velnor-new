use super::super::super::anchor_names;
use super::{audit_workflow, step_lists, verifiers};

#[test]
fn multiline_flow_anchors_on_opening_and_later_lines_share_namespace() {
    let verifiers = verifiers();
    for compact in [false, true] {
        let step = if compact {
            "jobs:\n build:\n  steps:\n   - &shared\n    name: Safe\n"
        } else {
            "jobs:\n  build:\n    steps:\n      - &shared\n        name: Safe\n"
        };
        let flows = [
            concat!(
                "trusted: &shared { KEY: safe }\n",
                "metadata: { inner: &shared { KEY: hostile },\n",
                "            other: value }\n",
                "selected: *shared\n",
            ),
            concat!(
                "trusted: &shared { KEY: safe }\n",
                "metadata: {\n",
                "  inner: { nested: &shared { KEY: hostile } },\n",
                "  other: value }\n",
                "selected: *shared\n",
            ),
        ];

        for prefix in flows {
            let workflow = format!("{prefix}{step}");
            assert!(step_lists(&workflow)[0].1[0].invalid_anchor);
            assert!(
                audit_workflow(workflow, &verifiers)
                    .contains("invalid YAML anchor declaration or alias")
            );
        }
    }
}

#[test]
fn multiline_quoted_scalar_text_does_not_declare_an_anchor() {
    for compact in [false, true] {
        let step = if compact {
            "jobs:\n build:\n  steps:\n   - &shared\n    name: Safe\n"
        } else {
            "jobs:\n  build:\n    steps:\n      - &shared\n        name: Safe\n"
        };
        for prefix in [
            "message: \"first line\n  &shared ordinary\"\n",
            "message: 'first line\n  &shared ordinary'\n",
        ] {
            let workflow = format!("{prefix}{step}");
            assert!(!step_lists(&workflow)[0].1[0].invalid_anchor);
        }
    }
}

#[test]
fn multiline_flow_anchor_is_not_counted_twice() {
    let workflow = concat!(
        "metadata: {\n",
        "  inner: &flow_only value,\n",
        "  other: value }\n",
        "jobs:\n  build:\n    steps:\n",
        "      - &step\n        name: Safe\n",
    );
    assert!(!step_lists(workflow)[0].1[0].invalid_anchor);
}

#[test]
fn plain_scalar_apostrophes_and_flow_indicators_do_not_change_anchor_state() {
    let duplicate_declarations = concat!("first: &shared safe\n", "second: &shared hostile\n",);
    assert_eq!(
        anchor_names::duplicate_names(&format!(
            "message: Don't run sources\n{duplicate_declarations}"
        )),
        ["shared".to_owned()].into_iter().collect()
    );
    assert_eq!(
        anchor_names::duplicate_names(
            "selected: [don't, &shared safe]\nother: &shared duplicate\n"
        ),
        ["shared".to_owned()].into_iter().collect()
    );
    assert!(
        anchor_names::duplicate_names(
            "message: echo [foo, &shared plain text\nlater: &shared legitimate\n"
        )
        .is_empty()
    );
    assert!(
        anchor_names::duplicate_names(
            "message: echo {foo, &shared plain text\nlater: &shared legitimate\n"
        )
        .is_empty()
    );
    assert_eq!(
        anchor_names::duplicate_names(concat!(
            "don't:\n",
            "  first: &shared safe\n",
            "  second: &shared hostile\n",
        )),
        ["shared".to_owned()].into_iter().collect()
    );
}

#[test]
fn quoted_and_plain_keys_keep_colon_and_comment_markers_in_the_key() {
    for key in [
        "don't:",
        "name#part:",
        "'a''b: c':",
        "'a#b: c':",
        "\"a:b # c\":",
        "\"a\\\"b: c # d\":",
    ] {
        let document = format!("{key}\n  first: &shared safe\n  second: &shared hostile\n");
        assert!(
            anchor_names::duplicate_names(&document).contains("shared"),
            "nested duplicate anchor was missed for mapping key {key:?}"
        );
    }
}

#[test]
fn explicit_mapping_key_anchor_joins_the_document_namespace() {
    let workflow = concat!(
        "trusted: &shared { KEY: safe }\n",
        "? &shared { KEY: hostile }\n",
        ": replacement\n",
        "selected: *shared\n",
        "jobs:\n  build:\n    steps:\n",
        "      - name: Safe step\n        run: echo safe\n",
    );
    assert!(anchor_names::duplicate_names(workflow).contains("shared"));
    assert!(
        audit_workflow(workflow.to_owned(), &verifiers())
            .contains("invalid YAML anchor declaration or alias")
    );
}

#[test]
fn verbatim_tag_commas_do_not_hide_following_anchor_properties() {
    let unique = concat!(
        "metadata: [!<tag:example.org,2026:t> &only value]\n",
        "jobs:\n  build:\n    steps:\n",
        "      - name: Safe step\n        run: echo safe\n",
    );
    assert!(!anchor_names::scan(unique).malformed_tag);
    assert!(anchor_names::duplicate_names(unique).is_empty());

    let workflow = concat!(
        "trusted: &shared { KEY: safe }\n",
        "metadata: [!<tag:example.org,2026:t> &shared hostile]\n",
        "selected: *shared\n",
        "jobs:\n  build:\n    steps:\n",
        "      - name: Safe step\n        run: echo safe\n",
    );
    assert!(anchor_names::duplicate_names(workflow).contains("shared"));
    assert!(
        audit_workflow(workflow.to_owned(), &verifiers())
            .contains("invalid YAML anchor declaration or alias")
    );

    let unterminated = concat!(
        "metadata: [!<tag:example.org,2026:t &shared value]\n",
        "trusted: &shared { KEY: safe }\n",
        "selected: *shared\n",
        "jobs:\n  build:\n    steps:\n",
        "      - name: Safe step\n        run: echo safe\n",
    );
    assert!(anchor_names::scan(unterminated).malformed_tag);
    assert!(
        audit_workflow(unterminated.to_owned(), &verifiers())
            .contains("invalid YAML anchor declaration or alias")
    );
}
