# velnor-local

The local GitHub environment, a development tool that is not shipped. It runs GitHub's official runner, unmodified, against `runner.server` on an Apple Silicon Mac, in OrbStack containers, with a cache that follows GitHub's cache rules.

| Part | Pin |
|---|---|
| Stand-in | `ChristopherHX/runner.server` commit `992ccbbbf9afcde477c38c316e053b1af457ad40`, built by `docker/runner-server/Dockerfile`. One change: `DownloadArtifact` answers `Content-Type: application/zip`, without which `actions/download-artifact` saves the zip as a raw file |
| Runner | `actions/runner` 2.338.0, `actions-runner-linux-arm64-2.338.0.tar.gz`, sha256 `628b4a7258487b80c1d3c221095a7ded349f5ae0175fd3dfab708d07428f041b`, built by `docker/runner/Dockerfile` |
| Cache server | `velnor-local cache-server` from this workspace, built by `docker/velnor-local/Dockerfile`. It owns the name `github.localhost`, answers the cache calls and forwards every other request to the stand-in unchanged |
| Recorder | `mitmproxy/mitmproxy:12.1.2@sha256:0d7b2a14b4a71a908cd6f31b8f50e29bbc5c8720fe6caa706acf782da558e5c7`, used only to record runner calls: give it the alias `github.localhost` and `mitmdump --mode reverse:http://velnor-cache-server:80 --listen-port 80 --set keep_host_header=true`, and start the cache server without that alias |

## Start

Build the images once, from the repository root, then build `velnor-local` from `local/`:

```sh
docker build -t velnor-local/runner-server local/docker/runner-server
docker build -t velnor-local/runner local/docker/runner
docker build -t velnor-local/cache-server -f local/docker/velnor-local/Dockerfile local
cargo build --release --manifest-path local/Cargo.toml
```

`velnor-local up --dir "$STATE" --repo "$REPO" [--runners 2]` starts one environment and `velnor-local down --dir "$STATE"` removes its containers and network. `STATE` is a directory outside the checkout, without spaces. It holds `keys/` (the runtime token key pair, made once with `openssl` in the stand-in image), `cache/` (the cache server's archives and `index.json`) and `api/` (the API stub's `pulls.json`). The environment is named `velnor-<hash of STATE>`: a network of that name, the containers `<name>-standin`, `<name>-cache`, `<name>-api` and `<name>-runner-<n>`, all labelled `velnor-local=<name>`. Two state directories are two independent environments. Only the cache server and the API stub publish a port, each on a free port of `127.0.0.1`; `up` prints the stub's address. `up` returns when every runner has a session with runner.server.

Every runner container is linux/arm64 and registers with the labels `ubuntu-latest`, `ubuntu-22.04`, `ubuntu-24.04`, `ubuntu-26.04` and their `-arm` forms, so a workflow written for hosted runners runs unchanged and `runner.arch` is `ARM64`. Every one of these labels runs on the same Ubuntu 24.04 image, so a difference between Ubuntu releases does not show locally; the verdict says so. The runner image has `build-essential`, as hosted Ubuntu runners do.

## Run a scenario

```sh
velnor-local run warm --repo "$REPO" --dir "$STATE" --workflow .github/workflows/ci.yml --repository owner/name
```

A scenario is a JSON file in `scenarios/` (`--scenarios` names another directory; a path ending in `.json` is read as is): a description and an ordered list of steps.

| Step | What the driver does |
|---|---|
| `{"push": "<branch>"}` | Sends a `push` event with `ref`, `before` (the branch's previous push in this run, or forty zeros) and `after` |
| `{"commit": {"branch": "<b>", "append": "<path>", "line": "<text>"}}` | Appends the line to the file, which may be new, on the branch, created from the default branch when missing, and commits it |
| `{"open_pull_request": {"head": "<b>", "base": "<b>"}}` | Merges head into base as `refs/pull/<n>/merge` would, writes the pull request to `pulls.json`, registers its base with the cache server and sends `pull_request` `opened` with `--ref refs/pull/<n>/merge` |

`run` copies `REPO` to `$STATE/repo`, commits its uncommitted changes to tracked files there, and makes that commit the default branch of the copy (`--default-branch`, `main` unless set), so the scenario starts from exactly what is on disk, whichever branch `REPO` has checked out, and `REPO` is never modified. A step names the default branch as `{default}`. It starts the environment from an empty cache, replays the steps, stops the environment on every exit (a failed `up` and Ctrl-C included), and prints a verdict: each run with its workflow, event and conclusion, the seconds from the start of `up` to the first job, and what was executed and what was stood in. Event payloads have the fields that `conformance/event-payload.json` lists. Each event's payload is in `$STATE/events/` and the log of its runs in `$STATE/logs/`. `--workflow` limits the runs to one workflow file; without it, runner.server runs every workflow the event triggers. `run` exits with an error unless every run succeeded. It refuses a branch, path, workflow, repository or default branch that is empty or starts with `-`, and an appended path that is absolute, contains `..` or leads out of the copy through a link. It fails when `Runner.Client` exits with an error before it creates a run, and when the old `cache/`, `api/`, `events/` or `logs/` cannot be removed.

## Why each setting

- `Runner.Server__GitApiServerUrl=http://api-stub` becomes `GITHUB_API_URL` in every job, so the helper's API calls reach the stub. Action downloads keep their own URLs (`Runner.Server:ActionDownloadUrls`).
- `Runner.Server__GitServerUrl=http://github.localhost` becomes `GITHUB_SERVER_URL` in every job. `@actions/cache` uses the v2 cache protocol only when that host is `github.com`, ends in `.ghe.com` or ends in `.localhost`. For any other host, such as plain `localhost`, it assumes GitHub Enterprise Server and uses v1. `runner.server` sets the job variable `actions_uses_cache_service_v2=true` itself.
- `GITHUB_ACTIONS_RUNNER_FORCE_GHES=1` makes `config.sh` register through `/api/v3/actions/runner-registration`, which `runner.server` serves. Without it, the runner treats `github.localhost` as github.com and calls `api.github.localhost`, which does not exist.
- `runner.server` builds the job's cache address from the host the runner called, so the cache server receives the cache calls when it owns `github.localhost`. No change to the stand-in is needed.
- The cache server reads the run's ref, default branch and repository from the runtime token, which `runner.server` signs with `token.pem`. The cache server verifies it with `token.pub.der`. Keep `$STATE/keys/` outside the checkout.
- The cache server serves one repository on a trusted local network. Pull request registration has no authentication and is keyed by number only; the first registration of a number holds, so a job cannot change its own base later. It listens on `127.0.0.1:80` unless `--listen` says otherwise; inside the container `--listen 0.0.0.0:80` exposes it on the environment's network, and `up` publishes it only on `127.0.0.1`. Every job of an environment can reach the cache server and the API stub, so replay only code you trust; scenarios have no fork pull requests.
- A pull request run also reads its base branch, which the token does not carry. Until it is registered, the cache server refuses that run's cache calls with a JSON error, and the job continues without a cache.
- curl resolves every `*.localhost` name to the loopback address itself, so a curl in a container calls the cache server by its container name.
- `Runner.Client` talks to `runner-server` directly. Through a proxy that buffers the streamed `schedule2` answer, it times out after 100 seconds.
- `runner.server` replaces `actions/checkout` with its own local checkout of the working tree that `Runner.Client` sends.

## Test data

`testdata/cache-client/` holds the calls of `@actions/cache` 6.1.0 (`actions/cache` commit `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`) on a miss with save, then on a hit. Requests come from the real client. Responses come from `runner.server`, not from GitHub. Bearer tokens and URL signatures are replaced with `REDACTED`, and binary bodies with their size.

`testdata/runner-server/` holds `runner.server`'s answers for one local run of the lab's `mbx default` workflow: the run list (`/_apis/v1/Message/workflow/runs`) and the job's timeline (`/_apis/v1/Timeline/{id}`).

## Conformance

`conformance/<probe>.json` holds how real GitHub behaved, measured by the probes of [tailrocks/velnor-lab](https://github.com/tailrocks/velnor-lab) (`lab probes`) and checked there every week. One file per probe, one entry per leg, with tags, run ids and pull request numbers replaced by placeholders. `token`, `cache-save-existing`, `cache-restore-keys`, `cache-pr-reads`, `cache-other-pr` and `cache-own-pr` are replayed through `velnor-cache-rules` by `tests/it/conformance.rs`. The pushes and pull requests of `velnor-local run` carry every field of the `push` and `pull_request` legs of `event-payload`, with the observed types, checked by `crates/velnor-local/tests/it/driver.rs`. `needs-result` and the other legs of `event-payload` have no reader yet. Copy new files from the lab's `conformance/` when `lab probes` changes them.

## Cache rules and cache server

`crates/velnor-cache-rules/` answers "what would GitHub's cache do?" for any sequence of events: which scopes a run reads and writes, which entry a restore finds, whether a save is refused, and which entries GitHub evicts. It takes data and returns data. `crates/velnor-cache-rules/RULES.md` lists each rule with its source in GitHub's documentation, or marks it as modelled or probed.

`crates/velnor-local/` holds `velnor-local cache-server`. It serves `GetCacheEntryDownloadURL`, `CreateCacheEntry` and `FinalizeCacheEntryUpload`, the Azure-style uploads and the downloads, and asks `velnor-cache-rules` for every hit, miss, refusal and eviction. Each archive is one file in `archives/`, and `index.json` lists the entries and the registered pull requests. Its tests replay `testdata/cache-client/` in process.

Run the tests from `local/`:

```sh
cargo nextest run --locked
```

## API stub

`velnor-local api-stub` answers the GitHub API calls `velnor-ci` makes, and nothing else. Any other path gets 501 with a JSON body that names it. An issue that makes the helper call a new endpoint adds it to the stub in the same change.

| Endpoint | Source |
|---|---|
| `GET /repos/{owner}/{repo}/pulls/{n}/files` | `git diff base...head` in `--repo`, the shas from `pulls.json`; at most 3,000 files |
| `GET /repos/{owner}/{repo}/compare/{base}...{head}` | `--repo`; at most 300 files, on the first page |
| `GET /repos/{owner}/{repo}/contents/{path}?ref=` | `--repo` |
| `GET /repos/{owner}/{repo}/pulls?state=open` | `pulls.json` in `--dir`, a JSON array of pull request objects with `number`, `state`, `base.sha` and `head.sha` |
| `GET /repos/{owner}/{repo}/actions/caches`, `DELETE .../caches/{id}` | `index.json` in `--cache-dir`, the cache server's directory |

```sh
velnor-local api-stub --listen 127.0.0.1:8080 --repo "$REPO" --dir "$STATE/api" --cache-dir "$STATE/cache" --default-branch main
```

`--default-branch` is the branch a contents request without `ref` reads. A missing or malformed `pulls.json` is a 500. A diff larger than `--diff-limit-bytes` (256 MiB by default) is a 422. The stub has no authentication, like the cache server's pull request registration: it serves one repository to a trusted local network and listens on `127.0.0.1` unless `--listen` says otherwise.

The stub serves one repository: `{owner}/{repo}` selects the cache entries and appears in URLs, and the git repository is always `--repo`. Both processes take an exclusive lock on `index.lock` in the cache directory around every read-modify-write of `index.json`, and the cache server reads `index.json` again under that lock on every request, so a delete through the stub takes effect at once and is never overwritten. A request fails with a JSON error when the index cannot be read.
