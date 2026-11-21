// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use std::collections::BTreeSet;

const SETUP: &str = "tailrocks/velnor-new/actions/setup-velnor@";
const REPORT_STEP: &str = "report_cache_metrics";
const CHECK_STEP: &str = "Check cache waste";

#[derive(Default)]
struct JobPins {
    pins: BTreeSet<String>,
    producer: bool,
    checker: bool,
}

fn job_pins(shape: &str) -> Result<Vec<JobPins>, Box<dyn std::error::Error>> {
    let parsed: serde_json::Value = serde_saphyr::from_str(&crate::archive::plain_invocations(
        &workflow(&crate::base_save::pinned("0.1.110", shape)?)?,
    ))?;
    let mut jobs = Vec::new();
    for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
        let mut found = JobPins::default();
        for step in job["steps"].as_array().ok_or("steps")? {
            if step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with(SETUP))
            {
                found.pins.insert(
                    step["with"]["version"]
                        .as_str()
                        .ok_or("version")?
                        .to_owned(),
                );
            }
            found.producer |= step["id"] == REPORT_STEP;
            found.checker |= step["name"] == CHECK_STEP;
        }
        jobs.push(found);
    }
    Ok(jobs)
}

#[test]
fn every_report_job_and_the_checker_install_one_helper_pin() -> TestResult {
    for shape in ["", "shape = \"lint-test\"\n", "shape = \"groups\"\n"] {
        let jobs = job_pins(shape)?;
        assert!(
            jobs.iter().any(|job| job.producer) && jobs.iter().any(|job| job.checker),
            "{shape}"
        );
        for job in &jobs {
            if job.producer || job.checker || !job.pins.is_empty() {
                assert_eq!(job.pins, BTreeSet::from(["0.1.110".to_owned()]), "{shape}");
            }
        }
    }
    Ok(())
}
