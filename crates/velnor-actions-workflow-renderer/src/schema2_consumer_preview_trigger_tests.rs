//! Actual native transport guard compensates the audit's whole-on comment scope.
use super::*;
use crate::schema2::consumer_release_eligibility::preview_trigger;

fn document() -> Yaml {
    Yaml::Map(vec![
        (
            "on".to_owned(),
            Yaml::Map(vec![
                preview_trigger::trigger("main"),
                ("workflow_dispatch".to_owned(), steps::recovery_inputs()),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(Vec::new())),
    ])
}

fn events(document: &mut Yaml) -> &mut Vec<(String, Yaml)> {
    let Yaml::Map(fields) = document else {
        panic!("document")
    };
    let (_, Yaml::Map(events)) = &mut fields[0] else {
        panic!("events")
    };
    events
}

fn assert_rejected(document: &Yaml) {
    let error = render_preview_document(&fixture(), document).expect_err("closed native trigger");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem)
        if problem == "native_preview_trigger_mismatch"),
        "{error}"
    );
}

#[test]
fn canonical_native_trigger_carries_justified_annotation_and_renders() {
    let rendered =
        render_preview_document(&fixture(), &document()).expect("closed native triggers");
    assert!(rendered.contains("- CI # zizmor: ignore[dangerous-triggers]"));
    assert!(rendered.contains("workflow_run:"));
    assert!(rendered.contains("workflow_dispatch:"));
}

#[test]
fn annotated_native_transport_rejects_sibling_unknown_and_duplicate_events() {
    for event in [
        "pull_request_target",
        "issue_comment",
        "push",
        "unknown",
        "workflow_run",
        "workflow_dispatch",
    ] {
        let mut value = document();
        events(&mut value).push((event.to_owned(), Yaml::Null));
        assert_rejected(&value);
    }
    let mut duplicate_on = document();
    let Yaml::Map(fields) = &mut duplicate_on else {
        panic!("document")
    };
    fields.push(("on".to_owned(), Yaml::Map(Vec::new())));
    assert_rejected(&duplicate_on);
}

#[test]
fn annotated_native_transport_rejects_generic_or_mutated_completion_filters() {
    for replacement in [
        Yaml::Null,
        Yaml::Seq(vec![Yaml::str("CI")]),
        Yaml::Map(Vec::new()),
    ] {
        let mut value = document();
        events(&mut value)[0].1 = replacement;
        assert_rejected(&value);
    }
    for (field, replacement) in [
        ("workflows", "Other CI"),
        ("types", "requested"),
        ("branches", "other"),
    ] {
        let mut value = document();
        let Yaml::Map(filters) = &mut events(&mut value)[0].1 else {
            panic!("filters")
        };
        let (_, target) = filters
            .iter_mut()
            .find(|(key, _)| key == field)
            .expect("filter");
        *target = Yaml::Seq(vec![Yaml::str(replacement)]);
        assert_rejected(&value);
    }
    let mut duplicate_filter = document();
    let Yaml::Map(filters) = &mut events(&mut duplicate_filter)[0].1 else {
        panic!("filters")
    };
    filters.push(("branches".to_owned(), Yaml::Seq(vec![Yaml::str("main")])));
    assert_rejected(&duplicate_filter);
}

#[test]
fn native_transport_keeps_manual_recovery_schema_closed() {
    let mut value = document();
    events(&mut value)[1].1 = Yaml::Null;
    assert_rejected(&value);
    let mut value = document();
    events(&mut value).remove(1);
    assert_rejected(&value);
}
