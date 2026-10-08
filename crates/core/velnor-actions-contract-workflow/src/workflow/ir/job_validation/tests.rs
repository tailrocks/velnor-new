use super::*;
use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};

fn ephemeral_job(condition: Option<&str>) -> Job {
    Job {
        outputs: Vec::new(),
        display_name: "Check / native".to_owned(),
        runs_on: "native-scale-set".to_owned(),
        check_runner: Some(CheckRunner {
            label: "native-scale-set".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::EphemeralSelfHosted,
            container: None,
        }),
        timeout_minutes: super::super::JobTimeout::CRATE,
        needs: Vec::new(),
        condition: condition.map(str::to_owned),
        permissions: None,
        environment: None,
        steps: Vec::new(),
    }
}

#[test]
fn external_runner_admission_is_mandatory_even_for_public_typed_ir() {
    for condition in [
        None,
        Some("true"),
        Some("always()"),
        Some("success()"),
        Some("github.event_name == 'pull_request'"),
    ] {
        let error = ephemeral_job(condition)
            .validate_runner("check-native")
            .expect_err("external execution requires exact admission");
        assert!(
            error
                .to_string()
                .contains("ephemeral_check_requires_admission_condition")
        );
    }
    ephemeral_job(Some(EPHEMERAL_CHECK_ADMISSION_CONDITION))
        .validate_runner("check-native")
        .expect("canonical admission accepted");
}
