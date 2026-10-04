//! Native delivery root defaults, boundaries, and hostile input rejection.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{DeliveryConfig, OciReleaseConfig, RegistryAuthentication};

fn oci_config() -> OciReleaseConfig {
    OciReleaseConfig {
        enabled: false,
        registry: "docker.io".to_owned(),
        authentication: RegistryAuthentication::NamedSecrets {
            username_secret: "REGISTRY_USERNAME".to_owned(),
            password_secret: "REGISTRY_TOKEN".to_owned(),
        },
        images: Vec::new(),
    }
}

#[test]
fn delivery_omission_configures_no_families() -> Result<(), Box<dyn std::error::Error>> {
    let delivery: DeliveryConfig = serde_json::from_str("{}")?;
    assert!(!delivery.is_configured());
    assert_eq!(delivery.validate("config.toml"), Ok(()));
    assert_eq!(serde_json::to_string(&delivery)?, "{}");
    let config = super::impl_remed_contract::valid_config();
    let mut document = serde_json::to_value(config)?;
    document
        .as_object_mut()
        .ok_or("config must be an object")?
        .remove("delivery");
    let decoded: velnor_actions_contract::VelnorConfig = serde_json::from_value(document)?;
    assert!(!decoded.delivery.is_configured());
    assert_eq!(decoded.validate("config.toml"), Ok(()));
    Ok(())
}

#[test]
fn delivery_rejects_shell_and_workflow_extensions() {
    for field in [
        "run", "script", "shell", "jobs", "steps", "uses", "workflow",
    ] {
        let source = serde_json::json!({ (field): "arbitrary" });
        assert!(
            serde_json::from_value::<DeliveryConfig>(source.clone()).is_err(),
            "{field}"
        );
        for family in ["apt", "desktop", "oci"] {
            let nested = serde_json::json!({ (family): source.clone() });
            let error = serde_json::from_value::<DeliveryConfig>(nested)
                .expect_err("family rejects arbitrary executable fields");
            assert!(error.to_string().contains("unknown field"), "{error}");
        }
    }
}

#[test]
fn configured_delivery_requires_consumer_policy() {
    let mut config = super::impl_remed_contract::valid_config();
    config.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    config.delivery.oci = Some(oci_config());
    let error = config.validate("config.toml").expect_err("consumer only");
    assert!(
        error
            .to_string()
            .contains("delivery_requires_consumer_policy")
    );
}

#[test]
fn delivery_propagates_family_validation() {
    let mut oci = oci_config();
    oci.registry.clear();
    let delivery = DeliveryConfig {
        oci: Some(oci),
        ..DeliveryConfig::default()
    };
    let error = delivery
        .validate("config.toml")
        .expect_err("invalid registry");
    assert!(error.to_string().contains("delivery.oci"));
    assert!(error.to_string().contains("bad_registry"));
}
