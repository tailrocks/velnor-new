// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, FilterRef, GroupFilter, HelperCommand, Job, JobId, Step, StepKind, TestGroup,
    TestNeed, When,
};

use super::{ARCHIVE_FILE, ARCHIVE_ID, RootJobs};
use crate::members::Member;
use crate::mise::nextest_tool;
use crate::plan::{CHECKOUT_DEPTH, DEFAULT_GROUP_JOB, TEST_JOB, head, rust_additions};
use crate::plan_tools::{mirror_step_on, tools_restore};

impl RootJobs<'_> {
    fn member(&self) -> &Member {
        &self.members[0]
    }

    pub(super) fn groups(&self) -> Vec<&TestGroup> {
        crate::config_test_groups::of_root(
            &self.facts.config,
            &self.member().root,
            self.facts.rust_roots.len(),
        )
        .collect()
    }

    fn default_filter(&self, groups: &[&TestGroup]) -> FilterRef {
        if groups.is_empty() {
            self.member().filter(self.facts)
        } else {
            FilterRef::Group {
                include: None,
                exclude: groups.iter().map(|group| group.filter.clone()).collect(),
                except: self.member().direct_binaries(self.facts),
            }
        }
    }

    pub(super) fn archived_tests(&self) -> Vec<Job> {
        let build_id = self.id(TEST_JOB);
        let artifact = self.id(ARCHIVE_ID).to_string();
        let member = self.member();
        let mut build = self.setup(&build_id);
        build.extend(self.cargo(
            member,
            vec![CargoCommand::NextestArchive { file: ARCHIVE_FILE }],
        ));
        let groups = self.groups();
        build.extend(self.overlap_check(&groups));
        build.push(self.gated(StepKind::UploadArchive {
            name: artifact.clone(),
            file: member.root.manifest.sibling(&ARCHIVE_FILE),
        }));
        build.extend(self.cargo(member, member.built_without_nextest(self.facts)));
        build.extend(self.audit());
        build.extend(self.saves(
            &build_id,
            Some(member.root.manifest.sibling(&ARCHIVE_FILE)),
            true,
        ));
        let mut jobs = vec![
            self.lane_job(build_id.clone(), build),
            self.archive_runner(
                self.id(DEFAULT_GROUP_JOB),
                &build_id,
                &artifact,
                None,
                self.default_filter(&groups),
            ),
        ];
        jobs.extend(groups.iter().map(|group| {
            self.archive_runner(
                self.id(TEST_JOB.suffixed(group.name.as_str())),
                &build_id,
                &artifact,
                Some(group),
                FilterRef::Group {
                    include: Some(group.filter.clone()),
                    exclude: Vec::new(),
                    except: member.direct_binaries(self.facts),
                },
            )
        }));
        jobs
    }

    fn overlap_check(&self, groups: &[&TestGroup]) -> Option<Step> {
        (groups.len() > 1).then(|| Step {
            directory: self.member().root.manifest.parent(),
            ..self.gated(StepKind::TestGroupsCheck(
                groups
                    .iter()
                    .map(|group| GroupFilter {
                        name: group.name.to_string(),
                        filter: group.filter.clone(),
                    })
                    .collect(),
            ))
        })
    }

    fn archive_runner(
        &self,
        id: JobId,
        build_id: &JobId,
        artifact: &str,
        group: Option<&TestGroup>,
        filter: FilterRef,
    ) -> Job {
        let runner = group
            .and_then(|group| group.runner.clone())
            .unwrap_or_else(|| self.facts.config.runner.clone());
        let cargo = group.is_some_and(|group| group.needs.contains(&TestNeed::Cargo));
        let docker = group.is_some_and(|group| group.needs.contains(&TestNeed::Docker));
        let mirror = || mirror_step_on(self.facts, &runner).map(|step| self.gated(step.kind));
        let mut steps = head(
            self.facts,
            CHECKOUT_DEPTH,
            crate::sparse::paths(self.facts, std::slice::from_ref(&self.member().root)),
        );
        steps.extend(mirror());
        steps.extend(tools_restore(self.facts).into_iter().map(|restore| Step {
            gate: self.lane.steps(),
            ..restore
        }));
        steps.push(self.gated(StepKind::InstallTools {
            only: if cargo {
                None
            } else {
                nextest_tool(&self.facts.tools).map(str::to_owned)
            },
        }));
        steps.extend(mirror());
        if cargo {
            steps.extend(
                rust_additions(self.facts.toolchain.as_ref(), &self.member().root)
                    .into_iter()
                    .map(|step| Step {
                        gate: self.lane.steps(),
                        ..step
                    }),
            );
            steps.push(self.gated(StepKind::Helper(HelperCommand::Prepare)));
        }
        steps.push(self.gated(StepKind::DownloadArchive {
            name: artifact.to_owned(),
            into: self.member().root.manifest.parent(),
        }));
        if docker {
            steps.push(self.gated(StepKind::TestSetup {
                name: "Check that Docker answers".to_owned(),
                run: "docker info".to_owned(),
            }));
        }
        steps.extend(
            self.cargo(
                self.member(),
                vec![CargoCommand::NextestRunArchive {
                    file: ARCHIVE_FILE,
                    filter,
                    threads: group.and_then(|group| group.test_threads),
                    ci_profile: self.member().root.nextest_ci_profile,
                }],
            )
            .into_iter()
            .map(|step| Step {
                when: When::Success,
                ..step
            }),
        );
        let mut job = self.lane_job(id, steps);
        if let Some(group) = group.filter(|group| group.selects()) {
            job.gate = self.lane.group_job(group.name.clone()).or(job.gate);
        }
        job.needs.push(build_id.clone());
        job.runs_on = runner;
        if let Some(minutes) = group.and_then(|group| group.timeout_minutes) {
            job.timeout_minutes = minutes;
        }
        job
    }
}
