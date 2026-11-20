// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    ArtifactWhen, CacheKey, CargoCommand, FilterRef, GroupFilter, GroupUpload, Host, Job, JobId,
    NoTests, PrepareLocks, Step, StepKind, TestGroup, TestNeed, When,
};

use super::{ARCHIVE_FILE, ARCHIVE_ID, RootJobs};
use crate::base_cache::sources_locks;
use crate::members::Member;
use crate::mise::nextest_tool;
use crate::plan::{CHECKOUT_DEPTH, DEFAULT_GROUP_JOB, TEST_JOB, head, mbx, rust_additions, step};
use crate::plan_tools::{mirror_step_on, tools_prime, tools_restore};

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
            self.inline_lanes_timeout(self.lane_job(build_id.clone(), build)),
            self.archive_runner(
                self.id(DEFAULT_GROUP_JOB),
                &build_id,
                &artifact,
                None,
                self.default_filter(&groups),
                None,
            ),
        ];
        for group in groups {
            let base = TEST_JOB.suffixed(group.name.as_str());
            let filter = FilterRef::Group {
                include: Some(group.filter.clone()),
                exclude: Vec::new(),
                except: member.direct_binaries(self.facts),
            };
            let count = group.partition_count();
            if count < 2 {
                jobs.push(self.archive_runner(
                    self.id(base),
                    &build_id,
                    &artifact,
                    Some(group),
                    filter,
                    None,
                ));
                continue;
            }
            jobs.extend((1..=count).map(|index| {
                self.archive_runner(
                    self.id(base.suffixed(&index.to_string())),
                    &build_id,
                    &artifact,
                    Some(group),
                    filter.clone(),
                    Some((index, count)),
                )
            }));
        }
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

    fn artifact_steps(&self, group: &TestGroup, partition: Option<(u16, u16)>) -> Vec<Step> {
        group
            .artifacts
            .iter()
            .enumerate()
            .map(|(position, artifact)| Step {
                when: match artifact.when {
                    ArtifactWhen::Failure => When::Failure,
                    ArtifactWhen::Always => When::Always,
                },
                ..self.gated(StepKind::UploadGroupArtifact(GroupUpload {
                    position,
                    name: partition.map_or_else(
                        || artifact.name.clone(),
                        |(index, _)| format!("{}-{index}", artifact.name),
                    ),
                    paths: artifact.paths.clone(),
                    retention_days: artifact.retention_days,
                }))
            })
            .collect()
    }

    fn cache_restores(&self, build_id: &JobId) -> Vec<Step> {
        let cache_root = self.unit.cache_root(self.facts);
        [
            sources_locks(self.facts),
            step(StepKind::CacheRestore(CacheKey::Sources {
                host: Host,
                lane: cache_root.clone(),
            })),
            step(StepKind::CacheRestore(CacheKey::Target {
                job: build_id.clone(),
                root: cache_root,
                host: Host,
            })),
        ]
        .into_iter()
        .chain(crate::units::restore_file_times(
            self.facts,
            std::slice::from_ref(self.unit),
        ))
        .map(|restore| Step {
            gate: self.lane.steps(),
            ..restore
        })
        .collect()
    }

    fn archive_runner(
        &self,
        id: JobId,
        build_id: &JobId,
        artifact: &str,
        group: Option<&TestGroup>,
        filter: FilterRef,
        partition: Option<(u16, u16)>,
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
            crate::sparse::with_artifacts(
                crate::sparse::paths(self.facts, std::slice::from_ref(&self.member().root)),
                group.map_or(&[], |group| &group.artifacts),
            ),
        );
        steps.extend(mbx(self.facts, self.lane.steps()));
        steps.extend(mirror());
        steps.extend(tools_restore(self.facts).into_iter().map(|restore| Step {
            gate: self.lane.steps(),
            ..restore
        }));
        steps.extend(tools_prime(self.facts).map(|prime| Step {
            gate: self.lane.steps(),
            ..prime
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
            steps.push(self.gated(StepKind::Prepare(PrepareLocks::All)));
            if group.is_some_and(TestGroup::restores_cache) {
                steps.extend(self.cache_restores(build_id));
            }
        }
        steps.push(self.gated(StepKind::DownloadArchive {
            name: artifact.to_owned(),
            into: self.member().root.manifest.parent(),
        }));
        if docker {
            steps.push(self.gated(StepKind::TestSetup {
                name: "Check that Docker answers".to_owned(),
                run: "docker info".to_owned(),
                before_build: false,
            }));
        }
        steps.extend(
            self.cargo(
                self.member(),
                vec![CargoCommand::NextestRunArchive {
                    file: ARCHIVE_FILE,
                    filter,
                    threads: group.and_then(|group| group.test_threads),
                    partition,
                    ci_profile: self.member().root.nextest_ci_profile,
                    no_tests: group.map_or(NoTests::Pass, TestGroup::no_tests_mode),
                }],
            )
            .into_iter()
            .map(|step| Step {
                when: When::Success,
                ..step
            }),
        );
        steps.extend(
            group
                .into_iter()
                .flat_map(|group| self.artifact_steps(group, partition)),
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
