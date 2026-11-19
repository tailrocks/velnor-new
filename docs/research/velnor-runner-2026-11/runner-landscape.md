# CI runner and build-acceleration landscape: public-knowledge dossier

Date of research: 2026-11-19. Scope: input for the Velnor runner design. Not a benchmark. Facts only, no recommendations.

Conventions:
- [vendor] = the vendor's own claim or docs. [independent] = a third party or peer-reviewed source.
- "not public" = looked for and did not find it in the sources fetched in this pass. It does not prove the information does not exist.
- Numbers are quoted as the source gives them, with the comparison the source gives. Several vendor pages are marketing pages. Blog dates are noted where the page showed one.
- Some sources (WebFetch summaries) were read through a summarizing tool, so exact wording is paraphrased.
- Coverage is uneven: GitHub-hosted larger runners, CircleCI infrastructure, EngFlow and Cirrus have thin public detail in what was fetched. These are marked.

Item numbering used in each section:
1 job start path, 2 isolation, 3 placement and scheduling, 4 hardware, 5 caching, 6 source checkout, 7 test acceleration, 8 merge/queue, 9 published speed claims, 10 known limits.

---

## 1. Buildkite (agent, Elastic CI Stack, hosted agents, git mirrors, cache volumes)

Sources: https://buildkite.com/docs/pipelines/hosted-agents , https://buildkite.com/docs/pipelines/hosted-agents/cache-volumes , https://buildkite.com/docs/agent/v3/git-mirrors , https://buildkite.com/docs/agent/self-hosted/aws/elastic-ci-stack/architecture , https://pkg.go.dev/github.com/buildkite/buildkite-agent-scaler , https://buildkite.com/docs/test-engine/test-splitting

1. Job start path.
   - Self-hosted: the agent is a long-lived process that polls Buildkite for work (pull model). [vendor] (agent model; elastic-ci-stack architecture page above)
   - Elastic CI Stack (AWS): an Auto Scaling group plus launch template; a Lambda scaler polls the Buildkite API for queue depth (scheduled jobs) and busy-agent counts and adjusts the ASG. The Lambda scaler replaced AWS target-tracking and is described as faster because it uses Buildkite metrics rather than resource usage. [vendor] https://buildkite.com/docs/agent/self-hosted/aws/elastic-ci-stack/architecture
   - Scheduled actions set a minimum-capacity floor; the Lambda handles real-time scaling. [vendor] same page.
   - Scale-in: per the scaler README, the stack runs the scaler with scale-in disabled and relies on agent `--disconnect-after-idle-timeout` (agent v3.10.0) plus a systemd PostStop script. [vendor] https://pkg.go.dev/github.com/buildkite/buildkite-agent-scaler
   - The README says the stack depends on scaling up quickly from zero; an availability threshold can scale on agents failing to connect. [vendor] same.
   - Buildkite claims "up to 300%" faster initial scale-ups than native AutoScaling rules (reported via search summary, v5.0.0). [vendor] https://buildkite.com/resources/changelog/104-elastic-ci-stack-for-aws-v5-dot-0-dot-0
   - Hosted agents: a job on a hosted queue starts a new ephemeral agent; Linux agents start from a container base image with the agent layered on top; macOS from a VM image per queue Xcode/macOS version; cache volumes attach during startup, which "usually takes a few seconds"; jobs "dispatched within seconds". [vendor] https://buildkite.com/docs/pipelines/hosted-agents
   - Cold-start numbers: no figures beyond "a few seconds". Not public as a measured distribution.
2. Isolation.
   - Hosted: each queue/cluster has "hypervisor-level isolation"; agents destroyed after every job; per-cluster cache volumes, remote Docker builders, registry and secrets. Linux agents run "in a virtualized environment", macOS in a VM on dedicated Mac hardware, Windows in a Server 2022 VM. [vendor] https://buildkite.com/docs/pipelines/hosted-agents
   - Self-hosted: isolation is whatever the customer's host provides (instances in Elastic CI Stack). [vendor]
3. Placement and scheduling.
   - Queue/cluster model; `priority` key gives best-effort preference when capacity is scarce. [vendor] hosted-agents page.
   - Hosted platform runs "exclusively in US East Coast data centers". [vendor] same page.
   - Sticky placement for cache volumes: not public (volume attachment is described as best-effort).
4. Hardware. Hosted: "latest-generation Mac and AMD Zen-based hardware"; claims up to 3x faster than equivalent machines elsewhere (basis not stated on page). [vendor] hosted-agents page. Specific CPU model, clock and local NVMe size: not public in this page. Network: not public.
5. Caching.
   - Cache volumes: external disks attached to hosted instances, scoped to cluster and pipeline; first request creates; later jobs get a read/write fork (snapshot) of the last committed version; commit only on exit code 0, failed jobs discard; last-writer-wins. Linux: NVMe, standard fs such as ext4; macOS: sparse bundle images. Default/min 20 GB; one volume per step; retained up to 14 days from last use, not guaranteed. Attachment best-effort; access speed "non-deterministic". [vendor] https://buildkite.com/docs/pipelines/hosted-agents/cache-volumes
   - Container cache: internal OCI registry; remote Docker builders with layer cache (Enterprise plan only); no extra storage charge. [vendor] hosted-agents page.
   - Published restore/hit speeds: not public.
6. Source checkout.
   - Git mirrors: one bare mirror per repo per host created with `git clone --mirror`, checkouts use `git clone --reference`; file-based lock so one agent updates a mirror at a time (works across machines on network shares); optional LFS cache (agent v4.3.0+); fetch uses `git fetch origin <branch>` to avoid maintenance pruning objects a reference checkout needs. [vendor] https://buildkite.com/docs/agent/v3/git-mirrors
   - Elastic CI Stack: enable via `BuildkiteAgentEnableGitMirrors`. Hosted agents: repositories cached locally "at startup", transparent, no config. [vendor]
   - Shallow/partial clone behavior: not retrieved.
7. Test acceleration. Test Engine client `bktec` splits tests across partitions using historical timing data and rebalances over time; quarantines flaky tests (mute = run but ignore result; skip = do not run); example: 16 min of tests over 4 partitions, build about 10 min to about 4 min. Flake detection method not explained. [vendor] https://buildkite.com/docs/test-engine/test-splitting
8. Merge/queue. Buildkite has no first-party merge queue in the sources read. Not retrieved.
9. Speed claims: up to 3x faster hosted agents (compared with "equivalent machines from other CI/CD providers and cloud platforms", no methodology on page) [vendor]; 300% faster scale-up (vs native ASG rules) [vendor].
10. Limits (from docs). Cache volumes best-effort, 14-day retention not guaranteed, one per step; hosted US East only; signed-pipeline validation unsupported on hosted agents; Windows has no terminal/desktop access; remote Docker builders Enterprise-only. [vendor]

---

## 2. Blacksmith

Sources: https://arpitbhayani.me/videos/blacksmith-github-actions-internals-and-architecture , https://info.blacksmith.sh/task/blog/faster-disk-io-github-actions-runners , https://info.blacksmith.sh/task/blog/co-located-artifact-caching-github-actions , https://warpbuild.com/blog/blacksmith-warpbuild-comparison , https://www.blacksmith.sh/github-action-runners

1. Job start path. GitHub webhook to Blacksmith backend, signature validated, job requirements/matrix parsed, payload pushed onto an in-memory Redis queue; Go agent daemons on each host poll the queue, start a VM, run steps, stream logs and report to GitHub check-run APIs. [independent write-up of Blacksmith talk] https://arpitbhayani.me/videos/blacksmith-github-actions-internals-and-architecture . Boot: vendor-adjacent text says the microVM boots "in under 3 seconds" ([vendor] docs overview via search summary) and a minimal kernel "about 5 ms" (kernel boot only, per the same write-up). Warm pool/pre-boot: not public.
2. Isolation. Ephemeral Firecracker microVM per job on bare metal; KVM hardware isolation; cgroups enforce CPU, memory and disk I/O quotas. Environments destroyed after each use. [independent write-up] (above) and https://warpbuild.com/blog/blacksmith-warpbuild-comparison [competitor].
3. Placement and scheduling. Agents poll one shared Redis queue; placement/priority policy beyond that: not public. Sticky disk affinity details: not public in what was fetched. Customers of large size may see provisioning lead time (per competitor article). [competitor]
4. Hardware. AMD Ryzen 7950X, locally attached PCIe NVMe, boost up to 5.7 GHz, versus "AMD EPYC server CPUs with network-attached storage such as EBS" for standard runners. [vendor] https://info.blacksmith.sh/task/blog/faster-disk-io-github-actions-runners (read through the arpitbhayani write-up). Vendor page cites single-thread PassMark 4484 and roughly 2x single-thread vs GitHub, citing the RunsOn benchmark. [vendor] https://www.blacksmith.sh/github-action-runners . Native x86-64 and arm64 Linux; macOS on Apple Silicon. [competitor] warpbuild comparison.
5. Caching.
   - Co-located cache: caches kept on or next to the physical host, not an external object store; about 400 MB/s vs about 100 MB/s for standard runners (about 4x). [vendor]
   - 25 GB free per repository, LRU eviction after 7 days without access; dependency cache, Docker layer cache, sticky disks (disk data persisting between jobs). [competitor] warpbuild comparison.
   - Docker layers on sticky disks on NVMe; Ceph mentioned in one blog (backing store unclear). [vendor, via search summary] https://info.blacksmith.sh/task/blog/faster-disk-io-github-actions-runners
   - Sticky-disk attach/commit/concurrency semantics: not retrieved (docs page 404).
6. Source checkout. Git mirror: not mentioned in the write-up. Not public.
7. Test acceleration: not retrieved / not public. 
8. Merge/queue: none.
9. Claims. About 2x faster and half the cost; example 20 min vs 8 min; 4x cache throughput; 2x single-thread (all [vendor], baseline GitHub standard runners). Independent verification of sticky-disk design: not found.
10. Limits. Cache is per-repo with 7-day idle eviction; bare-metal capacity provisioning lead time for large customers [competitor].

---

## 3. Depot (runners, Depot Cache, remote container builds)

Sources: https://depot.dev/docs/github-actions/overview , https://depot.dev/blog/github-actions-cache , https://depot.dev/docs/container-builds/overview

1. Job start path. Runners launch on demand from `workflow_job` webhooks; each job gets an instance from a "pre-provisioned standby pool"; start-time figures: not public. [vendor] docs overview; cache post says "standby pool so builds start immediately". [vendor]
   - Container builds: CLI asks the API for a builder (org, project, arch), polls until ready; `cloud-agent` (open source) launches builder VMs; `machine-agent` registers and starts BuildKit with per-build mTLS certs; CLI then connects directly. [vendor] container-builds page.
2. Isolation. Runners: single-tenant ephemeral EC2 instances "never reused", terminated after the job. Builders: ephemeral EC2 per build; fork PRs on open-source repos get ephemeral builders with no cache access. [vendor]
3. Placement. Runners sit in the same private network as Depot's BuildKit builders. Builders: by default all builds for one architecture in a project share a builder VM; autoscaling launches a new builder "with a copy of the layer cache" when max builds per builder is reached. Region: not specified. [vendor]
4. Hardware. Ubuntu runners 2 vCPU/2 GB to 64 vCPU/256 GB; x86 on 4th-gen AMD EPYC Genoa; Arm on AWS Graviton4; part of memory reserved as disk; network 12.5 Gbps with cache up to 1000 MiB/s. Builder sizes 16 CPU/32 GB, 32/64, 64/128. [vendor]
5. Caching.
   - actions/cache: on-runner Go proxy implementing GitHub's Cache API; returns S3 blob URLs instead of Azure Blob URLs; keeps stock cache actions (no fork); tuned to 4 upload and 8 download streams (GitHub default 2); S3 storage. [vendor] https://depot.dev/blog/github-actions-cache
   - Claimed 1 GB/s save/restore vs GitHub default 100-150 MB/s; retention 30 days vs 7; size unlimited vs 10 GB at time of post. [vendor]
   - Docker layers: persistent local NVMe per builder (50 GB default, up to 500 GB), shared across team and CI, retention 7/14/30 days, 25-1000 GB limit, LRU; no tarball save/load because cache is on the builder. [vendor] container-builds page.
   - Included cache 25 GB (Developer) or 250 GB (Startup), then $0.20/GB/month; cache not isolated by branch. [vendor]
6. Source checkout: not public.
7. Test acceleration: not public.
8. Merge/queue: none.
9. Claims. Cache about 10x GitHub default (vendor measurement; GitHub runner network about 1 Gbps / 125 MB/s per the post); builds "up to 40x faster" than local or other CI (no detail on page). [vendor]
10. Limits. GitHub runners only for organization-owned repos (not personal accounts); large builder sizes need paid plans; Docker layer caching on runners not detailed. [vendor]

---

## 4. Namespace

Sources: https://namespace.so/docs/architecture/github-runners , https://namespace.so/docs/architecture/storage/cache-volumes , https://namespace.so/blog/zero-latency-caching (Dec 2023) , https://namespace.so/blog/so-you-want-to-build-your-own-datacenter

1. Job start path. Connect GitHub org, create runner profile, reference in `runs-on` label (`namespace-profile-<name>`). Cold-start numbers: not public. Job ordering is a runner control appended to profile name. [vendor]
2. Isolation. Page for GitHub runners does not describe isolation. A Namespace blog says they wanted fully isolated ephemeral jobs and since early 2024 run more than 95% of the platform on their own infrastructure (earlier on AWS bare metal). [vendor] datacenter blog (via search summary). Specific VM technology: not public in fetched pages.
3. Placement. Cache volumes: usually one generation per node; commit written to the committing node first, distributed in the background; a node without the newest generation forks the most recent one it holds; stale preferred over a miss so startup is not delayed; hit rate rises as more nodes get data. [vendor] cache-volumes page.
4. Hardware. Linux amd64 "high-performance AMD EPYC"; Linux arm64 AmpereOne (and Apple M4 Pro/M5 Max); Windows on EPYC; macOS M5 Max / M4 Pro; up to 512 GB RAM on enterprise. [vendor] github-runners page.
5. Caching.
   - Cache volumes: local NVMe; per-run private fork from the last committed generation; "last write wins"; commit only if main process exits 0; lineage reset if a generation outgrows configured size; best-effort not durable; min 20 GB. [vendor]
   - Docker image caching: reused images stay available; remote builders used for Docker builds. [vendor] blog.
   - `nscloud-checkout-action` uses cache volumes to speed monorepo checkouts; users reported saving up to a minute on large checkouts. [vendor]
   - Throughput/IOPS numbers: not public. Provisioning "close to zero impact on startup latency". [vendor]
6. Source checkout. Cache-volume-backed checkout action (above). [vendor]
7. Test acceleration: not public in fetched pages.
8. Merge/queue: none.
9. Claims. Cache volumes avoid upload/download; no quantified comparison on the pages read. Another Namespace Rust post exists (https://namespace.so/blog/reducing-github-actions-runtime-for-a-rust-project) not read.
10. Limits. Caches best-effort; misses more common while onboarding nodes; repository-level runners by default. [vendor]

---

## 5. WarpBuild

Sources: https://warpbuild.com/blog/snapshot-runners , https://www.warpbuild.com/docs/ci/features/snapshot-runners , https://warpbuild.com/blog/blacksmith-warpbuild-comparison

1. Job start path. Not detailed. Snapshot runners: docs say boot time can be slower than default runners at 45-60 s. [vendor] docs (via search summary). General startup figures: not public.
2. Isolation. Ephemeral VMs with encrypted runner storage, on AWS/GCP/Azure; BYOC option runs in customer cloud account. [vendor, as reported by WarpBuild's own comparison]
3. Placement. Cloud and BYOC; snapshots shared at org level. Affinity: not public.
4. Hardware. Native x86-64 and arm64 Linux, Windows, macOS on Apple M4 Pro/M5 Pro; CPU model and clock not stated in pages read. [vendor]
5. Caching.
   - Snapshot runners: `snapshot-save` action saves a full VM snapshot (excludes `/tmp`); `snapshot.key` label restores it; on network-attached disk; 15-day expiry; Ubuntu cloud runners only; may be slower than cache action for many-file directories like `node_modules`. [vendor] snapshot blog.
   - Reported timings (historical): Pocketbase 7-8 min on GitHub vs 3m19s (incl. 1m1s snapshot creation); Supabase Playwright 5m25s vs 4m56s; Flux 20m23s vs 14m28s. [vendor]
   - Cache: unlimited storage, 7-day retention from last access; remote Docker builders with persistent layer cache. [vendor, via competitor-style comparison page]
   - Pricing: $0.04 per job restore, $0.025/hour per snapshot (search summary). [vendor]
6-8: not public / none.
9. Claims. 2-10x faster consecutive runs; up to 10x with snapshots; no workload detail. [vendor]
10. Limits. Snapshot data exposure to other jobs in the org (docs warn); not on BYOC/Windows/macOS; creation overhead; benefit small for short jobs. [vendor]

---

## 6. Actuated

Sources: https://docs.actuated.com/faq/ , https://blog.alexellis.io/blazing-fast-ci-with-microvms/ , https://docs.actuated.dev

1. Job start path. GitHub App webhook; control plane schedules a microVM on one of the customer's servers; agent on the host runs the VM; GitHub runner in ephemeral mode (one build per VM). Boot "roughly 1-2 seconds". Bare-metal servers can take 5-10 min to provision. [vendor]
2. Isolation. Single-use Firecracker microVM destroyed after the build; iptables isolates runners from the network; usable with public repos. [vendor]
3. Placement. Customer-supplied hosts; VMs placed by RAM reserved per VM; excess jobs queue; packing across the fleet; plan limits server count; servers can be suspended on a schedule. [vendor]
4. Hardware. Customer's: bare metal or nested-virt VMs (Intel/AMD nested only; Arm nested not ready); x86-64 and arm64; guest Ubuntu 22.04; kernel 5.10.201 stable. Examples: AWS a1.metal, Ampere Altra 80-core, Mac mini M1 under Asahi, Raspberry Pi 4. [vendor]
5. Caching. Docs link "self-hosted cache" and "registry mirror" pages; contents not retrieved. Not public in this pass.
6-8: not public / none.
9. Claims. Parca arm64 build 33m5s on hosted QEMU vs 1m26s (about 22x); "33 minutes to under 2 minutes" on Arm. [vendor] (baseline: GitHub-hosted arm64 emulation).
10. Limits. No Windows or macOS; control plane data in UK; customer manages hosts and firewall; commercial product. [vendor]

---

## 7. RunsOn

Sources: https://runs-on.com/docs/ , https://github.com/runs-on/runs-on , https://runs-on.com/features

1. Job start path. Control plane deployed in customer's AWS account (CloudFormation/Terraform); private GitHub App; fresh ephemeral EC2 runner per job, scaling to zero. Warm pools: docs reportedly offer pools of stopped or hot instances with "<10s queuing" (search summary of docs; page fetched did not show it). Each pooled instance still runs one job. Boot latency distribution: not public. [vendor]
2. Isolation. Fresh EC2 instance per job inside customer's account/VPC. [vendor]
3. Placement. Multi-AZ, spot with on-demand fallback; Fleet mode uses Terraform-defined runner catalog. [vendor]
4. Hardware. CPUs 1 to 896 vCPU, latest-gen instances (EC2 families); GPU and Windows available. [vendor]
5. Caching. Options: built-in S3 cache ("Magic Cache", claimed 5x faster than GitHub's and effectively unlimited), Docker pull-through cache, EFS, EBS sticky disks. [vendor] repo README. Linux AMIs built from GitHub's runner images, refreshed every 15 days. [vendor]
6-8: not public / none.
9. Claims. 5-13x lower cost; 5x cache; 24.7 fresh runners per second at scale (throughput). [vendor] RunsOn also publishes a CPU benchmark referenced by Blacksmith. [vendor]
10. Limits. Requires AWS account; server and agent commercially licensed (MIT for CloudFormation assets), flat license from EUR 300/year. [vendor]

---

## 8. Ubicloud runners

Sources: https://www.ubicloud.com/blog/github-actions-transparent-cache , https://www.ubicloud.com/blog/ubicloud-premium-runners-40-faster-builds-10x-larger-cache , https://betterstack.com/community/comparisons/ubicloud-alternatives/

1. Job start path. Ephemeral VM per job; pool of pre-provisioned VMs (per a third-party comparison; [independent] betterstack). Numbers: not public in pages read.
2. Isolation. Ephemeral VM on bare-metal hosts (Hetzner, Leaseweb, AWS bare metal); control plane is a Ruby app SSH-ing into Linux hosts. [independent] betterstack / repo write-ups.
3. Placement: not public.
4. Hardware. "AMD Ryzen CPUs" for Premium runners; model and clock not named on the page. [vendor]
5. Caching. Local cache proxy on each runner VM; Ubicloud forked the GitHub runner to point at it; proxy forwards to a Ubicloud cache server; objects stored in Cloudflare R2, per-repo bucket, encrypted. Measured: GitHub cache 20-25 MB/s down / 20-25 up; transparent cache 80-100 down / 60-80 up (about 4x/3x). 10 GB GitHub default vs 30 GB standard vs 100 GB premium cache. [vendor] transparent-cache blog. Plans to evaluate own object storage.
6-8: not public / none.
9. Claims. Premium "around 2x faster" for CPU-heavy workflow (baseline not named; likely vs standard); 2 vCPU Premium $0.0016/min vs GitHub $0.008/min. [vendor]
10. Limits: cache placed outside Azure, hit rate not reported. [vendor]

---

## 9. Cirrus Runners (and Cirrus CI)

Sources: https://cirrus-ci.org/blog/2022/11/03/github-actions-on-m1-via-cirrus-runners , https://bitrise.io/blog/post/cirrus-ci-shutting-down-move-to-build-hub , https://warpbuild.com/blog/cirrus-ci-shutting-down

1. Start path. Fixed-price per concurrent runner (about $150 per runner per month, unlimited minutes) [competitor/independent]. Start numbers: not public.
2. Isolation. VM per job: Tart (macOS and Linux on Apple Silicon via Apple's Virtualization.Framework) and Vetu for Linux. [vendor via search summary]
3-4. Apple Silicon (M4 Pro), Linux arm64/x64 and GPU runners listed; CPU detail: not public.
5-8: not public.
10. Status: reported that Cirrus Labs joined OpenAI on 2026-04-07, Cirrus CI shut down 2026-06-01, Cirrus Runners closed to new customers; Tart relicensing reported inconsistently across sources. [independent] bitrise and warpbuild posts (both competitors).

---

## 10. BuildJet

Sources: https://runs-on.com/blog/buildjet-shutting-down-runson-alternative/ , https://warpbuild.com/blog/buildjet-shutting-down

- Shutdown announced 2026-02-06; jobs stop 2026-03-31 (per search summary of WarpBuild/RunsOn posts). Stated reason: "The gap we set out to fill has largely closed". [vendor, relayed by competitors]
- Architecture, hardware, cache design: not public in the sources fetched (no BuildJet docs retrieved). Claims of Ryzen hardware or 4x cache were not confirmed.

---

## 11. GitHub-hosted larger runners

Source: https://docs.github.com/en/actions/concepts/runners/larger-runners

- Managed VMs with runner app preinstalled; all larger runners autoscale; Linux and Windows get static IPs and Azure private networking; GPU runners offered. [vendor]
- Start path, hardware model, warm pool, cache backend (GitHub cache is Azure Blob per Depot's post), numeric limits: not retrieved here. Standard hosted runners 2 core / 7 GB per Ellis blog (older). [independent]
- GitHub cache measured at 100-150 MB/s [vendor competitor, Depot]; 20-25 MB/s [vendor competitor, Ubicloud]. Competitors measured different values; no GitHub-published figure retrieved.
- Merge queue: see section 21.

---

## 12. GitLab Runner (autoscaler)

Sources: https://docs.gitlab.com/runner/runner_autoscale/ , https://docs.gitlab.com/runner/executors/docker_autoscaler/

1. Start path. A runner manager polls GitLab for jobs and provisions cloud instances through "fleeting" (an abstraction for a group of autoscaled instances using cloud plugins for AWS, GCP, Azure). Taskscaler: not explained on those pages. [vendor]
2. Isolation. With `capacity_per_instance=1` and `max_use_count=1`, each job gets a disposable instance. Slot-based cgroups isolate concurrent jobs. Docker Autoscaling and Instance executors replace Docker Machine (legacy). [vendor]
3. Placement. `idle_count` (example 5) spare instances kept for at least `idle_time` (example 20 min); `concurrent` = max instances x capacity per instance; at least two managers recommended; managers must not run on spot. [vendor]
4. Hardware: customer's. 5. Caching: not covered on those pages. Suspend feature (experimental) resumes suspended environments' containers, volumes within `runtime_environment_ttl`. [vendor]
6-8: not retrieved.

---

## 13. CircleCI (Docker layer caching, test splitting)

Sources: https://circleci.com/docs/guides/optimize/docker-layer-caching/ , https://circleci.com/docs/guides/optimize/parallelism-faster-jobs/

- DLC: layers stored within the VM used for the job; works with `machine` executor and Remote Docker; each VM has its own cache; Remote Docker VMs exclusive to a job; DLC set-up and teardown steps (teardown uploads asynchronously); jobs in the same workflow generally cannot use an upstream job's uploaded cache; costs 200 credits per job run; named Buildx builders preserve volumes; purge via UI or `circleci dlc purge`. Known issue: QEMU/binfmt registration can corrupt cache. [vendor]
- Test splitting: strategies by name (default), filesize, or timings (`--split-by=timings`) from previous runs of JUnit XML (`file` and `time` attributes required); manual via `CIRCLE_NODE_TOTAL`/`CIRCLE_NODE_INDEX`; `--timings-type` name/classname/file; `circleci tests run` (Cloud) offers Rerun Failed Tests, and `circleci testsuite` adds dynamic splitting. [vendor]
- Isolation, hardware, placement, start times, merge queue: not retrieved.

---

## 14. BuildBuddy

Sources: https://www.buildbuddy.io/blog/fast-runners-at-scale , https://www.buildbuddy.io/docs/rbe-microvms , https://www.buildbuddy.io/docs/rbe-setup , https://www.buildbuddy.io/docs/remote-runner-introduction

1. Start path. Workflows and Remote Bazel run in persistent execution environments: Firecracker microVM snapshots on Linux, persistent runners on macOS; branch runs look for the best snapshot to resume (latest matching branch first). Linux microVM start in "hundreds of milliseconds". [vendor]
2. Isolation. Firecracker microVMs (also for RBE actions, enabling Docker-in-action); default container isolation otherwise. A fresh microVM per action by default. [vendor]
3. Placement. Requests with the same machine configuration reuse the same snapshotted runner; recycled runner is an optimization not a guarantee (lookup can fail if evicted or machine loaded); targets share a VM only if platform properties identical; `runner-recycling-key`. `--remote_execution_priority` -1000..1000 best effort. [vendor]
4. Hardware: not public in pages read; remote runner disk limit 100 GB.
5. Caching. Snapshots chunked and content-addressed; memory restored lazily through userfaultfd handler; disks are FUSE-backed copy-on-write with dirty chunks uploaded under new hashes; chunks cached on host and shared across VMs/zones; each clone has its own network namespace with NAT. [vendor] fast-runners post. Remote cache for Bazel via REAPI.
6. Checkout: not retrieved.
7. Test acceleration: Bazel-native (parallel remote actions, `--jobs`); no separate sharding data fetched.
9. Claims. Cached Bazel build about 6 min with about 3.5 min worker setup on GitHub Actions; snapshotted runners median CI run about 30 s; small repos tests in 6 s. [vendor] (baseline: GitHub Actions cold setup).
10. Limits. Snapshot of whole memory and disk makes large runners slower to restore (100 GB disk cap); Docker image cache starts empty per microVM unless recycled. [vendor]

---

## 15. EngFlow

Sources: https://docs.engflow.com/re/index.html , https://docs.engflow.com/glossary.html , https://github.com/EngFlow/free

- Cluster = schedulers + workers. Schedulers take client traffic and host the Action Cache (shared among schedulers). Workers act as CAS nodes and move blobs peer-to-peer (distributed CAS); no separate CAS backend. Implements REAPI v2.0.0. On AWS workers run on EC2 with attached EBS and Auto Scaling; managed or self-managed; a free single-node build exists. [vendor] (via search summary; pages not opened)
- Isolation, start times, hardware, published speed numbers: not retrieved.

---

## 16. Bazel/Buck2 remote execution model (REAPI)

Source: https://github.com/bazelbuild/remote-apis/blob/main/build/bazel/remote/execution/v2/remote_execution.proto [vendor/spec]

- Services: Execution (`Execute` streams long-running operations; `WaitExecution`), ActionCache (`GetActionResult`/`UpdateActionResult` keyed by Action digest), ContentAddressableStorage (`FindMissingBlobs`, `BatchUpdateBlobs`, `BatchReadBlobs`, `GetTree`; ByteStream for large blobs; chunk-mapping RPCs, with older split/splice deprecated), Capabilities.
- Action = command digest + input root digest (Merkle tree of Directory messages) + timeout + `do_not_cache` + optional `salt` + platform properties (sorted name/value; server defines meaning). Timeout is part of the digest. `salt` isolates cache namespaces.
- Clients call FindMissingBlobs before uploading; servers should extend lifetimes of present blobs.
- Buck2 as a REAPI client: not retrieved in this pass.

---

## 17. Google TAP

Sources: https://abseil.io/resources/swe-book/html/ch23.html [vendor/book] , https://mpapad.github.io/publications/pdfs/ICSE-SEIP2019.pdf and https://hackthology.com/speculative-testing-at-google-with-transition-prediction.html [vendor papers]

- Scale: more than 50,000 unique changes and more than four billion test cases daily. Presubmit uses a team-configured fast subset; passing presubmit implies 95%+ likelihood of passing the rest; optimistic submission with asynchronous full runs; average wait to submit about 11 minutes. [vendor] SWE book.
- Test selection: Blaze/Forge global dependency graph finds tests downstream of the change; runs Bazel test targets, not individual methods. [vendor]
- Batching: evaluates more than one change per second; batches related changes; culprit finding splits a failing batch and reruns individually; auto rollback when confidence is high; Build Cop role. Flaky tests complicate culprit finding; teams remove flaky tests from presubmit. [vendor]
- Milestones: affected targets can be deferred to a milestone. Of 5.5 M affected tests in one period only 63 K ever failed. A 2025 ICST paper's Transition Prediction model cut median detection time from 107 to 37 minutes. [vendor] hackthology/ICST.

---

## 18. Meta Sandcastle

Sources: https://engineering.fb.com/2017/05/24/developer-tools/jupiter-a-high-performance-job-matching-service/ , https://arxiv.org/pdf/1810.05286.pdf (Predictive Test Selection, ICSE-SEIP 2019)

- Sandcastle is Meta's CI system and a customer of Jupiter, a job-matching service (C++, Thrift): workers advertise traits (RAM, data center, load, preloaded repositories), jobs list required capabilities; atomic one-worker assignment; leases; priorities and FIFO; prerequisites; sharded with no leader; "hundreds of thousands of requests per second per shard", acquisition latency "tens of microseconds". [vendor]
- Predictive Test Selection: learns from historical outcomes; halved the infrastructure cost of testing changes while reporting over 95% of individual test failures and over 99.9% of faulty changes. [vendor, peer-reviewed]
- Sandcastle internals beyond that (isolation, caching, checkout): not public in sources found. The 2017 "rapid release" post does not mention Sandcastle. [vendor]

---

## 19. Uber SubmitQueue

Sources: https://www.uber.com/blog/research/keeping-master-green-at-scale , https://blog.acolyer.org/2019/04/18/keeping-master-green-at-scale/ , https://www.uber.com/blog/slashing-ci-costs-at-uber (not fetched successfully) , https://arxiv.org/pdf/2501.03440v2

- Guarantees an always-green mainline for a monorepo; builds a speculation graph with a probabilistic model, selecting changes likely to succeed and skipping builds already executed. A mobile mainline was green only 52% of the time before. [vendor/peer-reviewed] (EuroSys '19, via secondary summary)
- Follow-up: Uber reports CI resource usage down 53% and wait times down 37% (blog, via search summary). [vendor]
- A critical review (T.H. Tse, HKU) questions the "guarantee" language. [independent] https://i.cs.hku.hk/~tse/Papers/2020s/vacmcr20TR.pdf
- Full conflict-analyzer and throughput numbers: not retrieved (paper PDF exceeded fetch limit).

---

## 20. Merge-queue tools

### 20.1 GitHub merge queue
Source: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue [vendor]
- Merge group = latest base + the PR + PRs ahead; temp branches `gh-readonly-queue/{base}`; `merge_group` event; build concurrency 1-100; merge limits min/max (1-100) with wait time; "only merge non-failing" toggle; on failure group is rebuilt without the failed PR; jumping the queue rebuilds in-progress entries. Bisection: not documented (removal then rebuild). Timeout setting for status checks.

### 20.2 Mergify
Sources: https://docs.mergify.com/merge-queue/ , https://docs.mergify.com/merge-queue/batches/
- Speculative (parallel) checks, batches (`batch_size` 1-128 or dynamic `{min,max}`), two-step CI, scopes. Bisection: failed batch is split to find the culprit; batches on top are cancelled and retested; `batch_max_failure_resolution_attempts`. [vendor]

### 20.3 Graphite
Source: https://graphite.com/docs/graphite-merge-queue [vendor]
- Stack-aware; validates a whole stack in parallel; rebases onto trunk during merge; optional fast-forward for rebase/squash that skips re-running CI. Batching/speculative/bisection: not on that page.

### 20.4 Trunk
Sources: https://docs.trunk.io/merge-queue , https://docs.trunk.io/merge-queue/optimizations/batching [vendor]
- Predictive testing (each PR tested against main plus those ahead), parallel queues keyed by impacted targets (a PR impacting all targets serializes behind), batching (default 4, max wait 5 min), bisection by halving with separate concurrency and optional "skip redundant bisection tests", optimistic merging, pending-failure depth, anti-flake protection.

### 20.5 Aviator
Sources: https://docs.aviator.co/mergequeue/concepts/parallel-mode , https://docs.aviator.co/mergequeue/concepts/batching [vendor]
- Parallel mode builds draft PRs combining queued PRs (`max_parallel_builds`, Pro plan); batching with `batch_size` and `batch_max_wait_minutes`; bisection by requeueing smaller batches (1-5 into 1-3 and 4-5); fast forwarding; stacked PRs; `use_affected_targets`.

---

## Cross-system table

| System | Isolation | Warm start | Cache mechanism | Placement | Hardware | Test acceleration | Merge queue |
|---|---|---|---|---|---|---|---|
| Buildkite hosted | hypervisor-level VM, ephemeral agent | "few seconds", cache attach at start | NVMe cache volume fork/commit; registry; remote Docker builders | cluster-scoped; US East | AMD Zen, Macs | bktec timing split, quarantine | none retrieved |
| Buildkite Elastic | customer EC2 | Lambda scaler, scale from zero | git mirrors, customer-defined | queue/ASG | customer | bktec | none |
| Blacksmith | Firecracker microVM, ephemeral | Redis queue to Go agent; "<3 s" boot (vendor) | co-located cache, sticky disks (NVMe) | agents poll shared queue | Ryzen 7950X, local NVMe, 5.7 GHz | not public | none |
| Depot | single-tenant EC2 | standby pool | S3 via on-runner proxy; NVMe BuildKit layer cache | runner and builder in same network | EPYC Genoa, Graviton4 | not public | none |
| Namespace | not stated on page | not public | NVMe cache volume forks, generations | per-node generation, background distribution | EPYC, AmpereOne, Apple | not public | none |
| WarpBuild | ephemeral VM | snapshot runners 45-60 s boot | network-disk VM snapshot, cache action | org-level snapshots | x86/arm/Apple | not public | none |
| Actuated | Firecracker microVM, single-use | 1-2 s boot | self-hosted cache/registry mirror (not read) | by RAM on customer hosts | customer bare metal | none | none |
| RunsOn | EC2 per job | warm pools (hot/stopped) | S3, EFS, EBS sticky disk | multi-AZ, spot | EC2 families | none | none |
| Ubicloud | ephemeral VM on bare metal | pre-provisioned pool (3rd party) | R2 via local proxy | not public | AMD Ryzen | none | none |
| Cirrus Runners | Tart/Vetu VM | not public | not public | not public | Apple Silicon, Linux | n/a | none |
| BuildJet | not public | not public | not public | not public | not public | n/a | none |
| GitHub larger | managed VM | not retrieved | GitHub cache (Azure Blob) | autoscale | larger SKUs, GPU | n/a | merge queue |
| GitLab Runner | disposable instance | `idle_count` pool | executor-defined | fleeting plugins | customer | n/a | not retrieved |
| CircleCI | VM/container | not retrieved | DLC per-VM volumes | per job | not retrieved | timing split, rerun failed | none retrieved |
| BuildBuddy | Firecracker microVM | VM snapshot resume, hundreds of ms | chunked CoW snapshots + REAPI CAS/AC | same-config reuse | not public | Bazel remote exec | none |
| EngFlow | not retrieved | not retrieved | distributed CAS on workers | scheduler/worker | EC2+EBS | Bazel remote exec | none |
| TAP | internal | n/a | internal | internal | internal | dep-graph selection, batching | submit + culprit finding |
| Sandcastle | not public | n/a | not public | Jupiter capability matching | not public | predictive selection | not public |
| SubmitQueue | n/a | n/a | n/a | n/a | n/a | build skipping | speculation graph |
| GitHub MQ | n/a | n/a | n/a | n/a | n/a | n/a | merge groups, concurrency 1-100 |
| Mergify / Trunk / Aviator / Graphite | n/a | n/a | n/a | n/a | n/a | affected targets (Trunk, Aviator) | batching, speculative, bisection (Graphite less documented) |

---

## Recurring techniques

- Ephemeral VM or microVM per job, destroyed afterward: Blacksmith, Actuated, BuildBuddy, Buildkite hosted, Depot, RunsOn, Ubicloud, Cirrus (Tart/Vetu), GitLab (max_use_count=1), CircleCI Remote Docker.
- Firecracker specifically: Blacksmith, Actuated, BuildBuddy (and Fireactions, open source).
- Webhook-triggered on-demand provisioning with a pre-provisioned/standby pool: Depot, RunsOn, Ubicloud (third-party report), GitLab `idle_count`, Elastic CI scheduled capacity floor.
- Queue-polling agent model: Buildkite agent, Blacksmith Go agents on Redis queue, Jupiter workers (Sandcastle).
- Capability/trait matching between jobs and workers: Jupiter (Meta), REAPI platform properties, BuildBuddy exec properties.
- Local NVMe volume forked per job with commit-on-success (generations/lineage, last-writer-wins): Namespace cache volumes, Buildkite hosted cache volumes (same wording).
- Persistent disk reattached across jobs ("sticky disk"): Blacksmith, RunsOn (EBS), WarpBuild snapshot (network disk).
- Whole-VM snapshot restore: WarpBuild snapshot runners, BuildBuddy Firecracker snapshots (chunked, lazy UFFD, FUSE CoW).
- Cache backend redirection (proxy on runner returning alternate blob URLs, stock cache actions unchanged): Depot (S3), Ubicloud (R2), RunsOn (S3), Blacksmith (co-located).
- Cache co-located with compute to avoid WAN transfer: Blacksmith, Depot (same private network as builders), Ubicloud (moved off Azure path), Namespace (local NVMe).
- Higher download stream parallelism for cache: Depot (4 up / 8 down), Ubicloud (more upload concurrency).
- Persistent remote BuildKit builder with local layer cache: Depot, Buildkite remote Docker builders, WarpBuild remote builders, Namespace remote builders, CircleCI DLC (per-VM).
- Layer-cache scoping per project/org with LRU and retention windows: Depot, Blacksmith (7 days idle), WarpBuild (7 days), Buildkite (14 days).
- Git bare mirror with reference clone and file lock: Buildkite agent; Namespace uses cache-volume-backed checkout.
- Best-effort cache semantics (stale preferred over miss, not durable): Namespace, Buildkite hosted.
- High-clock consumer CPUs with local NVMe: Blacksmith (Ryzen 7950X), Ubicloud Premium (Ryzen); EPYC server parts: Depot, Namespace, Buildkite (Zen).
- Customer-owned hardware or cloud account with vendor control plane: Actuated, RunsOn, WarpBuild BYOC, Buildkite self-hosted, GitLab.
- Spot with on-demand fallback: RunsOn.
- Timing-based test splitting from historical JUnit-style data: CircleCI, Buildkite bktec.
- Flaky quarantine/mute: Buildkite Test Engine, Trunk anti-flake, TAP (removal from presubmit).
- Dependency-graph test selection / impacted targets: TAP, Trunk (parallel queues), Aviator (`use_affected_targets`), Mergify scopes.
- ML test selection from historical outcomes: Meta Predictive Test Selection, Google Transition Prediction.
- Content-addressed action cache plus CAS with FindMissingBlobs: REAPI, BuildBuddy, EngFlow.
- Speculative queue entries built on those ahead: GitHub merge queue, Mergify, Trunk, Aviator, Uber SubmitQueue (probabilistic), Graphite (stack-aware).
- Batching of PRs per CI run: Mergify, Trunk, Aviator, GitHub (merge group sizing), TAP.
- Bisection of failed batch by halving: Trunk, Mergify, Aviator, TAP (split and rerun).
- Optimistic submission/merge with async full test: TAP, Trunk (optimistic merging).
- Fast-forward merge of already-tested commit: Aviator, Graphite.
- Auto rollback on high-confidence culprit: TAP.
