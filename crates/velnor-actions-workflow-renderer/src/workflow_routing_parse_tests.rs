use std::fmt::Write as _;

use crate::RenderedFile;

use super::parse_final_workflow;

fn file(bytes: &str) -> RenderedFile {
    RenderedFile {
        path: ".github/workflows/parse.yml".to_owned(),
        bytes: bytes.to_owned(),
    }
}

fn error_text(file: &RenderedFile) -> String {
    match parse_final_workflow(file) {
        Err(error) => error.to_string(),
        Ok(_) => "unexpected_success".to_owned(),
    }
}

#[test]
fn parser_keeps_yaml_12_on_key_and_resolves_emitter_aliases() {
    let workflow = file(
        "on: [push]\njobs:\n  first:\n    runs-on: &runner ubuntu-26.04\n  second:\n    runs-on: *runner\n",
    );
    let document = parse_final_workflow(&workflow).expect("workflow parses");
    let mapping = document.as_mapping().expect("top-level map");
    assert!(mapping.iter().any(|(key, _)| key.as_str() == Some("on")));
    let jobs = mapping
        .iter()
        .find(|(key, _)| key.as_str() == Some("jobs"))
        .and_then(|(_, value)| value.as_mapping())
        .expect("jobs map");
    assert_eq!(jobs.len(), 2);
}

#[test]
fn parser_rejects_duplicate_keys_and_multiple_documents() {
    for text in [
        "jobs:\n  duplicate:\n    runs-on: ubuntu-26.04\n    runs-on: ubuntu-24.04\n",
        "jobs: {}\n---\njobs: {}\n",
    ] {
        assert!(parse_final_workflow(&file(text)).is_err());
    }
}

#[test]
fn parser_rejects_merge_keys_and_unknown_tags() {
    for text in [
        "jobs:\n  check:\n    <<: {runs-on: ubuntu-26.04}\n",
        "jobs:\n  check:\n    !!str <<: {runs-on: ubuntu-26.04}\n",
        "jobs:\n  check:\n    runs-on: !runner ubuntu-26.04\n",
    ] {
        assert!(parse_final_workflow(&file(text)).is_err());
    }
}

#[test]
fn parser_accepts_explicit_core_scalar_tags_and_rejects_unresolved_aliases() {
    let tagged = file("!!map\njobs:\n  check:\n    runs-on: !!str ubuntu-26.04\n");
    assert!(parse_final_workflow(&tagged).is_ok());
    let unresolved = file("jobs:\n  check:\n    runs-on: *missing\n");
    assert!(parse_final_workflow(&unresolved).is_err());
}

#[test]
fn parser_bounds_input_scalar_event_and_nesting_resources() {
    let input = format!("jobs: {{}}\n{}", " ".repeat(super::MAX_WORKFLOW_BYTES));
    assert!(error_text(&file(&input)).contains(":workflow_bytes"));

    let scalar = format!(
        "jobs: {{}}\nvalue: {}\n",
        "x".repeat(super::MAX_SCALAR_BYTES + 1)
    );
    assert!(error_text(&file(&scalar)).contains(":scalar_bytes"));

    let nested = format!(
        "jobs: {{}}\nvalue: {}x{}\n",
        "[".repeat(super::MAX_NESTING_DEPTH + 1),
        "]".repeat(super::MAX_NESTING_DEPTH + 1)
    );
    assert!(error_text(&file(&nested)).contains(":expanded_depth"));

    let events = format!("jobs: [{}]\n", "x,".repeat(super::MAX_SYNTAX_EVENTS));
    assert!(error_text(&file(&events)).contains(":syntax_events"));
}

#[test]
fn parser_bounds_expanded_scalar_bytes_before_loading_aliases() {
    let large_scalar = "x".repeat(super::MAX_SCALAR_BYTES);
    let mut source = format!("base: &base '{large_scalar}'\ncopies: [");
    for index in 0..70 {
        if index > 0 {
            source.push_str(", ");
        }
        source.push_str("*base");
    }
    source.push_str("]\n");
    assert!(error_text(&file(&source)).contains(":expanded_scalar_bytes"));
}

#[test]
fn parser_bounds_expanded_anchor_subtrees_before_loading() {
    let mut source = String::from("jobs: {}\nvalues:\n");
    source.push_str("  item-0: &item-0 [leaf]\n");
    for level in 1..24 {
        assert!(
            writeln!(
                source,
                "  item-{level}: &item-{level} [*item-{}, *item-{}]",
                level - 1,
                level - 1
            )
            .is_ok()
        );
    }
    assert!(error_text(&file(&source)).contains("expanded_nodes"));
}
