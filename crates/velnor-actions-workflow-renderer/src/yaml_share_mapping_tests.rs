use crate::yaml::{AnchorName, Yaml, render_yaml, share_repeated_workflow_nodes};

#[test]
fn repeated_job_and_step_input_maps_are_anchored_without_changing_expanded_data()
-> Result<(), String> {
    let env = Yaml::Map(vec![
        (
            "LONG_SETTING".to_owned(),
            Yaml::str("same long environment value"),
        ),
        (
            "TOKEN_LABEL".to_owned(),
            Yaml::str("${{ secrets.CI_TOKEN }}"),
        ),
    ]);
    let with = Yaml::Map(vec![
        ("path".to_owned(), Yaml::str("target/debug/deps")),
        (
            "key".to_owned(),
            Yaml::str("${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}"),
        ),
    ]);
    let step = |name: &str| {
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(name)),
            ("uses".to_owned(), Yaml::str("actions/cache@v4")),
            ("with".to_owned(), with.clone()),
            ("env".to_owned(), env.clone()),
        ])
    };
    let document = Yaml::Map(vec![
        (
            "metadata".to_owned(),
            Yaml::Map(vec![("env".to_owned(), env.clone())]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![
                (
                    "first".to_owned(),
                    Yaml::Map(vec![
                        ("env".to_owned(), env.clone()),
                        (
                            "steps".to_owned(),
                            Yaml::Seq(vec![step("Restore build cache one")]),
                        ),
                    ]),
                ),
                (
                    "second".to_owned(),
                    Yaml::Map(vec![
                        ("env".to_owned(), env),
                        (
                            "steps".to_owned(),
                            Yaml::Seq(vec![step("Restore build cache two")]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);
    let shared = share_repeated_workflow_nodes(document.clone());
    let rendered = render_yaml(&shared);

    assert!(rendered.contains("env: &m1"));
    assert!(rendered.contains("env: *m1"));
    assert!(rendered.contains("with: &m2"));
    assert!(rendered.contains("with: *m2"));
    assert!(!rendered.contains("- &s"));
    assert!(
        rendered.contains("metadata:\n  env:\n"),
        "outside jobs is not shared"
    );
    assert_eq!(expand_aliases(&document)?, expand_aliases(&shared)?);
    Ok(())
}

#[test]
fn identical_step_maps_are_anchored_without_changing_expanded_data() -> Result<(), String> {
    let step = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Repeated setup")),
        ("uses".to_owned(), Yaml::str("actions/cache@v4")),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                "path".to_owned(),
                Yaml::str("a sufficiently long repeated cache path"),
            )]),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "LONG_SETTING".to_owned(),
                Yaml::str("same sufficiently long repeated value"),
            )]),
        ),
    ]);
    let document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                "first".to_owned(),
                Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(vec![step.clone()]))]),
            ),
            (
                "second".to_owned(),
                Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(vec![step]))]),
            ),
        ]),
    )]);
    let shared = share_repeated_workflow_nodes(document.clone());
    let rendered = render_yaml(&shared);

    assert!(rendered.contains("- &s1"));
    assert!(rendered.contains("- *s1"));
    assert_eq!(expand_aliases(&document)?, expand_aliases(&shared)?);
    Ok(())
}

#[test]
fn repeated_input_map_aliases_are_deterministic_and_respect_existing_names() -> Result<(), String> {
    let shared_env = Yaml::Map(vec![(
        "LONG_VALUE".to_owned(),
        Yaml::str("a long value repeated in many independent job environments"),
    )]);
    let document = Yaml::Map(vec![
        (
            "jobs".to_owned(),
            Yaml::Map(vec![
                (
                    "one".to_owned(),
                    Yaml::Map(vec![("env".to_owned(), shared_env.clone())]),
                ),
                (
                    "two".to_owned(),
                    Yaml::Map(vec![("env".to_owned(), shared_env)]),
                ),
            ]),
        ),
        (
            "existing".to_owned(),
            Yaml::AnchoredMap {
                name: AnchorName::new("m1").ok_or("valid mapping anchor rejected")?,
                entries: vec![("unrelated".to_owned(), Yaml::str("value"))],
            },
        ),
    ]);

    let first = share_repeated_workflow_nodes(document.clone());
    let second = share_repeated_workflow_nodes(document.clone());
    assert_eq!(render_yaml(&first), render_yaml(&second));
    assert!(render_yaml(&first).contains("env: &m2"));
    assert!(render_yaml(&first).contains("env: *m2"));
    assert_eq!(expand_aliases(&document)?, expand_aliases(&first)?);
    Ok(())
}

#[test]
fn small_repeated_maps_and_steps_are_not_anchored_when_they_cost_more() {
    let step = || Yaml::Map(vec![("run".to_owned(), Yaml::str("x"))]);
    let document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                "one".to_owned(),
                Yaml::Map(vec![
                    (
                        "env".to_owned(),
                        Yaml::Map(vec![("A".to_owned(), Yaml::str("1"))]),
                    ),
                    ("steps".to_owned(), Yaml::Seq(vec![step()])),
                ]),
            ),
            (
                "two".to_owned(),
                Yaml::Map(vec![
                    (
                        "env".to_owned(),
                        Yaml::Map(vec![("A".to_owned(), Yaml::str("1"))]),
                    ),
                    ("steps".to_owned(), Yaml::Seq(vec![step()])),
                ]),
            ),
        ]),
    )]);
    let shared = share_repeated_workflow_nodes(document.clone());
    assert_eq!(render_yaml(&shared), render_yaml(&document));
}

fn expand_aliases(node: &Yaml) -> Result<Yaml, String> {
    use std::collections::{BTreeMap, BTreeSet};

    fn expand(
        node: &Yaml,
        anchors: &mut BTreeMap<AnchorName, Yaml>,
        resolving: &mut BTreeSet<AnchorName>,
    ) -> Result<Yaml, String> {
        Ok(match node {
            Yaml::AnchoredScalar { name, value } => {
                let value = Yaml::Str(value.clone());
                anchors.insert(name.clone(), value.clone());
                value
            }
            Yaml::AnchoredMap { name, entries } => {
                let value = expand(&Yaml::Map(entries.clone()), anchors, resolving)?;
                anchors.insert(name.clone(), value.clone());
                value
            }
            Yaml::Alias(name) => {
                if !resolving.insert(name.clone()) {
                    return Err(format!("cyclic YAML alias {name:?}"));
                }
                let target = anchors
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("YAML alias {name:?} has no earlier anchor"))?;
                let value = expand(&target, anchors, resolving)?;
                resolving.remove(name);
                value
            }
            Yaml::Map(entries) => Yaml::Map(
                entries
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), expand(value, anchors, resolving)?)))
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            Yaml::Seq(items) => Yaml::Seq(
                items
                    .iter()
                    .map(|item| expand(item, anchors, resolving))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            other => other.clone(),
        })
    }

    expand(node, &mut BTreeMap::new(), &mut BTreeSet::new())
}
