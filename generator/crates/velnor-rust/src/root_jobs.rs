// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, FilterRef, Gate, GateNeed, Job, JobId, PlanFrom, RepoFacts, RepoPath, RustMember,
    RustRoot, Shape, ShapeDecision, Step, StepKind, TargetKind, When,
};

use crate::doc_steps::has_doctest_library;
use crate::mise::nextest_tool;
use crate::plan::{head, job, setup, step};

const RUST_JOB: JobId = JobId::from_static("rust");
const LINT_JOB: JobId = JobId::from_static("rust-lint");
const TEST_JOB: JobId = JobId::from_static("rust-test");
const DEFAULT_GROUP_JOB: JobId = JobId::from_static("rust-test-default");
const ARCHIVE_ID: JobId = JobId::from_static("nextest-archive");
const ARCHIVE_FILE: RepoPath = RepoPath::from_static("nextest-archive.tar.zst");

pub(crate) struct Lane {
    pub(crate) slug: String,
    pub(crate) plan: Option<JobId>,
}

impl Lane {
    fn gate(&self, need: GateNeed) -> Gate {
        Gate {
            slug: self.slug.clone(),
            need,
            from: self.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job),
        }
    }

    pub(crate) fn steps(&self) -> Option<Gate> {
        self.plan.is_none().then(|| self.gate(GateNeed::Selected))
    }

    fn job(&self) -> Option<Gate> {
        self.plan.is_some().then(|| self.gate(GateNeed::Selected))
    }

    fn cargo(&self, command: &CargoCommand) -> Option<Gate> {
        match command {
            CargoCommand::Doctest => Some(self.gate(GateNeed::Doctests)),
            CargoCommand::Doc => Some(self.gate(GateNeed::Docs)),
            _ => self.steps(),
        }
    }
}

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
        nextest: root.members.iter().any(RustMember::has_test_target),
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
            base.suffixed(&self.lane.slug)
        } else {
            base
        }
    }

    fn cargo(&self, commands: Vec<CargoCommand>) -> impl Iterator<Item = Step> {
        let lane = self.lane;
        let directory = self.root.manifest.parent();
        commands.into_iter().map(move |command| Step {
            gate: lane.cargo(&command),
            kind: StepKind::Cargo(command),
            directory: directory.clone(),
            when: When::NotCancelled,
        })
    }

    fn lane_job(&self, id: JobId, steps: Vec<Step>) -> Job {
        Job {
            needs: self.lane.plan.clone().into_iter().collect(),
            gate: self.lane.job(),
            steps,
            ..job(self.facts, id)
        }
    }

    fn tests(&self) -> Vec<CargoCommand> {
        let run = CargoCommand::NextestRun {
            filter: FilterRef::All,
        };
        self.nextest
            .then_some(run)
            .into_iter()
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
        let mut steps = setup(self.facts, self.root, true, self.lane);
        steps.extend(
            self.cargo(
                lint()
                    .into_iter()
                    .chain(self.tests())
                    .chain([CargoCommand::Doc])
                    .chain(check_alone(self.root))
                    .collect(),
            ),
        );
        self.lane_job(self.id(RUST_JOB), steps)
    }

    fn split(&self, comment: Option<String>, archive: bool) -> Vec<Job> {
        let mut steps = setup(self.facts, self.root, true, self.lane);
        steps.extend(
            self.cargo(
                lint()
                    .into_iter()
                    .chain([CargoCommand::Doc])
                    .chain(check_alone(self.root))
                    .collect(),
            ),
        );
        let mut jobs = vec![Job {
            comment,
            ..self.lane_job(self.id(LINT_JOB), steps)
        }];
        if archive && self.nextest {
            jobs.extend(self.archived_tests());
        } else if !self.tests().is_empty() {
            let mut steps = setup(self.facts, self.root, false, self.lane);
            steps.extend(self.cargo(self.tests()));
            jobs.push(self.lane_job(self.id(TEST_JOB), steps));
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
        let mut build = setup(self.facts, self.root, false, self.lane);
        build.extend(self.cargo(vec![CargoCommand::NextestArchive { file: ARCHIVE_FILE }]));
        build.push(self.gated(StepKind::UploadArchive {
            name: artifact.clone(),
            file: self.root.manifest.sibling(&ARCHIVE_FILE),
        }));
        build.extend(self.cargo(self.doctests()));
        let mut group = head(self.facts);
        group.extend([
            self.gated(StepKind::InstallTools {
                only: nextest_tool(&self.facts.tools).map(str::to_owned),
            }),
            self.gated(StepKind::DownloadArchive {
                name: artifact,
                into: self.root.manifest.parent(),
            }),
        ]);
        group.extend(
            self.cargo(vec![CargoCommand::NextestRunArchive {
                file: ARCHIVE_FILE,
                filter: FilterRef::All,
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

fn lint() -> Vec<CargoCommand> {
    vec![
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
    ]
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
