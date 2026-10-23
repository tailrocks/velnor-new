//! `workflow.dependency_seed_timeout_minutes` defaults, serialization and bounds.
use super::{WorkflowConfig, named};

#[test]
fn seed_timeout_defaults_to_thirty_and_round_trips_overrides() {
    let value = serde_json::json!({
        "name": "CI",
        "policy": "consumer-v1",
        "generator_validation": "bootstrap",
        "max_parallel_jobs": 2,
        "crate_timeout_minutes": 10
    });
    let config: WorkflowConfig = serde_json::from_value(value.clone()).expect("default workflow");
    assert_eq!(config.dependency_seed_timeout_minutes, 30);
    assert_eq!(config.crate_timeout_minutes, 10);
    assert_eq!(config.plan_timeout_minutes, 5);
    let serialized = serde_json::to_value(&config).expect("serialize workflow");
    assert_eq!(serialized.get("dependency_seed_timeout_minutes"), None);
    assert_eq!(
        serialized.get("crate_timeout_minutes"),
        Some(&serde_json::json!(10))
    );

    let mut opted = value;
    opted["dependency_seed_timeout_minutes"] = serde_json::json!(45);
    let config: WorkflowConfig = serde_json::from_value(opted).expect("seed override");
    assert_eq!(config.dependency_seed_timeout_minutes, 45);
    assert_eq!(config.crate_timeout_minutes, 10);
    assert_eq!(
        serde_json::to_value(config)
            .expect("serialize override")
            .get("dependency_seed_timeout_minutes"),
        Some(&serde_json::json!(45))
    );
}

#[test]
fn seed_timeout_accepts_only_github_job_limits() {
    for minutes in [0_u16, 1, 30, 360, 361, u16::MAX] {
        let mut config = named("CI");
        config.dependency_seed_timeout_minutes = minutes;
        let result = config.validate("config.toml");
        assert_eq!(result.is_ok(), (1..=360).contains(&minutes), "{minutes}");
        if let Err(error) = result {
            let text = error.to_string();
            assert!(
                text.contains("workflow.dependency_seed_timeout_minutes"),
                "{text}"
            );
            assert!(
                text.contains(&format!("bad_timeout:{minutes}:expected_1..=360")),
                "{text}"
            );
        }
    }
}
