use super::super::super::bind_verifier_composite;
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
        assert!(
            rendered.contains(&format!("velnor_tool {variable} '")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("PATH={}", crate::VERIFY_PATH)),
            "{rendered}"
        );
        assert!(
            rendered.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
            "{rendered}"
        );
        assert!(
            rendered.contains("shell: /bin/bash --noprofile --norc -e -o pipefail {0}"),
            "{rendered}"
        );
        assert!(
            rendered.contains("${HOME:?missing HOME}/.local/share/mise/bin/mise"),
            "{rendered}"
        );
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
fn literal_verifier_jobs_reject_all_inherited_trust_overrides_without_needing_setup() {
    let verifier_actions = [
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
        crate::verifier_composite::VERIFY_JACTIONLINT_USES,
    ];
    let forbidden_environment = [
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

        for key in forbidden_environment {
            let mut overridden = workflow(
                vec![(key.to_owned(), Yaml::str("/tmp/attacker-value"))],
                vec![
                    step(&[("uses", "actions/checkout@0123")]),
                    step(&[("name", "Verify pinned tool"), ("uses", uses)]),
                ],
            );
            let error = bind_workflow(&mut overridden)
                .expect_err("literal verifier jobs reject inherited trust overrides");
            assert!(
                error
                    .to_string()
                    .contains("build:verifier_trusted_environment_override"),
                "action {uses}, environment {key}: {error}"
            );
        }
    }
}

#[test]
fn verifier_composite_binding_preserves_static_pins_and_only_rewrites_its_own_ref() {
    let reference = "VELNOR_TOOL_JACTIONLINT";
    let mut pinned = Yaml::Seq(vec![Yaml::str(
        "mise exec jactionlint@2.3.0 -- jactionlint",
    )]);
    bind_verifier_composite(&mut pinned, reference).expect("literal pin needs no binding");
    assert_eq!(
        pinned,
        Yaml::Seq(vec![Yaml::str(
            "mise exec jactionlint@2.3.0 -- jactionlint"
        )])
    );

    let mut dynamic = Yaml::Seq(vec![Yaml::str(
        "mise exec jactionlint@${VELNOR_TOOL_JACTIONLINT} -- jactionlint",
    )]);
    bind_verifier_composite(&mut dynamic, reference).expect("canonical dynamic ref");
    assert_eq!(
        dynamic,
        Yaml::Seq(vec![Yaml::str(
            "mise exec jactionlint@${{ env.VELNOR_TOOL_JACTIONLINT }} -- jactionlint",
        )])
    );

    let mut unrelated = Yaml::Seq(vec![Yaml::str(
        "mise exec jactionlint@${VELNOR_TOOL_RUST} -- jactionlint",
    )]);
    let error = bind_verifier_composite(&mut unrelated, reference)
        .expect_err("a different project binding must not be accepted");
    assert!(
        error
            .to_string()
            .contains("verifier_composite_tool_binding"),
        "{error}"
    );
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
fn verifier_jobs_reject_a_home_override() {
    let mut document = workflow(
        vec![("HOME".to_owned(), Yaml::str("/tmp/attacker-home"))],
        rust_verifier_workflow_steps(),
    );
    let error = bind_workflow(&mut document).expect_err("job HOME must be fixed");
    assert!(
        error
            .to_string()
            .contains("verifier_trusted_environment_override"),
        "{error}"
    );
}

#[test]
fn verifier_jobs_reject_a_path_override() {
    let mut document = workflow(Vec::new(), rust_verifier_workflow_steps());
    let Yaml::Map(fields) = &mut document else {
        panic!("workflow map")
    };
    fields.insert(
        1,
        (
            "env".to_owned(),
            Yaml::Map(vec![("PATH".to_owned(), Yaml::str("/tmp/attacker-bin"))]),
        ),
    );
    let error = bind_workflow(&mut document).expect_err("workflow PATH must be fixed");
    assert!(
        error
            .to_string()
            .contains("verifier_trusted_environment_override"),
        "{error}"
    );
}
