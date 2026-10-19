//! Invalid projections and bounded-expansion failures never fall back silently.

use super::{anchor, job, mapping, step, workflow};
use crate::yaml::share::{share_workflow_nodes, validation};
use crate::{RenderError, yaml::Yaml};

fn rejects(source: Yaml, expected: &str) {
    let error = share_workflow_nodes(source).expect_err("invalid projection must reject");
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn duplicate_anchors_are_global_across_scalar_and_map_kinds() -> Result<(), RenderError> {
    let name = anchor("same")?;
    let env = Yaml::AnchoredMap {
        name: name.clone(),
        entries: vec![("KEY".into(), Yaml::str("value"))],
    };
    let command = Yaml::Map(vec![(
        "run".into(),
        Yaml::AnchoredScalar {
            name,
            value: "printf true".into(),
        },
    )]);
    rejects(workflow(vec![job(env, vec![command])]), "duplicate_anchor");
    Ok(())
}

#[test]
fn unknown_and_forward_aliases_fail_before_rewriting() -> Result<(), RenderError> {
    let alias = Yaml::Map(vec![("run".into(), Yaml::Alias(anchor("later")?))]);
    let definition = Yaml::Map(vec![(
        "run".into(),
        Yaml::AnchoredScalar {
            name: anchor("later")?,
            value: "printf true".into(),
        },
    )]);
    rejects(
        workflow(vec![job(mapping("root"), vec![alias.clone()])]),
        "unknown_forward_or_cyclic_alias",
    );
    rejects(
        workflow(vec![job(mapping("root"), vec![alias, definition])]),
        "unknown_forward_or_cyclic_alias",
    );
    Ok(())
}

#[test]
fn self_referential_maps_are_not_expandable() -> Result<(), RenderError> {
    let env = Yaml::AnchoredMap {
        name: anchor("cycle")?,
        entries: vec![("SELF".into(), Yaml::Alias(anchor("cycle")?))],
    };
    rejects(
        workflow(vec![job(env, vec![])]),
        "unknown_forward_or_cyclic_alias",
    );
    Ok(())
}

#[test]
fn anchors_cannot_replace_jobs_or_escape_supported_value_scopes() -> Result<(), RenderError> {
    let document = workflow(vec![Yaml::AnchoredMap {
        name: anchor("job")?,
        entries: vec![],
    }]);
    rejects(document, "map_anchor_scope");
    let source = Yaml::Map(vec![(
        "defaults".into(),
        Yaml::AnchoredScalar {
            name: anchor("outside")?,
            value: "hello".into(),
        },
    )]);
    rejects(source, "scalar_anchor_scope");
    let env = Yaml::AnchoredMap {
        name: anchor("env")?,
        entries: vec![],
    };
    let mut source = workflow(vec![job(env, vec![])]);
    if let Yaml::Map(root) = &mut source {
        root.push(("unexpected".into(), Yaml::Alias(anchor("env")?)));
    }
    rejects(source, "alias_scope_or_kind");
    Ok(())
}

#[test]
fn mapping_alias_cannot_be_a_run_scalar() -> Result<(), RenderError> {
    let env = Yaml::AnchoredMap {
        name: anchor("env")?,
        entries: vec![],
    };
    let run = Yaml::Map(vec![("run".into(), Yaml::Alias(anchor("env")?))]);
    rejects(workflow(vec![job(env, vec![run])]), "alias_scope_or_kind");
    Ok(())
}

#[test]
fn merges_duplicate_keys_and_comment_injected_tags_reject() {
    for entries in [
        vec![("<<".into(), Yaml::str("merge"))],
        vec![
            ("KEY".into(), Yaml::str("a")),
            ("KEY".into(), Yaml::str("b")),
        ],
    ] {
        rejects(
            workflow(vec![job(Yaml::Map(entries), vec![])]),
            "duplicate_or_merge_key",
        );
    }
    let tag = Yaml::annotated("value", "comment\nunsafe: !!python/object {}");
    rejects(
        workflow(vec![job(
            mapping("root"),
            vec![Yaml::Map(vec![("run".into(), tag)])],
        )]),
        "multiline_comment",
    );
}

#[test]
fn excessive_depth_is_rejected_before_recursive_rendering() {
    let mut node = Yaml::str("leaf");
    for _ in 0..34 {
        node = Yaml::Seq(vec![node]);
    }
    rejects(Yaml::Map(vec![("deep".into(), node)]), "depth");
}

#[test]
fn excessive_nodes_are_rejected_before_projection() {
    let many = Yaml::Seq(vec![Yaml::Bool(false); 262_145]);
    rejects(Yaml::Map(vec![("many".into(), many)]), "nodes");
}

#[test]
fn repeated_aliases_cannot_amplify_past_expanded_byte_budget() -> Result<(), RenderError> {
    let command = Yaml::AnchoredScalar {
        name: anchor("large")?,
        value: "x".repeat(1024 * 1024),
    };
    let mut steps = vec![Yaml::Map(vec![("run".into(), command)])];
    for _ in 0..20 {
        steps.push(Yaml::Map(vec![(
            "run".into(),
            Yaml::Alias(anchor("large")?),
        )]));
    }
    rejects(
        workflow(vec![job(mapping("root"), steps)]),
        "expanded_bytes",
    );
    Ok(())
}

#[test]
fn typed_tag_like_strings_stay_quoted_data() -> Result<(), RenderError> {
    let source = workflow(vec![job(mapping("root"), vec![step("!!python/object {}")])]);
    let shared = share_workflow_nodes(source.clone())?;
    assert_eq!(validation::expand(&shared)?.value, source);
    Ok(())
}

#[test]
fn oversized_scalar_rejects_before_escaping_or_cloning() {
    rejects(
        workflow(vec![job(
            mapping("root"),
            vec![Yaml::Map(vec![(
                "run".into(),
                Yaml::str("x".repeat(16 * 1024 * 1024 + 1)),
            )])],
        )]),
        "expanded_bytes",
    );
}

#[test]
fn mapping_and_whole_step_anchor_kinds_cannot_cross() -> Result<(), RenderError> {
    let entries = vec![
        ("name".into(), Yaml::str("step-shaped mapping")),
        ("run".into(), Yaml::str("printf true")),
    ];
    let env = Yaml::AnchoredMap {
        name: anchor("mapping")?,
        entries: entries.clone(),
    };
    rejects(
        workflow(vec![job(env, vec![Yaml::Alias(anchor("mapping")?)])]),
        "alias_scope_or_kind",
    );
    let whole_step = Yaml::AnchoredMap {
        name: anchor("step")?,
        entries,
    };
    rejects(
        workflow(vec![
            job(mapping("first"), vec![whole_step]),
            job(Yaml::Alias(anchor("step")?), vec![]),
        ]),
        "alias_scope_or_kind",
    );
    Ok(())
}
