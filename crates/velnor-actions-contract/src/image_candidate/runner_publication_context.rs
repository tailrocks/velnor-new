// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerPublicationDockerfileContext {
    pub dockerfile: &'static str,
    pub tracked_sources: &'static [&'static str],
    pub generated_sources: &'static [&'static str],
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
const RESOURCE_PROBE_GENERATED_SOURCES: [&str; 1] = ["resource-probe"];

#[must_use]
pub fn runner_publication_dockerfile_context(
    image: &str,
) -> Option<RunnerPublicationDockerfileContext> {
    match image {
        "runner" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/runner/ubuntu-26.04/Dockerfile",
            tracked_sources: &RUNNER_CONTEXT_SOURCES,
            generated_sources: &EMPTY_CONTEXT_SOURCES,
        }),
        "dind" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/dind/Dockerfile",
            tracked_sources: &DIND_CONTEXT_SOURCES,
            generated_sources: &EMPTY_CONTEXT_SOURCES,
        }),
        "resource_probe" => Some(RunnerPublicationDockerfileContext {
            dockerfile: "images/resource-probe/Dockerfile",
            tracked_sources: &EMPTY_CONTEXT_SOURCES,
            generated_sources: &RESOURCE_PROBE_GENERATED_SOURCES,
        }),
        _ => None,
    }
}
