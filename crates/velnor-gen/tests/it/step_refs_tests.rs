// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::json;

use crate::step_refs::{Callees, dangling_refs, expressions, step_refs};

fn references(expr: &str) -> Vec<(&str, &str)> {
    step_refs(expr)
        .into_iter()
        .map(|found| (found.id, found.reference))
        .collect()
}

#[test]
fn the_reference_check_reads_only_expressions_and_only_step_ids() {
    assert_eq!(
        references("steps.toolsanchor.outputs.cache-hit == 'true' && x.steps.nope.y"),
        vec![("toolsanchor", "steps.toolsanchor.outputs.cache-hit")]
    );
    assert_eq!(
        references("steps.a-b_1.outputs.x"),
        vec![("a-b_1", "steps.a-b_1.outputs.x")]
    );
    assert_eq!(
        references("toJSON(steps.toolsanchor)"),
        vec![("toolsanchor", "steps.toolsanchor")]
    );
    assert_eq!(
        expressions("run $x steps.a.b ${{ steps.c.outputs.d }} tail"),
        vec![" steps.c.outputs.d "]
    );
}

#[test]
fn a_dangling_name_sharing_a_prefix_with_a_real_step_is_reported_whole() {
    let workflow = json!({"jobs": {"rust-root": {"steps": [
        {"id": "toolsanchor", "run": "true"},
        {"run": "${{ steps.toolsanchor.outputs.cache-hit }}"},
        {"run": "${{ steps.toolsanchorx.outputs.cache-hit == 'true' }}"},
        {"run": "${{ toJSON(steps.toolsanchor2) }}"},
    ]}}});
    let (checked, dangling) = dangling_refs(&workflow, &Callees::new());
    assert_eq!(checked, 3);
    assert_eq!(
        dangling,
        vec![
            "rust-root: `steps.toolsanchor2` names no step of this job (id `toolsanchor2`) in `toJSON(steps.toolsanchor2)`",
            "rust-root: `steps.toolsanchorx.outputs.cache-hit` names no step of this job (id `toolsanchorx`) in `steps.toolsanchorx.outputs.cache-hit == 'true'`",
        ]
    );
}

#[test]
fn a_step_name_inside_a_quoted_string_is_not_a_reference() {
    assert_eq!(references("format('steps.nope.x {0}', y)"), Vec::new());
}

#[test]
fn a_closing_delimiter_inside_a_quoted_string_does_not_end_the_expression() {
    assert_eq!(
        expressions("${{ format('{{literal}} {0}', steps.missing.outputs.x) }} tail"),
        vec![" format('{{literal}} {0}', steps.missing.outputs.x) "]
    );
}

#[test]
fn a_step_reference_to_a_step_that_has_not_run_yet_is_dangling() {
    let workflow = json!({"jobs": {"build": {"steps": [
        {"id": "first", "if": "${{ steps.second.outcome == 'success' }}", "run": "true"},
        {"id": "second", "run": "true"}
    ]}}});
    assert_ne!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_bare_if_is_read_as_an_expression() {
    let workflow = json!({"jobs": {"build": {"steps": [
        {"id": "first", "run": "true"},
        {"if": "steps.missing.outcome == 'success'", "run": "true"}
    ]}}});
    assert_ne!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_job_output_may_reference_a_step_of_its_job() {
    let workflow = json!({"jobs": {"build": {
        "outputs": {"y": "${{ steps.second.outputs.y }}"},
        "steps": [{"id": "first", "run": "true"}, {"id": "second", "run": "true"}]
    }}});
    assert_eq!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_needs_output_of_a_direct_need_is_clean() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": ["build"], "steps": [{"run": "echo ${{ needs.build.outputs.key }}"}]}
    }});
    assert_eq!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_needs_output_that_the_needed_job_does_not_declare_is_dangling() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": ["build"], "steps": [{"run": "echo ${{ needs.build.outputs.missing }}"}]}
    }});
    assert_ne!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_needs_reference_to_a_job_that_is_not_a_direct_need_is_dangling() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": [], "steps": [{"run": "echo ${{ needs.build.result }}"}]}
    }});
    assert_ne!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_job_level_condition_cannot_reference_a_step_of_its_job() {
    let workflow = json!({"jobs": {"build": {
        "if": "${{ steps.first.outcome == 'success' }}",
        "steps": [{"id": "first", "run": "true"}]
    }}});
    assert_ne!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_nested_key_named_if_is_not_an_expression() {
    let workflow = json!({"jobs": {"build": {"steps": [
        {"id": "first", "uses": "example/action@v1", "with": {"if": "steps.missing"}}
    ]}}});
    assert_eq!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}

#[test]
fn a_needs_output_of_a_reusable_call_is_checked_against_the_callee() {
    let workflow = json!({"jobs": {
        "release": {"uses": "./.github/workflows/release.yml"},
        "deploy": {"needs": ["release"], "steps": [{"run": "echo ${{ needs.release.outputs.nope }}"}]}
    }});
    let callees = Callees::from([(
        "./.github/workflows/release.yml".to_owned(),
        BTreeSet::from(["digest".to_owned()]),
    )]);
    assert_ne!(dangling_refs(&workflow, &callees).1, Vec::<String>::new());
}

#[test]
fn a_needs_output_of_a_reusable_call_the_callee_declares_is_clean() {
    let workflow = json!({"jobs": {
        "release": {"uses": "./.github/workflows/release.yml"},
        "deploy": {"needs": ["release"], "steps": [{"run": "echo ${{ needs.release.outputs.digest }}"}]}
    }});
    let callees = Callees::from([(
        "./.github/workflows/release.yml".to_owned(),
        BTreeSet::from(["digest".to_owned()]),
    )]);
    assert_eq!(dangling_refs(&workflow, &callees).1, Vec::<String>::new());
}

#[test]
fn a_needs_output_of_a_remote_reusable_call_is_accepted() {
    let workflow = json!({"jobs": {
        "release": {"uses": "example/workflows/.github/workflows/release.yml@v1"},
        "deploy": {"needs": ["release"], "steps": [{"run": "echo ${{ needs.release.outputs.digest }}"}]}
    }});
    assert_eq!(
        dangling_refs(&workflow, &Callees::new()).1,
        Vec::<String>::new()
    );
}
