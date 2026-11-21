// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use std::collections::BTreeSet;

const SETUP: &str = "tailrocks/velnor-new/actions/setup-velnor@";

fn helper_pins(shape: &str) -> Result<(BTreeSet<String>, usize, bool), Box<dyn std::error::Error>> {
    let parsed: serde_json::Value = serde_saphyr::from_str(&crate::archive::plain_invocations(
        &workflow(&crate::base_save::pinned("0.1.110", shape)?)?,
    ))?;
    let mut pins = BTreeSet::new();
    let mut producers = 0;
    let mut checked = false;
    for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
        let steps = job["steps"].as_array().ok_or("steps")?;
        for step in steps {
            if step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with(SETUP))
            {
                pins.insert(
                    step["with"]["version"]
                        .as_str()
                        .ok_or("version")?
                        .to_owned(),
                );
            }
            producers += usize::from(step["id"] == "report_cache_metrics");
            checked |= step["name"] == "Check cache waste";
        }
    }
    Ok((pins, producers, checked))
}

#[test]
fn the_report_producers_and_the_checker_of_a_workflow_install_one_helper_pin() -> TestResult {
    for shape in ["", "shape = \"lint-test\"\n"] {
        let (pins, producers, checked) = helper_pins(shape)?;
        assert!(producers > 0 && checked, "{shape}");
        assert_eq!(pins, BTreeSet::from(["0.1.110".to_owned()]), "{shape}");
    }
    Ok(())
}
