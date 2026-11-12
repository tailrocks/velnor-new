# Vision, promises and rules

This file says what Velnor's Rust generator is for, what it promises the people who use it, and which lines no change may cross. Changing this file requires the owner's explicit approval.

The work that gets the project there is tracked in the roadmap, issue [#574](https://github.com/tailrocks/velnor-new/issues/574).

## What this file governs

| Code | Rule book |
|---|---|
| New work: the Rust generator in `generator/`, the CI helper in `ci-helper/`, the local environment in `local/`, and the workflows the generator writes | This file |
| The existing engine in `crates/`, its configuration in `.velnor/` and the workflows it generates. That includes this repository's own CI, Rust included, until the generator takes over the Rust part. The engine is maintained, not developed | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |

The root `AGENTS.md` applies to all code. Hard decisions bind the existing engine only. Where new work needs the same choice, this file states it.

New work serves Rust projects on GitHub Actions with GitHub-hosted runners. Jobs are executed by GitHub's official runner. Velnor does not implement a runner of its own.

## Vision

Velnor reads a Rust project and writes the GitHub Actions workflows that check it. It gives the shortest wait for a trustworthy result that GitHub-hosted runners allow, and the project's owner tunes nothing by hand. Velnor analyses the project and decides.

## Terms

- **Selects.** Design rule 2 defines which crates a run selects. Every statement below about what a run checks or compiles follows that rule.
- **Configuration.** One combination of target platform, features and build profile.
- **Warm cache.** A cache that still holds the work of an earlier run. A statement about reuse holds with a warm cache. A cache that is missing, evicted or rejected is covered by line 3 under "What we never compromise".

## What we promise our users

1. **Nothing to do means nothing runs.** A run checks only what it selects. A run that selects no crate runs no Rust job.
2. **Work is never done twice.** Within a run, nothing is compiled a second time for the same purpose. Across runs, nothing that a warm cache holds is downloaded or compiled again.
3. **Fast never means checked less.** Every check still runs when something it reads changes, with the project's own compiler, features and settings. A change that breaks a check fails the workflow, however fast the workflow becomes.

## What we never compromise

1. **Correct before fast.** A check is skipped only when the change provably cannot affect it. When Velnor cannot tell what a file affects, it runs the checks.
2. **Green means verified.** The required check passes only when everything that was selected ran and passed. A test step that should have run tests and ran none is not a pass.
3. **A broken cache is slow, never wrong, and never silent.** A missing, rejected or stale cache takes a correct cold path. A cache that fails at its job is reported by a red check, as design rule 3 defines.
4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token. The token is discarded, and cargo then runs offline. A pull request writes a cache only into its own scope. A pull request from a fork only reads.
5. **Every speed claim has a measurement.** A change states its effect with numbers from real runs, before and after.
6. **Inside GitHub's free limits.** The design fits in GitHub's 10 GB of cache and on standard hosted runners. It does not depend on paid storage or larger machines.

## Targets

These are the numbers behind the promises, for a workflow the generator writes, on GitHub-hosted runners. Counts are exact. Times are medians of at least three ordinary runs, because the same build varies from run to run.

| Situation | Target |
|---|---|
| A run that selects no crate | Velnor's own jobs finish within 1 minute |
| A run that selects everything, with a warm cache that holds every crate | 0 crates compiled, 0 crates downloaded |
| A run that selects some crates, with a warm cache | Only those crates are compiled and tested |
| A second push to the same pull request, with a warm cache | Compiles only what the changes since the first push select |
| Time from job start to the first cargo command, cache restore included | At most 30 seconds |
| Number of jobs | Does not grow with the number of crates in a workspace |
| Cache storage | At most 8 GB of GitHub's 10 GB, pull request layers included |
| An ordinary pull request, compared with the previous Velnor version | Neither the wait for the required check nor the runner-minutes gets worse. The one exception is a check that correctness needs, with its cost measured and accepted by the owner |
| The generator's own tests | Under 1 minute |

## Design rules

1. **Analyse, then decide.** Velnor reads the project (workspaces, crates, dependencies, features, tests, tools) and chooses the best workflow from what it finds. A project that configures nothing still gets a fast workflow.
2. **What a run selects.** A changed file that a crate's build, tests, lint or documentation reads selects that crate and every crate that depends on it. Source files, manifests and build scripts always do. Any other file inside a crate's directory selects the crate unless Velnor can show that nothing reads it. The lock file, the toolchain file, cargo configuration and the workflow itself select everything. A documentation or licence file outside every crate selects nothing, unless a build, test, lint or documentation step reads it. Velnor finds the other files each crate reads. Any file it cannot place selects everything until the project maps it. A scheduled full run selects everything. So does a push to the default branch or a merge queue run in a project that chooses that.
3. **The cache proves itself.** Every run reports how many crates cargo compiled and how many the run should have needed. A check named `Cache` turns red when a crate is compiled although the restored cache holds it, and when the default branch cannot save its cache. That check is visible on every pull request and does not block merging. A run that restores no cache reports a cold run, and that is not a failure.
4. **Compile at most twice.** In one run, and for one configuration, a crate is compiled at most once as metadata, for lint and documentation, and at most once in full, for tests. Test binaries are built once for each configuration and handed to every test group that runs it.
5. **The shape fits the project.** Each workspace, or group of workspaces, starts as one job. A job is added for speed only when it shortens the wait by more than its own start-up time. A job is added for need when a test group requires what the shared job cannot give, such as a different machine. A large project builds once and runs its tests in groups. Velnor chooses from measured build and test times and writes the reason into the workflow.
6. **Test groups carry their needs.** A group names the tests that run together, by crate, test binary or name. It can require Docker, limit how many tests run at once, and set its own time limit and machine.
7. **No pointless steps, and no silent ones.** A step is left out only when Velnor can show it has nothing to check. Every run decides again and lists what it left out and why.
8. **A pull request builds on its own last run.** A pull request saves what it built, and only that pull request reads it.
9. **No logic in YAML.** Every generated `run:` step is one command. Decisions and loops live in one small helper program, written in Rust and shipped prebuilt.
10. **Configuration matches reality.** Velnor checks its configuration against the repository, such as the name of the default branch. A mismatch fails the run.
11. **Local first.** A workflow is verified on the developer's machine with GitHub's official runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards.
12. **Proven on real projects.** A change is tried on the small lab project `tailrocks/velnor-lab` first. This repository runs on its own generator, and that is the final proof.
13. **Projects stay current.** A new Velnor version reaches a project only after it has passed on the lab project, and it is merged only with the project's own checks green.
14. **Decided by experiment.** A choice between tools or designs is settled by an experiment on the lab project. The question, the measurements and the rule that picks the winner are written down before the first run. Whoever runs the experiment applies the rule and commits the result with its raw data. A choice that would change this file or a hard decision goes to the owner.

## What each project decides

Velnor provides the capability. The project chooses in its configuration.

| Choice | Options | Default |
|---|---|---|
| What a push to the default branch, or a merge queue run, selects | What the change selects, or everything | What the change selects |
| A scheduled full run | Off, hourly, nightly, weekly, or a custom schedule | Nightly |
| Test groups | Which tests run together, and what each group needs | One group, split automatically when its tests are long |
| Workspaces that share a job | Any set of workspaces | Each workspace on its own |
| Checking out only what Rust needs | On or off | Off |
| Receiving new Velnor versions | Off, as a pull request, or merged automatically | Merged automatically |

## How to review a change

Check the change against every promise, line, target and rule above that it can affect. A miss needs a reason in the pull request, and a target that gets worse needs the owner's acceptance. Then check that the change holds for one big crate, many workspaces, tests that need Docker, private dependencies and a large checkout.
