// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::JobId;
use velnor_rust::Need;

#[derive(Debug)]
pub struct Comparison {
    pub passed: bool,
    pub lanes: Vec<(JobId, String)>,
    pub problems: Vec<String>,
}

#[must_use]
pub fn compare(needs: &BTreeMap<JobId, Need>, lanes: &[JobId]) -> Comparison {
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for lane in lanes {
        let result = match needs.get(lane) {
            None => {
                problems.push(format!("{lane} is missing from needs"));
                "missing".to_owned()
            }
            Some(Need { result: None, .. }) => {
                problems.push(format!("{lane} result is missing"));
                "missing".to_owned()
            }
            Some(Need {
                result: Some(result),
                ..
            }) => {
                if result != "success" {
                    problems.push(format!("{lane} result is {result}"));
                }
                result.clone()
            }
        };
        seen.push((lane.clone(), result));
    }
    Comparison {
        passed: problems.is_empty(),
        lanes: seen,
        problems,
    }
}

impl Comparison {
    #[must_use]
    pub fn table(&self) -> String {
        let verdict = if self.passed { "pass" } else { "fail" };
        let mut lines = vec![
            format!("**Qualification compare: {verdict}**"),
            String::new(),
            "| Lane | Result |".to_owned(),
            "| --- | --- |".to_owned(),
        ];
        lines.extend(
            self.lanes
                .iter()
                .map(|(lane, result)| format!("| {lane} | {result} |")),
        );
        for problem in &self.problems {
            lines.push(String::new());
            lines.push(problem.clone());
        }
        lines.join("\n") + "\n"
    }
}
