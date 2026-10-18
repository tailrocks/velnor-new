//! Shared job-budget defaults and suite validation boundaries.
use super::RustSuiteConfig;
use crate::config::validate_rust_suites;

#[test]
fn suite_timeout_default_and_shorter_budgets_share_job_ceiling() {
    assert_eq!(
        RustSuiteConfig::new("bounded", "default").timeout_minutes,
        5
    );
    let json = r#"{"id":"bounded","profile":"default"}"#;
    let suite: RustSuiteConfig = serde_json::from_str(json).expect("default timeout");
    assert_eq!(suite.timeout_minutes, 5);
    for minutes in [0, 1, 2, 3, 4, 5, 6, 30, 360, u16::MAX] {
        let mut suite = RustSuiteConfig::new("bounded", "default");
        suite.timeout_minutes = minutes;
        assert_eq!(
            validate_rust_suites(&[suite], "config.toml").is_ok(),
            (1..=5).contains(&minutes),
            "{minutes}"
        );
    }
}
