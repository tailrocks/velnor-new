// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Job, MERGE_GROUP_SKIP_OUTPUT, MERGE_GROUP_STEP, StepKind, TESTS_BY_PACKAGE_OUTPUT,
    TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT,
};

use crate::cargo::reports_tests;
use crate::tests::Tests;

pub(super) fn test_outputs<'a>(
    job: &'a Job,
    tests: &'a Tests,
) -> impl Iterator<Item = (String, String)> + 'a {
    job.steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::Cargo(command) if reports_tests(command)))
        .then(|| {
            [
                Some(TESTS_RUN_OUTPUT),
                Some(TESTS_LISTED_OUTPUT),
                job.reports_ignored.then_some(TESTS_IGNORED_OUTPUT),
                Some(TESTS_BY_PACKAGE_OUTPUT),
            ]
        })
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|name| tests.outputs(name))
}

pub(super) fn merge_group_output(job: &Job) -> Option<(String, String)> {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::MergeGroupPassed { .. }))
        .then(|| {
            (
                MERGE_GROUP_SKIP_OUTPUT.to_owned(),
                format!("${{{{ steps.{MERGE_GROUP_STEP}.outputs.skip }}}}"),
            )
        })
}
