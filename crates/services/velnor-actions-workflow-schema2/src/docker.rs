//! Provider-specific Docker endpoints shared by qualification probes.

use super::{RunnerLane, RunnerSpec};

const HOSTED_DOCKER_ENDPOINT: &str = "unix:///var/run/docker.sock";
const SCALE_SET_DOCKER_ENDPOINT: &str = "unix:///run/docker/docker.sock";

const HOSTED_DOCKER_GUARD: &str = r#"set -euo pipefail
test -z "${DOCKER_CONTEXT:-}"
case "${DOCKER_HOST:-}" in
  ""|unix:///var/run/docker.sock) ;;
  *) printf '%s\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;
esac
test -S /var/run/docker.sock
docker --host unix:///var/run/docker.sock info >/dev/null"#;

const SCALE_SET_DOCKER_GUARD: &str = r#"set -euo pipefail
test -z "${DOCKER_CONTEXT:-}"
test "${DOCKER_HOST:-}" = "unix:///run/docker/docker.sock"
test -S /run/docker/docker.sock
test ! -e /var/run/docker.sock
docker --host unix:///run/docker/docker.sock info >/dev/null"#;

pub(super) fn docker_endpoint(runner: &RunnerSpec) -> &'static str {
    if runner.lane == RunnerLane::Hosted {
        HOSTED_DOCKER_ENDPOINT
    } else {
        SCALE_SET_DOCKER_ENDPOINT
    }
}

pub(super) fn docker_provider_guard(runner: &RunnerSpec) -> (&'static str, &'static str) {
    if runner.lane == RunnerLane::Hosted {
        ("Require GitHub-hosted stock Docker", HOSTED_DOCKER_GUARD)
    } else {
        ("Require Velnor private DinD socket", SCALE_SET_DOCKER_GUARD)
    }
}
