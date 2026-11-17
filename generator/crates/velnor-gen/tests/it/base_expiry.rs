// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, pinned, step, steps};
use crate::facts::TestResult;
use crate::pipeline::workflow;

const REF_IS_MAIN: &str =
    "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)";
const CONFIGS: [&str; 4] = [
    "",
    "on_main = \"everything\"\n",
    "on_main = \"affected\"\nfull_run = \"off\"\n",
    "full_run = \"off\"\n",
];

fn lane(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs("0.1.81", config)?
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, block)| block)
        .ok_or_else(|| "no rust job".into())
}

#[test]
fn every_default_branch_run_of_every_configuration_carries_the_age_limit() -> TestResult {
    let expected =
        format!("VELNOR_BASE_MAX_AGE_DAYS: '${{{{ {REF_IS_MAIN} && ''7'' || ''0'' }}}}'");
    for config in CONFIGS {
        let lane = lane(config)?;
        let restored = step(&lane, "velnor-ci restored").ok_or("no restored")?;
        let limit = restored
            .lines()
            .find(|line| line.contains("VELNOR_BASE_MAX_AGE_DAYS"))
            .ok_or("no limit")?;
        assert_eq!(limit.trim(), expected, "{config}");
    }
    Ok(())
}

#[test]
fn a_run_that_found_the_base_expired_plans_everything_and_a_fresh_one_does_not() -> TestResult {
    for config in CONFIGS {
        let lane = lane(config)?;
        let order = names(&lane);
        let restored = order
            .iter()
            .position(|name| name == "velnor-ci restored")
            .ok_or("no restored")?;
        assert_eq!(
            order.get(restored + 1).map(String::as_str),
            Some("velnor-ci plan --full"),
            "{config}: {order:?}"
        );
        let replan = step(&lane, "velnor-ci plan --full").ok_or("no replan")?;
        assert!(replan.contains("id: replan"), "{replan}");
        assert!(
            replan.contains("if: '${{ steps.restored.outputs.expired == ''true'' && "),
            "{replan}"
        );
        assert!(replan.contains("run: velnor-ci plan --full"), "{replan}");
        assert!(
            step(&lane, "velnor-ci restored")
                .ok_or("no restored")?
                .contains("id: restored"),
            "{lane}"
        );
    }
    Ok(())
}

#[test]
fn the_steps_after_the_restore_read_the_full_plan_when_there_is_one_and_the_first_plan_otherwise()
-> TestResult {
    let lane = lane("on_main = \"affected\"\nfull_run = \"off\"\n")?;
    let test = step(&lane, "Test").ok_or("no test")?;
    assert!(
        test.contains(
            "TEST_FILTER: '${{ steps.replan.outputs.rust_root_test_filter || steps.plan.outputs.rust_root_test_filter }}'"
        ),
        "{test}"
    );
    let first_plan = lane
        .split("name: velnor-ci restored")
        .next()
        .ok_or("no head")?;
    assert!(!first_plan.contains("steps.replan"), "{first_plan}");
    Ok(())
}

#[test]
fn a_repository_that_saves_no_base_plans_nothing_again() -> TestResult {
    let text = workflow(&pinned("0.1.79", "")?)?;
    assert!(!text.contains("plan --full"), "{text}");
    assert!(!text.contains("steps.replan"), "{text}");
    Ok(())
}

#[test]
fn a_lane_whose_replan_failed_saves_no_base() -> TestResult {
    for config in CONFIGS {
        let lane = lane(config)?;
        let mut saves = 0;
        for block in steps(&lane) {
            let base_save = block.contains("/save@") && !block.contains("tools cache");
            if block.contains("velnor-ci cache decide") || base_save {
                saves += 1;
                assert!(
                    block.contains("steps.replan.outcome != ''failure''"),
                    "{config}: {block}"
                );
            }
        }
        assert!(saves > 0, "{config}: {lane}");
    }
    Ok(())
}
