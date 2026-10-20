//! `workflow.crate_timeout_minutes` default, serialization and bounds.
use super::{WorkflowConfig, named};

#[test]
fn crate_timeout_defaults_to_five_and_accepts_github_job_limit() {
    let value = serde_json::json!({
        "name": "CI",
        "policy": "consumer-v1",
        "generator_validation": "bootstrap",
        "max_parallel_jobs": 2
    });
    let config: WorkflowConfig = serde_json::from_value(value.clone()).expect("default workflow");
    assert_eq!(config.crate_timeout_minutes, 5);
    assert_eq!(
        serde_json::to_value(&config)
            .expect("serialize workflow")
            .get("crate_timeout_minutes"),
        None,
        "the default crate timeout stays out of emitted config"
    );
    let mut opted = value;
    opted["crate_timeout_minutes"] = serde_json::json!(45);
    let config: WorkflowConfig = serde_json::from_value(opted).expect("explicit timeout");
    assert_eq!(config.crate_timeout_minutes, 45);
    assert_eq!(
        serde_json::to_value(&config)
            .expect("serialize workflow")
            .get("crate_timeout_minutes"),
        Some(&serde_json::json!(45))
    );
    for minutes in [0_u16, 1, 5, 6, 45, 360, 361, u16::MAX] {
        let mut config = named("CI");
        config.crate_timeout_minutes = minutes;
        let result = config.validate("config.toml");
        assert_eq!(result.is_ok(), (1..=360).contains(&minutes), "{minutes}");
        if let Err(error) = result {
            let text = error.to_string();
            assert!(text.contains("workflow.crate_timeout_minutes"), "{text}");
            assert!(
                text.contains(&format!("bad_timeout:{minutes}:expected_1..=360")),
                "{text}"
            );
        }
    }
}

#[test]
fn plan_timeout_defaults_to_five_and_round_trips_explicit_value() {
    let value = serde_json::json!({
        "name": "CI",
        "policy": "consumer-v1",
        "generator_validation": "bootstrap",
        "max_parallel_jobs": 2
    });
    let config: WorkflowConfig = serde_json::from_value(value.clone()).expect("default workflow");
    assert_eq!(config.plan_timeout_minutes, 5);
    assert_eq!(
        serde_json::to_value(&config)
            .expect("serialize workflow")
            .get("plan_timeout_minutes"),
        None,
        "the default planner timeout stays out of emitted config"
    );

    let mut opted = value;
    opted["plan_timeout_minutes"] = serde_json::json!(15);
    let config: WorkflowConfig = serde_json::from_value(opted).expect("explicit timeout");
    assert_eq!(config.plan_timeout_minutes, 15);
    assert_eq!(
        serde_json::to_value(&config)
            .expect("serialize workflow")
            .get("plan_timeout_minutes"),
        Some(&serde_json::json!(15))
    );
}

#[test]
fn plan_timeout_accepts_only_github_job_limits() {
    for minutes in [0_u16, 1, 5, 15, 360, 361, u16::MAX] {
        let mut config = named("CI");
        config.plan_timeout_minutes = minutes;
        let result = config.validate("config.toml");
        assert_eq!(result.is_ok(), (1..=360).contains(&minutes), "{minutes}");
        if let Err(error) = result {
            let text = error.to_string();
            assert!(text.contains("workflow.plan_timeout_minutes"), "{text}");
            assert!(
                text.contains(&format!("bad_timeout:{minutes}:expected_1..=360")),
                "{text}"
            );
        }
    }
}
