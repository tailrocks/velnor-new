// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_workflows::{reads_secret_in_condition, reads_secret_in_text, reads_secret_in_value};

const READS: [&str; 5] = [
    "${{ secrets.A }}",
    "${{ secrets['A'] }}",
    "${{ SECRETS.A }}",
    "${{ toJSON(secrets) }}",
    "${{ format('{0}', secrets.A) }}",
];

#[test]
fn every_form_of_the_secrets_context_in_text_is_a_read() {
    for text in [
        "${{ secrets.TOKEN }}",
        "${{ secrets['TOKEN'] }}",
        "${{ SECRETS.TOKEN }}",
        "${{ toJSON(secrets) }}",
        "a ${{ 'x' }} b ${{ secrets.T }}",
        "${{ '}}' && secrets.T }}",
    ] {
        assert!(reads_secret_in_text(text), "{text}");
    }
    for text in [
        "echo secrets.TOKEN",
        "${{ 'secrets.TOKEN' }}",
        "${{ github.ref }} secrets.A",
        "${{ vars.secrets_name }}",
    ] {
        assert!(!reads_secret_in_text(text), "{text}");
    }
}

#[test]
fn a_condition_is_read_with_or_without_the_delimiters() {
    assert!(reads_secret_in_condition("secrets.A != ''"));
    assert!(reads_secret_in_condition("${{ true }} && secrets.A"));
    assert!(reads_secret_in_condition("${{ secrets.A != '' }}"));
    assert!(!reads_secret_in_condition("github.ref == 'secrets.A'"));
}

#[test]
fn every_form_of_a_secret_read_in_a_job_tree_is_a_read() {
    for read in READS {
        let value: Value = json!({"steps": [{"env": {"TOKEN": read}}]});
        assert!(reads_secret_in_value(&value), "value {read}");
        let key: Value = json!({"steps": [{"env": {read: "x"}}]});
        assert!(reads_secret_in_value(&key), "key {read}");
    }
    let mention = json!({"env": {"NOTE": "secrets.A"}, "steps": [{"run": "echo secrets.A"}]});
    assert!(!reads_secret_in_value(&mention));
}

#[test]
fn a_condition_is_read_as_one_expression_in_a_job_tree() {
    for condition in [
        "secrets.A != ''",
        "${{ secrets.A != '' }}",
        "${{ true }} && secrets.A != ''",
        "secrets.A != '' && ${{ true }}",
    ] {
        let tree = json!({"steps": [{"if": condition, "run": "x"}]});
        assert!(reads_secret_in_value(&tree), "{condition}");
    }
    let tree = json!({"steps": [{"if": "github.ref == 'secrets.A'"}]});
    assert!(!reads_secret_in_value(&tree));
}
