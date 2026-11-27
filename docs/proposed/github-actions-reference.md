# GitHub Actions Official Reference & Compatibility Sources

## Purpose

This document establishes the authoritative sources of truth for GitHub Actions protocols, runner execution, scale set listeners, and runner environment images. For artifact upload semantics, see the [pinned action source reference](github-actions-artifact-reference.md). For installation token transport, see the [official token compatibility reference](github-installation-token-reference.md).

When designing workflows in the Velnor V1 generator or implementing runner infrastructure in `crates/velnor-runner-*`, developers and agents must **always check the original GitHub Actions source code for compatibility** and never guess protocol contracts or operational semantics.

---

## Official Source Repositories

### 1. Runner Core & Execution Protocol

- **Repository**: [actions/runner](https://github.com/actions/runner)
- **Role**: Official GitHub Actions runner daemon and execution engine.
- **Reference Areas**:
  - `src/Runner.Worker`: Job execution, step runner pipelines, command listeners, output and environment parsing.
  - `src/Runner.Listener`: Runner registration, session management, message polling, and job acquisition.
  - `src/Runner.Common`: Shared contracts, constants, job message schemas, and configuration formats.

### 2. Auto-scaling Runner Scale Sets

- **Repository**: [actions/scaleset](https://github.com/actions/scaleset)
- **Role**: Official controller and listener for GitHub Actions Runner Scale Sets.
- **Reference Areas**:
  - Scale set message protocol and session acquisition contracts (`JobMessageBase` in `types.go` deprecates `runnerRequestId` and sets `runnerRequestId: 0` on current `JobAvailable`, `JobAssigned`, `JobStarted`, and `JobCompleted` messages; because a job can be reassigned to a scale set up to three times with a new `scaleSetAssignTime` / `requestAssignedTime`, `velnor-runner-github` derives the positive synthetic `request_id` for `runnerRequestId: 0` from `jobId` plus the non-zero `scaleSetAssignTime` or `requestAssignedTime`).
  - The pinned [client.go](https://github.com/actions/scaleset/blob/716908e507993947ada4273aecf2361c02e5836c/client.go#L348-L405) joins API paths to the Actions service URL and sends the paired admin token as a bearer credential. Its [refresh path](https://github.com/actions/scaleset/blob/716908e507993947ada4273aecf2361c02e5836c/client.go#L1136-L1159) replaces the URL and token together. Velnor preserves service URL paths and accepts HTTPS URLs without user information.
  - Scale set and session deletion (`journal` created-set delete, `delete_runner_scale_set`, `delete_session`). The pinned [`DeleteRunnerScaleSet`](https://github.com/actions/scaleset/blob/716908e507993947ada4273aecf2361c02e5836c/client.go#L656-L674) issues `DELETE /_apis/runtime/runnerscalesets/{id}` with `api-version=6.0-preview` (added by `newActionsServiceRequest`) and treats only `204 No Content` as success. The pinned [`MessageSessionClient.Close`](https://github.com/actions/scaleset/blob/716908e507993947ada4273aecf2361c02e5836c/session_client.go#L39-L42) and `deleteMessageSession` issue `DELETE /_apis/runtime/runnerscalesets/{id}/sessions/{sessionId}` and also accept only `204`. The upstream source does not define any other status: Velnor therefore counts only `204` as a delete, never trusts a bare `404` for the scale set (it requires a by-id miss and an absent listing in the runner group before recording `deleted`), records `400/401/403/409/422` as a definite refusal that left the set in place, and treats every other answer as uncertain. A `404` on a session delete is accepted as already gone only when the journal shows Velnor itself deleted the set that held the session. This is a Velnor decision, not upstream behavior.
  - Ephemeral runner lifecycles, graceful teardown, and runner pool auto-scaling contracts.
  - Compatibility with GitHub Enterprise Server and GitHub.com scale set endpoints.

### Organization scale-set registration

- The actions/scaleset client registers with `POST /actions/runner-registration` carrying the `url` of the registration scope and a registration token; GitHub documents the organization token as `POST /orgs/{org}/actions/runners/registration-token` and the repository token as `POST /repos/{owner}/{repo}/actions/runners/registration-token`. For an organization binding, `velnor-runner-host` sends `url=https://github.com/{org}` and uses the organization token path; the organization must equal the owner of the bound repository, compared exactly as configured.
- Runner groups are listed at `_apis/runtime/runnergroups` on the admin connection. A named group must match exactly one entry and the unnamed choice must match exactly one default entry. Duplicate numeric group IDs anywhere in the response are rejected, because the journal keeps only the numeric ID.
- An organization scale set is shared by every repository of the organization that selects the same group, name and labels, so the host never adopts an existing organization scale set that this connection did not create; a name conflict stays uncertain.
- The bound repository ID, owner and name are verified before registration and again on every admin refresh. Owner and name compare ASCII case-insensitively against GitHub's canonical spelling; the repository ID must match exactly.

### 3. Runner Environments & Tooling Inventories

- **Repository**: [actions/runner-images](https://github.com/actions/runner-images)
- **Role**: Canonical image definitions and software inventories for GitHub-hosted runners.
- **Reference Areas**:
  - Toolcache directory hierarchies (`RUNNER_TOOL_CACHE`, `AGENT_TOOLSDIRECTORY`).
  - Pre-installed utility versions, PATH layouts, and environment configurations.
  - OS-specific tool locations for Ubuntu Linux and macOS runner images.

The pinned [`xcode-27` runner image inventory](https://github.com/actions/runner-images/blob/9329c776c4a38d3dcb31fc78b8180005041c07fb/images/macos/xcode-27-arm64-Readme.md)
defines the available macOS/Xcode versions and installation paths. Velnor represents this ARM64 image as the typed `xcode-27-arm64` runner (`runs-on: xcode-27`).
Consumer qualification binds the exact `version`, `build`, `sdk_version` and `developer_dir` through `system_tools`; the image's default Xcode is not a substitute.
Velnor probes and runs the named task with that `DEVELOPER_DIR` and never changes the host's `xcode-select` setting.

---

## Compatibility Rules and Verification Procedures

### Interrupted JIT requests

Recovery retains a late-registration tombstone for 24 hours from the claim
timestamp, or until the runner is observed and deleted. The
[JIT configuration endpoint](https://docs.github.com/en/rest/actions/self-hosted-runners#create-configuration-for-a-just-in-time-runner-for-an-organization)
does not bound the time a request may take to create a runner. A runner created
after 24 hours has a JIT configuration nobody holds, so it cannot come online
or run a job. GitHub removes offline ephemeral runners itself; this residual is
accepted.

### Job protocol client (`velnor-runner`)

`crates/velnor-runner` follows `actions/runner` v2.338.2. Each row names the
upstream mechanism the code copies.

| Behavior | Upstream source |
| --- | --- |
| Every run-service call makes at most 5 attempts with a random 5 to 15 second back-off. Acquire does not retry a typed 404, 409 or 422. Completion does not retry a 401 or a typed 404. Renew does not retry a typed 404. Everything else is retried, including a 400 and a dropped connection. | `src/Runner.Common/RunServer.cs` lines 63-102, `src/Runner.Common/RunnerService.cs` lines 71-110 |
| A run-service error is typed only when its body has `source` equal to `actions-run-service`. The body `statusCode`, not the HTTP status, decides. A completion that gets a typed 404 fails as job not found, as upstream records the job-not-found exception and fails. A 404 with another or no `source` is an ordinary retried error. | `src/Sdk/RSWebApi/RunServiceHttpClient.cs` lines 100-112, 165-172, 212-219, 237-256 |
| A broker error is typed only when its body has `source` equal to `actions-broker-listener`. Poll types `RunnerNotFound`, `RunnerVersionTooOld`, `HostedRunnerDeprovisioned` and `RunnerSessionInvalid`. Acknowledge types `AcknowledgeJobNotFound` and `RunnerNotFound`. Every other broker error is untyped. | `src/Sdk/WebApi/WebApi/BrokerHttpClient.cs` lines 116-133, 268-276, 285-304; `src/Sdk/RSWebApi/Contracts/BrokerErrorKind.cs` |
| Session creation maps 403 to access denied and 409 to a session conflict, and does not retry. | `src/Sdk/WebApi/WebApi/BrokerHttpClient.cs` lines 157-167 |
| The worker applies only Regex mask hints. It logs and skips every other type, including Variable, and the job still runs. A `type` that is not an integer, a numeric string or a known name fails the job message. | `src/Runner.Worker/Worker.cs` lines 176-190; `src/Sdk/WebApi/WebApi/VssJsonMediaTypeFormatter.cs` line 87; `src/Runner.Sdk/Util/StringUtil.cs` lines 12-18, 35-38 |
| A feed stream WebSocket connect is cancelled after 30 seconds. | `src/Runner.Common/JobServer.cs` lines 207-208 |
| A blob upload sets `x-ms-blob-type: BlockBlob` only when the blob storage type is Azure. | `src/Sdk/WebApi/WebApi/ResultsHttpClient.cs` lines 268-271 |

Deliberate deviations:

- The crate has no logger. A mask hint with an unsupported type parses to
  `MaskKind::Unsupported` and carries the spelling from the wire. The worker
  that consumes the job message writes the "Unsupported mask type" line and
  skips the hint.
- A mask `type` that is a float, boolean, null, object, array or an unknown
  name fails the job message, as upstream's string-enum converter does
  (`VssJsonMediaTypeFormatter.cs` line 87, `StringUtil.cs` lines 12-18,
  35-38). A string that trims to an `i32` parses as that number. Any other
  string is a comma-separated list of names, matched case-insensitively and
  trimmed, whose values are ORed, so `Variable, Regex` becomes 3 and parses
  to `MaskKind::Unsupported("3")`. A number inside a list fails the job
  message. A numeric string outside `i32` fails in both. A JSON integer
  outside `i32` fails the job message, a deviation from .NET, where
  `Enum.ToObject` truncates it. An unknown
  integer parses to `MaskKind::Unsupported`, and a missing `type` parses to
  `Unsupported("0")`, the enum default. The names Variable and Regex match
  case-insensitively.
- The parser maps `Variable` to `MaskKind::Variable`. The consumer decides
  what to do with it.
- A completion treats the first HTTP 401 as fatal. Upstream's HTTP handler
  first invalidates the token and repeats the request with a fresh one, and
  raises the unauthorized exception only when that fails
  (`src/Sdk/Common/Common/RawHttpMessageHandler.cs` lines 175-217). The crate
  has no token provider, so refreshing the token belongs to its caller.
- An acquire whose 2xx response has no JSON object (an empty body, a
  non-JSON body, `null` or an array) or whose JSON object fails the typed
  `AgentJobRequestMessage` parse is a transient failure, and is retried under
  the acquire policy. Upstream's `ReadContentAsAsync` returns `null` for a
  body that is not JSON or does not lex as JSON, so `GetJobMessageAsync`
  returns `null` and `JobDispatcher.Run` fails on its first dereference of the
  message. Upstream's `RetryRequest` retries any other exception, including a
  parse error (`src/Sdk/WebApi/WebApi/RawHttpClientBase.cs` lines 261-285;
  `src/Runner.Common/RunServer.cs` lines 63-71; `src/Runner.Common/RunnerService.cs`
  lines 71-110; `src/Runner.Listener/JobDispatcher.cs` lines 89-91).
- An OAuth token exchange yields a token only for HTTP 200. Upstream treats a
  200, or a 400 with a JSON body, as a valid response and checks `access_token`
  first: a non-empty `access_token` issues a token even when the same response
  carries `error`, and only an empty one falls through to the `error` branch.
  Velnor does the same for a 200, so a 200 with both `access_token` and `error`
  yields the token. It refuses a 400 that carries an access token, and refuses
  a 201 and every other 2xx (`src/Sdk/WebApi/WebApi/OAuth/VssOAuthTokenHttpClient.cs`
  lines 99-102, `IsValidTokenResponse`; `src/Sdk/WebApi/WebApi/OAuth/VssOAuthTokenProvider.cs`
  lines 138-170, `OnGetTokenAsync`). A 400 carrying `error` still maps to the
  token error, and `invalid_client` still means the registration is gone.
- The acquire typed-parse error names the failure category, the line and the
  column, and the JSON path of the field. It never repeats the offending value,
  because serde's own message quotes it and the body can carry a secret.
- The run-service, Distributed Task, feed and results clients accept only an
  HTTPS URL on `*.actions.githubusercontent.com` with no port, query or
  fragment. GitHub Enterprise Server hands the runner
  `https://[hostname]/_services/pipelines/` endpoints (`docs/checks/actions.md`)
  and runs jobs through the Distributed Task job server, not the run service
  (`src/Runner.Worker/JobRunner.cs` lines 66-107). `velnor-runner` runs this
  repository's jobs on github.com (`docs/goals.md`), so those endpoints are
  refused by design.
- `TimelineRecord` carries the record state and the three issue counters, not
  `Issues` or `Variables`. Upstream sends those two collections only through
  the Distributed Task job server, and skips that update for a run-service job
  (`src/Runner.Worker/JobRunner.cs` lines 66-85 start the queue with
  `resultsServiceOnly: true`; `src/Runner.Common/JobServerQueue.cs` lines
  696-698 guard `UpdateTimelineRecordsAsync` with `!_resultsServiceOnly`). A
  run-service job reports annotations, with `rawDetails`, and outputs in
  `completejob` (`src/Runner.Worker/JobRunner.cs` line 332).
- `AgentJobRequestMessage.variables` is a case-sensitive map. Upstream's wire
  message also deserializes into a plain dictionary; the case-insensitive
  store is `Runner.Worker.Variables`, a `ConcurrentDictionary` with
  `StringComparer.OrdinalIgnoreCase` (`src/Runner.Worker/Variables.cs` line
  17). The step executor that builds that store owns the case folding.
- Step-update and blob uploads to the results service make 3 attempts with a
  200 ms step between them. This is Velnor's policy; it is not copied from
  upstream.

### Performance review sources

The [review gates](../reviews/ci-performance.md) use these primary sources:

| Source | Review consequence |
|---|---|
| [Dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching) | Entries are immutable and branch-scoped; eviction and service failure require cold correctness. GitHub's default is 10 GB per repository; paid expansion does not change Velnor's adopted budget. |
| [Pinned cache restore implementation](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/restoreImpl.ts) and [save implementation](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts) | `cache-hit` reports exact key matching, not payload validity; partial matches need reconciliation, and unchanged exact entries need no save. |
| [Separate cache save](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/save/README.md) | Save reusable state after successful production; unrelated later test failures must not discard it. Velnor's writer and cancellation rules still apply. |
| [Job outputs](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/pass-job-outputs) and [workflow artifacts](https://docs.github.com/en/actions/tutorials/store-and-share-data) | Use validated job outputs for bounded metadata and identity-checked artifacts for files; never use a best-effort cache as the sole required handoff. |
| [Required status checks](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks) | Skipped jobs can report success and filtered workflows can leave checks pending; preserve Velnor's planner and fail-closed `Required` aggregation. |
| [Actions limits](https://docs.github.com/en/actions/reference/limits) | Verify current workflow, matrix, concurrency and cache request limits for representative consumer scale. |

These are review criteria, not new runtime behavior or cache capacity. Existing
[cache decisions](cache-decisions.md) remain binding.

### Runner-scoped crates mirror

The official runner [sets the runner environment context](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/JobRunner.cs#L150-L156) from the job message before executing steps. GitHub’s [runner context contract](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#runner-context) defines `runner.environment` as `github-hosted` or `self-hosted` and permits the runner context in step conditions.

The new generator emits mirror setup only for the configured matching runner label. Every emitted mirror step additionally requires `runner.environment == 'self-hosted'` and excludes fork pull requests by requiring the pull-request head repository to match `github.repository`. Label spelling alone cannot establish the runner environment: new hosted labels and custom scale-set names remain protected by the runtime condition. An absent or unrecognized runner environment skips mirror setup.

### Ledger event and source identity

The runner's [`GitHubContext.GetRuntimeEnvironmentVariables`](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/GitHubContext.cs)
exports the supplied `sha`, `workflow_sha`, `event_name`, and `event_path` context values as environment variables.
[`ExecutionContext.WriteWebhookPayload`](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/ExecutionContext.cs)
writes the event context to `event.json` and sets its exported path when that context exists.
GitHub's [pull-request event contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
identifies `GITHUB_SHA` as the test merge commit; the logical pull-request source is `github.event.pull_request.head.sha`.

Planning and ledger emission therefore share Velnor's event classifier and reference resolver.
Emission reads the bounded event file, rejects malformed JSON, and requires the independently resolved event and head to match the validated staged Plan before writing a ledger.
Fork pull requests use the contract event `fork`; `workflow_dispatch` uses `qualification`.
The original runner ref and workflow SHA remain unchanged, and Required keeps its strict source and event comparisons.
Public CLI regressions cover a synthetic merge SHA distinct from the logical head, both event normalizations, and rejection without a ledger when event context is missing, malformed, or changed.

### Native release job authority

Aggregate workflow status alone never establishes caller authority; verify the expected producer job independently.
The official [attempt-specific jobs endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt) supplies the bounded complete inventory selecting one generated role's positive job ID. The [individual job endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#get-a-job-for-a-workflow-run) refreshes its identity, status and conclusion using existing Actions read permissions. The runner's [GitHubContext export](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/GitHubContext.cs) exports `GITHUB_JOB`; Velnor maps this machine identifier to the generated display name through a closed mapping checked against actual rendered jobs.
Native guards authenticate exact source, workflow, run and attempt; aggregate `queued` or `in_progress` requires no conclusion plus the unique expected job executing with no conclusion. Only immutable locators are cached; subsequent guards fetch the individual job again. Historical signed-run evidence remains separate from the current publisher, recovery or tap caller's authority.

### GitHub App installation token mint and revocation

`actions/create-github-app-token` at the pinned commit `64ab729a6294000f5aa4a4bc0b73d1f32bc45510` (v3.3.0) declares `post: "dist/post.cjs"` with no `post-if` in [`action.yml`](https://github.com/actions/create-github-app-token/blob/64ab729a6294000f5aa4a4bc0b73d1f32bc45510/action.yml), so its post step runs at the end of every job that ran the main step, with the action's inputs (the private key among them) as `INPUT_*` variables. Its [`lib/post.js`](https://github.com/actions/create-github-app-token/blob/64ab729a6294000f5aa4a4bc0b73d1f32bc45510/lib/post.js) sends `DELETE /installation/token` with the token and only warns when that request fails. Its [`lib/get-permissions-from-inputs.js`](https://github.com/actions/create-github-app-token/blob/64ab729a6294000f5aa4a4bc0b73d1f32bc45510/lib/get-permissions-from-inputs.js) turns each `permission-<name>` input into the API permission `<name>` with `-` replaced by `_` (so `permission-pull-requests` asks for `pull_requests`), and inherits every App permission when no such input is set. `velnor-ci app-token` takes the API names directly in `--permission=<name>:<access>`, asks for `contents: read` when no flag is given, and never sends an empty permission set. The generated release jobs revoke the token with the same `DELETE /installation/token` request in their last step, sent by `velnor-ci revoke-token` to `github.api_url`. The step does not use `gh`, because `gh help environment` reads `GH_TOKEN` only for github.com and ghe.com hosts and needs `GH_HOST` and `GH_ENTERPRISE_TOKEN` for a GitHub Enterprise Server.

### Reusable workflow signer identity

The [GitHub CLI attestation verifier](https://cli.github.com/manual/gh_attestation_verify)
supports a reusable workflow as the signer through `--signer-workflow`. GitHub's
[OIDC claim reference](https://docs.github.com/en/actions/reference/security/oidc)
defines `job_workflow_ref` and `job_workflow_sha` for the reusable workflow,
while `workflow_ref` and `workflow_sha` identify the caller workflow. Fulcio's
[official identity template](https://github.com/sigstore/fulcio/blob/e8104e83e11a651c7e9e712c02a57d5b43d19d5d/config/identity/config.yaml)
maps the reusable workflow claims to `buildSignerURI`/`buildSignerDigest`,
builds the certificate subject alternative name from the reusable workflow ref,
and maps caller claims to `buildConfigURI`/`buildConfigDigest`. The source
repository digest and run invocation URI identify the caller run.

The Runner-family publisher checks the Actions run as
`product-release.yml`, checks the exact repo-qualified referenced-workflow
path/ref/SHA for `product-release-images.yml`, and verifies attestations with
that callee as signer. The source selected by the caller can differ from the
caller's workflow-head SHA, so source identity comes from the closed producer
record, committed lock, OCI revision and successful same-source CI run rather
than from the certificate's caller source digest.

### Step shell resolution and `addPath`

[`ScriptHandler.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Worker/Handlers/ScriptHandler.cs) resolves a step's shell with `WhichUtil.Which(command, …, prependPath)`, where `prependPath` holds every `core.addPath`/`GITHUB_PATH` entry ahead of the job `PATH`. In a composite action (`IsActionStep`) even `shell: bash` takes this route. [`WhichUtil.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Sdk/Util/WhichUtil.cs) returns a fully qualified command as given when the file exists and never searches `PATH` for it; a custom shell string is split at its first space and must contain `{0}`. `jdx/mise-action` adds the Mise `bin/` and, by default, `shims/` with `core.addPath`, so after a tools-cache restore a bare shell or helper name can resolve to restored bytes. Velnor's restore checks therefore use `/bin/bash --noprofile --norc -p -e -o pipefail {0}` and reset `PATH` to `/usr/bin:/bin:/usr/sbin:/sbin` first ([hermetic checks](tools-cache-v2.md#the-checks-run-hermetically)).

Trusted generator helper invocations use the same absolute Bash shell. Bash's
[`-p` mode](https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html)
ignores startup files, imported functions, and shell options inherited through
the environment. This prevents persisted `BASH_ENV`, exported functions, or
`SHELLOPTS` from bypassing the helper's copy, digest check, and execution before
the generated command runs.

### Composite action `required` inputs

[`ActionRunner.cs`](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ActionRunner.cs) at `v2.338.2` fills each declared input a caller did not pass with its `default` and never checks `required`, so an omitted `required: true` input reaches the steps as an empty string. `velnor-runner` deviates on purpose when it expands `tailrocks/velnor-new/actions/setup-velnor`: a `required: true` input with no value and no default fails the step with `setup-velnor input '<name>' is required` before any inner step runs, so a missing `version` stops the install instead of reaching the shell script as an empty string.

The expressions of a composite step (`name`, `shell`, `env` and `run`) are evaluated against the contexts of the step that uses the action, as [`CompositeActionHandler.cs`](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/Handlers/CompositeActionHandler.cs) at `v2.338.2` does: `github`, `runner` and `env` (the job environment plus the `env` of the outer step) come from the outer step, `inputs` holds the action's resolved inputs and `steps` is empty. `secrets` and `needs` are not available to a composite step upstream (`action_yaml.json` lists `github`, `inputs`, `strategy`, `matrix`, `steps`, `job`, `runner` and `env`), and `velnor-runner` supplies no `strategy`, `matrix`, `job` or `vars`; an expression that names one of them fails the step with `unsupported: setup-velnor expression: unsupported expression context <name>` before any inner step runs. An action whose `runs.steps` is missing or empty fails with `setup-velnor action.yml declares no steps`. The `env` of the inner step is rendered first and merged over the `env` context before the step's `name`, `shell` and `run` are rendered, as upstream does ([`CompositeActionHandler.cs`](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/Handlers/CompositeActionHandler.cs#L287-L293)). `github.action_path` is `<store root>/actions/tailrocks/velnor-new/<sha>`; only `action.yml` is read from that revision, so the directory is not created and a script that reads another file from it fails. An input `default` that contains `${{` fails the step with `unsupported: setup-velnor input '<name>' has a default that is an expression`, because defaults are not evaluated. `velnor-runner` reads `action.yml` when the step runs, so the unknown-input, required-input and default checks cannot run at admission; the reference, the repository and the trusted facts are checked at admission, by the same function the step uses.

### Mise isolation

Mise renders Tera templates, so a config file runs code as soon as any `mise` command loads it: `exec()` in `[vars]` or `[tools]` runs during `mise config ls --json`. Which files mise loads depends on its discovery rules ([`docs/configuration.md`](https://github.com/jdx/mise/blob/v2026.11.10/docs/configuration.md?plain=1#L41-L53) at `v2026.11.10`): the system, global and every parent directory, `.local`, environment, `conf.d`, `.tool-versions` and idiomatic variants, `.miserc.toml`, and the file names that `MISE_DEFAULT_CONFIG_FILENAME`, `MISE_OVERRIDE_CONFIG_FILENAMES`, `MISE_ENV` and `MISE_CD` change. That list grows with each release, so the `jdx/mise-action` adapter does not model it. It gives every mise process a private working directory and an exact environment, so mise cannot reach a file or a variable that exists before the step and that the adapter did not choose. A process that an earlier step left running as the same user can still write into `<RUNNER_TEMP>` while the step runs, including the private copy; the per-job process namespace of task #2911 contains that process, and this adapter does not.

The adapter reads `mise.toml` and `mise.lock` once, as git blobs at the job's trusted commit SHA from the broker's mirror (`git --git-dir=<mirror> ls-tree` and `show`), and requires both to be regular files (mode 100644 or 100755) of that commit. A blob that is not valid UTF-8 fails with `<name> in the trusted commit is not valid UTF-8`; the adapter decodes the captured bytes strictly and never repairs them. It never reads the work tree or runs git there, so an uncommitted edit, a `skip-worktree` or `assume-unchanged` edit and a planted `.git/config` (`core.fsmonitor`) neither run nor reach mise. It writes byte copies to `<RUNNER_TEMP>/velnor-mise-<uuid>/config` (a directory created with mode 0700), and runs every mise process there with the environment cleared and exactly these variables:

- `PATH` is the directory of the mise binary followed by `/usr/bin:/bin:/usr/sbin:/sbin`, and `HOME` is the empty directory `<root>/home`.
- `MISE_CEILING_PATHS` is `<root>`. mise stops at a ceiling path without searching it ([`file.rs`](https://github.com/jdx/mise/blob/v2026.11.10/crates/mise-util/src/file.rs#L2047-L2075) and [`miserc.rs`](https://github.com/jdx/mise/blob/v2026.11.10/src/config/miserc.rs#L632-L660)), so nothing above `<root>/config` loads. At `v2026.11.10` the ceiling matches an ancestor of the working directory by plain string equality, so the adapter canonicalizes `<root>` before it writes the variable.
- `MISE_SYSTEM_CONFIG_DIR`, `MISE_SYSTEM_DATA_DIR` and `MISE_SYSTEM_PLUGINS_DIRS` name the empty `<root>/system`, and `MISE_TRUSTED_CONFIG_PATHS` is `<root>/config`.
- `MISE_DATA_DIR` is the tools store data directory, `MISE_SHIMS_DIR` is `<RUNNER_TEMP>/mise-shims`, `MISE_CACHE_DIR` is `<root>/cache`, `MISE_AUTO_INSTALL` is `false` and `MISE_YES` is `1`.
- `MISE_CARGO_HOME` and `MISE_RUSTUP_HOME` are `<tools>/cargo` and `<tools>/rustup` in the tools store, which jobs cannot write. The job's `CARGO_HOME`, `RUSTUP_HOME` and `HOME` never reach mise, whether they come from the job message, the process or `GITHUB_ENV`, because the Rust plugin runs the `rustup`, `rustc` and `cargo` it finds in those homes and links `installs/rust/<version>` to `<cargo>/bin`. At `v2026.11.10` the plugin takes its homes from `[env]` of the accepted config, then `MISE_CARGO_HOME` and `MISE_RUSTUP_HOME`, then `CARGO_HOME` and `RUSTUP_HOME`, then `$HOME/.cargo` and `$HOME/.rustup` ([`select_rust_home`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/rust.rs#L1763-L1779), [`RustHomes::from_sources`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/rust.rs#L1489-L1523)). Every other path it derives sits under `MISE_CACHE_DIR` (`rust/rustup-init`, [`rustup_path`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/rust.rs#L1386-L1388)), `MISE_DATA_DIR` (`rust-proxies`, `installs/rust`, [`list_bin_paths`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/rust.rs#L1101-L1104)) or `PATH` (the proxy lookup), and the adapter sets all three.

Later steps keep the job's own homes. The adapter exports `MISE_CARGO_HOME` and `MISE_RUSTUP_HOME` (the store paths) so a later `mise` call resolves the same homes, and `RUSTUP_HOME` and `RUSTUP_TOOLCHAIN` as `mise env --json` prints them. It does not export `CARGO_HOME` when the value is the store's read-only cargo home, because that value would override the job's own and make `cargo fetch` fail with `Operation not permitted`; a job that sets `CARGO_HOME` keeps it, and a job that sets none has none.

The adapter accepts a closed set of keys in `mise.toml` and refuses everything else with `unsupported: mise policy: mise.toml key '<key>' is not supported by velnor-runner`, because a key that points mise at another file (`include`, `env_file`, `dotenv`, `[env] _.file`, `task_config`) lets a file that a job wrote, or a remote one, run code in the private copy, and `mise config ls --json` still lists only the copy. The set comes from the keys that this repository's `mise.toml` and the adapter tests use, checked against the schema and `docs/configuration.md` of mise `v2026.11.10`:

- Top-level keys: `min_version`, `tools`, `env`, `vars`, `settings` and `tasks`. Every other key of the schema is refused, among them `include`, `env_file`, `dotenv`, `env_path`, `plugins`, `hooks`, `alias`, `tool_alias`, `shell_alias`, `watch_files`, `secrets`, `redactions`, `task_config`, `task_templates`, `tool_config`, `deps`, `oci`, `dotfiles`, `bootstrap`, `history`, `monorepo_root`, `daemons`, `wrappers` and any key a later release adds. `wrappers` is refused because `mise install --locked` rebuilds `<MISE_DATA_DIR>/command-wrappers` for it ([`sync_command_wrapper_shims`](https://github.com/jdx/mise/blob/v2026.11.10/src/shims.rs#L1083-L1127)): on the read-only data directory that fails with `Permission denied`, and on a writable one it writes symlinks that the job chose into the shared store. The rust tool option `mr_boxington` adds the same `cargo` wrapper ([`add_rust_wrapper`](https://github.com/jdx/mise/blob/v2026.11.10/src/config/command_wrapper.rs#L8-L60)) and is refused with the tool options below.
- `[env]` and `[vars]` accept values and `_.path` only, as below.
- `[settings]` accepts `lockfile` (which must be `true`), `locked`, `cargo.binstall`, `idiomatic_version_file`, `idiomatic_version_file_enable_tools`, `jobs`, `http_timeout` and `http_retries`, written flat, dotted or as a `[settings.<group>]` table. Every other setting is refused with `unsupported: mise policy: [settings] key '<name>' is not supported by velnor-runner`, among them every setting that names a file, directory, URL or command (`env_file`, `ceiling_paths`, `override_config_filenames`, `rust.cargo_home`, `rust.rustup_home`, `trusted_config_paths`, `shims_dir`, `netrc_file`, the `*_credential_command` settings).

The adapter also checks the values of every `[tools]` entry, in the string, table and array forms, and every entry of `mise.lock`. These checks refuse what the adapter has a reason to refuse before mise runs; they are not the boundary. The boundary is the read-only tools store below: a value policy cannot stop the author of the job, who also writes `mise.toml` and `mise.lock`, and at `v2026.11.10` the `cargo`, `go`, `npm`, `gem`, `pipx`, `dotnet` and `spinel` backends never read the checksums that the lock records. The backend of a `[tools]` key is its prefix (`aqua:`, `github:`), or, for a name without a prefix such as `node` or `jactionlint`, the `backend` of its `mise.lock` entries; a name that has none is refused with `unsupported: mise policy: tool '<name>' without a backend in mise.lock is not supported by velnor-runner`. The backend names are those of [`BackendType::guess`](https://github.com/jdx/mise/blob/v2026.11.10/src/backend/backend_type.rs#L61-L85).

- Download backends are allowed: `core`, `aqua`, `github`, `gitlab`, `forgejo` and `packslip`. `mise install --locked` installs what `mise.lock` records for most of them, one URL and checksum per platform, but not for every one: `core:dotnet`, `core:rust` and `core:swift` return `supports_lockfile_url() = false` ([`dotnet`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/dotnet.rs#L114-L116), [`rust`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/rust.rs#L710-L712), [`swift`](https://github.com/jdx/mise/blob/v2026.11.10/src/plugins/core/swift.rs#L319-L321)) and fetch from their own sources. The adapter allows them by name, and they run only when the tools store already holds them, because a missing tool cannot be installed into a read-only store.
- Package backends are refused outright: `cargo`, `go`, `npm`, `gem`, `pipx`, `pypi`, `conda`, `dotnet`, `spm` and `spinel`. They install from a registry or a source repository, which `--locked` does not pin, and several never write or read a checksum, so a checksum rule would check a string that the job wrote. `ubi` is refused too: it installs from any `http` URL and `supports_lockfile_url()` is `false` for it ([`ubi.rs`](https://github.com/jdx/mise/blob/v2026.11.10/src/backend/ubi.rs#L474-L476)). A repository that needs such a tool pins it through a download backend such as `github:` or `aqua:`, and the store build installs it.
- Every other backend is refused with `backend '<prefix>' of tool '<key>'`: `asdf` and `vfox` (their plugins are cloned from a URL and run), a plugin backend named by any other prefix, `http` and `s3` (their sources are options that are refused below). A tool that is written without a prefix and locked to one of these is refused the same way.
- Every entry of every tool in `mise.lock` is checked, the ones the config names and the others, not only the first. A tool key with a prefix (`npm:cowsay`) is checked as a backend itself, whether or not a `[tools]` entry or `install_args` names it, so a lock entry without a `backend` cannot carry a package backend past the check. Every entry must carry a `backend` (an entry without one, and a tool that is not a table or an array of tables, is refused with `tool '<key>' without a backend in mise.lock`). The `backend` of an entry must be allowed, must not contain `[` or `]` (inline options such as `core:node[postinstall='...']` reach mise's tool options), and must have the same prefix as the key when the key has one (`github:acme/tool` cannot be locked to `aqua:acme/tool`). A refusal reads `backend '<prefix>' of tool '<key>'`, `backend '<backend>' of tool '<key>' with inline options` or `backend '<prefix>' of tool '<key>' that differs from its key`.
- A version or option string that starts with `path:` or contains `file://` is refused with `tool '<key>' source '<text>'`, because it runs a program from a place that a job can write. A key with `[` or `]` (inline options) and a value that is not a string, a table or an array are refused too.
- A tool option table accepts `version`, `os`, `profile`, `components`, `targets` and `workflow`, and nothing else. The first two select the version and the platforms; the next three select the rust toolchain profile, components and targets; `workflow` pins the signing workflow that `packslip` verifies. Every other key is refused with `tool option '<name>' of '<key>'`, among them `postinstall` and `install_env` (run commands, [`insert_core_option`](https://github.com/jdx/mise/blob/v2026.11.10/src/toolset/tool_version_options.rs#L662-L727)), `depends`, `mr_boxington`, `url`, `platforms`, `api_url` and every backend option that names a source, a path or a command. The value of an accepted key is a scalar or an array of scalars; a table is refused under the key's name.
- `[env]` refuses the names `CARGO_HOME`, `RUSTUP_HOME`, `PATH`, `HOME` and every name that starts with `MISE_` or `RUSTUP_`, compared without regard to case, in the table and the `[[env]]` form, with `unsupported: mise policy: [env] name '<name>' is not supported by velnor-runner`. These names choose the homes, the programs and the directories that mise runs, and the adapter sets them itself.

The tools store is the boundary, and it must be mounted read-only. The adapter resolves `<store>/tools` and opens it from `/` one component at a time with `openat(2)`, `O_DIRECTORY` and `O_NOFOLLOW`. The path must be its own real path: a store root reached through a link (a link in a directory the runner user can write, pointing at the mount) fails the step with `mise policy: <tools path> is reached through a link (<real path>); the tools store path must not contain links`, because the adapter probes the real directories but hands mise the path as given, and whoever can write the directory that holds the link can retarget it after the probe. Every directory on the way down, `<store>/tools` included, must be owned by root and writable by neither its group nor others. One that is not fails the step with `mise policy: <directory> is owned by uid <uid>; every directory above the tools store must be owned by uid 0` or `mise policy: <directory> is writable by its group or others (mode <mode>); no directory above the tools store may be`. With such ancestors only root can rename or replace a directory of the store after the checks, which is why the file that the adapter hashes is the file that runs: before this rule a user who could write an ancestor of `VELNOR_STORE_ROOT` could swap the tree after the probe and run another `mise`. The default store root `/var/lib/velnor` passes when root owns it and `/var/lib`.

The adapter then opens the mise binary from the `<store>/tools` descriptor with `openat(2)` and `O_NOFOLLOW`, one component at a time: `mise`, `<version>`, then the file `mise`. A link at any of the three fails the step with `mise policy: <path> is reached through a link; the tools store holds only real files`, a binary that is not a regular file with `mise policy: <path> is not a regular file; the tools store holds only real files`, and the file that is hashed for the `sha256` input is the descriptor that was opened. A symlink to a writable file would otherwise pass the directory check below and run. Before it runs mise, it opens `MISE_DATA_DIR` (`<store>/tools/data`), `MISE_RUSTUP_HOME` (`<store>/tools/rustup`), `MISE_CARGO_HOME` (`<store>/tools/cargo`) and the directory of the mise binary (`<store>/tools/mise/<version>`) the same way, with `O_DIRECTORY | O_NOFOLLOW` from that descriptor, and calls `fstatvfs(2)` on each. A directory that is a link fails with `mise policy: <name> <directory> is reached through a link; the tools store holds only real directories`. The directory must report `ST_RDONLY`, which only a read-only mount sets. Mode bits and `access(2)` do not count: a `0555` directory still holds writable children (`installs`, `downloads`), a `0555` directory owned by the step's user can be made writable with `chmod`, and the check and the later use of a path are two steps. A directory that is not on a read-only mount fails the step with `mise policy: <name> <directory> is not on a read-only mount; the tools store must be mounted read-only`, and a missing one with `mise policy: <name> <directory> is missing from the tools store`, where `<name>` is `MISE_DATA_DIR`, `MISE_RUSTUP_HOME`, `MISE_CARGO_HOME` or `the mise binary directory`.

The read-only flag belongs to one mount, so the adapter also refuses a writable mount at or under any of the four directories, with `mise policy: <name> <directory> holds a writable mount at <mount point>; the tools store must be one read-only mount`. On Linux it reads `/proc/self/mountinfo`, where a mount counts as writable only when both its mount options and its superblock options say `rw`. Elsewhere there is no mount table to read without a child process or `unsafe` code, and `velnor-runner` forbids `unsafe`, so the adapter scans: it opens every directory below each of the four with `openat(2)` and `O_NOFOLLOW`, calls `fstatvfs(2)` on it, and takes the first directory that lacks `ST_RDONLY` as the mount point, because under a read-only mount only the root of another mount can lack it. The scan reads directories, not files: it costs one `openat` and one `fstatvfs` per directory of the four trees, on a blocking thread, and it runs once per `jdx/mise-action` step (measured on macOS with a store of 20,200 directories: about 0.8 to 1.7 s per step; when the scan ran at all three check points the same store cost 11.9 to 18.9 s per step). A table that cannot be parsed fails the step with `mise policy: the mount table from /proc/self/mountinfo cannot be parsed`. Without this check a read-only data directory with a writable `installs` mount lets mise download into the shared store.

The checks are point-in-time. The adapter runs them in full at the start of each `jdx/mise-action` step. When `mise install --locked` has returned and before the first `mise env`, and again just before it writes `GITHUB_PATH` and `GITHUB_ENV`, it opens and `fstatvfs`es the four directories again and, where a mount table exists (Linux, `/proc/self/mountinfo`), reads it again; it does not repeat the directory scan. So a mount that appears while the step runs fails the step with the message above and exports nothing, except that off Linux a writable mount nested below one of the four directories that appears after the start of the step is not seen until the next `jdx/mise-action` step scans. A mount that a process adds after the last check, during a later `run:` step for example, is used until the next `jdx/mise-action` step checks again. The adapter cannot prevent that. Task #2911 owes a store that jobs cannot mount over (below).

The tools store build installs every tool that `mise.lock` lists, including the runtime symlinks that mise rebuilds (`latest`, the major and minor names), so `mise install --locked` has nothing to write. Every entry of the `PATH` that the adapter exports from mise is resolved to its real path when it lies under one of the four directories: a link inside the store that leaves it (`<data>/installs/<tool>` pointing at a writable place, or an entry that climbs out with `..`) fails the step with `mise policy: PATH entry <entry> resolves to <real path> outside the read-only tools store` before anything is written to `GITHUB_PATH` or `GITHUB_ENV`; a link that stays inside the store is accepted. Entries outside the four directories (the inherited system directories and the `_.path` entries of the accepted config, which the job writes anyway) are not resolved. This is the adapter's only check of links. A link inside the store that is not a `PATH` entry, such as a file link to a writable file or a link under a tool the root config does not name, is not detected: the store build owes that none exists (below). A tool that is missing from the store fails with `tool is not installed in the tools store`, and a job cannot make mise install one, whatever `mise.toml` and `mise.lock` say. Tests mark a directory read-only for these probes through `VELNOR_TEST_READ_ONLY_DIRS`, replace the mount table with a file through `VELNOR_TEST_MOUNTINFO` (an empty value selects the host's table), and name the owner and the topmost directory whose ancestors are examined through `VELNOR_TEST_STORE_TRUST` (`<uid>:<directory>`); all three exist only in a build with the `test-support` feature and are not compiled into a release build. Two macOS tests mount a read-only disk image with `hdiutil` and run the adapter against it without these variables, one with a writable image mounted under each of the four directories.

The store build owes the adapter three properties, recorded on task #2911. The store holds no link that leaves it: every symlink under the four directories resolves inside them, and the root-run build verifies this, because the adapter checks only the `PATH` entries it exports. The four directories are one read-only mount with no writable mount nested under any of them. And jobs cannot mount anything over the store: the image is attached by root with ownership enforced, or the store is a VM-local disk, and a job has no `CAP_SYS_ADMIN`. The last one is a blocking requirement, because the adapter checks only at the points above and a mount that a job adds between two checks is used. The adapter refuses a store that breaks the first two at the moment it checks, but it cannot repair one.

Adding a key or a setting to either list needs a review that reads what mise does with it at the pinned release.

Before `mise install --locked`, the adapter runs `mise config ls --json` and fails the step with `unsupported: mise policy: mise loads [...]` unless mise lists exactly `<root>/config/mise.toml`. This check does not carry the isolation; it fails loudly when a later mise release reads a file from a place the environment above does not pin. The adapter removes `<root>` when the step succeeds, and on an error or a cancel through `Drop`.

Paths that name the private copy are rewritten to the workspace before they are masked and exported: every `mise env --paths` line and every value of `mise env --json` and `mise env --redacted --json` has each occurrence of `<root>/config` that ends at a path boundary replaced by the workspace path. A boundary is the end of the value, `/`, `:`, `,`, `;`, a quote or whitespace, so `PATH` entries, `-I<root>/config/include` and `<root>/config:<root>/config/x` are rewritten, while `<root>/config2` and `<root>/config-post` are not. So `_.path = ["bin", "{{config_root}}/tbin"]` reaches later steps as `<workspace>/bin` and `<workspace>/tbin`, and `CFLAGS = "-I{{config_root}}/include"` as `-I<workspace>/include`.

The adapter runs only the mise release that `.velnor/version-policy.toml` pins under `[tools] mise`, which `velnor-runner` embeds when it is built. A `version` input that names any other release fails the step with `unsupported: mise <version> is not the mise version <pinned> pinned in .velnor/version-policy.toml` before the tools store is read, because the checks in this section describe that release (`v2026.11.10`) and no other. The integration tests run the real mise at exactly that version: they take the first `mise` on the `PATH` that reports it, and fail when none does.

The adapter publishes the `cache-hit` output only. Upstream `jdx/mise-action` also publishes `versions` and one output per installed tool (`steps.<id>.outputs.<tool>`); the adapter does not run `mise ls` and leaves them unset, so a later step that reads one gets an empty string. This is a recorded deviation: the workflows of this repository read none of them. Admission refuses a `jdx/mise-action` step whose revision, inputs or trusted facts the step would refuse, by calling the function the step calls, so no earlier step runs before the refusal.

These deviations from [`jdx/mise-action`](https://github.com/jdx/mise-action/tree/236a21c3b91f5db6d4e174b348674a0f0caac06d) 236a21c3, which runs `mise` in `getCwd()` with `process.env`, are on purpose:

- mise runs in a private copy, so the templates, `exec()` and hooks of the accepted config see that copy as `config_root` and as the working directory.
- Only `_.path` directives are supported in `[env]` and `[vars]`, in the table and in the `[[env]]` form. `_.file`, `_.source`, `_.python` and any other `_` directive fail with `unsupported: mise policy: [env] _.<name> is not supported by velnor-runner`, because a relative path in the copy is skipped silently while `read_file()` fails.
- The job's `MISE_*`, `XDG_*`, `HOME` and `PATH` do not reach the adapter's mise. Later steps keep them: the adapter exports `MISE_TRUSTED_CONFIG_PATHS=<workspace>` and `MISE_YES=1` only when the job lacks them, as upstream `set()` does.
- The adapter does not load `mise.local.toml`, environment variants, `conf.d`, `.tool-versions`, idiomatic version files, miserc, or parent, global or system config, and it does not reject them. Later `run:` steps that call mise or its shims in the workspace still load them.
- Shims live in `<RUNNER_TEMP>/mise-shims`, because the tools store data directory is read-only.

### Workflow command parsing

[`ScriptHandler.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Worker/Handlers/ScriptHandler.cs) gives a step's standard output and its standard error each an `OutputManager` that feeds the same `ActionCommandManager`, so a workflow command on either stream runs. [`ActionCommand.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Common/ActionCommand.cs) calls `TrimStart()` on a line before it checks for the `::` prefix, so a command can follow leading Unicode whitespace. Command data escapes `%`, `\r` and `\n` as `%25`, `%0D` and `%0A`; command properties also escape `:` and `,` as `%3A` and `%2C`. `::stop-commands::<token>` pauses command processing until a command named by the token resumes it. `velnor-ci` therefore escapes every control and non-space whitespace character and `%` in anything it prints, and escapes `:` in names taken from repository content, so no printed line can start a command on either stream.

### Step executor (`velnor-runner exec`)

These facts hold for runner v2.338.2 ([`d1eb12d4`](https://github.com/actions/runner/tree/d1eb12d4b115c301201de27d667284e1a80f2ab5)). `crates/velnor-runner/src/exec` follows the Worker and differs from it in the places below; check them again when the pin changes.

- **Mask hints.** [`Worker.cs:176-189`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Worker.cs#L176-L189) applies only `regex` hints, through `AddRegex` plus the pattern text as a literal value, and traces a warning for every other type. The executor masks a hint of any other type as a literal value: dropping a value the service asked to hide can only leak. A `regex` hint that does not compile fails the job, as the exception from `AddRegex` does ([`Worker.cs:97`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Worker.cs#L97), [`Program.cs:54-70`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Program.cs#L54-L70)); the error names the hint by position and never prints the pattern.
- **Annotations.** [`ExecutionContext.AddIssue`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ExecutionContext.cs#L796-L801) masks only `Message`. The executor masks `message`, `title` and `file` of every annotation, rejection lines included, with the final masker before `run_job` returns, so a value added by `::add-mask::` after the annotation is covered.
- **Masking.** [`SecretMasker.MaskSecrets`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTLogging/Logging/SecretMasker.cs#L216) finds every match in the original text, merges the ranges that overlap or touch and replaces each merged range once, so secrets that overlap leave no fragment. The executor does the same for values, including overlapping occurrences of one value; the matches of a `regex` hint are the non-overlapping ones of the regex engine, so a pattern cannot make the scan quadratic on a 1 MiB line.
- **Names that are case-insensitive.** Workflow command properties ([`ActionCommand.cs:34`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Common/ActionCommand.cs#L34)) and action inputs ([`PipelineTemplateConverter.cs:174`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateConverter.cs#L174)) are looked up without regard to case. `StepContext::inputs` is an `Inputs` map with that lookup and keeps the casing of the first write.
- **Step environment and condition.** The step `env` is evaluated, and merged into the `env` context, before the condition and the display name ([`StepsRunner.cs:108-132`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L108-L221)); a step whose `env` does not evaluate fails whatever its condition says.
- **Runner debug.** `runner.debug` is `1` (and `RUNNER_DEBUG` with it) when the job variable `ACTIONS_STEP_DEBUG` parses as true ([`JobRunner.cs:160-163`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobRunner.cs#L160-L163), [`ExecutionContext.cs:1086`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ExecutionContext.cs#L1086)); the `vars` fallback is not read because `vars` is outside the expression surface.
- **Secrets context.** `secrets` is built from the secret job variables only, other than `system.accesstoken` and `system.github.token`; a `secrets` entry in the message `ContextData` is replaced ([`ExecutionContext.cs:1009`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ExecutionContext.cs#L1009), [`Variables.cs:160-173`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Variables.cs#L160-L173)). A value that only `ContextData` carries is not a registered secret and would reach the log unmasked, so it is not read.
- **Timeouts.** A step that times out gets the log line and an error annotation with the same text; upstream's annotation reads `The action '<name>' has timed out after N minutes.` ([`StepsRunner.cs:328`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L328)).
- **Command recognition and annotations.** A line is a command only if its name is registered, for the `::` form and the `##[` form alike ([`ActionCommand.cs:73-97`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Common/ActionCommand.cs#L73-L97)), so an unregistered `::name::` prefix does not hide a later `##[add-mask]`. Line and column properties are normalized as [`ValidateLinesAndColumns`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionCommandManager.cs#L720-L772) does, and a `file` below the workspace is made relative with `/` separators ([`ActionCommandManager.cs:672-703`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionCommandManager.cs#L672-L703)); the `repo` property is not carried because `Annotation` has no such field. A lone `\r` ends an output line like `\n` and `\r\n` do (`StreamReader.ReadLine`).
- **Failures of a step.** An error that ends a step before or after its process ran (evaluation, setup, an adapter that returns `Err`) is logged and annotated, and the file commands the step wrote are still applied, as `ActionRunner` does in its `finally` block ([`ActionRunner.cs:280-282`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionRunner.cs#L280-L282)). `github.action_repository`, `github.action_ref` and the `GITHUB_ACTION_*` variables follow the current step and are absent for steps that are not repository actions ([`ActionRunner.cs:127-139`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionRunner.cs#L127-L139)).
- **Job environment and outputs.** Each `EnvironmentVariables` token is evaluated with the `env` context of the tokens before it ([`JobExtension.cs:216-226`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L216-L226)). `JobOutputs` names are case-insensitive and the last value wins ([`PipelineTemplateEvaluator.cs:228-250`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateEvaluator.cs#L228-L250)). Admission skips a step with `enabled: false`. A workspace or temp path that is not valid UTF-8 fails the job before any step instead of being exported lossily; the encoder length thresholds count UTF-16 code units as C# `String.Length` does.
- **Timeout, stop token, job outputs, expression length.** A `timeout-minutes` value that is not a number is annotated with `An error occurred when attempting to determine the step timeout.` and the step runs without a timeout, as upstream does ([`StepsRunner.cs:296-310`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L296-L310)); a timeout that fails to evaluate is annotated the same way, followed by the evaluation error, and the step runs without one; zero or a negative number is no timeout. A fractional number of minutes is accepted and kept, which deviates from upstream, where the converter reads the value as an `Int32` and truncates it ([`PipelineTemplateConverter.cs:220-221`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateConverter.cs#L220-L221)). A stop-commands token is matched one character at a time with .NET's `OrdinalIgnoreCase` table, which ICU fills with the simple (one-to-one) uppercase of every UTF-16 unit ([`GlobalizationNative_InitOrdinalCasingPage`](https://github.com/dotnet/runtime/blob/main/src/native/libs/System.Globalization.Native/pal_common.c)): a character whose uppercase form is more than one character, such as `ß`, stays as it is, so `straße-token` is not resumed by `strasse-token`; U+0131 and U+017F map to themselves (the table switches off the Turkish I and long s rules), so `ıd-token-xyz` is not resumed by `id-token-xyz` and `ſecret-token` is not resumed by `secret-token`; and the 27 Greek letters with an iota subscript (U+1F80-1F87, U+1F90-1F97, U+1FA0-1FA7, U+1FB3, U+1FC3, U+1FF3) map to the simple uppercase of UnicodeData, so `ᾳbcdefgh` is resumed by `ᾼBCDEFGH`. A job output that does not evaluate fails the job with an error annotation. The 21,000 expression length limit counts UTF-16 code units ([`ExpressionParser.cs:443`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTExpressions2/Expressions2/ExpressionParser.cs#L443)).
- **Known limits.** `regex` mask hints use `regex-automata`, so .NET-only syntax such as look-behind makes the job fail before step 1 instead of masking: the failure is closed and loud, and a .NET-compatible engine would be a new dependency that #2893 excluded. A cancel that arrives after the last step has started finalizing does not change the job result, which is derived from the steps as in `JobRunner`.
- **Step outputs.** `steps.<id>.outputs` is a case-insensitive dictionary in which the last write wins and the casing of the first write is kept ([`StepsContext.cs:80-81`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsContext.cs#L80-L81), [`DictionaryContextData.cs:113-126`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTPipelines/Pipelines/ContextData/DictionaryContextData.cs#L113-L126)). Environment variable names stay case-sensitive, as on Linux.
- **Step environment.** A step process starts from an empty environment. It receives `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL`, `LC_CTYPE`, `LC_COLLATE`, `LC_MESSAGES`, `LC_MONETARY`, `LC_NUMERIC`, `LC_TIME` and `TMPDIR` from the runner, then the job and step environment, the runner variables and the file-command paths. Every process an adapter starts goes through `StepContext::run_process` or `StepContext::capture` and gets the same treatment. `HOME` is the host's, so `run_job` is not a sandbox: it must not run an untrusted job on the owner's machine until the sandbox user exists.
- **Process control.** Each step process leads its own process group. Timeout and cancel send SIGTERM to the group, wait up to 5 s for the group to empty and then send SIGKILL to what is left, also when the leader has already exited; job end and dropping the job use SIGKILL. A process that leaves the group (`setsid`, `setpgid`, or a shell with job control, `set -m`, which gives each pipeline a group) is not reached; actions/runner finds those by the `RUNNER_TRACKING_ID` environment marker instead ([`JobExtension.cs:588-594`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L588-L594), [`:893-926`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L893-L926)). Group ids are not protected against reuse: a group whose members all exited is dropped from the list when the next process is tracked, but one that exits after that check and whose id an unrelated new group takes before job end would be signalled. The window is one job and needs the pid space to wrap inside it; the risk is accepted.
- **Cancellation.** `velnor-runner exec` is a Velnor-only local command, so SIGTERM, SIGINT and SIGHUP there are a job cancel; a second signal drops the job future, which kills the step process groups, and exits 130. Exit codes: 0 the job succeeded, 1 it failed or was cancelled, 2 a usage error or a message whose `MessageType` is not `PipelineAgentJobRequest` (compared without regard to case, [`Runner.cs:661`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Listener/Runner.cs#L661)), 130 a second signal. `velnor-runner run` (the JIT runner, #2958) follows upstream instead: Ctrl-C and SIGTERM are a runner shutdown ([`Terminal.cs:42`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Common/Terminal.cs#L42), `ShutdownRunner`), where the job is Failed, not Canceled, the step that is running is cancelled even when its condition would survive a cancel, and no later step condition is evaluated, so every later step is skipped ([`StepsRunner.cs:130-215`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L130-L215)); the code of `run` is not part of #2893. For a job cancel `adapter::invoke` races the adapter against the step's stop signal and the step timeout: it fires the adapter's own token first, so its processes get SIGTERM then SIGKILL, and drops an adapter that has not returned after 6 s. A step that starts after the cancel runs without a stop signal. A step that starts before it has its condition re-evaluated against live values when the cancel fires, as upstream does ([`StepsRunner.cs:157-181`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L157-L181)): a step whose condition is still true (`always()`) keeps running and only the deadline applies, any other step is cancelled. A step that ends on its deadline while the job is cancelled is Cancelled, with no timeout line and no `continue-on-error`, because upstream reports a timeout only when the job token has not fired ([`StepsRunner.cs:322-336`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L322-L336)).
- **Output limits.** A step's output line is read up to 1 MiB plus 64 KiB. The mask ranges are found over everything that was read, and the raw text is cut at 1 MiB with a marker; a range that crosses the cut is masked whole, so a secret that straddles the cut or the read bound leaves no fragment. The rest of the line is read and discarded. A secret longer than 64 KiB that straddles the read bound is not covered. `StepContext::capture` returns at most 8 MiB of standard output and fails with `printed more than 8388608 bytes` beyond that. Reads are bounded to 64 KiB per piece, so one line without a newline cannot grow memory without limit. After the leader exits, its remaining output is read until the streams close, until the executor has spent 2 s in total waiting for more (the time the sink takes to handle a line does not count), or until 66 MiB (the channel and two pipes at their largest) have been read. The step timeout and a job cancel also end that wait: the step then ends as timed out or cancelled and its process group is killed. A descendant that holds the pipe open is cut off by these bounds.
- **Runner OS.** `runner.os` and `RUNNER_OS` are always `Linux`, and `runner.arch` follows the build target with the same `X86`, `X64`, `ARM` and `ARM64` mapping as the broker query (`runner_architecture`); any other architecture fails the job before step 1. The executor stands in for the Linux runner that the job message addresses, so a job gives the same expression results on the owner's Mac as in CI; actions/runner derives the name from the host ([`JobRunner.cs:165`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobRunner.cs#L165)).
- **Expression surface.** The step conditions and templates use the fixed allow-list of the expression engine: `github`, `runner`, `needs`, `steps`, `inputs`, `secrets`, `env`, and the functions `success`, `always`, `cancelled`, `format`, `contains`, `startsWith`, `hashFiles` and `toJSON`. `vars`, `matrix`, `strategy`, `job` and `failure()` fail the step with `unsupported expression ...`; adding one is a decision for the owner, not a change of the executor.
- **Value encoders.** Every masked value is also masked in the eleven forms that [`HostContext.cs:103-113`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Common/HostContext.cs#L103-L113) registers: three Base64 shifts, command-line, expression, JSON, URI data, XML, double-quote trimmed and the two PowerShell ampersand sections ([`ValueEncoders.cs`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTLogging/Logging/ValueEncoders.cs)).
- **Workflow commands.** `::name::` and the legacy `##[name]` form are both read ([`ActionCommand.cs:51-189`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Common/ActionCommand.cs#L51-L189)). `stop-commands` suspends every command until `::<token>::` and the state ends with the step ([`ActionCommandManager.cs:60-154`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionCommandManager.cs#L60-L154)); a token that is empty, `pause-logging` or a command name fails the step, and the `ACTIONS_ALLOW_UNSECURE_STOPCOMMAND_TOKENS` opt-in is not honoured. A step keeps at most 10 annotations per level and each message is cut at 4096 characters ([`ExecutionContext.cs:796-841`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ExecutionContext.cs#L796-L841)). Commands other than `add-mask`, `error`, `warning`, `notice` and `stop-commands` are output.
- **Step context.** Before each step `github.action` and `GITHUB_ACTION` take the step name and `event.json` is written again ([`StepsRunner.cs:118`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L118), [`ActionRunner.cs:141`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ActionRunner.cs#L141)). `run` steps get `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN` from the `SystemVssConnection` endpoint when it has `GenerateIdTokenUrl` ([`ScriptHandler.cs:314-319`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Handlers/ScriptHandler.cs#L314-L319)); `ACTIONS_ORCHESTRATION_ID`, `ACTIONS_RUNTIME_*`, `ACTIONS_CACHE_URL` and `ACTIONS_RESULTS_URL` belong to the live runner and the adapters.
- **Continue on error.** The token is evaluated only after the step failed, with the environment the step ran in plus the step's own `GITHUB_ENV` writes, which `FileCommandManager` applies through `SetEnvContext` before `StepsRunner` evaluates the token ([`FileCommandManager.cs:209-210`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/FileCommandManager.cs#L209-L210), [`StepsRunner.cs:353`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L353)); an evaluation error is logged and annotated while the step stays failed ([`ExecutionContext.cs:1422-1450`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/ExecutionContext.cs#L1422-L1450)). The token is read as a Boolean ([`PipelineTemplateConverter.cs:99-111`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateConverter.cs#L99-L111)), so an expression that yields the string `true`, such as `${{ env.ALLOW }}` with `ALLOW=true`, is a conversion error and the step stays failed. A step whose `if` does not evaluate completes as failed and `continue-on-error` is not applied to it, so the job fails ([`StepsRunner.cs:237-241`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L237-L241), [`:353`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L353)).
- **Job defaults.** Every `Defaults` entry is evaluated once at job setup with the job-level contexts, and an entry that has `run` replaces the whole run dictionary of the earlier entries, so a workflow `run.shell` is gone after a job entry that sets only `run.working-directory`; empty values are dropped ([`JobExtension.cs:279-294`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L279-L294)). An entry that does not evaluate fails the job before step 1.
- **Admission and step conditions.** Admission looks at every enabled step without evaluating its `if`: a `uses` that no adapter handles or a literal shell that is not `bash` or `sh` fails the job before step 1 even when the step's condition would be false, where upstream skips such a step (it resolves the shell only when the step runs). This is deliberate: #2893 defines `unsupported: <feature>` as a failure before any step so that no partial run with side effects precedes the rejection, and a condition can read `steps` or `env` values that exist only later. Expression shells are the exception and are checked when their step runs.
- **Shell.** A custom shell is split into arguments with the rules of .NET's `ProcessStartInfo.Arguments` (double quotes group, `\\"` escapes), and `{0}` is replaced in each argument afterwards, so a path with spaces stays one argument. Admission checks only a literal `shell` of a step and the evaluated `defaults.run.shell`, and rejects one that is not `bash` or `sh` or that has no `{0}` after the built-in expansion (upstream throws `Invalid shell option` when the step runs, [`ScriptHandler.cs:242-246`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Handlers/ScriptHandler.cs#L242-L246)). A shell given as an expression is never evaluated at admission, because it can read the step environment, `steps` outputs or `GITHUB_ENV` writes that exist only later; it is checked when its step runs, and an unsupported one fails that step with `unsupported: shell …` before its script is written or started. An annotation message is cut at 4096 UTF-16 code units. A step with `background: true` is rejected at admission: the executor has no background coordination ([`BackgroundStepCoordinator.cs`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/BackgroundStepCoordinator.cs)).
- **File commands.** On Linux and macOS only `\n` ends a line of `GITHUB_ENV` or `GITHUB_OUTPUT`; `\r\n` is a Windows rule ([`FileCommandManager.cs:470-505`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/FileCommandManager.cs#L470-L505)), so a trailing `\r` stays in the value. `GITHUB_PATH` entries are taken verbatim, empty lines apart, and a line ends at `\n`, `\r\n` or a lone `\r`, as `StreamReader.ReadLine` ends it; the executor splits a step's output lines the same way ([`FileCommandManager.cs:127-158`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/FileCommandManager.cs#L127-L158)). `GITHUB_ARTIFACTS` and `GITHUB_ARTIFACTS_LIST` are not provided: upstream processes them only when the `actions_runner_allow_artifacts_file` feature flag or `ACTIONS_RUNNER_ALLOW_ARTIFACTS_FILE` is set ([`CreateArtifactsFileCommand.cs:46-79`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/CreateArtifactsFileCommand.cs#L46-L79)). The summary is read at most 1 MiB plus one byte and dropped above 1 MiB, and a summary whose reported length is zero is skipped without an annotation, so a link to `/dev/zero` is skipped ([`FileCommandManager.cs:236-260`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/FileCommandManager.cs#L236-L260), [`:240`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/FileCommandManager.cs#L240)). Upstream reads `GITHUB_ENV`, `GITHUB_OUTPUT` and `GITHUB_PATH` without a bound; the executor opens every file-command file with `O_NONBLOCK`, reads it only if it is a regular file and at most 1 MiB, and fails the step with an error annotation for a FIFO, a device or a larger file. Step scripts, file-command files and `event.json` are created `0600`, and the temp directory, `_runner_file_commands`, `step_summary` and `_github_workflow` `0700`; upstream uses the default umask.
- **Native `actions/checkout`.** `velnor-runner` runs `actions/checkout@*` itself, from a bare mirror that its store broker keeps under the store root, and never starts the upstream action. Admission refuses the job before step 1 when the job message gives no usable trusted facts (for example a missing `repository_id`), when an input other than `fetch-depth` and `persist-credentials` is declared, when `persist-credentials` is not `false`, or when `fetch-depth` is `Infinity`. `fetch-depth` is read as [actions/checkout](https://github.com/actions/checkout/blob/v7.0.1/src/input-helper.ts#L122-L126) reads it, as `Math.floor(Number(value))` with an empty value meaning 1, so `2.9`, `1e2` and `0x2` are numbers, and a negative or non-numeric value means the whole history. Declared input names fold case as the case-insensitive names bullet above says, and the last declaration decides, so admission and the step read the same value. A `server_url` with credentials, a query or a fragment is refused, because the clone address is the URL plus `/<owner>/<repository>.git`. The workspace reads objects through `objects/info/alternates`, and its `.git/config` holds no token. A checkout that fails after the mirror looked complete asks the broker for one repair and then retries once, whatever the repair answers, because another job may have repaired the mirror first. The broker's 10-minute repair cap starts after it holds the mirror lock. A `Sound` answer is cached for 24 hours per commit and window, and a cached answer still has the commit's own objects verified; any refetch discards every cached answer. The cache is read and written only when the failed step was `read-tree` or `checkout-index`; a failure of any other step scans the whole window. A 64-character commit id (a SHA-256 repository) is refused at admission, because the mirror is a SHA-1 repository. A git step that is cancelled or dropped kills its whole process group, so no helper of it reaches a later step. The broker keeps in the mirror only what it owns. When it fetches or repairs, it removes whatever it does not own at the top of the mirror, in `objects/` and `objects/pack/`, and in `refs/`, never following a link: a pack counts only when both its `.idx` and its `.pack` are regular files, an `objects/pack` that is not a directory is replaced, and a file under `refs/` stays only when it holds a 40- or 64-digit object id or a `ref: refs/` line, while every lock, link, pipe and directory posing as a lock goes. The loose-object directories are listed only when the previous job did not end cleanly: the broker writes a `sweep-needed` marker when it takes the mirror lock and removes it after a job that succeeded, and a leftover marker makes the next job keep only regular files in them; a marker that is not a regular file is removed without following it before the broker writes a new one. A pipe or device planted inside the mirror is outside this: the step's own presence probe reads the mirror before the broker runs, and planting one needs write access to the mirror as the runner user, which #2911 contains. The broker's git processes inherit the mirror lock, so a git that outlives a killed runner keeps the lock until it exits, and a cleanup that runs on the blocking pool keeps the lock until it returns. Every broker fetch also gives up on a transfer slower than 1 byte per second for 600 seconds (`http.lowSpeedLimit` and `http.lowSpeedTime`), and the ref update of a repair inherits the lock too, so an orphaned git lives only as long as its transfer. A fetch that takes longer than 30 minutes is stopped with its process group and leaves the marker.

### Local stand-in registration and cache protocol

These facts hold for runner v2.338.0 ([`197ca027`](https://github.com/actions/runner/tree/197ca027b90de199464dee3bb9ff198a1db60967)), the version in `.velnor/runner.lock`, and for `actions/cache` [`55cc8345`](https://github.com/actions/cache/tree/55cc8345863c7cc4c66a329aec7e433d2d1c52a9). `local/` depends on them. Check them again when either pin changes.

- [`UrlUtil.IsHostedServer`](https://github.com/actions/runner/blob/197ca027b90de199464dee3bb9ff198a1db60967/src/Runner.Sdk/Util/UrlUtil.cs) returns `false` when `GITHUB_ACTIONS_RUNNER_FORCE_GHES` is true. Otherwise it returns `true` for `github.com`, `www.github.com`, `github.localhost`, `*.ghe.localhost` and `*.ghe.com`.
- [`ConfigurationManager`](https://github.com/actions/runner/blob/197ca027b90de199464dee3bb9ff198a1db60967/src/Runner.Listener/Configuration/ConfigurationManager.cs) registers a hosted server at `{scheme}://api.{host}/actions/runner-registration`, and any other server at `{scheme}://{host}/api/v3/actions/runner-registration`. A stand-in at `github.localhost` that serves only the second route therefore needs `GITHUB_ACTIONS_RUNNER_FORCE_GHES=1` on the runner.
- [`ActionManager`](https://github.com/actions/runner/blob/197ca027b90de199464dee3bb9ff198a1db60967/src/Runner.Worker/ActionManager.cs) deletes the actions directory at the start of every job, so each job downloads its actions again. When `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` names an existing directory, the runner first copies `<owner>_<repo>/<sha>.tar.gz` from it, with `/` and `\` in the owner and repository name replaced by `_`. The runner never writes to that directory.
- The v2 cache client of toolkit [`0786132e`](https://github.com/actions/toolkit/tree/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src), bundled in that `actions/cache`, makes three calls. [`cacheTwirpClient.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/shared/cacheTwirpClient.ts) posts JSON to `/twirp/github.actions.results.api.v1.CacheService/<Method>` under `ACTIONS_RESULTS_URL` with the runtime token as `Authorization: Bearer`. It parses every answer as JSON, and retries up to five attempts when the body is not JSON or the status is 500, 502, 503 or 504. [`cache.twirp-client.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/generated/results/api/v1/cache.twirp-client.ts) writes proto field names (`restore_keys`, `size_bytes`) and ignores unknown fields in answers.
- [`cache.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/cache.ts): `restoreCacheV2` sends `GetCacheEntryDownloadURL {key, restore_keys, version}`, treats `ok: false` as a miss and reports a restore-key hit when `matched_key` differs from `key`. `saveCacheV2` sends `CreateCacheEntry {key, version}`; on `ok: false` it logs `Cache reservation failed: <message>` as a warning, then `Failed to save: Unable to reserve cache …` as info, and the step succeeds. It then uploads to `signed_upload_url` with the Azure blob client (128 MiB single-shot limit, 64 MiB blocks, 8 at a time) and sends `FinalizeCacheEntryUpload {key, version, size_bytes}` with the size as a string, reading `entry_id`.
- The Azure blob client bundled in `actions/cache` `dist/` (`@azure/storage-blob` 12.32.0) accepts only `201` for a whole-blob `PUT` (`x-ms-blob-type: BlockBlob`), a `PUT ?comp=block&blockid=<base64>` and the closing `PUT ?comp=blocklist`, whose XML body lists `<Latest>` block ids in order. For a host that is neither `*.blob.*` nor an IP or `localhost`, `getBlobAndContainerNamesFromUrl` takes the container and blob from the first path segments, so the path needs at least `/<container>/<blob>`.
- [`cacheHttpClient.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/cacheHttpClient.ts) downloads from a host that does not end in `.blob.core.windows.net` with one plain `GET`. [`downloadUtils.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/downloadUtils.ts) aborts when the socket is idle for `SocketTimeout`, 5000 ms ([`constants.ts`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/constants.ts)), and fails when the file size differs from `Content-Length`.
- `runner.server` [`992ccbbb`](https://github.com/ChristopherHX/runner.server/tree/992ccbbbf9afcde477c38c316e053b1af457ad40) signs each job's runtime token with RS256 and puts `repository`, `ref` and `defaultRef` (`refs/heads/` plus the payload's `repository.default_branch`, else `main`) in it ([`MessageController.cs`](https://github.com/ChristopherHX/runner.server/blob/992ccbbbf9afcde477c38c316e053b1af457ad40/src/Runner.Server/Controllers/MessageController.cs)). The key is random per start unless `Runner.Server:RSASignatureKeyPath` names a PEM file ([`Startup.cs`](https://github.com/ChristopherHX/runner.server/blob/992ccbbbf9afcde477c38c316e053b1af457ad40/src/Runner.Server/Startup.cs)). It sets the job's `ResultsServiceUrl` and `CacheServerUrl` to the scheme and `Host` header of the runner's request unless `Runner.Server:ServerUrl` is set ([`VssControllerBase.cs`](https://github.com/ChristopherHX/runner.server/blob/992ccbbbf9afcde477c38c316e053b1af457ad40/src/Runner.Server/Controllers/VssControllerBase.cs)), so a server that owns the runner's host name and forwards the other requests with their `Host` header receives every cache call.
- In `actions/cache`, `isGhes()` reads the host of `GITHUB_SERVER_URL`. Only `github.com`, `*.ghe.com` and `*.localhost` count as GitHub. For any other host, `getCacheServiceVersion()` returns `v1`. Otherwise it returns `v2` when `ACTIONS_CACHE_SERVICE_V2` is set, and the runner's [`NodeScriptActionHandler`](https://github.com/actions/runner/blob/197ca027b90de199464dee3bb9ff198a1db60967/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs) sets that variable for Node actions when the job variable `actions_uses_cache_service_v2` is true.

### Workflow YAML sharing

GitHub's official [workflow reuse reference](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#yaml-anchors-and-aliases) documents same-document environment mapping aliases and an entire job alias containing a steps sequence. These are YAML node substitution, not a reusable
workflow or composite-action call: expressions and environment values retain
their use-site job and step context. The [official announcement](https://github.blog/changelog/2025-09-18-actions-yaml-anchors-and-non-public-workflow-templates/)
records workflow anchor/alias support.

Velnor shares the released helper launcher and verification selectors as exact step `env` scalar aliases. Its other workflow sharing applies only when needed to fit the unchanged
500,000-byte transport limit: exact job `env`, step `env`/`with`, whole step mapping, and existing `run` scalar reuse. It never moves execution, credentials,
permissions, dependencies, caches or outputs into a different scope. Definitions precede uses in one globally unique document namespace. Staged shell validation
normalizes only those emitted forms before its existing ancestry, effective-shell and ShellCheck checks. Input is bounded to 500,000 bytes, nesting to 32 levels,
and normalization work to 262,144 nodes and 16 MiB; merge keys, tags on shared nodes, forward/cyclic references and other alias scopes fail closed.

The launcher remains a workflow-owned step environment value, including on `always()` steps. The official runner’s [`StepsRunner`](https://github.com/actions/runner/blob/main/src/Runner.Worker/StepsRunner.cs#L96-L114) copies the global environment first, then overwrites it with evaluated step environment values. A persisted `GITHUB_ENV` value therefore cannot replace the launcher or its verification selectors. The launcher hashes each fresh helper copy through `/usr/bin/env -i PATH=/usr/bin:/bin`, so Perl startup variables cannot change macOS `shasum` execution. The installed helper directory stays owner-writable (`0700`) for runner cleanup; the helper file is executable and owner-read-only (`0500`).

This Velnor admission does **not** extend to local action manifests. The original
runner's [legacy `YamlObjectReader.EvaluateCurrent`](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/YamlObjectReader.cs)
rejects scalar/mapping/sequence anchors and alias events, but
[`ActionManifestManager` at runner v2.337.0](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/ActionManifestManager.cs)
loads action manifests through the separate `GitHub.Actions.WorkflowParser`
reader. The legacy implementation alone does not prove that package's behavior.
Action-manifest alias compatibility is not qualified by this workflow-only
change; Velnor emits no new aliases into composite `action.yml` files.

1. **Protocol Fidelity Over Assumptions**:
   - `actions/runner` and `actions/scaleset` define the exact wire protocols, message shapes, and validation behaviors.
   - Any runner implementation or generator assumption must be verified against the official C#/Go source code in these repositories.

2. **Always Check Original Source Code**:
   - When resolving ambiguous runner behavior, environment variable precedence, or error handling semantics, inspect the official source code directly.
   - Do not rely on third-party summaries or stale documentation when the official source is available.

3. **Traceability in Design & Implementation**:
   - Every protocol decision, compatibility shim elimination, or runner contract in `crates/velnor-runner-*` must cite the specific source file and mechanism in `actions/runner` or `actions/scaleset`.
   - Reference updates and version bumps must be accompanied by compatibility verification against updated upstream sources.

---

## GitHub Actions & Dependabot Platform Contracts

### 1. Native Step Concurrency (`background`, `wait`, `wait-all`, `cancel`, `parallel`)

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-06-25-actions-steps-can-now-be-run-in-parallel/), [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepsbackground), `actions/runner` v2.335.0+; pinned runner v2.338.0 `src/Runner.Worker/BackgroundStepCoordinator.cs`, `JobExtension.cs`):
  - `background: true` on a `run` or `uses` step starts it asynchronously; the job continues to the next step. A composite action may run as a background step but cannot declare background steps or `parallel:` internally.
  - `wait` (one id or a list) and `wait-all` (no arguments) block until the referenced or all active background steps finish; neither accepts `if:` and both always run. Outputs and environment changes of a background step become visible only after a wait that includes it.
  - A failed background step fails the next `wait`/`wait-all` that includes it (unless the step sets `continue-on-error`); the coordinator merges member results into the wait step's result, so later default-`success()` steps are skipped while `always()` steps still run. An implicit `wait-all` runs before post-job cleanup for unjoined members.
  - Post steps run only after every regular step. In `src/Runner.Worker/StepsRunner.cs` (v2.338.0, tag commit `197ca027b9`, lines 57-74) the loop runs while `JobSteps` is non-empty; only when it is empty does it run the safety-net `WaitForUnwaitedStepsAsync` and then move the `PostJobSteps` stack onto the queue. An action's post step is registered by `ExecutionContext.RegisterPostJobStep` in `src/Runner.Worker/ActionRunner.cs:136`. So a composite or JavaScript action's post handler (for example `jdx/mr-boxington-action`, which runs `mbx cache export`) starts after the last regular step, background steps included.
  - Background steps are linked to the job cancellation token, so job `timeout-minutes` and run cancellation cancel them. `cancel` terminates one member (SIGTERM, then SIGKILL after a 7.5 s grace).
  - At most 10 background steps of one job run concurrently (`DefaultMaxBackgroundSteps = 10`); additional members queue until a slot is free.
  - `parallel:` is sugar for a group of background steps followed by a wait.
- **Velnor Implementation**:
  - `Step::parallel` in `velnor-actions-contract` marks a member of a contiguous group; members may not carry an id, a typed role or an internal operation. `velnor-actions-workflow-renderer` (`parallel_steps`) renders each member with `background: true` and joins each group with one generated `Wait for parallel checks` `wait-all` step; composite renderers, lane sharing and obligation extraction reject or skip parallel members.
  - Each Rust crate job uses one such group for its checks (see the former hard decision 1, removed in #608, and [native Rust crate jobs](../implemented/native-rust-crate-jobs.md)).
  - Pinned `jactionlint` v2.3.0 parses and validates `background:` members and the body-less `wait-all` join natively (rejecting `background: "yes"`, a `wait-all` with `run:`, and a step with neither `run` nor `uses`), so generation-time and CI lint pass no ignore pattern ([hard decision 10](../jactionlint-decision.md)). The superseded actionlint v1.7.12 could not parse the keys (rhysd/actionlint#693). Pinned `zizmor` v1.31.1 (support since v1.27.0) parses the syntax.

### 2. Workflow Execution Protections

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-17-actions-new-workflow-execution-protections/)):
  - GitHub separates general workflow execution permissions from **restricted untrusted-actor triggers** that can run workflow code on a privileged ref or react to untrusted public events:
    - `pull_request_target`
    - `issue_comment`
    - `issues`
    - `discussion`
    - `discussion_comment`
    - `fork`
    - `gollum`
    - `label`
    - `public`
    - `watch`
    - `workflow_run`
  - Public repositories default to blocking workflows triggered by `pull_request_target`, `issue_comment`, `issues`, `discussion`, and `discussion_comment` from non-collaborators unless explicitly approved or relaxed by repository/organization policy.
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `ALLOWED_WORKFLOW_EVENTS` (`pull_request`, `push`, `merge_group`, `workflow_dispatch`, `schedule`), `RESTRICTED_ACTION_EVENTS` (the 11 restricted triggers above), and `DEFAULT_BLOCKED_PUBLIC_REPO_EVENTS`, along with `is_allowed_workflow_event`, `is_restricted_action_event`, `validate_workflow_trigger_event`, and `validate_workflow_trigger_events`.
  - Velnor's `Trigger` IR is structurally disjoint from `RESTRICTED_ACTION_EVENTS`: generic generated workflows never emit `pull_request_target`, `workflow_run`, or comment/issue triggers, ensuring public-repository forks execute under `pull_request` with read-only permissions and no secrets without requiring manual maintainer approval for restricted triggers.

For the private Bun registry fence, the [runner expression implementation](https://github.com/actions/runner/blob/5ba50443372004d4d67620909426c5d28a0066af/src/Sdk/DTExpressions2/Expressions2/Sdk/Functions/ToJson.cs) serializes a Boolean as `true` or `false`, null as `null`, and a string as a JSON string. The generated first step compares the serialized fork value to `false` for `pull_request`; other values fail. GitHub's [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_iddefaultsrun-shell) specifies `bash -e` on Linux and macOS when no shell is configured, with `sh -e` as fallback. This fence assumes the reviewed workflow trust model documented in [fork-PR trust model](../implemented/fork-pr-trust-model.md).

### 3. Cache Access Control (`cache-mode`)

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode)):
  - Workflows and jobs can declare `cache-mode` at the workflow root or per job (`jobs.<job_id>.cache-mode`) to enforce runtime-level GitHub Actions cache service permissions:
    - `write`: read and write cache entries (default on standard events).
    - `read`: permit cache restores and deny saves at the service boundary; pinned-client handling is specified below.
    - `write-only`: write cache entries without restoring existing entries.
    - `none`: disable all cache reads and writes.
  - `cache-mode` does not accept `${{ }}` expressions: a workflow file with one is rejected by GitHub (a run with zero jobs and the file path as its title, velnor-lab#383, probe-cache-mode-expr.yml), as the workflow-syntax and dependency-caching pages imply by listing only the four literals. Details and the enforcement and default rules are in entry 5 below.
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `CacheMode` (`Read`, `Write`, `WriteOnly`, `None`, `Expression(String)`) and optional `cache_mode: Option<CacheMode>` on both `WorkflowIr` and `Job`, with `CacheMode::for_pull_request_policy(PullRequestCachePolicy)` mapping `PullRequestCachePolicy::ReadOnly` to `CacheMode::Expression("${{ github.event_name == 'pull_request' && 'read' || 'write' }}")` and `PullRequestCachePolicy::SharedSeed` to `CacheMode::Write`.
  - `velnor-actions-workflow-renderer` emits top-level and job-level `cache-mode:` when `cache_mode` is `Some(...)` and omits it when `None`.
  - Pinned `jactionlint` v2.3.0 parses workflow- and job-level `cache-mode` literals (`read`, `write`, `write-only`, `none`) and rejects any other value, including a `${{ }}` expression; `JactionlintCapabilities::check_step_syntax(StepSyntax::CacheMode)` records the capability and generation vets it.

### 4. Dependabot Default Package Cooldown & Velnor Freshness

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-07-14-supply-chain-defense-by-default-dependabot-adds-3-day-package-cooldown/)):
  - Dependabot applies a **3-day default cooldown** (`cooldown.default-days: 3`, configurable from `1` to `90` days) to newly published package versions for version updates, reducing exposure to compromised releases during the initial publication window while keeping **security updates immediate**.
- **Velnor Implementation**:
  - `velnor-actions-freshness` supports an optional `default_cooldown_days` integer header key in `.velnor/version-policy.toml` (`DEFAULT_COOLDOWN_DAYS = 3`, bounded `MIN_COOLDOWN_DAYS = 1` to `MAX_COOLDOWN_DAYS = 90`).
  - When `default_cooldown_days` is omitted from `.velnor/version-policy.toml`, the freshness policy header check defaults to `3` days; values outside `1..=90` or non-integers fail the `policy-header` gate.
  - Security advisory scanning via `cargo-deny` remains immediate and is never delayed by `default_cooldown_days`.

### 5. Cache rules verified against original sources

Client behavior below is bound to [`actions/cache` at `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`](https://github.com/actions/cache/tree/55cc8345863c7cc4c66a329aec7e433d2d1c52a9), which bundles `@actions/cache` 6.1.0, and the corresponding [toolkit source at `0786132e6a1a4451c2d392bbbf481c2b172d4312`](https://github.com/actions/toolkit/tree/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src).
Platform behavior comes from GitHub's [dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching), [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax), [Actions limits](https://docs.github.com/en/actions/reference/limits) and [REST cache API](https://docs.github.com/en/rest/actions/cache). Requalify client behavior when its pin changes and verify current service limits before relying on them.

#### 5.1 Storage limits, eviction and retention

- GitHub defaults to 10 GB per repository and seven days without access; administrators can configure storage and retention. Storage above the free allowance is billed. Velnor's adopted quota remains binding regardless of paid platform expansion.
- Eviction removes least-recently-accessed entries. An over-limit save can succeed before eviction; successful saving therefore does not prove sufficient admission headroom.
- Billing-budget exhaustion can make caches read-only. Never assume a larger configured quota guarantees a successful write.
- The REST API exposes storage- and retention-limit controls. Resolve their documented routes and permissions before implementing administration; do not infer them from the cache data-plane API.

#### 5.2 Immutable entries and save ownership

- Existing cache contents cannot be updated. Identity includes key, version and scope; the same key can identify distinct entries in different scopes or versions.
- The pinned action [skips saving an exact primary-key hit](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts). Concurrent reservation refusal is a nonfatal save outcome; it does not prove which producer supplied complete state.
- The [version hash](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/cacheUtils.ts) includes paths, compression, platform-specific settings and a salt. Review writer ownership against the complete identity.
- The combined action defaults to [`post-if: success()`](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/action.yml); `save-always` is deprecated. Use a separately guarded save when successful production must survive unrelated later validation failures.
- Velnor save protocol: the store broker, which alone holds store write rights, creates a pipe and an upload directory `incoming/<broker id>/<n>` (`incoming/<broker id>` is 0700 and mounted by no sandbox; the `<n>` directory inside it is 0755, and the data file is frozen to 0444 before it is synced, so a published entry is never unreadable to the step uid). `BeginCache {key, version}` answers `Ready` and hands out the write end of the pipe (over `SCM_RIGHTS` between processes, a `PipeWriter` in the same process); fork and non-writing events are denied before `Ready`. The packer writes the archive into the pipe, then sends `SealCache {bytes}`, or `AbortCache`. The broker writes one inode it created, reads at most the lower of the toolkit limit (10 GiB) and the owner's budget, plus one byte, compares the sealed byte count with what arrived, syncs it, and only then takes the scope lock and the store lock. A publish that its job abandoned (a cancelled job) is re-checked after the scope lock, after the store lock and before any eviction, and again before the commit rename, so a cancelled job neither evicts live entries nor installs its entry. Under them it writes `key`, `version`, `accessed` and `complete` (in that order, each a 0444 file, each synced) into the staged directory, syncs the directory, and renames it into the scope; the rename is the commit point, so an entry is visible in the scope only when it is complete and a crash at any point leaves either no entry or a whole one. Only after the rename does it freeze the directory to 0555, because a non-root broker cannot rename a 0555 directory to another parent on Linux. It never copies a staged file, and no descriptor a job holds refers to the published inode: a writer kept after the seal gets `EPIPE`. A dropped upload, an abort, a wrong count and an over-limit stream publish nothing and are removed.
- Velnor reclaims the trees of a killed broker. A broker creates its liveness file under a temporary name in `live/`, takes an `flock` on it and only then renames it to `live/<broker id>`, so the name never exists without a lock behind it and a reclaimer that lists `live/` during a claim cannot win it. A new broker purges each `incoming/<id>` and `aside/<id>` whose liveness file it can lock without blocking (or that has none) and unlinks that file, after checking that the locked descriptor and the name have the same device and inode, so an abnormal exit leaks at most until the next start. A live broker's trees are never touched, and a claim that loses a race with a reclaimer starts again under a new temporary name.
- Velnor verifies the published inode instead of trusting path permissions. The broker renames from the `incoming/<id>` and `<n>` descriptors it kept, then requires the entry to be the directory it streamed into and its `data.tar` to have the device and inode of the descriptor it wrote; otherwise it moves the entry aside and fails the save, which also closes a purge racing a publish. `live/`, `incoming/`, `aside/` and the broker's own `incoming/<id>` and `aside/<id>` must belong to the broker's effective uid with mode 0700, or the broker hands out no pipe. The store root is created 0755. A `cache/`, `v1/`, owner or scope directory is staged as `incoming/<broker id>/chain-<n>`, set to its mode, fsynced and renamed into place without replacing anything (`RENAME_NOREPLACE`, `RENAME_EXCL` on macOS), so a directory never exists in the store with a mode that depends on the umask, a crash cannot keep the rename without the mode, and a crash leaves the staged directory to the reclaim above. The parent of each directory the publish creates is fsynced. `cache/`, `v1/` and the owner directory are 0711 (a step can walk through them but cannot list them, so it cannot enumerate other repositories); the scope directory is 0755, because a restore lists its own scope to match restore keys by prefix, and the entries inside stay readable (0555 directories, 0444 files). A directory the broker owns whose mode differs from that (a crash left it private, or an older store left it wider) is set to the mode on the next save. This hides other repositories from enumeration only: the owner and scope names are hashes of public facts, so a step that computes them still reads that scope, and real isolation needs a mount per scope (#2911).
- Limit of the owner and mode checks: on virtiofs, the deployed store, the guest sees host uids squashed, so uid and mode prove nothing there. The integrity of a publish then rests on the device and inode comparisons above and on `incoming/` never being mounted into a job, a requirement of the deployment. The broker does not refuse a virtiofs store.
- Velnor storage budget: [`docs/goals.md`](../goals.md) adopts at most 8 GB of GitHub's 10 GB, and Velnor reads GB as 8 x 10^9 bytes. The one definition is `velnor_cache_rules::DEFAULT_CACHE_BUDGET_BYTES`, which the broker (`Layout::budget`), `velnor-rust` and `velnor-ci` all read. Each cache owner (GitHub server identity and repository id) has its own budget, and its entries live under `cache/v1/<owner hash>/<scope hash>/<entry>`. The size of an entry is the size of its `data.tar`; incomplete entries are not counted. At publish, after the scope lock and under a second, store-wide lock (`locks/store`, always taken in that order), the broker totals the complete entries of the owner and evicts with the `velnor-cache-rules` `evict` function. That function follows GitHub's [usage limits and eviction policy](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching): it removes entries not accessed in over 7 days, then deletes "in order of last access date, from oldest to most recent" until the total is less than the limit. The broker passes it the entries already stored and the budget minus the size of the new entry, so the new entry is never a candidate: a sibling saved in the same second, or a clock that stepped back, cannot get a save that fits refused. The store therefore holds at most 8 x 10^9 bytes after every publish.
- Velnor deviation from GitHub's eviction: GitHub saves the new cache first and then evicts, which can evict the new entry itself. Velnor cuts an upload off at the lower of the toolkit limit and the budget ("does not fit in the ... byte cache budget"), so no stream puts more than the budget (plus one read) on disk and nothing is evicted for a save that cannot fit; a save of exactly the budget is admitted and evicts every older entry. Otherwise it evicts other entries until the new one fits, and it fsyncs each scope directory it removed an entry from before it renames the new entry in. An evicted entry is renamed into the broker's `aside/` and purged at once, so a restore that already opened its `data.tar` keeps reading it.
- Eviction adds the owner bits to a directory (`mode | 0700`) and never removes the others. The entry directory is published as 0555 and the steps run under another uid, so clearing the group and other bits would make a restore that races the eviction fail with `EACCES` instead of finding the entry gone.
- Velnor records access in the `accessed` file of an entry, whose modification time is the last access. The broker writes it at publish and renews it on `TouchCache {scope, key, version}`, which the runner sends after every restore hit, `lookup-only` included; the renewed stamp is fsynced before the touch is answered. The broker accepts a scope only when its own trusted facts can read it, and the store lock does not serialize a touch, so a touch that races an eviction scan can be lost. The 7-day rule and the order of eviction read this stamp through a clock seam (`Layout::clock`, seconds), which tests drive. A restore lookup applies the same 7-day rule (`velnor_cache_rules::is_expired` against the real clock), so an entry nobody has accessed for more than 7 days is a miss even when no save has evicted it yet. The save decides the same way: the runner's `Cache already exists` check and the broker's check under the scope lock read the entry through the one predicate `live_access` that the lookup and the eviction scan also use, so an entry idle for more than 7 days counts as absent, the save publishes the key again, and its eviction pass removes the old entry under the store lock. An entry also stores its `scope`, and equal access and creation seconds are ordered by scope, key and version as `RULES.md` rule 5d says, not by the hashed directory names. After an eviction the broker removes each scope directory that it emptied, and the scope's lock file, only while it holds that lock without waiting, so a scope that a publish holds is left alone. Every locker checks after `flock` that the lock file still has the name it opened and otherwise opens it again, so an unlinked lock file never gives two holders.
- Velnor takes the repository id of the cache namespace only from the broker's verified facts (a nonzero number, or a decimal string), never from the step's `github` context, so a step cannot move a save into another repository's namespace.
- The cache namespace hash covers the canonical GitHub server identity (scheme, host, port and path of `server_url`, the same identity the git mirror layout uses) as well as the repository id, so github.com and a GitHub Enterprise Server host that reuse one numeric repository id never share a scope directory or a scope lock.

#### 5.3 Restore matching

- Search tries an exact primary key, partial primary-key matches and ordered restore-key prefixes within the current branch before repeating on the default branch. For multiple partial matches, newest means creation time, not last access.
- Matching order is a documented service contract; the client only sends keys and version. A partial match requires payload reconciliation before use.
- The [pinned toolkit 6.1.0 client](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/cache.ts#L81-L93) permits at most ten total keys (`keys.length > 10`) and, in its `checkKey`, at most 512 JavaScript `key.length` units per key (UTF-16 code units, so a non-BMP character counts as two) and no commas. A restore must match the cache version; gzip and zstd entries do not cross-restore.
- `cache-hit` proves only an exact primary-key match, never validity, completeness or current-source validation.

#### 5.4 Cache scope and trust

- Runs can restore current-branch and default-branch caches; pull requests can also restore their base-branch scope. Child, sibling and unrelated tag scopes are isolated.
- Pull-request writes belong to `refs/pull/<number>/merge`, not the default branch, and cannot be restored by the base branch or other pull requests.
- Reuse across later pushes to the same pull request is inferred from its stable merge-ref scope. Require producer/consumer logs proving that path before claiming it works; same-run access does not establish it.
- Fork pull requests can read base/default-branch caches. Never place secrets or private state there, and preserve Velnor's read-only trust policy.

#### 5.5 `cache-mode` and client enforcement

- Accepted literals are `read`, `write`, `write-only` and `none`; a job-level setting overrides its workflow setting. The service enforces the effective mode through scoped tokens.
- Trusted triggers default to `write` and restricted low-trust triggers to `read`. The documented default does not establish `pull_request` behavior; use explicit Velnor policy.
- Pinned toolkit 6.1.0 does not inspect `ACTIONS_CACHE_MODE`. A denied save still attempts reservation and emits a warning; denied restore uses its generic failure path. Do not mistake service denial for skipped client work.
- A reusable workflow cannot exceed its caller's explicit mode; without one, a callee can request `write`. Explicit write access on a restricted trigger restores poisoning risk.
- Expressions are rejected (see entry 3), so a job has one mode for every event; per-event limits belong on the save steps.
- Observed on a same-repository pull request (velnor-lab#383): under `cache-mode: read`, `actions/cache/restore` hit, `actions/cache/save` logged the warning `Failed to save: Unable to reserve cache with key ... cache write denied: token has no writable scopes` and succeeded, and a REST `DELETE /actions/caches` with `actions: write` still deleted an entry. The mode gates the cache service token, not the REST API.

#### 5.6 Compression and archive construction

- The pinned [compression selector](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/cacheUtils.ts) chooses `ZstdWithoutLong` when zstd is available, otherwise gzip. It does not select the legacy `--long=30` variant for new saves.
- [Tar construction](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/tar.ts) uses `zstdmt` on non-Windows and `zstd -T0` on Windows, with no compression-level flag. Qualify the actual zstd default before budgeting it.
- Readers retain support for the legacy `--long=30` format, whose window can require 1 GB. Archive names are `cache.tzst` or `cache.tgz`, and compression participates in version identity.
- Creation uses `tar --posix -P` and a manifest. Absolute path forms are retained; platform-specific flags include delayed directory restore on macOS and GNU tar's `--force-local` on Windows.
- Velnor deviation: GNU tar 1.35 with `--posix -cf ... -P` archives FIFOs and devices, and skips sockets with `socket ignored` (exit 0). Velnor refuses all three at save, because its shared store holds only regular files, directories and links. The refusal of sockets is deliberate: a daemon socket left in a cached directory fails the save, so the directory is never published without it.
- Velnor's "changed while it was archived" check follows GNU tar 1.35 [`create.c`](https://git.savannah.gnu.org/cgit/tar.git/tree/src/create.c?h=release_1.35#n1816): it compares owner, mode and modification time, plus the size of a non-directory, and for a directory looked up again by name also its device and inode. A change of ctime alone (a new hard link, a `chmod` to the same mode) does not fail a save. A symlink is statted again after its target is read and must keep its identity, with a size equal to the target length.

#### 5.7 Extraction and failure handling

- Restore downloads and extracts within one action step using `tar -xf ... -P -C ...`. No user inspection step runs between download and extraction; debug listing is not filtering.
- The pinned extraction path has no expanded-size, entry-count or free-space admission and does not prevent absolute paths or overwriting existing files. Download completeness checks do not provide these protections.
- Cache contents are unsigned. Verify restored paths and executable state before use; a cache hit does not authenticate them.
- Restore errors return a miss after logging; partial extraction is not rolled back. Cold fallback must handle that partially materialized state safely.
- Velnor deviation: a lookup failure is caught in [`restoreCacheV1`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/cache.ts#L236-L251) and [`restoreCacheV2`](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/cache.ts#L368-L383), which log a warning, or an error for HTTP 5xx, and report a miss. Velnor fails the step, so a store fault is never mistaken for a cold cache.
- Velnor preserves hard links as GNU tar does: the first path of a file with several links is stored, the others become link members. A link member whose destination already is the file it names (same device and inode, which happens when declared roots overlap, as with `data` and `data/sub`, or when a member names itself) restores as a no-op instead of removing the file it would link to. A link member may name any regular file already inside the declared paths, as in GNU tar; the target need not come from the same archive.
- Velnor decides which declared paths nest by the directory entries a path walks through, not by its text or by where it ends up. It records the device and inode (`lstat`) of every declared path that exists, and records them again when the restore creates or replaces one (a directory, file or link member at the declared path), then resolves the parent of a path when its first member arrives, so that every earlier path has been restored by then. It walks that parent component by component, expanding every link itself (relative and absolute targets, `..` against the walked location, at most 40 links). The first entry whose identity belongs to another declared path makes the path nested under that path with the components that remain; a path whose walk meets no declared path is opened from its own resolved parent. A path under another declared path is opened from the descriptor of the outer path, at its first use, walking the remaining components without following links. A workspace link that the restored outer directory replaces (`data` as a link with the paths `data` and `data/sub`, or `alias -> data` with `data` and `alias/sub`, also under another letter case or normalization on a case-insensitive or normalizing volume) therefore never redirects the inner path, as when GNU tar extracts both below the replaced directory. A link that is itself a declared path does not make its target nested (`current -> releases/42` with `current` and `releases/42/assets`, `self -> .` with `x` and `self`), because the link and its target are different entries. A `..` that remains after the outer entry (`data2 -> data/..` with `data` a link) fails closed, which GNU tar does not do: it follows the link out of the declared path. The inner path also fails closed when the outer path was restored as a file or a link, and the save refuses such a pair up front when a link or file lies on the way from the outer path to the inner one (`data` and `data/link/subdir` with `data/link` a link), because the published entry could not be restored. The save finds the outer path of an inner one with the same walk as the restore, by device and inode, so an alias does not hide the pair (`alias -> data` and `data/link -> ../elsewhere` with the paths `data` and `alias/link/subdir` are refused). The directories the restore creates for a path that has no archive member (the missing parents of a declared path and the implicit parents inside it) take mode 0777 under the process umask, as GNU tar creates them. A link that leads to a declared path that the restore has not created yet resolves to it once it exists: on a fresh checkout that holds only `current -> releases/42`, the paths `releases/42` and `current/assets` restore `current/assets` below `releases/42`, as GNU tar 1.35 does, and a link that still dangles when its path arrives (the paths in the other order) fails the restore, as GNU tar fails (`Cannot mkdir: No such file or directory`). Velnor deviation: declared paths that each walk through the other are refused when the parent of the first of them is resolved and the walk comes back to it, with `each resolve below the other`, although GNU tar restores some such configurations (`self/me` and `me/self` with `self -> .` and `me -> .`). The refusal fails closed and is contrived; no archive of a real workflow needs it. The cost is linear in the number of declared paths: each path adds one walk of its own parent and hash lookups, measured at 4000 declared paths by a test with a time bound.
- Velnor deviation: a cache entry that disappears between the lookup and the opening of its archive (evicted by the broker meanwhile) is a miss with a `::warning::Failed to restore` line, as the toolkit treats a failed restore, not a failed step. Only a missing entry (`ENOENT`) counts as gone: `EACCES` on an entry or its archive fails the step, because the broker never leaves an entry the step uid cannot read (the upload directory is 0755 from its creation, eviction only adds owner bits, and an entry without its marker is incomplete and never found). A failure after the archive is open still fails the step.
- The finalization of restored directory modes and times is cancel-aware: the cancel checkpoint runs once per directory, so a cancelled job stops before it finalizes the next one. The lookup checks cancellation after it has read the readable scopes, so a cancelled job never reports a hit or a miss.
- Restoring into an existing directory that the step uid cannot read or enter (mode `000`) fails the step, as GNU tar 1.35 does (`Cannot open: Permission denied`, exit 2); Velnor does not widen the mode of a directory it did not create.
- Velnor deviation: GNU tar replaces an empty directory with a file member. Velnor refuses, failing closed rather than removing a directory during restore.
- Restored modes follow GNU tar for a non-root user: the process umask is applied to every member's saved mode. Velnor applies the umask for every user, because steps run as non-root; GNU tar run as root keeps the saved modes.
- Setuid, setgid and sticky bits are dropped on restore, matching upstream: `actions/cache` 4.1.0 (`@actions/cache` 4.1.0, `lib/internal/tar.js`) extracts with `-xf <archive> -P -C <dir>` and no `-p` or `--no-same-permissions`. GNU tar 1.35 run as uid 1000 on members saved as `4755`, `1777` and `2775` writes `755` with and without `--no-same-permissions`.
- Velnor reads `cache-mode` from the job variable `actions_cache_mode` and does not export `ACTIONS_CACHE_MODE` to `run:` steps; the official runner sets it for Node actions only, and the native cache adapter takes the mode from the job facts.
- A restore under `cache-mode` `none` or `write-only` never reads the store, so it needs no broker facts and succeeds with the read-denied warning and a miss even when the facts are unavailable. An unknown mode still fails the step.
- Velnor deviation: an entry whose archive holds no members (an empty file, or only end-of-archive blocks) is a miss with a warning, not a hit, because the native packer always writes at least one root member.
- Velnor runs only `actions/cache/restore` and `actions/cache/save` at `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`; any other revision fails the step as `unsupported:` instead of running this implementation under a different action's name. The job-wide preflight rejects such a revision, an input outside the supported set and a literal glob path before the first step runs, as it does for unsupported checkout inputs. An expression-valued `with:` or input name cannot be checked then, so it is checked when the step runs: an unsupported input is refused at that step, after the earlier steps have run.

#### 5.8 Entry size, request limits and retries

- Only the legacy v1 save path checks a 10 GiB compressed archive limit, with a GitHub Enterprise Server exception. The v2 client has no equivalent check; a service-side v2 per-entry cap is unqualified. Repository quota is a separate constraint.
- Velnor deviation: the limit is 10 x 1024^3 bytes of archive and is inclusive, as in the toolkit's strict `>` check. Exactly that size is saved, and one byte more fails with `over the 10GB limit, not saving cache.`. The packer's counting writer stops at the limit, and the broker, which does not trust the packer, stops reading at the limit plus one byte.
- Documented per-repository rates are 200 new-entry uploads, 1,500 downloads and 400 deletions per minute. Account for all writers, readers and pruning together.
- The pinned client reports `retry-after` on HTTP 429 and throws without retry. Its retryable statuses are 502/503/504, with two default retries and a 5,000 ms delay. A nonfatal save failure does not establish successful persistence.

#### 5.9 Authorized writers

- Default-branch writes are permitted by default for `push`, `workflow_dispatch`, `repository_dispatch`, `delete`, `registry_package`, `page_build` and `schedule`. Other events resolving to that branch, including `pull_request_target`, `issue_comment` and `workflow_run`, default to read-only; an explicit write-capable mode changes that boundary.
- Dispatches to another ref write to that ref's scope. Pull-request writes are confined to their merge ref.
- Fork pull-request save behavior, merge-group save behavior and the pull-request default mode remain unqualified service behavior. Velnor keeps fork and merge-group jobs read-only regardless. A pull request from this repository writes only its own merge-ref scope (`refs/pull/<n>/merge`), which no other ref reads.
- The pinned action's [event gate](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/utils/actionUtils.ts) checks only that `GITHUB_REF` exists. Enforce Velnor's writer policy independently of client permissiveness.

#### Qualification boundaries

Service matching and scope rules rely on official documentation, not public service implementation. Require live qualification for same-PR cross-push reuse, service-specific size limits and warning behavior before making acceptance claims. Upgrading an action or toolkit version requires rechecking cache-mode handling, archive flags and failure outcomes; newer-client behavior does not qualify the pinned client.

### 6. REST calls answered by `velnor-local api-stub`

Each fact comes from the GitHub REST documentation: [pull request files](https://docs.github.com/en/rest/pulls/pulls#list-pull-requests-files), [compare two commits](https://docs.github.com/en/rest/commits/commits#compare-two-commits), [repository content](https://docs.github.com/en/rest/repos/contents#get-repository-content), [Actions caches](https://docs.github.com/en/rest/actions/cache) and [pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api).

- Pagination: `per_page` defaults to 30 and is at most 100. The `Link` header lists `prev`, `next`, `last` and `first` as `<URL>; rel="..."`, comma separated. A response that is not paginated has no `Link` header.
- Pull request files: at most 3,000 files. Each entry has `sha`, `filename`, `status` (`added`, `removed`, `modified`, `renamed`, `copied`, `changed`, `unchanged`), `additions`, `deletions`, `changes`, `blob_url`, `raw_url`, `contents_url`, and optionally `patch` and `previous_filename`.
- Compare: the file list holds at most 300 files and appears only on the first page. Without `per_page` or `page` the commit list holds at most 250 commits. `status` is `diverged`, `ahead`, `behind` or `identical`. Top-level fields: `url`, `html_url`, `permalink_url`, `diff_url`, `patch_url`, `base_commit`, `merge_base_commit`, `status`, `ahead_by`, `behind_by`, `total_commits`, `commits`, `files`.
- Contents: `ref` is a commit, branch or tag and defaults to the default branch. A file has `type`, `encoding`, `size`, `name`, `path`, `content`, `sha`, `url`, `git_url`, `html_url`, `download_url` and `_links`. A directory is an array of such items. A symlink has `type: "symlink"` and `target`. A submodule has `type: "submodule"`. A blob over 1 MB cannot be fetched with the default media type.
- Cache list: parameters `per_page`, `page`, `ref`, `key` (prefix), `sort` (`created_at`, `last_accessed_at` by default, `size_in_bytes`) and `direction` (`desc` by default). The body is `{total_count, actions_caches: [{id, ref, key, version, last_accessed_at, created_at, size_in_bytes}]}`. Deleting by id answers 204.
- Compare, as recorded in the captured responses under `docs/implemented/evidence`: `url`, `html_url`, `diff_url` and `patch_url` keep the refs as requested, `permalink_url` pins `owner:sha...owner:sha`, and `commit.message` has no trailing newline.
- Contents: a directory lists at most 1,000 entries. A symlink whose target is a normal file in the repository returns that file's object; any other symlink returns `type: "symlink"` with `target`. An omitted `ref` reads the default branch, which the stub takes from `--default-branch`.
- Compare commits are chronological. Without `per_page` or `page` the list holds the newest 250 commits, so its last commit is the most recent of the comparison, and the stub sends no `Link` header. With either parameter the pages are chronological slices of 30 by default and at most 100.
- A submodule inside a directory listing has `type: "file"`; asked for directly it has `type: "submodule"`. The stub does the same.
- Modelled: a submodule entry carries the gitlink sha and no `submodule_git_url`; the stub does not read `.gitmodules`.
- A removed file's `blob_url`, `raw_url` and `contents_url` use the base sha, and the file name is one percent-encoded segment (`src%2Findex.ts`), as in the captured responses.
- Not served: `contents/` with no path is the repository root listing on GitHub. The stub answers it with 501 because the helper does not call it; the issue that makes the helper list the root adds it.
- Modelled, not documented: a diff over the stub's size limit answers 422 with `{"message": "Validation Failed", "errors": [{"resource": "Diff", "code": "too_large"}]}`.
- Modelled, not documented: the stub answers 404 with `{"message": "Not Found", "documentation_url", "status"}` for a missing resource and 403 with a `too_large` error for a blob over 1 MB. The stub's commit objects carry `sha`, `url`, `html_url`, `commit` (`author`, `committer`, `message`, `tree`) and `parents`, a subset of GitHub's.

## Native preview publication API boundaries

The [workflow-jobs REST contract](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt)
provides the attempt-specific endpoint
`GET /repos/{owner}/{repo}/actions/runs/{run_id}/attempts/{attempt_number}/jobs`.
The job response includes `run_id`, `head_sha`, and `head_branch`; it does not
include a `run_attempt` property. Velnor binds the attempt through that endpoint,
checks those returned identities and the single successful `Required` job, then
re-fetches the latest workflow run and rejects a changed run ID or attempt.
Executable API fixtures omit the nonexistent job property so they cannot conceal
an incompatibility with the upstream response shape.

The [repository REST contract](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository)
provides `GET /repos/{owner}/{repo}/immutable-releases` with `enabled` and
`enforced_by_owner`. The [release REST contract](https://docs.github.com/en/rest/releases/releases)
returns each published release's `immutable` status; its assets expose byte sizes
and `sha256:` digests. Native publication must check the prerequisite before any
unfinished mutation and independently verify the resulting immutable release.
Draft creation, asset uploads, and publication are separate mutations. No
observation authorizes replacing an asset, forcing a tag, or deleting a release.

The immutable-release settings GET requires Administration read permission. The
workflow token cannot request this permission through `permissions: contents`.
Native publication therefore uses the separate `PREVIEW_IMMUTABILITY_READ_TOKEN`
environment secret only for this fixed settings route, while release mutations
continue using the workflow token. The settings credential is never forwarded
to repository tasks, the signer, or tap promotion.

Release responses do not include `make_latest`; it is a request-only string enum.
`gh api --field`
converts literal booleans and expands `@file` values, so native release requests
use `--raw-field` for string fields and `--field` only for draft/prerelease
booleans. Both channels publish with `make_latest: false`; preview releases are
also prereleases, while stable releases remain ordinary releases without being
forced to latest. The release response has no `make_latest` field, so Velnor
verifies the returned draft/prerelease/immutable state and exact assets without
assuming that a newly published stable release becomes latest.

### Review-completion provider qualification

The [official Codex GitHub review guide](https://learn.chatgpt.com/docs/third-party/github)
documents requested and automatic reviews, but does not establish a supported
machine-state completion protocol. Native merge admission therefore continues
to require the separately authenticated `velnor.review-completion.v1` check;
reactions, table prose, and an absent finding are not substitutes.

A provider summary or completion marker does not qualify an adapter. Require a
supported completion contract binding the full head to complete code and security
finding dispositions; observed App implementation details are insufficient.

GitHub's [IssueComment GraphQL fields](https://docs.github.com/en/graphql/reference/issues#issuecomment)
expose the latest editor and edit time. Any future adapter must authenticate
the actual numeric App, author, and latest editor, independently fetch the PR's
full current head, parse closed machine fields, and revalidate all current
feedback. Positive exact-head completion and forged/edited-state rejection
remain deployment gates. Unattended merging stays disabled until that proof
exists; an unsupported success enum must fail closed.

The [required-review documentation](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews)
distinguishes a comment from approval: a blocking change request requires the
reviewer's subsequent approval or an actual review dismissal. A later comment
must not clear that request. Native feedback aggregation must preserve this
distinction independently of response order, and must reject unknown states.
GitHub also exposes [current per-user review connections](https://docs.github.com/en/graphql/reference/pulls#pullrequest);
any server-side opinion query must retain complete pagination and bind the same
PR/head as the full feedback snapshot.

Native capture uses paginated, server-derived opinionated state alongside all
underlying feedback records. Comment reviews and dismissed reviews must not be
mistaken for an active approval or change request. Unexpected opinionated states
still fail closed.

### Pull-request response boundaries

The pull-request listing omits the `merged` Boolean. Native completed
reconciliation must fetch each matching PR's detail
before trusting that state. A non-null merge SHA alone is insufficient because
the [pull-request API](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request)
also uses that field for a synthetic test merge before completion.

The [Git Database commit endpoint](https://docs.github.com/en/rest/git/commits#get-a-commit-object)
returns `message` at the top level; `commit.message` belongs to the repository
commit response. Protocol fixtures must model the endpoint actually requested.
Historical proof reconstruction binds the original branch base and commit parent,
while the actual PR detail base is retained separately. Selected open and merged
PRs must match that original base. A closed, unmerged historical PR may have
observed a later base without losing its original proof or feedback obligations.

The exact-head feedback query includes numeric actor identity for both `User` and
`Bot` in `latestOpinionatedReviews`, including `Bot.databaseId`. Selecting only the
`User` fragment makes bot reviews unidentifiable. Protocol coverage must include
a bot's outstanding change request.

GitHub documents that applicable rulesets layer with classic branch protections;
rules are aggregated and the most restrictive duplicated rule applies. A strict
status-check source therefore continues to require an up-to-date branch when
another source has loose checks. Native protection resolution must union required
checks and retain strictness from any authenticated effective source, rather than
rejecting an otherwise valid loose contributor. Classic PR-review protection is
nullable; disabled review protection contributes no nested PR-bypass allowances.
Missing controls must not prevent complementary authenticated sources from being
combined; malformed present controls must not become positive protection evidence.
Sources: [ruleset layering](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets),
[strict and loose status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets),
and [classic branch protection API](https://docs.github.com/en/rest/branches/branch-protection).

GitHub's [protected-branch documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
states that after required status checks pass, commits may be pushed directly to
the protected branch or merged through a pull request. Strict status checks only
require the PR branch to be current before merging. Therefore a workflow whose
contract promises PR-only promotion must independently require the server-side
pull-request rule; required checks and a client-side PR workflow are insufficient.

## Job budgets and cancellation

The pinned runner's [executable-job schema](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Sdk/DTPipelines/workflow-v1.0.json#L55-L75)
accepts `timeout-minutes` on a job with `runs-on`. GitHub's
[job-timeout contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes)
says expiration automatically cancels that job; its default is 360 minutes.
GitHub's [Actions limits](https://docs.github.com/en/actions/reference/limits)
cap each GitHub-hosted job at six hours of execution time (self-hosted: five
days), so Velnor's configurable upper bound is 360 minutes.
Velnor must explicitly emit a timeout between one and 360 minutes (five by
default; never GitHub's implicit default) on every
executable job, including CI, qualification, native image, signing, publication,
recovery, and reusable-workflow callees. GitHub provides no workflow-wide timeout
keyword: queued time, environment approval, and successive jobs are separate
from this per-job execution budget.
Runner-family production has a 330-minute job budget and a 110-minute build
command deadline. The remaining 220 minutes cover all other job phases together.
Discovery uses separate read-only jobs for the CI producer and the qualification
result, each bounded by a 350-minute wait in a 360-minute job. Qualification
and publication run in separate jobs with their own budgets.
These bounds fail closed when scheduling or upstream work takes longer; they do
not promise completion for every possible queue delay or workflow configuration.

A [reusable-workflow caller](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#supported-keywords-for-jobs-that-call-a-reusable-workflow)
allows a restricted set of job keys that excludes `timeout-minutes`. Emit the
limit on every concrete job inside the called workflow; adding it to a `uses`
caller is invalid. The runner schema above describes executable jobs, not the
GitHub service's reusable-call expansion; it does not establish caller syntax.

Cancellation is not an instantaneous physical process cutoff. The listener
[receives the server's cancellation message](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Listener/JobDispatcher.cs#L140-L154)
and [starts a separate worker cancellation window](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Listener/JobDispatcher.cs#L1269-L1286).
The worker [re-evaluates the running step's condition](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/StepsRunner.cs#L145-L185)
before canceling its token. The process invoker allows
[7.5 seconds for SIGINT and 2.5 seconds for SIGTERM](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Sdk/ProcessInvoker.cs#L32-L33),
then [kills the process tree if those signals did not finish the process](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Sdk/ProcessInvoker.cs#L443-L465).
The [official cancellation contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-cancellation)
also permits a five-minute cancellation grace before server-forced termination.
These mechanisms cannot substantiate a claim that all processes disappear
exactly 300 seconds after job start.

Native producer execution therefore needs its own monotonic deadline shared
across preparation, acquisition, probes, task execution, and output admission,
with process-group termination and bounded reaping reserved within that budget.
Starting a fresh timeout for each phase would permit cumulative overruns.
Expiration must prevent a success proof or publication handoff; GitHub's outer
job limit additionally bounds checkout, tool installation, and source fetch.

## Native preview after completed CI

The dedicated native preview renderer admits only the canonical CI completion
trigger, outside the generic trigger IR. GitHub documents that `workflow_run`
uses the default-branch commit/ref, can receive write tokens, and also fires for
unsuccessful upstream completions. Its name and branch filters do not establish
trusted source identity. See the [official event contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).

Pinned runner commit
[`9abfe11d2abd9de09d3a8d933d7ed701ea0518a3`](https://github.com/actions/runner/commit/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3)
exports the actual [event, ref, SHA, and workflow SHA](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/GitHubContext.cs#L14-L67)
and [writes the webhook payload](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/ExecutionContext.cs#L1320-L1337).
The service's event selection and privilege semantics are documented contracts,
not behavior proven by this runner implementation.

Before producer execution, eligibility authenticates the canonical CI workflow,
successful same-repository push run/attempt, Required job and latest exact-A CI.
Source A may precede workflow authority B and protected main tip H only when
`A <= B <= H` is independently proven. Foreign, PR, failed or superseded CI
cannot authorize preview. No CI artifact selects A. Signer and publisher retain
isolated permissions and independent revalidation. Receipts bind source A;
certificates and standard SLSA workflow claims bind B's producing `workflow_run`.
See [provider A/B evidence](../implemented/immutable-preview-api-qualification.md#workflow-run-source-and-authority-observation).

The pinned [completed-event schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/workflow_run/completed.schema.json)
requires the event action, repository, workflow, and workflow-run envelope; the
[run schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/common/workflow-run.schema.json)
provides the workflow ID/path/name, repository and head repository, attempt,
source, and terminal status used by admission. The envelope's workflow identity
must agree with independently fetched canonical CI metadata.

## Native completion trigger scanner disposition

Pinned zizmor 1.31.1 (since [retired](../jactionlint-decision.md#zizmor-is-retired); jactionlint honours the annotation), original commit
`effdb35fa7868863f11655db256d9008922fa427`, emits `dangerous-triggers`
[unconditionally for `workflow_run`](https://github.com/zizmorcore/zizmor/blob/effdb35fa7868863f11655db256d9008922fa427/crates/zizmor/src/audit/dangerous_triggers.rs#L51-L98).
The rule includes a hidden location covering the entire `on` block. Its inline
annotation can therefore suppress a dangerous sibling trigger as well; placement
on the canonical CI scalar does not narrow that upstream suppression scope.
Qualification must cover sibling suppression and prove that unrelated
template-injection findings remain active.

Velnor accepts this finding only for its dedicated authenticated native completion
protocol. Production structural admission requires exactly the native completion
and manual recovery events, with canonical CI, completed activity and main branch
filters. Mutation tests add dangerous sibling events while preserving the
annotation and require rejection before output. The generic restricted-event
policy, full staged scanner invocation, and other audits remain unchanged. An
inline annotation alone is never sufficient release authority.

## Recursive MBX input matching qualification

Use the pinned runner v2.337.0 [hashFiles implementation](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Misc/layoutbin/hashFiles/index.js)
to qualify root and scoped patterns against deep Cargo members, custom member
paths, build scripts and source members named `target` below another directory.
Coverage must prove digest changes, actual workspace target exclusion and scoped
sibling isolation. Pattern compatibility does not establish hosted performance or
Cargo artifact reuse.

GitHub's [expression reference](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#hashfiles)
specifies recursive `**` patterns and ordered `!` exclusions. The patterns exclude
only the selected workspace's mutable target, never every directory named target.
The native Rust regression tests also bind these patterns to the rendered cache
keys and unchanged generation, job, toolchain and restore-prefix boundaries.

See [credential-free preview progress encoding](../implemented/native-preview-progress.md)
for the bounded diagnostic output protocol and its runner-source references.

See [native release asset upload](../implemented/native-release-upload.md) for
the qualified upload-host, authentication and binary-input source contract.
See [immutable preview API qualification](../implemented/immutable-preview-api-qualification.md).
See [explicit-token tool launch](../implemented/explicit-token-tool-launch.md).

## Pull Request Merge References

[GitHub's mergeability guidance](https://docs.github.com/en/rest/guides/using-the-rest-api-to-interact-with-your-git-database#checking-mergeability-of-pull-requests) defines asynchronous calculation of the test merge commit after a REST pull-request GET. GitHub requires polling that endpoint until `mergeable` is `true` or `false` before reading the merge reference. Two equal reference observations while `mergeable` is null do not establish freshness.

Velnor bounds this polling by attempts and the existing public-operation deadline. Each response must retain the original pull-request identity, repository identities, base and head, state, draft status, and merge status. After calculation finishes, Velnor retains the exact-reference checks, ordered base/head parent checks, and unchanged-reference recheck. The merge reference does not replace the independently verified pull-request context.

## Merge queue entry identity

Use GitHub's [MergeQueueEntry schema](https://docs.github.com/en/enterprise-cloud%40latest/graphql/reference/pulls#mergequeueentry) to authenticate the current queue entry. Keep its base and queue head distinct from the original pull-request head; do not infer queue topology or batching from one observed commit.

A cutover author check must bind the current queue entry's base and head to the authenticated merge-group context, then authenticate the associated pull request and its current author. A queue branch name alone is not author evidence. The pull-request test merge reference and an assumed two-parent queue commit are not substitutes for queue membership. Missing, stale, ambiguous or truncated membership must fail closed.

The private-source token check (`velnor-ci private-token-allowed`) relies on a contract that GitHub does not document, so it is recorded here. GitHub creates one ref `gh-readonly-queue/<base>/pr-<number>-<sha>` per queue entry and deletes it when the entry merges or leaves the queue. The commit of an entry holds the commits of every entry ahead of it. The check lists the refs of the base, requires the live ref of its own entry to equal `GITHUB_SHA`, and authenticates each ref whose commit is among the commits `compare/<base>...<GITHUB_SHA>` reports. An entry without a ref cannot add code the check misses: if it merged, its commits are in the base branch and are not in that comparison, and if it left the queue, GitHub rebuilt the entries behind it, so the live ref no longer equals `GITHUB_SHA` and the check refuses. The comparison is read for at most three pages of 100 commits and a longer one is refused.

## Private Bun artifact admission

GitHub documents that authenticated users with repository read access can
download workflow artifacts. Artifact retention does not restrict that audience.
A public consuming repository therefore cannot receive a plaintext private-asset
handoff. Source: [Downloading workflow artifacts](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/download-workflow-artifacts).

The generated native admission step uses the current
`GET /repos/{owner}/{repo}` response rather than trigger-time event visibility.
It requires matching identity, Boolean private status and private visibility.
The existing bounded native GitHub transport supplies fixed-origin HTTPS,
workflow-token authentication and a deadline. Source:
[Get a repository](https://docs.github.com/en/rest/repos/repos?apiVersion=2026-03-10#get-a-repository).

## Local reusable workflow identity

The [workflow-run API](https://docs.github.com/en/rest/actions/workflow-runs#get-a-workflow-run) identifies reusable workflows through `referenced_workflows`, including their path and resolved `sha`; a SHA-pinned entry can omit `ref`. Bind the resolved workflow identity independently of the caller's source identity and do not require optional fields to establish a SHA-pinned reference.

The official runner's [v2.338.2 `JobContext`](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/JobContext.cs) exposes `check_run_id`, `workflow_ref`, `workflow_sha`, `workflow_repository`, and `workflow_file_path`. Admission matches the current job's check-run ID to the [workflow-jobs API](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt) `check_run_url` for the exact attempt, then checks the defining reusable workflow against the run's resolved references. A SHA-pinned private fixture run, `42585338912` attempt 1, confirmed this correlation on 2026-11-09 and confirmed that its reusable reference omitted `ref`. Missing or ambiguous identity fields fail closed. This fixture establishes context compatibility only; it does not establish SDK or runner-image qualification.

A reusable signer's certificate URI can use a branch-qualified ref while its resolved workflow SHA is carried separately. The certificate also identifies the caller workflow and SHA, event, run ID and attempt. Field decoding establishes representation only: admission must verify the signed attestation, trusted issuer and exact workflow/source digests.

## Commit archive retrieval

GitHub's [repository tarball API](https://docs.github.com/en/rest/repos/contents#download-a-repository-archive-tar) responds with a redirect, which the pinned `gh` client follows. GitHub CLI's [redirect discussion](https://github.com/cli/cli/issues/8055) describes that behavior. Publication independently verifies selected archive bytes and executable modes against the authenticated commit tree; archive permission metadata and a successful download alone do not establish source identity. Limits are 64 MiB for the compressed response, 256 MiB for the expanded stream, 16 MiB per selected file and 128 MiB for selected content.

## Attestation history and bundle retrieval

GitHub's [repository attestation endpoint](https://docs.github.com/en/rest/repos/attestations#list-attestations) lists attestations for a `sha256:<digest>` subject and paginates using Link headers, with at most 100 entries per page. The response may include an inline bundle or a signed `bundle_url`; GitHub's [breaking changes](https://docs.github.com/en/enterprise-cloud@latest/rest/about-the-rest-api/breaking-changes) describe removing inline bundles. The pinned [GitHub CLI 2.102.0 client](https://github.com/cli/cli/blob/v2.102.0/pkg/cmd/attestation/api/client.go) fetches URL bundles without GitHub API credentials and decodes raw Snappy data. Its attestation lookup limit is finite and capped at 1,000, so an exact run cannot be selected reliably from a fixed prefix of subject history. The [verification command](https://cli.github.com/manual/gh_attestation_verify) accepts a JSONL bundle file containing multiple attestations.

Velnor enumerates the bounded history to completion, retrieves signed bundles without the GitHub token, verifies a bounded JSONL batch cryptographically, and then enforces the exact source, signer, run and attempt on the selected certificate. Oversized, invalid or incomplete evidence fails closed.

## Image publication inventory and registry errors

GitHub's REST endpoints list [workflow-run artifacts](https://docs.github.com/en/rest/actions/artifacts#list-workflow-run-artifacts), [jobs for a run attempt](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt), and [runs for a workflow](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-workflow) with `per_page`, `page`, and `total_count`. The workflow-run endpoint limits a filtered search to 1,000 results; Velnor's CI search uses the `branch`, `event`, and `head_sha` filters. A capped search is not evidence that an omitted run does not exist.

The image publisher requests 100 entries per page and admits at most 20 pages, 2,000 entries, and 16 MiB of cumulative serialized response JSON for an inventory. It requires the same `total_count` on every page, the expected number of entries per page, and unique nonzero IDs. Missing, inconsistent, duplicated, or over-budget evidence fails publication. Each API response is also bounded before JSON parsing.

The [OCI Distribution Specification error table](https://github.com/opencontainers/distribution-spec/blob/4f7750b2ee7ab20238c4e866072e2178d746560b/spec.md#error-codes) distinguishes `NAME_UNKNOWN`, an unknown repository, from `MANIFEST_UNKNOWN`, an unknown manifest. Both represent absence for the authenticated image tag lookup; `UNAUTHORIZED` and `DENIED` do not. The pinned [Crane 0.22.1 error formatter](https://github.com/google/go-containerregistry/blob/v0.22.1/pkg/v1/remote/transport/error.go#L43-L70) prints a single structured error's code and message without its HTTP status. Velnor therefore binds the exact requested reference and manifest URL, accepts only those two absence codes with a bounded single-line message, and rejects other or ambiguous diagnostics. This relies on the registry's OCI error-code semantics; the Crane output does not establish that the response had a particular HTTP status.

## Generator publication concurrency

GitHub supports `queue: max` with `cancel-in-progress: false`, allowing up to
100 pending runs per concurrency group. Generator publication separates main
pushes, SDK preview tags, and reusable release calls into distinct groups; SDK
preview publications remain serialized within their channel. See the
[workflow concurrency reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#concurrency).

## SDK preview deployment admission

GitHub's [job context](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#job-context) identifies the reusable workflow that defines the current job by repository, path, ref, and SHA. Its [context availability table](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability) permits these values in step environment expressions. In a reusable workflow the `github` context identifies the caller; the two identities are checked separately. The reusable authority is referenced by a full commit SHA, and the bootstrap emits no tag-publication caller until an independently merged authority is pinned.

GitHub defines protected-branches environment admission for branches. SDK preview
publication uses the separate `generator-source-preview` environment with a
selected `sdk-preview-*` tag policy and the owner reviewer. The stable
`generator-release` environment retains its existing policy. See
[deployment branches and tags](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments#deployment-branches-and-tags)
and the [tag-specific deployment policy API](https://docs.github.com/en/rest/deployments/branch-policies#create-a-deployment-branch-policy).

GitHub's [release by tag endpoint](https://docs.github.com/en/rest/releases/releases#get-a-release-by-tag-name) returns published releases. Its [release listing endpoint](https://docs.github.com/en/rest/releases/releases#list-releases) includes drafts only for callers with push access. The preview draft job has contents write access and searches a bounded complete release listing before creating a release; the read-only source job does not infer draft absence from a hidden result. Candidate selection and later jobs retain the exact original artifact IDs recorded in the owned draft.

## Workflow run-step length

GitHub limits a single [`jobs.<job_id>.steps[*].run` value](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepsrun) to 21,000 characters. Velnor materializes longer fixed Bash scripts in bounded steps after binding project tool references. It verifies the reconstructed bytes before running the original script. Oversized scripts with GitHub expressions or unsupported runner and shell settings fail generation.

The source preview caller admits only the immutable
[publication authority](https://github.com/tailrocks/velnor-new/blob/72701af69f6f9b693e639814fcf479409da3295c/.github/workflows/source-preview-publication.yml)
at commit `72701af69f6f9b693e639814fcf479409da3295c`. The reviewed workflow
blob has SHA-256 `b157479660663d7241291be5f392f4338e6b60a2063bda61d7aa02f82f94990e`.
Its source job has a five-minute timeout; draft, attest and publish have
ten-minute timeouts and depend in order on their predecessors. It has no
nested reusable workflow calls. Timeout admission uses this fixed authority
record, not a candidate workflow generated in the caller's checkout.

The image release caller separately admits the immutable
[Mac proof authority](https://github.com/tailrocks/velnor-new/blob/533d225c08785adc4367340e1db4dc111ee2b5a2/.github/workflows/source-preview-mac-proof.yml)
at commit `533d225c08785adc4367340e1db4dc111ee2b5a2`. Its reviewed workflow
blob has SHA-256 `ff3ef47eb0f5ea6e43eedb505c5aea15a1ba2e8f523d81959f81e592034b3f1c`.
It contains one `verify` job with a 140-minute timeout, no predecessors, and no
nested reusable workflow calls. This record applies only to the
`verify-mac-proof` job in `product-release-images.yml`; other external references
remain rejected.

## Private production Mac runner constraints

GitHub documents repository-scoped runner registration separately from
organization runner groups in its [runner administration API](https://docs.github.com/en/rest/actions/self-hosted-runners#create-a-registration-token-for-a-repository).
A repository-scoped runner trusts the repository's reviewed writer/workflow
surface; it does not imply organization group workflow restrictions.
The [workflow dispatch API](https://docs.github.com/en/rest/actions/workflows#create-a-workflow-dispatch-event)
accepts a branch or tag ref. Admission must compare the actual run and workflow
commit to its pin rather than claim that dispatch accepts a raw commit SHA.

The [OIDC reference](https://docs.github.com/en/actions/reference/security/oidc)
and pinned [Fulcio certificate extensions](https://github.com/sigstore/fulcio/blob/24deee31680022186e823592f8a2d760841fe043/docs/oid-info.md)
define the identity fields used by the additional Mac proof. Cosign issuer/SAN
verification alone does not check every repository, workflow and invocation
extension required by the runner image contract.

The pinned [attest-build-provenance action inputs](https://github.com/actions/attest-build-provenance/blob/4d101475d8b20a2381f78447822ac1eab6504dd8/action.yml#L7-L23)
accept `subject-digest` in `algorithm:hex_digest` form together with
`subject-name`. This mode is exclusive with `subject-path` and
`subject-checksums`. Runner-family final attestation passes the four names and
digests emitted by the authenticated native verifier, so later local file
changes cannot select different attestation subjects.

### Artifact download extraction

The pinned [upload-artifact implementation](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/src/shared/upload-artifact.ts)
uploads through `@actions/artifact`. The toolkit's
[runtime credential reader](https://github.com/actions/toolkit/blob/main/packages/artifact/src/internal/shared/config.ts)
uses `ACTIONS_RUNTIME_TOKEN`. Same-run uploads do not require `actions: write`
on `GITHUB_TOKEN`. Release build jobs grant only `contents: read`; the candidate
manifest job also grants `actions: read` for artifact downloads.

The [pinned artifact reference](github-actions-artifact-reference.md#artifact-download-extraction-boundary)
records the download action's extraction and digest boundary. Verify artifact
producer and identity before download; the digest check runs after extraction.

The pinned [download-artifact source](https://github.com/actions/download-artifact/blob/9000827ccba6bdab643e8b6fd33ac0654aef8333/src/download-artifact.ts#L48-L166)
(revision `9000827ccba6bdab643e8b6fd33ac0654aef8333`, lines 48-166) picks its
mode from the truthiness of `artifact-ids` and parses each id with `parseInt`.
An empty id therefore downloads every artifact of the run, and a malformed id
is parsed instead of rejected. Generated workflows put a "Validate exact
artifact ID" step directly before each download. It accepts only a positive
integer without leading zeros that is at most 9007199254740991, the largest
exact JavaScript integer. Recheck this guard when the action pin changes.

### Hosted macOS source qualification

The [runner image label table](https://github.com/actions/runner-images/blob/main/README.md)
identifies `macos-15` as ARM64 and `macos-15-intel` as x64. The published
[ARM64](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md)
and [Intel](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-Readme.md)
inventories do not list GNU coreutils as a guaranteed tool. Source qualification
uses the platform `shasum -a 256` command for lock identities instead of relying
on `sha256sum`.

## Tag ruleset drift

The maintenance drift job uses the [pinned github-script API client](https://github.com/actions/github-script/blob/3a2844b7e9c422d3c10d287c895573f7108da1b3/README.md#this-action) and its pagination plugin without checkout.
The [ruleset API](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset) omits bypass actors without write access to the ruleset. Missing bypass data fails the check; the dedicated credential needs Administration write permission.

## Rulesets API: bypass_actors visibility

GitHub's [repository ruleset endpoint](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset) says `bypass_actors` is returned only when the requester has write access to the ruleset. The [branch rules endpoint](https://docs.github.com/en/rest/repos/rules#get-rules-for-a-branch) returns effective rules and does not document bypass actors. The release gate requests Administration write because it requires an explicit array.

## PR-only check exemption contracts

The [Checks API](https://docs.github.com/en/rest/checks/runs) limits the commit-reference check-runs endpoint to the 1,000 most recent check suites for a SHA. To inspect the full history, enumerate check suites for the reference and list check runs for each suite; both suite and run listings are paginated, with at most 100 items per page. A check run includes its suite identity and the suite includes its App identity. An unfinished suite from the App that owns an exempted required check cannot establish that the check is absent. The endpoint requires `checks: read` for fine-grained GitHub App tokens.

The [Rules API](https://docs.github.com/en/rest/repos/rules) returns the effective rules for a branch with their ruleset IDs and source identities. Fetch each candidate ruleset with `includes_parents=true` (the default) and verify that its ID, source, source type, target and rules match the effective rule. A repository ruleset response can omit `bypass_actors` unless the credential has write access to that ruleset, even when the request succeeds; absence is unknown, not proof that bypass is disabled. Organization and enterprise ruleset details may be unavailable without the necessary permissions or a supporting GitHub plan; inaccessible details cannot authorize an exemption. An active branch ruleset must explicitly return an empty `bypass_actors` list to prove non-bypassable pull-request and required-status-check enforcement. GitHub defines `pull_request` and `required_status_checks` as distinct rules; verify both rules in the effective enforcement used for the exemption. The ruleset read endpoint documents `Metadata: read` for fine-grained tokens, but visibility of bypass actors has the stricter write-access condition.

## Generated Rust CI workflow events and concurrency

### Literal action inputs

Action inputs can contain repository paths whose bytes resemble Actions expressions.
The generator represents these paths as JSON strings passed to `fromJSON` inside
a single-quoted expression string. JSON escapes protect the JSON string, and
doubled apostrophes protect the expression string.
The runner's [template reader](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTObjectTemplating/ObjectTemplating/TemplateReader.cs#L493-L540)
ignores closing expression braces inside quoted strings.
The [`fromJSON` function](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/Sdk/Functions/FromJson.cs#L11-L21)
parses the JSON value, and the evaluator creates a
[string token](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTObjectTemplating/ObjectTemplating/Tokens/TemplateToken.cs#L52-L82).
The [action runner](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ActionRunner.cs#L174-L185)
receives evaluated input strings without a second template evaluation.
Expression results therefore remain literal input values.
The runner's [expression parser](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/ExpressionParser.cs#L441-L445)
enforces the [21,000-character expression limit](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/ExpressionConstants.cs#L28-L33).

Large helper path lists use hex-encoded UTF-8 paths in JSON environment values.
The shell command contains only the fixed environment-variable name.
The helper decodes the complete specification before changing files.

### Workflow events and concurrency

The runner's [`toJSON` implementation](https://github.com/actions/runner/blob/main/src/Sdk/DTExpressions2/Expressions2/Sdk/Functions/ToJson.cs)
serializes nested mappings with two spaces per level, LF newlines, a space after
each colon, and commas between entries. The Required gate's environment size
estimate includes this formatting and JSON string escaping for job identifiers,
output names, and output values. The expression stays in `env`; the shell command
contains no interpolated expressions.

These behaviors are GitHub service semantics, so the runner source does not define them. The generator relies on the GitHub documentation below and tests the rendered text, not the service.

- Pull-request activity types: the [`pull_request` event](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request) lists `opened`, `synchronize`, `reopened`, `ready_for_review`, `edited` and `converted_to_draft`. The generator lists the first five always (`edited` also fires when the base branch is retargeted, so a green result for the old base is replaced, as docs/implemented/affected-selection.md requires). It adds `converted_to_draft` only with `skip_draft_pull_requests`, so a running non-draft run is cancelled by the replacement run that skips its jobs.
- Draft state: `github.event.pull_request.draft` is the pull-request payload field. A `ready_for_review` run carries `draft == false`.
- Conditionally skipped jobs: [GitHub reports a job skipped by a condition as successful](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions), so a required check cannot rely on `needs` results to notice skipped lanes. With `skip_draft_pull_requests`, the verdict job fails on a draft run and only a non-draft run can pass it.
- Label-gated runs: with `pr_ci = "label"` the `pull_request` trigger adds `labeled`, and the label names are read from `github.event.pull_request.labels.*.name`, the labels the pull request had when the event fired. A job skipped by its condition is reported as successful, so `Required` runs the gate only where the full suite was requested and otherwise fails on a failed or cancelled need. A `pull_request` run from a fork gets a read-only `GITHUB_TOKEN`, so the job that removes `ci:run` runs only on pull requests from the same repository, and a fork pull request never runs the full suite on a label (the merge queue does). Added labels other than `ci:run` and `ci:auto` run nothing and get their own concurrency group,, and `edited` stays a trigger type so a base retarget still replaces the result; an edit that keeps the base (`github.event.changes.base` absent) runs nothing and gets its own concurrency group, so it cannot cancel a requested full run.
- Pull-request state: CI jobs exclude a `pull_request` event when `github.event.pull_request.state == 'closed'` or `github.ref` is outside `refs/pull/`. The [GitHub context contract](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#github-context) assigns merged PR events the destination branch ref, independently of the possibly empty PR payload. Open edits still run so base retargeting replaces the previous result. Missing state with a PR ref retains the conservative planning path and Required execution, including for forks.
- Concurrency: [group names must be unique across workflows](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency) to avoid cross-workflow cancellation. The group uses `github.workflow_ref`, the workflow file path, not the display name. Only `pull_request` runs with a PR ref and a state other than `closed` share the PR group and cancel in progress. Other events get a per-run group. GitHub also replaces a pending run in the same group regardless of `cancel-in-progress`, so excluded PR edits require a separate group.
- `merge_group` and `push` to the configured default branch start the same workflow; `default_branch` is a validated config key and a push to any other branch starts no run.

### Generated Rust pull-request delta isolation

[GitHub's cache restrictions](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache) scope a `pull_request` cache to `refs/pull/N/merge`; the base branch and other pull requests cannot restore it. Delta keys form their own `dlt` family beside the `tools`, `src`, `tgt`, `private-src` and `private-tgt` families of the shared and private key layouts: the family tag follows `velnor-` in every key and restore prefix, so a delta key is never equal to, and never a prefix of, a base key. They also encode the PR number explicitly and retain it in every restore prefix. No generated PR or merge-group job receives `actions: write`.

The [`workflow_run` event](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run) requires its workflow file on the default branch and can grant write permissions after an unprivileged workflow finishes. The generated cleanup workflow uses that trusted source, installs only the locked released helper, and executes no checkout, project command, cache restore or artifact from the triggering run. Its helper independently validates the completed workflow run and same-repository PR identities before deleting only that PR's older delta caches.

## Runtime cache-prefix bounds

The runner [sets the OS and architecture contexts from VarUtil](https://github.com/actions/runner/blob/af09a511a0869abf002daef236c66c5b146d683a/src/Runner.Worker/JobRunner.cs#L150-L151).
[VarUtil's complete value sets](https://github.com/actions/runner/blob/af09a511a0869abf002daef236c66c5b146d683a/src/Runner.Common/Util/VarUtil.cs#L21-L55)
make `Windows` and `ARM64` the longest values, at seven and five ASCII bytes.
The generator bounds JSON cache outputs from the same key segments that render
cache fallbacks. The compatibility width is shared with the helper's producer;
Base-save, source-save and cache-verification values require at most the five characters of `false`.
These bounds include JSON escaping and do not depend on a cache-key length limit.

The pinned [`actions/cache` save implementation](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts) catches save errors, emits warnings and exits successfully. Its [save action inputs](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/save/action.yml) expose neither a strict failure switch nor a saved-key output. Generated base-cache writers therefore verify the exact key and ref with the [read-only cache listing API](https://docs.github.com/en/rest/actions/cache#list-github-actions-caches-for-a-repository). The separate Cache check reports failed verification without changing Required. Cargo-lane reports collect local logs, provenance, step results and metrics without API access or `GITHUB_TOKEN`; a separate no-checkout Cache job performs API enrichment with `actions: read`. The Actions runner [status-function implementation](https://github.com/actions/runner/blob/main/src/Runner.Worker/Expressions/SuccessFunction.cs) evaluates `success()` against job status for steps; save steps combine this with their event and explicit decision guards, so an earlier producer or inline input-audit failure cannot authorize publication.

## Public source artifact admission

A generated inline step uses the pinned GitHub CLI and a step-scoped token to
read metadata from the fixed `tailrocks/velnor-new` repository through GitHub's
[Get an artifact endpoint](https://docs.github.com/en/rest/actions/artifacts#get-an-artifact).
It validates a positive numeric artifact ID before the request, bounds the
response to 1 MiB and removes stale or incomplete metadata on failure. The
repository admission script reads only that metadata file without credentials
or network access. Admission checks the same-run producer identity, artifact
digest, source locks, size and expiry. An unavailable metadata response takes
the generator release qualifier's existing source-fetch fallback. The mandatory
warm qualification fails if it cannot fetch or admit the source artifact.

## Lowercase hexadecimal `hashFiles` digests

The runner's [hashFiles implementation](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Misc/layoutbin/hashFiles/index.js)
prints `result.digest('hex')`, which is lowercase hexadecimal. Cache maintenance
classifies a tools anchor key only when its lock hash is lowercase, so a runner
implementation that emitted another casing would leave its anchors unclassified.

## `hashFiles` deviations from the runner

Velnor's expression engine evaluates `hashFiles` itself, mirroring the pinned
runner v2.338.2 [hashFiles implementation](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js)
and its [HashFilesFunction](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Expressions/HashFilesFunction.cs).
Pattern handling follows `@actions/glob`: trimming and `#` comment lines
([index.js:1425](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L1425)), implicit `/**`
descendants that need at least one more segment
([index.js:1350](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L1350) and
[index.js:2152](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L2152)), `~` expanding to
HOME before the workspace filter
([index.js:2207](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L2207)), and a dangling
symlink matched by a pattern failing the call, because the runner's
`statSync` on each match throws
([index.js:94](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L94)).
Each segment is matched by a port of the minimatch 3.1.2 translation. The
bundle the runner ships, [`layoutbin/hashFiles/index.js`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js) of v2.338.2,
contains `@actions/glob` 0.4.0 and minimatch 3.1.2: its `minimatch.js` and its
`internal-pattern.js` equal the files of those two npm packages apart from the
`require` calls. The `@actions/glob`
0.7.0 and minimatch 10.2.5 in the runner's
[package-lock.json](https://github.com/actions/runner/blob/v2.338.2/src/Misc/expressionFunc/hashFiles/package-lock.json)
are not what ships, and a rebuild of the bundle will move the runner to
minimatch 10, so the port and the corpus must be revisited then. The port uses
the options `@actions/glob` passes (`dot`, `nobrace`, `noext`,
`nonegate`, `nocomment`, case-sensitive). The port keeps minimatch's own
behaviour: a `-` after a range is literal, `\-` inside a class becomes a range
operator, `[!]` and `[^]` match any code unit, `\]` escapes, a range written
the wrong way round or ruined by a dropped backslash falls back as minimatch
does, and a segment that has a wildcard never matches a name that starts with a
line terminator. The 65,536-unit pattern limit counts the whole rooted pattern,
with the escaped working directory in front, and also the longer pattern the
runner builds when it adds `/**`. An unclosed class such as `foo[*` is translated again from the
text after the `[`, and the `(?=.)` that minimatch puts in front of that
translation stays, so `foo[*` does not match `foo[`. minimatch's other
lookahead there, the one that rejects a bare `.` or `..`, only fires at the
start of a name, and the re-translated text always follows the `[`, so it can
never fire. A search root keeps no `.`
component, as `path.join` drops it, so `[.]/**` hashes nothing; a pattern whose
first segment is `[.]` or `\.` and that gets implicit descendants fails with
`Relative pathing`, as it does in the runner.
A name that is not valid UTF-8 fails the call when a pattern matches it or can
still match something below it, as it does in the runner on Linux: `readdir`
decodes the name with U+FFFD, the `lstat` of that spelling fails with
`ENOENT`, and nothing catches the error. The `lstat` runs when the entry is
taken from the stack, after the entries the walk reached before it, so the error
names the first such entry in walk order. When a file really is named with
U+FFFD, the `lstat` succeeds and the entry is read under that spelling, once
per entry that decodes to it. A twin whose target leaves the workspace is dropped by the
outside-target symlink rule below. A name no pattern can reach, and every directory
no pattern can reach, is never looked up and fails nothing.
`crates/velnor-runner/tests/it/expr/minimatch_corpus.json` holds recorded
answers of minimatch 3.1.2, about fifty thousand, generated by
`scripts/minimatch-corpus.ts`, which refuses any other minimatch version. Bun
runs that script and writes the file, and a child process of Node 24.22.1, the
V8 of the shipped runner, evaluates minimatch and the regular expressions
(`scripts/minimatch-corpus-node.ts`, which refuses any other Node): the
JavaScriptCore engine of Bun rejects `a{2147483648,2147483647}` where V8
accepts it, so an evaluation on Bun records a different answer for that class.
Run it as `bun scripts/minimatch-corpus.ts <node 24.22.1 binary> <directory
holding node_modules/minimatch@3.1.2> <output.json> [pattern count] [seed]`.
It embeds each segment in a longer path so patterns made of whitespace are
kept, and it samples names around the boundaries of each pattern. Nine
behaviours differ on purpose:

- **Outside-target symlinks are ignored.** The runner only checks that the
  lexical path starts with the workspace
  ([index.js:90](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L90)) and then reads
  through the link, so a symlinked file or a symlinked parent directory pulls
  files from outside the workspace into the digest. Velnor resolves the parent
  chain of each search root and the target of each matched symlink, and drops
  every path whose real location leaves the canonical workspace. Velnor opens
  each directory and file with `openat` relative to its parent's descriptor and
  never follows a final symlink, reads a regular file only after `fstat` shows
  one, and opens the target of a matched link from the workspace root one
  component at a time the same way. The walk keeps a directory descriptor open only at a checkpoint level (every
  level up to 31, then 16 per doubling of the depth, `is_checkpoint` in
  `crates/velnor-runner/src/expr/anchor.rs`) and reopens any other directory
  from the nearest descriptor above it, one component at a time with `openat`
  and no link followed. A branching tree holds about 143 descriptors at 4,096
  levels, so it walks under a limit of 256 open files, and a directory replaced
  by a link while the walk is reopening it fails the call. A path replaced by a link after the walk
  decided about it therefore cannot pull a file from outside the workspace into
  the digest: a replaced file is read through its resolved target only when that
  target stays inside, a replaced directory or link target fails the call, and
  a file replaced by a FIFO fails with `not a regular file` instead of blocking.
  The search root itself is examined with `lstat` and never resolved, as in the runner
  ([index.js:1362](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L1362)),
  and it is examined before its parents are resolved, so a path too long for
  the system fails the call with `ENAMETOOLONG` even when a parent is
  missing, as it does in the runner.
- **Search roots are clamped to the workspace.** The runner walks every search
  root a pattern yields, wherever it lies
  ([index.js:1358](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L1358)), and only afterwards drops matches outside the
  workspace ([index.js:90](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L90)), so `hashFiles('/')` or a bare `~`
  traverses the whole file system or home directory. Velnor narrows a root that
  contains the workspace to the workspace and skips a root outside it, as
  `PatternSet::roots` in `crates/velnor-runner/src/expr/glob.rs` does. Upstream
  fails when its walk outside the workspace hits an unreadable path; Velnor
  returns the workspace digest and the walk stays bounded.
- **`--follow-symbolic-links` is rejected.** The runner accepts the option
  ([HashFilesFunction.cs:52](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/Expressions/HashFilesFunction.cs#L52))
  and follows links
  ([index.js:1446](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L1446)). Velnor never
  follows symlinks and returns the runner's `Invalid glob option` error for it.
- **A class that holds a named or modified group is refused.** Velnor
  translates every segment the way minimatch 3.1.2 does and matches it over
  UTF-16 code units. minimatch tests each class by compiling
  `RegExp('[' + class + ']')` from the pattern text, and JavaScript closes that
  class at the first `]` that no backslash escapes in the regular-expression
  text, which can come before the `]` the pattern's own scan stops at: after
  `\@` the pattern keeps its escape but the regular expression does not, so
  `[a\@])]` compiles as `[a\@]` followed by `)]` and fails, and `[\@]z-a]`
  compiles. Velnor validates the class up to that `]`, then the rest of the
  text as a regular expression (groups, lookarounds, quantifiers including
  braces, escapes and further classes, in the non-unicode grammar of V8 13.6),
  and takes the same fallback minimatch takes when the compilation fails. Two
  syntaxes stay out of reach and fail the call with `which is not supported`:
  a named group (`(?<name>`, including a non-ASCII name such as `(?<é>`), whose
  identifier and duplicate rules need the Unicode identifier tables, and a
  flag group (`(?i:`), which Node 24 accepts and Node 20 rejects. Velnor
  refuses them even where V8 would reject the same text for another reason,
  such as a repeated name.
- **A segment past V8's regular-expression limit fails the call.** minimatch
  builds one `RegExp` per segment and V8 compiles it at the first `test`, so
  the runner's process fails with `Regular expression too large` when a path
  reaches a segment whose compiled form is too large. V8 13.6.233.17, the
  engine of Node 24.22.1, cannot compile a run of more than
  [`kMaxCPOffset`](https://github.com/v8/v8/blob/13.6.233.17/src/regexp/regexp-macro-assembler.h#L35),
  32,767, single-character matchers (characters, `?` and classes) with no
  loop or lookahead between them. Run against the shipped bundle on that Node
  version, `?` repeated 32,767 times hashes and 32,768 times fails, `a`
  repeated 32,767 times before `*` hashes and 32,768 times fails, and 16,384
  `?` on each side of a `*`, or of an unclosed `[` whose `(?=.)` ends the
  run, hash. Velnor counts each run between stars and lookaheads and fails the
  call with the same text once one passes `V8_MAX_TEXT_RUN` (32,767) in
  `crates/velnor-runner/src/expr/segment.rs`, at the first name a path brings
  to the segment. Two V8 behaviours stay out of reach. V8 prunes text that a
  one-byte name cannot match, so a run that holds a character above U+00FF
  fails in the runner only against a name that holds such a character, while
  Velnor fails it for every name. Thousands of wildcards in one segment end in
  V8's `Stack overflow` at a count that depends on the stack the process has
  left, which the size of its environment reduces. The shipped bundle on
  Node 24.22.1 for macOS arm64 fails from 14,678 `*` characters with a
  one-byte environment variable, from 13,140 with a 100,000-byte one and from
  8,524 with a 400,000-byte one.
- **Unclosed classes nest at most 4,096 deep.** minimatch translates the text
  after an unclosed `[` again, once per bracket, and the shipped bundle fails
  with `RangeError: Maximum call stack size exceeded` once the nesting passes a
  depth that depends on the stack the process has left, which the size of its
  environment reduces: on Node 24.22.1 for macOS arm64, `*/` followed by
  brackets hashes up to 4,702 brackets and fails from 4,703 with a one-byte
  environment variable, fails from 4,223 with a 100,000-byte one and from
  2,784 with a 400,000-byte one, and the bundle runs out of memory far above
  that. Velnor translates the chain iteratively, in time and memory linear in
  the pattern, and fails the call above `MAX_CLASS_NESTING` (4,096) in
  `crates/velnor-runner/src/expr/translate.rs`, so a pattern with 4,097 or
  more nested brackets fails here and may hash upstream.
- **A matched file that is not regular fails the call.** A FIFO, or a symlink
  that resolves to one, makes the runner block while `createReadStream` opens
  it, until the function timeout fires. A socket already fails at once in the
  runner, because opening it is an error. Velnor fails at once with
  `<path>: not a regular file` for a FIFO, a socket and a device, and a symlink
  to a directory is
  skipped as it is upstream
  ([index.js:94](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L94)).
  An entry that no pattern matches and that is not a directory, such as the
  socket in `sock/*` or the unreadable file in `sealed.md/*`, is passed over
  and hashes nothing, as in the runner, which reads only directories.
- **A timed-out call leaves its worker to stop itself.** The runner runs
  `hashFiles` in a Node child process and kills the process tree when the
  120-second token fires
  ([HashFilesFunction.cs:102-120](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/Expressions/HashFilesFunction.cs#L102-L120),
  [ProcessInvoker.cs:332-334](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Sdk/ProcessInvoker.cs#L332-L334)).
  Velnor evaluates the call on a worker thread, which cannot be killed. The
  caller gets the `couldn't finish within` error at the deadline, and the
  worker checks the deadline before each entry it visits and each chunk it
  reads, so it ends by itself. Only a worker blocked inside one system call on
  a hung file system outlives the call, and it holds its descriptors until
  that call returns.
- **A `HOME` with `.` or `..` segments is rejected for every `~` pattern.**
  Velnor rejects such a `HOME` for every pattern that starts with `~`. The runner
  rejects it with `Relative pathing '.' and '..' is not allowed` only for
  patterns that get implicit descendants, those that do not end in `**` or that
  end in a separator
  ([index.js:2198](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Misc/layoutbin/hashFiles/index.js#L2198)).
  The pattern quoted in Velnor's error is the `HOME`-expanded pattern text. The
  runner quotes its rebuilt `<collapsed>/**` text.

## Expression memory accounting deviations from the runner

The runner counts memory per function node with a
[`MemoryCounter`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTExpressions2/Expressions2/Sdk/MemoryCounter.cs)
and names the node's expression in the error
([MemoryCounter.cs:28](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTExpressions2/Expressions2/Sdk/MemoryCounter.cs#L28)).
Velnor's counters for `format`, `toJSON` and wildcard projections use the same
sizes and the same 10 MiB limit, and print the same
`ConvertToExpression` text. Four behaviours differ:

- **There is no expression-wide total.** The runner also adds every
  intermediate result of an expression to one `EvaluationMemory`, which fails
  against the same limit
  ([EvaluationMemory.cs:82](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTExpressions2/Expressions2/Sdk/EvaluationMemory.cs#L82)).
  Velnor checks each call on its own, so an expression whose calls each stay
  under the limit but exceed it together succeeds.
- **A wildcard projection charges the strings it copies.** The runner's
  projection shares the string objects and counts only the pointers. Velnor's
  `Value` owns its strings, so a projection copies each string it keeps, and
  it charges the string's size before it copies it. A projection of a string
  larger than the limit fails here and succeeds upstream.
- **A template charges one string overhead per literal run.** The runner turns
  a string with several `${{ }}` spans into one
  [`format('...', ...)` expression](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Sdk/DTObjectTemplating/ObjectTemplating/TemplateReader.cs#L619),
  parses it at the
  [21,000-character limit](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/ExpressionParser.cs#L443),
  and counts its pieces. Velnor rebuilds the same text, fails with
  `Exceeded max expression length 21000` when it is longer than 21,000 UTF-16
  units, parses every span before it evaluates any, and reports the
  `format('...', ...)` text with each span printed as `ConvertToExpression`
  prints it. It evaluates the spans in order and charges one string overhead
  per literal run instead of one per escaped brace. A string without a span is
  returned unchanged and uncharged.
- **A template follows the runner's span rules.** The
  [template reader](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/WorkflowParser/ObjectTemplating/TemplateReader.cs#L630-L645)
  rejects an empty or whitespace-only `${{ }}` span with `An expression was
  expected`, and Velnor does the same. A scalar that is exactly one span stays
  a basic expression, and the
  template evaluator rejects an array result in a string field with
  [`A sequence was not expected`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/WorkflowParser/ObjectTemplating/TemplateEvaluator.cs#L103)
  and an object result with
  [`A mapping was not expected`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/WorkflowParser/ObjectTemplating/TemplateEvaluator.cs#L139). Velnor returns a `String`, so it
  fails a single span that evaluates to a collection with those messages
  instead of keeping its type, while a span among other segments is
  stringified by `format` as `Array` or `Object`. Velnor parses the rebuilt
  `format` call for every template of more than one segment, so its length,
  parameter count and nesting depth fail as upstream does. The call takes at
  most
  [255 parameters](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/ExpressionConstants.cs#L15),
  the format string included, so 255 spans fail with `Too many parameters
  supplied`.

## Expression string comparison deviations from the runner

The runner compares strings with `StringComparer.OrdinalIgnoreCase`, which in
.NET 8 upper-cases each UTF-16 code point with its own casing tables. Velnor
upper-cases with the simple mappings of the Unicode version the Rust standard
library ships (Unicode 18 in the pinned toolchain) and adds the .NET exceptions
for U+0131, U+017F and the Greek letters with ypogegrammeni. Three pairs differ:
U+10D70 and U+10D50, U+1C8A and U+1C89, and U+A7CD and U+A7CC were added to
Unicode after the data .NET 8 carries, so the runner treats each pair as two
different letters while Velnor treats it as one. An expression that compares
such letters, which no workflow in practice does, answers differently.

## Expression functions and contexts of the job runner

The expression engine in `crates/velnor-runner/src/expr/` accepts a fixed allow-list. Contexts: `github`, `runner`, `needs`, `steps`, `inputs`, `secrets`, `env`, `vars`. Functions: `format`, `contains`, `startsWith`, `endsWith`, `hashFiles`, `toJSON`, `fromJSON`, `success`, `always`, `cancelled`, `failure`. Names are matched case-insensitively, and any other name (`job`, `jobs`, `matrix`, `strategy`, `toYAML`, `join`, `case`) fails at parse time with `unsupported expression context <name>` or `unsupported expression function <name>`, the name as written. Expressions that GitHub's service evaluates before it sends the job (`strategy.matrix`, job-level `if`) never reach the runner. The contracts below follow actions/runner v2.338.2.

- **`failure()`.** The runner's [`FailureFunction`](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/Expressions/FailureFunction.cs#L18-L40) returns true for a composite main step when the `action_status` github context equals `failure`, and for every other step when `job.status` equals `failure`. Both default to success. Velnor's runner has no composite steps, so only the second branch exists: `failure()` is true exactly when the job status is `JobStatus::Failure`, and false for success and for cancelled.
- **`vars`.** The service sends the repository, environment and organization variables as the `vars` entry of the job message's context data, a case-insensitive dictionary of strings. The runner copies every context data entry into its expression values unchanged ([ExecutionContext.cs:1001-1006](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ExecutionContext.cs#L1001-L1006)), and the template evaluator lists [`vars` as a named value](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateEvaluator.cs#L485-L496). The runner reads the dictionary only for the debug switches ([ExecutionContext.cs:1398-1414](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ExecutionContext.cs#L1398-L1414)). A key that is not set reads as null, so `vars.MISSING` is the empty string and `vars.MISSING == '1'` is false. `ExprEnv::set_job_contexts` takes the materialized context data of a job message and keeps only the contexts the service owns: `github`, `needs`, `inputs` and `vars`, matched case-insensitively. The runner builds `secrets`, `runner`, `env` and `steps` itself and overwrites whatever the message holds under those names ([ExecutionContext.cs:1009-1010](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ExecutionContext.cs#L1009-L1010), [1044](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ExecutionContext.cs#L1044), [1047](https://github.com/actions/runner/blob/v2.338.2/src/Runner.Worker/ExecutionContext.cs#L1047), and `steps` per step at [StepsRunner.cs:100](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/StepsRunner.cs#L100)), so Velnor drops those entries too; other entries such as `matrix` or `job` stay unreadable. A message without `vars` leaves the context an empty object.
- **Fields the service evaluates.** The generated-workflow test in `crates/velnor-runner/tests/it/expr/generated_workflow.rs` skips every field GitHub's service resolves before it sends the job, because the runner never sees those expressions. GitHub's [context availability table](https://docs.github.com/en/actions/learn-github-actions/contexts#context-availability) lists them with the contexts each may use: `on.*` (including `on.workflow_call.outputs.<id>.value`, which reads `jobs`), `name`, `run-name`, `concurrency` and `permissions` at the workflow level, and `jobs.<id>.runs-on`, `if`, `name`, `needs`, `strategy` (so `strategy.matrix`), `concurrency`, `timeout-minutes`, `continue-on-error`, `permissions`, `uses`, `with`, `secrets` and `environment` at the job level. The runner evaluates only the step fields, the job fields `container`, `services`, `defaults.run`, `outputs` and `environment.url`, the workflow- and job-level `env` ([`JobExtension.cs`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L240-L245), through `EvaluateStepEnvironment`) and `jobs.<id>.snapshot` ([`JobExtension.cs`](https://github.com/actions/runner/blob/d1eb12d4b115c301201de27d667284e1a80f2ab5/src/Runner.Worker/JobExtension.cs#L557)): its [`PipelineTemplateEvaluator`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/PipelineTemplateEvaluator.cs#L56-L375) has `EvaluateStep*` (lines 56-193), `EvaluateJobContainer` (220), `EvaluateJobOutput` (247), `EvaluateEnvironmentUrl` (284), `EvaluateJobDefaultsRun` (311), `EvaluateJobServiceContainers` (348) and `EvaluateJobSnapshotRequest` (375), and no method for the others. The test keeps the `matrix` uses in `declared-workflows` (`build-assets.yml`, `tools-build.yml`) and `reusable-workflows` (`native-checks.yml`) as known parse errors, because those step `env` values read `matrix`, which stays out of scope; each (file, message) entry is asserted to still fail to parse.
- **`endsWith(searchString, searchValue)`.** The runner's [`EndsWith`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/Sdk/Functions/EndsWith.cs#L12-L28) converts both arguments to strings only when both are primitives (null, boolean, number, string), and returns false when either is an array or an object. The comparison is ordinal and ignores case. An empty search value is a suffix of every string. Velnor compares by characters, as `startsWith` does, so a multi-byte suffix never splits a character.
- **`fromJSON(value)`.** The runner's [`FromJson`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTExpressions2/Expressions2/Sdk/Functions/FromJson.cs#L11-L24) converts the argument to a string and parses it with Newtonsoft's `JsonTextReader`, with dates kept as strings and numbers read as doubles; the result is an ordinary context value, so `fromJSON(x).a[0]` works. The argument is any expression, not only a literal, and the object keys are case-insensitive. Velnor parses strict JSON (`serde_json`), so it deviates in these ways: invalid JSON, including the empty string and null, fails with `Error parsing fromJson: <detail>` where the detail is the JSON parser's own message, not Newtonsoft's text; content after the first JSON value is an error (`fromJSON('{} x')` fails), where Newtonsoft stops after the first value; and Newtonsoft 13.0.3's `JsonTextReader` accepts single-quoted strings, unquoted object keys, comments and the literals `NaN` and `Infinity`, all of which Velnor rejects. A number whose magnitude is at or above the halfway point 2^1024 − 2^970 (about 1.797693134862315807937e308) between the largest finite double and 2^1024, so that it rounds outside the double range (`1e400`, `-1e400`, `1.7976931348623159e308`), reads as `Infinity` or `-Infinity`, as .NET 8 `double.TryParse` returns it, where `serde_json` alone reports an error; a number that rounds to the largest finite double (`1.7976931348623158e308`, which has 309 integer digits when written in full) stays finite because `serde_json` reads floats with correct rounding (`float_roundtrip`). Duplicate object keys follow the runner in two steps. First [`JContainer.ReadProperty`](https://github.com/JamesNK/Newtonsoft.Json/blob/13.0.3/Src/Newtonsoft.Json/Linq/JContainer.cs#L916-L939) replaces a property whose name repeats exactly (ordinal) in place, keeping its position. Then [`DictionaryContextData`](https://github.com/actions/runner/blob/v2.338.2/src/Sdk/DTPipelines/Pipelines/ContextData/DictionaryContextData.cs#L113-L119) compares the remaining names without case (`StringComparer.OrdinalIgnoreCase`) and keeps the first name's casing with the last value in that order. Both steps index the names in a hash map, so the cost is linear in the number of keys. `{"a":1,"A":2}` is one entry named `a` with the value 2; `{"A":1,"a":2,"A":3}` is one entry named `A` with the value 2, not 3, because the exact repeat of `A` is replaced first and `a` is merged after it; `ä` and `Ä` merge, while `s` and `ſ` stay apart. The reader's `MaxDepth` is 64 (Newtonsoft 13.0.3 default, [`JsonReader.Push`](https://github.com/JamesNK/Newtonsoft.Json/blob/13.0.3/Src/Newtonsoft.Json/JsonReader.cs#L336-L361)), so the 65th nested array or object fails, however deep the input is. The runner shows Newtonsoft's raw message without a prefix: `The reader's MaxDepth of 64 has been exceeded. Path '<path>', line <n>, position <n>.` The path lists the 64 open ancestors (`[index]` for arrays, `name` joined by `.` for objects, `['name']` with `\'` and the other [escapes](https://github.com/JamesNK/Newtonsoft.Json/blob/13.0.3/Src/Newtonsoft.Json/Utilities/JavaScriptUtils.cs#L147-L247) when the name holds a dot, space, apostrophe, slash, double quote, square bracket, parenthesis, backslash, tab, newline, carriage return, form feed, backspace, `U+0085`, `U+2028` or `U+2029`); the line and position are those of the offending bracket, counted in UTF-16 code units, with `\n`, `\r` and `\r\n` each ending a line. Velnor recognises the depth itself before `serde_json` runs, so 65, 128 and 200 levels all give this message, and a syntax error that comes first in the text keeps Velnor's own `Error parsing fromJson: <detail>` wording. Every other `fromJSON` error uses that Velnor-specific wording (the runner shows Newtonsoft's text for them).

## Events that may write the runner's stores

`velnor-runner` lets a job write a shared store (the cache and the generations)
only when `TrustedFacts::may_write_stores` is true. The facts come from the
host's verified source, never from the job: `event_name` is a required field of
the `--trusted` file, and `exec` without that file reads it from the job
message's `github.event_name`.

- **Allowed:** `push`, `schedule`, `workflow_dispatch`, `repository_dispatch`
  and `release` on a branch or a tag, and a `pull_request` whose head is the
  base repository. Each writes the scope of its own ref.
- **Denied:** a fork `pull_request` (or one whose head repository is gone),
  `pull_request_target`, `issue_comment`, `workflow_run`, `merge_group`, a
  `push` to a merge-queue branch (`gh-readonly-queue/*`), a `push` or other
  event on a pull request ref, and every event name not listed above, such as
  `delete`, `registry_package` and `page_build`. Any new event name is denied
  until it is added here.
- **Why the ref alone is not enough:** `pull_request_target`, `issue_comment`
  and `workflow_run` run on `refs/heads/<default branch>`, so the ref would
  classify them as a branch run and let them write the default-branch scope.
  The event name decides, and the ref must agree with it.
- **Unqualified refs:** a merged-pull-request `closed` event carries a ref such
  as `main`. The runner qualifies it to `refs/heads/main`, as
  [actions/checkout](https://github.com/actions/checkout/blob/v7.0.1/src/input-helper.ts#L84-L88)
  does. The event name is still `pull_request`, so the run is denied.

### Difference from actions/cache

The toolkit holds no event list. `@actions/cache` asks the cache service for a
reservation and treats the answer as the policy
([`cache.ts`](https://github.com/actions/toolkit/blob/main/packages/cache/src/cache.ts),
[`RELEASES.md`](https://github.com/actions/toolkit/blob/main/packages/cache/RELEASES.md)):

- Since 6.1.0 a reservation refused with the `cache write denied:` prefix
  becomes a `CacheWriteDeniedError`, logged as a warning, and the job continues.
- Since 6.2.0 a download refused with `cache read denied:` is a warning too,
  and the `ACTIONS_CACHE_MODE` variable is honored: a restore is skipped for
  `none` and `write-only`, a save is skipped for `none` and `read`.

The service decides what a run may do:

- Since [2026-06-26](https://github.blog/changelog/2026-06-26-read-only-actions-cache-for-untrusted-triggers/)
  it issues a read-only cache token when the event is untrusted (someone other
  than a collaborator can trigger it) and the scope comes from the shared
  default-branch SHA: `pull_request_target`, `issue_comment` and a fork
  `workflow_run` cascade. `push`, `schedule`, `workflow_dispatch`,
  `repository_dispatch`, `delete`, `registry_package` and `page_build` keep
  read-write access, and so does any trigger with a non-default-branch scope,
  such as `pull_request` and `release`.
- Since [2026-09-10](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode/)
  a workflow or a job can set `cache-mode` to `read`, `write`, `write-only` or
  `none`. The service enforces it, a called workflow cannot exceed its caller,
  and an explicit `write` on a low-trust event overrides the read-only default
  with a warning annotation.

`TrustedFacts::may_write_stores` is the host-side counterpart for the runner's
own stores. It follows the same split, with three deliberate differences:
`merge_group` and a push to a merge-queue branch are denied (the service lists
neither, so Velnor fails closed), a fork `pull_request` is denied although the
service gives it a scope of its own, and `delete`, `registry_package` and
`page_build` are denied until someone needs them. The predicate does not read
`cache-mode`: it is evaluated first, from the host's facts, and a workflow
setting cannot widen it.
