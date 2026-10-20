use super::*;
use velnor_actions_contract::StepKind;

fn with_sources() -> ConsumerPreviewSpec {
    let mut spec = fixture();
    for root in ["", "nested/"] {
        let step = crate::ambient_shell_step(
            &format!("Prepare {root}Cargo.toml"),
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!("echo prepare-{root}Cargo.toml"),
            ],
            spec.producer_cargo_env.clone(),
        )
        .expect("source step");
        spec.producer_source_prep.push(step);
    }
    spec
}

#[test]
fn preview_prepares_every_workspace_before_dispatch_with_shared_homes() {
    let spec = with_sources();
    let rendered = render_consumer_preview(&spec).expect("preview sources");
    let yaml = rendered.bytes;
    let producer = job(&yaml, "preview-payload-000", Some("preview-payload-001"));
    let ordered = [
        "Create owned Cargo metadata homes",
        "Install pinned Cargo metadata toolchain",
        "prepare-Cargo.toml",
        "prepare-nested/Cargo.toml",
        "execute-preview-producer-v1",
    ];
    let mut previous = 0;
    for needle in ordered {
        let position = producer.find(needle).expect(needle);
        assert!(position > previous, "wrong preparation order for {needle}");
        previous = position;
    }
    for (key, value) in &spec.producer_cargo_env {
        assert!(
            producer.contains(&format!("{key}:")) && producer.contains(value),
            "missing producer env {key}"
        );
    }
    for forbidden in [
        "id-token:",
        "contents: 'write'",
        "environment:",
        "secrets.",
        "actions/cache",
    ] {
        assert!(
            !producer.contains(forbidden),
            "producer must not gain {forbidden}"
        );
    }
    for name in ["preview-signer", "preview-publisher", "preview-recovery"] {
        let isolated = job(&yaml, name, None);
        assert!(!isolated.contains("prepare-Cargo.toml"));
        assert!(!isolated.contains("MISE_CARGO_HOME"));
    }
}

#[test]
fn preview_owned_homes_are_on_every_producer_step_not_job_scope() {
    let spec = with_sources();
    let (_, Yaml::Map(fields)) = production::jobs(&spec).expect("producer").remove(0) else {
        unreachable!()
    };
    let plan = String::from_utf8(spec.plan.canonical_bytes().expect("full plan")).expect("utf8");
    let job_env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .expect("public plan env");
    assert_eq!(
        job_env.1,
        Yaml::Map(vec![("VELNOR_RELEASE_PLAN".to_owned(), Yaml::str(plan))])
    );
    let Some((_, Yaml::Seq(steps))) = fields.iter().find(|(key, _)| key == "steps") else {
        unreachable!()
    };
    for step in steps {
        let Yaml::Map(fields) = step else {
            unreachable!()
        };
        let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") else {
            panic!("missing step env")
        };
        for (key, value) in &spec.producer_cargo_env {
            assert!(env.contains(&(key.clone(), Yaml::str(value))));
        }
    }
}

#[test]
fn preview_rejects_divergent_source_homes_and_credential_environment() {
    let mut spec = with_sources();
    let StepKind::Shell { env, .. } = &mut spec.producer_source_prep[0].kind else {
        unreachable!()
    };
    env.insert("CARGO_HOME".to_owned(), "/other".to_owned());
    assert!(render_consumer_preview(&spec).is_err());
    let mut spec = with_sources();
    spec.producer_cargo_env
        .insert("GH_TOKEN".to_owned(), "forbidden".to_owned());
    assert!(render_consumer_preview(&spec).is_err());
}

#[test]
fn symbolic_project_rust_assembly_resolves_only_after_authenticated_acquisition_and_checkout() {
    let mut spec = with_sources();
    spec.install_rust_argv[3] = "rust@${VELNOR_TOOL_RUST}".to_owned();
    spec.producer_cargo_env.insert(
        "RUSTUP_TOOLCHAIN".to_owned(),
        "${VELNOR_TOOL_RUST}".to_owned(),
    );
    for prep in &mut spec.producer_source_prep {
        let StepKind::Shell { env, .. } = &mut prep.kind else {
            panic!("source shell")
        };
        env.insert(
            "RUSTUP_TOOLCHAIN".to_owned(),
            "${VELNOR_TOOL_RUST}".to_owned(),
        );
    }
    let actual = render_consumer_preview(&spec)
        .expect("full symbolic companion")
        .bytes;
    let mut document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(production::jobs(&spec).expect("composed production")),
    )]);
    crate::project_tool_resolver::bind_workflow(&mut document).expect("symbolic binding");
    let text = crate::yaml::render_yaml(&document);
    let assembly = job(&text, "preview-assemble", None);
    let checkout = assembly.find("actions/checkout@").expect("checkout");
    let resolver = assembly
        .find("Resolve project tool versions")
        .expect("resolver");
    let admission = assembly
        .find("acquire-preview-leaves-v1")
        .expect("authenticated admission");
    let setup = assembly.find("jdx/mise-action@").expect("Mise bootstrap");
    let metadata = assembly
        .find("Install pinned Cargo metadata toolchain")
        .expect("metadata");
    let source = assembly
        .find("prepare-Cargo.toml")
        .expect("source preparation");
    let execute = assembly
        .find("execute-preview-producer-v1")
        .expect("credential-free execution");
    assert!(
        setup < admission
            && admission < checkout
            && checkout < resolver
            && resolver < metadata
            && metadata < source
            && source < execute
    );
    assert!(assembly.contains("ref: ${{ needs.preview-eligibility.outputs.source_sha }}"));
    assert!(assembly.contains("fetch-depth: 0"));
    assert!(assembly.contains("persist-credentials: false"));
    assert!(!assembly[checkout..].contains("GH_TOKEN:"));
    let once = document.clone();
    crate::project_tool_resolver::bind_workflow(&mut document).expect("idempotent binding");
    assert_eq!(document, once);
    for (name, next) in [
        ("preview-signer", Some("preview-publisher")),
        ("preview-publisher", Some("preview-recovery")),
        ("preview-recovery", None),
    ] {
        let isolated = job(&actual, name, next);
        assert!(!isolated.contains("actions/checkout@"));
        assert!(!isolated.contains("Resolve project tool versions"));
        assert!(!isolated.contains("VELNOR_TOOL_RUST"));
    }
}
