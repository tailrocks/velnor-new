use std::error::Error;

use saphyr::{LoadableYamlNode, Yaml as ParsedYaml};

use super::share_workflow_nodes;
use crate::{
    marker::with_marker,
    yaml::{Yaml, render_yaml},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn repeated_step(name: &str, command: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("run".to_owned(), Yaml::str(command)),
        (
            "env".to_owned(),
            Yaml::Map(vec![("RUST_BACKTRACE".to_owned(), Yaml::str("1"))]),
        ),
    ])
}

fn workflow(steps: Vec<Yaml>) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "first".to_owned(),
            Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(steps))]),
        )]),
    )])
}

fn parsed(bytes: &str) -> Result<ParsedYaml<'_>, Box<dyn Error>> {
    ParsedYaml::load_from_str(bytes)?
        .pop()
        .ok_or_else(|| "missing YAML document".into())
}

#[test]
fn repeated_whole_steps_use_plain_mapping_aliases() {
    let source = workflow(vec![
        repeated_step("Install pinned Rust", "mise install rust@1.99.1"),
        repeated_step("Install pinned Rust", "mise install rust@1.99.1"),
        repeated_step("Install pinned Rust", "mise install rust@1.99.1"),
    ]);
    let source = share_workflow_nodes(source).expect("workflow sharing");
    let rendered = render_yaml(&source);
    assert!(rendered.contains("- &s1\n"));
    assert!(rendered.contains("- *s1\n"));
    assert!(
        rendered
            .find("&s1")
            .zip(rendered.find("*s1"))
            .is_some_and(|(anchor, alias)| anchor < alias)
    );
    assert!(!rendered.contains("<<:"));
}

#[test]
fn aliases_preserve_parsed_workflow_and_keep_distinct_steps() -> Result<(), Box<dyn Error>> {
    let source = workflow(vec![
        repeated_step("Prepare tool", "mise install rust@1.99.1"),
        repeated_step("Prepare tool", "mise install rust@1.99.1"),
        repeated_step("Prepare another tool", "mise install tofu@1.13.1"),
    ]);
    let canonical = with_marker(VERSION, &render_yaml(&source))?;
    let shared = share_workflow_nodes(source)?;
    let transformed = with_marker(VERSION, &render_yaml(&shared))?;
    assert!(transformed.len() < canonical.len());
    assert_eq!(parsed(&canonical)?, parsed(&transformed)?);
    Ok(())
}

#[test]
fn short_duplicate_mappings_are_not_rewritten() {
    let source = workflow(vec![
        Yaml::Map(vec![("name".to_owned(), Yaml::str("a"))]),
        Yaml::Map(vec![("name".to_owned(), Yaml::str("a"))]),
    ]);
    let before = render_yaml(&source);
    let after = render_yaml(&share_workflow_nodes(source).expect("workflow sharing"));
    assert_eq!(before, after);
}

#[test]
fn generated_mapping_anchor_names_skip_existing_anchors() -> Result<(), Box<dyn Error>> {
    let mut source = workflow(vec![
        repeated_step("Prepare tool", "mise install rust@1.99.1"),
        repeated_step("Prepare tool", "mise install rust@1.99.1"),
    ]);
    let Yaml::Map(root) = &mut source else {
        return Err("workflow root is not a map".into());
    };
    let Yaml::Map(jobs) = &mut root[0].1 else {
        return Err("workflow jobs are not a map".into());
    };
    let Yaml::Map(job) = &mut jobs[0].1 else {
        return Err("workflow job is not a map".into());
    };
    let Yaml::Seq(steps) = &mut job[0].1 else {
        return Err("workflow steps are not a sequence".into());
    };
    steps.insert(
        0,
        Yaml::AnchoredMap {
            name: crate::yaml::AnchorName::new("s1").ok_or("anchor name rejected")?,
            entries: match repeated_step("Existing step", "echo existing") {
                Yaml::Map(entries) => entries,
                _ => return Err("existing step is not a map".into()),
            },
        },
    );
    let rendered = render_yaml(&share_workflow_nodes(source).expect("workflow sharing"));
    assert!(rendered.contains("- &s2\n"));
    assert!(rendered.contains("- *s2\n"));
    parsed(&rendered)?;
    Ok(())
}
