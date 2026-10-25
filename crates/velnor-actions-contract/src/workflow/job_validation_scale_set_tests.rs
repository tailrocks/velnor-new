use super::*;
use crate::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION, RunsOn,
};

const SELECTOR: &str = "scale-set:velnor+ubuntu-26.04-scale-set";

fn scale_set_check_runner(
    executor: CheckExecutor,
    platform: CheckPlatform,
    condition: Option<&str>,
) -> Job {
    let label = match (executor, platform) {
        (CheckExecutor::Hosted, CheckPlatform::LinuxX64) => "ubuntu-26.04",
        (CheckExecutor::Hosted, CheckPlatform::MacosArm64) => "macos-15",
        (CheckExecutor::Hosted, CheckPlatform::MacosX64) => "macos-15-intel",
        (CheckExecutor::EphemeralSelfHosted, _) => "native-runner",
    };
    Job {
        services: Vec::new(),
        display_name: "Check / scale-set".to_owned(),
        runs_on: SELECTOR.to_owned(),
        check_runner: Some(CheckRunner {
            label: label.to_owned(),
            platform,
            executor,
            container: None,
        }),
        timeout_minutes: super::super::JobTimeout::CRATE,
        needs: Vec::new(),
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: Vec::new(),
    }
}

#[test]
fn linux_scale_set_admits_hosted_and_ephemeral_check_runners() {
    assert!(matches!(
        RunsOn::parse(SELECTOR).expect("typed scale-set selector"),
        RunsOn::ScaleSet(_)
    ));

    for executor in [CheckExecutor::Hosted, CheckExecutor::EphemeralSelfHosted] {
        scale_set_check_runner(
            executor,
            CheckPlatform::LinuxX64,
            Some(EPHEMERAL_CHECK_ADMISSION_CONDITION),
        )
        .validate_runner("check-scale-set")
        .expect("Linux scale-set runner with canonical admission");
    }
}

#[test]
fn hosted_linux_scale_set_accepts_only_the_exact_preserved_qualification_condition() {
    let condition = format!(
        "({}) && ({})",
        "always() && needs.plan.result == 'success'", EPHEMERAL_CHECK_ADMISSION_CONDITION
    );
    scale_set_check_runner(
        CheckExecutor::Hosted,
        CheckPlatform::LinuxX64,
        Some(&condition),
    )
    .validate_runner("check-scale-set")
    .expect("the known qualification condition and admission remain conjunctive");

    let invalid_conditions = [
        format!("(true) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})"),
        format!(
            "(always() && needs.plan.result == 'success') || ({EPHEMERAL_CHECK_ADMISSION_CONDITION})"
        ),
        format!(
            "({}) && (true)",
            "always() && needs.plan.result == 'success'"
        ),
    ];
    for invalid in invalid_conditions {
        let error = scale_set_check_runner(
            CheckExecutor::Hosted,
            CheckPlatform::LinuxX64,
            Some(&invalid),
        )
        .validate_runner("check-scale-set")
        .expect_err("only the exact canonical condition is supported");
        assert!(
            error
                .to_string()
                .contains("check_runner_scale_set_mismatch"),
            "{error}"
        );
    }
}

#[test]
fn scale_set_requires_exact_admission_for_each_executor() {
    for executor in [CheckExecutor::Hosted, CheckExecutor::EphemeralSelfHosted] {
        for condition in [
            None,
            Some("true"),
            Some("always()"),
            Some("success()"),
            Some("github.event_name == 'pull_request'"),
        ] {
            let error = scale_set_check_runner(executor, CheckPlatform::LinuxX64, condition)
                .validate_runner("check-scale-set")
                .expect_err("scale-set execution requires the exact admission condition");
            let expected = match executor {
                CheckExecutor::Hosted => "check_runner_scale_set_mismatch",
                CheckExecutor::EphemeralSelfHosted => {
                    "ephemeral_check_requires_admission_condition"
                }
            };
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}

#[test]
fn scale_set_rejects_both_macos_platforms_for_each_executor() {
    for executor in [CheckExecutor::Hosted, CheckExecutor::EphemeralSelfHosted] {
        for platform in [CheckPlatform::MacosArm64, CheckPlatform::MacosX64] {
            let error = scale_set_check_runner(
                executor,
                platform,
                Some(EPHEMERAL_CHECK_ADMISSION_CONDITION),
            )
            .validate_runner("check-scale-set")
            .expect_err("scale-set check runners are Linux x64 only");
            assert!(
                error
                    .to_string()
                    .contains("check_runner_scale_set_mismatch"),
                "{error}"
            );
        }
    }
}
