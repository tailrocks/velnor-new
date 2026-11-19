# tailrocks/velnor-new GitHub organisation inventory (read-only, 2026-11-19)

Issues total 413 (open 126, closed 287, closed in last 30d 287). Open PRs 114. Project items 160. Milestones 9. Labels 25.

Note: "body length" = length of GraphQL bodyText (plain text), approximates body size.

## 1. Project 2

- Title: Velnor fast Rust CI
- Description: Roadmap #574 of tailrocks/velnor-new: the fastest Rust CI a generator can produce
- Public: True, closed: False
- URL: https://github.com/orgs/tailrocks/projects/2

### README

```
This project shows the roadmap in issue https://github.com/tailrocks/velnor-new/issues/574.

- **Board**: every task as a card, by status.
- **Now**: what an agent is working on.
- **Next**: what can start, because nothing open blocks it.
- **Waiting**: blocked by another issue.
- **Needs you**: follow-ups only the owner can do, and tasks that could not be finished.
- **Done**: closed issues.

The process is in https://github.com/tailrocks/velnor-new/blob/main/docs/roadmap.md.
```

### Fields

| Field | Type | Options |
|---|---|---|
| Title | TITLE |  |
| Assignees | ASSIGNEES |  |
| Status | SINGLE_SELECT | Todo, In Progress, Done |
| Labels | LABELS |  |
| Linked pull requests | LINKED_PULL_REQUESTS |  |
| Milestone | MILESTONE |  |
| Repository | REPOSITORY |  |
| Reviewers | REVIEWERS |  |
| Parent issue | PARENT_ISSUE |  |
| Sub-issues progress | SUB_ISSUES_PROGRESS |  |
| Created | CREATED |  |
| Updated | UPDATED |  |
| Closed | CLOSED |  |
| Stage | SINGLE_SELECT | FAST-0 Measure, FAST-1 Foundation, FAST-2 Selection, FAST-3 Job shape, FAST-4 Cache, FAST-5 Dogfood, FAST-6 Guard, FAST-L Local, FAST-R Old work |

### Views

| View | Layout | Group by (vertical) | Filter |
|---|---|---|---|
| Board | BOARD_LAYOUT | Status | `-label:epic` |
| Now | TABLE_LAYOUT |  | `is:open label:in-progress` |
| Next | TABLE_LAYOUT |  | `is:open -is:blocked -label:epic -label:in-progress -label:blocked -label:owner-action` |
| Waiting | TABLE_LAYOUT |  | `is:open is:blocked -label:epic` |
| Needs you | TABLE_LAYOUT |  | `is:open label:owner-action,blocked` |
| Done | TABLE_LAYOUT |  | `is:closed` |
| Everything | TABLE_LAYOUT |  | `` |

All views show: Title, Assignees, Status, Linked pull requests, Sub-issues progress. No view groups/filters by Stage; no iteration field exists.

### Items

Repos: {'tailrocks/velnor-new': 160}; content types: {'Issue': 160}

Status: {'Todo': 23, 'Done': 124, 'In Progress': 13}

Stage: {None: 93, 'FAST-0 Measure': 6, 'FAST-1 Foundation': 6, 'FAST-L Local': 9, 'FAST-2 Selection': 7, 'FAST-3 Job shape': 10, 'FAST-4 Cache': 8, 'FAST-5 Dogfood': 6, 'FAST-6 Guard': 5, 'FAST-R Old work': 10}

Open issues NOT on the project: [890, 931, 934, 941, 949, 966, 987, 1005, 1007, 1016, 1028, 1033, 1034, 1047, 1057, 1059, 1060, 1066, 1074, 1075, 1076, 1077, 1078, 1081, 1108, 1110, 1113, 1114, 1117, 1118, 1119, 1120, 1121, 1123, 1124, 1125, 1127, 1135, 1140, 1142, 1143, 1144, 1145, 1146, 1148, 1150, 1152, 1153, 1155, 1157, 1159, 1161, 1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1183, 1184, 1185, 1186, 1187, 1188, 1201, 1202, 1203, 1204, 1206, 1209, 1210, 1213, 1217, 1218, 1220, 1221, 1233, 1290, 1303, 1362, 1366, 1367, 1370, 1371, 1373, 1374, 1375, 1376]

| # | type | state | Status | Stage | Milestone | Title |
|---|---|---|---|---|---|---|
| 160 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1.2] Rust detection: workspaces, packages, the toolchain file and tool pins become facts |
| 574 | Issue | OPEN | Todo |  |  | [FAST] Roadmap: the fastest Rust CI a generator can produce |
| 575 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0] Lab and measurement: the measuring commands, velnor-lab, the experiments, the decisions |
| 577 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2] Selection: the plan picks packages, with a reason for each |
| 578 | Issue | OPEN | Todo | FAST-3 Job shape | FAST-3 Job shape | [FAST-3] Job shape: one job by default, more only when it pays |
| 579 | Issue | OPEN | Todo | FAST-4 Cache | FAST-4 Cache | [FAST-4] Cache: a restored build that cargo treats as fresh |
| 580 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1] Foundation: the new generator box, Rust detection, the test layers, the YAML rules |
| 581 | Issue | OPEN | Todo | FAST-6 Guard | FAST-6 Guard and adopt | [FAST-6] Guard and document |
| 582 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-H] Remove the branches that hold nothing more to land |
| 583 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0.1] Start the CI helper `velnor-ci` with the run-measuring commands |
| 584 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0.2] Create `velnor-lab`: a small Rust project for experiments and real runs |
| 585 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0.3] Lab experiments: how to use Mr. Boxington, and which job shape |
| 586 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0.5] Decisions: what is decided, what the lab decides, and what stays with the owner |
| 593 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.1] Selection types, the pure `select` function and `velnor-ci plan` |
| 594 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.2] Classify paths outside packages: ignored, global, unknown |
| 595 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.3] A `Cargo.lock` change selects only the packages whose dependencies changed |
| 596 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.4] Discover compile-time inputs from cargo's dep-info |
| 597 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.5] Configurable: what runs on main, and an optional scheduled full run |
| 598 | Issue | CLOSED | Done | FAST-2 Selection | FAST-2 Selection | [FAST-2.6] `velnor-ci gate`: a sound required check, and a summary of why each package ran |
| 599 | Issue | CLOSED | Done | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.1] Shape selection and the single job |
| 600 | Issue | CLOSED | Done | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.2] Build tests once, hand them to every test job |
| 601 | Issue | OPEN | Todo | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.3] Test groups with needs |
| 602 | Issue | CLOSED | Done | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.4] Setup: the helper's `prepare`, `restored` and `run` commands, and a plan job that needs no toolchain |
| 603 | Issue | CLOSED | Done | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.5] Events, concurrency and a conditional policy lane |
| 604 | Issue | OPEN | Todo | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5.3] Switch velnor-new's Rust CI to the new generator |
| 605 | Issue | CLOSED | Done | FAST-4 Cache | FAST-4 Cache | [FAST-4.1] Cache families, typed keys and the CI build profile |
| 606 | Issue | CLOSED | Done | FAST-4 Cache | FAST-4 Cache | [FAST-4.2] Freshness: with Mr. Boxington, an unchanged project compiles nothing |
| 607 | Issue | OPEN | In Progress | FAST-4 Cache | FAST-4 Cache | [FAST-4.3] Main writes the base cache from the ordinary CI run |
| 608 | Issue | OPEN | Todo | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5.5] Delete the existing engine's Rust CI path |
| 609 | Issue | OPEN | In Progress | FAST-4 Cache | FAST-4 Cache | [FAST-4.5] PR delta layer: the second push of a PR reuses the first |
| 610 | Issue | OPEN | In Progress | FAST-4 Cache | FAST-4 Cache | [FAST-4.4] Budget manager: stay under 8 GB without losing the base |
| 611 | Issue | CLOSED | Done | FAST-4 Cache | FAST-4 Cache | [FAST-4.6] Tools cache: the toolchain and tools from one entry, nothing installed on a warm run |
| 612 | Issue | CLOSED | Done | FAST-4 Cache | FAST-4 Cache | [FAST-4.7] `velnor-ci report`: cache telemetry and a waste check in every run |
| 616 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1.1] Create the generator box: model, pure pipeline, one place for input and output |
| 617 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1.5] Publish `velnor-ci` and `velnor-gen` as prebuilt binaries, pinned by a lock file |
| 619 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1.3] Test layers: rule tests, golden tests, and a built-in speed limit |
| 620 | Issue | CLOSED | Done | FAST-1 Foundation | FAST-1 Foundation | [FAST-1.4] YAML rules: every generated step is one command |
| 621 | Issue | CLOSED | Done | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5.4] Proof on velnor-new: run the scenarios and post the table |
| 622 | Issue | OPEN | Todo | FAST-6 Guard | FAST-6 Guard and adopt | [FAST-6.1] Speed budget: fail loudly when CI gets slower |
| 623 | Issue | OPEN | Todo | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5.1] velnor-lab runs the generator for real every night |
| 624 | Issue | OPEN | Todo | FAST-6 Guard | FAST-6 Guard and adopt | [FAST-6.2] Documentation: one page on how Velnor keeps Rust CI fast |
| 626 | Issue | OPEN | Todo | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5] Dogfood: velnor-new's own Rust CI comes from the new generator |
| 627 | Issue | OPEN | Todo | FAST-L Local | FAST-L Local GitHub | [FAST-L] Local GitHub: the official runner, a stand-in service and our own cache, all on one machine (P4: velnor-new on macOS local runner) |
| 628 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.1] Spike: the official ARM runner runs a job from runner.server on this Mac |
| 629 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.2] GitHub's cache rules as a pure Rust module |
| 630 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.3] The local cache server: GitHub's cache protocol in front of our rules |
| 631 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.4] A local stub for the GitHub API calls the helper makes |
| 632 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.5] The driver: `velnor-local run` replays a scenario on this machine |
| 633 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.6] Conformance probes: record how real GitHub behaves, in velnor-lab |
| 634 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R] Land the finished work that still has value |
| 635 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.1] Merge #554: runner fixes for long live sessions |
| 636 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.2] Merge #556: source qualification and Rust jobs run only for affected changes |
| 637 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.3] Merge #523: keep the private registry address in the Bun lifecycle config |
| 638 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.4] Runner: trim a piped token before storing it |
| 639 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.5] Homebrew tap: accept a clean checkout under a strict umask |
| 640 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.6] Fix apostrophes in generated workflow commands |
| 641 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.7] Validate artifact ids before downloading in the preview and release workflows |
| 642 | Issue | CLOSED | Done | FAST-R Old work | FAST-R Old work | [FAST-R.8] Bind job-image publication to the exact run and the attested files |
| 644 | Issue | OPEN | In Progress | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.6] Workspaces that share a job |
| 645 | Issue | OPEN | Todo | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.7] Checking out only what Rust needs |
| 646 | Issue | OPEN | Todo | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.8] Private dependencies: fetch with a short-lived token, then build offline |
| 647 | Issue | CLOSED | Done | FAST-3 Job shape | FAST-3 Job shape | [FAST-3.9] Feature configurations, run once when identical |
| 648 | Issue | OPEN | Todo | FAST-6 Guard | FAST-6 Guard and adopt | [FAST-6.3] Projects receive new Velnor versions automatically |
| 649 | Issue | OPEN | Todo | FAST-6 Guard | FAST-6 Guard and adopt | [FAST-6.4] Adopt the new generator in the owner's Rust projects |
| 650 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.7] velnor-runner: native ARM containers |
| 651 | Issue | CLOSED | Done | FAST-L Local | FAST-L Local GitHub | [FAST-L.8] velnor-runner: a local mode, and the driver uses it |
| 652 | Issue | CLOSED | Done | FAST-0 Measure | FAST-0 Measure and decide | [FAST-0.4] Lab experiment: does blaze earn a place in a generated workflow |
| 653 | Issue | CLOSED | Done | FAST-5 Dogfood | FAST-5 Dogfood | [FAST-5.2] Dogfood: this repository's CI builds the helper from source |
| 662 | Issue | OPEN | Todo |  |  | [OWNER] Run the Qualification workflow once on the scale set after #554 |
| 665 | Issue | CLOSED | Done |  | FAST-1 Foundation | [FAST-1.1b] velnor-gen reads and writes a repository: the io module, generate and check |
| 674 | Issue | CLOSED | Done |  |  | [OWNER] Decide how the helper fetch step fits design rule 9 (one command per run step) |
| 680 | Issue | OPEN | Todo |  |  | [OWNER] Approve the existing engine's exemption for the hand-written release workflow |
| 682 | Issue | OPEN | Todo |  |  | [OWNER] Allow only admins to create rust-ci-v* tags |
| 685 | Issue | CLOSED | Done |  |  | [OWNER] Provide a private repository for the velnor-runner arm64 proof run (#650) |
| 694 | Issue | CLOSED | Done |  |  | [FAST] Follow the real edges of unowned path packages in select |
| 697 | Issue | CLOSED | Done |  |  | [FAST] Make velnor-ci inputs audit safe after a partial build and with unusual target layouts |
| 701 | Issue | CLOSED | Done |  |  | [OWNER] Change docs/goals.md rule 2 so a Cargo.lock change selects only the packages whose dependencies changed (#595) |
| 703 | Issue | CLOSED | Done |  |  | [FAST-2.3b] Select everything when an edit leaves Cargo.lock inconsistent with the manifests |
| 705 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.1b] `velnor-gen timings`: write the timings file from the last runs |
| 707 | Issue | CLOSED | Done |  |  | [OWNER] Decide the job-start check for velnor-runner's local mode (#651) |
| 710 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.1c] `velnor-ci plan` reads no file beyond what the doc-step rule needs |
| 712 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.1d] A sound rule for leaving out the doctest and documentation steps |
| 713 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.4b] The generated job installs the whole declared toolchain profile |
| 716 | Issue | CLOSED | Done |  |  | [FAST] Make velnor-ci inputs audit check each build-script run, custom target specs and host units of build-dependency members |
| 717 | Issue | CLOSED | Done |  |  | [FAST-R.9] Install velnor-ci through a setup-velnor action, so rule 9 needs no exception |
| 718 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.1e] Run custom test harnesses that nextest cannot list |
| 719 | Issue | CLOSED | Done |  |  | [FAST] Credit reads of a path package outside the workspaces by the package cargo's fingerprint names |
| 720 | Issue | CLOSED | Done |  |  | [OWNER] Create the velnor-release App, the release environment and the lab tag ruleset (for FAST-REL.1) |
| 721 | Issue | CLOSED | Done |  |  | [FAST-REL.1] Prove App-only tag creation on velnor-lab |
| 722 | Issue | CLOSED | Done |  |  | [FAST-REL.2] The engine leaves workflows that carry the velnor-gen header |
| 723 | Issue | CLOSED | Done |  |  | [FAST-REL.3] velnor-gen owns every workflow with its header |
| 724 | Issue | CLOSED | Done |  |  | [FAST-REL.4] velnor-ci release-gate reads the required checks from the branch rules and binds them to push runs |
| 725 | Issue | CLOSED | Done |  |  | [FAST-REL.5] velnor-ci release record and release publish |
| 726 | Issue | CLOSED | Done |  |  | [FAST-REL.6] Read release-plz.toml and .velnor/release.toml into a ReleasePlan |
| 727 | Issue | CLOSED | Done |  |  | [FAST-REL.7] Render release.yml: trigger, gate, release-pr, tag |
| 728 | Issue | CLOSED | Done |  |  | [FAST-REL.8] Render build, sums and publish, and add velnor-gen release ruleset |
| 729 | Issue | OPEN | Todo |  |  | [FAST-REL.9] Prove the whole release flow on velnor-lab |
| 730 | Issue | CLOSED | Done |  |  | [OWNER] Create the seed tags velnor-ci-v0.1.79 and velnor-gen-v0.1.79 (for FAST-REL.10) |
| 731 | Issue | OPEN | In Progress |  |  | [FAST-REL.10] Velnor's own release-plz config and per-package tags |
| 732 | Issue | OPEN | Todo |  |  | [OWNER] Switch the release-tags ruleset to the App-only bypass (for FAST-REL.11) |
| 733 | Issue | OPEN | Todo |  |  | [FAST-REL.11] Generate this repository's release.yml and delete the #676 exemption |
| 734 | Issue | CLOSED | Done |  |  | [OWNER] Push the bootstrap tag rust-ci-v0.1.80 after R4 and R5 (for FAST-REL.9) |
| 736 | Issue | CLOSED | Done |  |  | [FAST-L.9] velnor-host names a too-long control socket path instead of reporting a held lock |
| 737 | Issue | CLOSED | Done |  |  | [FAST-L.10] The resource probe works on a host whose mountinfo is larger than 64 KiB |
| 739 | Issue | CLOSED | Done |  |  | [FAST-1.x] The generator's tests start no external program |
| 740 | Issue | CLOSED | Done |  |  | [FAST-3.1f] Documentation checks no crate twice and documents only what a change affects |
| 742 | Issue | CLOSED | Done |  |  | [FAST-L.8.2] velnor-host: the local-mode feature, its release lock, and a pair core with no reservation |
| 743 | Issue | CLOSED | Done |  |  | [FAST-L.8.3] velnor-local: the local authority |
| 744 | Issue | CLOSED | Done |  |  | [FAST-L.8.4] velnor-host local: pairs from a plan, the local job-start check, replacement and cleanup |
| 745 | Issue | CLOSED | Done |  |  | [FAST-L.8.5] velnor-local: --runner pair runs scenarios on velnor-host pairs |
| 746 | Issue | CLOSED | Done |  |  | [FAST-L.8.6] velnor-local: fork pairs, the refusal proofs and the optional --policy |
| 747 | Issue | CLOSED | Done |  |  | [FAST-L.8.7] velnor-local: doctor, status and the README |
| 748 | Issue | CLOSED | Done |  |  | [FAST-L.8.1] velnor-local: build the pair images and mint a runner.server JIT |
| 750 | Issue | CLOSED | Done |  | FAST-3 Job shape | [FAST-3.1f] Trust boundary for project configuration that weakens generated checks |
| 751 | Issue | CLOSED | Done |  |  | [FAST-L.11] velnor-host recovers a reservation after a crash during storage initialization |
| 752 | Issue | CLOSED | Done |  |  | [FAST-L.12] velnor-host reports the real error when a pair's helper check fails, and does not leave the reservation stuck |
| 753 | Issue | CLOSED | Done |  |  | [FAST-L.13] velnor-host connect creates its state folder 0700, and completion diagnostics never panic |
| 754 | Issue | CLOSED | Done |  |  | [FAST-L.14] A finished pair leaves no private network, and completion cleanup proves it |
| 760 | Issue | CLOSED | Done |  |  | [FAST] velnor-ci inputs audit honours cfg-gated build-dependencies |
| 764 | Issue | CLOSED | Done |  |  | [FAST] Test that setup-velnor refuses a binary with the wrong SHA-256 |
| 768 | Issue | CLOSED | Done |  |  | [FAST-3.1b.1] A stale or malformed timings file never stops root discovery |
| 769 | Issue | CLOSED | Done |  |  | [FAST-3.1b.2] Timings use only samples from the current runner |
| 777 | Issue | CLOSED | Done |  |  | [FAST-3.5.1] The policy lane runs machete when a change can add an unused dependency |
| 779 | Issue | CLOSED | Done |  |  | [FAST-L.13.1] Every velnor-host state folder is created 0700 |
| 782 | Issue | CLOSED | Done |  |  | [FAST-3.1f.1] velnor-ci error output cannot forge a workflow command |
| 785 | Issue | CLOSED | Done |  |  | [FAST-L.12.1] Retire a pair that started but never bound and release its reservation |
| 786 | Issue | CLOSED | Done |  |  | [FAST-3.1f.2] Weakening warnings: only checked roots, correct "new" label, nextest overrides |
| 790 | Issue | CLOSED | Done |  |  | [FAST-1.y] velnor-ci audit tests pass in parallel |
| 794 | Issue | CLOSED | Done |  |  | [FAST-1.y.1] Remove test temp folders left by killed tests |
| 796 | Issue | CLOSED | Done |  |  | [FAST-REL.6.1] Build metadata in a release version is tagged or refused up front |
| 799 | Issue | CLOSED | Done |  |  | [FAST-3.4c] Wire every plan output into the generated steps |
| 800 | Issue | CLOSED | Done |  |  | [FAST-L.12.2] Test that a long helper error is cut at the detail limit |
| 801 | Issue | CLOSED | Done |  |  | [FAST-3.4d] Check the metadata file in CI and harden velnor-ci run |
| 802 | Issue | CLOSED | In Progress |  |  | [FAST-L.11.1] velnor-host prints recovery journal rows read-only |
| 806 | Issue | CLOSED | Done |  |  | [FAST-1.z] Tests stay well under the 2 s slow-timeout |
| 808 | Issue | CLOSED | Done |  |  | [FAST-3.4e] The plan reads changed files from git, with no token |
| 811 | Issue | CLOSED | Done |  |  | [FAST-6.x] Document the local recipe for fixture-dependent tests |
| 817 | Issue | CLOSED | Done |  |  | [FAST-REL.8.1] Release config values cannot become Actions expressions |
| 1000 | Issue | CLOSED | Done |  |  | Drive poll_connection in a test so its call sites are covered |
| 1011 | Issue | OPEN | In Progress |  |  | inputs save/restore ignore a custom build.target-dir |
| 1018 | Issue | OPEN | In Progress |  |  | inputs snapshot scope misses tracked path dependencies outside a nested root |
| 1032 | Issue | CLOSED | Done |  |  | Verify PR-only release exemptions against external checks and rule bypass |
| 1069 | Issue | CLOSED | Done |  |  | Base cache: report failed saves and skip saves after a failed producer |
| 1071 | Issue | CLOSED | Done |  |  | The from-source helper compiles when no job needs the binary |
| 1072 | Issue | OPEN | In Progress |  |  | A one-root policy job repeats the plan |
| 1073 | Issue | OPEN | In Progress |  |  | Scheduled inputs audit builds every workspace again |
| 1100 | Issue | CLOSED | Done |  |  | inputs audit treats a missing dep-info input or a file inside a rerun-if-changed directory as fresh |
| 1101 | Issue | CLOSED | Done |  |  | inputs audit writes the lock when a feature-gated unit's dep-info is stale |
| 1102 | Issue | CLOSED | Done |  |  | inputs audit walks the target directory with no depth or file-size bound |
| 1103 | Issue | CLOSED | Done |  |  | plan prints a rejected git path to stderr as a workflow command |
| 1104 | Issue | OPEN | In Progress |  |  | prepare's pinned-tool check mismatches mise OS filters and lock rows |
| 1105 | Issue | CLOSED | Done |  |  | complete-profile components are installed after the tools cache is saved |
| 1106 | Issue | CLOSED | Done |  |  | Base cache: gate saves on the plan's private-source result |
| 1128 | Issue | OPEN | In Progress |  |  | Release qualification connects to a different Docker socket when the path contains unix:// |
| 1129 | Issue | CLOSED | Done |  |  | Qualification receipt is issued when a tool container has extra capabilities, devices, or host PID |
| 1130 | Issue | CLOSED | Done |  |  | Readiness scan treats a journal as in-bounds when launch.db-wal exceeds the file cap |
| 1131 | Issue | CLOSED | Done |  |  | Readiness scan stops at the first occupying intent and skips the row cap |
| 1132 | Issue | OPEN | In Progress |  |  | Readiness scan counts the legacy intents table and misses live connection_intents |
| 1133 | Issue | CLOSED | Done |  |  | Readiness Docker /info body is not size-capped |
| 1305 | Issue | OPEN | Todo |  |  | P3: CI/CD performance for velnor-new (tracker) |

## 2. Milestones

| # | Title | State | Due | Open | Closed | Description |
|---|---|---|---|---|---|---|
| 1 | FAST-0 Measure and decide | open | None | 0 | 6 | The measuring tool, the lab project and the experiments. Every open choice gets its decision, with data. |
| 2 | FAST-1 Foundation | open | None | 0 | 7 | The new generator box, Rust detection, the test layers, the YAML rules and the released binaries. |
| 3 | FAST-2 Selection | open | None | 0 | 7 | A run checks only what a change selects, with a reason for every choice. |
| 4 | FAST-3 Job shape | open | None | 5 | 11 | One job by default, more only when it pays. Test groups, setup, events, private dependencies. |
| 5 | FAST-4 Cache | open | None | 4 | 4 | A restored build that cargo treats as fresh, inside 8 GB, with a check that proves it. |
| 6 | FAST-5 Dogfood | open | None | 4 | 2 | The lab, then this repository, run on the new generator. The old Rust CI path is deleted. |
| 7 | FAST-6 Guard and adopt | open | None | 5 | 0 | A speed budget, the documentation, automatic updates and adoption in the owner's projects. |
| 8 | FAST-L Local GitHub | open | None | 1 | 8 | The official runner, a local stand-in and a local cache server, so a workflow is verified on the developer's machine. |
| 9 | FAST-R Old work | open | None | 0 | 10 | Finished old work that still has value is landed, and old branches are removed. |

## 3. Issues (all open + closed in last 30d) = 413

Cols: state, type, labels, milestone, assignees, parent, subs(total/completed), closing PRs, created, updated, bodyLen

| # | St | IssueType | Title | Labels | MS | Asg | Parent | Subs | PRs | Created | Updated | Body |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 160 | C | Feature | [FAST-1.2] Rust detection: workspaces, packages, the toolchain file and tool pins become f | fast-ci,exec:sonnet,effort:medium | FAST-1 Foundation |  | 580 | 0/0 | #688 | 2026-10-17 | 2026-11-14 | 2909 |
| 574 | O | Feature | [FAST] Roadmap: the fastest Rust CI a generator can produce | fast-ci,epic |  |  |  | 91/72 |  | 2026-11-12 | 2026-11-18 | 13075 |
| 575 | C | Feature | [FAST-0] Lab and measurement: the measuring commands, velnor-lab, the experiments, the dec | fast-ci,epic | FAST-0 Measure and decide |  | 574 | 5/5 |  | 2026-11-12 | 2026-11-14 | 1773 |
| 577 | C | Feature | [FAST-2] Selection: the plan picks packages, with a reason for each | fast-ci,epic | FAST-2 Selection |  | 574 | 6/6 |  | 2026-11-12 | 2026-11-18 | 1987 |
| 578 | O | Feature | [FAST-3] Job shape: one job by default, more only when it pays | fast-ci,epic | FAST-3 Job shape |  | 574 | 10/6 |  | 2026-11-12 | 2026-11-14 | 2211 |
| 579 | O | Feature | [FAST-4] Cache: a restored build that cargo treats as fresh | fast-ci,epic | FAST-4 Cache |  | 574 | 7/4 |  | 2026-11-12 | 2026-11-12 | 2472 |
| 580 | C | Feature | [FAST-1] Foundation: the new generator box, Rust detection, the test layers, the YAML rule | fast-ci,epic | FAST-1 Foundation |  | 574 | 6/6 |  | 2026-11-12 | 2026-11-14 | 2191 |
| 581 | O | Feature | [FAST-6] Guard and document | fast-ci,epic | FAST-6 Guard and adopt |  | 574 | 4/0 |  | 2026-11-12 | 2026-11-12 | 846 |
| 582 | C | Task | [FAST-H] Remove the branches that hold nothing more to land | fast-ci,exec:sonnet,effort:medium | FAST-R Old work |  | 574 | 0/0 |  | 2026-11-12 | 2026-11-14 | 4268 |
| 583 | C | Feature | [FAST-0.1] Start the CI helper `velnor-ci` with the run-measuring commands | fast-ci,exec:sonnet,effort:medium | FAST-0 Measure and decide |  | 575 | 0/0 | #655,#658 | 2026-11-12 | 2026-11-14 | 2881 |
| 584 | C | Task | [FAST-0.2] Create `velnor-lab`: a small Rust project for experiments and real runs | fast-ci,exec:sonnet,effort:medium | FAST-0 Measure and decide |  | 575 | 0/0 |  | 2026-11-12 | 2026-11-14 | 5154 |
| 585 | C | Task | [FAST-0.3] Lab experiments: how to use Mr. Boxington, and which job shape | fast-ci,exec:opus,effort:high | FAST-0 Measure and decide |  | 575 | 0/0 |  | 2026-11-12 | 2026-11-14 | 6777 |
| 586 | C | Task | [FAST-0.5] Decisions: what is decided, what the lab decides, and what stays with the owner | fast-ci | FAST-0 Measure and decide |  | 575 | 0/0 |  | 2026-11-12 | 2026-11-14 | 6549 |
| 593 | C | Feature | [FAST-2.1] Selection types, the pure `select` function and `velnor-ci plan` | fast-ci,exec:opus,effort:high | FAST-2 Selection |  | 577 | 0/0 | #690 | 2026-11-12 | 2026-11-14 | 4423 |
| 594 | C | Feature | [FAST-2.2] Classify paths outside packages: ignored, global, unknown | fast-ci,exec:sonnet,effort:medium | FAST-2 Selection |  | 577 | 0/0 | #692 | 2026-11-12 | 2026-11-14 | 1932 |
| 595 | C | Feature | [FAST-2.3] A `Cargo.lock` change selects only the packages whose dependencies changed | fast-ci,exec:sonnet,effort:high | FAST-2 Selection |  | 577 | 0/0 | #698 | 2026-11-12 | 2026-11-14 | 3205 |
| 596 | C | Feature | [FAST-2.4] Discover compile-time inputs from cargo's dep-info | fast-ci,exec:opus,effort:high | FAST-2 Selection |  | 577 | 0/0 | #693 | 2026-11-12 | 2026-11-14 | 2875 |
| 597 | C | Feature | [FAST-2.5] Configurable: what runs on main, and an optional scheduled full run | fast-ci,exec:sonnet,effort:medium | FAST-2 Selection |  | 577 | 0/0 |  | 2026-11-12 | 2026-11-16 | 3815 |
| 598 | C | Feature | [FAST-2.6] `velnor-ci gate`: a sound required check, and a summary of why each package ran | fast-ci,exec:sonnet,effort:high,in-progress | FAST-2 Selection |  | 577 | 0/0 | #998 | 2026-11-12 | 2026-11-17 | 1731 |
| 599 | C | Feature | [FAST-3.1] Shape selection and the single job | fast-ci,exec:opus,effort:high | FAST-3 Job shape |  | 578 | 0/0 | #706 | 2026-11-12 | 2026-11-14 | 4709 |
| 600 | C | Feature | [FAST-3.2] Build tests once, hand them to every test job | fast-ci,exec:sonnet,effort:high | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2810 |
| 601 | O | Feature | [FAST-3.3] Test groups with needs | fast-ci,exec:sonnet,effort:high | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2975 |
| 602 | C | Feature | [FAST-3.4] Setup: the helper's `prepare`, `restored` and `run` commands, and a plan job th | fast-ci,exec:opus,effort:high | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 4519 |
| 603 | C | Feature | [FAST-3.5] Events, concurrency and a conditional policy lane | fast-ci,exec:sonnet,effort:medium | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2440 |
| 604 | O | Task | [FAST-5.3] Switch velnor-new's Rust CI to the new generator | fast-ci,exec:opus,effort:high,needs-owner | FAST-5 Dogfood |  | 626 | 0/0 |  | 2026-11-12 | 2026-11-17 | 4086 |
| 605 | C | Feature | [FAST-4.1] Cache families, typed keys and the CI build profile | fast-ci,exec:opus,effort:high,in-progress | FAST-4 Cache |  | 579 | 0/0 | #981 | 2026-11-12 | 2026-11-16 | 4978 |
| 606 | C | Feature | [FAST-4.2] Freshness: with Mr. Boxington, an unchanged project compiles nothing | fast-ci,exec:opus,effort:high | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-18 | 3049 |
| 607 | O | Feature | [FAST-4.3] Main writes the base cache from the ordinary CI run | fast-ci,exec:sonnet,effort:high,in-progress | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2878 |
| 608 | O | Task | [FAST-5.5] Delete the existing engine's Rust CI path | fast-ci,exec:opus,effort:medium | FAST-5 Dogfood |  | 626 | 0/0 |  | 2026-11-12 | 2026-11-17 | 2950 |
| 609 | O | Feature | [FAST-4.5] PR delta layer: the second push of a PR reuses the first | fast-ci,exec:opus,effort:high,in-progress | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-17 | 2706 |
| 610 | O | Feature | [FAST-4.4] Budget manager: stay under 8 GB without losing the base | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-17 | 2861 |
| 611 | C | Feature | [FAST-4.6] Tools cache: the toolchain and tools from one entry, nothing installed on a war | fast-ci,exec:sonnet,effort:medium | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-18 | 1882 |
| 612 | C | Feature | [FAST-4.7] `velnor-ci report`: cache telemetry and a waste check in every run | fast-ci,exec:sonnet,effort:high,in-progress | FAST-4 Cache |  | 579 | 0/0 |  | 2026-11-12 | 2026-11-18 | 2811 |
| 616 | C | Feature | [FAST-1.1] Create the generator box: model, pure pipeline, one place for input and output | fast-ci,exec:opus,effort:medium | FAST-1 Foundation |  | 580 | 0/0 | #666 | 2026-11-12 | 2026-11-14 | 5284 |
| 617 | C | Task | [FAST-1.5] Publish `velnor-ci` and `velnor-gen` as prebuilt binaries, pinned by a lock fil | fast-ci,exec:sonnet,effort:high | FAST-1 Foundation |  | 580 | 0/0 |  | 2026-11-12 | 2026-11-14 | 3894 |
| 619 | C | Feature | [FAST-1.3] Test layers: rule tests, golden tests, and a built-in speed limit | fast-ci,exec:sonnet,effort:medium | FAST-1 Foundation |  | 580 | 0/0 | #670 | 2026-11-12 | 2026-11-14 | 3959 |
| 620 | C | Feature | [FAST-1.4] YAML rules: every generated step is one command | fast-ci,exec:sonnet,effort:medium | FAST-1 Foundation |  | 580 | 0/0 | #672 | 2026-11-12 | 2026-11-14 | 2647 |
| 621 | C | Task | [FAST-5.4] Proof on velnor-new: run the scenarios and post the table | fast-ci,exec:sonnet,effort:low | FAST-5 Dogfood |  | 626 | 0/0 | #1256 | 2026-11-12 | 2026-11-18 | 1485 |
| 622 | O | Feature | [FAST-6.1] Speed budget: fail loudly when CI gets slower | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-6 Guard and adopt |  | 581 | 0/0 |  | 2026-11-12 | 2026-11-18 | 1534 |
| 623 | O | Task | [FAST-5.1] velnor-lab runs the generator for real every night | fast-ci,exec:sonnet,effort:medium,needs-owner | FAST-5 Dogfood |  | 626 | 0/0 |  | 2026-11-12 | 2026-11-16 | 3039 |
| 624 | O | Task | [FAST-6.2] Documentation: one page on how Velnor keeps Rust CI fast | fast-ci,exec:sonnet,effort:medium | FAST-6 Guard and adopt |  | 581 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2355 |
| 626 | O | Feature | [FAST-5] Dogfood: velnor-new's own Rust CI comes from the new generator | fast-ci,epic | FAST-5 Dogfood |  | 574 | 5/2 |  | 2026-11-12 | 2026-11-12 | 1142 |
| 627 | O | Feature | [FAST-L] Local GitHub: the official runner, a stand-in service and our own cache, all on o | fast-ci,epic | FAST-L Local GitHub |  | 574 | 8/8 |  | 2026-11-12 | 2026-11-18 | 4557 |
| 628 | C | Task | [FAST-L.1] Spike: the official ARM runner runs a job from runner.server on this Mac | fast-ci,exec:opus,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 | #656,#659 | 2026-11-12 | 2026-11-14 | 4023 |
| 629 | C | Feature | [FAST-L.2] GitHub's cache rules as a pure Rust module | fast-ci,exec:sonnet,effort:high | FAST-L Local GitHub |  | 627 | 0/0 | #661 | 2026-11-12 | 2026-11-14 | 3258 |
| 630 | C | Feature | [FAST-L.3] The local cache server: GitHub's cache protocol in front of our rules | fast-ci,exec:opus,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 | #667 | 2026-11-12 | 2026-11-14 | 3468 |
| 631 | C | Feature | [FAST-L.4] A local stub for the GitHub API calls the helper makes | fast-ci,exec:sonnet,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 | #669 | 2026-11-12 | 2026-11-14 | 2317 |
| 632 | C | Feature | [FAST-L.5] The driver: `velnor-local run` replays a scenario on this machine | fast-ci,exec:opus,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 | #704 | 2026-11-12 | 2026-11-14 | 2854 |
| 633 | C | Task | [FAST-L.6] Conformance probes: record how real GitHub behaves, in velnor-lab | fast-ci,exec:sonnet,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 | #684 | 2026-11-12 | 2026-11-14 | 2214 |
| 634 | C | Feature | [FAST-R] Land the finished work that still has value | fast-ci,epic | FAST-R Old work |  | 574 | 8/8 |  | 2026-11-12 | 2026-11-14 | 2900 |
| 635 | C | Task | [FAST-R.1] Merge #554: runner fixes for long live sessions | fast-ci,exec:sonnet,effort:low,needs-owner | FAST-R Old work |  | 634 | 0/0 |  | 2026-11-12 | 2026-11-14 | 1981 |
| 636 | C | Task | [FAST-R.2] Merge #556: source qualification and Rust jobs run only for affected changes | fast-ci,exec:opus,effort:medium | FAST-R Old work |  | 634 | 0/0 | #660,#556 | 2026-11-12 | 2026-11-14 | 1858 |
| 637 | C | Task | [FAST-R.3] Merge #523: keep the private registry address in the Bun lifecycle config | fast-ci,exec:sonnet,effort:medium | FAST-R Old work |  | 634 | 0/0 | #523 | 2026-11-12 | 2026-11-14 | 1398 |
| 638 | C | Bug | [FAST-R.4] Runner: trim a piped token before storing it | fast-ci,exec:sonnet,effort:medium | FAST-R Old work |  | 634 | 0/0 | #686 | 2026-11-12 | 2026-11-14 | 1082 |
| 639 | C | Bug | [FAST-R.5] Homebrew tap: accept a clean checkout under a strict umask | fast-ci,exec:sonnet,effort:medium | FAST-R Old work |  | 634 | 0/0 | #708 | 2026-11-12 | 2026-11-14 | 1282 |
| 640 | C | Bug | [FAST-R.6] Fix apostrophes in generated workflow commands | fast-ci,exec:haiku,effort:medium | FAST-R Old work |  | 634 | 0/0 | #700 | 2026-11-12 | 2026-11-14 | 1067 |
| 641 | C | Bug | [FAST-R.7] Validate artifact ids before downloading in the preview and release workflows | fast-ci,exec:sonnet,effort:low | FAST-R Old work |  | 634 | 0/0 | #702 | 2026-11-12 | 2026-11-14 | 963 |
| 642 | C | Bug | [FAST-R.8] Bind job-image publication to the exact run and the attested files | fast-ci,exec:opus,effort:medium | FAST-R Old work |  | 634 | 0/0 | #663 | 2026-11-12 | 2026-11-14 | 1236 |
| 644 | O | Feature | [FAST-3.6] Workspaces that share a job | fast-ci,exec:sonnet,effort:high | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 1826 |
| 645 | O | Feature | [FAST-3.7] Checking out only what Rust needs | fast-ci,exec:sonnet,effort:medium | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 1745 |
| 646 | O | Feature | [FAST-3.8] Private dependencies: fetch with a short-lived token, then build offline | fast-ci,exec:opus,effort:high,needs-owner | FAST-3 Job shape |  | 578 | 0/0 |  | 2026-11-12 | 2026-11-16 | 3936 |
| 647 | C | Feature | [FAST-3.9] Feature configurations, run once when identical | fast-ci,exec:sonnet,effort:high,in-progress,blocked | FAST-3 Job shape |  | 578 | 0/0 | #995 | 2026-11-12 | 2026-11-16 | 2542 |
| 648 | O | Feature | [FAST-6.3] Projects receive new Velnor versions automatically | fast-ci,exec:opus,effort:high,needs-owner | FAST-6 Guard and adopt |  | 581 | 0/0 |  | 2026-11-12 | 2026-11-16 | 3339 |
| 649 | O | Task | [FAST-6.4] Adopt the new generator in the owner's Rust projects | fast-ci,exec:sonnet,effort:high,needs-owner | FAST-6 Guard and adopt |  | 581 | 0/0 |  | 2026-11-12 | 2026-11-16 | 2673 |
| 650 | C | Feature | [FAST-L.7] velnor-runner: native ARM containers | fast-ci,exec:opus,effort:medium | FAST-L Local GitHub |  | 627 | 0/0 |  | 2026-11-12 | 2026-11-16 | 1945 |
| 651 | C | Feature | [FAST-L.8] velnor-runner: a local mode, and the driver uses it | fast-ci,exec:opus,effort:medium,in-progress | FAST-L Local GitHub |  | 627 | 7/7 |  | 2026-11-12 | 2026-11-17 | 2200 |
| 652 | C | Task | [FAST-0.4] Lab experiment: does blaze earn a place in a generated workflow | fast-ci,exec:opus,effort:medium | FAST-0 Measure and decide |  | 575 | 0/0 |  | 2026-11-12 | 2026-11-14 | 5492 |
| 653 | C | Feature | [FAST-5.2] Dogfood: this repository's CI builds the helper from source | fast-ci,exec:sonnet,effort:high | FAST-5 Dogfood |  | 626 | 0/0 | #984 | 2026-11-12 | 2026-11-16 | 2905 |
| 662 | O |  | [OWNER] Run the Qualification workflow once on the scale set after #554 | owner-action |  |  | 574 | 0/0 |  | 2026-11-13 | 2026-11-13 | 1292 |
| 665 | C |  | [FAST-1.1b] velnor-gen reads and writes a repository: the io module, generate and check | fast-ci,exec:opus,effort:medium | FAST-1 Foundation |  | 580 | 0/0 | #668 | 2026-11-13 | 2026-11-14 | 2157 |
| 674 | C |  | [OWNER] Decide how the helper fetch step fits design rule 9 (one command per run step) | owner-action |  |  | 574 | 0/0 |  | 2026-11-13 | 2026-11-14 | 1199 |
| 680 | O |  | [OWNER] Approve the existing engine's exemption for the hand-written release workflow | owner-action |  |  | 574 | 0/0 |  | 2026-11-13 | 2026-11-14 | 1327 |
| 682 | O |  | [OWNER] Allow only admins to create rust-ci-v* tags | owner-action |  |  | 574 | 0/0 |  | 2026-11-13 | 2026-11-14 | 1305 |
| 685 | C |  | [OWNER] Provide a private repository for the velnor-runner arm64 proof run (#650) | owner-action |  |  | 574 | 0/0 |  | 2026-11-13 | 2026-11-14 | 2241 |
| 694 | C |  | [FAST] Follow the real edges of unowned path packages in select | fast-ci |  |  | 574 | 0/0 | #711 | 2026-11-14 | 2026-11-14 | 2350 |
| 697 | C |  | [FAST] Make velnor-ci inputs audit safe after a partial build and with unusual target layo | fast-ci |  |  | 574 | 0/0 | #714 | 2026-11-14 | 2026-11-14 | 2153 |
| 701 | C |  | [OWNER] Change docs/goals.md rule 2 so a Cargo.lock change selects only the packages whose | owner-action |  |  | 574 | 0/0 | #715 | 2026-11-14 | 2026-11-14 | 1753 |
| 703 | C |  | [FAST-2.3b] Select everything when an edit leaves Cargo.lock inconsistent with the manifes | fast-ci,exec:sonnet,effort:high |  |  | 574 | 0/0 | #709 | 2026-11-14 | 2026-11-14 | 1585 |
| 705 | C |  | [FAST-3.1b] `velnor-gen timings`: write the timings file from the last runs | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-3 Job shape |  | 574 | 0/0 | #766,#837 | 2026-11-14 | 2026-11-15 | 2033 |
| 707 | C |  | [OWNER] Decide the job-start check for velnor-runner's local mode (#651) | owner-action |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-14 | 1607 |
| 710 | C |  | [FAST-3.1c] `velnor-ci plan` reads no file beyond what the doc-step rule needs | fast-ci,exec:sonnet,effort:medium | FAST-3 Job shape |  | 574 | 0/0 | #762 | 2026-11-14 | 2026-11-15 | 1775 |
| 712 | C |  | [FAST-3.1d] A sound rule for leaving out the doctest and documentation steps | fast-ci,exec:opus,effort:high | FAST-3 Job shape |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 2175 |
| 713 | C |  | [FAST-3.4b] The generated job installs the whole declared toolchain profile | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-3 Job shape |  | 574 | 0/0 | #804,#837 | 2026-11-14 | 2026-11-15 | 1421 |
| 716 | C |  | [FAST] Make velnor-ci inputs audit check each build-script run, custom target specs and ho | fast-ci,exec:opus,effort:high |  |  | 574 | 0/0 | #758 | 2026-11-14 | 2026-11-15 | 3367 |
| 717 | C |  | [FAST-R.9] Install velnor-ci through a setup-velnor action, so rule 9 needs no exception | fast-ci,exec:sonnet,effort:high |  |  | 574 | 0/0 | #755,#836 | 2026-11-14 | 2026-11-15 | 2579 |
| 718 | C |  | [FAST-3.1e] Run custom test harnesses that nextest cannot list | fast-ci,exec:sonnet,effort:medium | FAST-3 Job shape |  | 574 | 0/0 | #767,#836 | 2026-11-14 | 2026-11-15 | 1428 |
| 719 | C |  | [FAST] Credit reads of a path package outside the workspaces by the package cargo's finger | fast-ci,exec:opus,effort:high |  |  | 574 | 0/0 | #735 | 2026-11-14 | 2026-11-14 | 2017 |
| 720 | C |  | [OWNER] Create the velnor-release App, the release environment and the lab tag ruleset (fo | owner-action |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-14 | 2918 |
| 721 | C |  | [FAST-REL.1] Prove App-only tag creation on velnor-lab | fast-ci,exec:opus,effort:medium |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-14 | 2219 |
| 722 | C |  | [FAST-REL.2] The engine leaves workflows that carry the velnor-gen header | fast-ci,exec:opus,effort:medium |  |  | 574 | 0/0 | #738,#836 | 2026-11-14 | 2026-11-15 | 1095 |
| 723 | C |  | [FAST-REL.3] velnor-gen owns every workflow with its header | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #772 | 2026-11-14 | 2026-11-15 | 881 |
| 724 | C |  | [FAST-REL.4] velnor-ci release-gate reads the required checks from the branch rules and bi | fast-ci,exec:opus,effort:high |  |  | 574 | 0/0 | #749 | 2026-11-14 | 2026-11-14 | 1410 |
| 725 | C |  | [FAST-REL.5] velnor-ci release record and release publish | fast-ci,exec:opus,effort:high |  |  | 574 | 0/0 | #756 | 2026-11-14 | 2026-11-14 | 1256 |
| 726 | C |  | [FAST-REL.6] Read release-plz.toml and .velnor/release.toml into a ReleasePlan | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #763,#836 | 2026-11-14 | 2026-11-15 | 1333 |
| 727 | C |  | [FAST-REL.7] Render release.yml: trigger, gate, release-pr, tag | fast-ci,exec:opus,effort:high |  |  | 574 | 0/0 | #810,#836 | 2026-11-14 | 2026-11-15 | 1130 |
| 728 | C |  | [FAST-REL.8] Render build, sums and publish, and add velnor-gen release ruleset | fast-ci,exec:opus,effort:high,in-progress |  |  | 574 | 0/0 | #815,#857 | 2026-11-14 | 2026-11-16 | 1208 |
| 729 | O |  | [FAST-REL.9] Prove the whole release flow on velnor-lab | fast-ci,exec:sonnet,effort:high,blocked |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 1811 |
| 730 | C |  | [OWNER] Create the seed tags velnor-ci-v0.1.79 and velnor-gen-v0.1.79 (for FAST-REL.10) | owner-action |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-18 | 1244 |
| 731 | O |  | [FAST-REL.10] Velnor's own release-plz config and per-package tags | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #1307 | 2026-11-14 | 2026-11-18 | 1618 |
| 732 | O |  | [OWNER] Switch the release-tags ruleset to the App-only bypass (for FAST-REL.11) | owner-action |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-14 | 1904 |
| 733 | O |  | [FAST-REL.11] Generate this repository's release.yml and delete the #676 exemption | fast-ci,exec:opus,effort:medium |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 1520 |
| 734 | C |  | [OWNER] Push the bootstrap tag rust-ci-v0.1.80 after R4 and R5 (for FAST-REL.9) | owner-action |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 819 |
| 736 | C |  | [FAST-L.9] velnor-host names a too-long control socket path instead of reporting a held lo | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #773,#837 | 2026-11-14 | 2026-11-15 | 1072 |
| 737 | C |  | [FAST-L.10] The resource probe works on a host whose mountinfo is larger than 64 KiB | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #757 | 2026-11-14 | 2026-11-15 | 1302 |
| 739 | C |  | [FAST-1.x] The generator's tests start no external program | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #771,#836 | 2026-11-14 | 2026-11-15 | 1263 |
| 740 | C |  | [FAST-3.1f] Documentation checks no crate twice and documents only what a change affects | fast-ci,exec:opus,effort:high |  |  | 578 | 0/0 |  | 2026-11-14 | 2026-11-16 | 2120 |
| 742 | C |  | [FAST-L.8.2] velnor-host: the local-mode feature, its release lock, and a pair core with n | fast-ci,exec:opus,effort:high,in-progress |  |  | 651 | 0/0 | #787,#857 | 2026-11-14 | 2026-11-16 | 2585 |
| 743 | C |  | [FAST-L.8.3] velnor-local: the local authority | fast-ci,exec:opus,effort:medium |  |  | 651 | 0/0 | #836 | 2026-11-14 | 2026-11-15 | 2531 |
| 744 | C |  | [FAST-L.8.4] velnor-host local: pairs from a plan, the local job-start check, replacement  | fast-ci,exec:opus,effort:high,in-progress |  |  | 651 | 0/0 | #821,#857 | 2026-11-14 | 2026-11-16 | 2875 |
| 745 | C |  | [FAST-L.8.5] velnor-local: --runner pair runs scenarios on velnor-host pairs | fast-ci,exec:opus,effort:medium,in-progress |  |  | 651 | 0/0 | #842,#857 | 2026-11-14 | 2026-11-16 | 2366 |
| 746 | C |  | [FAST-L.8.6] velnor-local: fork pairs, the refusal proofs and the optional --policy | fast-ci,exec:opus,effort:high |  |  | 651 | 0/0 | #999 | 2026-11-14 | 2026-11-16 | 2503 |
| 747 | C |  | [FAST-L.8.7] velnor-local: doctor, status and the README | fast-ci,exec:sonnet,effort:medium |  |  | 651 | 0/0 | #978 | 2026-11-14 | 2026-11-16 | 1870 |
| 748 | C |  | [FAST-L.8.1] velnor-local: build the pair images and mint a runner.server JIT | fast-ci,exec:opus,effort:medium |  |  | 651 | 0/0 | #759,#836 | 2026-11-14 | 2026-11-15 | 1863 |
| 750 | C |  | [FAST-3.1f] Trust boundary for project configuration that weakens generated checks | fast-ci,exec:opus,effort:high | FAST-3 Job shape |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 2192 |
| 751 | C |  | [FAST-L.11] velnor-host recovers a reservation after a crash during storage initialization | fast-ci,exec:opus,effort:high,in-progress |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 1884 |
| 752 | C |  | [FAST-L.12] velnor-host reports the real error when a pair's helper check fails, and does  | fast-ci,exec:sonnet,effort:high |  |  | 574 | 0/0 |  | 2026-11-14 | 2026-11-16 | 1079 |
| 753 | C |  | [FAST-L.13] velnor-host connect creates its state folder 0700, and completion diagnostics  | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #775,#857 | 2026-11-14 | 2026-11-16 | 1094 |
| 754 | C |  | [FAST-L.14] A finished pair leaves no private network, and completion cleanup proves it | fast-ci,exec:sonnet,effort:high,in-progress |  |  | 574 | 0/0 | #820,#837 | 2026-11-14 | 2026-11-15 | 1296 |
| 760 | C |  | [FAST] velnor-ci inputs audit honours cfg-gated build-dependencies | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #770,#837 | 2026-11-14 | 2026-11-15 | 1233 |
| 764 | C |  | [FAST] Test that setup-velnor refuses a binary with the wrong SHA-256 | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #783,#857 | 2026-11-15 | 2026-11-16 | 1037 |
| 768 | C |  | [FAST-3.1b.1] A stale or malformed timings file never stops root discovery | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #780,#837 | 2026-11-15 | 2026-11-15 | 744 |
| 769 | C |  | [FAST-3.1b.2] Timings use only samples from the current runner | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #837 | 2026-11-15 | 2026-11-15 | 666 |
| 777 | C |  | [FAST-3.5.1] The policy lane runs machete when a change can add an unused dependency | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #791,#837 | 2026-11-15 | 2026-11-15 | 943 |
| 779 | C |  | [FAST-L.13.1] Every velnor-host state folder is created 0700 | fast-ci,exec:sonnet,effort:low,in-progress |  |  | 574 | 0/0 | #784,#857 | 2026-11-15 | 2026-11-16 | 769 |
| 782 | C |  | [FAST-3.1f.1] velnor-ci error output cannot forge a workflow command | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #789,#837 | 2026-11-15 | 2026-11-15 | 960 |
| 785 | C |  | [FAST-L.12.1] Retire a pair that started but never bound and release its reservation | fast-ci,exec:opus,effort:high,in-progress |  |  | 574 | 0/0 | #803,#857 | 2026-11-15 | 2026-11-16 | 1879 |
| 786 | C |  | [FAST-3.1f.2] Weakening warnings: only checked roots, correct "new" label, nextest overrid | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #788,#837 | 2026-11-15 | 2026-11-15 | 1206 |
| 790 | C |  | [FAST-1.y] velnor-ci audit tests pass in parallel | fast-ci,exec:haiku,effort:low |  |  | 574 | 0/0 | #793 | 2026-11-15 | 2026-11-16 | 1116 |
| 794 | C |  | [FAST-1.y.1] Remove test temp folders left by killed tests | fast-ci,exec:haiku,effort:low,in-progress |  |  | 574 | 0/0 | #795,#837 | 2026-11-15 | 2026-11-15 | 947 |
| 796 | C |  | [FAST-REL.6.1] Build metadata in a release version is tagged or refused up front | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #797,#836 | 2026-11-15 | 2026-11-15 | 1111 |
| 799 | C |  | [FAST-3.4c] Wire every plan output into the generated steps | fast-ci,exec:sonnet,effort:high |  |  | 574 | 0/0 |  | 2026-11-15 | 2026-11-16 | 1235 |
| 800 | C |  | [FAST-L.12.2] Test that a long helper error is cut at the detail limit | fast-ci,exec:haiku,effort:low,in-progress |  |  | 574 | 0/0 | #805,#837 | 2026-11-15 | 2026-11-15 | 703 |
| 801 | C |  | [FAST-3.4d] Check the metadata file in CI and harden velnor-ci run | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #812,#837 | 2026-11-15 | 2026-11-15 | 1326 |
| 802 | C |  | [FAST-L.11.1] velnor-host prints recovery journal rows read-only | fast-ci,exec:sonnet,effort:medium |  |  | 574 | 0/0 | #836,#816,#936 | 2026-11-15 | 2026-11-15 | 999 |
| 806 | C |  | [FAST-1.z] Tests stay well under the 2 s slow-timeout | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #814,#837 | 2026-11-15 | 2026-11-15 | 1110 |
| 808 | C |  | [FAST-3.4e] The plan reads changed files from git, with no token | fast-ci,exec:sonnet,effort:medium,in-progress |  |  | 574 | 0/0 | #813,#837 | 2026-11-15 | 2026-11-15 | 1518 |
| 811 | C |  | [FAST-6.x] Document the local recipe for fixture-dependent tests | fast-ci,exec:haiku,effort:low |  |  | 574 | 0/0 | #958 | 2026-11-15 | 2026-11-16 | 1018 |
| 817 | C |  | [FAST-REL.8.1] Release config values cannot become Actions expressions | fast-ci,exec:haiku,effort:low,in-progress |  |  | 574 | 0/0 | #819,#857 | 2026-11-15 | 2026-11-16 | 1000 |
| 822 | C |  | [FAST-L.14.1] Observer and settling are tested on their safety paths and log what they ref | fast-ci,exec:sonnet,effort:medium,in-progress |  |  |  | 0/0 | #837 | 2026-11-15 | 2026-11-15 | 1561 |
| 827 | C |  | [FAST-2.5.1] Runs on the default branch are never cancelled by a later push | fast-ci,exec:haiku,effort:low,in-progress |  |  |  | 0/0 | #831,#837 | 2026-11-15 | 2026-11-15 | 979 |
| 829 | C |  | [CI] Hosted jobs run on ubuntu-24.04, which starts in seconds instead of hours | fast-ci,exec:sonnet,effort:medium,blocked |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-15 | 1195 |
| 830 | C |  | [FAST-L.12.1] Retiring an unbound pair removes its storage-initializer helper | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 | #964 | 2026-11-15 | 2026-11-16 | 852 |
| 832 | C |  | [FAST-L.8.3.1] The local authority binds each entry to the run it dispatched, never a look | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 2024 |
| 833 | C |  | [FAST-REL.7.1] Release config refuses credential providers, shared tags and an ambiguous m | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 1392 |
| 834 | C |  | [FAST-3.1.1] Custom-harness targets the generator cannot decide are never skipped silently | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #946 | 2026-11-15 | 2026-11-16 | 1065 |
| 838 | C |  | Cleanup observer: a recycled namespace inode can be mistaken for the destroyed pair's name | in-progress |  |  |  | 0/0 | #1021 | 2026-11-15 | 2026-11-16 | 1155 |
| 839 | C |  | [FAST-L.15] Non-blocking points from the review of Batch 2's constituent PRs | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 2454 |
| 840 | C |  | [FAST-6.0] Enforce the 400-line file cap and split the files over it | fast-ci,exec:haiku,effort:low,in-progress |  |  |  | 0/0 | #1049 | 2026-11-15 | 2026-11-17 | 891 |
| 841 | C |  | [FAST-L.8.4.1] Run the local-mode bouncer tests in required CI and report failed pair star | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #1010 | 2026-11-15 | 2026-11-16 | 1307 |
| 843 | C |  | [FAST-CI.1a] State producers run: adopt the helper release that authorizes producer jobs | fast-ci,exec:sonnet,effort:high,blocked |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 5341 |
| 844 | C |  | [FAST-CI.1b] Scale-set lanes hit the Cargo sources and Bun caches | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 6632 |
| 845 | C |  | [FAST-CI.1c] Scale-set lanes restore sealed MBX compiled state | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 6286 |
| 846 | C |  | [FAST-CI.1d] Verify the cache hits on a second run | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 2009 |
| 847 | C |  | [FAST-CI.2] Ledger CLI off the critical path: cached build, skipped when no lane runs | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 5494 |
| 848 | C |  | [FAST-CI.3] A pull request behind main gets an actionable stale-base answer and recovers b | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 4875 |
| 849 | C |  | [FAST-CI.4] Split check-maintenance-helpers so it is not the longest job | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 5229 |
| 850 | C |  | [FAST-CI.5] Install only the tools each lane needs and reuse a verified toolchain on the s | fast-ci,exec:sonnet,effort:high |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 7196 |
| 851 | C |  | [FAST-2.5.2] Multi-root repositories get one inputs-audit job that builds every root | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #946 | 2026-11-15 | 2026-11-16 | 1199 |
| 852 | C |  | Require the image kind label to match the role in the readiness audit | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #912 | 2026-11-15 | 2026-11-16 | 1272 |
| 853 | C |  | Verify every requested mise version in velnor-ci prepare | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #916 | 2026-11-15 | 2026-11-16 | 898 |
| 854 | C |  | Keep all-roots selection when a shared target source has unknown readers | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #908 | 2026-11-15 | 2026-11-16 | 1057 |
| 855 | C |  | Escape the default branch in the release workflow trigger | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #909 | 2026-11-15 | 2026-11-16 | 1250 |
| 856 | C |  | Reject Cargo compiler overrides in the plan instead of warning | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 1064 |
| 858 | C |  | [FAST-B3.1] Retire a removed connection's unbound pair when its remote poll fails | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #964 | 2026-11-15 | 2026-11-16 | 1036 |
| 859 | C |  | [FAST-B3.2] Schedule cleanup of removed connections' unbound pairs at daemon startup | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 1114 |
| 860 | C |  | [FAST-B3.3] Run unbound-pair retirement under the poll-turn deadline | fast-ci,exec:sonnet,effort:medium,blocked |  |  |  | 0/0 | #1027 | 2026-11-15 | 2026-11-17 | 1038 |
| 861 | C |  | [FAST-B3.4] Recover an agent minted by velnor-host local mode that has no labelled resourc | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 1606 |
| 862 | C |  | [FAST-B3.5] Retry a transient JIT mint failure in a local-mode slot instead of stopping ev | fast-ci,exec:sonnet,effort:medium,in-progress |  |  |  | 0/0 | #948 | 2026-11-15 | 2026-11-16 | 1102 |
| 863 | C |  | [FAST-B3.6] Propagate `docker port` errors when opening the untrusted backend | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 885 |
| 864 | C |  | [FAST-B3.7] Fail closed when the authority identity check cannot run `ps` | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 886 |
| 865 | C |  | [FAST-B3.8] Probe runner.server and the cache before pair registration in pair mode | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 1031 |
| 866 | C |  | [FAST-B3.9] Report pairs ready only from their current listening state | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 896 |
| 867 | C |  | [FAST-B3.10] Resolve the default velnor-host outside the checkout | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #942 | 2026-11-15 | 2026-11-16 | 974 |
| 868 | C |  | [FAST-B3.11] Fail a pair-mode run that lacks a job-start decision for an expected job | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #953 | 2026-11-15 | 2026-11-16 | 905 |
| 869 | C |  | [FAST-B3.12] Check the package inventory when a release root is a workspace member manifes | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 1039 |
| 870 | C |  | [FAST-B3.13] Recheck the default-branch tip before the tag job mints its token | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #918 | 2026-11-15 | 2026-11-16 | 1031 |
| 871 | C |  | [FAST-B3.14] Install release-plz from a repository-pinned digest before minting the releas | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #910 | 2026-11-15 | 2026-11-16 | 1090 |
| 872 | C |  | [FAST-B3.15] Reject a non-crates.io `[registry] default` in effective Cargo configs of pub | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #919 | 2026-11-15 | 2026-11-16 | 1074 |
| 873 | C |  | [FAST-B3.16] Require a release-plz version that works with `persist-credentials: false` | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #944 | 2026-11-15 | 2026-11-16 | 930 |
| 874 | C |  | [FAST-B3.17] Publish the draft GitHub release of packages that have no `[[binaries]]` entr | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #944 | 2026-11-15 | 2026-11-16 | 1027 |
| 875 | C |  | [FAST-B3.18] Compare the build checkout with the commit release-plz tagged | fast-ci,exec:sonnet,effort:medium,in-progress |  |  |  | 0/0 | #994 | 2026-11-15 | 2026-11-17 | 1046 |
| 876 | C |  | [FAST-B3.19] Reject a generated release workflow above GitHub's file size limit | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #944 | 2026-11-15 | 2026-11-16 | 846 |
| 877 | C |  | [FAST-B3.20] Honour or reject `publish_timeout` above the tag job timeout | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #944 | 2026-11-15 | 2026-11-16 | 896 |
| 878 | C |  | [FAST-B3.21] Validate workspace-inherited package names at generation | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #935 | 2026-11-15 | 2026-11-16 | 998 |
| 879 | C |  | [FAST-B3.22] Classify a root as crates.io publishing only when some package inherits the w | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #935 | 2026-11-15 | 2026-11-16 | 915 |
| 880 | C |  | [FAST-B3.23] Refuse or safely render release roots that begin with a dash | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #937 | 2026-11-15 | 2026-11-16 | 914 |
| 881 | C |  | [FAST-B3.24] Reject `[net] offline = true` for crates.io publishing roots | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 789 |
| 882 | C |  | [FAST-B3.25] Limit the toolchain-file check to files that can select the toolchain | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 895 |
| 883 | C |  | [FAST-B3.26] Make `velnor-gen release ruleset` output match the documented gh api command | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 920 |
| 884 | C |  | [FAST-B3.27] Compare tag templates of unlisted workspace members within a root | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #935 | 2026-11-15 | 2026-11-16 | 930 |
| 885 | C |  | [FAST-B3.28] Parse workflow names as YAML scalars when resolving `after` | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 777 |
| 886 | C |  | [FAST-B3.29] Resolve `[[binaries]]` for workspace-inherited packages | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #935 | 2026-11-15 | 2026-11-16 | 811 |
| 887 | C |  | [FAST-B3.30] Revalidate the whole release policy at runtime before minting the App token | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #920 | 2026-11-15 | 2026-11-16 | 1069 |
| 888 | C |  | [FAST-B3.31] Own `actions/setup-velnor/action.yml` as a generator input | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 866 |
| 889 | C |  | [FAST-B3.32] Exclude packages with `release = false` from the inherited inventory | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 821 |
| 890 | O |  | [FAST-B3.33] Make each release root retryable in the tag job | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #944,#1313 | 2026-11-15 | 2026-11-16 | 839 |
| 891 | C |  | [FAST-B3.34] Scope the crates.io redirect check to the roots a Cargo config reaches | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 791 |
| 892 | C |  | [FAST-B3.35] Compare complete tag templates for collisions | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #937 | 2026-11-15 | 2026-11-16 | 750 |
| 893 | C |  | [FAST-B3.36] Bound the velnor-ci download in setup-velnor by the 256 MiB limit | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 881 |
| 894 | C |  | [FAST-B3.37] Read `.cargo` text only from `config` and `config.toml` in check-config | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 836 |
| 895 | C |  | [FAST-B3.38] Quote reserved YAML scalars used as job output keys | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 842 |
| 896 | C |  | [FAST-B3.39] Reject tag templates that can end a ref component in `.lock` | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #972 | 2026-11-15 | 2026-11-16 | 812 |
| 897 | C |  | [FAST-B3.40] Apply crates.io name rules to crates.io publishing packages | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #937 | 2026-11-15 | 2026-11-16 | 807 |
| 898 | C |  | [FAST-B3.41] Carry the repository's Rust toolchain into the release jobs or reject nightly | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #950 | 2026-11-15 | 2026-11-16 | 844 |
| 899 | C |  | [FAST-B3.42] Validate that `after` covers every required workflow | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #951 | 2026-11-15 | 2026-11-16 | 926 |
| 900 | C |  | [FAST-B3.43] Compare the suffixes around the version placeholder before reporting a tag co | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #937 | 2026-11-15 | 2026-11-16 | 942 |
| 901 | C |  | [FAST-B3.44] Reject conflicting release-plz version selectors in mise.toml | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 787 |
| 902 | C |  | [FAST-B3.45] Cancel the JIT mint when velnor-host shutdown is requested | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-15 | 2026-11-16 | 1015 |
| 903 | C |  | [FAST-B3.46] Give a fork pull request's head repository its own id | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #953 | 2026-11-15 | 2026-11-16 | 845 |
| 904 | C |  | [FAST-B3.47] Carry each root's publishing mode into the runtime Cargo config check | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #938 | 2026-11-15 | 2026-11-16 | 1108 |
| 905 | C |  | [FAST-B3.1] Local runner plan rejects one network shared by the trusted and untrusted side | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #953 | 2026-11-16 | 2026-11-16 | 669 |
| 906 | C |  | [FAST-B3.2] Local runner keeps the agent identity until its retirement succeeds | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #953 | 2026-11-16 | 2026-11-16 | 680 |
| 907 | C |  | [FAST-B3.3] Private state and log directories are tightened when they already exist | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #953 | 2026-11-16 | 2026-11-16 | 627 |
| 913 | C |  | Bind the locked release-plz URL to the configured version and refuse the ubi backend by na | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #921 | 2026-11-16 | 2026-11-16 | 1179 |
| 914 | C |  | [FAST-2.5.3] Readiness audit refuses a missing or mismatched image kind through a fake Doc | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #976 | 2026-11-16 | 2026-11-16 | 1309 |
| 915 | C |  | [FAST-2.5.4] Readiness audit compares the container image ID with the admitted runtime ima | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #945 | 2026-11-16 | 2026-11-16 | 1260 |
| 922 | C |  | Rerun plan-wide release rules (shared tags, overlapping PR branch prefixes) at runtime bef | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #927 | 2026-11-16 | 2026-11-16 | 655 |
| 923 | C |  | Bind runtime release tag patterns to the patterns the tag rulesets were provisioned for | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #925 | 2026-11-16 | 2026-11-16 | 679 |
| 926 | C |  | Recheck generator-only release rules on the commit being released | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #932 | 2026-11-16 | 2026-11-16 | 1008 |
| 928 | C |  | Make the JIT config file write finish before it is renamed into place | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #929 | 2026-11-16 | 2026-11-16 | 941 |
| 930 | C |  | Don't rebuild archive guard or drop compiled state for freshness-only changes | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #970 | 2026-11-16 | 2026-11-16 | 755 |
| 931 | O |  | Detect bypass actors added to the tag immutability rulesets | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1090 |
| 934 | O |  | Require velnor-ci 0.1.80+ for release generation once it is released | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-17 | 818 |
| 939 | C |  | Runtime check-config uses the same crates.io publishing classification as generation | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #972 | 2026-11-16 | 2026-11-16 | 685 |
| 941 | O |  | CI speed: generator follow-ups from consumer repos | in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 9163 |
| 943 | C |  | Pin the tag-collision boundary cases in velnor-release tag tests | fast-ci,exec:haiku,effort:low |  |  |  | 0/0 | #1013 | 2026-11-16 | 2026-11-16 | 456 |
| 949 | O |  | [OWNER] Approve an admin-read secret for the daily tag-ruleset drift check (#931) | needs-owner,owner-action |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 915 |
| 952 | C |  | release hazards: honor .cargo/config over .cargo/config.toml precedence for net.offline | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #972 | 2026-11-16 | 2026-11-16 | 389 |
| 954 | C |  | Scope a CI configuration to named packages so local-mode tests run in required CI | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #962 | 2026-11-16 | 2026-11-16 | 595 |
| 955 | C |  | [FAST-2.5.5] Run the inputs audit after cargo doc in the one-job shape | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1379 |
| 956 | C |  | [FAST-3.4d] velnor-ci run signals the whole command, not only its first process | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #957 | 2026-11-16 | 2026-11-16 | 1053 |
| 959 | C |  | Compare inherited workspace tags across release roots | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #972 | 2026-11-16 | 2026-11-16 | 593 |
| 960 | C |  | Two fixture-dependent tests fail locally on macOS with the documented recipe | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #968 | 2026-11-16 | 2026-11-16 | 759 |
| 963 | C |  | velnor-host keeps its session credentials after a held poll | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #991 | 2026-11-16 | 2026-11-16 | 626 |
| 965 | C |  | Run cargo fmt once per workspace, not once per configuration | fast-ci,exec:sonnet,effort:low,in-progress |  |  |  | 0/0 | #969 | 2026-11-16 | 2026-11-16 | 469 |
| 966 | O |  | Wire the in-cluster crates.io mirror explicitly in velnor-gen config | fast-ci,exec:sonnet,effort:medium,blocked |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-17 | 852 |
| 967 | C |  | Adopt the scoped local-mode configuration in velnor-new's own config once velnor-actions w | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #990 | 2026-11-16 | 2026-11-16 | 762 |
| 971 | C |  | Check whether a cross-target configuration passes --target to cargo fmt | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 440 |
| 973 | C |  | capture-opentofu-goldens.sh check also verifies MANIFEST.sha256 | fast-ci,exec:haiku,effort:low |  |  |  | 0/0 | #993 | 2026-11-16 | 2026-11-16 | 496 |
| 974 | C |  | Make the RUSTC_WRAPPER marker test in velnor-ci able to fail | fast-ci,exec:haiku,effort:low |  |  |  | 0/0 | #993 | 2026-11-16 | 2026-11-16 | 636 |
| 975 | C |  | Close the local-mode mint test gaps left after #948 | fast-ci,exec:sonnet,effort:low |  |  |  | 0/0 | #993 | 2026-11-16 | 2026-11-16 | 530 |
| 980 | C |  | Doctest detection counts a README that rustdoc does not read | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #988 | 2026-11-16 | 2026-11-16 | 773 |
| 982 | C |  | Cache key inputs: config shadowing, ignored cargo configs, build-dir placeholders | fast-ci,exec:sonnet,effort:low,in-progress |  |  |  | 0/0 | #1020 | 2026-11-16 | 2026-11-17 | 784 |
| 983 | C |  | velnor-ci plan reports .velnor/rust-ci.lock as an unknown path | fast-ci,exec:haiku,effort:low |  |  |  | 0/0 | #989 | 2026-11-16 | 2026-11-16 | 435 |
| 987 | O |  | Recover assetless-release outputs after a failed multi-root tag run | fast-ci,exec:sonnet,effort:medium |  |  |  | 0/0 | #997 | 2026-11-16 | 2026-11-18 | 728 |
| 996 | C |  | Allow explicit PR-only required checks in release-gate | in-progress |  |  |  | 0/0 | #1006 | 2026-11-16 | 2026-11-16 | 741 |
| 1000 | C |  | Drive poll_connection in a test so its call sites are covered | fast-ci,in-progress |  |  |  | 0/0 | #1030 | 2026-11-16 | 2026-11-16 | 662 |
| 1001 | C |  | Release generation: .lock-version check ignores release = false; unparsable manifests are  | fast-ci |  |  |  | 0/0 | #1014 | 2026-11-16 | 2026-11-16 | 927 |
| 1004 | C |  | Run the native image inventory check in PR CI and the Required gate | in-progress |  |  |  | 0/0 | #1023 | 2026-11-16 | 2026-11-16 | 606 |
| 1005 | O |  | A cancelled run's reservation stays reserved: completion cleanup fails at volume remove | fast-ci,in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-17 | 992 |
| 1007 | O |  | Require velnor-ci 0.1.81+ for generation (release and the Required gate) once it is releas | fast-ci,in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1057 |
| 1011 | O |  | inputs save/restore ignore a custom build.target-dir | fast-ci,in-progress |  |  | 574 | 0/0 |  | 2026-11-16 | 2026-11-17 | 664 |
| 1015 | C |  | Required gate: needs JSON can exceed the 128 KiB env limit on very large plans | fast-ci,in-progress |  |  |  | 0/0 | #1040 | 2026-11-16 | 2026-11-17 | 786 |
| 1016 | O |  | Release preflight: test the 100-PR refusal; drop the unreachable identical arm | fast-ci,in-progress |  |  |  | 0/0 | #1022,#1312 | 2026-11-16 | 2026-11-16 | 471 |
| 1018 | O |  | inputs snapshot scope misses tracked path dependencies outside a nested root | fast-ci,in-progress |  |  | 574 | 0/0 |  | 2026-11-16 | 2026-11-17 | 1124 |
| 1028 | O |  | Source qualification guard mutation can fail with Text file busy |  |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1130 |
| 1029 | C |  | Local pair CI fixture fails with missing Docker image digest | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1472 |
| 1031 | C |  | Required gate: check test execution per selected package, not per root | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-17 | 495 |
| 1032 | C |  | Verify PR-only release exemptions against external checks and rule bypass | in-progress |  |  | 574 | 0/0 | #1080 | 2026-11-16 | 2026-11-17 | 1682 |
| 1033 | O |  | OpenTofu: emit native task records for consumer group ledgers | in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2277 |
| 1034 | O |  | Consumer freeze triage: repair non-Rust generation/cache/adoption blockers | in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 7412 |
| 1036 | C |  | [FAST-4.7] Helper report telemetry and pure cache waste evaluation for #612 |  |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-18 | 715 |
| 1038 | C |  | Tag-ruleset drift check: pin the expected bypass list per ruleset, including release-tags | fast-ci,in-progress |  |  |  | 0/0 | #1044 | 2026-11-16 | 2026-11-16 | 584 |
| 1039 | C |  | Real-Docker local-mode tests must run only against an explicit, dedicated engine | fast-ci,in-progress |  |  |  | 0/0 | #1048 | 2026-11-16 | 2026-11-17 | 1010 |
| 1041 | C |  | [FAST-4.7] Collect Cargo artifact identities for report compilation accounting |  |  |  |  | 0/0 | #1051 | 2026-11-16 | 2026-11-18 | 1033 |
| 1043 | C |  | Local-mode test: network removed while the dind endpoint is still attached | fast-ci,in-progress |  |  |  | 0/0 | #1048 | 2026-11-16 | 2026-11-17 | 754 |
| 1047 | O |  | Docs drift: statements in the contracts that no longer match the code | fast-ci,in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 7631 |
| 1054 | C |  | A crash after a JIT claim commits leaves the reservation claimed and blocks the connection | fast-ci,in-progress |  |  |  | 0/0 | #1191 | 2026-11-16 | 2026-11-17 | 5021 |
| 1055 | C |  | A crash during bootstrap create or start leaves requested rows that cleanup never claims | fast-ci,in-progress,blocked |  |  |  | 0/0 | #1216 | 2026-11-16 | 2026-11-17 | 6489 |
| 1057 | O |  | A crate already on crates.io with no git tag is a green release | fast-ci |  |  |  | 0/0 | #1314 | 2026-11-16 | 2026-11-16 | 3034 |
| 1058 | C |  | A credential restore that outlives its 10s deadline leaves the session refresh claimed | fast-ci,in-progress |  |  |  | 0/0 | #1111 | 2026-11-16 | 2026-11-17 | 2935 |
| 1059 | O |  | velnor-gen release ruleset accepts a .lock version that generate refuses | fast-ci |  |  |  | 0/0 | #1236 | 2026-11-16 | 2026-11-16 | 3150 |
| 1060 | O |  | A changed file a crate embeds can skip that crate when the inputs lock omits the read | fast-ci |  |  |  | 0/0 | #1355 | 2026-11-16 | 2026-11-16 | 5512 |
| 1061 | C |  | A push workflow with no name is invisible to the release after check | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2160 |
| 1062 | C |  | Dockerfile analysis skips an unpinned FROM hidden by a false heredoc | fast-ci |  |  |  | 0/0 | #1251 | 2026-11-16 | 2026-11-18 | 2335 |
| 1063 | C |  | Image selection misses COPY and ADD paths expanded from an ARG | fast-ci,in-progress |  |  |  | 0/0 | #1199 | 2026-11-16 | 2026-11-17 | 2129 |
| 1064 | C |  | Runner image publish checks out the repo with contents write | fast-ci,in-progress |  |  |  | 0/0 | #1167 | 2026-11-16 | 2026-11-17 | 1910 |
| 1065 | C |  | Release builds keep an actions:write token while repository code runs | fast-ci,in-progress |  |  |  | 0/0 | #1160 | 2026-11-16 | 2026-11-17 | 4040 |
| 1066 | O |  | Qualification dispatch succeeds when every job is skipped | fast-ci,in-progress |  |  |  | 0/0 | #1169 | 2026-11-16 | 2026-11-17 | 2191 |
| 1067 | C |  | Same-repo job code can delete every local cache | fast-ci |  |  |  | 0/0 | #1208 | 2026-11-16 | 2026-11-17 | 3580 |
| 1068 | C |  | Pull request code decides the required check when helper.path is set | fast-ci,in-progress |  |  |  | 0/0 | #1109 | 2026-11-16 | 2026-11-17 | 2968 |
| 1069 | C |  | Base cache: report failed saves and skip saves after a failed producer | fast-ci,in-progress |  |  | 574 | 0/0 | #1271 | 2026-11-16 | 2026-11-18 | 2150 |
| 1070 | C |  | Plan accepts a mise template that the install step executes | fast-ci |  |  |  | 0/0 | #1193,#1243 | 2026-11-16 | 2026-11-18 | 5124 |
| 1071 | C |  | The from-source helper compiles when no job needs the binary | fast-ci |  |  | 574 | 0/0 | #1189 | 2026-11-16 | 2026-11-17 | 2313 |
| 1072 | O |  | A one-root policy job repeats the plan | fast-ci |  |  | 574 | 0/0 | #1189,#1262 | 2026-11-16 | 2026-11-17 | 2996 |
| 1073 | O |  | Scheduled inputs audit builds every workspace again | fast-ci,in-progress |  |  | 574 | 0/0 | #1320 | 2026-11-16 | 2026-11-18 | 4007 |
| 1074 | O |  | Post-JIT retirement fences allow a runner to reach started | fast-ci |  |  |  | 0/0 | #1283 | 2026-11-16 | 2026-11-16 | 1963 |
| 1075 | O |  | Unbound retirement records containers absent while they are still started | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 1969 |
| 1076 | O |  | Pre-JIT cleanup progress advances while bootstrap containers still exist | fast-ci |  |  |  | 0/0 | #1280 | 2026-11-16 | 2026-11-16 | 1595 |
| 1077 | O |  | Execution proof source can be replaced because the guard uses != | fast-ci |  |  |  | 0/0 | #1277 | 2026-11-16 | 2026-11-16 | 2897 |
| 1078 | O |  | An uncertain Doctor probe whose child is aborted cannot open | fast-ci,in-progress |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-18 | 2063 |
| 1079 | C |  | connection_runtime_bindings CHECK accepts a partial scale-set or engine triple | fast-ci |  |  |  | 0/0 | #1252 | 2026-11-16 | 2026-11-18 | 2178 |
| 1081 | O |  | cargo-deny is skipped unless the root file is exactly deny.toml | fast-ci |  |  |  | 0/0 | #1194 | 2026-11-16 | 2026-11-17 | 1875 |
| 1082 | C |  | build.target check rejects host-tuple and configs cargo does not read | fast-ci,in-progress |  |  |  | 0/0 | #1163 | 2026-11-16 | 2026-11-17 | 2276 |
| 1083 | C |  | a string rustflags @file argument skips the cap-lints refusal | fast-ci |  |  |  | 0/0 | #1197 | 2026-11-16 | 2026-11-17 | 1454 |
| 1084 | C |  | an absolute [patch] path is recorded as a path inside the repository | fast-ci,in-progress |  |  |  | 0/0 | #1198 | 2026-11-16 | 2026-11-17 | 1631 |
| 1085 | C |  | a toml name that starts with mise or rtx is refused as a mise config | fast-ci,in-progress |  |  |  | 0/0 | #1163 | 2026-11-16 | 2026-11-17 | 1206 |
| 1086 | C |  | repo_profile drops a profile that cargo loads through include | fast-ci,in-progress |  |  |  | 0/0 | #1164 | 2026-11-16 | 2026-11-17 | 1551 |
| 1087 | C |  | Required check is skipped on cancel, and GitHub counts that skip as success | fast-ci |  |  |  | 0/0 | #1257 | 2026-11-16 | 2026-11-18 | 2905 |
| 1088 | C |  | velnor-ci run drops a nextest N/M summary and can publish the other stream's count | fast-ci,in-progress |  |  |  | 0/0 | #1166 | 2026-11-16 | 2026-11-17 | 1819 |
| 1089 | C |  | --require-doctests treats ignored doctests as tests that ran | fast-ci,in-progress |  |  |  | 0/0 | #1166 | 2026-11-16 | 2026-11-17 | 1255 |
| 1090 | C |  | velnor-ci timings counts an empty pull_requests array as the default branch | fast-ci,in-progress |  |  |  | 0/0 | #1180 | 2026-11-16 | 2026-11-17 | 1778 |
| 1091 | C |  | velnor-ci run grace path loses a parsed count and can accept a partial doctest line | fast-ci,in-progress |  |  |  | 0/0 | #1166 | 2026-11-16 | 2026-11-17 | 2039 |
| 1092 | C |  | velnor-ci run ignores a failed console write and can block on a FIFO log | fast-ci,in-progress |  |  |  | 0/0 | #1166 | 2026-11-16 | 2026-11-17 | 1629 |
| 1093 | C |  | velnor-ci timings misses group-job test steps and stores a skipped step as zero | fast-ci,in-progress |  |  |  | 0/0 | #1180 | 2026-11-16 | 2026-11-17 | 1981 |
| 1094 | C |  | record-sample corrupts cargo metadata when the checkout path is a prefix of another path | fast-ci,in-progress |  |  |  | 0/0 | #1181 | 2026-11-16 | 2026-11-17 | 1100 |
| 1095 | C |  | record-sample deletes a sample before the new files are written | fast-ci,in-progress |  |  |  | 0/0 | #1181 | 2026-11-16 | 2026-11-17 | 860 |
| 1096 | C |  | A non-regular build.rs skips the silent build-script note | fast-ci,in-progress |  |  |  | 0/0 | #1190 | 2026-11-16 | 2026-11-17 | 832 |
| 1097 | C |  | Release generation misses a push workflow whose name is not a name: line | fast-ci,in-progress |  |  |  | 0/0 | #1162 | 2026-11-16 | 2026-11-17 | 1448 |
| 1098 | C |  | Golden samples drop symlinks and UPDATE_GOLDENS=0 rewrites expected files | fast-ci,in-progress |  |  |  | 0/0 | #1165 | 2026-11-16 | 2026-11-17 | 1436 |
| 1099 | C |  | Preset full-run cron is hashed from the repository basename | fast-ci,in-progress |  |  |  | 0/0 | #1211 | 2026-11-16 | 2026-11-17 | 988 |
| 1100 | C |  | inputs audit treats a missing dep-info input or a file inside a rerun-if-changed directory | fast-ci,in-progress |  |  | 574 | 0/0 | #1192 | 2026-11-16 | 2026-11-17 | 2989 |
| 1101 | C |  | inputs audit writes the lock when a feature-gated unit's dep-info is stale | fast-ci,in-progress |  |  | 574 | 0/0 | #1195 | 2026-11-16 | 2026-11-17 | 2353 |
| 1102 | C |  | inputs audit walks the target directory with no depth or file-size bound | fast-ci,in-progress |  |  | 574 | 0/0 | #1179 | 2026-11-16 | 2026-11-17 | 1962 |
| 1103 | C |  | plan prints a rejected git path to stderr as a workflow command | fast-ci,in-progress |  |  | 574 | 0/0 | #1196 | 2026-11-16 | 2026-11-17 | 2025 |
| 1104 | O |  | prepare's pinned-tool check mismatches mise OS filters and lock rows | fast-ci,in-progress |  |  | 574 | 0/0 | #1200 | 2026-11-16 | 2026-11-17 | 4832 |
| 1105 | C |  | complete-profile components are installed after the tools cache is saved | fast-ci |  |  | 574 | 0/0 | #1258 | 2026-11-16 | 2026-11-18 | 2155 |
| 1106 | C |  | Base cache: gate saves on the plan's private-source result | fast-ci |  |  | 574 | 0/0 |  | 2026-11-16 | 2026-11-17 | 1413 |
| 1108 | O |  | A failed storage-initializer handoff leaves the helper running and the reservation reserve | fast-ci |  |  |  | 0/0 | #1361 | 2026-11-16 | 2026-11-16 | 5380 |
| 1110 | O |  | A crash after scale-set create leaves a matching set unadopted | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2447 |
| 1112 | C |  | Mac qualification receipt accepts / and noncanonical Docker roots | fast-ci |  |  |  | 0/0 | #1244 | 2026-11-16 | 2026-11-18 | 2443 |
| 1113 | O |  | velnor-host disconnect leaves a scale set this controller created | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-18 | 1604 |
| 1114 | O |  | velnor-host uses the process cwd when HOME is missing or relative | fast-ci |  |  |  | 0/0 | #1249 | 2026-11-16 | 2026-11-16 | 2015 |
| 1115 | C |  | Repeating velnor-host connect with a job-authority file does not store the new credential | fast-ci |  |  |  | 0/0 | #1264 | 2026-11-16 | 2026-11-18 | 1880 |
| 1116 | C |  | velnor-host service install accepts euid 0 and plants a RunAtLoad agent | fast-ci |  |  |  | 0/0 | #1248 | 2026-11-16 | 2026-11-18 | 1688 |
| 1117 | O |  | Removing daemon.lock lets a second velnor-host daemon open a session | fast-ci |  |  |  | 0/0 | #1263 | 2026-11-16 | 2026-11-16 | 1823 |
| 1118 | O |  | A scale-set create that did not create a set is never retried | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 3285 |
| 1119 | O |  | Scale-set create records the POST body without a re-read | fast-ci |  |  |  | 0/0 | #1402 | 2026-11-16 | 2026-11-18 | 2680 |
| 1120 | O |  | A scale-set response with a different runnerGroupId is accepted | fast-ci |  |  |  | 0/0 | #1260 | 2026-11-16 | 2026-11-16 | 2264 |
| 1121 | O |  | Job-authority inventory drops schedule and merge_group tag filters and authorizes every br | fast-ci |  |  |  | 0/0 | #1347 | 2026-11-16 | 2026-11-16 | 2977 |
| 1122 | C |  | Connection id "." is accepted and disconnect cannot drain it | fast-ci |  |  |  | 0/0 | #1267 | 2026-11-16 | 2026-11-18 | 2043 |
| 1123 | O |  | Qualification probe bind-mounts a root or noncanonical Docker directory | fast-ci |  |  |  | 0/0 | #1269 | 2026-11-16 | 2026-11-16 | 3068 |
| 1124 | O |  | Uncertain qualification create leaves the owned container in place | fast-ci |  |  |  | 0/0 | #1359 | 2026-11-16 | 2026-11-16 | 3078 |
| 1125 | O |  | Qualification accepts a log whose multiplexed frame header has no payload | fast-ci |  |  |  | 0/0 | #1346 | 2026-11-16 | 2026-11-16 | 2520 |
| 1126 | C |  | Qualification cleanup treats a renamed container as already removed | fast-ci |  |  |  | 0/0 | #1270 | 2026-11-16 | 2026-11-18 | 2833 |
| 1127 | O |  | Qualification volume cleanup deletes a volume that reused the name | fast-ci |  |  |  | 0/0 | #1272 | 2026-11-16 | 2026-11-16 | 2354 |
| 1128 | O |  | Release qualification connects to a different Docker socket when the path contains unix:// | fast-ci |  |  | 574 | 0/0 | #1273 | 2026-11-16 | 2026-11-16 | 2311 |
| 1129 | C |  | Qualification receipt is issued when a tool container has extra capabilities, devices, or  | fast-ci |  |  | 574 | 0/0 | #1278 | 2026-11-16 | 2026-11-18 | 2901 |
| 1130 | C |  | Readiness scan treats a journal as in-bounds when launch.db-wal exceeds the file cap | fast-ci |  |  | 574 | 0/0 | #1276 | 2026-11-16 | 2026-11-18 | 2816 |
| 1131 | C |  | Readiness scan stops at the first occupying intent and skips the row cap | fast-ci |  |  | 574 | 0/0 | #1253 | 2026-11-16 | 2026-11-18 | 2209 |
| 1132 | O |  | Readiness scan counts the legacy intents table and misses live connection_intents | fast-ci |  |  | 574 | 0/0 | #1279 | 2026-11-16 | 2026-11-16 | 2589 |
| 1133 | C |  | Readiness Docker /info body is not size-capped | fast-ci |  |  | 574 | 0/0 | #1281 | 2026-11-16 | 2026-11-18 | 2371 |
| 1134 | C |  | A failed poll deletes the connection setup receipt | fast-ci |  |  |  | 0/0 | #1285 | 2026-11-16 | 2026-11-18 | 2377 |
| 1135 | O |  | A zero scale-set assign time other than one sentinel changes the synthetic request id | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2048 |
| 1136 | C |  | An Actions service URL with userinfo is accepted and receives the admin bearer | fast-ci |  |  |  | 0/0 | #1245 | 2026-11-16 | 2026-11-18 | 1665 |
| 1137 | C |  | Hook source comparison accepts an event ref that is not the run branch | fast-ci |  |  |  | 0/0 | #1282 | 2026-11-16 | 2026-11-18 | 2414 |
| 1138 | C |  | A legal connection cannot be drained while the control socket is up | fast-ci |  |  |  | 0/0 | #1284 | 2026-11-16 | 2026-11-18 | 2316 |
| 1139 | C |  | Doctor reports ready while the host drain marker is set | fast-ci |  |  |  | 0/0 | #1255 | 2026-11-16 | 2026-11-18 | 1770 |
| 1140 | O |  | On macOS the control socket answers before the request is written | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2077 |
| 1141 | C |  | A second observation start drops a completed session poll | fast-ci |  |  |  | 0/0 | #1254 | 2026-11-16 | 2026-11-18 | 2067 |
| 1142 | O |  | Capacity::reserve replaces a live worker slot when another intent fits under max | fast-ci |  |  |  | 0/0 | #1390 | 2026-11-16 | 2026-11-16 | 2681 |
| 1143 | O |  | CleanupProof::covers accepts a short container id and an empty volume | fast-ci |  |  |  | 0/0 | #1275 | 2026-11-16 | 2026-11-16 | 2161 |
| 1144 | O |  | WorkerEvent::Acquired does not record acquire success | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2021 |
| 1145 | O |  | OwnershipMismatch drops the owned container and volume ids | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 2356 |
| 1146 | O |  | A per-file release attestation never admits the runner release | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-18 | 3294 |
| 1147 | C |  | A per-file release attestation never admits the runner release | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-16 | 3294 |
| 1148 | O |  | A second valid release attestation leaves the runner release unusable | fast-ci |  |  |  | 0/0 |  | 2026-11-16 | 2026-11-18 | 2494 |
| 1149 | C |  | Cleanup observer misses a live mount-namespace file descriptor | fast-ci |  |  |  | 0/0 | #1286 | 2026-11-16 | 2026-11-18 | 2555 |
| 1150 | O |  | Cleanup scan drops an owned mount when the walked inode differs | fast-ci |  |  |  | 0/0 | #1287 | 2026-11-16 | 2026-11-16 | 2869 |
| 1151 | C |  | Cleanup observation reports a cgroup path absent after the proving walks | fast-ci |  |  |  | 0/0 | #1250 | 2026-11-16 | 2026-11-18 | 1911 |
| 1152 | O |  | Btrfs publication skips the bound subvolume device and still writes | fast-ci |  |  |  | 0/0 | #1389 | 2026-11-16 | 2026-11-16 | 2271 |
| 1153 | O |  | Unlinked dockerd is dropped and a different dockerd file is bound | fast-ci |  |  |  | 0/0 | #1288 | 2026-11-16 | 2026-11-18 | 2543 |
| 1154 | C |  | A replaced admission lock still admits a second launch | fast-ci |  |  |  | 0/0 | #1242 | 2026-11-16 | 2026-11-18 | 2353 |
| 1155 | O |  | A replaced lineage lock still verifies the journal | fast-ci |  |  |  | 0/0 | #1289 | 2026-11-16 | 2026-11-16 | 2465 |
| 1156 | C |  | A published anchor revision is rejected after directory sync fails | fast-ci |  |  |  | 0/0 | #1291 | 2026-11-16 | 2026-11-18 | 2158 |
| 1157 | O |  | A leftover anchor temp file blocks the first lineage bind | fast-ci |  |  |  | 0/0 | #1294 | 2026-11-16 | 2026-11-16 | 2279 |
| 1159 | O |  | prepare: compare target-dir with the path recorded at generation | fast-ci |  |  |  | 0/0 | #1321 | 2026-11-17 | 2026-11-17 | 1021 |
| 1161 | O |  | fast-ci: list tests without a second discovery pass; lift the 32 KiB package-map cap | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-18 | 911 |
| 1170 | O |  | Local plain mode reports ready when runner.server explicitly says the runner is offline | fast-ci |  |  |  | 0/0 | #1388 | 2026-11-17 | 2026-11-17 | 1888 |
| 1171 | O |  | Restored reports a cold build after target discovery fails and leaves nested caches intact | fast-ci |  |  |  | 0/0 | #1392 | 2026-11-17 | 2026-11-17 | 2669 |
| 1172 | O |  | Automatic planning can skip Rust checks when tracked files differ from the event commit | fast-ci |  |  |  | 0/0 | #1353 | 2026-11-17 | 2026-11-17 | 2817 |
| 1173 | O |  | Bench collect accepts an incomplete jobs response as complete metrics | fast-ci |  |  |  | 0/0 | #1235 | 2026-11-17 | 2026-11-17 | 2603 |
| 1174 | O |  | Local shutdown erases authority ownership records after an invalid PID read | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-17 | 2520 |
| 1175 | O |  | Local cache authorization accepts signed runtime tokens before their not-before time | fast-ci |  |  |  | 0/0 | #1384 | 2026-11-17 | 2026-11-17 | 2031 |
| 1176 | O |  | Restored reports a cold build after failed discovery leaves nested caches intact | fast-ci |  |  |  | 0/0 | #1392 | 2026-11-17 | 2026-11-17 | 3004 |
| 1177 | O |  | Local authority reports deletion proved when the verification inventory is malformed | fast-ci |  |  |  | 0/0 | #1343 | 2026-11-17 | 2026-11-17 | 2202 |
| 1178 | O |  | A forged Velnor marker lets generation delete an unrelated workflow | fast-ci |  |  |  | 0/0 | #1306 | 2026-11-17 | 2026-11-17 | 1946 |
| 1182 | C |  | Flaky runner test: local_job_start_check_refuses_a_run_whose_event_ref_or_sha_differ fails | fast-ci,in-progress |  |  |  | 0/0 | #1212 | 2026-11-17 | 2026-11-17 | 938 |
| 1183 | O |  | The probe reports f_bfree as docker_root_free_bytes, so the 10 GiB Docker-root disk gate r | fast-ci |  |  |  | 0/0 | #1344 | 2026-11-17 | 2026-11-17 | 5331 |
| 1184 | O |  | Daily release-tags drift check does not validate creation rule and tag conditions |  |  |  |  | 0/0 | #1325 | 2026-11-17 | 2026-11-17 | 744 |
| 1185 | O |  | velnor-host local reports a crashed runner container as a served pair | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-17 | 5011 |
| 1186 | O |  | A torn write to the minted-agent journal is read back as a record | fast-ci |  |  |  | 0/0 | #1360 | 2026-11-17 | 2026-11-17 | 4055 |
| 1187 | O |  | The minted-journal bound check passes when the journal cannot be read | fast-ci |  |  |  | 0/0 | #1342 | 2026-11-17 | 2026-11-17 | 1990 |
| 1188 | O |  | A blank pr_only_checks entry is accepted and then breaks every release of the repository | fast-ci |  |  |  | 0/0 | #1311 | 2026-11-17 | 2026-11-17 | 3345 |
| 1201 | O |  | A selected Nextest lane passes when no tests are runnable | fast-ci |  |  |  | 0/0 | #1403 | 2026-11-17 | 2026-11-17 | 1700 |
| 1202 | O |  | Release integration tests pass positive cases after unrelated errors | fast-ci |  |  |  | 0/0 | #1405 | 2026-11-17 | 2026-11-17 | 2031 |
| 1203 | O |  | A failed OS identity read allows incompatible runner images to share a cache | fast-ci |  |  |  | 0/0 | #1391 | 2026-11-17 | 2026-11-17 | 2231 |
| 1204 | O |  | Missing path dependency artifacts let input audit write an incomplete lock | fast-ci |  |  |  | 0/0 | #1319 | 2026-11-17 | 2026-11-17 | 2081 |
| 1205 | C |  | Release after-gate: workflow discovery includes YAML files in .github/workflows subdirecto | fast-ci |  |  |  | 0/0 | #1207 | 2026-11-17 | 2026-11-17 | 796 |
| 1206 | O |  | Startup admits work after legacy reconciliation returns Hold | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-18 | 2129 |
| 1209 | O |  | Lab proof: changed-source selection adds the image job (follow-up to #1063) | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-18 | 824 |
| 1210 | O |  | fast-ci: share Sources cache payloads across Cargo roots | fast-ci |  |  |  | 0/0 | #1357 | 2026-11-17 | 2026-11-17 | 990 |
| 1213 | O |  | Base cache compatibility omits effective Cargo configuration outside the checkout |  |  |  |  | 0/0 | #1356 | 2026-11-17 | 2026-11-17 | 1035 |
| 1214 | C |  | Flaky runner test under load: removed::deadline::a_slow_removed_connection_retirement_does | fast-ci |  |  |  | 0/0 | #1222 | 2026-11-17 | 2026-11-18 | 827 |
| 1215 | C |  | Audit freshness: detect sibling deletion next to an excluded target (follow-up to #1192) | fast-ci,in-progress |  |  |  | 0/0 | #1192 | 2026-11-17 | 2026-11-17 | 979 |
| 1217 | O |  | Measure runner minutes for argument-expanded COPY selection (#1199 follow-up) | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-18 | 776 |
| 1218 | O |  | Cargo Deny fails on main: chrono unmaintained (RUSTSEC-2026-0543) via turso | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-18 | 788 |
| 1220 | O |  | Steps after repository code in the same job can't be trusted; put the trust boundary at th | fast-ci |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-17 | 1481 |
| 1221 | O |  | velnor-local: wait for a replacement pair before dispatching the next event |  |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-17 | 706 |
| 1233 | O |  | Dependency Dashboard |  |  |  |  | 0/0 |  | 2026-11-17 | 2026-11-19 | 41332 |
| 1290 | O |  | P1: velnor-new runs entirely on velnor-gen (Rust CI + Rust releases) | fast-ci |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 1382 |
| 1293 | C |  | Runner journal schema validation accepts an extra column on intents | fast-ci |  |  |  | 0/0 | #1298 | 2026-11-18 | 2026-11-18 | 472 |
| 1303 | O |  | reconcile_gate never observes GitHub: it compares a row against its own runner ids |  |  |  |  | 0/0 | #1309 | 2026-11-18 | 2026-11-18 | 992 |
| 1305 | O |  | P3: CI/CD performance for velnor-new (tracker) |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 1436 |
| 1362 | O |  | velnor-ci sources fetch --roots: one fetch step per lane, restore the 20+ root limit |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 1292 |
| 1366 | O |  | Unit tests disabled by `test = false` never run in CI | fast-ci |  |  |  | 0/0 | #1387,#1389 | 2026-11-18 | 2026-11-18 | 2829 |
| 1367 | O |  | velnor-host disconnect --wait never completes: is_drained counts the connection's own boun |  |  |  |  | 0/0 | #1401 | 2026-11-18 | 2026-11-18 | 7199 |
| 1370 | O |  | docs: pr_ci = "label" is safe only with a merge queue that runs the full suite on merge_gr |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 607 |
| 1371 | O |  | Generated ci.yml cancels in-progress PR runs on every title/body edit (pull_request: edite |  |  |  |  | 0/0 | #1385 | 2026-11-18 | 2026-11-18 | 1834 |
| 1373 | O |  | test(local): compare up's wait against the service's absolute readiness deadline |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 529 |
| 1374 | O |  | velnor-gen reusable workflows (#1327): follow-ups from review |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 1026 |
| 1375 | O |  | Test groups (#601, PR #1358): follow-ups from review |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 644 |
| 1376 | O |  | Fork fence (PR #1328): label spelling cannot prove a GitHub-hosted runner |  |  |  |  | 0/0 |  | 2026-11-18 | 2026-11-18 | 643 |

## Hierarchy tree (all issues in dataset; [O]=open [C]=closed; closed shown only if in 30d window or ancestor chain)

Top-level parents (no parent, with sub-issues): 1

- [O] #574 [FAST] Roadmap: the fastest Rust CI a generator can produce (91 kids, 19 open)
  - [C] #575 [FAST-0] Lab and measurement: the measuring commands, velnor-lab, the experiment (5 kids, 0 open)
    - [C] #583 [FAST-0.1] Start the CI helper `velnor-ci` with the run-measuring commands (0 kids, 0 open)
    - [C] #584 [FAST-0.2] Create `velnor-lab`: a small Rust project for experiments and real ru (0 kids, 0 open)
    - [C] #585 [FAST-0.3] Lab experiments: how to use Mr. Boxington, and which job shape (0 kids, 0 open)
    - [C] #586 [FAST-0.5] Decisions: what is decided, what the lab decides, and what stays with (0 kids, 0 open)
    - [C] #652 [FAST-0.4] Lab experiment: does blaze earn a place in a generated workflow (0 kids, 0 open)
  - [C] #577 [FAST-2] Selection: the plan picks packages, with a reason for each (6 kids, 0 open)
    - [C] #593 [FAST-2.1] Selection types, the pure `select` function and `velnor-ci plan` (0 kids, 0 open)
    - [C] #594 [FAST-2.2] Classify paths outside packages: ignored, global, unknown (0 kids, 0 open)
    - [C] #595 [FAST-2.3] A `Cargo.lock` change selects only the packages whose dependencies ch (0 kids, 0 open)
    - [C] #596 [FAST-2.4] Discover compile-time inputs from cargo's dep-info (0 kids, 0 open)
    - [C] #597 [FAST-2.5] Configurable: what runs on main, and an optional scheduled full run (0 kids, 0 open)
    - [C] #598 [FAST-2.6] `velnor-ci gate`: a sound required check, and a summary of why each p (0 kids, 0 open)
  - [O] #578 [FAST-3] Job shape: one job by default, more only when it pays (10 kids, 4 open)
    - [C] #599 [FAST-3.1] Shape selection and the single job (0 kids, 0 open)
    - [C] #600 [FAST-3.2] Build tests once, hand them to every test job (0 kids, 0 open)
    - [O] #601 [FAST-3.3] Test groups with needs (0 kids, 0 open)
    - [C] #602 [FAST-3.4] Setup: the helper's `prepare`, `restored` and `run` commands, and a p (0 kids, 0 open)
    - [C] #603 [FAST-3.5] Events, concurrency and a conditional policy lane (0 kids, 0 open)
    - [O] #644 [FAST-3.6] Workspaces that share a job (0 kids, 0 open)
    - [O] #645 [FAST-3.7] Checking out only what Rust needs (0 kids, 0 open)
    - [O] #646 [FAST-3.8] Private dependencies: fetch with a short-lived token, then build offl (0 kids, 0 open)
    - [C] #647 [FAST-3.9] Feature configurations, run once when identical (0 kids, 0 open)
    - [C] #740 [FAST-3.1f] Documentation checks no crate twice and documents only what a change (0 kids, 0 open)
  - [O] #579 [FAST-4] Cache: a restored build that cargo treats as fresh (7 kids, 3 open)
    - [C] #605 [FAST-4.1] Cache families, typed keys and the CI build profile (0 kids, 0 open)
    - [C] #606 [FAST-4.2] Freshness: with Mr. Boxington, an unchanged project compiles nothing (0 kids, 0 open)
    - [O] #607 [FAST-4.3] Main writes the base cache from the ordinary CI run (0 kids, 0 open)
    - [O] #609 [FAST-4.5] PR delta layer: the second push of a PR reuses the first (0 kids, 0 open)
    - [O] #610 [FAST-4.4] Budget manager: stay under 8 GB without losing the base (0 kids, 0 open)
    - [C] #611 [FAST-4.6] Tools cache: the toolchain and tools from one entry, nothing installe (0 kids, 0 open)
    - [C] #612 [FAST-4.7] `velnor-ci report`: cache telemetry and a waste check in every run (0 kids, 0 open)
  - [C] #580 [FAST-1] Foundation: the new generator box, Rust detection, the test layers, the (6 kids, 0 open)
    - [C] #160 [FAST-1.2] Rust detection: workspaces, packages, the toolchain file and tool pin (0 kids, 0 open)
    - [C] #616 [FAST-1.1] Create the generator box: model, pure pipeline, one place for input a (0 kids, 0 open)
    - [C] #617 [FAST-1.5] Publish `velnor-ci` and `velnor-gen` as prebuilt binaries, pinned by  (0 kids, 0 open)
    - [C] #619 [FAST-1.3] Test layers: rule tests, golden tests, and a built-in speed limit (0 kids, 0 open)
    - [C] #620 [FAST-1.4] YAML rules: every generated step is one command (0 kids, 0 open)
    - [C] #665 [FAST-1.1b] velnor-gen reads and writes a repository: the io module, generate an (0 kids, 0 open)
  - [O] #581 [FAST-6] Guard and document (4 kids, 4 open)
    - [O] #622 [FAST-6.1] Speed budget: fail loudly when CI gets slower (0 kids, 0 open)
    - [O] #624 [FAST-6.2] Documentation: one page on how Velnor keeps Rust CI fast (0 kids, 0 open)
    - [O] #648 [FAST-6.3] Projects receive new Velnor versions automatically (0 kids, 0 open)
    - [O] #649 [FAST-6.4] Adopt the new generator in the owner's Rust projects (0 kids, 0 open)
  - [C] #582 [FAST-H] Remove the branches that hold nothing more to land (0 kids, 0 open)
  - [O] #626 [FAST-5] Dogfood: velnor-new's own Rust CI comes from the new generator (5 kids, 3 open)
    - [O] #604 [FAST-5.3] Switch velnor-new's Rust CI to the new generator (0 kids, 0 open)
    - [O] #608 [FAST-5.5] Delete the existing engine's Rust CI path (0 kids, 0 open)
    - [C] #621 [FAST-5.4] Proof on velnor-new: run the scenarios and post the table (0 kids, 0 open)
    - [O] #623 [FAST-5.1] velnor-lab runs the generator for real every night (0 kids, 0 open)
    - [C] #653 [FAST-5.2] Dogfood: this repository's CI builds the helper from source (0 kids, 0 open)
  - [O] #627 [FAST-L] Local GitHub: the official runner, a stand-in service and our own cache (8 kids, 0 open)
    - [C] #628 [FAST-L.1] Spike: the official ARM runner runs a job from runner.server on this  (0 kids, 0 open)
    - [C] #629 [FAST-L.2] GitHub's cache rules as a pure Rust module (0 kids, 0 open)
    - [C] #630 [FAST-L.3] The local cache server: GitHub's cache protocol in front of our rules (0 kids, 0 open)
    - [C] #631 [FAST-L.4] A local stub for the GitHub API calls the helper makes (0 kids, 0 open)
    - [C] #632 [FAST-L.5] The driver: `velnor-local run` replays a scenario on this machine (0 kids, 0 open)
    - [C] #633 [FAST-L.6] Conformance probes: record how real GitHub behaves, in velnor-lab (0 kids, 0 open)
    - [C] #650 [FAST-L.7] velnor-runner: native ARM containers (0 kids, 0 open)
    - [C] #651 [FAST-L.8] velnor-runner: a local mode, and the driver uses it (7 kids, 0 open)
      - [C] #742 [FAST-L.8.2] velnor-host: the local-mode feature, its release lock, and a pair c (0 kids, 0 open)
      - [C] #743 [FAST-L.8.3] velnor-local: the local authority (0 kids, 0 open)
      - [C] #744 [FAST-L.8.4] velnor-host local: pairs from a plan, the local job-start check, re (0 kids, 0 open)
      - [C] #745 [FAST-L.8.5] velnor-local: --runner pair runs scenarios on velnor-host pairs (0 kids, 0 open)
      - [C] #746 [FAST-L.8.6] velnor-local: fork pairs, the refusal proofs and the optional --pol (0 kids, 0 open)
      - [C] #747 [FAST-L.8.7] velnor-local: doctor, status and the README (0 kids, 0 open)
      - [C] #748 [FAST-L.8.1] velnor-local: build the pair images and mint a runner.server JIT (0 kids, 0 open)
  - [C] #634 [FAST-R] Land the finished work that still has value (8 kids, 0 open)
    - [C] #635 [FAST-R.1] Merge #554: runner fixes for long live sessions (0 kids, 0 open)
    - [C] #636 [FAST-R.2] Merge #556: source qualification and Rust jobs run only for affected  (0 kids, 0 open)
    - [C] #637 [FAST-R.3] Merge #523: keep the private registry address in the Bun lifecycle co (0 kids, 0 open)
    - [C] #638 [FAST-R.4] Runner: trim a piped token before storing it (0 kids, 0 open)
    - [C] #639 [FAST-R.5] Homebrew tap: accept a clean checkout under a strict umask (0 kids, 0 open)
    - [C] #640 [FAST-R.6] Fix apostrophes in generated workflow commands (0 kids, 0 open)
    - [C] #641 [FAST-R.7] Validate artifact ids before downloading in the preview and release w (0 kids, 0 open)
    - [C] #642 [FAST-R.8] Bind job-image publication to the exact run and the attested files (0 kids, 0 open)
  - [O] #662 [OWNER] Run the Qualification workflow once on the scale set after #554 (0 kids, 0 open)
  - [C] #674 [OWNER] Decide how the helper fetch step fits design rule 9 (one command per run (0 kids, 0 open)
  - [O] #680 [OWNER] Approve the existing engine's exemption for the hand-written release wor (0 kids, 0 open)
  - [O] #682 [OWNER] Allow only admins to create rust-ci-v* tags (0 kids, 0 open)
  - [C] #685 [OWNER] Provide a private repository for the velnor-runner arm64 proof run (#650 (0 kids, 0 open)
  - [C] #694 [FAST] Follow the real edges of unowned path packages in select (0 kids, 0 open)
  - [C] #697 [FAST] Make velnor-ci inputs audit safe after a partial build and with unusual t (0 kids, 0 open)
  - [C] #701 [OWNER] Change docs/goals.md rule 2 so a Cargo.lock change selects only the pack (0 kids, 0 open)
  - [C] #703 [FAST-2.3b] Select everything when an edit leaves Cargo.lock inconsistent with t (0 kids, 0 open)
  - [C] #705 [FAST-3.1b] `velnor-gen timings`: write the timings file from the last runs (0 kids, 0 open)
  - [C] #707 [OWNER] Decide the job-start check for velnor-runner's local mode (#651) (0 kids, 0 open)
  - [C] #710 [FAST-3.1c] `velnor-ci plan` reads no file beyond what the doc-step rule needs (0 kids, 0 open)
  - [C] #712 [FAST-3.1d] A sound rule for leaving out the doctest and documentation steps (0 kids, 0 open)
  - [C] #713 [FAST-3.4b] The generated job installs the whole declared toolchain profile (0 kids, 0 open)
  - [C] #716 [FAST] Make velnor-ci inputs audit check each build-script run, custom target sp (0 kids, 0 open)
  - [C] #717 [FAST-R.9] Install velnor-ci through a setup-velnor action, so rule 9 needs no e (0 kids, 0 open)
  - [C] #718 [FAST-3.1e] Run custom test harnesses that nextest cannot list (0 kids, 0 open)
  - [C] #719 [FAST] Credit reads of a path package outside the workspaces by the package carg (0 kids, 0 open)
  - [C] #720 [OWNER] Create the velnor-release App, the release environment and the lab tag r (0 kids, 0 open)
  - [C] #721 [FAST-REL.1] Prove App-only tag creation on velnor-lab (0 kids, 0 open)
  - [C] #722 [FAST-REL.2] The engine leaves workflows that carry the velnor-gen header (0 kids, 0 open)
  - [C] #723 [FAST-REL.3] velnor-gen owns every workflow with its header (0 kids, 0 open)
  - [C] #724 [FAST-REL.4] velnor-ci release-gate reads the required checks from the branch ru (0 kids, 0 open)
  - [C] #725 [FAST-REL.5] velnor-ci release record and release publish (0 kids, 0 open)
  - [C] #726 [FAST-REL.6] Read release-plz.toml and .velnor/release.toml into a ReleasePlan (0 kids, 0 open)
  - [C] #727 [FAST-REL.7] Render release.yml: trigger, gate, release-pr, tag (0 kids, 0 open)
  - [C] #728 [FAST-REL.8] Render build, sums and publish, and add velnor-gen release ruleset (0 kids, 0 open)
  - [O] #729 [FAST-REL.9] Prove the whole release flow on velnor-lab (0 kids, 0 open)
  - [C] #730 [OWNER] Create the seed tags velnor-ci-v0.1.79 and velnor-gen-v0.1.79 (for FAST- (0 kids, 0 open)
  - [O] #731 [FAST-REL.10] Velnor's own release-plz config and per-package tags (0 kids, 0 open)
  - [O] #732 [OWNER] Switch the release-tags ruleset to the App-only bypass (for FAST-REL.11) (0 kids, 0 open)
  - [O] #733 [FAST-REL.11] Generate this repository's release.yml and delete the #676 exempti (0 kids, 0 open)
  - [C] #734 [OWNER] Push the bootstrap tag rust-ci-v0.1.80 after R4 and R5 (for FAST-REL.9) (0 kids, 0 open)
  - [C] #736 [FAST-L.9] velnor-host names a too-long control socket path instead of reporting (0 kids, 0 open)
  - [C] #737 [FAST-L.10] The resource probe works on a host whose mountinfo is larger than 64 (0 kids, 0 open)
  - [C] #739 [FAST-1.x] The generator's tests start no external program (0 kids, 0 open)
  - [C] #750 [FAST-3.1f] Trust boundary for project configuration that weakens generated chec (0 kids, 0 open)
  - [C] #751 [FAST-L.11] velnor-host recovers a reservation after a crash during storage init (0 kids, 0 open)
  - [C] #752 [FAST-L.12] velnor-host reports the real error when a pair's helper check fails, (0 kids, 0 open)
  - [C] #753 [FAST-L.13] velnor-host connect creates its state folder 0700, and completion di (0 kids, 0 open)
  - [C] #754 [FAST-L.14] A finished pair leaves no private network, and completion cleanup pr (0 kids, 0 open)
  - [C] #760 [FAST] velnor-ci inputs audit honours cfg-gated build-dependencies (0 kids, 0 open)
  - [C] #764 [FAST] Test that setup-velnor refuses a binary with the wrong SHA-256 (0 kids, 0 open)
  - [C] #768 [FAST-3.1b.1] A stale or malformed timings file never stops root discovery (0 kids, 0 open)
  - [C] #769 [FAST-3.1b.2] Timings use only samples from the current runner (0 kids, 0 open)
  - [C] #777 [FAST-3.5.1] The policy lane runs machete when a change can add an unused depend (0 kids, 0 open)
  - [C] #779 [FAST-L.13.1] Every velnor-host state folder is created 0700 (0 kids, 0 open)
  - [C] #782 [FAST-3.1f.1] velnor-ci error output cannot forge a workflow command (0 kids, 0 open)
  - [C] #785 [FAST-L.12.1] Retire a pair that started but never bound and release its reserva (0 kids, 0 open)
  - [C] #786 [FAST-3.1f.2] Weakening warnings: only checked roots, correct "new" label, nexte (0 kids, 0 open)
  - [C] #790 [FAST-1.y] velnor-ci audit tests pass in parallel (0 kids, 0 open)
  - [C] #794 [FAST-1.y.1] Remove test temp folders left by killed tests (0 kids, 0 open)
  - [C] #796 [FAST-REL.6.1] Build metadata in a release version is tagged or refused up front (0 kids, 0 open)
  - [C] #799 [FAST-3.4c] Wire every plan output into the generated steps (0 kids, 0 open)
  - [C] #800 [FAST-L.12.2] Test that a long helper error is cut at the detail limit (0 kids, 0 open)
  - [C] #801 [FAST-3.4d] Check the metadata file in CI and harden velnor-ci run (0 kids, 0 open)
  - [C] #802 [FAST-L.11.1] velnor-host prints recovery journal rows read-only (0 kids, 0 open)
  - [C] #806 [FAST-1.z] Tests stay well under the 2 s slow-timeout (0 kids, 0 open)
  - [C] #808 [FAST-3.4e] The plan reads changed files from git, with no token (0 kids, 0 open)
  - [C] #811 [FAST-6.x] Document the local recipe for fixture-dependent tests (0 kids, 0 open)
  - [C] #817 [FAST-REL.8.1] Release config values cannot become Actions expressions (0 kids, 0 open)
  - [O] #1011 inputs save/restore ignore a custom build.target-dir (0 kids, 0 open)
  - [O] #1018 inputs snapshot scope misses tracked path dependencies outside a nested root (0 kids, 0 open)
  - [C] #1032 Verify PR-only release exemptions against external checks and rule bypass (0 kids, 0 open)
  - [C] #1069 Base cache: report failed saves and skip saves after a failed producer (0 kids, 0 open)
  - [C] #1071 The from-source helper compiles when no job needs the binary (0 kids, 0 open)
  - [O] #1072 A one-root policy job repeats the plan (0 kids, 0 open)
  - [O] #1073 Scheduled inputs audit builds every workspace again (0 kids, 0 open)
  - [C] #1100 inputs audit treats a missing dep-info input or a file inside a rerun-if-changed (0 kids, 0 open)
  - [C] #1101 inputs audit writes the lock when a feature-gated unit's dep-info is stale (0 kids, 0 open)
  - [C] #1102 inputs audit walks the target directory with no depth or file-size bound (0 kids, 0 open)
  - [C] #1103 plan prints a rejected git path to stderr as a workflow command (0 kids, 0 open)
  - [O] #1104 prepare's pinned-tool check mismatches mise OS filters and lock rows (0 kids, 0 open)
  - [C] #1105 complete-profile components are installed after the tools cache is saved (0 kids, 0 open)
  - [C] #1106 Base cache: gate saves on the plan's private-source result (0 kids, 0 open)
  - [O] #1128 Release qualification connects to a different Docker socket when the path contai (0 kids, 0 open)
  - [C] #1129 Qualification receipt is issued when a tool container has extra capabilities, de (0 kids, 0 open)
  - [C] #1130 Readiness scan treats a journal as in-bounds when launch.db-wal exceeds the file (0 kids, 0 open)
  - [C] #1131 Readiness scan stops at the first occupying intent and skips the row cap (0 kids, 0 open)
  - [O] #1132 Readiness scan counts the legacy intents table and misses live connection_intent (0 kids, 0 open)
  - [C] #1133 Readiness Docker /info body is not size-capped (0 kids, 0 open)

Open issues with no parent and no children (orphans): 92: [890, 931, 934, 941, 949, 966, 987, 1005, 1007, 1016, 1028, 1033, 1034, 1047, 1057, 1059, 1060, 1066, 1074, 1075, 1076, 1077, 1078, 1081, 1108, 1110, 1113, 1114, 1117, 1118, 1119, 1120, 1121, 1123, 1124, 1125, 1127, 1135, 1140, 1142, 1143, 1144, 1145, 1146, 1148, 1150, 1152, 1153, 1155, 1157, 1159, 1161, 1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1183, 1184, 1185, 1186, 1187, 1188, 1201, 1202, 1203, 1204, 1206, 1209, 1210, 1213, 1217, 1218, 1220, 1221, 1233, 1290, 1303, 1305, 1362, 1366, 1367, 1370, 1371, 1373, 1374, 1375, 1376]


## 4. Labels

| Label | Colour | Issues (all, incl closed) | PRs | Description |
|---|---|---|---|---|
| fast-ci | 0E8A16 | 377 (open 103) | 1 | Fast Rust CI roadmap |
| exec:sonnet | C5DEF5 | 164 (open 16) | 0 | Suggested executor: Sonnet 5.5 |
| effort:medium | fef2c0 | 138 (open 12) | 0 | Reasoning effort: medium |
| in-progress | 0e8a16 | 93 (open 17) | 3 | Somebody is working on this issue |
| effort:high | f9d0c4 | 51 (open 9) | 0 | Reasoning effort: high |
| exec:opus | BFD4F2 | 42 (open 6) | 0 | Suggested executor: Opus 5.5 |
| effort:low | e6f4ea | 29 (open 1) | 0 | Reasoning effort: low |
| exec:haiku | c5def5 | 12 (open 0) | 0 | Executor: Haiku 5.5 |
| owner-action | b60205 | 12 (open 5) | 0 | A follow-up that only the owner can do |
| epic | 5319E7 | 10 (open 6) | 0 | Parent issue with sub-issues |
| needs-owner | d93f0b | 7 (open 6) | 0 | A step in this issue can only be done by the owner |
| blocked | 5319e7 | 7 (open 2) | 0 | Could not be finished; the evidence is in the last comment |
| bug | d73a4a | 0 (open 0) | 0 | Something isn't working |
| documentation | 0075ca | 0 (open 0) | 0 | Improvements or additions to documentation |
| duplicate | cfd3d7 | 0 (open 0) | 0 | This issue or pull request already exists |
| enhancement | a2eeef | 0 (open 0) | 0 | New feature or request |
| good first issue | 7057ff | 0 (open 0) | 0 | Good for newcomers |
| help wanted | 008672 | 0 (open 0) | 0 | Extra attention is needed |
| invalid | e4e669 | 0 (open 0) | 0 | This doesn't seem right |
| question | d876e3 | 0 (open 0) | 0 | Further information is requested |
| wontfix | ffffff | 0 (open 0) | 0 | This will not be worked on |
| owner-decision | D93F0B | 0 (open 0) | 0 | Needs a decision only the owner can make |
| dependencies | ededed | 0 (open 0) | 3 |  |
| major | ededed | 0 (open 0) | 1 |  |
| needs-migration-review | ededed | 0 (open 0) | 1 |  |

## 5. Open PRs

CI = statusCheckRollup state of last commit. Roadmap-issue column marks closing refs (and `fast/<n>-` branch name).

| PR | Draft | Branch | Closes | Branch-issue | CI | Updated | Title |
|---|---|---|---|---|---|---|---|
| #1026 |  | peer/1018-1011-inputs |  |  | FAILURE | 2026-11-19 | fix(inputs): restore sibling packages and custom target directories |
| #1046 |  | fast/1007-floor-0.1.81 |  | 1007 | FAILURE | 2026-11-18 | Require velnor-ci 0.1.81+ for generation and delete the older-helper branches (# |
| #1050 |  | fast/1047-docs-drift |  | 1047 | FAILURE | 2026-11-18 | Docs: match the contracts to the code |
| #1107 |  | peer/609-pr-delta |  |  | FAILURE | 2026-11-18 | feat(cache): reuse isolated PR build deltas |
| #1168 |  | peer/consumer-nonrust-repairs |  |  | SUCCESS | 2026-11-18 | Repair non-Rust manifests and verified tool-cache reuse |
| #1169 | D | fast/1066-qualification-skipped | #1066 | 1066 | FAILURE | 2026-11-18 | fix(qualification): fail a dispatch that selects no job (#1066) |
| #1193 |  | peer/1070-mise-template | #1070 |  | SUCCESS | 2026-11-18 | fix(generator): reject executable mise configuration |
| #1194 |  | peer/1081-deny-paths | #1081 |  | FAILURE | 2026-11-18 | fix(generator): run cargo-deny with each root configuration |
| #1200 |  | peer/1104-mise-platform-pins | #1104 |  | FAILURE | 2026-11-18 | fix(mise): preserve platform-specific tool locks |
| #1223 |  | fast/604-shadow-rust-ci |  | 604 | FAILURE | 2026-11-19 | [FAST-5.3] Shadow: velnor-new's Rust CI from velnor-gen (phase A of #604) |
| #1235 | D | fast/1173-collect-pagination | #1173 | 1173 | FAILURE | 2026-11-18 | fix(ci-helper): refuse incomplete jobs pagination in bench collect |
| #1236 |  | fast/1059-ruleset-metadata | #1059 | 1059 | FAILURE | 2026-11-18 | fix(generator): load cargo metadata for the release ruleset commands |
| #1249 |  | peer/1114-fix | #1114 |  | FAILURE | 2026-11-18 | fix(runner): refuse unresolvable HOME instead of using the cwd |
| #1259 |  | pool/622 |  |  | FAILURE | 2026-11-18 | feat(generator): add a speed budget that fails loudly when CI gets slower (#622) |
| #1260 |  | peer/1120-scaleset-group | #1120 |  | SUCCESS | 2026-11-18 | fix(runner): reject scale-set responses naming another runner group |
| #1262 | D | pool/1072 | #1072 |  | FAILURE | 2026-11-18 | fix(generator): plan once for a one-root policy workflow |
| #1263 |  | peer/1117-daemon-lock-unlink | #1117 |  | FAILURE | 2026-11-18 | fix(runner): refuse a second daemon when daemon.lock is unlinked |
| #1269 |  | peer/1123-probe-root | #1123 |  | SUCCESS | 2026-11-18 | fix(runner): refuse noncanonical probe Docker directory |
| #1272 |  | peer/1127-volume-identity | #1127 |  | SUCCESS | 2026-11-18 | fix(runner): qualification volume cleanup re-checks identity right before delete |
| #1273 |  | peer/1128-unix-socket | #1128 |  | SUCCESS | 2026-11-18 | fix(runner): reject embedded unix:// in Docker socket path |
| #1274 |  | pool/644 |  |  | FAILURE | 2026-11-18 | feat(generator): check the workspaces of a group in one job set |
| #1275 |  | peer/1143-cleanup-proof-covers | #1143 |  | SUCCESS | 2026-11-18 | fix(runner): CleanupProof::covers requires full container id and named volume |
| #1279 |  | peer/1132-live-intents | #1132 |  | SUCCESS | 2026-11-18 | fix(runner): count live connection_intents rows in the readiness scan |
| #1280 |  | pool/1076 | #1076 |  | FAILURE | 2026-11-19 | fix(runner-host): refuse pre-JIT cleanup progress over a broken claim premise (# |
| #1283 |  | pool/1074 | #1074 |  | FAILURE | 2026-11-19 | fix(runner): a post-JIT retirement claim stops a runner from starting |
| #1287 |  | peer/1150-owned-mount-inode | #1150 |  | SUCCESS | 2026-11-18 | fix(runner): count an owned volume mount covered by another mount |
| #1288 |  | peer/1153-dockerd-deleted | #1153 |  | SUCCESS | 2026-11-18 | fix(runner): keep an unlinked dockerd as the daemon candidate |
| #1289 |  | peer/1155-lineage-lock-identity | #1155 |  | SUCCESS | 2026-11-18 | fix(runner): fail closed when the lineage lock file is replaced |
| #1294 |  | peer/1157-anchor-tmp | #1157 |  | FAILURE | 2026-11-18 | fix(runner): first lineage bind survives a leftover anchor temp file |
| #1295 | D | chore/renovate-ignore-generated |  |  | FAILURE | 2026-11-18 | chore(renovate): stop bumping generated outputs; hold x509-cert 0.2 |
| #1296 |  | renovate/tailrocks-velnor-new-digest |  |  | FAILURE | 2026-11-18 | chore(deps): update tailrocks/velnor-new digest |
| #1297 |  | renovate/cargo-minorpatch |  |  | FAILURE | 2026-11-18 | fix(deps): update cargo minor/patch |
| #1300 | D | fix/ci-helper-mirror-test-timeouts |  |  | FAILURE | 2026-11-18 | fix(ci-helper): 30 s limit for cargo-spawning mirror tests (2 s terminate-after  |
| #1302 |  | fast/1290-release-attestation |  | 1290 | FAILURE | 2026-11-18 | feat(generator): opt-in build-provenance attestation for release binaries |
| #1304 |  | peer/1028-goldens-etxtbsy |  |  | FAILURE | 2026-11-18 | fix(scripts): write the mutated archive guard atomically (ETXTBSY flake) |
| #1306 | D | fast/1178-forged-marker | #1178 | 1178 | FAILURE | 2026-11-18 | generator: never delete a workflow on a forged marker (#1178) |
| #1307 | D | fast/731-release-plz | #731 | 731 | FAILURE | 2026-11-18 | release: release-plz config and per-group tags (#731) |
| #1308 |  | fast/1290-feature-lanes |  | 1290 | FAILURE | 2026-11-19 | [P1] velnor-gen: feature lanes, nextest filter expressions, cargo-machete lint |
| #1309 |  | peer/1303-gate-github-observe | #1303 |  | FAILURE | 2026-11-18 | fix(runner): reconcile gate releases only on proven runner absence |
| #1311 |  | fast/1188-blank-pr-only-checks | #1188 | 1188 | FAILURE | 2026-11-18 | generator: refuse a blank pr_only_checks entry (#1188) |
| #1312 |  | fast/1016-preflight-tests | #1016 | 1016 | FAILURE | 2026-11-18 | release preflight: test the 100-PR refusal (#1016) |
| #1313 |  | fast/890-retryable-roots | #890 | 890 | FAILURE | 2026-11-18 | release: make each release root retryable (#890) |
| #1314 |  | fast/1057-published-no-tag | #1057 | 1057 | FAILURE | 2026-11-19 | release: refuse a published crate with no tag (#1057) |
| #1315 |  | fast/1290-attest-verify |  | 1290 | FAILURE | 2026-11-18 | [P1] release: verify release attestations before publish (stacked on #1302) |
| #1316 |  | fast/1290-raw-assets |  | 1290 | FAILURE | 2026-11-18 | [P1] Add opt-in raw binary asset format to release binaries (#1290) |
| #1317 |  | fast/1161-single-discovery |  | 1161 | FAILURE | 2026-11-18 | (#1161) lift the 32 KiB tests_by_package cap |
| #1318 |  | fast/p3-image-jobs |  |  | SUCCESS | 2026-11-18 | ci: stop the failing image jobs on PRs |
| #1319 |  | fast/1204-audit-lock | #1204 | 1204 | FAILURE | 2026-11-18 | fix(ci-helper): fail the input audit on missing required path dependency library |
| #1320 | D | fast/1073-per-root-audit | #1073 | 1073 | FAILURE | 2026-11-18 | (#1073) per-root inputs audit; no double workspace build |
| #1321 |  | fast/1159-target-dir | #1159 | 1159 | FAILURE | 2026-11-19 | fix(prepare): compare target-dir with the path recorded at generation |
| #1322 |  | fast/p3-skip-double-ci |  |  | FAILURE | 2026-11-19 | velnor-gen: skip re-testing on push after a green merge_group (#1305) |
| #1323 |  | fast/p3-mbx-remote-cache |  |  | FAILURE | 2026-11-19 | velnor-gen: opt-in mbx remote cache (#1305) |
| #1324 |  | fast/k8s-runtime-design |  |  | FAILURE | 2026-11-18 | docs: proposed Kubernetes runtime for velnor-host (one runner, two runtimes) |
| #1325 | D | fast/1184-drift-tag-rules | #1184 | 1184 | FAILURE | 2026-11-18 | fix(ci): drift check validates release-tags creation rule and tag conditions |
| #1326 |  | fast/1290-lane-guard-helper |  | 1290 | FAILURE | 2026-11-18 | [P1] velnor-ci: lane-guard checks a feature lane selects every feature-only test |
| #1327 |  | fast/1290-workflow-call |  | 1290 | FAILURE | 2026-11-19 | [P1] velnor-gen: opt-in reusable workflows (workflow_call) for release migration |
| #1328 |  | fast/1290-fork-fence-followup |  | 1290 | FAILURE | 2026-11-19 | velnor-gen fork fence follow-up: hosted-label allowlist, strict fork_runner, hon |
| #1329 | D | fast/host-linux-credential-file |  |  | FAILURE | 2026-11-18 | feat(runner): read the host credential from a file on Linux |
| #1330 | D | fast/host-org-scale-set |  |  | FAILURE | 2026-11-18 | feat(runner): org-level scale set with a named runner group |
| #1331 |  | peer/1206-hold-admission |  |  | FAILURE | 2026-11-18 | fix(runner): admit work only when the legacy gate says Advertise (#1206) |
| #1333 |  | fast/p3-linear-schema-validate |  |  | FAILURE | 2026-11-18 | runner: validate the journal schema once on open (linear, not quadratic) |
| #1334 | D | fast/state-seal-ancestor |  |  | FAILURE | 2026-11-18 | fix(ci): seal state for any head that is an ancestor of main's tip |
| #1335 |  | fast/1290-lane-guard-gen |  | 1290 | FAILURE | 2026-11-18 | [P1] velnor-gen: emit a lane-guard step after filtered feature lanes (#1290) |
| #1336 |  | fast/1290-release-integration |  | 1290 | FAILURE | 2026-11-18 | [P1] velnor-gen release: multi-bin, raw multi-bin, pre-publish checks, asset nam |
| #1337 |  | fast/1290-attest-later |  | 1290 | FAILURE | 2026-11-19 | velnor-gen release attest follow-up: verify job with attestations:read only, den |
| #1338 | D | fast/check-jobs-cache |  |  | FAILURE | 2026-11-18 | perf(ci): restore cargo sources cache in check jobs |
| #1339 |  | fast/1290-refuse-nested-release-root |  | 1290 | FAILURE | 2026-11-18 | [P1] velnor-gen: refuse a release-plz root below the git root (release-plz 0.4.3 |
| #1340 |  | fast/1290-release-gaps |  | 1290 | FAILURE | 2026-11-19 | [P1] velnor-gen release parity: smoke, musl raw target, COMMIT asset, raw member |
| #1341 |  | renovate/actions-create-github-app-token-3.x |  |  | FAILURE | 2026-11-19 | chore(deps): update actions/create-github-app-token action to v3.4.0 |
| #1342 |  | fast/1187-unreadable-journal | #1187 | 1187 | FAILURE | 2026-11-18 | fix(runner): fail the minted-journal bound check when the journal cannot be read |
| #1343 |  | fast/1177-malformed-inventory | #1177 | 1177 | FAILURE | 2026-11-18 | fix(local): a malformed runner inventory is no proof of deletion |
| #1344 |  | fast/1183-bavail | #1183 | 1183 | FAILURE | 2026-11-18 | fix(runner): report f_bavail as docker_root_free_bytes |
| #1346 | D | fast/1125-frame-payload | #1125 | 1125 | FAILURE | 2026-11-18 | fix(runner): reject multiplexed log frames with a missing or truncated payload |
| #1347 |  | fast/1121-authority-filters | #1121 | 1121 | FAILURE | 2026-11-18 | fix(runner): refuse tag filters on schedule and merge_group authority contexts |
| #1348 |  | renovate/jdx-mise-2026.x |  |  | FAILURE | 2026-11-18 | chore(deps): update dependency jdx/mise to v2026.11.10 |
| #1349 | D | fast/runner-host-test-tail |  |  | FAILURE | 2026-11-18 | perf(runner): cache historical-schema journals in runner-host tests |
| #1351 |  | fast/1290-archive-pinning |  | 1290 | FAILURE | 2026-11-19 | [P1] velnor-gen: archive-ready helper pinning (lock records archive and binary d |
| #1352 |  | fast/1290-setup-velnor-archive |  | 1290 | FAILURE | 2026-11-19 | [P1] setup-velnor: install velnor-ci from the release archive (#1290) |
| #1353 | D | fast/1172-plan-tracked-files | #1172 | 1172 | FAILURE | 2026-11-18 | fix(ci-helper): plan selects everything when tracked files differ from HEAD |
| #1354 | D | fast/645-sparse-checkout |  | 645 | FAILURE | 2026-11-18 | feat(generator): opt-in sparse checkout per Rust lane (#645) |
| #1355 | D | fast/1060-embedded-inputs | #1060 | 1060 | FAILURE | 2026-11-18 | fix(generator): count embedded file reads as crate inputs |
| #1356 | D | fast/1213-cache-key-cargo-config | #1213 | 1213 | FAILURE | 2026-11-18 | fix(ci-helper): hash external Cargo config into the base cache key |
| #1357 |  | peer/1210-shared-sources | #1210 |  | FAILURE | 2026-11-19 | fast-ci: share Sources cache payloads across Cargo roots (#1210) |
| #1358 |  | peer/601-test-groups-slice1 |  |  | FAILURE | 2026-11-19 | feat(generator): test groups with docker/cargo needs, slice 1 (#601) |
| #1359 | D | fast/1124-uncertain-create | #1124 | 1124 | FAILURE | 2026-11-18 | fix(qualification): remove owned container after uncertain unjournaled create |
| #1360 |  | fast/1186-torn-journal | #1186 | 1186 | FAILURE | 2026-11-19 | fix(runner): checksum minted-agent journal records (#1186) |
| #1361 | D | fast/1108-handoff-cleanup | #1108 | 1108 | FAILURE | 2026-11-18 | fix(runner): record uncertainty when the storage-initializer handoff fails |
| #1365 | D | research/velnor-runner-2026-11 |  |  | FAILURE | 2026-11-19 | research: Velnor runner design panel (2026-11) |
| #1368 |  | fast/old-engine-rust-ci-off |  |  | FAILURE | 2026-11-18 | velnor-actions: [stacks.rust] ci = false switch (ships in v0.1.81; #604 step 4) |
| #1369 |  | fast/flake-velnor-local-up-wait |  |  | FAILURE | 2026-11-18 | test(local): take the start instant before the environment exists |
| #1377 | D | fast/hosted-gap |  |  | FAILURE | 2026-11-18 | [P0] velnor-actions: place source-qualification jobs on the scale set (opt-in, f |
| #1378 | D | fast/hosted-gap-2 |  |  | FAILURE | 2026-11-18 | [P0] velnor-actions: place image-candidate, source-preview and cache-prune jobs  |
| #1383 |  | fast/ci-mirror-wrapper-machete |  |  | FAILURE | 2026-11-19 | fix(ci): velnor-ci crates-mirror re-run on a read-only wrapper; cargo-machete st |
| #1384 |  | fast/1175-nbf | #1175 | 1175 | FAILURE | 2026-11-19 | fix(local): reject runtime tokens before their nbf |
| #1385 |  | fast/1371-edited-no-cancel | #1371 | 1371 | FAILURE | 2026-11-19 | fix(generator): a title or body edit no longer cancels the running PR run (#1371 |
| #1386 |  | fast/624-ci-fast-doc |  | 624 | FAILURE | 2026-11-19 | docs: one page on how Velnor keeps Rust CI fast (#624) |
| #1387 |  | fast/1366-enable-unit-tests | #1366 | 1366 | FAILURE | 2026-11-19 | test: run the unit tests of four crates that disabled their own targets |
| #1388 |  | fast/1170-plain-ready | #1170 | 1170 | FAILURE | 2026-11-19 | fix(local): plain-mode up waits for runners to report online |
| #1389 |  | fast/1152-btrfs-device | #1152,#1366 | 1152 | FAILURE | 2026-11-19 | fix(probe): verify the bound btrfs subvolume device before publication writes |
| #1390 |  | fast/1142-capacity-live-slot | #1142 | 1142 | FAILURE | 2026-11-19 | fix(runner-core): Capacity::reserve never replaces a live worker slot (#1142) |
| #1391 | D | fast/1203-os-identity-fail-closed | #1203 | 1203 | FAILURE | 2026-11-19 | fix(ci-helper): fail prepare when the OS identity cannot be read |
| #1392 | D | fast/1171-discovery-cold-build | #1171,#1176 | 1171 | FAILURE | 2026-11-19 | fix(ci-helper): fail restored when target discovery fails |
| #1393 |  | peer/1362-sources-fetch |  |  | FAILURE | 2026-11-19 | velnor-ci sources fetch --roots: one fetch step per saving lane, 20+ roots (#136 |
| #1394 |  | fast/source-qual-stale-merge |  |  | FAILURE | 2026-11-19 | source qualification: accept a base that moved while the run waited (bounded fet |
| #1395 |  | fast/646-private-deps |  | 646 | FAILURE | 2026-11-19 | [FAST-3.8] Suggest the private sources table when a lock holds private dependenc |
| #1396 |  | fast/1290-cache-key-member-version |  | 1290 | FAILURE | 2026-11-19 | velnor-ci/velnor-gen: cache keys ignore workspace-member versions (0.1.84) |
| #1397 |  | fast/1290-lane-clippy-doc |  | 1290 | FAILURE | 2026-11-19 | velnor-gen: [[test_lane]] lint and doc with the lane's features |
| #1398 |  | fast/1075-retirement-absence |  | 1075 | FAILURE | 2026-11-19 | fix(runner-host): record bootstrap containers absent only after proven gone (jou |
| #1399 |  | fast/1078-doctor-probe |  | 1078 | FAILURE | 2026-11-19 | fix(runner-host): let an uncertain Doctor probe with an aborted child recover (j |
| #1401 |  | peer/1367-drain-own-session | #1367 |  | PENDING | 2026-11-19 | fix(host): drain waits ignore the connection's own bound session |
| #1402 | D | fast/1119-create-reread | #1119 | 1119 | FAILURE | 2026-11-19 | fix(runner): keep the id of a refused created scale set (#1119) |
| #1403 |  | fast/1201-empty-test-lane | #1201 | 1201 | FAILURE | 2026-11-19 | fix(generator): a scheduled test lane that ran zero tests fails Required |
| #1404 |  | fast/controller-test-reads-pin |  |  | FAILURE | 2026-11-19 | test: derive shipped-controller identities from the pinned files |
| #1405 |  | fast/1202-release-positive-cases | #1202 | 1202 | FAILURE | 2026-11-19 | test(release): positive cases must succeed, not merely avoid one error |

### Issues with work in progress (open PR linked or label in-progress)

- #604 OPEN [FAST-5.3] Switch velnor-new's Rust CI to the new generator | PRs [1223] | in-progress label: False
- #607 OPEN [FAST-4.3] Main writes the base cache from the ordinary CI run | PRs [] | in-progress label: True
- #609 OPEN [FAST-4.5] PR delta layer: the second push of a PR reuses the first | PRs [] | in-progress label: True
- #610 OPEN [FAST-4.4] Budget manager: stay under 8 GB without losing the base | PRs [] | in-progress label: True
- #622 OPEN [FAST-6.1] Speed budget: fail loudly when CI gets slower | PRs [] | in-progress label: True
- #624 OPEN [FAST-6.2] Documentation: one page on how Velnor keeps Rust CI fast | PRs [1386] | in-progress label: False
- #645 OPEN [FAST-3.7] Checking out only what Rust needs | PRs [1354] | in-progress label: False
- #646 OPEN [FAST-3.8] Private dependencies: fetch with a short-lived token, then  | PRs [1395] | in-progress label: False
- #731 OPEN [FAST-REL.10] Velnor's own release-plz config and per-package tags | PRs [1307] | in-progress label: False
- #890 OPEN [FAST-B3.33] Make each release root retryable in the tag job | PRs [1313] | in-progress label: False
- #941 OPEN CI speed: generator follow-ups from consumer repos | PRs [] | in-progress label: True
- #1005 OPEN A cancelled run's reservation stays reserved: completion cleanup fails | PRs [] | in-progress label: True
- #1007 OPEN Require velnor-ci 0.1.81+ for generation (release and the Required gat | PRs [1046] | in-progress label: True
- #1011 OPEN inputs save/restore ignore a custom build.target-dir | PRs [] | in-progress label: True
- #1016 OPEN Release preflight: test the 100-PR refusal; drop the unreachable ident | PRs [1312] | in-progress label: True
- #1018 OPEN inputs snapshot scope misses tracked path dependencies outside a neste | PRs [] | in-progress label: True
- #1033 OPEN OpenTofu: emit native task records for consumer group ledgers | PRs [] | in-progress label: True
- #1034 OPEN Consumer freeze triage: repair non-Rust generation/cache/adoption bloc | PRs [] | in-progress label: True
- #1047 OPEN Docs drift: statements in the contracts that no longer match the code | PRs [1050] | in-progress label: True
- #1057 OPEN A crate already on crates.io with no git tag is a green release | PRs [1314] | in-progress label: False
- #1059 OPEN velnor-gen release ruleset accepts a .lock version that generate refus | PRs [1236] | in-progress label: False
- #1060 OPEN A changed file a crate embeds can skip that crate when the inputs lock | PRs [1355] | in-progress label: False
- #1066 OPEN Qualification dispatch succeeds when every job is skipped | PRs [1169] | in-progress label: True
- #1070 CLOSED Plan accepts a mise template that the install step executes | PRs [1193] | in-progress label: False
- #1072 OPEN A one-root policy job repeats the plan | PRs [1262] | in-progress label: False
- #1073 OPEN Scheduled inputs audit builds every workspace again | PRs [1320] | in-progress label: True
- #1074 OPEN Post-JIT retirement fences allow a runner to reach started | PRs [1283] | in-progress label: False
- #1075 OPEN Unbound retirement records containers absent while they are still star | PRs [1398] | in-progress label: False
- #1076 OPEN Pre-JIT cleanup progress advances while bootstrap containers still exi | PRs [1280] | in-progress label: False
- #1078 OPEN An uncertain Doctor probe whose child is aborted cannot open | PRs [1399] | in-progress label: True
- #1081 OPEN cargo-deny is skipped unless the root file is exactly deny.toml | PRs [1194] | in-progress label: False
- #1104 OPEN prepare's pinned-tool check mismatches mise OS filters and lock rows | PRs [1200] | in-progress label: True
- #1108 OPEN A failed storage-initializer handoff leaves the helper running and the | PRs [1361] | in-progress label: False
- #1114 OPEN velnor-host uses the process cwd when HOME is missing or relative | PRs [1249] | in-progress label: False
- #1117 OPEN Removing daemon.lock lets a second velnor-host daemon open a session | PRs [1263] | in-progress label: False
- #1119 OPEN Scale-set create records the POST body without a re-read | PRs [1402] | in-progress label: False
- #1120 OPEN A scale-set response with a different runnerGroupId is accepted | PRs [1260] | in-progress label: False
- #1121 OPEN Job-authority inventory drops schedule and merge_group tag filters and | PRs [1347] | in-progress label: False
- #1123 OPEN Qualification probe bind-mounts a root or noncanonical Docker director | PRs [1269] | in-progress label: False
- #1124 OPEN Uncertain qualification create leaves the owned container in place | PRs [1359] | in-progress label: False
- #1125 OPEN Qualification accepts a log whose multiplexed frame header has no payl | PRs [1346] | in-progress label: False
- #1127 OPEN Qualification volume cleanup deletes a volume that reused the name | PRs [1272] | in-progress label: False
- #1128 OPEN Release qualification connects to a different Docker socket when the p | PRs [1273] | in-progress label: False
- #1132 OPEN Readiness scan counts the legacy intents table and misses live connect | PRs [1279] | in-progress label: False
- #1142 OPEN Capacity::reserve replaces a live worker slot when another intent fits | PRs [1390] | in-progress label: False
- #1143 OPEN CleanupProof::covers accepts a short container id and an empty volume | PRs [1275] | in-progress label: False
- #1150 OPEN Cleanup scan drops an owned mount when the walked inode differs | PRs [1287] | in-progress label: False
- #1152 OPEN Btrfs publication skips the bound subvolume device and still writes | PRs [1389] | in-progress label: False
- #1153 OPEN Unlinked dockerd is dropped and a different dockerd file is bound | PRs [1288] | in-progress label: False
- #1155 OPEN A replaced lineage lock still verifies the journal | PRs [1289] | in-progress label: False
- #1157 OPEN A leftover anchor temp file blocks the first lineage bind | PRs [1294] | in-progress label: False
- #1159 OPEN prepare: compare target-dir with the path recorded at generation | PRs [1321] | in-progress label: False
- #1161 OPEN fast-ci: list tests without a second discovery pass; lift the 32 KiB p | PRs [1317] | in-progress label: False
- #1170 OPEN Local plain mode reports ready when runner.server explicitly says the  | PRs [1388] | in-progress label: False
- #1171 OPEN Restored reports a cold build after target discovery fails and leaves  | PRs [1392] | in-progress label: False
- #1172 OPEN Automatic planning can skip Rust checks when tracked files differ from | PRs [1353] | in-progress label: False
- #1173 OPEN Bench collect accepts an incomplete jobs response as complete metrics | PRs [1235] | in-progress label: False
- #1175 OPEN Local cache authorization accepts signed runtime tokens before their n | PRs [1384] | in-progress label: False
- #1176 OPEN Restored reports a cold build after failed discovery leaves nested cac | PRs [1392] | in-progress label: False
- #1177 OPEN Local authority reports deletion proved when the verification inventor | PRs [1343] | in-progress label: False
- #1178 OPEN A forged Velnor marker lets generation delete an unrelated workflow | PRs [1306] | in-progress label: False
- #1183 OPEN The probe reports f_bfree as docker_root_free_bytes, so the 10 GiB Doc | PRs [1344] | in-progress label: False
- #1184 OPEN Daily release-tags drift check does not validate creation rule and tag | PRs [1325] | in-progress label: False
- #1186 OPEN A torn write to the minted-agent journal is read back as a record | PRs [1360] | in-progress label: False
- #1187 OPEN The minted-journal bound check passes when the journal cannot be read | PRs [1342] | in-progress label: False
- #1188 OPEN A blank pr_only_checks entry is accepted and then breaks every release | PRs [1311] | in-progress label: False
- #1201 OPEN A selected Nextest lane passes when no tests are runnable | PRs [1403] | in-progress label: False
- #1202 OPEN Release integration tests pass positive cases after unrelated errors | PRs [1405] | in-progress label: False
- #1203 OPEN A failed OS identity read allows incompatible runner images to share a | PRs [1391] | in-progress label: False
- #1204 OPEN Missing path dependency artifacts let input audit write an incomplete  | PRs [1319] | in-progress label: False
- #1210 OPEN fast-ci: share Sources cache payloads across Cargo roots | PRs [1357] | in-progress label: False
- #1213 OPEN Base cache compatibility omits effective Cargo configuration outside t | PRs [1356] | in-progress label: False
- #1290 OPEN P1: velnor-new runs entirely on velnor-gen (Rust CI + Rust releases) | PRs [1302, 1308, 1315, 1316, 1326, 1327, 1328, 1335, 1336, 1337, 1339, 1340, 1351, 1352, 1396, 1397] | in-progress label: False
- #1303 OPEN reconcile_gate never observes GitHub: it compares a row against its ow | PRs [1309] | in-progress label: False
- #1366 OPEN Unit tests disabled by `test = false` never run in CI | PRs [1387, 1389] | in-progress label: False
- #1367 OPEN velnor-host disconnect --wait never completes: is_drained counts the c | PRs [1401] | in-progress label: False
- #1371 OPEN Generated ci.yml cancels in-progress PR runs on every title/body edit  | PRs [1385] | in-progress label: False

Open PRs with no linked issue: [1026, 1107, 1168, 1259, 1274, 1295, 1296, 1297, 1300, 1304, 1318, 1322, 1323, 1324, 1329, 1330, 1331, 1333, 1334, 1338, 1341, 1348, 1349, 1358, 1365, 1368, 1369, 1377, 1378, 1383, 1393, 1394, 1404]

### Open issues blocked by open issues (blockedBy relationship)

#604<-[610, 601, 623], #609<-[607], #610<-[607], #623<-[609], #624<-[608], #648<-[623], #649<-[648, 608], #731<-[729], #733<-[731, 732], #931<-[949], #934<-[1007]

## 6. Issue templates and conventions

- `.github/ISSUE_TEMPLATE/`: does not exist. No issue forms, no config.yml. No org-level `tailrocks/.github` repo (404). Only `.github/PULL_REQUEST_TEMPLATE.md` (sections: Related pull requests, Summary, What ships, Behavior changes, ...).
- `docs/roadmap.md` (132 lines): process for roadmap #574. State lives in GitHub (no progress file). Mapping: done = closed; blocked = "blocked by" relationships; model = labels `exec:haiku|sonnet|opus` + `effort:low|medium|high`; owner steps = label `needs-owner` + `[OWNER] ...` follow-up issues (label `owner-action`, child of #574, WITHOUT `fast-ci`); in-flight = label `in-progress`; stuck = label `blocked`; experiments decided in #586. Ready-issue query: open, label `fast-ci`, no sub-issues, no open blockedBy, no `in-progress`/`blocked`, sorted by blocking count. Loop: max 3 issues in parallel; branches `fast/<issue number>-<short name>`; one issue = one PR ending `Closes #N` (or `Refs #N` when a lab run is needed after merge). Project board: Status moved by hand with `gh project item-edit` on claim (options Todo f75ad846, In Progress 47fc9ee4); GitHub adds new children of #574 to the project automatically and moves cards on PR open / issue close. Roadmap says the board "is a display, not the state".
- The roadmap and AGENTS.md depend on labels `fast-ci`, `in-progress`, `blocked`, `owner-action`, `needs-owner`, `exec:*`, `effort:*`, `epic` and on #574 as the single parent. Reorganisation must keep the ready-issue query working or update docs/roadmap.md.
- `AGENTS.md`: line 30 points to docs/roadmap.md for roadmap work; line 43/44 review rules and "docs stay evergreen: no PR/branch references". No explicit issue-writing template.
- Issue #574 body holds the rules for executors ("Rules for whoever executes an issue", 9 rules: one issue one PR, ~800 source-line cap, before/after from `velnor-ci bench`, three-place verification, subtract before adding, etc.), plus Targets, Order of work, Models, Not in this plan.
- Title conventions observed: `[FAST-<stage>.<n>] ...` (e.g. FAST-3.4), `[FAST-B3.<n>]` (50 issues), `[FAST-REL.<n>]` (14), `[FAST-CI.<n>]`, `[OWNER] ...` (12); many newer issues (#9xx-#13xx) have plain titles with no prefix. Stage names match project Stage field and milestones 1:1 (FAST-0..6, L, R).
- Issue types: Feature 46, Task 17, Bug 5, none 345 (of 413).

## 7. API capabilities

- `gh auth status`: account donbeave (keyring), scopes: admin:org, delete_repo, gist, project, repo, workflow, write:packages. `project` (read+write) and `repo` are present, so ProjectV2 mutations (addProjectV2ItemById, updateProjectV2ItemFieldValue, createProjectV2Field, updateProjectV2View...) and sub-issue mutations (addSubIssue, removeSubIssue, reprioritizeSubIssue), issue type updates (updateIssue issueTypeId), label and milestone writes are all permitted by scope. `admin:org` also covers org-level settings (issue types admin). Note `delete_repo` is a dangerous extra scope.
- Issue types feature: ENABLED for org tailrocks. `organization.issueTypes`: Task (A specific piece of work), Bug (An unexpected problem or behavior), Feature (A request, idea, or new functionality); all isEnabled=true. Currently used on 68 issues.
- Sub-issues and blockedBy relationships are in use and readable via GraphQL (parent, subIssues, subIssuesSummary, blockedBy). The Sub-issues progress and Parent issue project fields exist.
- GitHub API budget: docs/roadmap.md says one shared token, 5000 calls/hour. A heavy GraphQL query (PRs with checks) returned HTTP 504 once; paginate with small pages.
- Caveat: the project has no iteration/date/number fields; only Status and Stage are custom.

## Raw files (raw/)

project_meta.json (fields, views, readme), project_items.json (160 items + all field values), milestones.json, labels.json / labels_flat.json, issues_all.json (413, all states), issues_selected.json (open + closed in 30d; here identical to all because the repo's first issue is 2026-10-17), prs_open_all.json (114), open_blockedby.json. Collection scripts: tools/.
