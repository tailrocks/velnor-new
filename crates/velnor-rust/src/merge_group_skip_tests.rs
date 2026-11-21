// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    BuildRecord, CargoCommand, FilterRef, Job, JobId, PackageName, Permissions, PrRun,
    ReleaseAssetPart, RunnerLabel, StepKind, TargetName, When,
};

use super::*;

fn job(id: JobId, kinds: Vec<StepKind>) -> Job {
    Job {
        id,
        name: None,
        comment: None,
        needs: Vec::new(),
        when: When::Success,
        pr: PrRun::Always,
        runs_on: RunnerLabel::from_static("ubuntu-latest"),
        permissions: Permissions(BTreeMap::new()),
        timeout_minutes: 30,
        gate: None,
        skip_after_merge_group: None,
        plan_outputs: Vec::new(),
        group_outputs: Vec::new(),
        reports_ignored: false,
        build_record: BuildRecord::Unrecorded,
        chained_saves: false,
        mbx_stats: velnor_model::MbxStats::Summary,
        selection_name: None,
        steps: kinds.into_iter().map(step).collect(),
    }
}

#[test]
fn the_checks_and_only_the_checks_are_classified_as_checks() {
    let checks = [
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
        CargoCommand::Doc,
        CargoCommand::Doctest,
        CargoCommand::Machete,
        CargoCommand::NextestRun {
            filter: FilterRef::All,
            ci_profile: true,
        },
    ];
    let builds = [
        CargoCommand::BuildAll,
        CargoCommand::NextestBuild,
        CargoCommand::BuildBinary {
            package: PackageName::from_static("app"),
            binary: TargetName::from_static("app"),
        },
    ];
    for command in checks {
        assert!(!command.is_target_build(), "{command:?}");
    }
    for command in builds {
        assert!(command.is_target_build(), "{command:?}");
    }
}

#[test]
#[should_panic(expected = "reads no merge-group-passed step")]
fn a_skip_naming_a_missing_merge_group_step_stops_generation() {
    let mut marked = step(StepKind::Cargo(CargoCommand::FmtCheck));
    marked.skip_after_merge_group = Some(PlanFrom::ThisJob);
    let mut jobs = [job(RUST_JOB, vec![StepKind::Cargo(CargoCommand::Doc)])];
    jobs[0].steps.push(marked);
    assert_skips_resolve(&jobs);
}

fn build_inputs(build: bool) -> Job {
    let mut kinds = vec![
        StepKind::ReleaseAssets {
            assets: Vec::new(),
            part: ReleaseAssetPart::Check,
        },
        StepKind::TestSetup {
            name: String::from("setup"),
            run: String::from("true"),
            before_build: true,
        },
    ];
    if build {
        kinds.push(StepKind::Cargo(CargoCommand::NextestBuild));
    }
    kinds.push(StepKind::Cargo(CargoCommand::Clippy { extra: Vec::new() }));
    job(RUST_JOB, kinds)
}

#[test]
fn the_build_inputs_are_skipped_only_when_no_target_build_of_the_job_survives() {
    for (build, skipped) in [(true, false), (false, true)] {
        let mut jobs = [build_inputs(build)];
        mark(&mut jobs, true, false, None, &|_| false);
        let marks: Vec<bool> = jobs[0].steps[..2]
            .iter()
            .map(|step| step.skip_after_merge_group.is_some())
            .collect();
        assert_eq!(marks, [skipped, skipped], "build: {build}");
        assert!(
            jobs[0]
                .steps
                .last()
                .is_some_and(|step| step.skip_after_merge_group.is_some()),
            "the clippy check is skipped either way"
        );
    }
}
