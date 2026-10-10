use std::collections::BTreeMap;

use super::share_repeated_workflow_nodes;
use crate::yaml::{AnchorName, Yaml, render_yaml};

#[path = "yaml_share_mapping_tests.rs"]
mod mapping;

fn repeated_runs(extra: Vec<(String, Yaml)>) -> Yaml {
    let command = "printf \"this repeated command is long enough to save bytes\"";
    let step = vec![
        ("name".to_owned(), Yaml::str("first")),
        ("run".to_owned(), Yaml::str(command)),
        (
            "env".to_owned(),
            Yaml::Map(vec![("run".to_owned(), Yaml::str(command))]),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![("run".to_owned(), Yaml::str(command))]),
        ),
    ];
    let steps = Yaml::Seq(vec![
        Yaml::Map(step.clone()),
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("second")),
            ("run".to_owned(), Yaml::str(command)),
        ]),
    ]);
    Yaml::Map(
        vec![
            (
                "defaults".to_owned(),
                Yaml::Map(vec![("run".to_owned(), Yaml::str(command))]),
            ),
            (
                "jobs".to_owned(),
                Yaml::Map(vec![(
                    "job".to_owned(),
                    Yaml::Map(vec![("steps".to_owned(), steps)]),
                )]),
            ),
        ]
        .into_iter()
        .chain(extra)
        .collect(),
    )
}

#[test]
fn sharing_is_confined_to_step_runs_and_supported_input_maps() -> Result<(), String> {
    let node = repeated_runs(Vec::new());
    let shared = share_repeated_workflow_nodes(node.clone());
    let first = render_yaml(&shared);
    let second = render_yaml(&share_repeated_workflow_nodes(node.clone()));
    assert_eq!(first, second);
    assert!(first.contains("run: &r1"));
    assert!(first.contains("run: *r1"));
    assert!(first.contains("defaults:\n  run: \"printf"));
    assert!(first.contains("env: &m1\n          run: \"printf"));
    assert!(first.contains("with: *m1"));
    assert_eq!(
        mapping::expand_aliases(&node)?,
        mapping::expand_aliases(&shared)?
    );
    Ok(())
}

#[test]
fn generated_anchor_names_skip_existing_names() -> Result<(), String> {
    let name = AnchorName::new("r1").ok_or("valid anchor name rejected")?;
    let existing = (
        "metadata".to_owned(),
        Yaml::AnchoredScalar {
            name,
            value: "unrelated".to_owned(),
        },
    );
    let rendered = render_yaml(&share_repeated_workflow_nodes(repeated_runs(vec![
        existing,
    ])));
    assert!(rendered.contains("run: &r2"));
    assert!(rendered.contains("run: *r2"));
    Ok(())
}

#[test]
fn anchor_names_reject_yaml_syntax_and_non_ascii_characters() {
    for invalid in ["", "bad.name", "name with spaces", "é", "&nested"] {
        assert!(AnchorName::new(invalid).is_none(), "accepted {invalid:?}");
    }
    assert!(AnchorName::new("r-10_A").is_some());
}

#[test]
fn short_repeats_are_not_rewritten_when_anchors_cost_more() {
    let node = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "j".to_owned(),
            Yaml::Map(vec![(
                "steps".to_owned(),
                Yaml::Seq(vec![
                    Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("one")),
                        ("run".to_owned(), Yaml::str("x")),
                    ]),
                    Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("two")),
                        ("run".to_owned(), Yaml::str("x")),
                    ]),
                ]),
            )]),
        )]),
    )]);
    assert_eq!(
        render_yaml(&share_repeated_workflow_nodes(node.clone())),
        render_yaml(&node)
    );
}

#[test]
fn parent_step_aliases_do_not_leave_unused_nested_anchors() -> Result<(), String> {
    let command = "printf '%s' 'a repeated command long enough to share safely'";
    let repeated_env = Yaml::Map(vec![(
        "TOKEN".to_owned(),
        Yaml::str("a repeated environment value"),
    )]);
    let repeated_with = Yaml::Map(vec![(
        "INPUT".to_owned(),
        Yaml::str("a repeated action input value"),
    )]);
    let step = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("same step")),
        ("run".to_owned(), Yaml::str(command)),
        ("env".to_owned(), repeated_env),
        ("with".to_owned(), repeated_with),
    ]);
    let source = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "job".to_owned(),
            Yaml::Map(vec![(
                "steps".to_owned(),
                Yaml::Seq(vec![step.clone(), step]),
            )]),
        )]),
    )]);

    let shared = share_repeated_workflow_nodes(source.clone());
    let rendered = render_yaml(&shared);
    assert!(rendered.contains("- &s1"));
    assert!(rendered.contains("*s1"));
    assert!(
        !rendered.contains("&m"),
        "nested mapping anchor is redundant"
    );
    assert!(!rendered.contains("&r"), "nested run anchor is redundant");
    assert_eq!(
        mapping::expand_aliases(&source)?,
        mapping::expand_aliases(&shared)?,
        "pruning must preserve the expanded workflow"
    );

    let mut anchors = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    collect_anchor_uses(&shared, &mut anchors, &mut aliases);
    assert!(!anchors.is_empty());
    assert!(anchors.iter().all(|(name, count)| {
        *count == 1 && aliases.get(name).copied().unwrap_or_default() > 0
    }));
    Ok(())
}

fn collect_anchor_uses(
    node: &Yaml,
    anchors: &mut BTreeMap<AnchorName, usize>,
    aliases: &mut BTreeMap<AnchorName, usize>,
) {
    match node {
        Yaml::AnchoredScalar { name, .. } | Yaml::AnchoredMap { name, .. } => {
            *anchors.entry(name.clone()).or_default() += 1;
            if let Yaml::AnchoredMap { entries, .. } = node {
                for (_, value) in entries {
                    collect_anchor_uses(value, anchors, aliases);
                }
            }
        }
        Yaml::Alias(name) => *aliases.entry(name.clone()).or_default() += 1,
        Yaml::Map(entries) => {
            for (_, value) in entries {
                collect_anchor_uses(value, anchors, aliases);
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                collect_anchor_uses(item, anchors, aliases);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. } => {}
    }
}

#[test]
fn repeated_run_scalars_preserve_yaml_escaping_and_expression_text() {
    let commands = [
        "printf 'first line'\nprintf 'second line'",
        "printf \"quoted\"",
        "printf '%s' 'C:\\\\work\\\\path'",
        "printf '%s' 'héllo 世界'",
        "case: a long value that YAML could otherwise parse as a mapping",
        "printf '%s' \"${{ github.ref }}\"",
    ];
    let steps = commands
        .iter()
        .enumerate()
        .flat_map(|(index, command)| {
            (0..2).map(move |repeat| {
                Yaml::Map(vec![
                    (
                        "name".to_owned(),
                        Yaml::str(format!("step-{index}-{repeat}")),
                    ),
                    ("run".to_owned(), Yaml::str(*command)),
                ])
            })
        })
        .collect();
    let source = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "job".to_owned(),
            Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(steps))]),
        )]),
    )]);
    let shared = share_repeated_workflow_nodes(source);
    let Yaml::Map(root) = &shared else {
        panic!("root map")
    };
    let Yaml::Map(jobs) = &root[0].1 else {
        panic!("jobs map")
    };
    let Yaml::Map(job) = &jobs[0].1 else {
        panic!("job map")
    };
    let Yaml::Seq(steps) = &job[0].1 else {
        panic!("steps")
    };
    for (index, command) in commands.iter().enumerate() {
        let Yaml::Map(first) = &steps[index * 2] else {
            panic!("step map")
        };
        let Yaml::Map(second) = &steps[index * 2 + 1] else {
            panic!("step map")
        };
        assert_eq!(first[1].0, "run");
        assert_eq!(second[1].0, "run");
        assert!(matches!(&first[1].1, Yaml::AnchoredScalar { value, .. } if value == command));
        assert!(matches!(&second[1].1, Yaml::Alias(_)));
    }
    let rendered = render_yaml(&shared);
    assert!(rendered.contains("\\n"), "multiline run stays quoted");
    assert!(rendered.contains("\\\\"), "backslashes stay escaped");
    assert!(rendered.contains("héllo 世界"), "Unicode stays intact");
    assert!(
        rendered.contains("${{ github.ref }}"),
        "expressions stay intact"
    );
}

#[test]
#[ignore = "external parser round-trip harness; set VELNOR_EDGE_CAPTURE_PATH"]
fn write_edge_corpus_for_external_psych_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, fs, path::PathBuf};

    let capture = PathBuf::from(
        env::var_os("VELNOR_EDGE_CAPTURE_PATH")
            .ok_or("set VELNOR_EDGE_CAPTURE_PATH to an external diagnostic file")?,
    );
    if !capture.is_absolute() || capture.starts_with(env::current_dir()?) {
        return Err("capture must be an absolute path outside the source checkout".into());
    }
    let commands = [
        "printf 'first line'\nprintf 'second line'",
        "printf \"quoted\"",
        "printf '%s' 'C:\\\\work\\\\path'",
        "printf '%s' 'héllo 世界'",
        "case: a long value that YAML could otherwise parse as a mapping",
        "printf '%s' \"${{ github.ref }}\"",
    ];
    let steps = commands
        .iter()
        .enumerate()
        .flat_map(|(index, command)| {
            (0..2).map(move |repeat| {
                Yaml::Map(vec![
                    (
                        "name".to_owned(),
                        Yaml::str(format!("edge-{index}-{repeat}")),
                    ),
                    ("run".to_owned(), Yaml::str(*command)),
                ])
            })
        })
        .collect();
    let document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "edge".to_owned(),
            Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(steps))]),
        )]),
    )]);
    let marked = crate::marker::with_marker(
        "0.1.4",
        &render_yaml(&share_repeated_workflow_nodes(document)),
    )?;
    fs::write(capture, marked)?;
    Ok(())
}

#[test]
fn profitability_edges_account_for_anchor_name_length() -> Result<(), String> {
    let command = |value: &str| {
        Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "job".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(
                        (0..2)
                            .map(|index| {
                                Yaml::Map(vec![
                                    ("name".to_owned(), Yaml::str(format!("step-{index}"))),
                                    ("run".to_owned(), Yaml::str(value)),
                                ])
                            })
                            .collect(),
                    ),
                )]),
            )]),
        )])
    };
    let no_anchor = share_repeated_workflow_nodes(command("abcdefg"));
    assert_eq!(render_yaml(&no_anchor), render_yaml(&command("abcdefg")));
    let anchor = share_repeated_workflow_nodes(command("abcdefgh"));
    assert!(render_yaml(&anchor).contains("run: &r1"));

    let mut with_collisions = match command("abcdefghij") {
        Yaml::Map(mut root) => {
            let names = (1..=9)
                .map(|index| {
                    Ok((
                        format!("existing-{index}"),
                        Yaml::AnchoredScalar {
                            name: AnchorName::new(format!("r{index}"))
                                .ok_or("test anchor rejected")?,
                            value: "unused".to_owned(),
                        },
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            root.extend(names);
            Yaml::Map(root)
        }
        _ => return Err("test root must be map".to_owned()),
    };
    with_collisions = share_repeated_workflow_nodes(with_collisions);
    let rendered = render_yaml(&with_collisions);
    assert!(rendered.contains("run: &r10"));
    assert!(rendered.contains("run: *r10"));
    Ok(())
}
