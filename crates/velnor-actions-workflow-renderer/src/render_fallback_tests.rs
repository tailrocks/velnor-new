use super::{
    render_checked_workflow, render_checked_workflow_before_extraction, render_marked_workflow,
};
use crate::{
    RenderError, marker,
    workflow_size::MAX_WORKFLOW_BYTES,
    yaml::{AnchorName, Yaml, render_yaml, render_yaml_compact, share_workflow_nodes},
};

const VERSION: &str = "0.1.4";

fn workflow(command: &str, repeats: usize) -> Yaml {
    let steps = (0..repeats)
        .map(|index| {
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(format!("step-{index}"))),
                ("run".to_owned(), Yaml::str(command)),
            ])
        })
        .collect();
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "job".to_owned(),
            Yaml::Map(vec![
                ("timeout-minutes".to_owned(), Yaml::Int(5)),
                ("steps".to_owned(), Yaml::Seq(steps)),
            ]),
        )]),
    )])
}

fn nested_compactable_workflow(padding: usize) -> Yaml {
    let entries = (0..128)
        .map(|index| {
            (
                format!("entry-{index:03}"),
                Yaml::str(format!("value-{index:03}")),
            )
        })
        .collect();
    let mut nested = Yaml::Map(entries);
    for level in (0..8).rev() {
        nested = Yaml::Map(vec![(format!("level-{level}"), nested)]);
    }
    Yaml::Map(vec![
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![
                    ("timeout-minutes".to_owned(), Yaml::Int(5)),
                    ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
                    ("env".to_owned(), nested),
                ]),
            )]),
        ),
        ("padding".to_owned(), Yaml::str("x".repeat(padding))),
    ])
}

fn compact_equivalence_workflow() -> Yaml {
    let defaults = AnchorName::new("defaults").expect("valid anchor");
    let script = AnchorName::new("script").expect("valid anchor");
    Yaml::Map(vec![
        (
            "on".to_owned(),
            Yaml::Map(vec![("push".to_owned(), Yaml::Map(Vec::new()))]),
        ),
        (
            "defaults".to_owned(),
            Yaml::AnchoredMap {
                name: defaults.clone(),
                entries: vec![
                    ("shell".to_owned(), Yaml::str("bash")),
                    (
                        "run".to_owned(),
                        Yaml::AnchoredScalar {
                            name: script.clone(),
                            value: "set -eu\nprintf '%s\\n' 'hello: world'\n".to_owned(),
                        },
                    ),
                    (
                        "env".to_owned(),
                        Yaml::Map(vec![
                            ("MODE".to_owned(), Yaml::str("on")),
                            ("COUNT".to_owned(), Yaml::Int(2)),
                        ]),
                    ),
                ],
            },
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
                    (
                        "steps".to_owned(),
                        Yaml::Seq(vec![
                            Yaml::Map(vec![
                                ("name".to_owned(), Yaml::str("Checkout")),
                                (
                                    "uses".to_owned(),
                                    Yaml::str("actions/checkout@0123456789abcdef"),
                                ),
                            ]),
                            Yaml::Alias(defaults),
                            Yaml::Map(vec![
                                ("name".to_owned(), Yaml::str("Run shared script")),
                                ("run".to_owned(), Yaml::Alias(script)),
                                (
                                    "with".to_owned(),
                                    Yaml::Map(vec![(
                                        "nested".to_owned(),
                                        Yaml::Seq(vec![Yaml::Map(vec![
                                            ("first".to_owned(), Yaml::str("one")),
                                            (
                                                "second".to_owned(),
                                                Yaml::Seq(vec![
                                                    Yaml::str("two"),
                                                    Yaml::Map(vec![(
                                                        "third".to_owned(),
                                                        Yaml::Bool(true),
                                                    )]),
                                                ]),
                                            ),
                                        ])]),
                                    )]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            )]),
        ),
    ])
}

#[test]
fn under_cap_workflow_is_byte_identical_even_when_sharing_is_profitable() -> Result<(), RenderError>
{
    let command = format!("echo {}", "x".repeat(8_000));
    let document = workflow(&command, 2);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    assert!(canonical.len() < MAX_WORKFLOW_BYTES);
    assert_eq!(
        render_marked_workflow(".github/workflows/ci.yml", &document, VERSION)?,
        canonical
    );
    assert_eq!(
        render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)?,
        canonical
    );
    Ok(())
}

#[test]
fn non_workflow_action_document_keeps_canonical_bytes() -> Result<(), RenderError> {
    let document = Yaml::Map(vec![(
        "runs".to_owned(),
        Yaml::Map(vec![
            ("using".to_owned(), Yaml::str("composite")),
            (
                "steps".to_owned(),
                Yaml::Seq(
                    (0..3)
                        .map(|_| {
                            Yaml::Map(vec![
                                ("shell".to_owned(), Yaml::str("bash")),
                                ("run".to_owned(), Yaml::str("echo repeated")),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
    )]);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    let rendered =
        render_checked_workflow(".github/actions/sample/action.yml", &document, VERSION)?;
    assert_eq!(rendered, canonical);
    assert!(!rendered.contains("&r"));
    assert!(!rendered.contains("*r"));
    Ok(())
}

#[test]
fn over_cap_workflow_uses_marked_step_run_aliases() -> Result<(), RenderError> {
    let command = "x".repeat(260_000);
    let document = workflow(&command, 2);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    assert!(canonical.len() > MAX_WORKFLOW_BYTES);
    let rendered = render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)?;
    assert!(rendered.starts_with(&marker::marker_for_version(VERSION)?));
    assert!(rendered.len() < MAX_WORKFLOW_BYTES);
    assert!(rendered.contains("run: &r1"));
    assert!(rendered.contains("run: *r1"));
    Ok(())
}

#[test]
fn pre_extraction_fallback_shares_values_deterministically() -> Result<(), RenderError> {
    let command = "x".repeat(260_000);
    let document = workflow(&command, 2);
    let first =
        render_checked_workflow_before_extraction(".github/workflows/ci.yml", &document, VERSION)?;
    let second =
        render_checked_workflow_before_extraction(".github/workflows/ci.yml", &document, VERSION)?;

    assert_eq!(first, second);
    assert!(first.starts_with(&marker::marker_for_version(VERSION)?));
    assert!(first.len() <= MAX_WORKFLOW_BYTES);
    assert!(first.contains("run: &r1"));
    assert!(first.contains("run: *r1"));
    Ok(())
}

#[test]
fn pre_extraction_fallback_keeps_the_workflow_size_limit() {
    let command = "x".repeat(MAX_WORKFLOW_BYTES + 100);
    let document = workflow(&command, 1);
    let error =
        render_checked_workflow_before_extraction(".github/workflows/ci.yml", &document, VERSION)
            .expect_err("irreducible workflow must fail");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")));
}

#[test]
fn irreducible_over_cap_workflow_still_fails_closed() {
    let command = "x".repeat(MAX_WORKFLOW_BYTES + 100);
    let document = workflow(&command, 1);
    let error = render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)
        .expect_err("irreducible workflow must retain the production guard");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")));
}

#[test]
fn compact_emission_preserves_nested_sequence_maps_anchors_and_scripts() -> Result<(), RenderError>
{
    let document = compact_equivalence_workflow();
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    let compact = marker::with_marker(VERSION, &render_yaml_compact(&document))?;
    assert!(canonical.contains("      - name: Checkout\n"));
    assert!(compact.contains("   -\n    name: Checkout\n"));

    let canonical_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: canonical,
    };
    let compact_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: compact,
    };
    let canonical_yaml = crate::workflow_routing::parse_authority_document(&canonical_file)?;
    let compact_yaml = crate::workflow_routing::parse_authority_document(&compact_file)?;
    assert_eq!(canonical_yaml, compact_yaml);
    Ok(())
}

#[test]
fn compact_fallback_fits_the_exact_workflow_cap_and_rejects_one_more_byte()
-> Result<(), RenderError> {
    let base = nested_compactable_workflow(0);
    let shared_base = share_workflow_nodes(base.clone())?;
    let compact_base = marker::with_marker(VERSION, &render_yaml_compact(&shared_base))?;
    let canonical_base = marker::with_marker(VERSION, &render_yaml(&base))?;
    // Replacing the empty string's `""` representation consumes two bytes.
    let padding = MAX_WORKFLOW_BYTES - compact_base.len() + 2;

    let exact_document = nested_compactable_workflow(padding);
    let exact_shared = share_workflow_nodes(exact_document.clone())?;
    let exact_compact = marker::with_marker(VERSION, &render_yaml_compact(&exact_shared))?;
    let exact_canonical = marker::with_marker(VERSION, &render_yaml(&exact_document))?;
    let exact_pretty = marker::with_marker(VERSION, &render_yaml(&exact_shared))?;
    assert_eq!(exact_compact.len(), MAX_WORKFLOW_BYTES);
    assert!(exact_canonical.len() > MAX_WORKFLOW_BYTES);
    assert!(exact_pretty.len() > MAX_WORKFLOW_BYTES);
    assert_eq!(
        render_checked_workflow(".github/workflows/ci.yml", &exact_document, VERSION)?,
        exact_compact
    );

    let over_document = nested_compactable_workflow(padding + 1);
    let over = render_checked_workflow(".github/workflows/ci.yml", &over_document, VERSION)
        .expect_err("one byte over the compact limit must fail closed");
    assert!(matches!(over, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/ci.yml:500001:500000"));
    assert!(canonical_base.len() > compact_base.len());
    Ok(())
}

#[cfg(feature = "test-render-capture")]
#[test]
fn full_tree_capture_preserves_size_gate_and_retains_complete_diagnostics()
-> Result<(), RenderError> {
    let command = "x".repeat(MAX_WORKFLOW_BYTES + 100);
    let document = workflow(&command, 1);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    assert!(render_checked_workflow(".github/workflows/ci.yml", &document, VERSION).is_err());

    crate::render::test_render_capture::clear();
    let result = crate::render::test_render_capture::with_full_tree_capture(|| {
        render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)
    });
    assert!(matches!(result, Err(RenderError::InvalidWorkflow(problem))
        if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")));
    let captured = crate::render::test_render_capture::take()
        .expect("rejected render still retains its complete diagnostic documents");
    let shared = share_workflow_nodes(document.clone())?;
    let compact = marker::with_marker(VERSION, &render_yaml_compact(&shared))?;
    assert_eq!(captured.canonical, canonical);
    assert_eq!(captured.selected, compact);
    assert!(captured.selected.len() > MAX_WORKFLOW_BYTES);
    assert!(
        crate::workflow_size::check_workflow_size(".github/workflows/ci.yml", &captured.selected)
            .is_err()
    );
    assert!(render_checked_workflow(".github/workflows/ci.yml", &document, VERSION).is_err());
    Ok(())
}
