use super::tests::{fixture_records, producer_job, setup};
use super::validate_source_producer;
use velnor_actions_contract::{Job, PermissionLevel, SourceBoundOperation, StepKind};

fn rebind_identity(job: &mut Job, identity: &str, restore_keys: &str) {
    let meta = job.source_producer.as_mut().expect("source metadata");
    meta.source_identity = identity.to_owned();
    let save_key = meta.save_key();
    for step in &mut job.steps {
        if let Some(id) = step.id.as_ref().map(|value| value.as_str()) {
            match id {
                "npm-source-restore" => {
                    let StepKind::Action { with, .. } = &mut step.kind else {
                        panic!("source restore action")
                    };
                    with.insert("key".to_owned(), format!("{identity}-lookup"));
                    with.insert("restore-keys".to_owned(), restore_keys.to_owned());
                }
                "npm-source-save" | "npm-source-publication" => {
                    let StepKind::Action { with, .. } = &mut step.kind else {
                        panic!("source transport action")
                    };
                    with.insert("key".to_owned(), save_key.clone());
                }
                _ => {}
            }
        }
        if let StepKind::SourceBoundHelper { env, .. } = &mut step.kind {
            if env.contains_key("VELNOR_SOURCE_IDENTITY") {
                env.insert("VELNOR_SOURCE_IDENTITY".to_owned(), identity.to_owned());
            }
            if step
                .id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-source-report")
            {
                env.insert(
                    "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY".to_owned(),
                    save_key.clone(),
                );
            }
        }
    }
}

#[test]
fn canonical_bootstrap_rejects_injected_controls() {
    for (key, value) in [
        ("NODE_OPTIONS", "--require=/tmp/injected.js"),
        ("BASH_ENV", "/tmp/injected.sh"),
        ("MISE_LOCKFILE", "1"),
        ("UNEXPECTED_SOURCE_ENV", "1"),
    ] {
        let mut job = producer_job();
        let step = job
            .steps
            .iter_mut()
            .find(|step| {
                matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap)
            })
            .expect("canonical bootstrap");
        let StepKind::SourceBoundHelper { env, .. } = &mut step.kind else {
            panic!("canonical bootstrap helper")
        };
        env.insert(key.to_owned(), value.to_owned());
        let error = validate_source_producer(&job, &setup(), &fixture_records())
            .expect_err("injected env rejected");
        assert!(
            error
                .to_string()
                .contains("source_producer_bootstrap_prefix_changed")
        );
    }
}

#[test]
fn tool_installation_rejects_injected_controls() {
    for (key, value) in [
        ("NODE_OPTIONS", "--require=/tmp/injected.js"),
        ("BASH_ENV", "/tmp/injected.sh"),
        ("UNEXPECTED_SOURCE_ENV", "1"),
    ] {
        let mut job = producer_job();
        let step = job
            .steps
            .iter_mut()
            .find(|step| {
                step.id
                    .as_ref()
                    .is_some_and(|id| id.as_str() == "npm-source-prepare")
            })
            .expect("tool installation");
        let StepKind::SourceBoundHelper { env, .. } = &mut step.kind else {
            panic!("tool installation helper")
        };
        env.insert(key.to_owned(), value.to_owned());
        let error = validate_source_producer(&job, &setup(), &fixture_records())
            .expect_err("injected env rejected");
        assert!(
            error
                .to_string()
                .contains("tool_consumer_install_identity_changed")
        );
    }
}

#[test]
fn source_authority_requires_private_permissions_and_no_environment() {
    let mut permissions = producer_job();
    permissions
        .permissions
        .as_mut()
        .expect("source permissions")
        .pages = PermissionLevel::Read;
    let error = validate_source_producer(&permissions, &setup(), &fixture_records())
        .expect_err("pages rejected");
    assert!(
        error
            .to_string()
            .contains("source_producer_private_authority")
    );

    let mut environment = producer_job();
    environment.environment = Some("source".to_owned());
    let error = validate_source_producer(&environment, &setup(), &fixture_records())
        .expect_err("environment rejected");
    assert!(
        error
            .to_string()
            .contains("source_producer_private_authority")
    );
}

#[test]
fn missing_bootstrap_descriptor_fails_closed() {
    let mut job = producer_job();
    job.source_producer.as_mut().expect("metadata").tool_cache = None;
    let error = validate_source_producer(&job, &setup(), &fixture_records())
        .expect_err("descriptor required");
    assert!(error.to_string().contains("bootstrap_descriptor_missing"));
}

#[test]
fn canonical_b3_compatibility_restore_key_is_admitted() {
    let identity = format!("npm-source-test-v1-b3-{}", "c".repeat(64));
    let prefix = &identity[..identity.len() - 67];
    let mut job = producer_job();
    rebind_identity(
        &mut job,
        &identity,
        &format!("{identity}-snapshot-\n{prefix}"),
    );
    assert!(
        validate_source_producer(&job, &setup(), &fixture_records())
            .expect("compatible restore key")
    );
}

#[test]
fn weak_or_malformed_compatibility_restore_keys_are_rejected() {
    for (identity, fallback) in [
        (
            "npm-source-test-v1".to_owned(),
            "npm-source-test-v1".to_owned(),
        ),
        (
            format!("npm-source-test-v1-b3-{}", "c".repeat(63)),
            "npm-source-test-v1-".to_owned(),
        ),
        (
            format!("npm-source-test-v1-b3-{}", "g".repeat(64)),
            "npm-source-test-v1-".to_owned(),
        ),
    ] {
        let mut job = producer_job();
        rebind_identity(
            &mut job,
            &identity,
            &format!("{identity}-snapshot-\n{fallback}"),
        );
        let error = validate_source_producer(&job, &setup(), &fixture_records())
            .expect_err("weak fallback rejected");
        assert!(
            error
                .to_string()
                .contains("source_producer_mixed_computation")
        );
    }
}
