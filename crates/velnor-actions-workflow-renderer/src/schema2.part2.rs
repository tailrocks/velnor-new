fn mode_trigger() -> Yaml {
    Yaml::Map(vec![(
        "workflow_dispatch".to_owned(),
        Yaml::Map(vec![(
            "inputs".to_owned(),
            Yaml::Map(vec![
                (
                    "mode".to_owned(),
                    Yaml::Map(vec![
                        ("type".to_owned(), Yaml::str("string")),
                        ("required".to_owned(), Yaml::Bool(false)),
                        ("default".to_owned(), Yaml::str("both")),
                    ]),
                ),
                (
                    "probe_id".to_owned(),
                    Yaml::Map(vec![
                        ("type".to_owned(), Yaml::str("string")),
                        ("required".to_owned(), Yaml::Bool(false)),
                        ("default".to_owned(), Yaml::str("")),
                    ]),
                ),
            ]),
        )]),
    )])
}

fn empty_dispatch() -> Yaml {
    Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))])
}

fn job(
    id: &str,
    name: &str,
    runner: &RunnerSpec,
    timeout: i64,
    needs: Vec<String>,
    step_name: &str,
    run: &str,
) -> (String, Yaml) {
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runner.runs_on.clone()),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
    ];
    runner.push_default_shell(&mut fields, false);
    if !needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(needs.into_iter().map(Yaml::str).collect()),
        ));
    }
    fields.push((
        "steps".to_owned(),
        Yaml::Seq(vec![Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(step_name)),
            ("run".to_owned(), Yaml::str(run)),
        ])]),
    ));
    (id.to_owned(), Yaml::Map(fields))
}
