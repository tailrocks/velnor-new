// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{rendered, steps};

#[test]
fn the_compile_refuses_cpu_specific_rustflags_of_the_runner_environment() -> TestResult {
    for name in crate::image_workflow::SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in crate::image_workflow::jobs(&workflow)? {
            if !id.ends_with("-build") {
                continue;
            }
            let names: Vec<&str> = steps(job)
                .iter()
                .filter_map(|step| step["name"].as_str())
                .collect();
            let guard = names
                .iter()
                .position(|text| text.starts_with("Refuse CPU-specific rustflags"))
                .ok_or("no cpu guard")?;
            let compile = names
                .iter()
                .position(|text| text.starts_with("Compile"))
                .ok_or("no compile")?;
            assert!(guard < compile, "{name} {id}");
        }
    }
    Ok(())
}
