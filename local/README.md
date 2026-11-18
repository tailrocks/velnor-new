# velnor-local

The local GitHub environment, a development tool that is not shipped. It runs GitHub's official runner, unmodified, against `runner.server` on an Apple Silicon Mac, in OrbStack containers, with a cache that follows GitHub's cache rules.

| Part | Pin |
|---|---|
| Stand-in | `ChristopherHX/runner.server` commit `992ccbbbf9afcde477c38c316e053b1af457ad40`, built by `docker/runner-server/Dockerfile`. One change: `DownloadArtifact` answers `Content-Type: application/zip`, without which `actions/download-artifact` saves the zip as a raw file |
| Runner | `actions/runner` 2.338.0, `actions-runner-linux-arm64-2.338.0.tar.gz`, sha256 `628b4a7258487b80c1d3c221095a7ded349f5ae0175fd3dfab708d07428f041b`, built by `docker/runner/Dockerfile` |
| Cache server | `velnor-local cache-server` from this workspace, built by `docker/velnor-local/Dockerfile`. It owns the name `github.localhost`, answers the cache calls and forwards every other request to the stand-in unchanged |
| Recorder | `mitmproxy/mitmproxy:12.1.2@sha256:0d7b2a14b4a71a908cd6f31b8f50e29bbc5c8720fe6caa706acf782da558e5c7`, used only to record runner calls: give it the alias `github.localhost` and `mitmdump --mode reverse:http://velnor-cache-server:80 --listen-port 80 --set keep_host_header=true`, and start the cache server without that alias |

## From zero to a run

You need an Apple Silicon Mac with OrbStack (or Docker Desktop), a Rust toolchain, and `git`. Everything below runs on the machine and spends no money. `$VELNOR` is a checkout of this repository, `$LAB` a checkout of the repository whose workflows you want to run, and `$STATE` a directory outside both, without spaces.

```sh
export VELNOR="$HOME/src/velnor-new" LAB="$HOME/src/velnor-lab"
git clone https://github.com/tailrocks/velnor-new "$VELNOR" && cd "$VELNOR"
git clone https://github.com/tailrocks/velnor-lab "$LAB"
export STATE="$HOME/.velnor/warm"

docker build -t velnor-local/runner-server local/docker/runner-server
docker build -t velnor-local/runner local/docker/runner
docker build -t velnor-local/cache-server -f local/docker/velnor-local/Dockerfile local
cargo build --release --manifest-path local/Cargo.toml
cargo build --release --manifest-path ci-helper/Cargo.toml -p velnor-ci
export PATH="$VELNOR/local/target/release:$VELNOR/ci-helper/target/release:$PATH"

velnor-local doctor
velnor-local build
velnor-local doctor
velnor-local run warm --repo "$LAB" --dir "$STATE" --workflow .github/workflows/mbx-default.yml --repository tailrocks/velnor-lab --keep
```

`doctor` checks four parts and prints `ok` or `miss` for each (images read `n/a` when Docker is unreachable, because it cannot tell), with one `fix:` line for every part that is missing. It exits 1 when any part is `miss`, and 0 when all are `ok`:

| Part | `ok` when | `fix:` |
|---|---|---|
| docker | `docker info` answers and the daemon is linux/arm64; it names OrbStack or Docker and the socket | start OrbStack or Docker Desktop |
| stand-in, cache server and plain runner images | `velnor-local/runner-server`, `velnor-local/runner` and `velnor-local/cache-server` exist | the `docker build` line of each missing image |
| pair images | `velnor-local/pair-runner`, `velnor-local/pair-dind` and `velnor-local/pair-probe` exist | `velnor-local build` |
| velnor-host with local mode | the `velnor-host` that `build` left in the checkout answers `local --help`, so it was built with `local-mode` | `velnor-local build` |

`doctor --velnor-host <path>` checks another binary. The first `doctor` above prints `miss` for the pair images and for `velnor-host`; after `velnor-local build` the second prints `ok` four times. `velnor-ci` writes the metrics file of each run; `run` finds it on `PATH`, or `--velnor-ci <path>` names it. `--keep` leaves the environment up after the verdict, so `status` has something to show. `run` prints `success` for each run and, in pair mode, `pair trusted-N, admitted` under each job; the verdict ends with the executed, stood in and not exercised lines.

To look at a running environment, ask for its status from another terminal while `run --keep` or `up` has it up:

```sh
velnor-local status --dir "$STATE"
velnor-local down --dir "$STATE"
```

`status` prints the environment's name and age, the runner mode, the authority's address, the number of events in the ledger, each pair as `busy (run <id>, job <name>)` or `idle`, and the last refusal of the job-start check, or `none`. It reads the state directory only and changes nothing; when the authority is not running it exits 1 and says so. `down` removes the environment.

## Run an unreleased velnor-ci

`setup-velnor` downloads the pinned release of `velnor-ci` and installs it only when its SHA-256 matches the workflow's pin, so a helper built from a working tree can never match. `--velnor-ci-binary <path>` on `up` and `run` (not `--velnor-ci`, the host program that writes the metrics file, which on a Mac is not a Linux executable) hands the runners a linux-arm64 ELF build of `velnor-ci` instead, and the workflow stays as generated, pins and all:

```sh
velnor-local run warm --repo "$REPO" --dir "$STATE" --runner plain --velnor-ci-binary "$VELNOR_CI_LINUX_ARM64"
```

`up` refuses a path that is not an executable regular file of at most 256 MiB, is not a linux-arm64 ELF, or has a `:`, `,` or newline in it, and refuses the flag with `--runner pair`, because velnor-host builds the pair containers and mounts nothing from the host into them. It mounts the file read-only at `/opt/velnor/local/velnor-ci`, a marker file from the state directory at `/opt/velnor/local/.local-mode`, and sets `VELNOR_LOCAL_MODE=1`, in the trusted runners only; the untrusted side gets none of the three.

The action takes the local binary only when `VELNOR_LOCAL_MODE` is `1` and `/proc/self/mountinfo` lists both paths as read-only mount points; a directory a job created, a writable mount or a step's `env:` cannot satisfy that. Otherwise it warns and verifies the pinned release exactly as on GitHub. The SHA-256 inputs stay required. The binary is never written to the tool cache, and every use prints a warning and a step-summary line that name the local SHA-256, so a local result is not mistaken for a release result. The binary is unverified by design: this is for a developer's own machine, and the check proves nothing about the helper that was mounted.

## The three modes

| | GitHub-hosted | Self-hosted scale set | Local (this tool) |
|---|---|---|---|
| Jobs come from | GitHub | GitHub's scale set service | `runner.server`, fed by the `velnor-local` driver |
| Job containers | GitHub's | `velnor-host` pairs of dind and runner | `--runner pair` (the default): `velnor-host local` pairs; `--runner plain`: the official runner image |
| The job-start check asks | nobody | `https://api.github.com` | the `velnor-local` authority on `127.0.0.1`; only in builds with the `local-mode` feature |
| Cleanup | GitHub | the journal, then GitHub's admin API | containers, volumes, networks and agents removed by label and by name; removals recorded in `cleanup.jsonl` |

`pair` runs your jobs the way the self-hosted runner does, behind the same job-start check; `plain` runs the official runner image with no check and is for a machine that cannot build the pair images (`doctor` says when `pair` cannot run and which command fixes it). The verdict names the mode and says what ran for real, what was stood in and what was not exercised.

## Safety rules

- The authority listens on `127.0.0.1` only, and every call needs its token, which each `up` replaces. The cache and API stub control ports answer only with their own token.
- `velnor-host` with local mode exists only in builds made by `velnor-local build`. A release build refuses the `local-mode` feature, so production never carries the local check.
- Local mode keeps no reservation. Its cleanup audit is separate from the minted-agent recovery state in `runner/minted-agents.log`. `down` removes everything labelled `velnor-local=<name>`; after it, `docker ps -aq`, `docker volume ls -q` and `docker network ls -q` with that label print nothing.
- `down` appends its exact label filter and each removal's intent and confirmed outcome to `$STATE/cleanup.jsonl`. An intent without an outcome means completion was not confirmed. The host sweep writes the same records to `$STATE/runner/cleanup.jsonl`. Successful removals remain recorded when a later removal fails. Cleanup repairs an incomplete final record before appending, preserves complete records, and refuses deletion if a complete record is corrupt. Volumes are removed only through the label filter, without implicit anonymous-volume deletion.
- Untrusted runs (fork pull requests) use a second network and runner.server, a cache that only restores, and no secrets or token. That network is an ordinary Docker bridge; replay fork pull requests only where that is acceptable.
- `STATE` is outside the checkout. `run` copies the checkout and never modifies it.
- `build` refuses a working tree with uncommitted changes, because the images are named for `HEAD`.

## The approved-workflows check and `--policy`

The approved-workflows check is off by default in local mode: jobs run the checkout's workflows as they are, there is no prompt, and `up` and every `run` summary print a warning that says so. Treat a passing run without `--policy` as proof that the workflow runs, not that the real runner would approve it.

`up` and `run` take an optional `--policy <file>` in pair mode (`--runner plain` has no job-start check and refuses the flag). It turns the check on: the warning then says the check is on, and a job the policy does not approve is refused at job start with reason `policy`, which the verdict prints as `REFUSED at job start: policy` and which makes `run` exit non-zero.

The file is the approved job-authority policy JSON that `velnor-host connect --job-authority-policy` reads, parsed by the runner's own `ApprovedJobAuthorityPolicy` type, and it must be at most 256 KiB. The runner's structural validation runs on load, so a policy whose entries sit under another workflow, whose digest is stale or whose call graph is inconsistent is refused at startup; only the connection binding and the connection's source policy are not checked. `up` copies it to `runner/policy.json` (0600); `velnor-host local` reads it before it starts a pair and does not start when it is missing, unreadable or does not parse, so `up` fails. After the ledger admits a job, the decider approves it only when the policy:

- lists the job's root workflow path for the repository id the run reports, with the sha256 of the workflow file as committed at the run's commit (the authority hashes `git show <sha>:<workflow>`, and reports no hash, so the job is refused, when the workflow file in the checkout differs from that blob, so what runner.server executes is what was hashed), and that workflow calls no other workflow;
- has a job entry for that workflow and job key whose context matches the event and ref (`push`, `schedule` and the like by ref; `pull_request` by a rule whose number, base repository, base branch and head branch match).

A pull request from a fork is never approved, because its head repository is not the base repository; a `merge_group` job is never approved. The `run` command takes `--repository-id <n>`, the GitHub id the policy names, and events then carry it instead of an id derived from `--repository`; a policy that names another id approves nothing. The local check does not verify the policy's binding to a repository connection, and does not compare an entry's permissions, secrets, environments, needs or concurrency with the workflow; it only ever refuses more than the ledger check alone.

## Environments, pairs and the authority

`velnor-local build` builds the linux/arm64 images that pair mode runs, from the recipe in `images/runner/ubuntu-26.04/README.md`: the production runner image as `velnor-local/pair-runner`, the DinD image as `velnor-local/pair-dind` and the resource probe as `velnor-local/pair-probe`. Run it from the repository root, or pass `--root`; it prints each tag with its image id. The helper programs are compiled in a `rust` container into the volume `velnor-arm64-target`, which a later build reuses. It then builds `crates/velnor-runner/target/release/velnor-host` with `--features=local-mode` and without the release identity variables, because a release build refuses local mode.

`up` and `run` take `--runner pair|plain`. `pair`, the default, runs every job in a velnor-host dind and runner pair: the authority process (`supervise`) starts `velnor-host --state "$STATE/runner" local` (`--velnor-host` names another binary; the default is the `velnor-host` that `velnor-local build` left in the checkout that holds the `velnor-local` executable, checked to be a regular executable file with the executable's owner and no group or other write bit, and never one found through the working directory) and writes its plan to its stdin and to `runner/plan.json` (0600): the authority's address and token, the Docker socket, the ids of `velnor-local/pair-runner` and `velnor-local/pair-dind`, and `--runners` trusted and `--fork-runners` untrusted pairs on the environment's networks. `up` waits until every pair prints `pair <side>-<n> listening` in `runner/velnor-host.log`, then until runner.server reports every pair's agent online (up to 60 s); it fails, and stops the authority and velnor-host it started, if an agent is still offline after that. velnor-host checks each job at start against the authority and appends the decision to `runner/decisions.jsonl`; the verdict prints it under the job's run as `pair trusted-N, admitted (event <ledger id>)` or `REFUSED at job start: <reason>`, and a refused job makes `run` exit non-zero: the driver sees the refusal in `decisions.jsonl`, cancels the runs of that event at runner.server (a killed job never reports back) and goes on to the verdict. `plain` runs the official runner image in containers with no job-start check, and its verdict says `job-start check: none (plain containers)`. `down` stops the authority, which stops velnor-host (it removes its pairs and deletes their agents), kills a velnor-host that outlived it, and then removes every container, volume and network labelled `velnor-local=<name>`.

`velnor-local up --dir "$STATE" --repo "$REPO" [--runners 2]` starts one environment and `velnor-local down --dir "$STATE"` removes its containers and network. `STATE` is a directory outside the checkout, without spaces. It holds `keys/` (the runtime token key pair, made once with `openssl` in the stand-in image), `cache/` (the cache server's archives and `index.json`) and `api/` (the API stub's `pulls.json`). The environment is named `velnor-local-<hash of STATE>`: a network of that name, the containers `<name>-standin`, `<name>-cache`, `<name>-api` and, with `--runner plain`, `<name>-runner-<n>`, all labelled `velnor-local=<name>`. Two state directories are two independent environments. Only the cache server and the API stub publish a port, each a control port on a free port of `127.0.0.1` that answers only requests carrying `Authorization: Bearer <the token in keys/control>` (made once per state directory) and refuses the rest with 401, because a container can reach the host's loopback ports; `up` prints the stub's address. With `--runner plain`, `up` returns when every runner has a session with runner.server.

`up` also starts the local authority, a `velnor-local supervise --dir "$STATE"` process of the host, and prints `authority on 127.0.0.1:<port>`. It listens on `127.0.0.1` only (there is no address option) and every call needs `Authorization: Bearer <the token in keys/authority>`, a fresh 32-byte token that each `up` replaces and that is not the control token; without it the answer is 401. It serves `POST /ledger`, `POST /jit` with `{"side": "trusted"|"untrusted", "name": "<agent name>"}` (the name is required, a request without it is a 400), `GET /runs/<side>/<run id>/<attempt>` and `DELETE /agents/<side>/<name>`. The driver posts `{id, side, event, ref, sha, workflow}` to `/ledger` before each `Runner.Client` call, and the authority appends it to `authority/ledger.jsonl` with the ids of the runs the side's runner.server already had. `GET /runs` answers 404 when that side's runner.server has no such run or attempt, and otherwise returns the run's event, ref, sha, workflow and job keys with `"ledger"`, the id of the entry that dispatched it, or `null` and a `"refusal"`: `not_dispatched` when no entry matches event, ref, sha and workflow on that side before the run was created, which includes a run that a job scheduled through runner.server, and `wrong_side` when only an entry of the other side matches. `down` stops the authority; `run --keep` leaves the environment, the authority included, up after the verdict.

Untrusted runs have a side of their own. With `--fork-runners <n>` (which `run` sets to one when a scenario opens a pull request from a fork), `up` also starts a second network `<name>-fork` with its own runner.server (`<name>-fork-standin`), cache server (`<name>-fork-cache`, same cache directory, started with `--restore-only`, so it refuses every cache write, as GitHub lets a fork pull request only read caches, and serves restores only to tokens for `refs/pull/<n>/merge` of a fork pull request the driver registered with it; a job that has the untrusted runner.server schedule a run on another ref gets no cache). A fork job therefore restores, read-only, from the scope of its own `refs/pull/<n>/merge`, its base branch and the default branch, as GitHub's rule for a pull request allows, and writes to none of them and runners (`<name>-fork-runner-<n>`). Both sides register the same runner labels; each runner registers with the runner.server of its own network only, so a runner of one side never takes a job of the other. That network has no `api-stub` and no API stub container, so a job there gets `GITHUB_API_URL=http://api-stub` like every job but cannot resolve or reach it. The untrusted runner.server signs its runtime tokens with its own key pair in `keys/fork/`, so the trusted cache server would refuse them. A fork pull request's run gets no metrics file and no start time, because the stub, which `velnor-ci bench collect` reads, cannot see its stand-in; the verdict says so, and its first-job time counts trusted runs only.

A job of either side gets no secrets and no token: `run` passes none to `Runner.Client`, so `secrets` is `{}` and `github.token` is empty, where GitHub gives a fork pull request a read-only token.

What the untrusted side does not close: its network is an ordinary Docker bridge, so a fork job still reaches what any container on this machine reaches, the host's loopback ports through `host.docker.internal` (the published ports answer only with the control token), other services on the host and the LAN, cloud metadata addresses on a cloud machine, and the internet. Replay fork pull requests only where that is acceptable.

Only `pull_request` is modelled for forks. `pull_request_target`, `workflow_run` and `issue_comment` are never sent, so a workflow that runs on them shows no run, which says nothing about whether it would be safe on GitHub.

Every runner container is linux/arm64 and registers with the labels `ubuntu-latest`, `ubuntu-22.04`, `ubuntu-24.04`, `ubuntu-26.04` and their `-arm` forms, so a workflow written for hosted runners runs unchanged and `runner.arch` is `ARM64`. Every one of these labels runs on the same image, Ubuntu 24.04 in plain mode and the self-hosted Ubuntu 26.04 pair image in pair mode, so a difference between Ubuntu releases does not show locally and a job can pass in one mode and fail in the other; the verdict says so. The runner image has `build-essential`, as hosted Ubuntu runners do.

## Run a scenario

```sh
velnor-local run warm --repo "$REPO" --dir "$STATE" --workflow .github/workflows/ci.yml --repository owner/name
```

`run` takes several scenarios: `velnor-local run docs leaf --parallel 2 --dir "$STATE" ...` runs each in its own environment under `$STATE/<scenario>`, at most `--parallel` at a time (one unless set), and prefixes every line, the verdict's included, with the scenario. Ctrl-C stops the running scenarios and keeps the queued ones from starting. Each environment has its own network, containers, ports, keys, cache and copy of the checkout.

A scenario is a JSON file in `scenarios/` (`--scenarios` names another directory; a path ending in `.json` is read as is): a description and an ordered list of steps. `warm`, `cold`, `docs`, `leaf`, `hub` and `lock` replay the scenarios of the lab, `fork` opens a pull request from the repository and one from a fork (with `testdata/workflows/fork-reach.yml` copied into the checkout, the first job reaches the API stub and the second cannot), `scheduled-run` (with `testdata/workflows/schedule-run.yml`) has a job post a second run to runner.server, whose job is refused as `not_dispatched`, `hello` pushes once (with `testdata/workflows/hello.yml`, for `--policy`), and `story` merges a pull request, opens a pull request, pushes to it again, opens a second one and closes the first.

| Step | What the driver does |
|---|---|
| `{"push": "<branch>"}` | Sends a `push` event with `ref`, `before` and `after`. `before` is the branch's previous push in this run; for a branch that existed before the scenario it is the parent of its tip, and for a branch the scenario created it is forty zeros. Every open pull request with this branch as its head also gets `pull_request` `synchronize` with its new merge commit, unless the merge conflicts, which the driver reports and GitHub also answers with no run |
| `{"commit": {"branch": "<b>", "append": "<path>", "line": "<text>"}}` | Appends the line to the file, which may be new, on the branch, created from the default branch when missing, and commits it |
| `{"update_lock": {"branch": "<b>", "package": "<crate>", "workspace": "<dir>"}}` | Bumps the crate to the next patch version after the one in the `Cargo.lock` of the workspace (the root of the copy unless `workspace` names a directory in it) with `cargo update -p <crate>@<locked version> --precise <version>`, run in that workspace so its `.cargo/config` applies, and commits the lock file on the branch. Cargo reads the crates.io index for this: it is the one request of the driver itself that leaves the machine; the others are the jobs' downloads of actions and tools |
| `{"open_pull_request": {"head": "<b>", "base": "<b>", "fork": false}}` | Merges head into base as `refs/pull/<n>/merge` would, writes the pull request to `pulls.json`, registers its base with the cache server and sends `pull_request` `opened` with `--ref refs/pull/<n>/merge`. With `"fork": true` the head repository is `velnor-local-fork/<name>` with `fork: true`, and this and every later event of that pull request go to the untrusted side. A `push` of that branch sends no `push` event, as GitHub sends none to the repository for a fork's branch, only `synchronize` to the pull request. `run` refuses a scenario that dispatches that branch, makes it a base, opens a pull request from the repository with it, or names the default branch as a fork's head. The `push` of the base that `merge_pull_request` sends and the `merge_group` event of a fork's pull request run on the trusted side, as on GitHub, where a maintainer approves the merge; the `pull_request` `closed` event itself stays on the untrusted side, as GitHub runs it for a fork |
| `{"merge_pull_request": "<head>"}` | Squash-merges the open pull request with that head into its base, sends `pull_request` `closed` with `merged: true`, then pushes the base |
| `{"close_pull_request": "<head>"}` | Sends `pull_request` `closed` for the open pull request with that head |
| `{"merge_group": "<head>"}` | Merges the pull request into its base on `gh-readonly-queue/<base>/pr-<n>-<base sha>` and sends `merge_group` `checks_requested` for that branch, as GitHub's merge queue does for a queue with one pull request; a group never includes pull requests queued before it |
| `{"schedule": "<cron>"}` | Sends `schedule` on the default branch |
| `{"workflow_dispatch": {"branch": "<b>", "inputs": {"<name>": <JSON value>}}}` | Sends `workflow_dispatch` on the branch with the inputs, to the one workflow `--workflow` names, which this step needs |

With several scenarios, `$STATE` in this paragraph stands for `$STATE/<scenario>`. `run` copies `REPO` to `$STATE/repo`, commits its uncommitted changes there (changes to tracked files and the untracked files that `.gitignore` does not exclude; an untracked link is copied as a link only when it points inside the checkout, otherwise `run` stops and names it; a nested git repository is left out with a message, as `git add` leaves it out), and makes that commit the default branch of the copy (`--default-branch`, `main` unless set), so the scenario starts from exactly what is on disk, whichever branch `REPO` has checked out, and `REPO` is never modified. A step names the default branch as `{default}`. It starts the environment from an empty cache, replays the steps, stops the environment on every exit (a failed `up` and Ctrl-C included), and prints a verdict: each run with its workflow, event, conclusion and metrics file, and under it, per job, each cache key it looked up with the entry it restored and that entry's scope (the ref it was saved on), or that it found none, the seconds from the start of `up` to the first job, and what was executed and what was stood in. Event payloads have the fields that `conformance/event-payload.json` lists. Each event's payload is in `$STATE/events/` and the log of its runs in `$STATE/logs/`. After each run, `run` calls `velnor-ci bench collect <run id> --repo <repository> -o $STATE/metrics/<run id>.json` (`--velnor-ci` names the program) with `GITHUB_API_URL` set to the API stub, so the metrics file of a local run is written by the same code as for a real run, and `velnor-ci bench compare` reads both. `--workflow` limits the runs to one workflow file; without it, runner.server runs every workflow the event triggers. An event that triggers no workflow is reported as such and is not a failure. `Runner.Client` gets `--no-default-payload`, so an event carries exactly the payload the driver wrote and no `sender` of its own, and a `push` lists the files each new commit adds, removes and modifies, which a `paths` filter reads. `run` exits with an error unless every run succeeded and wrote its metrics file. It refuses a branch, path, workflow, repository or default branch that is empty or starts with `-`, and an appended path that is absolute, contains `..` or leads out of the copy through a link. It fails when `Runner.Client` exits with an error before it creates a run, and when the old `cache/`, `api/`, `events/` or `logs/` cannot be removed.

## Why each setting

- `Runner.Server__GitApiServerUrl=http://api-stub` becomes `GITHUB_API_URL` in every job, so the helper's API calls reach the stub. Action downloads keep their own URLs (`Runner.Server:ActionDownloadUrls`).
- `Runner.Server__GitServerUrl=http://github.localhost` becomes `GITHUB_SERVER_URL` in every job. `@actions/cache` uses the v2 cache protocol only when that host is `github.com`, ends in `.ghe.com` or ends in `.localhost`. For any other host, such as plain `localhost`, it assumes GitHub Enterprise Server and uses v1. `runner.server` sets the job variable `actions_uses_cache_service_v2=true` itself.
- The stand-in image patches runner.server: `refs/pull/<n>/merge` gets `ref_name` `<n>/merge` and `ref_type` `branch`, as on GitHub, and a `workflow` string in a payload (as `schedule` and `workflow_dispatch` carry) is ignored, where runner.server would answer 500, while a `workflow` object (the upstream workflow of `workflow_run`) is still read for `on.workflow_run.workflows`.
- `GITHUB_ACTIONS_RUNNER_FORCE_GHES=1` makes `config.sh` register through `/api/v3/actions/runner-registration`, which `runner.server` serves. Without it, the runner treats `github.localhost` as github.com and calls `api.github.localhost`, which does not exist.
- `runner.server` builds the job's cache address from the host the runner called, so the cache server receives the cache calls when it owns `github.localhost`. No change to the stand-in is needed.
- The cache server reads the run's ref, default branch and repository from the runtime token, which `runner.server` signs with `token.pem`. The cache server verifies it with `token.pub.der`. Keep `$STATE/keys/` outside the checkout.
- The cache server serves one repository on a trusted local network. Pull request registration is keyed by number only, and the first registration of a number holds. With a control port, as `up` starts it, registration is served only there, with the control token, and the open port answers 403, so no job can register a base. It listens on `127.0.0.1:80` unless `--listen` says otherwise; inside the container `--listen 0.0.0.0:80` exposes it on the environment's network, and `up` publishes it only on `127.0.0.1`. Every job of the trusted side can reach the cache server and the API stub. A pull request from a fork runs on the untrusted side, which cannot reach the stub (see the `fork` flag of `open_pull_request`).
- A pull request run also reads its base branch, which the token does not carry. Until it is registered, the cache server refuses that run's cache calls with a JSON error, and the job continues without a cache.
- curl resolves every `*.localhost` name to the loopback address itself, so a curl in a container calls the cache server by its container name.
- `Runner.Client` talks to `runner-server` directly. Through a proxy that buffers the streamed `schedule2` answer, it times out after 100 seconds.
- `runner.server` replaces `actions/checkout` with its own local checkout of the working tree that `Runner.Client` sends.

## Test data

`testdata/cache-client/` holds the calls of `@actions/cache` 6.1.0 (`actions/cache` commit `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`) on a miss with save, then on a hit. Requests come from the real client. Responses come from `runner.server`, not from GitHub. Bearer tokens and URL signatures are replaced with `REDACTED`, and binary bodies with their size.

`testdata/runner-server/` holds `runner.server`'s answers for one local run of the lab's `mbx default` workflow: the run list (`/_apis/v1/Message/workflow/runs`), the run's jobs (`/_apis/v1/Message?runid=`) and the job's timeline (`/_apis/v1/Timeline/{id}`).

## Conformance

`conformance/<probe>.json` holds how real GitHub behaved, measured by the probes of [tailrocks/velnor-lab](https://github.com/tailrocks/velnor-lab) (`lab probes`) and checked there every week. One file per probe, one entry per leg, with tags, run ids and pull request numbers replaced by placeholders. `token`, `cache-save-existing`, `cache-restore-keys`, `cache-pr-reads`, `cache-other-pr` and `cache-own-pr` are replayed through `velnor-cache-rules` by `tests/it/conformance.rs`. The pushes and pull requests of `velnor-local run` carry every field of the `push` and `pull_request` legs of `event-payload`, with the observed types, checked by `crates/velnor-local/tests/it/driver.rs`. Copy new files from the lab's `conformance/` when `lab probes` changes them.

Every probe also runs in the local environment. `scenarios/probe-<probe>.json` replays what `lab probes` does on GitHub for that probe (the same pushes, pull requests and dispatches, with a numeric tag that is not a pull request number), and `velnor-local conformance --dir "$STATE"` reads the observation each probe job prints into the run's logs and compares every leg with `conformance/<probe>.json`:

```sh
velnor-local run probe-cache-pr-reads --repo "$LAB" --dir "$STATE" --workflow .github/workflows/probe-cache-pr-reads.yml --repository tailrocks/velnor-lab
velnor-local conformance --dir "$STATE"
```

It prints each leg as `reproduced`, with its differences, or as `not observed`, and exits with an error unless every leg is reproduced. The legs of `event-payload`, `cache-restore-keys`, `cache-pr-reads`, `cache-other-pr` and `cache-own-pr` are reproduced. The rest differ, each for a reason listed in #632 (the lab-side fixes are tailrocks/velnor-lab#289):

- `cache-save-existing` reads the job log from `https://api.github.com`, not from `GITHUB_API_URL`; with the probe reading `GITHUB_API_URL` it is reproduced.
- `needs-result` cancels its run with `gh run cancel`, which the runner image does not have and which would call github.com. Cancellation is not modelled: a run that cancels itself is not cancelled locally, so `if: cancelled()` and `needs.<job>.result == 'cancelled'` behave differently from GitHub.
- `token` reads GitHub's own claims, such as the cache scopes in `ac`; runner.server signs other claims (`ref`, `defaultRef`, `repository`), which the cache server reads, and the cache scopes they lead to are what `cache-pr-reads`, `cache-other-pr` and `cache-own-pr` check.

## Cache rules and cache server

`crates/velnor-cache-rules/` answers "what would GitHub's cache do?" for any sequence of events: which scopes a run reads and writes, which entry a restore finds, whether a save is refused, and which entries GitHub evicts. It takes data and returns data. `crates/velnor-cache-rules/RULES.md` lists each rule with its source in GitHub's documentation, or marks it as modelled or probed.

`crates/velnor-local/` holds `velnor-local cache-server`. It serves `GetCacheEntryDownloadURL`, `CreateCacheEntry` and `FinalizeCacheEntryUpload`, the Azure-style uploads and the downloads, and asks `velnor-cache-rules` for every hit, miss, refusal and eviction. Each archive is one file in `archives/`, and `index.json` lists the entries and the registered pull requests. `restores.jsonl` gets one line per `GetCacheEntryDownloadURL`: the runtime token's `scp` (`Actions.Results:<run id>:<job id>`), the key, and the matched key and its scope, or `null` for a miss. `run` reads it for the verdict. The journal records lookups, so an `actions/cache/restore` with `lookup-only: true` also reads as restored, although it downloads nothing. Its tests replay `testdata/cache-client/` in process.

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
| `GET /repos/{owner}/{repo}/actions/caches`, `DELETE .../caches/{id}` | Control port only, using `index.json` in `--cache-dir`, the cache server's directory |
| `GET /repos/{owner}/{repo}/actions/runs/{id}`, `.../runs/{id}/attempts/{n}/jobs`, `.../actions/jobs/{id}/logs` | runner.server at `--standin`: its run, its jobs with their timeline records as steps, and the step logs in order. Times are cut to whole seconds, as GitHub reports them. runner.server records no queue time, so a job's `created_at` equals its `started_at` and a local metrics file always has `queue_seconds` 0 |

```sh
velnor-local api-stub --listen 127.0.0.1:8080 --repo "$REPO" --dir "$STATE/api" --cache-dir "$STATE/cache" --default-branch main
```

`--default-branch` is the branch a contents request without `ref` reads. A missing or malformed `pulls.json` is a 500. A diff larger than `--diff-limit-bytes` (256 MiB by default) is a 422. The stub serves one repository to a trusted local network and listens on `127.0.0.1` unless `--listen` says otherwise.

Cache listing and deletion require `--control-listen` and `--control-token`. The control port requires `Authorization: Bearer <the token in the control-token file>` and returns 401 for a missing or wrong token. The job port refuses cache listing and deletion with 403, even with a valid control token or without a control listener. Other API endpoints remain available on the job port without authentication.

The stub serves one repository: `{owner}/{repo}` selects the cache entries and appears in URLs, and the git repository is always `--repo`. Both processes take an exclusive lock on `index.lock` in the cache directory around every read-modify-write of `index.json`, and the cache server reads `index.json` again under that lock on every request, so a delete through the stub takes effect at once and is never overwritten. A request fails with a JSON error when the index cannot be read.

## Docker integration tests

The runner's `local_pair_public` and `local_host_public` tests skip Docker work with a message unless `VELNOR_TEST_DOCKER_HOST` explicitly selects an isolated daemon. The local `it` binary's `removal_audit::` tests fail without the same explicit endpoint. These tests ignore `DOCKER_HOST` and the default socket. An empty, malformed or unreachable explicit endpoint fails. Daemon-free checks still run.

From the repository root, this command starts a uniquely named disposable daemon on a dynamically assigned loopback port, waits for readiness, runs both suites and removes that exact daemon and its anonymous volumes on exit. It requires a Docker host that permits privileged containers.

```sh
( set -eu; name="velnor-test-dind-$(uuidgen | tr '[:upper:]' '[:lower:]')"; docker run -d --privileged --name "$name" -e DOCKER_TLS_CERTDIR= -p 127.0.0.1::2375 docker:29-dind >/dev/null; trap 'docker rm -fv "$name" >/dev/null' EXIT; endpoint="tcp://$(docker port "$name" 2375/tcp)"; attempts=0; until docker -H "$endpoint" info >/dev/null 2>&1; do attempts=$((attempts + 1)); [ "$attempts" -lt 60 ]; sleep 1; done; VELNOR_TEST_DOCKER_HOST="$endpoint" cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked --features local-mode -p velnor-runner-host --test local_pair_public --test local_host_public --no-tests fail; VELNOR_TEST_DOCKER_HOST="$endpoint" cargo nextest run --manifest-path local/Cargo.toml --locked -p velnor-local --test it -E 'test(removal_audit::)' --no-tests fail )
```

The CI profile opts into `unix:///var/run/docker.sock` for these two binaries and the local `removal_audit::` tests only on Linux when `GITHUB_ACTIONS=true` and `VELNOR_TEST_DOCKER_HOST` is unset. It preserves an explicit endpoint, including an empty value that fails validation. A local `--profile ci` run still needs an explicit endpoint.
