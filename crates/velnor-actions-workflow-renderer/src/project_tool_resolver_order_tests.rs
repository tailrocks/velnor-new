use super::super::bind_verifier_composite;
use super::{SETUP, bind_workflow, step, workflow};
use crate::yaml::{Yaml, render_yaml};

#[test]
fn resolver_follows_both_checkout_and_mise_and_binding_remains_idempotent() {
    for setup_first in [false, true] {
        let checkout = step(&[("uses", "actions/checkout@0123")]);
        let setup = step(&[("uses", SETUP)]);
        let mut steps = if setup_first {
            vec![setup, checkout]
        } else {
            vec![checkout, setup]
        };
        steps.push(step(&[(
            "run",
            "mise exec rust@${VELNOR_TOOL_RUST} -- cargo metadata",
        )]));
        let mut doc = workflow(Vec::new(), steps);
        bind_workflow(&mut doc).expect("both prerequisites");
        let text = render_yaml(&doc);
        let resolver = text
            .find("Resolve project tool versions")
            .expect("resolver");
        assert!(text.find("actions/checkout@").expect("checkout") < resolver);
        assert!(text.find(SETUP).expect("Mise") < resolver);
        assert!(resolver < text.find("-- cargo metadata").expect("metadata"));
        bind_workflow(&mut doc).expect("repeat");
        assert_eq!(render_yaml(&doc), text);
        assert_eq!(text.matches("Resolve project tool versions").count(), 1);
    }
}

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

#[test]
fn real_reads_before_checkout_fail_but_inert_rustup_is_hoisted() {
    let mut real = workflow(
        Vec::new(),
        vec![
            step(&[("uses", SETUP)]),
            step(&[("name", "early Rust"), ("run", "echo ${VELNOR_TOOL_RUST}")]),
            step(&[("uses", "actions/checkout@0123")]),
        ],
    );
    let error = bind_workflow(&mut real).expect_err("pre-checkout real reference");
    assert!(error.to_string().contains("early Rust"), "{error}");
    let inert = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("source-free acquisition")),
        ("run".to_owned(), Yaml::str("echo acquire")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "RUSTUP_TOOLCHAIN".to_owned(),
                Yaml::str("${VELNOR_TOOL_RUST}"),
            )]),
        ),
    ]);
    let mut doc = workflow(
        Vec::new(),
        vec![
            step(&[("uses", SETUP)]),
            inert,
            step(&[("uses", "actions/checkout@0123")]),
            step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
        ],
    );
    bind_workflow(&mut doc).expect("inert environment hoisted");
    let text = render_yaml(&doc);
    assert!(
        !text[..text.find("actions/checkout@").expect("checkout")].contains("RUSTUP_TOOLCHAIN")
    );
    assert!(text.contains("velnor_export_rustup=1"));
    assert!(
        text.find("actions/checkout@").expect("checkout")
            < text
                .find("Resolve project tool versions")
                .expect("resolver")
    );
}

#[test]
fn source_free_real_reference_and_prebound_resolver_before_checkout_fail_closed() {
    let mut missing = workflow(
        Vec::new(),
        vec![
            step(&[("uses", SETUP)]),
            step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
        ],
    );
    assert!(
        bind_workflow(&mut missing)
            .expect_err("no checkout")
            .to_string()
            .contains("no_checkout")
    );
    let mut misplaced = workflow(
        Vec::new(),
        vec![
            step(&[("uses", SETUP)]),
            step(&[
                ("name", "Resolve project tool versions"),
                ("run", "echo prebound"),
            ]),
            step(&[("uses", "actions/checkout@0123")]),
            step(&[("run", "echo ${VELNOR_TOOL_RUST}")]),
        ],
    );
    assert!(
        bind_workflow(&mut misplaced)
            .expect_err("prebound order")
            .to_string()
            .contains("resolver_prerequisite_order")
    );
}

#[test]
fn cache_identity_must_follow_checkout_and_resolution() {
    let identity = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("V2 identity")),
        (
            "uses".to_owned(),
            Yaml::str("./.github/actions/velnor-tools-prelude-u26"),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                "d".to_owned(),
                Yaml::str(format!(
                    "{}-${{VELNOR_TOOL_RUST}}-${{VELNOR_TOOL_RUST_LOCK}}",
                    "a".repeat(64)
                )),
            )]),
        ),
    ]);
    let mut early = workflow(
        Vec::new(),
        vec![
            step(&[("uses", SETUP)]),
            identity.clone(),
            step(&[("uses", "actions/checkout@0123")]),
        ],
    );
    let error = bind_workflow(&mut early).expect_err("cache identity cannot read before checkout");
    assert!(error.to_string().contains("V2 identity"), "{error}");
    for setup_first in [false, true] {
        let checkout = step(&[("uses", "actions/checkout@0123")]);
        let setup = step(&[("uses", SETUP)]);
        let mut steps = if setup_first {
            vec![setup, checkout]
        } else {
            vec![checkout, setup]
        };
        steps.push(identity.clone());
        let mut doc = workflow(Vec::new(), steps);
        bind_workflow(&mut doc).expect("resolved identity");
        let text = render_yaml(&doc);
        assert!(
            text.find("Resolve project tool versions")
                .expect("resolver")
                < text.find("V2 identity").expect("identity")
        );
        assert!(text.contains("velnor_lock VELNOR_TOOL_RUST_LOCK"));
        bind_workflow(&mut doc).expect("idempotent identity");
        assert_eq!(render_yaml(&doc), text);
    }
}
