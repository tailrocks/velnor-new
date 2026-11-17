// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, CustomHarness, FilterRef, FullRun, HelperCommand, Job, JobId, OnMain,
    PackageName, RepoFacts, RepoPath, RustRoot, Shape, ShapeDecision, Step, StepKind, TargetKind,
    When,
};

use crate::base_cache::{save_steps, saving_run};
use crate::doc_steps::has_doctest_library;
use crate::harness::{runs_in_direct_step, selector_id, uses_nextest};
use crate::lane::Lane;
use crate::mise::nextest_tool;
use crate::plan::{
    CHECKOUT_DEPTH, HELPER_JOB, LINT_JOB, RUST_JOB, TEST_JOB, head, job, mirror_step, setup, step,
    tools_restore,
};

const DEFAULT_GROUP_JOB: JobId = JobId::from_static("rust-test-default");
const ARCHIVE_ID: JobId = JobId::from_static("nextest-archive");
const ARCHIVE_FILE: RepoPath = RepoPath::from_static("nextest-archive.tar.zst");

struct RootJobs<'a> {
    facts: &'a RepoFacts,
    root: &'a RustRoot,
    lane: &'a Lane,
    suffixed: bool,
    nextest: bool,
    doctests: bool,
}

pub(crate) fn root_jobs(
    facts: &RepoFacts,
    root: &RustRoot,
    decision: ShapeDecision,
    suffixed: bool,
    lane: &Lane,
) -> Vec<Job> {
    let builder = RootJobs {
        facts,
        root,
        lane,
        suffixed,
        nextest: root
            .members
            .iter()
            .any(|member| uses_nextest(&facts.config, member)),
        doctests: has_doctest_library(root),
    };
    let comment = Some(decision.reason);
    match decision.shape {
        Shape::OneJob => vec![Job {
            comment,
            ..builder.single()
        }],
        Shape::LintAndTest => builder.split(comment, false),
        Shape::BuildThenGroups => builder.split(comment, true),
    }
}

impl RootJobs<'_> {
    fn id(&self, base: JobId) -> JobId {
        if self.suffixed {
            base.suffixed(self.lane.slug.as_str())
        } else {
            base
        }
    }

    fn cargo(&self, commands: Vec<CargoCommand>) -> impl Iterator<Item = Step> {
        let lane = self.lane;
        let directory = self.root.manifest.parent();
        commands
            .into_iter()
            .map(move |command| lane.cargo(command, directory.clone()))
    }

    fn saves(&self, job: &JobId, archive: Option<RepoPath>, sources: bool) -> Vec<Step> {
        save_steps(self.facts, self.root, self.lane, job, archive, sources)
    }

    fn lane_job(&self, id: JobId, steps: Vec<Step>) -> Job {
        Job {
            needs: self
                .lane
                .plan
                .clone()
                .into_iter()
                .chain(self.lane.from_source.then_some(HELPER_JOB))
                .collect(),
            gate: self.lane.job(),
            steps,
            ..job(self.facts, id)
        }
    }

    fn audit(&self) -> Option<Step> {
        if self.facts.rust_roots.len() != 1 {
            return None;
        }
        let when = audit_when(self.facts)?;
        Some(Step {
            when,
            gate: self.lane.steps(),
            ..step(StepKind::Helper(HelperCommand::InputsAudit))
        })
    }

    fn direct_harnesses(&self) -> Vec<CargoCommand> {
        let mut seen = BTreeSet::new();
        self.root
            .members
            .iter()
            .flat_map(|member| {
                member
                    .custom_harnesses
                    .iter()
                    .filter(|harness| runs_in_direct_step(&self.facts.config, member, harness))
                    .filter(|harness| seen.insert((member.name.clone(), selector_id(harness))))
                    .map(|harness| CargoCommand::TestTarget {
                        package: member.name.clone(),
                        target: harness.name.clone(),
                        kind: harness.kind,
                        features: harness.required_features.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn filter(&self) -> FilterRef {
        let direct: Vec<String> = self
            .root
            .members
            .iter()
            .flat_map(|member| {
                member
                    .custom_harnesses
                    .iter()
                    .filter(|harness| runs_in_direct_step(&self.facts.config, member, harness))
                    .map(|harness| binary_id(&member.name, harness))
            })
            .collect();
        if direct.is_empty() {
            FilterRef::All
        } else {
            FilterRef::AllExcept(direct)
        }
    }

    fn tests(&self) -> Vec<CargoCommand> {
        let run = CargoCommand::NextestRun {
            filter: self.filter(),
        };
        self.nextest
            .then_some(run)
            .into_iter()
            .chain(self.direct_harnesses())
            .chain(self.doctests())
            .collect()
    }

    fn doctests(&self) -> Vec<CargoCommand> {
        self.doctests
            .then_some(CargoCommand::Doctest)
            .into_iter()
            .collect()
    }

    fn single(&self) -> Job {
        let draft = self.facts.config.skip_draft_pull_requests;
        let skip_on_draft = |step: Step| Step {
            skip_on_draft: draft
                && matches!(
                    step.kind,
                    StepKind::Cargo(
                        CargoCommand::NextestBuild
                            | CargoCommand::NextestRun { .. }
                            | CargoCommand::TestTarget { .. }
                            | CargoCommand::Doctest
                    )
                ),
            ..step
        };
        let job_id = self.id(RUST_JOB);
        let mut steps = setup(self.facts, self.root, self.lane, &job_id);
        let saves = self.saves(&job_id, None, true);
        let before_tests = self.nextest && !saves.is_empty();
        let checks = || {
            self.cargo(
                [CargoCommand::Doc]
                    .into_iter()
                    .chain(check_alone(self.root))
                    .collect(),
            )
        };
        steps.extend(self.cargo(lint()));
        if before_tests {
            steps.extend(checks());
            steps.extend(
                self.cargo(vec![CargoCommand::NextestBuild])
                    .map(skip_on_draft)
                    .map(saving_run),
            );
            steps.extend(saves.iter().cloned());
            steps.extend(self.cargo(self.tests()).map(skip_on_draft));
            steps.extend(self.audit());
        } else if saves.is_empty() {
            steps.extend(self.cargo(self.tests()).map(skip_on_draft));
            steps.extend(self.audit());
            steps.extend(checks());
        } else {
            let mut workspace = vec![CargoCommand::Doc];
            let mut after = check_alone(self.root);
            for command in self.tests() {
                match command {
                    CargoCommand::Doctest => workspace.push(command),
                    other => after.push(other),
                }
            }
            steps.extend(self.cargo(workspace));
            steps.extend(saves);
            steps.extend(self.cargo(after).map(skip_on_draft));
            steps.extend(self.audit());
        }
        Job {
            skip_on_draft: false,
            ..self.lane_job(job_id, steps)
        }
    }

    fn split(&self, comment: Option<String>, archive: bool) -> Vec<Job> {
        let tests = self.tests();
        let lint_id = self.id(LINT_JOB);
        let mut steps = setup(self.facts, self.root, self.lane, &lint_id);
        steps.extend(self.cargo(lint().into_iter().chain([CargoCommand::Doc]).collect()));
        if tests.is_empty() {
            steps.extend(self.audit());
        }
        steps.extend(self.saves(&lint_id, None, tests.is_empty()));
        steps.extend(self.cargo(check_alone(self.root)));
        let mut jobs = vec![Job {
            comment,
            skip_on_draft: false,
            ..self.lane_job(lint_id, steps)
        }];
        if archive && self.nextest {
            jobs.extend(self.archived_tests());
        } else if !tests.is_empty() {
            let test_id = self.id(TEST_JOB);
            let mut steps = setup(self.facts, self.root, self.lane, &test_id);
            let saves = self.saves(&test_id, None, true);
            let before_tests = self.nextest && !saves.is_empty();
            if before_tests {
                steps.extend(self.cargo(vec![CargoCommand::NextestBuild]).map(saving_run));
                steps.extend(saves.iter().cloned());
            }
            steps.extend(self.cargo(tests));
            steps.extend(self.audit());
            if !before_tests {
                steps.extend(saves);
            }
            jobs.push(self.lane_job(test_id, steps));
        }
        jobs
    }

    fn gated(&self, kind: StepKind) -> Step {
        Step {
            gate: self.lane.steps(),
            ..step(kind)
        }
    }

    fn archived_tests(&self) -> [Job; 2] {
        let build_id = self.id(TEST_JOB);
        let artifact = self.id(ARCHIVE_ID).to_string();
        let mut build = setup(self.facts, self.root, self.lane, &build_id);
        build.extend(self.cargo(vec![CargoCommand::NextestArchive { file: ARCHIVE_FILE }]));
        build.push(self.gated(StepKind::UploadArchive {
            name: artifact.clone(),
            file: self.root.manifest.sibling(&ARCHIVE_FILE),
        }));
        build.extend(self.saves(
            &build_id,
            Some(self.root.manifest.sibling(&ARCHIVE_FILE)),
            true,
        ));
        build.extend(
            self.cargo(
                self.direct_harnesses()
                    .into_iter()
                    .chain(self.doctests())
                    .collect(),
            ),
        );
        build.extend(self.audit());
        let mut group = head(self.facts, CHECKOUT_DEPTH, self.lane.from_source);
        group.extend(mirror_step(self.facts).map(|step| self.gated(step.kind)));
        group.extend(tools_restore(self.facts).map(|restore| Step {
            gate: self.lane.steps(),
            ..restore
        }));
        group.push(self.gated(StepKind::InstallTools {
            only: nextest_tool(&self.facts.tools).map(str::to_owned),
        }));
        group.extend(mirror_step(self.facts).map(|step| self.gated(step.kind)));
        group.push(self.gated(StepKind::DownloadArchive {
            name: artifact,
            into: self.root.manifest.parent(),
        }));
        group.extend(
            self.cargo(vec![CargoCommand::NextestRunArchive {
                file: ARCHIVE_FILE,
                filter: self.filter(),
            }])
            .map(|step| Step {
                when: When::Success,
                ..step
            }),
        );
        let mut group_job = self.lane_job(self.id(DEFAULT_GROUP_JOB), group);
        group_job.needs.push(build_id.clone());
        [self.lane_job(build_id, build), group_job]
    }
}

pub(crate) fn audit_when(facts: &RepoFacts) -> Option<When> {
    match (facts.config.on_main, &facts.config.full_run) {
        (OnMain::Everything, _) => Some(When::ScheduledOrPush),
        (OnMain::Affected, FullRun::Off) => None,
        (OnMain::Affected, _) => Some(When::Scheduled),
    }
}

fn lint() -> Vec<CargoCommand> {
    vec![
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
    ]
}

fn binary_id(package: &PackageName, harness: &CustomHarness) -> String {
    let name = &harness.name;
    match harness.kind {
        TargetKind::Lib | TargetKind::ProcMacro => package.to_string(),
        TargetKind::Bin => format!("{package}::bin/{name}"),
        TargetKind::Example => format!("{package}::example/{name}"),
        TargetKind::Bench => format!("{package}::bench/{name}"),
        TargetKind::Test | TargetKind::BuildScript => format!("{package}::{name}"),
    }
}

fn check_alone(root: &RustRoot) -> Vec<CargoCommand> {
    root.members
        .iter()
        .filter(|member| {
            member.targets.iter().any(|target| {
                !target.requires_features
                    && target.kinds.iter().any(|kind| {
                        matches!(
                            kind,
                            TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin
                        )
                    })
            })
        })
        .map(|member| CargoCommand::CheckAlone {
            package: member.name.clone(),
        })
        .collect()
}
