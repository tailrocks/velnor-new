# velnor-local

The local GitHub environment, a development tool that is not shipped. It runs GitHub's official runner, unmodified, against `runner.server` on an Apple Silicon Mac, in OrbStack containers.

| Part | Pin |
|---|---|
| Stand-in | `ChristopherHX/runner.server` commit `992ccbbbf9afcde477c38c316e053b1af457ad40`, built by `docker/runner-server/Dockerfile`. One change: `DownloadArtifact` answers `Content-Type: application/zip`, without which `actions/download-artifact` saves the zip as a raw file |
| Runner | `actions/runner` 2.338.0, `actions-runner-linux-arm64-2.338.0.tar.gz`, sha256 `628b4a7258487b80c1d3c221095a7ded349f5ae0175fd3dfab708d07428f041b`, built by `docker/runner/Dockerfile` |
| Recorder | `mitmproxy/mitmproxy:12.1.2@sha256:0d7b2a14b4a71a908cd6f31b8f50e29bbc5c8720fe6caa706acf782da558e5c7`, a reverse proxy that plays `github.localhost` and records every runner call |

## Start

Run from the repository root. `STATE` is a directory outside the checkout. `$STATE/rec/` receives the recording. `$STATE/action-archive-cache/` holds `<owner>_<repo>/<sha>.tar.gz` files, each fetched once from `https://api.github.com/repos/<owner>/<repo>/tarball/<sha>`. Leave it empty to download every action in every job.

```sh
docker build -t velnor-local/runner-server local/docker/runner-server
docker build -t velnor-local/runner local/docker/runner
docker network create velnor-local
docker run -d --name velnor-runner-server --network velnor-local --network-alias runner-server -e Runner.Server__GitServerUrl=http://github.localhost velnor-local/runner-server
docker run -d --name velnor-proxy --network velnor-local --network-alias github.localhost -v "$STATE/rec:/rec" mitmproxy/mitmproxy:12.1.2@sha256:0d7b2a14b4a71a908cd6f31b8f50e29bbc5c8720fe6caa706acf782da558e5c7 mitmdump --mode reverse:http://runner-server:80 --listen-port 80 --set keep_host_header=true -w /rec/flows.mitm
docker run -d --name velnor-runner-1 --network velnor-local -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1 -e ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/action-archive-cache -v "$STATE/action-archive-cache:/opt/action-archive-cache:ro" velnor-local/runner sh -c './config.sh --unattended --url http://github.localhost/runner/server --token anything --name arm-1 --labels ubuntu-24.04-arm && exec ./run.sh'
docker run -d --name velnor-runner-2 --network velnor-local -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1 -e ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/action-archive-cache -v "$STATE/action-archive-cache:/opt/action-archive-cache:ro" velnor-local/runner sh -c './config.sh --unattended --url http://github.localhost/runner/server --token anything --name arm-2 --labels ubuntu-24.04-arm && exec ./run.sh'
```

## Run a workflow

`REPO` is a local checkout. Uncommitted changes are included. `$STATE/events/pull_request.json` is a `pull_request` payload.

```sh
docker run --rm --network velnor-local -v "$REPO:/repo" -w /repo --entrypoint dotnet velnor-local/runner-server /opt/runner.server/Runner.Client.dll --server http://runner-server --event push
docker run --rm --network velnor-local -v "$REPO:/repo" -v "$STATE/events:/events:ro" -w /repo --entrypoint dotnet velnor-local/runner-server /opt/runner.server/Runner.Client.dll --server http://runner-server --event pull_request --payload /events/pull_request.json --ref refs/pull/1/merge
```

## Why each setting

- `Runner.Server__GitServerUrl=http://github.localhost` becomes `GITHUB_SERVER_URL` in every job. `@actions/cache` uses the v2 cache protocol only when that host is `github.com`, ends in `.ghe.com` or ends in `.localhost`. For any other host, such as plain `localhost`, it assumes GitHub Enterprise Server and uses v1. `runner.server` sets the job variable `actions_uses_cache_service_v2=true` itself.
- `GITHUB_ACTIONS_RUNNER_FORCE_GHES=1` makes `config.sh` register through `/api/v3/actions/runner-registration`, which `runner.server` serves. Without it, the runner treats `github.localhost` as github.com and calls `api.github.localhost`, which does not exist.
- `Runner.Client` talks to `runner-server` directly. Through the proxy, which buffers the streamed `schedule2` answer, it times out after 100 seconds.
- `runner.server` replaces `actions/checkout` with its own local checkout. In the archive cache that action is `actions_checkout/992ccbbbf9afcde477c38c316e053b1af457ad40<checkout sha>.tar.gz`.

## Test data

`testdata/cache-client/` holds the calls of `@actions/cache` 6.1.0 (`actions/cache` commit `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`) on a miss with save, then on a hit. Requests come from the real client. Responses come from `runner.server`, not from GitHub. Bearer tokens and URL signatures are replaced with `REDACTED`, and binary bodies with their size.

## Cache rules

`crates/velnor-cache-rules/` answers "what would GitHub's cache do?" for any sequence of events: which scopes a run reads and writes, which entry a restore finds, whether a save is refused, and which entries GitHub evicts. It takes data and returns data. `crates/velnor-cache-rules/RULES.md` lists each rule with its source in GitHub's documentation, or marks it as modelled or probed.

Run its tests from `local/`:

```sh
cargo nextest run
```
