// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod archive;

use velnor_model::{
    CargoCommand, HelperCommand, Job, JobId, RepoFacts, RepoPath, Shape, ShapeDecision, Step,
    StepKind, When,
};

use crate::audit_job::{AuditLocation, audit_when};
use crate::base_cache::{save_steps, saving_run};
use crate::lane::Lane;
use crate::members::{Member, check_alone, lint};
use crate::plan::{LINT_JOB, RUST_JOB, TEST_JOB, job, setup_units, step};
use crate::units::Unit;

const ARCHIVE_ID: JobId = JobId::from_static("nextest-archive");
const ARCHIVE_FILE: RepoPath = RepoPath::from_static("nextest-archive.tar.zst");

struct RootJobs<'a> {
    facts: &'a RepoFacts,
    unit: &'a Unit,
    lane: &'a Lane,
    members: Vec<Member>,
    suffixed: bool,
    audit: AuditLocation,
}

pub(crate) fn root_jobs(
    facts: &RepoFacts,
    unit: &Unit,
    decision: ShapeDecision,
    suffixed: bool,
    lane: &Lane,
) -> Vec<Job> {
    let members: Vec<Member> = unit
        .roots
        .iter()
        .map(|root| Member::new(root, lane, facts, unit.grouped))
        .collect();
    let builder = RootJobs {
        facts,
        unit,
        lane,
        members,
        suffixed,
        audit: if suffixed
            || (decision.shape != Shape::OneJob && crate::base_cache::writes_base(facts))
        {
            AuditLocation::Separate
        } else {
            AuditLocation::Inline
        },
    };
    let comment = Some(decision.reason);
    match decision.shape {
        Shape::OneJob => vec![Job {
            comment,
            ..builder.single()
        }],
        Shape::LintAndTest => builder.split(comment, false),
        Shape::BuildThenGroups => builder.split(comment, !unit.grouped),
    }
}

impl RootJobs<'_> {
    fn id(&self, base: JobId) -> JobId {
        if self.suffixed {
            base.suffixed(self.unit.slug.as_str())
        } else {
            base
        }
    }

    fn cargo(
        &self,
        member: &Member,
        commands: impl IntoIterator<Item = CargoCommand>,
    ) -> Vec<Step> {
        commands
            .into_iter()
            .flat_map(|command| {
                let skipped = match &command {
                    CargoCommand::NextestFeatures {
                        features, packages, ..
                    } if !packages.is_empty() => {
                        Some(member.lane.skipped_lane(features, packages, &member.root))
                    }
                    _ => None,
                };
                let mut step = member.lane.cargo(command, Some(&member.root));
                if self.unit.grouped {
                    step.directory = None;
                    step.manifest = Some(member.root.manifest.clone());
                    if step.target_dir.is_some() {
                        step.target_dir = Some(self.unit.target_dir.clone());
                    }
                }
                [Some(step), skipped].into_iter().flatten()
            })
            .collect()
    }

    fn saves(&self, job: &JobId, archive: Option<RepoPath>, sources: bool) -> Vec<Step> {
        save_steps(self.facts, self.unit, self.lane, job, archive, sources)
    }

    fn setup(&self, job: &JobId) -> Vec<Step> {
        setup_units(
            self.facts,
            std::slice::from_ref(self.unit),
            self.lane,
            std::slice::from_ref(job),
        )
    }

    fn nextest(&self) -> bool {
        self.members.iter().any(|member| member.nextest)
    }

    fn lane_job(&self, id: JobId, steps: Vec<Step>) -> Job {
        let mut steps = if self.unit.grouped {
            steps
        } else {
            crate::test_setup::inserted(self.facts, &self.unit.roots[0], self.lane, steps)
        };
        crate::delta_cache::append(&mut steps, self.facts, self.unit, self.lane, &id);
        Job {
            permissions: crate::base_cache::lane_permissions(self.facts),
            needs: self
                .lane
                .plan
                .clone()
                .into_iter()
                .chain(
                    ((self.audit == AuditLocation::Separate)
                        && crate::base_cache::writes_base(self.facts))
                    .then_some(crate::audit_job::AUDIT_JOB),
                )
                .collect(),
            when: if (self.audit == AuditLocation::Separate)
                && crate::base_cache::writes_base(self.facts)
            {
                When::NotCancelled
            } else {
                When::Success
            },
            gate: self.lane.job(),
            steps,
            ..job(self.facts, id)
        }
    }

    fn audit(&self) -> Option<Step> {
        if self.audit == AuditLocation::Separate {
            return None;
        }
        let when = audit_when(self.facts)?;
        Some(Step {
            when,
            gate: self.lane.steps(),
            ..step(StepKind::Helper(HelperCommand::InputsAudit))
        })
    }

    fn member_steps(&self, member: &Member, steps: Vec<Step>) -> Vec<Step> {
        if self.unit.grouped {
            crate::test_setup::inserted(self.facts, &member.root, &member.lane, steps)
        } else {
            steps
        }
    }

    fn heavy(&self, step: Step) -> Step {
        Step {
            pr: if matches!(
                step.kind,
                StepKind::Cargo(
                    CargoCommand::NextestBuild
                        | CargoCommand::NextestRun { .. }
                        | CargoCommand::NextestFeatures { .. }
                        | CargoCommand::LaneGuard { .. }
                        | CargoCommand::TestTarget { .. }
                        | CargoCommand::Doctest
                )
            ) {
                crate::plan::full(self.facts)
            } else {
                step.pr
            },
            ..step
        }
    }

    fn checks(&self, member: &Member) -> Vec<Step> {
        self.cargo(
            member,
            [CargoCommand::Doc]
                .into_iter()
                .chain(check_alone(&member.root)),
        )
    }

    fn tests_of(&self, member: &Member) -> Vec<Step> {
        let tests = self
            .cargo(member, member.tests(self.facts))
            .into_iter()
            .map(|step| self.heavy(step))
            .collect();
        self.member_steps(member, tests)
    }

    fn single(&self) -> Job {
        let job_id = self.id(RUST_JOB);
        let mut steps = self.setup(&job_id);
        let saves = self.saves(&job_id, None, true);
        let before_tests = self.nextest() && !saves.is_empty();
        for member in &self.members {
            steps.extend(self.cargo(member, lint()));
        }
        if before_tests {
            for member in &self.members {
                steps.extend(self.checks(member));
            }
            for member in &self.members {
                if !member.nextest {
                    continue;
                }
                steps.extend(
                    self.cargo(member, vec![CargoCommand::NextestBuild])
                        .into_iter()
                        .map(|step| self.heavy(step))
                        .map(saving_run),
                );
            }
            for member in &self.members {
                steps.extend(self.tests_of(member));
            }
            steps.extend(self.audit());
            steps.extend(saves.iter().cloned());
        } else if saves.is_empty() {
            for member in &self.members {
                steps.extend(self.tests_of(member));
            }
            steps.extend(self.audit());
            for member in &self.members {
                steps.extend(self.checks(member));
            }
        } else {
            for member in &self.members {
                let mut workspace = vec![CargoCommand::Doc];
                let mut after = check_alone(&member.root);
                for command in member.tests(self.facts) {
                    match command {
                        CargoCommand::Doctest => workspace.push(command),
                        other => after.push(other),
                    }
                }
                let member_steps = self
                    .cargo(member, workspace)
                    .into_iter()
                    .chain(self.cargo(member, after))
                    .map(|step| self.heavy(step))
                    .collect();
                steps.extend(self.member_steps(member, member_steps));
            }
            steps.extend(self.audit());
            steps.extend(saves);
        }
        Job {
            pr: crate::plan::quick(self.facts),
            ..self.lane_job(job_id, steps)
        }
    }

    fn split(&self, comment: Option<String>, archive: bool) -> Vec<Job> {
        let tests: Vec<CargoCommand> = self
            .members
            .iter()
            .flat_map(|member| member.tests(self.facts))
            .collect();
        let lint_id = self.id(LINT_JOB);
        let mut steps = self.setup(&lint_id);
        for member in &self.members {
            steps.extend(self.cargo(member, lint().into_iter().chain([CargoCommand::Doc])));
        }
        if tests.is_empty() {
            steps.extend(self.audit());
        }
        steps.extend(self.saves(&lint_id, None, tests.is_empty()));
        for member in &self.members {
            steps.extend(self.cargo(member, check_alone(&member.root)));
        }
        let mut jobs = vec![Job {
            comment,
            pr: crate::plan::quick(self.facts),
            ..self.lane_job(lint_id, steps)
        }];
        if archive && self.nextest() && self.facts.config.private_sources.is_none() {
            jobs.extend(self.archived_tests());
        } else if !tests.is_empty() {
            let test_id = self.id(TEST_JOB);
            let mut steps = self.setup(&test_id);
            let saves = self.saves(&test_id, None, true);
            let saving = self.nextest() && !saves.is_empty();
            if saving {
                for member in self.members.iter().filter(|member| member.nextest) {
                    steps.extend(
                        self.cargo(member, vec![CargoCommand::NextestBuild])
                            .into_iter()
                            .map(saving_run),
                    );
                }
                for member in &self.members {
                    let tests = self.cargo(
                        member,
                        member
                            .nextest_run(self.facts)
                            .into_iter()
                            .chain(member.built_without_nextest(self.facts)),
                    );
                    steps.extend(self.member_steps(member, tests));
                }
                steps.extend(saves.iter().cloned());
            } else {
                for member in &self.members {
                    let tests = self.cargo(member, member.tests(self.facts));
                    steps.extend(self.member_steps(member, tests));
                }
            }
            steps.extend(self.audit());
            if !saving {
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
}
