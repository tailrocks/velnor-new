// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use std::collections::BTreeSet;

const SETUP: &str = "tailrocks/velnor-new/actions/setup-velnor@";
const PIN: &str = "0.1.110";

#[test]
fn every_job_that_reports_or_checks_the_cache_installs_the_one_helper_pin() -> TestResult {
    for shape in ["", "shape = \"lint-test\"\n", "shape = \"groups\"\n"] {
        let parsed: serde_json::Value = serde_saphyr::from_str(
            &crate::archive::plain_invocations(&workflow(&crate::base_save::pinned(PIN, shape)?)?),
        )?;
        let mut producers = 0;
        let mut checkers = 0;
        for (id, job) in parsed["jobs"].as_object().ok_or("jobs")? {
            let steps = job["steps"].as_array().ok_or("steps")?;
            let reports = steps
                .iter()
                .any(|step| step["id"] == "report_cache_metrics");
            let checks = steps.iter().any(|step| step["name"] == "Check cache waste");
            if !reports && !checks {
                continue;
            }
            producers += usize::from(reports);
            checkers += usize::from(checks);
            let pins = steps
                .iter()
                .filter(|step| {
                    step["uses"]
                        .as_str()
                        .is_some_and(|uses| uses.starts_with(SETUP))
                })
                .map(|step| {
                    step["with"]["version"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("version")
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            assert_eq!(pins, BTreeSet::from([PIN.to_owned()]), "{id} {shape}");
        }
        assert!(producers > 0 && checkers > 0, "{shape}");
    }
    Ok(())
}
