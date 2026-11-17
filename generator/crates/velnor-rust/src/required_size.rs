// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, GateNeed, Job, RepoFacts, RootSlug, StepKind, TESTS_LISTED_OUTPUT,
    TESTS_RUN_OUTPUT,
};

pub(crate) fn needs_max_bytes(facts: &RepoFacts, jobs: &[Job]) -> usize {
    jobs.iter().fold(4_usize, |total, job| {
        let outputs = job
            .plan_outputs
            .iter()
            .fold(0_usize, |total, (slug, need)| {
                total.saturating_add(property_bytes(
                    &need.output_name(slug),
                    output_bytes(facts, slug, *need),
                ))
            });
        let tests = job.steps.iter().any(|step| {
            matches!(
                step.kind,
                StepKind::Cargo(
                    CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. }
                )
            )
        });
        let counters = if tests {
            property_bytes(TESTS_RUN_OUTPUT, 20)
                .saturating_add(property_bytes(TESTS_LISTED_OUTPUT, 20))
        } else {
            0
        };
        total
            .saturating_add(64)
            .saturating_add(string_bytes(job.id.as_str()))
            .saturating_add(string_bytes("cancelled"))
            .saturating_add(outputs)
            .saturating_add(counters)
    })
}

fn property_bytes(key: &str, value_bytes: usize) -> usize {
    24_usize
        .saturating_add(string_bytes(key))
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
                        GateNeed::PackageArgs => string_bytes(name).saturating_add(4),
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

fn string_bytes(text: &str) -> usize {
    text.chars()
        .fold(2_usize, |bytes, c| bytes.saturating_add(json_char_bytes(c)))
}

fn filter_char_bytes(c: char) -> usize {
    match c {
        '%' | ':' | '#' => 3,
        c if c.is_control() || (c.is_whitespace() && c != ' ') => 8,
        c => json_char_bytes(c),
    }
}

fn json_char_bytes(c: char) -> usize {
    match c {
        '"' | '\\' => 2,
        c if c.is_control() => 6,
        c if c.is_ascii() => 1,
        c => 6 * c.len_utf16(),
    }
}
