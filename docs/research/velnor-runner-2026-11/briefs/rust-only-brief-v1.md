# Velnor runner research: Rust-only redesign brief v1 (2026-11-19)

You are one of eight independent panel members. You answered the original brief (`~/.cache/ci-ambition/panel/brief-v1.md`), critiqued other proposals, and critiqued a draft design. The owner has since made a **fundamental decision**. This round asks you to redesign the system from that decision, for one goal: **CI/CD must stop being a bottleneck at all, optimised to the maximum of what is possible, at the SAME confidence as today.** Analyse every angle from that requirement.

## The owner's binding decisions (not up for debate; design within them)

1. **Rust only.** Velnor executes GitHub Actions jobs itself with a native Rust executor. GitHub's official C# runner (Runner.Listener and Runner.Worker) is never used in production, not even as a fallback.
2. **The official runner is only a conformance oracle.** The latest official runner release runs the same jobs in a lab, and a differential harness proves that Velnor behaves the same.
3. **Latest GitHub APIs only, nothing legacy.**
   - In: scale-set sessions, broker long-poll, JIT config, run-service acquire/renew/complete, the Results service for logs/timeline/summaries, cache service v2, artifacts v4, OIDC.
   - Out: the legacy session with AES message encryption, legacy pipelines and distributed task, the legacy expression evaluator, cache v1, artifacts v3.
4. **No Node and no JavaScript actions.** Velnor implements natively in Rust the closed set of actions velnor-new's generated workflows use (actions/checkout, actions/cache, jdx/mise-action, jdx/mr-boxington-action, actions/attest, and so on). Unknown actions are refused before the job starts.
5. **velnor-legacy stays archived.** Code may be copied from it into velnor-new, reviewed and adapted to the latest APIs, and owned there. The repository is never reactivated or depended on.
6. **Experimental dependencies are fine** (for example mbx-cache and blaze). Judge them only on evidence.
7. GitHub stays the platform: code, PRs, the merge queue, the workflow YAML format and its API.
8. **No pull requests from forks.** The code is public, but only the project's own people and agents open PRs. Drop all fork handling. Design isolation for trusted authors whose code may be buggy, not for hostile outsiders. PR results must still never write into trusted `main` caches or receipts without verification: that is a correctness rule, not a security rule.
9. **macOS release builds are paused.** Nobody uses them now; the owner builds and installs on macOS locally with cargo. So no velnor-new job needs GitHub-hosted machines: every job runs on Velnor (Linux, the owner's cluster). Disabling the macOS release workflow is a phase-0 work item. macOS returns later, so keep the design open to adding it, but don't spend phase time on it.
10. **Build once, verify once, reuse everywhere (maximum speed AND maximum reuse).** Each unit of work (dependency fetch, compile, test, package, sign or attest, publish) runs once per unique input. Every later stage reuses its result by content address: PR run, merge-queue run, push to `main`, release, nightly or audit runs, and cache warmers. A release PROMOTES the exact artifacts that `main` built and verified; it never rebuilds them. Doing the same operation twice is allowed only where it is measurably faster (speculation, racing two paths, a local copy beating a fetch), with the numbers shown. Duplicate work that also makes things slower is a red flag the design must remove. Resolve the profile tension explicitly: release binaries use an optimised profile while CI tests use debug. Choose, with numbers, between testing the release build, building release artifacts once at merge time, or something better, and never rebuild silently. The same principle applies to PRs, at three levels: (a) within one PR across its pushes: re-verify only the packages a push changed and their dependents; a rebase-only push with the same tree gets its verdict instantly; a re-run with no code change, such as an `edited` trigger, costs zero; a new push never waits for the cancelled run of the previous push; (b) across different PRs: unchanged parts of the workspace are verified once and shared between concurrent PRs; stacked PRs reuse their base PR's results; two PRs with identical changes are detected by tree content; (c) before the PR: an agent's pre-push verification, if run in the same sealed Velnor sandbox, is reused instead of repeated. PR-produced results may be reused for identical inputs, but never become `main`'s trusted caches or receipts without verification.
11. **Correctness first: fast and incorrect is wrong.** Correctness is the hard constraint; speed is optimised only inside it. Any element that could turn a real failure green is rejected, however fast. A result is reused only when its key provably covers EVERYTHING that can change it: sources, toolchain, dependency versions, env, flags, features, the binaries a test executes (for example `CARGO_BIN_EXE_*`), data files, network, time and randomness. When in doubt, re-execute. For every reuse or skip mechanism you propose, give its soundness case: (1) why the key is complete; (2) the failure mode if it is not; (3) how that failure is detected (for example sampled re-execution of reused results, or a full cold audit of every landed commit); (4) the automatic fallback (reuse off until the cause is fixed). False reds (flakes) have a budget; false greens have ZERO tolerance.
12. **CI runs on demand, not on every push.** Agents push many intermediate commits, and evidence shows 45% of PR runs were cancelled, so per-push full runs waste most of their work. A push starts no heavy verification. Full PR verification starts only through the owner's label flow below; the merge queue always verifies what it merges. Merge rules do not change: merging still requires a passing verdict on the exact merged tree, so on-demand never lowers confidence. Specify the trigger through GitHub's own surface (the `labeled` and `unlabeled` events, `synchronize`, `merge_group`) **The owner's exact trigger flow (binding):** (1) a draft PR receiving pushes runs nothing; (2) marking it ready for review still runs nothing; (3) adding the PR label (for example `velnor:verify`) starts the PR's CI on the current head; (4) while the label is present, every new push starts a new pipeline and the previous one is cancelled immediately, with no stall; (5) removing the label stops all further PR runs. Nothing else starts PR CI: no auto-labelling from ready-for-review or auto-merge. The label only starts verification and is never the gate. **A PR can be merged or enter the merge queue ONLY when its LATEST commit has a passing CI verdict** (binding): the required check on the PR head must be green. An unlabelled PR never gets a green required check, so it can never merge. A push after a green run invalidates it: the new head must pass again, started automatically while the label is present. Velnor must never report success for a commit it did not verify; 'not requested' shows as pending or a non-success state that GitHub's ruleset treats as blocking. The merge queue is an ADDITIONAL check, never a substitute: it verifies the PR combined with the current `main`, or the queue tip, and reuses the PR's verdict only when that tree is identical to the one the PR passed. Specify the ruleset settings (required checks on PRs and on `merge_group`) that enforce this. Verify how label events reach Velnor: workflow triggers from `GITHUB_TOKEN` changes are suppressed, but App webhook delivery may differ. Cite GitHub docs or the API, and give a fallback.

## Inputs

- **Design v2:** `~/.cache/ci-ambition/panel/v2/velnor-runner-architecture.md`, and its decision log `~/.cache/ci-ambition/panel/v2/decisions.md`. v2 already folds in the panel's earlier feedback and these owner decisions.
- **The earlier rounds:** `~/.cache/ci-ambition/panel/draft/` (the proposals, reviews and comparison); the design feedback comparison is at `~/.cache/ci-ambition/panel/v2/design-feedback-comparison.md`.
- **Evidence:** `~/.cache/ci-ambition/panel/evidence.md`, plus the new measurements:
  - mbx vs reflinked snapshots on this Mac: `~/.cache/ci-ambition/panel/v2/experiments/mbx-cache-mac.md`;
  - the same comparison on the real cluster (fixed checkout path, XFS reflink, mbx-cache server, a lockfile change, page cache): `~/.cache/ci-ambition/panel/v2/experiments/mbx-cache-cluster.md`;
  - blaze on velnor-new: `~/.cache/ci-ambition/panel/v2/experiments/blaze.md`.
- **Code, read-only:** `~/Projects/tailrocks/velnor-new`, `~/.cache/ci-ambition/refs/{velnor-legacy,actions-runner,arc,mr-boxington-cache,blaze}`. The official runner's source is the reference for protocol and semantics: cite `path:line`.

## What to design (be concrete: components, data flow, numbers)

Write `rust-only.md` in your scratch directory with these `##` sections:

1. **Summary.** In 15 lines or fewer: your design, and the push-to-merge and PR-verdict P50/P90 it reaches, both committed and stretch.
2. **The latency budget.** Trace one typical velnor-new PR from `git push` to merged on `main`, stage by stage, with today's measured time and your design's time for each. The stages:
   - the GitHub event, webhook and job offer;
   - acquire;
   - sandbox start;
   - checkout;
   - toolchain and tools;
   - dependency fetch;
   - compile;
   - tests;
   - result reporting;
   - the required check;
   - merge-queue entry and its run;
   - the merge.
   Name the bottleneck at each stage and how you remove it. Mark every number as measured, from evidence, or estimated (with the arithmetic).
2b. **Duplicate-work audit.** A table of EVERY operation across ALL stages: PR, merge group, push to `main`, release-plz release and its assets (`<bin>-<target>.tar.gz`, SHA256SUMS, attestations), scheduled or nightly runs, cache warmers, and conformance runs. Read the real workflows: `.github/workflows/*`, release-plz config and the release workflow, `ci-helper/` and `generator/`. Include the PR dimensions: push N vs push N+1 of one PR, rebase-only pushes, re-runs with no code change, concurrent PRs sharing unchanged parts, stacked PRs, and agent pre-push vs CI. Columns: operation, stages that run it, same input (yes or no), today's cost (minutes, GB of cache), your decision (eliminate or reuse, or keep as duplicate because it is faster, with numbers), and the mechanism (content-addressed key, promoted artifact, receipt). End with the total runner-minutes and cache GB per merged PR and per release, today against your design.
3. **The native Rust runner.** Cover:
   - the crates and components;
   - the protocol client, latest APIs only;
   - the job-message model;
   - the expression engine;
   - step, composite and post-step semantics;
   - contexts, env, secrets and masking;
   - cancellation, timeouts and lock renewal;
   - logs and timeline via Results;
   - crash safety.
   Say what you'd copy from velnor-legacy and velnor-new, and what you'd write fresh. Say how it starts a job in the fewest milliseconds (pre-warmed sandboxes, snapshot/restore, pre-acquired sessions, pipelining acquire with sandbox start).
4. **Native actions.** For each action in the closed set: the native design, what it does locally instead of over the network, and the expected time against today's measured step time.
5. **The conformance oracle.** Cover:
   - the differential harness: corpus, normalisation and comparison rules;
   - how it runs against each new official release;
   - pass criteria for cutover;
   - how it catches GitHub protocol drift within hours.
6. **Compile, cache and test.** Choose between reflinked snapshots, mbx and mbx-cache, blaze, and anything better, using the measured numbers. Also cover:
   - test-level scheduling across the cluster;
   - breaking the 125–138 s single-test floor;
   - verdict reuse without trusting anything unproven.
7. **Scheduling and the cluster.** Cover placement, zero queue, zero cold start, using all 480 threads, isolation of untrusted code, both sites, the other orgs sharing the nodes, and host and kernel tuning.
8. **The merge queue and GitHub integration.** Cover:
   - no duplicate verification across PR, merge group and main;
   - speculation on the queue tip;
   - the `Required` and release-gate constraints;
   - auto-merge and agent behaviour;
   - the cancelled-run stall.
9. **What would make CI not a bottleneck at all.** Write this section as an absolute: the theoretical floor of push-to-merge for this repository, and the design that reaches it. Identify the irreducible costs.
10. **Critique of v2.** Name what it gets wrong or misses under the new decisions, by section, and which of its decisions you'd overturn.
11. **Phases.** Give the order of delivery with numeric targets per phase (PR verdict, push-to-merge, runner-minutes, cache hit %, conformance pass %). Mark the first 5 work items.
12. **Risks.** For each top risk: how it's detected and contained, including protocol breakage and false greens.

Aim for 500–900 lines. Be quantitative, specific and ambitious. Never relax the same-confidence rule. Print the file as your final message too.

## Rules (binding)

- Read-only everywhere except your scratch directory (the current working directory).
- No pushes, merges, comments, issues, or GitHub or cluster changes. Read-only kubectl verbs only.
- Never print secrets or kubeconfigs.
- Never run `rex`. Never touch `~/.local/share/mise`. Never use Grok. Kill processes by PID only.
