// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    HelperRelease, ImageConfig, ImagePrBuild, PrivateSources, RepoPath, RunnerLabel,
};

use crate::helper_integrity::{defaults, secure_with};
use crate::image_build::build_job;
use crate::image_check::check_job;
use crate::image_gate::{EXEMPTIONS_JOB, admin_token, exemptions_job, gate_job};
use crate::image_publish::publish_job;
use crate::image_release::{authority_job, promote_job};
use crate::release_steps::revoke_app_token;
use crate::rules::{RenderedJob, RuleViolation, check_rules};
use crate::workflow::{ShellcheckMarks, yaml_job};
use crate::yaml::{Yaml, emit, entry, text};
use crate::{GENERATED_HEADER, MAX_WORKFLOW_BYTES, RenderedWorkflow, seal};

pub const IMAGE_WORKFLOW_PATH: RepoPath = RepoPath::from_static(".github/workflows/image.yml");
pub const IMAGE_ENVIRONMENT: &str = "images";
pub const IMAGE_BUILDKIT: &str =
    "moby/buildkit@sha256:baf6138eb8c37644800c7c913fba051e840ebb9ba13eaf92d92ef9a1712d0b4c";
pub const IMAGE_SYSROOT_LOCK: &str = ".velnor/image-sysroot.lock";
pub const IMAGE_TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMbx {
    Off,
    Private,
    Shared,
}

#[derive(Debug, Clone)]
pub struct ImageBuild<'a> {
    pub image: &'a ImageConfig,
    pub root: &'a str,
    pub packages: Vec<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ImageWorkflow<'a> {
    pub images: Vec<ImageBuild<'a>>,
    pub helper: &'a HelperRelease,
    pub repository: &'a str,
    pub branch: &'a str,
    pub runner: &'a RunnerLabel,
    pub private: Option<&'a PrivateSources>,
    pub mbx: ImageMbx,
    pub skip_draft_pull_requests: bool,
}

impl ImageWorkflow<'_> {
    pub(crate) fn pr_only_checks(&self) -> Vec<String> {
        let mut checks: Vec<String> = self
            .images
            .iter()
            .flat_map(|build| build.image.pr_only_checks.iter().cloned())
            .collect();
        checks.sort();
        checks.dedup();
        checks
    }
}

pub(crate) struct Events<'a> {
    pub(crate) repository: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) skip_drafts: bool,
}

impl Events<'_> {
    pub(crate) fn branch_push(&self) -> String {
        format!(
            "github.event_name == 'push' && github.ref == 'refs/heads/{}' && github.repository == '{}'",
            self.branch, self.repository
        )
    }

    pub(crate) fn tag_push(&self, image: &ImageConfig) -> String {
        format!(
            "github.event_name == 'push' && startsWith(github.ref, 'refs/tags/{}-v') && github.repository == '{}'",
            image.name, self.repository
        )
    }

    pub(crate) fn pull_request(&self) -> String {
        let draft = if self.skip_drafts {
            " && github.event.pull_request.draft != true"
        } else {
            ""
        };
        format!("{}{draft}", Self::pull_request_base())
    }

    fn pull_request_base() -> &'static str {
        "github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, '[bot]') && !endsWith(github.event.pull_request.user.login, '[bot]') && (github.event.action != 'edited' || github.event.changes.base != null)"
    }
}

fn admin_position(job: &RenderedJob) -> Option<usize> {
    if job.id == EXEMPTIONS_JOB {
        Some(1)
    } else if job.id.ends_with("-authority") {
        Some(2)
    } else {
        None
    }
}

pub fn render_image(
    workflow: &ImageWorkflow<'_>,
) -> Result<BTreeMap<RepoPath, String>, Vec<RuleViolation>> {
    let events = Events {
        repository: workflow.repository,
        branch: workflow.branch,
        skip_drafts: workflow.skip_draft_pull_requests,
    };
    let mut jobs: Vec<RenderedJob> = Vec::new();
    if !workflow.pr_only_checks().is_empty() {
        jobs.push(exemptions_job(workflow, &events));
    }
    jobs.push(gate_job(workflow, &events));
    for build in &workflow.images {
        jobs.push(build_job(workflow, build, &events));
        jobs.extend(check_job(workflow, build, &events));
        jobs.push(publish_job(workflow, build, &events));
        jobs.push(authority_job(workflow, build, &events));
        jobs.push(promote_job(workflow, build));
    }
    let jobs: Vec<RenderedJob> = jobs
        .into_iter()
        .map(|job| {
            let job = match admin_position(&job) {
                Some(position) if !workflow.pr_only_checks().is_empty() => {
                    revoke_app_token(admin_token(workflow, job, position))
                }
                _ => job,
            };
            RenderedJob {
                steps: secure_with(workflow.helper, job.steps),
                ..job
            }
        })
        .collect();
    let rendered = RenderedWorkflow { jobs };
    check_rules(&rendered)?;
    let document = [
        Some(entry("name", text("Image"))),
        Some(entry("on", triggers(workflow))),
        Some(entry("permissions", Yaml::Map(Vec::new()))),
        Some(entry(
            "concurrency",
            Yaml::Map(vec![
                entry(
                    "group",
                    Yaml::Expr(
                        "image-${{ github.ref }}${{ github.event.action == 'edited' && github.event.changes.base == null && format('-edit-{0}', github.run_id) || '' }}"
                            .to_owned(),
                    ),
                ),
                entry(
                    "cancel-in-progress",
                    Yaml::Expr(
                        "${{ github.event_name == 'pull_request' && !(github.event.action == 'edited' && github.event.changes.base == null) }}"
                            .to_owned(),
                    ),
                ),
            ]),
        )),
        Some(entry(
            "env",
            Yaml::Map(vec![
                entry("MISE_AUTO_INSTALL", text("false")),
                entry("RUSTUP_AUTO_INSTALL", text("0")),
            ]),
        )),
        defaults(&rendered.jobs).map(|value| entry("defaults", value)),
        Some(entry(
            "jobs",
            Yaml::Map(
                rendered
                    .jobs
                    .iter()
                    .map(|job| (job.id.clone(), yaml_job(job, ShellcheckMarks::Jobs)))
                    .collect(),
            ),
        )),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let file = seal(GENERATED_HEADER, &emit(&document));
    if file.len() > MAX_WORKFLOW_BYTES {
        return Err(vec![crate::rule_messages::size_report::too_large(
            IMAGE_WORKFLOW_PATH.as_str(),
            &file,
        )]);
    }
    Ok(BTreeMap::from([(IMAGE_WORKFLOW_PATH, file)]))
}

fn names(items: &[String]) -> Yaml {
    Yaml::Seq(items.iter().map(|item| text(item)).collect())
}

fn triggers(workflow: &ImageWorkflow<'_>) -> Yaml {
    let mut events = Vec::new();
    let reaching: Vec<&ImageBuild<'_>> = workflow
        .images
        .iter()
        .filter(|build| build.image.pr_build != ImagePrBuild::Off)
        .collect();
    if !reaching.is_empty() {
        let mut pull_request = vec![entry(
            "types",
            names(
                &[
                    "opened",
                    "synchronize",
                    "reopened",
                    "ready_for_review",
                    "edited",
                ]
                .map(str::to_owned),
            ),
        )];
        if reaching
            .iter()
            .all(|build| build.image.pr_build == ImagePrBuild::Inputs)
        {
            pull_request.push(entry("paths", names(&pr_paths(&reaching))));
        }
        events.push(entry("pull_request", Yaml::Map(pull_request)));
    }
    events.push(entry(
        "push",
        Yaml::Map(vec![
            entry("branches", names(&[workflow.branch.to_owned()])),
            entry(
                "tags",
                names(
                    &workflow
                        .images
                        .iter()
                        .map(|build| build.image.tag_glob())
                        .collect::<Vec<_>>(),
                ),
            ),
        ]),
    ));
    Yaml::Map(events)
}

fn pr_paths(builds: &[&ImageBuild<'_>]) -> Vec<String> {
    let mut paths = vec![
        IMAGE_WORKFLOW_PATH.to_string(),
        "mise.toml".to_owned(),
        "mise.lock".to_owned(),
        ".velnor/rust-ci.toml".to_owned(),
        ".velnor/rust-ci.lock".to_owned(),
        "**/Cargo.toml".to_owned(),
        "**/Cargo.lock".to_owned(),
        "**/.cargo/**".to_owned(),
        "**/rust-toolchain*".to_owned(),
        ".dockerignore".to_owned(),
    ];
    for build in builds {
        let image = build.image;
        paths.push(image.dockerfile.clone());
        paths.push(format!("{}.dockerignore", image.dockerfile));
        if image.context != "." {
            paths.push(format!("{}/.dockerignore", image.context));
        }
        for input in &image.inputs {
            paths.extend([input.clone(), format!("{input}/**")]);
        }
        if image.openssl == velnor_model::ImageOpenssl::Sysroot {
            paths.push(IMAGE_SYSROOT_LOCK.to_owned());
        }
    }
    paths.sort();
    paths.dedup();
    paths
}
