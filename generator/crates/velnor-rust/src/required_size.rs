// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, GateNeed, Job, RepoFacts, RootSlug, StepKind, TESTS_BY_PACKAGE_OUTPUT,
    TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT, cache_lane_outputs,
    json_char_bytes, json_string_bytes,
};

const PACKAGE_ENTRY_BYTES: usize = 43;

pub(crate) fn needs_max_bytes<'a>(
    facts: &RepoFacts,
    jobs: impl IntoIterator<Item = &'a Job>,
) -> usize {
    jobs.into_iter().fold(4_usize, |total, job| {
        let outputs = job
            .plan_outputs
            .iter()
            .fold(0_usize, |total, (slug, need)| {
                total.saturating_add(property_bytes(
                    &need.output_name(slug),
                    output_bytes(facts, slug, *need),
                ))
            });
        let tests = job.steps.iter().find_map(|step| match &step.kind {
            StepKind::Cargo(
                CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. },
            ) => Some(step.reads.as_ref().map(|plan| plan.slug.clone())),
            _ => None,
        });
        let counters = match tests {
            Some(slug) => property_bytes(TESTS_RUN_OUTPUT, 20)
                .saturating_add(property_bytes(TESTS_LISTED_OUTPUT, 20))
                .saturating_add(if job.reports_ignored {
                    property_bytes(TESTS_IGNORED_OUTPUT, 20)
                } else {
                    0
                })
                .saturating_add(property_bytes(
                    TESTS_BY_PACKAGE_OUTPUT,
                    package_map_bytes(facts, slug.as_ref()),
                )),
            None => 0,
        };
        total
            .saturating_add(64)
            .saturating_add(json_string_bytes(job.id.as_str()))
            .saturating_add(json_string_bytes("cancelled"))
            .saturating_add(outputs)
            .saturating_add(counters)
            .saturating_add(
                cache_lane_outputs(&job.steps)
                    .iter()
                    .fold(0_usize, |total, output| {
                        total.saturating_add(property_bytes(
                            output.name(),
                            output.max_content_bytes(),
                        ))
                    }),
            )
    })
}

fn package_map_bytes(facts: &RepoFacts, slug: Option<&RootSlug>) -> usize {
    let Some(slug) = slug else {
        return usize::MAX;
    };
    let Some(root) = facts
        .rust_roots
        .iter()
        .find(|root| RootSlug::of(&root.manifest) == *slug)
    else {
        return usize::MAX;
    };
    root.members.iter().fold(0_usize, |total, member| {
        total
            .saturating_add(json_string_bytes(member.name.as_str()))
            .saturating_add(PACKAGE_ENTRY_BYTES)
    })
}

fn property_bytes(key: &str, value_bytes: usize) -> usize {
    24_usize
        .saturating_add(json_string_bytes(key))
        .saturating_add(value_bytes)
        .saturating_add(2)
}

fn output_bytes(facts: &RepoFacts, slug: &RootSlug, need: GateNeed) -> usize {
    match need {
        GateNeed::DoctestsKnown => 5,
        GateNeed::Selected
        | GateNeed::Doctests
        | GateNeed::Docs
        | GateNeed::Deny
        | GateNeed::Machete
        | GateNeed::Policy
        | GateNeed::TestsExpected => 4,
        GateNeed::PackageArgs | GateNeed::TestFilter => {
            let Some(root) = facts
                .rust_roots
                .iter()
                .find(|root| RootSlug::of(&root.manifest) == *slug)
            else {
                return usize::MAX;
            };
            root.members
                .iter()
                .fold(0_usize, |total, member| {
                    let name = member.name.as_str();
                    let bytes = match need {
                        GateNeed::PackageArgs => json_string_bytes(name).saturating_add(4),
                        _ => name.chars().fold(12_usize, |bytes, c| {
                            bytes.saturating_add(filter_char_bytes(c))
                        }),
                    };
                    total.saturating_add(bytes)
                })
                .max(11)
        }
    }
}

fn filter_char_bytes(c: char) -> usize {
    match c {
        '%' | ':' | '#' => 3,
        c if c.is_control() || (c.is_whitespace() && c != ' ') => 8,
        c => json_char_bytes(c),
    }
}
