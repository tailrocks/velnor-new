use std::fmt::Write as _;

use super::super::parse::duplicate_anchor_names;
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

fn anchored_verifier(template: &InlineVerifierTemplate, compact: bool) -> String {
    let (prefix, item, field, child) = layout(compact);
    let mut yaml = format!("{item}\n{field}name: Neutral verifier\n{field}env: &verify_env\n");
    for (key, value) in &template.env {
        writeln!(yaml, "{child}{key}: {}", quote_scalar(value)).unwrap_or_default();
    }
    writeln!(
        yaml,
        "{field}shell: {}\n{field}run: &verify_run {}",
        quote_scalar(template.shell.expect("canonical verifier shell")),
        template.run_scalar
    )
    .unwrap_or_default();
    format!("{prefix}{yaml}")
}

fn aliased_verifier(template: &InlineVerifierTemplate, compact: bool) -> String {
    let (_, item, field, _) = layout(compact);
    format!(
        "{item}\n{field}name: Another neutral label\n{field}env: *verify_env\n{field}shell: {}\n{field}run: *verify_run\n",
        quote_scalar(template.shell.expect("canonical verifier shell"))
    )
}

#[test]
fn duplicate_env_and_run_redefinitions_poison_later_verifier_aliases() {
    let verifier_set = verifiers();
    let template = verifier_set
        .inline_installed_rust_templates
        .last()
        .expect("installed Rust inline verifier");
    for compact in [false, true] {
        let (_, item, field, child) = layout(compact);
        let mut yaml = anchored_verifier(template, compact);
        writeln!(
            yaml,
            "{item}\n{field}name: Ordinary step\n{field}env: &verify_env\n{child}MISE_NO_CONFIG: \"0\"\n{field}run: &verify_run \"echo forged\""
        )
        .unwrap_or_default();
        writeln!(
            yaml,
            "{item}\n{field}name: Install pinned Rust\n{field}run: mise install rust@1.99.1"
        )
        .unwrap_or_default();
        yaml.push_str(&aliased_verifier(template, compact));

        let lists = step_lists(&yaml);
        let alias = lists[0].1.last().expect("later alias step");
        assert!(alias.invalid_anchor, "{compact}: {}", alias.text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "a redefinition must invalidate the full document: {found}"
        );
    }
}

#[test]
fn anchors_share_one_namespace_across_kinds_and_outside_steps() {
    for compact in [false, true] {
        let (prefix, item, field, child) = layout(compact);
        let yaml = format!(
            "metadata: &shared {{ KEY: outside }}\n{prefix}{item}\n{field}name: Run anchor\n{field}run: &shared \"ordinary\"\n{item}\n{field}name: Env anchor\n{field}env: &shared\n{child}KEY: inside\n{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect()
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[2].invalid_anchor, "{compact}: {}", steps[2].text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{found}"
        );
    }
}

#[test]
fn whole_step_redefinitions_cannot_authorize_a_later_alias() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let yaml = format!(
            "{prefix}{item} &verify\n{field}name: Neutral verifier\n{field}uses: ./.github/actions/velnor-verify-rust-manifest # zizmor: ignore[self-repository]\n{item} &verify\n{field}name: Ordinary step\n{field}run: echo forged\n{item} *verify\n"
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[2].invalid_anchor, "{compact}: {}", steps[2].text);
        let found = problem(&files(".github/workflows/ci.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{found}"
        );
    }
}

#[test]
fn unresolved_and_wrong_kind_aliases_fail_the_document_audit() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let install = format!(
            "{item}\n{field}name: Install pinned Rust\n{field}run: mise install rust@1.99.1\n"
        );
        let wrong_kind = format!(
            "{item}\n{field}name: Scalar run anchor\n{field}run: &wrong_kind echo ordinary\n"
        );
        for definition in ["", wrong_kind.as_str()] {
            let yaml = format!(
                "{prefix}{definition}{install}{item}\n{field}name: Verify Rust channel manifest\n{field}env: *wrong_kind\n{field}shell: /usr/bin/bash -p\n{field}run: echo verify\n"
            );
            let found = problem(&files(".github/workflows/release.yml", &yaml));
            assert!(
                found.contains("unresolved or malformed YAML anchor"),
                "unknown and wrong-kind aliases fail closed ({compact}): {found}"
            );
        }
        let missing_step = format!("{prefix}{install}{item} *missing_step\n");
        let found = problem(&files(".github/workflows/release.yml", &missing_step));
        assert!(
            found.contains("unresolved or malformed YAML anchor"),
            "a missing whole-step alias fails closed ({compact}): {found}"
        );
    }
}

#[test]
fn quoted_comments_and_literal_shell_bodies_are_not_anchor_declarations() {
    let yaml = "metadata: &safe { KEY: value }\nquoted: \"&safe\"\nsingle_quoted: '&safe'\nflow: { text: 'literal '' &safe, still text', key: value }\ncomment: value # &safe\nscript: |\n  printf '%s' '&safe'\n  # &safe\n";
    assert!(duplicate_anchor_names(yaml).is_empty());
}

#[test]
fn flow_sequence_mapping_anchors_join_the_document_namespace() {
    let yaml = "metadata:\n  - { first: &shared one, second: &shared two }\nnested:\n  - - &nested one\n    - &nested two\nblock_seed: &block one\nblock_nested:\n  - - |\n        ordinary literal &block\n    - &block two\n";
    assert_eq!(
        duplicate_anchor_names(yaml),
        ["block".to_owned(), "nested".to_owned(), "shared".to_owned()]
            .into_iter()
            .collect()
    );
}

#[test]
fn multiline_flow_redefinitions_invalidate_later_aliases_at_both_widths() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let yaml = format!(
            "trusted: &shared {{ KEY: safe }}\nmetadata: {{ inner: &shared {{ KEY: hostile }},\n            other: value }}\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "multiline flow collection ({compact})"
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[0].invalid_anchor, "{compact}: {}", steps[0].text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{compact}: {found}"
        );
    }
}

#[test]
fn plain_scalar_apostrophes_do_not_hide_flow_anchor_redefinitions() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let yaml = format!(
            "plain: don't say \"quoted\" [as text] {{also text}} # &comment_only\ntrusted: &shared {{ KEY: safe }}\nmetadata: {{ message: don't, # &flow_comment_only\n            duplicate: &shared hostile }}\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "plain punctuation ({compact})"
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[0].invalid_anchor, "{compact}: {}", steps[0].text);
    }
}

#[test]
fn apostrophes_in_plain_mapping_keys_do_not_hide_flow_nodes() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let yaml = format!(
            "don't: {{ first: &shared safe, second: &shared hostile }}\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "plain mapping key ({compact})"
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[0].invalid_anchor, "{compact}: {}", steps[0].text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "plain mapping key ({compact}): {found}"
        );
    }
}

#[test]
fn quoted_flow_keys_find_adjacent_anchor_values() {
    for (key_kind, key) in [("double", "\"x\""), ("single", "'it''s'")] {
        for compact in [false, true] {
            let (prefix, item, field, _) = layout(compact);
            let yaml = format!(
                "trusted: &shared {{ KEY: safe }}\nmetadata: {{{key}:&shared {{ KEY: hostile }}}}\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
            );
            assert_eq!(
                duplicate_anchor_names(&yaml),
                ["shared".to_owned()].into_iter().collect(),
                "{key_kind} quoted key ({compact})"
            );
            let steps = &step_lists(&yaml)[0].1;
            assert!(
                steps[0].invalid_anchor,
                "{key_kind}, {compact}: {}",
                steps[0].text
            );
            let found = problem(&files(".github/workflows/release.yml", &yaml));
            assert!(
                found.contains("duplicate YAML anchor in document"),
                "{key_kind}, {compact}: {found}"
            );
        }
    }
}

#[test]
fn plain_value_colons_and_url_punctuation_remain_plain_scalars() {
    let yaml = "url: https://example.test/a:b\nplain: value:with:colons\nmessage: don't mistake [text] {for flow}\n";
    assert!(duplicate_anchor_names(yaml).is_empty());
}

#[test]
fn flow_collection_keys_find_adjacent_anchor_values() {
    for (key_kind, key) in [("sequence", "[x]"), ("map", "{}")] {
        for compact in [false, true] {
            let (prefix, item, field, _) = layout(compact);
            let yaml = format!(
                "trusted: &shared {{ KEY: safe }}\nmetadata: {{{key}:&shared {{ KEY: hostile }}}}\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
            );
            assert_eq!(
                duplicate_anchor_names(&yaml),
                ["shared".to_owned()].into_iter().collect(),
                "{key_kind} key ({compact})"
            );
            let steps = &step_lists(&yaml)[0].1;
            assert!(
                steps[0].invalid_anchor,
                "{key_kind}, {compact}: {}",
                steps[0].text
            );
            let found = problem(&files(".github/workflows/release.yml", &yaml));
            assert!(
                found.contains("duplicate YAML anchor in document"),
                "{key_kind}, {compact}: {found}"
            );
        }
    }
}

#[test]
fn explicit_mapping_key_anchors_join_the_document_namespace() {
    for compact in [false, true] {
        let (prefix, item, field, _) = layout(compact);
        let yaml = format!(
            "trusted: &shared {{ KEY: safe }}\n? &shared {{ KEY: hostile }}\n: replacement\n{prefix}{item}\n{field}name: Env alias\n{field}env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "explicit mapping key ({compact})"
        );
        let steps = &step_lists(&yaml)[0].1;
        assert!(steps[0].invalid_anchor, "{compact}: {}", steps[0].text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{compact}: {found}"
        );
    }
}

#[test]
fn multiline_quoted_scalars_do_not_declare_anchors() {
    let yaml = "trusted: &shared safe\ntext: \"line\n  &shared ordinary\n  end\"\nsingle: &single safe\nsingle_text: 'line\n  &single ordinary\n  end'\nselected: *shared\nselected_single: *single\n";
    assert!(duplicate_anchor_names(yaml).is_empty());
}

#[test]
fn inline_head_block_scalar_does_not_hide_sibling_anchor_declarations() {
    for compact in [false, true] {
        let (prefix, item, _, _) = layout(compact);
        let inline_field_indent = item.len() - item.trim_start().len() + 2;
        let block_indent = inline_field_indent + 2;
        let env_child_indent = inline_field_indent + if compact { 1 } else { 2 };
        let fields = " ".repeat(inline_field_indent);
        let body = " ".repeat(block_indent);
        let env_child = " ".repeat(env_child_indent);
        let yaml = format!(
            "trusted: &shared {{ KEY: safe }}\n{prefix}{item} run: |\n{body}printf '%s' '&literal_only'\n{fields}env: &shared\n{env_child}KEY: hostile\n{item} env: *shared\n"
        );
        assert_eq!(
            duplicate_anchor_names(&yaml),
            ["shared".to_owned()].into_iter().collect(),
            "inline block scalar ({compact})"
        );
        assert!(!duplicate_anchor_names(&yaml).contains("literal_only"));
        let steps = &step_lists(&yaml)[0].1;
        assert_eq!(steps.len(), 2);
        assert!(steps[1].invalid_anchor, "{compact}: {}", steps[1].text);
        let found = problem(&files(".github/workflows/release.yml", &yaml));
        assert!(
            found.contains("duplicate YAML anchor in document"),
            "{compact}: {found}"
        );
    }
}

#[test]
fn inline_head_run_anchors_use_real_mapping_columns() {
    for (compact, yaml, expected) in [
        (
            false,
            "jobs:\n  build:\n    steps:\n      - run: &script |\n          printf '%s\\n' 'key: value'\n      - name: Script alias\n        run: *script\n",
            "printf '%s\\n' 'key: value'\n",
        ),
        (
            true,
            "jobs:\n build:\n  steps:\n   - run: &script |\n      printf '%s\\n' 'key: value'\n   - name: Script alias\n     run: *script\n",
            "printf '%s\\n' 'key: value'\n",
        ),
    ] {
        let steps = &step_lists(yaml)[0].1;
        assert_eq!(steps.len(), 2);
        for step in steps {
            assert!(!step.unexpected_nested, "{compact}: {}", step.text);
            assert_eq!(step.run_scalar.as_deref(), Some(expected));
        }
    }
}

#[test]
fn inline_head_mapping_columns_reject_mixed_indentation() {
    let malformed = "jobs:\n build:\n  steps:\n   - run: &script |\n      echo safe\n    name: Mixed width\n   - name: Alias\n     run: *script\n";
    let steps = &step_lists(malformed)[0].1;
    assert!(steps[0].unexpected_nested);
}
