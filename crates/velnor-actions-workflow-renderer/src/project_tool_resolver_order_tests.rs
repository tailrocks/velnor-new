//! Resolver placement: after both Mise setup and source checkout.

use super::bind_workflow;
use super::tests::{SETUP, step, workflow};
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
