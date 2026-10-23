use super::super::{SETUP, bind_workflow, step, workflow};
use crate::yaml::{Yaml, render_yaml};

#[test]
fn verifier_actions_bind_their_project_tools_in_the_job_before_cache_restore() {
    for (uses, variable) in [
        (
            velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
            "VELNOR_TOOL_RUST",
        ),
        (
            velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
            "VELNOR_TOOL_RUST",
        ),
        (
            crate::verifier_composite::VERIFY_JACTIONLINT_USES,
            "VELNOR_TOOL_JACTIONLINT",
        ),
    ] {
        let checkout = step(&[("uses", "actions/checkout@0123")]);
        let setup = step(&[("uses", SETUP)]);
        let restore = step(&[
            ("name", "Restore cached tools"),
            ("uses", "actions/cache@0123"),
        ]);
        let verifier = step(&[("name", "Verify tool"), ("uses", uses)]);
        let prepare = step(&[
            ("name", "Prepare pinned tools"),
            (
                "run",
                match variable {
                    "VELNOR_TOOL_RUST" => "mise install rust@${VELNOR_TOOL_RUST}",
                    _ => "mise install jactionlint@${VELNOR_TOOL_JACTIONLINT}",
                },
            ),
        ]);
        let mut doc = workflow(
            Vec::new(),
            vec![checkout, setup, prepare, restore, verifier],
        );

        bind_workflow(&mut doc).expect("verifier action requires a job-scoped binding");
        let rendered = render_yaml(&doc);
        let resolver = rendered
            .find("name: Resolve project tool versions")
            .expect("resolver step");
        let restore = rendered
            .find("name: Restore cached tools")
            .expect("restore step");
        let verifier = rendered.find("name: Verify tool").expect("verifier step");
        assert!(resolver < restore && restore < verifier, "{rendered}");
        let resolver_environment = rendered[resolver..restore]
            .split_once("\n        env:\n")
            .and_then(|(_, environment)| {
                environment
                    .split_once("\n        shell:")
                    .map(|part| part.0)
            })
            .expect("resolver step environment");
        assert!(
            rendered.contains(&format!("velnor_tool {variable} '")),
            "{rendered}"
        );
        assert!(
            resolver_environment
                .lines()
                .any(|line| line.trim() == format!("PATH: {}", crate::VERIFY_PATH)),
            "trusted PATH is in the resolver step environment:\n{rendered}"
        );
        assert!(
            rendered.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
            "{rendered}"
        );
        assert!(
            rendered.contains(crate::VERIFY_VERIFIER_SHELL),
            "{rendered}"
        );
        assert!(
            rendered.contains("function_env_scan_complete=0"),
            "{rendered}"
        );
        assert!(rendered.contains("velnor_bind_selected_mise"), "{rendered}");
        assert_eq!(rendered.matches("Resolve project tool versions").count(), 1);
    }
}

#[test]
fn a_dynamic_verifier_job_without_setup_mise_fails_but_a_pinned_private_job_needs_no_binding() {
    let checkout = step(&[("uses", "actions/checkout@0123")]);
    let prepare = step(&[
        ("name", "Prepare pinned tools"),
        ("run", "mise install jactionlint@${VELNOR_TOOL_JACTIONLINT}"),
    ]);
    let verifier = step(&[
        ("name", "Verify jactionlint"),
        ("uses", crate::verifier_composite::VERIFY_JACTIONLINT_USES),
    ]);
    let mut dynamic_without_setup = workflow(
        Vec::new(),
        vec![checkout.clone(), prepare, verifier.clone()],
    );
    let error = bind_workflow(&mut dynamic_without_setup)
        .expect_err("a project-tool reference requires Setup Mise");
    assert!(error.to_string().contains("build:no_mise_setup"), "{error}");

    // Private wiring can render a verifier call when the action body is built
    // from literal catalog pins. In that case no project-version resolver is
    // needed, and the renderer must not invent a Mise prerequisite.
    let mut pinned_without_setup = workflow(Vec::new(), vec![checkout, verifier]);
    bind_workflow(&mut pinned_without_setup).expect("literal verifier pins need no resolver");
    let rendered = render_yaml(&pinned_without_setup);
    assert!(
        !rendered.contains("Resolve project tool versions"),
        "{rendered}"
    );
}

#[test]
fn literal_verifier_jobs_preserve_supported_selector_context_without_project_binding() {
    let verifier_actions = [
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
        crate::verifier_composite::VERIFY_JACTIONLINT_USES,
    ];
    let selector_environment = [
        "HOME",
        "MISE_DATA_DIR",
        "XDG_DATA_HOME",
        "PATH",
        "BASH_ENV",
        "ENV",
    ];

    for uses in verifier_actions {
        let mut clean = workflow(
            Vec::new(),
            vec![
                step(&[("uses", "actions/checkout@0123")]),
                step(&[("name", "Verify pinned tool"), ("uses", uses)]),
            ],
        );
        bind_workflow(&mut clean).expect("literal verifier job does not need project resolution");
        let rendered = render_yaml(&clean);
        assert!(
            !rendered.contains("Resolve project tool versions"),
            "{rendered}"
        );
        assert!(!rendered.contains(SETUP), "{rendered}");

        for key in selector_environment {
            let mut selected = workflow(
                vec![(key.to_owned(), Yaml::str("/tmp/selected-context"))],
                vec![
                    step(&[("uses", "actions/checkout@0123")]),
                    step(&[("name", "Verify pinned tool"), ("uses", uses)]),
                ],
            );
            bind_workflow(&mut selected)
                .expect("literal verifier needs no project resolver or selector ban");
            let rendered = render_yaml(&selected);
            assert!(
                !rendered.contains("Resolve project tool versions"),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("{key}: /tmp/selected-context")),
                "selected environment {key} was dropped: {rendered}"
            );
        }
    }
}

fn rust_verifier_workflow() -> Yaml {
    workflow(
        Vec::new(),
        vec![
            step(&[("uses", "actions/checkout@0123")]),
            step(&[("uses", SETUP)]),
            step(&[
                ("name", "Verify Rust channel manifest"),
                ("run", "echo rust@${VELNOR_TOOL_RUST}"),
            ]),
        ],
    )
}

fn rust_verifier_workflow_steps() -> Vec<Yaml> {
    let Yaml::Map(fields) = rust_verifier_workflow() else {
        panic!("workflow map")
    };
    let (_, Yaml::Map(jobs)) = fields
        .into_iter()
        .find(|(key, _)| key == "jobs")
        .expect("jobs")
    else {
        panic!("jobs map")
    };
    let Yaml::Map(job) = jobs.into_iter().next().expect("one job").1 else {
        panic!("job map")
    };
    let (_, Yaml::Seq(steps)) = job
        .into_iter()
        .find(|(key, _)| key == "steps")
        .expect("steps")
    else {
        panic!("steps sequence")
    };
    steps
}

fn resolver_step_fields_mut(document: &mut Yaml) -> &mut Vec<(String, Yaml)> {
    let Yaml::Map(root) = document else {
        panic!("workflow map")
    };
    let (_, jobs) = root
        .iter_mut()
        .find(|(key, _)| key == "jobs")
        .expect("jobs");
    let Yaml::Map(jobs) = jobs else {
        panic!("jobs map")
    };
    let Yaml::Map(job) = &mut jobs[0].1 else {
        panic!("job map")
    };
    let (_, steps) = job
        .iter_mut()
        .find(|(key, _)| key == "steps")
        .expect("steps");
    let Yaml::Seq(steps) = steps else {
        panic!("steps sequence")
    };
    let resolver = steps
        .iter_mut()
        .find(|step| {
            matches!(
                step,
                Yaml::Map(fields)
                    if fields.iter().any(|(key, value)| {
                        key == "name" && matches!(value, Yaml::Str(name) if name == "Resolve project tool versions")
                    })
            )
        })
        .expect("resolver");
    let Yaml::Map(fields) = resolver else {
        panic!("resolver map")
    };
    fields
}

#[test]
fn verifier_resolver_rejects_tampered_shell() {
    let mut document = rust_verifier_workflow();
    bind_workflow(&mut document).expect("canonical resolver");
    let fields = resolver_step_fields_mut(&mut document);
    fields
        .iter_mut()
        .find(|(key, _)| key == "shell")
        .expect("shell")
        .1 = Yaml::str("bash");
    let error = bind_workflow(&mut document).expect_err("tampered resolver must fail closed");
    assert!(
        error.to_string().contains("resolver_step_tampered"),
        "{error}"
    );
}

#[test]
fn project_resolver_keeps_supported_selector_context_behind_its_trusted_envelope() {
    for key in [
        "HOME",
        "MISE_DATA_DIR",
        "XDG_DATA_HOME",
        "PATH",
        "BASH_ENV",
        "ENV",
    ] {
        let mut document = workflow(
            vec![(key.to_owned(), Yaml::str("/tmp/selected-context"))],
            rust_verifier_workflow_steps(),
        );
        bind_workflow(&mut document).expect("the canonical verifier boundary owns these inputs");
        let rendered = render_yaml(&document);
        let resolver = rendered
            .find("Resolve project tool versions")
            .expect("project references require one resolver");
        assert!(
            rendered.contains(&format!("{key}: /tmp/selected-context")),
            "{rendered}"
        );
        assert!(rendered[resolver..].contains(crate::VERIFY_VERIFIER_SHELL));
        assert!(rendered[resolver..].contains("function_env_scan_complete=0"));
        assert!(rendered[resolver..].contains("velnor_bind_selected_mise"));
    }
}
