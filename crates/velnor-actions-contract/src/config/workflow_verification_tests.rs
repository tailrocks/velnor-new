//! Contract tests for optional main-workflow verification settings.

use super::VerificationConfig;

#[test]
fn alert_defaults_to_disabled() {
    let config = VerificationConfig::default();
    assert!(!config.alert);
    let decoded: VerificationConfig = serde_json::from_str("{}").expect("serde defaults");
    assert!(!decoded.alert);
}

#[test]
fn alert_requires_schedule_or_dispatch() {
    let error = VerificationConfig {
        schedule: None,
        workflow_dispatch: false,
        alert: true,
    }
    .validate("config.toml")
    .expect_err("alert without an event must fail");
    assert_eq!(
        error.to_string(),
        "config.toml: workflow.verification.alert: requires_schedule_or_dispatch"
    );
}

#[test]
fn alert_accepts_each_native_trigger() {
    for config in [
        VerificationConfig {
            schedule: Some("17 3 * * *".to_owned()),
            workflow_dispatch: false,
            alert: true,
        },
        VerificationConfig {
            schedule: None,
            workflow_dispatch: true,
            alert: true,
        },
    ] {
        config
            .validate("config.toml")
            .expect("alert trigger is valid");
    }
}

#[test]
fn schedule_accepts_numeric_posix_cron() {
    for schedule in ["17 3 * * *", "0 0 * * *", "1,2 0-23/2 1-31 1-12 0-6"] {
        VerificationConfig {
            schedule: Some(schedule.to_owned()),
            workflow_dispatch: false,
            alert: false,
        }
        .validate("config.toml")
        .expect("numeric POSIX cron is valid");
    }
}

#[test]
fn schedule_rejects_bad_ranges_and_steps() {
    for schedule in [
        "0 24 * * *",
        "*/0 * * * *",
        "0 0 31-1 * *",
        "0 0 * * 7",
        "0 0 * JAN *",
        "0  0 * * *",
    ] {
        assert!(
            VerificationConfig {
                schedule: Some(schedule.to_owned()),
                workflow_dispatch: false,
                alert: false,
            }
            .validate("config.toml")
            .is_err(),
            "{schedule}"
        );
    }
}
