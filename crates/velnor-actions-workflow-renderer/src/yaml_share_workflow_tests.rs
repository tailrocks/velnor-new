//! Exact-node sharing and full typed workflow equivalence.

use super::{share_workflow_nodes, validation};
use crate::{
    RenderError,
    yaml::{AnchorName, Yaml, render_yaml},
};

pub(super) fn anchor(value: &str) -> Result<AnchorName, RenderError> {
    AnchorName::new(value).ok_or_else(|| validation::invalid("test_anchor"))
}

pub(super) fn mapping(value: &str) -> Yaml {
    Yaml::Map(vec![("LONG_VARIABLE".into(), Yaml::str(value.repeat(12)))])
}

pub(super) fn step(value: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".into(),
            Yaml::str("A repeated complete action or shell step"),
        ),
        (
            "run".into(),
            Yaml::str("printf 'same long command, still evaluated in its own job context'"),
        ),
        ("env".into(), mapping(value)),
        (
            "if".into(),
            Yaml::str("${{ success() && github.ref != '' }}"),
        ),
    ])
}

pub(super) fn workflow(jobs: Vec<Yaml>) -> Yaml {
    Yaml::Map(vec![
        ("name".into(), Yaml::str("CI")),
        (
            "permissions".into(),
            Yaml::Map(vec![("contents".into(), Yaml::str("read"))]),
        ),
        (
            "jobs".into(),
            Yaml::Map(
                jobs.into_iter()
                    .enumerate()
                    .map(|(i, job)| (format!("job-{i}"), job))
                    .collect(),
            ),
        ),
    ])
}

pub(super) fn job(env: Yaml, steps: Vec<Yaml>) -> Yaml {
    Yaml::Map(vec![
        ("runs-on".into(), Yaml::str("ubuntu-26.04")),
        ("env".into(), env),
        ("steps".into(), Yaml::Seq(steps)),
    ])
}

fn action_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".into(),
            Yaml::str("Repeated checkout with exact inputs"),
        ),
        (
            "uses".into(),
            Yaml::str("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"),
        ),
        ("with".into(), mapping("unchanged-action-input")),
    ])
}

#[test]
fn exact_fields_and_steps_share_with_nested_run_aliases() -> Result<(), RenderError> {
    let source = workflow(vec![
        job(
            mapping("same-job-environment"),
            vec![
                step("same-step-env"),
                step("same-step-env"),
                action_step(),
                action_step(),
            ],
        ),
        job(
            mapping("same-job-environment"),
            vec![
                step("same-step-env"),
                step("same-step-env"),
                action_step(),
                action_step(),
            ],
        ),
    ]);
    let shared = share_workflow_nodes(source.clone())?;
    assert_eq!(validation::expand(&shared)?.value, source);
    let text = render_yaml(&shared);
    assert!(text.contains("env: &m"));
    assert!(text.contains("env: *m"));
    assert!(text.contains("with:"));
    assert_no_unused_declarations(&shared);
    assert!(text.contains("- &s"));
    assert!(text.contains("- *s"));
    assert!(
        !text.contains("run: &r"),
        "whole-step aliases removed all nested run uses"
    );
    assert!(text.len() < render_yaml(&source).len());
    assert_eq!(
        share_workflow_nodes(source)?,
        shared,
        "deterministic representation"
    );
    Ok(())
}

#[test]
fn different_environments_and_job_boundaries_keep_every_field() -> Result<(), RenderError> {
    let source = workflow(vec![
        job(mapping("job-a"), vec![step("step-a"), step("step-b")]),
        job(mapping("job-b"), vec![step("step-a"), step("step-b")]),
    ]);
    let shared = share_workflow_nodes(source.clone())?;
    assert_eq!(validation::expand(&shared)?.value, source);
    let Yaml::Map(root) = validation::expand(&shared)?.value else {
        return Err(validation::invalid("test_root"));
    };
    let Yaml::Map(jobs) = &root[2].1 else {
        return Err(validation::invalid("test_jobs"));
    };
    assert_ne!(jobs[0].1, jobs[1].1, "job environment never projected away");
    Ok(())
}

#[test]
fn mapping_equality_preserves_types_order_and_comments() -> Result<(), RenderError> {
    let values = [
        Yaml::str("false"),
        Yaml::Bool(false),
        Yaml::quoted("false"),
        Yaml::annotated("false", "literal comment"),
    ];
    let mut steps = Vec::new();
    for value in values {
        steps.push(Yaml::Map(vec![(
            "env".into(),
            Yaml::Map(vec![("VALUE".into(), value)]),
        )]));
    }
    let source = workflow(vec![job(mapping("root"), steps)]);
    assert_eq!(
        validation::expand(&share_workflow_nodes(source.clone())?)?.value,
        source
    );
    Ok(())
}

#[test]
fn existing_declaration_is_not_removed_by_an_equivalent_step() -> Result<(), RenderError> {
    let command = "printf 'keep this earlier anchor definition for later use'";
    let first = Yaml::Map(vec![("run".into(), Yaml::str(command))]);
    let definition = Yaml::Map(vec![(
        "run".into(),
        Yaml::AnchoredScalar {
            name: anchor("r1")?,
            value: command.into(),
        },
    )]);
    let later = Yaml::Map(vec![
        ("name".into(), Yaml::str("later")),
        ("run".into(), Yaml::Alias(anchor("r1")?)),
    ]);
    let source = workflow(vec![job(mapping("root"), vec![first, definition, later])]);
    let expected = validation::expand(&source)?.value;
    let shared = share_workflow_nodes(source)?;
    assert_eq!(validation::expand(&shared)?.value, expected);
    assert!(render_yaml(&shared).contains("&r1"));
    assert_no_unused_declarations(&shared);
    Ok(())
}

#[test]
fn generated_names_skip_prior_map_and_scalar_names_globally() -> Result<(), RenderError> {
    let Yaml::Map(entries) = mapping("existing") else {
        return Err(validation::invalid("test_map"));
    };
    let env = Yaml::AnchoredMap {
        name: anchor("m1")?,
        entries,
    };
    let mut steps = vec![step("same"), step("same"), step("same")];
    for (index, step) in steps.iter_mut().enumerate() {
        if let Yaml::Map(entries) = step {
            entries[0].1 = Yaml::str(format!("distinct step {index}"));
        }
    }
    let original = workflow(vec![job(env, steps)]);
    let expected = validation::expand(&original)?.value;
    let shared = share_workflow_nodes(original)?;
    assert_eq!(validation::expand(&shared)?.value, expected);
    assert!(render_yaml(&shared).contains("env: &m2"));
    Ok(())
}

#[cfg(test)]
#[path = "yaml_share_rejections_tests.rs"]
mod rejections;

#[test]
fn declared_task_flow_inputs_keep_their_original_typed_representation() -> Result<(), RenderError> {
    let action = Yaml::Map(vec![
        ("name".into(), Yaml::str("declared task")),
        (
            "uses".into(),
            Yaml::str("./.github/actions/declared-task-0"),
        ),
        (
            "with".into(),
            Yaml::FlowMap(vec![("j".into(), "a-long-exact-job-input".repeat(8))]),
        ),
    ]);
    let source = workflow(vec![job(mapping("root"), vec![action.clone(), action])]);
    let shared = share_workflow_nodes(source.clone())?;
    assert_eq!(validation::expand(&shared)?.value, source);
    assert!(render_yaml(&shared).contains("with: {"));
    Ok(())
}

#[test]
fn quoted_job_environment_keys_are_valid_and_remain_unshared() -> Result<(), RenderError> {
    let env = Yaml::Map(vec![(
        "ON".into(),
        Yaml::str("valid-posix-environment-variable".repeat(8)),
    )]);
    let source = workflow(vec![job(env.clone(), vec![]), job(env, vec![])]);
    assert_eq!(share_workflow_nodes(source.clone())?, source);
    Ok(())
}

#[test]
fn quoted_nested_keys_skip_field_anchors_but_preserve_legal_whole_steps() -> Result<(), RenderError>
{
    let quoted = Yaml::Map(vec![("ON".into(), Yaml::str("a-long-value".repeat(12)))]);
    let action = Yaml::Map(vec![
        (
            "name".into(),
            Yaml::str("exact action with quoted nested keys"),
        ),
        ("uses".into(), Yaml::str("./.github/actions/exact-action")),
        ("env".into(), quoted.clone()),
        ("with".into(), quoted),
    ]);
    let source = workflow(vec![job(mapping("root"), vec![action.clone(), action])]);
    let shared = share_workflow_nodes(source.clone())?;
    assert_eq!(validation::expand(&shared)?.value, source);
    let text = render_yaml(&shared);
    assert!(!text.contains("env: &m") && !text.contains("with: &m"));
    assert!(text.contains("- &s") && text.contains("\"ON\":"));
    Ok(())
}

fn assert_no_unused_declarations(node: &Yaml) {
    fn collect(
        node: &Yaml,
        declarations: &mut std::collections::BTreeSet<String>,
        aliases: &mut std::collections::BTreeSet<String>,
    ) {
        match node {
            Yaml::AnchoredScalar { name, .. } => {
                declarations.insert(name.as_str().into());
            }
            Yaml::Alias(name) => {
                aliases.insert(name.as_str().into());
            }
            Yaml::AnchoredMap { name, entries } => {
                declarations.insert(name.as_str().into());
                for (_, value) in entries {
                    collect(value, declarations, aliases);
                }
            }
            Yaml::Map(entries) => {
                for (_, value) in entries {
                    collect(value, declarations, aliases);
                }
            }
            Yaml::Seq(items) => {
                for value in items {
                    collect(value, declarations, aliases);
                }
            }
            _ => {}
        }
    }
    let mut declarations = std::collections::BTreeSet::new();
    let mut aliases = std::collections::BTreeSet::new();
    collect(node, &mut declarations, &mut aliases);
    assert_eq!(
        declarations, aliases,
        "every declaration has an actual literal use and every use resolves"
    );
}

#[test]
fn unused_nested_definitions_are_pruned_but_all_live_uses_survive() -> Result<(), RenderError> {
    let Yaml::Map(entries) = mapping("kept-live-mapping") else {
        return Err(validation::invalid("test_map"));
    };
    let env = Yaml::AnchoredMap {
        name: anchor("live")?,
        entries,
    };
    let source = workflow(vec![
        job(
            env,
            vec![
                step("repeated"),
                step("repeated"),
                action_step(),
                action_step(),
            ],
        ),
        job(Yaml::Alias(anchor("live")?), vec![]),
    ]);
    let expected = validation::expand(&source)?.value;
    let shared = share_workflow_nodes(source)?;
    assert_eq!(validation::expand(&shared)?.value, expected);
    assert_no_unused_declarations(&shared);
    let text = render_yaml(&shared);
    assert!(text.contains("&live") && text.contains("*live"));
    assert!(
        !text.contains("run: &r"),
        "whole-step aliases own all repeated run uses"
    );
    Ok(())
}
