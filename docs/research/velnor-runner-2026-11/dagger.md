# Dagger: architecture dossier (facts only)

Researched 2026-11-19. Facts only; no recommendations.

Tags: **[vendor]** = Dagger's docs, repo docs, changelog, blog. **[independent]** = third party, or
a user report in a public thread. **[code]** = read directly from the dagger/dagger source tree
(shallow clone of `main` at commit `fcce0f8`, 2026-11-18, at `~/.cache/ci-ambition/refs/dagger`).
Code is Dagger's own, so [code] is a primary source for what the code does, not for performance
or marketing claims. "Not public" = I found nothing public.

Source caveat: Dagger is mid-transition. `main` is ahead of the newest tagged changelog entry
(v0.21.7, 2026-06-17); the website changelog lists a "v1.0 beta" on 2026-10-22. The public docs
site mixes versions (stable, `next`, `devel`), and several docs URLs I tried returned 404.
Much of the older material you will find online (BuildKit-based engine, `dagger run`/`dagger call`
centric docs, layer cache vs. volume cache) describes the pre-v0.21 design. Where I can,
I say which generation a fact belongs to.

---

## 1. What Dagger is

- Repo README, current: "Dagger is a platform for automating software delivery. It can build, test
  and ship any codebase, reliably and at scale." Runs "locally, in your CI server, or directly in the
  cloud." [vendor] https://github.com/dagger/dagger (README.md)
- Docs intro (current): "Dagger is third-generation CI, after Jenkins and GitHub Actions. It is built
  to match the speed and scale of coding agents and give them the guardrails and context they need."
  It "runs every code change, whether machine-generated or handwritten, through the same build and
  test pipelines, before or after push." [vendor] docs/current_docs/getting-started/introduction.mdx
- Components, per README and docs:
  - **Engine**: serves the Dagger API and runs pipelines; open source; stores the cache.
    "Runs on a local machine, on a server, or as a Cloud Engine." [vendor] glossary.mdx
  - **API**: a typed GraphQL API ("DagQL" is the in-repo GraphQL implementation, `dagql/`). Core types
    include `Container`, `Directory`, `File`, `Secret`, `Service`, `GitRepository`, `CacheVolume`,
    `LLM`, `Env`. [code] `core/`, `dagql/`; [vendor] docs/reference/api/*
  - **SDKs**: README says "SDKs for 8 languages": Go, Python, TypeScript, PHP, Java, .NET, Elixir,
    Rust; "each SDK is generated from the API schema." [vendor] README. The repo `sdk/` dir also has
    a `cue` directory. [code] A further Dagger-authored language, Dang (`vito/dang`), exists as a module
    SDK. [vendor] docs/reference/sdks/dang.mdx
  - **Modules**: "a reusable, typed API for automation: functions, objects, Checks, services, and
    Generators that anyone can run from a workspace, in CI, in Cloud, or from an agent." Written with
    an SDK. [vendor] glossary.mdx. The Daggerverse is Dagger's public module index; I did not
    re-verify its current state in this pass (docs mention "official modules" shipped in-repo under
    `modules/`). [code]
  - **Workspace / Checks (new in the 0.20-0.21 line)**: `dagger.toml` + `dagger.lock` at project
    root list installed modules. `dagger check` "runs all checks in your workspace in parallel. Exits
    non-zero if any check fails. Results are identical on your laptop, in CI, and in the cloud."
    [vendor] using/checking.mdx. Modules discover project units (Go packages, test files, linter
    configs) at run time as typed "collections" and expose them as Checks; `dagger check -l --all`
    lists each. `dagger generate` runs Generators that return reviewable changesets; v0.21.0 adds an
    automatic `check` per `generate` function (staleness check). [vendor] CHANGELOG v0.21.0
  - **CLI**: `dagger check`, `dagger generate`, `dagger call`, `dagger run` (older docs),
    `dagger workspace activity`, `dagger cloud checks on`, `dagger lock update`, a TUI, a
    `--engine` selector. [vendor] docs/reference/cli
- Older generation (pre-0.20): the user wrote a "Dagger Function" in their SDK language, ran it with
  `dagger call` / `dagger run`, and the SDK sent GraphQL to the engine over a session. [vendor]
  archived docs; the engine API is still GraphQL. Not re-fetched in this pass.
- Company: Dagger, Inc. (NOTICE file reads "Copyright 2012-2017 Dagger, Inc."; this is the
  legal-boilerplate line, not a founding date). [code]

## 2. Execution model

### Engine, BuildKit, DAG
- Historically the engine embedded BuildKit; every API call built an LLB graph that BuildKit's
  solver executed and cached. [independent/vendor mix; see Go docs for `engine/buildkit`
  https://pkg.go.dev/github.com/dagger/dagger/engine/buildkit]
- **Current state: the BuildKit solver is removed.** v0.21.0 (2026-05-22): "Migrate caching to DagQL
  and remove the BuildKit solver backend, making DagQL responsible for cache lookups, persistence,
  and pruning." [vendor] CHANGELOG.md L161 (PR #11856). The website changelog calls it "Project
  Theseus": "Dagger replaces the BuildKit solver with its own engine ... the largest engine change
  since the project started." [vendor] https://dagger.io/changelog/
- `go.mod` on `main` has zero references to `buildkit`. [code] BuildKit-derived code remains
  vendored/copied in places (`cmd/engine/BUILDKIT_AUTHORS`, `cmd/engine/NOTICE`; snapshotter,
  containerd leases, CNI namespace pooling) and the engine config still accepts a "buildkit-style
  `engine.toml`". [code] `helm/dagger/values.yaml`; CHANGELOG v0.21.0
- So the current execution graph is the DagQL result DAG, not an LLB DAG. Each result "can depend on
  other cache-backed results, and those dependencies form the result DAG" (structural deps from
  receiver/args/implicit inputs/module; explicit deps added via `AddExplicitDependency`).
  [vendor] internal-docs/cachebasics.md (in-repo engineering doc)
- Evaluation is lazy: results are "detached" until materialised; values compute on demand.
  [vendor] internal-docs/lazy_evaluation.md; docs: "Dagger computes artifacts lazily, as an
  incremental DAG of sandboxed functions, on your machine or on a shared remote engine." [vendor]
- Containers per operation: `withExec` runs the command in a container (OCI runtime, containerd
  snapshotters, CNI networking). "Tools run in containers, orchestrated by sandboxed functions."
  [vendor] README; [code] `engine/snapshots`, `core/container_exec.go`. Module functions
  themselves run in a container built from the module's SDK runtime.
- v0.21.0 perf notes: reduced bbolt/containerd metadata overhead, lazy containerd leases, pooled CNI
  namespaces, batched `withDirectory` chains "instead of materializing them quadratically".
  [vendor] CHANGELOG.
- Platform: "The only requirement is a Linux container runtime. Runs natively on Linux, or via Docker
  Desktop and similar products on macOS and Windows." [vendor] README. A Windows Changeset-export fix
  appears in the changelog, so a Windows host client exists. [vendor]

### Provisioning
- The CLI picks an engine from `--engine` / `DAGGER_ENGINE`. Schemes in the CLI catalog:
  `cloud`; `image://IMAGE` (also `image+docker|apple|podman|finch|nerdctl`); `container://NAME`
  (same runtimes); `tcp://HOST:PORT` ("no authentication"); `tls://`; `ssh://`; `kube-pod://POD`;
  `unix://PATH`. [code] `engine/client/drivers/schemes.go`
- Default on a dev machine: "the CLI starts a local engine by default." [vendor] self-hosting/index.mdx.
  Engine runs as an OCI container started via Docker/Podman/Apple container etc. [code] drivers/
- A matching CLI/engine version is required: "Install the CLI in the CI job at the same version as the
  engine." [vendor] self-hosting/index.mdx
- Engine config: `/etc/dagger/engine.json` (log level, registries/mirrors, GC, security, resource
  metrics); unknown keys stop the engine starting. [vendor] reference/config-files/engine-json.mdx

### Session and attachment model
- One engine serves many clients. A client opens a session; the engine multiplexes it. The client
  provides "attachables" back to the engine over the session: filesystem sync (local dirs, "streams
  local files, secrets, network tunnels"), secrets, sockets (e.g. SSH agent), registry auth.
  [vendor] glossary.mdx (remote engine entry); [code] `engine/client/filesync.go`, `socket.go`,
  `secretprovider/`, `auth.go`
- Session resources (secrets, sockets) are cached as **handle-form** objects: "If a cache hit has a
  direct or transitive dependency on a session resource, the hit is only valid if the caller's session
  has already loaded that session resource handle." Equivalent secrets can share cache, but a session
  that never loaded the resource cannot receive another client's resource-backed result.
  [vendor] internal-docs/session_resources.md
- v0.21: in-flight equivalent DagQL calls are de-duplicated across clients in the same session.
  [vendor] CHANGELOG v0.21.0
- Host access is policy-gated (`engine/client/hostpolicy.go`, `core/` workspace/host code). [code]
  Details beyond that: not public in docs I could fetch.

---

## 3. Caching

### 3.1 What goes into a key (current DagQL engine)
- The cache key is "the identity derived from this structured call" (`ResultCall`): field name,
  return type, receiver, **module provenance** (the module is itself a result reference), explicit
  args, implicit inputs, view, nth list selection, and "extra digests." [vendor]
  internal-docs/cachebasics.md
- Request-only policy (not part of identity): `ConcurrencyKey`, `TTL`, `DoNotCache`, `IsPersistable`.
  Fields can adjust these via "dynamic input functions" that run before cache lookup. [vendor]
  internal-docs/dynamicinputs.md
- Four identity layers: recipe digest; extra digests (chiefly **content digest**); a structural term
  (`selfDigest + canonical input eq-classes -> output eq-class`); a local shared-result ID. A
  union-find over digest equivalence classes ("e-graph") lets different recipes that provably produce
  the same content reuse one result. [vendor] internal-docs/egraph.md (the doc says "not a full rewrite
  engine or a generic academic e-graph")
- Docs-level statement: "Dagger also caches function results, keyed by inputs (arguments, parent
  state, module source). A function cache hit skips the function entirely." [vendor] glossary.mdx
- How file/dir inputs are hashed: directories and files are content-addressed snapshots; host paths
  are synced via the filesync attachable and mirrored (`ClientFilesyncMirror`), with content
  digests. [vendor] internal-docs/mutablecache.md, filesync.md. The exact hash function for
  directory trees: I did not trace it (not documented in the user docs).
- Remote Git: v0.21.0 "fixes remote Git tree cache keys so cache reuse follows the actual checkout
  inputs." HTTP fetches use ETag/Last-Modified conditional requests and keep the snapshot unless the
  content digest changed. [vendor] CHANGELOG; internal-docs/mutablecache.md
- Function cache policy: `Default` (cache by inputs, optional `ttl`), `PerSession`, `Never`.
  `Never` "forces the function to run every call but does not disable layer caching for the operations
  inside it." A function miss "runs it, but individual operations inside may still hit the layer cache."
  [vendor] docs/reference/sdks/dang.mdx L939-945; same enum in `core/schema/module.go`.
- Two cache levels therefore exist today: function-result cache and per-operation (layer) cache. [vendor]

### 3.2 Layer cache vs. cache volumes
- Layer/operation cache: immutable, content-addressed results.
- `CacheVolume`: "a directory whose contents persist across Dagger sessions, such as a package
  manager's download cache." [vendor] glossary.mdx; partials/types/_cache-volume.mdx. It is mutable
  state, not part of a key's content. Internally it is a cache object with snapshot accounting
  (persisted, pruned, size-counted) like the other mutable-backed objects. [vendor]
  internal-docs/mutablecache.md
- Older docs (0.9-era, archive): "layers cache" vs. "volumes cache"; volumes cache "is maintained
  locally, and where applicable is replicated to Dagger Cloud." [independent summary of vendor
  archived docs] https://archive.docs.dagger.io/0.9/cloud
- Whether concurrent writers to one cache volume are isolated: sharing modes (shared/private/locked)
  existed historically in the API; I did not re-verify them on current `main`.

### 3.3 Invalidation and correctness caveats
- Non-hermetic steps are the user's responsibility. A `withExec` is keyed by its inputs, not by what
  it reads from the network or clock. Dagger offers escape hatches: `@cache(policy: Never)`, TTLs,
  and the common `withEnvVariable("CACHEBUSTER", time)` idiom. [vendor] dang.mdx cache-policy section
  for the first two; the CACHEBUSTER idiom is from older docs, not re-fetched.
- Network: remote Git refs and container image tags are resolved live unless locked. v0.21.0 adds
  opt-in lockfile modes `--lock live|pinned|frozen`, and `dagger lock update`. "Locking is opt-in."
  [vendor] CHANGELOG v0.21.0
- Time: no automatic clock input; TTL-based invalidation applies to the `Default` policy. [vendor]
- Secrets: "Secrets are never exposed in logs, written to container filesystems, or inserted into the
  cache." [vendor] glossary.mdx. Secret-dependent results are session-conditional (see 2.3). The
  1Password provider caches resolved values for a run. [vendor] CHANGELOG
- Persistence is "best effort": the DagQL cache is "fundamentally an in-memory cache"; state is
  checkpointed to SQLite on graceful shutdown and reloaded on start; an ungraceful kill may lose it;
  suspect state is wiped and the engine cold-starts. [vendor] internal-docs/cache_persistence.md
- Known bug classes visible in the changelog (v0.21.x): "ephemeral `missing shared result` errors after
  cache persistence or engine restarts"; module source cache dependency retention; cache entries without
  computed size during prune. [vendor] CHANGELOG v0.21.x
- Pruning: `gc.maxUsedSpace` (default 75% of disk), `reservedSpace` (10% up to 10GB), `minFreeSpace`
  (20%); entries are pruned whole; policies configurable. [vendor] engine-json.mdx. (An old issue
  stated the BuildKit-inherited default as keeping cache under 10% of disk; the maintainers called it
  not great. [independent] https://github.com/dagger/dagger/issues/7392 snippet via search; not
  re-fetched.)

### 3.4 Dagger Cloud distributed cache
- Marketing/docs: Cloud Engines are "Dagger engines that we run for you, with scale-out, distributed
  cache and observability." [vendor] https://dagger.io/changelog/ (22 Oct 2026)
- Mechanism visible in code: each engine keeps one WebSocket to a remote-cache service
  (`/v1/engine`), speaks JSON "Envelope" messages (hello/welcome, export, upload, merge, offer, renew,
  lookup, open/settle; 64 MiB message cap; token-revocation close code 4001). The engine "serves from
  its first second whether or not the connection is up." Cache parts are offered to the engine as
  download addresses; uploads run 16 blobs concurrently (measured per code comment). [code]
  `engine/remotecache/remotecache.go`, `protocol/protocol.go`
- Engine entries carry a "Cloud holding": a record of the Cloud entry, whether a record is stored and
  its expiry, and the Cloud's complete parts with their offers. [code] `dagql/cache_cloud_holding.go`
- Helm chart has a `magicache:` block with `url: https://api.dagger.cloud/magicache` and a token
  (secret). So the older self-hosted-engine-plus-Cloud-cache path is configured by token. [code]
  `helm/dagger/values.yaml` L228-233, `templates/magicache-secret.yaml`
- Scope and trust: archived docs said cache "is namespaced by Dagger Cloud Organization name ...
  no cache data is shared across organizations." [independent summary of vendor archived docs]
  https://archive.docs.dagger.io/0.9/cloud. Per-branch, per-PR, per-fork or trust-tier scoping within an
  org: not public in anything I could fetch. How a poisoned/untrusted write is prevented from reaching
  main: not public.
- Scheduling: for Cloud Engines, "Dagger Cloud uses engine telemetry to find the best balance between
  compute and cache." A "scheduler v2" became default on 2026-05-06 (v0.20.8) and assigns compute
  slots. [vendor] self-hosting/index.mdx; dagger.io/changelog
- Cost (https://dagger.io/pricing, fetched 2026-11-19) [vendor]:
  - Starter $20/mo (1 user, Cloud Traces 1 month retention, SOC 2 compliant)
  - Scale $200/mo (unlimited users, access control, traces 1 year)
  - Enterprise custom (SSO, 24/7 support, custom retention)
  - Usage: pipeline CPU 0.5c per CPU-min new / 0.1c repeat; memory 0.5c / 0.1c per GB-min;
    inputs 2.5c / 0.5c per GB; outputs 10c / 2c per GB. "You pay 5x less for operations that could be
    served from cache"; "repeat work" = same inputs as an operation run in the last 30 days.
    Billing = max(plan price, usage). The page does not describe cache storage limits or runner sizes.
  - Earlier docs: individual-plan subscribers lacked the experimental distributed cache. [independent
    summary of vendor docs]
- Cloud Engines on arm64: "coming soon", undated. [vendor] changelog

---

## 4. Local and CI parity

- Claim: "Local and CI behavior are identical"; "The Checks that run on each push are the same Checks
  that `dagger check` runs on your machine, so the two cannot drift apart." [vendor] README;
  getting-started/cloud-checks.mdx
- Mechanism: the same engine version and the same module code; functions are sandboxed with
  "host dependencies explicit and strictly typed"; tools run in containers; the CLI and engine must be
  the same version. [vendor] README; self-hosting/index.mdx
- Where host difference still enters (from the vendor's own text):
  - Host files, env vars, secrets, sockets enter through attachables; their content is part of the
    inputs, so a different checkout gives a different key. [vendor]
  - CPU architecture: containers run on the engine's architecture; Apple Silicon engines run arm64
    unless a platform is specified. Cloud Engines were amd64-only with arm64 "coming soon" (see 3.4).
    [vendor] changelog (the arm64 line); the general platform rule is my reading of the OCI model,
    not a Dagger quote.
  - OS: engine needs Linux; on macOS/Windows it runs in a VM via Docker Desktop. [vendor] README
  - Engine config (registries/mirrors, GC, resources) is per engine. [vendor] engine-json.mdx
  - Resource limits on the engine host (CPU, memory, disk) differ; v0.21+ sends CPU/memory metrics to
    OTel. [vendor] changelog (Sep 30 note via search summary)
- A user report says a fully cached run of a Rust monorepo pipeline took ~18 s versus ~4 s running
  cargo directly (Dec 2023); maintainers asked which operation took the time and suggested large host
  context transfer. [independent] https://github.com/dagger/dagger/discussions/6324

---

## 5. CI integration

- Dagger does not replace the CI trigger in the self-hosted model: "A CI job on another platform, for
  example GitHub Actions or GitLab CI, runs `dagger check`. The CLI in that job connects to a
  self-hosted engine." [vendor] self-hosting/index.mdx
- Docs list triggers for GitHub Actions, CircleCI, AWS CodeBuild and others under "adopting/triggers".
  [vendor] https://docs.dagger.io/next/adopting/triggers (page itself now only describes Cloud Checks)
- **Cloud Checks (new, 2026-10-22):** "runs your checks on Cloud Engines each time you push to GitHub or
  open a pull request"; "You do not need a CI workflow file." Requires a GitHub repo whose `origin`
  points at it; enable with `dagger cloud checks on` (installs the Dagger Cloud GitHub App); watch with
  `dagger workspace activity`. [vendor] getting-started/cloud-checks.mdx; dagger.io/changelog
  - `dagger check --scale-out` exists; `dagger generate --scale-out` always runs locally. [vendor]
    CHANGELOG v0.21.0
- **Engine cold start in ephemeral CI:** "If [`DAGGER_ENGINE`] is not set, the CLI starts an engine on
  the CI runner. On an ephemeral runner, the cache of that engine is lost after each job." "Do not run
  engines on a pool of ephemeral machines. Each new machine starts with an empty cache." "A new engine
  adds compute, but its cache is empty. Thus, a new engine can make checks faster or slower."
  [vendor] self-hosting/index.mdx
- **Kubernetes (Helm):** chart `oci://registry.dagger.io/dagger-helm`; "By default, the chart deploys
  the engine as a DaemonSet: one engine on each node. The engine keeps its cache on the disk of the
  node. A CI runner connects to the engine on its own node." Recommends "persistent nodes with large,
  fast disks, for example local NVMe disks. If the cluster replaces a node, the cache of that engine is
  lost." Runner pods mount the engine socket via `hostPath` (`/run/dagger-RELEASE-dagger-helm`;
  socket group 1001) and set `DAGGER_ENGINE=unix:///run/dagger/engine.sock`. Alternative:
  `kind: StatefulSet` with optional PVC (default 10Gi request) for multiple engines per node; pod
  access via `kube-pod://POD?namespace=NS`. Chart version selects the engine image; must equal CLI
  version. [vendor] self-hosting/index.mdx; [code] helm/dagger/values.yaml, templates/
- Scaling guidance: one engine on a bigger machine first; then split by check across engines (one pool
  per check); find the limiting resource (CPU, memory, disk IO); optionally push a check to Cloud
  Engines. No cross-engine cache sharing in self-hosting without Cloud ("A self-hosted setup does not
  have this scheduler"). [vendor] self-hosting/index.mdx
- Security: a `tcp://` engine has no authentication; "any system that can connect to the port can run
  containers on the engine"; TLS client certs or `kube-pod://` RBAC are the alternatives. [vendor]
- The engine needs privileged execution (it runs containers); the chart's `securityContext` options
  are in `values.yaml`. [code]
- Vendor doc for a warm-up procedure (pre-pulling images, seeding cache): not public beyond the above.

---

## 6. Observability

- "Every operation emits a full OpenTelemetry trace, enriched by granular logs and metrics. Visualize
  the trace directly in the terminal, or in a web view." Traces can be exported to Jaeger, Honeycomb or
  any OTel backend. [vendor] README
- CLI: live TUI; progress modes (pretty, plain, logs, dots, report) and an interactive tests view
  (v0.21.0); `dagger trace` replays traces; the Cloud trace URL is printed with the logs frontend.
  [vendor] CHANGELOG v0.21.0
- Engine-side telemetry is stored in a "telemetry store" and ingested/published via OTLP
  (`engine/client/telemetry_*.go`, `engine/telemetry`, `dagql/dagui`, `dagql/idtui`). [code]
- Dagger Cloud: traces UI; retention 1 month (Starter) / 1 year (Scale) / custom. [vendor] pricing.
  v1.0 beta "sees each of your tests, not only the job that runs them" (per-test visibility, rerun of
  failing tests individually). [vendor] dagger.io/changelog (22 Oct 2026)
- Cache hit visibility: the engine counts cache spans (`engine/server/cachespancount.go`) and exposes
  `EngineCacheEntry.dagqlCall` / `recordTypes` for inspection. [code]; CHANGELOG v0.21.0.
- OTel resource metrics (CPU/memory) sent to the backend: reported in the 30 Sep 2026 changelog via a
  search summary; not re-verified on the primary page. [vendor, unverified]

---

## 7. AI and agents

### LLM, Env, Agents (in the engine)
- `LLM` is a core API type: "initializes a Large Language Model"; it "acts through tools."
  `withWorkspace` exposes a workspace's modules as tools; `withTools` exposes an object's methods;
  built-in Dang tools `DangCheck` and `DangEval` always exist (`DangEval` can make any API call,
  including containers with network access). [vendor] partials/types/_llm.mdx
- `Env` declares typed inputs and desired outputs. Pattern: `dag.llm().withEnv(env).withPrompt(...)`.
  `withEnv` "allow[s] the LLM to interact with an environment via MCP"; `withMCPServer(name, service)`
  adds an external MCP server; `withSystemPrompt`, `withModel` (with provider), `tokenUsage`.
  [independent summary of vendor docs, via search] https://docs.dagger.io/api/llm,
  https://docs.dagger.io/getting-started/types/env
- `Agent` (new, in `core/agent.go`): "a conversation loop packaged as an addressable, long-lived entity
  within the session"; spawned from `LLM.spawn`; has an agent-control channel (`engine/agentcontrol`)
  and message/stop/reseed/rewind handling. [code]; design doc `hack/designs/async-agents.md` is cited in
  the source but not in this clone (not verified).
- LLM provider/model catalog and config live in `core/modelcatalog` and `engine/llm_config.go`. [code]
- Modules can be exposed as an MCP server for clients such as Claude Desktop/Cursor; older docs limited
  this to modules without required constructor args. [independent summary of vendor docs]
- The repo ships agent skills (`skills/`, e.g. engine-debugging, docs-writing-style) and `evals/`.
  [code]

### container-use
- Separate repo `dagger/container-use` (CLI `cu`). "An open-source MCP server that runs coding agents
  in isolated containers so parallel tasks don't collide." Each environment = a container + a Git
  branch (worktree-backed; a bare repo and worktree are created under `~/.config/container-use/`).
  Requires Docker and Git; "comes with Dagger under the hood." `cu watch`, `cu terminal <env>`,
  `cu merge`. [vendor] https://dagger.io/blog/agent-container-use ; [independent] InfoQ
  https://www.infoq.com/news/2025/08/container-use and pkg.go.dev for the worktree layout.
- Vendor claims on the post: every command and output is recorded, "a perfect audit trail"; "the same
  environment runs identically on a laptop and in CI because Dagger Engine powers both"; "content-
  addressable caching keeps common operations fast across many parallel environments." The post
  also says "Container Use is in early development." No benchmarks or case studies are given. [vendor]
- A Zed write-up notes git worktrees alone still leave AI-written code running unsandboxed on the host;
  container-use adds the container. [independent] https://zed.dev/blog/container-use-background-agents

### Agent verification claims
- Positioning: Dagger "runs every code change, whether machine-generated or handwritten, through the
  same build and test pipelines, before or after push." Checks are the gate; modules "read your native
  configuration." Test results can be "a pre-merge or post-merge gate." [vendor] introduction.mdx
- "Vertically integrated: sandbox, scheduler, container runtime, build cache and tracing share one
  execution and data model," which it says gives "compute and token efficiency that are out of reach
  for second-generation CI." [vendor] introduction.mdx. No published measurement backing "token
  efficiency": not public.

---

## 8. Published performance claims, known limits, criticisms

### Vendor performance claims
- "Every operation is incremental by default"; "Change one file and only the affected operations
  re-run." [vendor] README
- Cloud pricing: repeat work billed 5x lower (see 3.4). [vendor]
- TypeScript SDK cold start: bundling cut module init from ~21 s to ~11 s average (from ~155 MB of
  deps to ~4.5 MB bundle); bundling `tsx` alone gave 7-10 s on a cold cache; baseline "20-30 seconds to
  every cold start." [vendor] https://dagger.io/blog/typescript-sdk-performance (Jul 2025)
- Older changelog notes: module loading "~23% / ~74% / ~91% faster" in listed cases. [vendor]
  CHANGELOG L1165-1183
- v0.21.0 engine perf and memory work (see 2.1). No before/after figures in the entry. [vendor]
- Cloud scheduler v2 "assigns compute slots and supports per-customer tuning"; no numbers. [vendor]
- Published head-to-head benchmarks vs. GitHub Actions, Buildkite, Bazel, etc.: not public in anything
  I found.

### Known limits and criticisms
- Cold engines: the vendor's own statement that fresh engines have empty caches and "can make checks
  faster or slower" (sec. 5). [vendor] Persistent in-memory cache design loses state on ungraceful exit
  (sec. 3.3). [vendor]
- Per-run overhead: 18 s cached vs 4 s native (2023 user report). [independent]
  https://github.com/dagger/dagger/discussions/6324. Age is relevant: the engine has since changed.
- Cache size and GC: BuildKit-inherited GC defaults were criticised in a Dagger issue; on Kubernetes
  engine memory can "spike well above that" requested. [independent] https://github.com/dagger/dagger/issues/7392
  (via search snippet; not re-fetched). The current default `maxUsedSpace` is 75%. [vendor]
- Complexity: three layers at once (SDK, module, engine) plus the new workspace/check model; the
  migration doc `config/migrate-dagger-json.mdx` lists API casing changes with "no deprecated aliases".
  [vendor]. Public independent critiques of the complexity: only forum-level (Hacker News mirror
  threads on BuildKit that mention Dagger removing BuildKit); I found no published third-party review.
- BuildKit-removal risk: the largest engine change since the project began shipped 2026-05; the
  changelog has a sequence of v0.21.x fixes for cache persistence/retention. [vendor]
- Host-context transfer cost: the host directory loads fully by default, so large contexts slow runs
  (maintainer-collaborator comment in the discussion above). [independent]
- Self-hosted limits: no cross-engine shared cache without Cloud; the doc tells operators to hand-
  assign checks to engines. [vendor]
- Lock-in: see 9.

---

## 9. Licence; open-source vs. commercial

- Engine, CLI, SDKs, docs: Apache License 2.0 (`LICENSE`, 191 lines, "Apache License, Version 2.0").
  SDK manifests state Apache-2.0 (TypeScript `package.json`; Python classifier "Apache Software
  License"). [code]
- "The Dagger Engine is open source. It runs on any system that can run Linux containers, with or
  without a Dagger Cloud account." [vendor] self-hosting/index.mdx
- Commercial: **Dagger Cloud**: traces UI and retention, Cloud Engines (managed remote engines,
  scale-out scheduler, distributed cache, access control), Cloud Checks (GitHub-triggered CI on those
  engines), SSO and support tiers. Pricing in 3.4. [vendor] glossary.mdx, pricing page.
- The Cloud-side server of the remote-cache protocol (the `magicache` service at
  `api.dagger.cloud`) is not in the open repo; the engine-side client and wire protocol are. [code]
  `engine/remotecache/`. The service implementation, storage backend, and retention: not public.
- Portability points: workflow code lives in the user's repo as modules in 8+ SDK languages; checks
  run on any engine the user hosts. The cache-sharing scheduler is Cloud-only (self-hosting doc).
  Cloud Checks triggers need the Dagger GitHub App. [vendor] These are factual features of the
  split, not an assessment.
- Module registry (Daggerverse): hosted by Dagger; terms and uptime commitments: not public here.
- Contributor/IP terms (CLA/DCO): not checked in this pass.

---

## Source list
- Repo (read-only clone, commit fcce0f8): README.md, CHANGELOG.md, NOTICE, LICENSE, helm/dagger/,
  docs/current_docs/{getting-started,self-hosting,using,reference}/, internal-docs/{cachebasics,
  egraph,dynamicinputs,mutablecache,session_resources,cache_persistence}.md, engine/remotecache/,
  engine/client/drivers/, dagql/cache_cloud_holding.go, core/agent.go.
- https://dagger.io/pricing ; https://dagger.io/changelog/ ; https://dagger.io/blog/agent-container-use ;
  https://dagger.io/blog/typescript-sdk-performance
- https://github.com/dagger/dagger/discussions/6324 ; https://github.com/dagger/dagger/issues/7392
- https://archive.docs.dagger.io/0.9/cloud ; https://docs.dagger.io/api/llm ;
  https://docs.dagger.io/getting-started/types/env
- https://www.infoq.com/news/2025/08/container-use ; https://zed.dev/blog/container-use-background-agents ;
  https://pkg.go.dev/github.com/dagger/container-use

## Gaps (not public or not verified)
- Cache-key hashing for directories and exec results at the byte level; hermeticity enforcement.
- Distributed-cache trust model, per-branch/PR/fork scoping, poisoning defence, storage limits.
- Cloud Engine hardware specs, isolation model between tenants, SLAs.
- Independent benchmarks of v0.21+ (post-BuildKit) engines.
- Warm-up/seed procedures for new engines.
