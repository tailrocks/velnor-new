// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerPublicationDockerfileContext {
    pub dockerfile: &'static str,
    pub tracked_sources: &'static [&'static str],
    pub named_contexts: &'static [&'static str],
}

impl RunnerPublicationDockerfileContext {
    pub fn analyze(
        &self,
        text: &str,
    ) -> Result<crate::config::DockerfileFacts, crate::config::DockerfileProblem> {
        let facts = crate::config::analyze_runner_dockerfile(text, self.named_contexts)?;
        if !self.matches_sources(&facts) {
            return Err(crate::config::DockerfileProblem::UnsupportedRunnerSyntax(
                "role_context".to_owned(),
            ));
        }
        Ok(facts)
    }

    #[must_use]
    pub fn matches_sources(&self, facts: &crate::config::DockerfileFacts) -> bool {
        facts.context_sources.len() == self.tracked_sources.len()
            && self
                .tracked_sources
                .iter()
                .all(|name| facts.context_sources.contains(*name))
            && facts.named_contexts.len() == self.named_contexts.len()
            && self
                .named_contexts
                .iter()
                .all(|name| facts.named_contexts.contains(*name))
    }
}

const RUNNER_CONTEXT_SOURCES: [&str; 13] = [
    "entrypoint.sh",
    "wait-docker-sock.sh",
    "wait-docker-api.sh",
    "clear-dockerenv.sh",
    "tar-shim.sh",
    "tar-absolute.sh",
    "tar-extract.sh",
    "tar-extract-plan.sh",
    "tar-member.pl",
    "tar-dir-meta.pl",
    "tar-member-stream.pl",
    "tar-member-rewrite.pl",
    "tar-pax.pl",
];

const DIND_CONTEXT_SOURCES: [&str; 3] =
    ["cgroup-v2-delegation.sh", "entrypoint.sh", "LICENSE-moby"];

const EMPTY_CONTEXT_SOURCES: [&str; 0] = [];
const RUNNER_NAMED_CONTEXTS: [&str; 4] = [
    "resource_probe",
    "storage_init",
    "job_start_hook",
    "job_started_hook",
];
const PROBE_NAMED_CONTEXT: [&str; 1] = ["resource_probe"];

#[must_use]
pub fn runner_publication_dockerfile_context(
    image: &str,
) -> Option<RunnerPublicationDockerfileContext> {
    match image {
        "runner" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/runner/ubuntu-26.04/Dockerfile",
            tracked_sources: &RUNNER_CONTEXT_SOURCES,
            named_contexts: &RUNNER_NAMED_CONTEXTS,
        }),
        "dind" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/dind/Dockerfile",
            tracked_sources: &DIND_CONTEXT_SOURCES,
            named_contexts: &PROBE_NAMED_CONTEXT,
        }),
        "resource_probe" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/resource-probe/Dockerfile",
            tracked_sources: &EMPTY_CONTEXT_SOURCES,
            named_contexts: &PROBE_NAMED_CONTEXT,
        }),
        _ => None,
    }
}
