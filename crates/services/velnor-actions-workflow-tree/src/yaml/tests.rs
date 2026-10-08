use super::{AnchorName, Yaml, render_yaml, share_repeated_run_scalars};

fn step(command: &str, label: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(label)),
        ("run".to_owned(), Yaml::str(command)),
    ])
}

fn map_entries(node: &Yaml) -> &[(String, Yaml)] {
    let Yaml::Map(entries) = node else {
        panic!("expected map");
    };
    entries
}

fn entry_value<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
    let Some((_, value)) = map_entries(node)
        .iter()
        .find(|(candidate, _)| candidate == key)
    else {
        panic!("missing test mapping key: {key}");
    };
    value
}

fn run_value<'a>(document: &'a Yaml, job_id: &str, index: usize) -> &'a Yaml {
    let jobs = entry_value(document, "jobs");
    let selected = entry_value(jobs, job_id);
    let workflow_steps = entry_value(selected, "steps");
    let Yaml::Seq(steps) = workflow_steps else {
        panic!("expected steps sequence");
    };
    let Some(step) = steps.get(index) else {
        panic!("missing test step at index {index}");
    };
    entry_value(step, "run")
}

#[test]
fn repeated_run_scalars_keep_exact_text_and_emit_stable_aliases() {
    let command = format!(
        "printf '%s\\n' '${{{{ runner.os }}}}'\necho {}",
        "x".repeat(160)
    );
    let document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                "first".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![step(&command, "first")]),
                )]),
            ),
            (
                "second".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![step(&command, "second")]),
                )]),
            ),
        ]),
    )]);

    let shared = share_repeated_run_scalars(document);
    let Yaml::AnchoredScalar { name, value } = run_value(&shared, "first", 0) else {
        panic!("first run should own the anchor");
    };
    assert_eq!(AnchorName::new("velnor_run_1").as_ref(), Some(name));
    assert_eq!(value, &command);
    assert_eq!(run_value(&shared, "second", 0), &Yaml::Alias(name.clone()));
    let rendered = render_yaml(&shared);
    assert!(rendered.contains("run: &velnor_run_1 \"printf"));
    assert!(rendered.contains("run: *velnor_run_1"));
}

#[test]
fn unique_and_unprofitable_run_scalars_are_unchanged() {
    let document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                "first".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![step("echo ok", "one")]),
                )]),
            ),
            (
                "second".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![step("echo ok", "two")]),
                )]),
            ),
            (
                "third".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![step(&"z".repeat(160), "unique")]),
                )]),
            ),
        ]),
    )]);
    let shared = share_repeated_run_scalars(document);
    assert_eq!(
        run_value(&shared, "first", 0),
        &Yaml::Str("echo ok".to_owned())
    );
    assert_eq!(
        run_value(&shared, "second", 0),
        &Yaml::Str("echo ok".to_owned())
    );
    assert!(matches!(run_value(&shared, "third", 0), Yaml::Str(_)));
    assert!(!render_yaml(&shared).contains("&velnor_run_"));
}

#[test]
fn non_run_fields_are_not_anchored() {
    let value = "a long value ".repeat(20);
    let shared = share_repeated_run_scalars(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(value.clone())),
        ("other".to_owned(), Yaml::str(value)),
    ]));
    assert_eq!(
        render_yaml(&shared),
        format!(
            "name: {}\nother: {}\n",
            super::quote_scalar("a long value ".repeat(20).as_str()),
            super::quote_scalar("a long value ".repeat(20).as_str())
        )
    );
}
