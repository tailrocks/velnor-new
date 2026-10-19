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
    assert!(!fields.iter().any(|(key, _)| key == "env"));
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
