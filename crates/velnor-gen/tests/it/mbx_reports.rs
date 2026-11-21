// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::mbx_default::{NEXTEST, NO_LAYOUT, OPT_OUT, OPTION, WRAPPER, pinned, with_mise};
use crate::pipeline::workflow;
use crate::yaml_tree::document;

const SWITCH: &str = "VELNOR_MBX_REPORTS";

fn builds(job: &serde_json::Value) -> bool {
    job["steps"].as_array().is_some_and(|steps| {
        steps.iter().any(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains(" run -- velnor-ci inputs build -- "))
        })
    })
}

#[test]
fn every_compiling_job_asks_the_new_helper_for_one_mbx_report_per_cargo_session() -> TestResult {
    let text = workflow(&pinned(None, "0.1.111")?)?;
    let yaml = document(&text)?;
    let mut compiling = 0;
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        let switch = job["env"][SWITCH].as_str();
        if builds(job) {
            compiling += 1;
            assert_eq!(switch, Some("true"), "{id}");
        } else {
            assert_eq!(switch, None, "{id}");
        }
    }
    assert_ne!(compiling, 0);
    Ok(())
}

#[test]
fn an_older_helper_or_a_job_without_mbx_gets_no_report_switch() -> TestResult {
    assert!(!workflow(&pinned(None, "0.1.110")?)?.contains(SWITCH));
    let off = with_mise(pinned(Some(OPT_OUT), "0.1.111")?, NO_LAYOUT)?;
    assert!(!workflow(&off)?.contains(SWITCH));
    Ok(())
}

#[test]
fn an_mbx_pin_older_than_its_report_release_keeps_summary_mode() -> TestResult {
    let mise = |version: &str| {
        format!("[tools]\n{OPTION}mr-boxington = \"{version}\"\n{NEXTEST}{WRAPPER}")
    };
    let older = workflow(&with_mise(pinned(None, "0.1.111")?, &mise("v1.29.0"))?)?;
    assert!(!older.contains(SWITCH));
    let current = workflow(&with_mise(pinned(None, "0.1.111")?, &mise("v1.30.0"))?)?;
    assert!(current.contains(SWITCH));
    Ok(())
}
