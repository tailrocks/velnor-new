use super::super::*;
use super::{check, tool};

#[test]
fn admitted_tool_closure_has_a_truthful_worst_case_receipt_budget() {
    let check = check();
    let mut maximum_admitted = 0;
    for count in 1..=64 {
        let tool = tool(count);
        let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&tool))
            .expect("bounded receipt estimate");
        if bound > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
            break;
        }
        maximum_admitted = count;
        validate_check_budget(&check, &[tool], "config", "checks[0]")
            .expect("the computed fitting closure is admitted");
    }
    assert!(maximum_admitted > 1, "small closures remain usable");
    let allowed = tool(maximum_admitted);
    let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&allowed))
        .expect("max admitted estimate");
    assert!(bound <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    let rejected = tool(maximum_admitted + 1);
    let error = validate_check_budget(&check, &[rejected], "config", "checks[0]")
        .expect_err("the next closure cannot fit the report transport");
    assert!(
        error
            .to_string()
            .contains("check_execution_receipt_budget_exceeded")
    );
}
