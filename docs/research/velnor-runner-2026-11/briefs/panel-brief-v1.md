# Velnor runner research — panel brief v1 (2026-11-18)

You are one independent member of a research panel. Several other models receive this SAME brief and answer separately; you will not see their answers. Do your own research and judgement. Nobody hands you answers: the evidence pack below is facts only, with no recommendations. Where your own measurement or code reading disagrees with it, say so and trust the code.

The single question: **how should Velnor, our own GitHub-Actions-compatible runner, be designed from scratch so that CI/CD stops being the bottleneck for velnor-new's development?**

## The goal

We build software with AI agents. The slowest part of delivering work is no longer writing the code; it's waiting for CI/CD. Every pull request waits a long time before it's verified and merged.

The goal: the shortest possible time from push to merge, with the SAME level of confidence as today. We may cut work that makes no sense, such as testing what a change can't affect. We must never cut verification the change actually needs.

GitHub stays our platform: the code, PRs, the merge queue, and the GitHub Actions workflow format and API. Velnor must be compatible with GitHub Actions from the outside (it takes GitHub's jobs, reads the same workflow YAML and reports back), but inside it's free to be designed from scratch for speed, without inheriting the old architecture's limits.

## The problem (evidence; re-measure everything, trust nothing)

- **velnor-new CI today** (tailrocks/velnor-new#574 issuecomments 6969131218 and 6969256836; #1305 comments 6980386757, 6980497787 and 6980735625; ~/.cache/velnor-p3-ci-baseline.md):
  - a PR run takes ~92 min median, ~67 min of it queue wait;
  - ~39 jobs and ~73 runner-minutes per run;
  - CI runs twice per merge;
  - the biggest costs are the velnor-runner-host tests (~18 runner-min), Check / maintenance-helpers (~10 runner-min, with no cache), and waits of 12–34 min for GitHub-hosted runners;
  - caches mostly miss: the state seal never succeeded, because the merge queue advances main; there's no toolchain cache; the mbx remote cache isn't set.
- **2026-11-18 measurement:** 86 queued jobs waited on GitHub-hosted runners and 0 on our cluster. The cluster sat at 18–29% CPU.
- **Our hardware:** fully under our control. A Hetzner k3s cluster: 5 workers (EPYC, 96 threads and 128 GB each, TB-scale NVMe) plus a control plane, in two sites (DE and FI) with a WireGuard pod network. Today it runs GitHub's official Actions Runner Controller (ARC) scale set: one ephemeral pod per job with a docker:dind sidecar, so nothing survives between jobs. Details are in ~/Projects/tailrocks/velnor-runners/README.md and state/RESULT.md. NEVER print state/kubeconfig or any secret; read-only kubectl only. More servers can be bought later; design for today's hardware and say what more would buy.
- **The stack velnor-new uses,** always on the latest versions: Rust (pinned toolchain, see docs/hard-decisions.md), cargo-nextest, cargo-deny, cargo-machete, mise, mbx / Mr. Boxington (https://mr-boxington.jdx.dev), blaze (https://github.com/indexable-inc/blaze), actions/checkout, actions/cache, jdx/mise-action and jdx/mr-boxington-action. Workflows come from velnor-new's own generator (velnor-gen) and helper (velnor-ci). Read `.github/workflows/*`, `.velnor/*`, `generator/` and `ci-helper/` to see exactly what's used.

## Material to study (code is the source of truth; docs may be stale)

- **tailrocks/velnor-new**, the current project. Today its runner (`crates/velnor-runner`, velnor-host) runs GitHub's OFFICIAL runner inside Docker containers it manages, with a journal, ownership proofs, crash recovery and a local mode. See `docs/proposed/kubernetes-runtime.md` (PR #1324) and `docs/deferred/self-hosted-runner.md`, whose "V2-native" stage is a planned Rust runtime.
- **github.com/tailrocks/velnor-legacy**, archived: an earlier Rust runner that executed jobs itself, as a REFERENCE CONCEPT. It has an expression engine, step and composite execution, native Rust adapters for checkout, cache, rust-cache and sccache, a scale-set client, Docker and Firecracker backends, and mbx integration. Study its CODE (build it and run its tests); find out from its git history why it was archived.
- **GitHub's official runner** (github.com/actions/runner, C#) and **ARC** (github.com/actions/actions-runner-controller): to learn the current protocol, and to identify the architectural limits we must NOT copy.

Code from either Velnor repository may be reused in velnor-new wherever it's good.

## The owner's hypotheses (test them; don't assume them)

1. Building everything in Rust, including the runner itself and native Rust versions of the actions we use instead of JavaScript actions, will speed up CI dramatically, because native code beats interpreted code.
2. A from-scratch, Rust-only, GitHub-compatible runner beats reusing or wrapping GitHub's official runner and ARC.
3. Kubernetes is the right base: it schedules and scales pods, so it lets us use all of our hardware in parallel.
4. Local caches on hardware we control beat remote caches. A runner that ships a proper caching system built in, where the cache layout knows which server a job runs on, is a big win.
5. Workflows can be written for maximum speed: skip what a change doesn't touch, and run only and exactly the verification it needs, as fast as possible.

The owner accepts these costs in exchange for speed: instability, more work, breakage when GitHub changes its protocol (we fix it quickly with AI agents), and dropping legacy compatibility (newest APIs only). Being afraid of rewriting in Rust is NOT a reason to reject an idea. Each model must still say plainly where a hypothesis is wrong or unproven, with evidence and numbers. The question is which design gets the minimum time to the same confidence, not which design confirms the hypotheses.

## Questions every panel member answers independently

1. Where does the time go today, measured from real runs? Which parts are queueing, setup, downloads, compilation, tests, cache transfer, and duplicate work?
2. Which of the hypotheses above hold, partly hold or fail? Give evidence and expected numbers for each.
3. The runner architecture you recommend: what stays compatible with GitHub (protocol and workflow format), and what's redesigned inside. What to take from velnor-new, velnor-legacy, the official runner and ARC, and what NOT to copy.
4. The cache system: what's cached (toolchains, tools, crate sources, build artifacts and mbx, test results), where it lives (node-local, shared tier, remote), how jobs are placed on warm nodes, how correctness and trust are kept (PRs vs main, forks), and the expected hit rates.
5. Workflow design for speed: change-aware selection (skip what wasn't touched without losing confidence), test sharding and parallelism across the cluster, no double runs, and the minimum honest time to verdict for a typical velnor-new PR.
6. Kubernetes use: pod lifecycle, pre-warming, node affinity, isolation of untrusted PRs, and scaling across all workers and both sites.
7. The role of mbx and blaze, and whether blaze fits velnor-new's pinned toolchain.
8a. **Servers and cluster tuning** (below the software): what to change on the hosts and in Kubernetes to make Rust CI fast. Measure the current state read-only first: `kubectl describe nodes` and node-level facts the owner's notes and RESULT.md give. No changes from this session. Cover at least:
   - **The OS** (Debian 13 today): CPU governor and frequency scaling, SMT, NUMA layout on the EPYC 9454 (pinning, node-local memory for builds), transparent hugepages, swap and memory overcommit, file-descriptor and inotify limits, `vm.dirty_*` writeback, kernel version and scheduler settings.
   - **Storage:** NVMe layout and RAID (the workers have 2–5 disks); filesystem choice and mount options (e.g. noatime; XFS vs ext4 vs btrfs or ZFS for many small files and reflinks); a dedicated fast volume for caches, target dirs and containerd; tmpfs for build scratch; containerd snapshotter settings; image pre-pull and a local registry or mirror.
   - **Network:** the cross-site WireGuard pod network (DE to FI latency); keeping cache traffic node-local or same-site; MTU; local mirrors for crates.io, toolchains and container images.
   - **Kubernetes:** kubelet reservations and eviction thresholds, CPU manager and topology manager policies (static or exclusive CPUs for build pods), pod resource requests vs limits for compile-heavy jobs, priority classes, node labels and taints for CI pools, scheduler tuning for warm-node placement, image GC thresholds, and whether k3s defaults (e.g. flannel, local-path) suit CI.
   - **Build-specific:** linker choice (mold or lld), `CARGO_BUILD_JOBS` vs cores per pod, sccache, mbx and blaze placement, rustc codegen settings for CI profiles, and test parallelism vs CPU and memory per pod.

   For each change, give the expected gain with evidence, the risk, and how to roll it back. Rank them by gain per effort.
8. A phased plan, where each phase ends in a measured improvement on velnor-new with a numeric target (PR wall time, runner-minutes, cache hit %). Include the top risks and how each is detected and contained.


## Context already decided by the owner (you may still comment)

- The crate layout is moving to one root workspace with `crates/velnor-*`, like NVIDIA/OpenShell; in progress.
- Releases are release-plz tags; assets are `<bin>-<target-triple>.tar.gz` plus SHA256SUMS and an attestation per archive.
- A new org-level GitHub App for Velnor's runner on the cluster; privileged dind on dedicated tainted nodes for now.
- velnor-actions v0.1.81 is being released to move today's GitHub-hosted jobs onto the cluster (a short-term fix, independent of this research).

## Where things are on this machine (read-only unless stated)

- Evidence pack (facts only, re-measured today): `~/.cache/ci-ambition/panel/evidence.md`. Start here.
- velnor-new (current project, main branch): `~/Projects/tailrocks/velnor-new`. **Read only**: other agents use this checkout; never edit, never switch branches, never run git commands that write. To build or run tests, copy it first: `git -C ~/Projects/tailrocks/velnor-new archive HEAD | tar -x -C <your scratch dir>/vn` and use your own `CARGO_TARGET_DIR` inside your scratch dir.
- velnor-legacy (archived Rust runner, full history): `~/.cache/ci-ambition/refs/velnor-legacy` (read only; build/test with `CARGO_TARGET_DIR` in your scratch dir).
- GitHub's official runner: `~/.cache/ci-ambition/refs/actions-runner`; ARC: `~/.cache/ci-ambition/refs/arc` (read only).
- The cluster's owner docs: `~/Projects/tailrocks/velnor-runners/README.md` and `state/RESULT.md` (never open `state/kubeconfig` or anything holding credentials).
- Optional live cluster reads: `KUBECONFIG=$HOME/Projects/tailrocks/velnor-runners/state/kubeconfig kubectl get|describe|top ...` — READ-ONLY verbs only.
- GitHub reads: `gh` is authenticated; read-only calls only (`gh api` GET, `gh run view`, `gh pr view`).
- Web: allowed for public docs (mbx https://mr-boxington.jdx.dev, blaze https://github.com/indexable-inc/blaze, GitHub docs, crates).
- Your scratch dir: the current working directory you were started in. Write ONLY there.

## Rules (binding; auto-approve mode removes prompts, not these rules)

- Research and plan only. No implementation PRs, no pushes, no merges, no comments, no issues, no GitHub setting changes.
- Cluster: read-only kubectl verbs only (get, describe, top, logs). Never apply/patch/delete/scale/cordon/drain/exec. Never ssh to nodes.
- Never print, copy or quote kubeconfigs, Secret data, tokens or keys.
- Never spend money. Kill processes by PID only; delete exact paths only, and only inside your scratch dir. Never touch `~/.local/share/mise`. Never run `rex`. Never use Grok.
- Don't change `docs/goals.md` or `docs/hard-decisions.md`; if your design contradicts them (notably `docs/goals.md` line 16, "jobs are always executed by GitHub's official runner"), say so and flag it as an owner question.

## What to deliver

Write your complete answer as Markdown to `proposal.md` in your scratch dir (the current working directory). Also print it as your final message.

- Answer ALL questions 1–8 (including 8a) above, in that order, with one `##` heading per question, plus a `## Summary` at the top (≤ 15 lines) and a `## Owner questions` section at the end.
- Hypotheses (question 2): give each a verdict — holds / partly holds / fails / unproven — with evidence and expected numbers.
- Every number: say whether you measured it (how), took it from the evidence pack, or estimated it (show the arithmetic). Cite files as `path:line`, runs by id/URL.
- Be concrete and ambitious, but honest: the target is minimum push-to-merge time at the SAME confidence, not confirming the hypotheses.
- Length: as long as it needs to be, but aim for something a senior engineer can read in 30–45 minutes (roughly 400–900 lines).
- Don't write a header naming yourself; the manager adds it.
