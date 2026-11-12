# Vision, promises and rules

This file says what Velnor is for, what it promises the people who use it, and which lines no change may cross. Every change and every review is checked against it.

Where another document in this repository disagrees with this file, this file wins, and the other document is corrected in the same change.

The work that gets the project there is tracked in the roadmap, issue [#574](https://github.com/tailrocks/velnor-new/issues/574).

## Vision

Velnor reads a Rust project and writes the GitHub Actions workflows that check it. It gives the shortest wait for a trustworthy result that GitHub-hosted runners allow, and the project's owner tunes nothing by hand. Velnor analyses the project and decides.

## What we promise our users

1. **Nothing to do means nothing runs.** A change is checked only for what it can affect. A change to files that no Rust build, test, lint or documentation step reads runs no Rust job.
2. **Work is never done twice.** Within a run, a crate is compiled at most once for lint and documentation and at most once for tests. A later run does not download or compile what an earlier run on the default branch, or on the same pull request, already produced.
3. **Fast never means checked less.** Every check still runs when something it reads changes, with the project's own compiler, features and settings. A faster workflow fails on every change the slower one failed on.

## What we never compromise

1. **Correct before fast.** A check is skipped only when the change provably cannot affect it. When Velnor cannot tell what a file affects, it runs the checks.
2. **Green means verified.** The required check passes only when everything that was selected ran and passed. A test run that executed no test is not a pass.
3. **A broken cache is slow, never wrong, and never silent.** A missing, rejected or stale cache takes a correct cold path. A cache that cannot be saved, or that fails to prevent a rebuild, turns a check red.
4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token. The token is discarded, and cargo then runs offline. A pull request writes a cache only into its own scope. A pull request from a fork only reads.
5. **Every speed claim has a measurement.** A change states its effect with numbers from real runs, before and after.
6. **Inside GitHub's free limits.** The design fits in GitHub's 10 GB of cache and on standard hosted runners. It does not depend on paid storage or larger machines.

## What "fast" means

These are targets for a generated workflow on GitHub-hosted runners. Counts are exact. Times are medians of at least three ordinary runs, because the same build varies from run to run.

| Situation | Target |
|---|---|
| A change that touches no Rust input | No Rust job runs. Velnor's own jobs finish within 1 minute |
| A full run where no Rust input changed since the cache was saved | 0 crates compiled, 0 crates downloaded |
| A change to one crate | Only that crate and what depends on it is compiled and tested |
| A second push to the same pull request | Recompiles only the crates changed since the first push, and what depends on them |
| An empty cache | No crate is compiled more than twice |
| Time from job start to the first cargo command, cache restore included | At most 30 seconds |
| Number of jobs | Does not grow with the number of crates |
| Cache storage | At most 8 GB of GitHub's 10 GB, pull request layers included |
| An ordinary pull request, compared with the previous Velnor version | Neither the wait for the required check nor the runner-minutes gets worse |
| The tests of the generator in `generator/` | Under 1 minute |

## Design rules

1. **Analyse, then decide.** Velnor reads the project (workspaces, crates, dependencies, features, tests, tools) and chooses the best workflow from what it finds. A project that configures nothing still gets a fast workflow.
2. **No Rust change, no Rust jobs.** A file inside a crate's directory selects that crate. The lock file, the toolchain file, cargo configuration and the workflow itself select everything. Documentation and licence files outside every crate select nothing. Velnor finds the other files each crate reads. Any file it cannot place selects everything until the project maps it.
3. **The cache proves itself.** Every run reports how many crates cargo compiled and how many the change should have needed. Compiling a crate the run expected to be fresh turns a check named `Cache` red. So does a cache that cannot be saved on the default branch. That check is visible on every pull request and does not block merging.
4. **Compile at most twice.** In one run a crate is compiled at most once as metadata, for lint and documentation, and at most once in full, for tests. Test binaries are built on one machine and handed to every test group. A second identical configuration, test group or machine compiles nothing again.
5. **The shape fits the project.** One job by default. A job is added only when it shortens the wait by more than its own start-up time. A large project builds once and runs its tests in groups. Velnor chooses from measured build and test times and writes the reason into the workflow.
6. **Test groups carry their needs.** A group names the tests that run together, by crate, test binary or name. It can require Docker, limit how many tests run at once, and set its own time limit and machine.
7. **No pointless steps, and no silent ones.** A step is left out only when Velnor can show it has nothing to check. Every run decides again and lists what it left out and why. The scheduled full run runs everything.
8. **A pull request builds on its own last run.** Only that pull request reads its cache.
9. **No logic in YAML.** Every generated `run:` step is one command. Decisions and loops live in one small helper program, written in Rust and shipped prebuilt.
10. **Configuration matches reality.** Velnor checks its configuration against the repository, such as the name of the default branch. A mismatch fails the run.
11. **Local first.** A workflow is verified on the developer's machine with GitHub's official runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards.
12. **Proven on real projects.** A change is tried on the small lab project `tailrocks/velnor-lab` first. This repository runs on its own generator, and that is the final proof.
13. **Projects stay current.** A project receives a new Velnor version automatically, after that version has passed on the lab project and with the project's own checks green.

## What each project decides

Velnor provides the capability. The project chooses in its configuration.

| Choice | Options | Default |
|---|---|---|
| What a merge to the default branch runs | Only what changed, or everything | Only what changed |
| A scheduled full run | Off, hourly, nightly, weekly, or a custom schedule | Nightly |
| Test groups | Which tests run together, and what each group needs | One group, split automatically when its tests are long |
| Workspaces that share a job | Any set of workspaces | Each workspace on its own |
| Checking out only what Rust needs | On or off | Off |
| Receiving new Velnor versions | Off, as a pull request, or merged automatically | Merged automatically |

## Scope

- Rust projects on GitHub Actions with GitHub-hosted runners.
- The existing engine in `crates/velnor-actions-*` keeps generating CI for other languages and the release and image workflows. It is maintained, not developed.
- Jobs are executed by GitHub's official runner. Velnor does not implement a runner of its own.

## How this file relates to the others

- [`hard-decisions.md`](hard-decisions.md) records what the workflows of the existing engine must satisfy today. New work follows this file. Decisions 1 and 3 there are rewritten when the new generator takes over this repository's Rust CI.
- [`reviews/ci-performance.md`](reviews/ci-performance.md) is the detailed review procedure. It applies the rules in this file.

## How to review a change

Ask these questions. A "no" needs a reason in the pull request.

1. For each row of the targets table that this change can affect, is there a measurement before and after, and is none worse?
2. Does every check that failed before still fail on the same input?
3. If a check or step was removed or narrowed, what proves it had nothing to verify, and what brings it back when that changes?
4. Does it avoid repeating work that this run or an earlier run already did?
5. Does a project that configures nothing still get the right behaviour?
6. Is every generated step still one command, with the logic in the helper?
7. Does the number of jobs stay independent of the number of crates?
8. If it can fail, does it fail loudly?
9. Does it hold for one big crate, many workspaces, tests that need Docker, private dependencies and a large checkout?
10. Was it verified locally, then on the lab project?
11. Does it cross none of the six lines under "What we never compromise"?
