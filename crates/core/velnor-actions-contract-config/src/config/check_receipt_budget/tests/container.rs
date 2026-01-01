use super::super::*;
use super::{check, large_docker_profile, tool};
use crate::config::{CheckExecutor, CheckPlatform};

#[test]
fn large_container_profile_reduces_admitted_probes_before_the_receipt_gate() {
    let profile = large_docker_profile();
    profile
        .validate(
            CheckPlatform::LinuxX64,
            CheckExecutor::EphemeralSelfHosted,
            "config",
            "checks[0].runner.container",
        )
        .expect("large profile is within declared bounds");
    let mut check = check();
    check.runner.label = "native-scale".into();
    check.runner.executor = CheckExecutor::EphemeralSelfHosted;
    check.runner.container = Some(profile.clone());
    let mut maximum_admitted = 0;
    for count in 1..=64 {
        let tool = tool(count);
        let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&tool))
            .expect("bounded container receipt estimate");
        if bound > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
            break;
        }
        maximum_admitted = count;
        validate_check_budget(&check, &[tool], "config", "checks[0]")
            .expect("fitting container closure is admitted");
    }
    assert!(
        maximum_admitted > 0,
        "container checks retain a usable closure"
    );
    let allowed = tool(maximum_admitted);
    assert!(
        check_execution_receipt_upper_bound(&check, std::slice::from_ref(&allowed))
            .expect("max admitted bound")
            <= MAX_CHECK_EXECUTION_RECEIPT_BYTES
    );
    let rejected = tool(maximum_admitted + 1);
    assert!(
        validate_check_budget(&check, &[rejected], "config", "checks[0]")
            .expect_err("the next container closure exceeds the staged proof limit")
            .to_string()
            .contains("check_execution_receipt_budget_exceeded")
    );
}
