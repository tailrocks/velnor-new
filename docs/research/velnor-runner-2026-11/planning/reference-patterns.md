# Patterns for a large agent-executed GitHub plan (public-safe)

This note distills ideas from three private reference plans (called reference A, B and C). It contains no names, numbers, titles or text from them. Every example below is newly written for tailrocks/velnor-new.

---

## 1. What the references showed (generalised)

**Reference A: a single-repo roadmap.**
- The project was a view over one repository's issues, generated from a tracker file kept in the repo, so it was reproducible and never hand-edited.
- It had one root issue, epics under it, and tasks under epics. Order came from "blocked by" links, not from the tree.
- Custom fields covered phase, kind, model, size, and start/target dates. The dates existed only so the roadmap layout could draw bars.
- Model and size existed twice, as a label and as a field. Phase existed twice, as a milestone and as a field.
- Only the default table view was saved. The README told readers to group by hand.
- Milestones were ordered gates. Each had a due date about four weeks after the previous one. Each description was a theme sentence plus an "Exit:" clause. A deliberately last, low-priority milestone said it starts only when all earlier ones are done.
- Every issue ended with a generated footer: stable plan key, model, size, parent, path, blockers, and a hidden comment holding the key. Re-running the generator updated issues by key, so there were no duplicates.
- Decision issues had Question / Options / Recommendation / What changes once decided, and said that agents prepare the decision and a human makes it.
- A root-issue table routed work to models and stated the escalation rules.

**Reference B: a multi-repo plan.**
- One project spanned about fifteen repositories. It had a fixed three-level hierarchy (parent, sub, leaf) held in a Level field.
- Titles carried a dotted code in square brackets that encoded the path.
- Phase names stated an observable outcome rather than an activity.
- Fields recorded agent, model and effort for both the implementer and the reviewer.
- There was one milestone name shared by all repos. Phases lived in a project field, because a field can span repos and milestones cannot.
- Saved views were named work queues built on label filters, including a "ready to start" queue. None saved a grouping or sort.
- Leaf issues were long, script-like bodies with a fixed section order: summary with a proof command, what exists today, read first, test first, numbered steps with Run/Expected/Reason, scope and out of scope, "if blocked", completion summary.
- Parent issues carried an exit check made of numbered copy-paste commands with expected output.
- Sub-issues lived in the repository whose code changes, and the parent lived in the central repo.
- Per-repo label sets were synced by script, and squash-only rulesets had one required check.

**Reference C: a milestone and label scheme.**
- Labels used `prefix: value` form for type, priority, size, area, source and ownership.
- Human-only work was marked with an ownership label rather than an assignee.

---

## Best ideas to borrow (ranked)

1. **A fixed, shallow hierarchy with a Level field and a dotted code in the title.** A weak model, or a human skimming a flat list, sees where an issue sits without opening it. The code is also a stable cross-reference key in prose.
2. **A leaf template that works as a script.** Use a summary with a proof command, "what exists today" with paths and line ranges, read-first, test-first, and numbered steps that each have `Run:`, `Expected:` and `Reason:`. Add scope and out of scope, and "if blocked". This is the biggest lever for weak models: they never design, search or guess, and every step has a pass/fail check.
3. **An "if blocked" protocol that forbids asking questions and defines the stop state.** Push, comment with the failing command and the last output lines, add a blocked marker, stop, and name which next issue may start meanwhile. An unattended fleet cannot wait on a human, so this makes failure cheap and visible.
4. **Model, effort and agent for both implementer and reviewer as project fields, with a model label for filters.** This enables a "by model" view, cost accounting, and a dispatcher that reads the field to pick the worker.
5. **An escalation ladder stated as a rule.** Start at the recommended rung. Escalate one rung only after ruling out missing context. A tier change is relabelled with a recorded reason. The top tier is reserved for architecture issues that produce a design doc. This keeps the cheap model on mechanical work and prevents silent over-spend.
6. **Parent issues with an executable exit check plus a "done when" line.** Phase completion becomes a script, not an opinion.
7. **Native sub-issues plus native "blocked by" links, mirrored in a body footer, and a "Ready" view.** The fleet's work queue is one saved query: open, unblocked, lowest open milestone.
8. **A machine-readable footer with a stable plan key, generated from a tracker file in the repo.** Re-sync is idempotent, there are no duplicate issues, and the tracker file is the review surface for the plan.
9. **Milestones as ordered gates with a "theme + Exit:" description and real due dates.** Add an explicit rule that the next workable issue is any open, unblocked task in the lowest open milestone, and gate low-priority work as "starts only when earlier gates are done".
10. **Decision issues and human-only work with their own queue.** Agents never decide what needs the owner, and the owner gets one list.
11. **Phase names that state an observable outcome, not an activity.**
12. **Sub-issues live in the repo whose code changes; the parent lives in the central repo.** PRs and CI stay per-repo while the project spans repos.
13. **"Decisions already made", each with a source or "plan default".** This stops agents reopening settled questions.
14. **A completion-summary contract** (commands with exit codes, red/green excerpts, files changed, PR link, three lines). It gives the reviewer model a cheap objective check.
15. **Per-repo label sets synced by script, and squash-only rulesets with one required check.**

## Patterns to avoid

- **Very large leaf bodies** (tens of KB) that embed the whole design, a repeated routing table, and a long execution-rules block in every issue. They are hard on a weak model's context and on humans. Put shared rules in one repo file and link it. Keep a leaf under about 8 KB unless truly needed.
- **A routing matrix copied into every leaf and listing many providers' models.** It is noise. Pick one model and effort and one escalation rung per role in the issue, and keep the matrix in one doc.
- **Absolute local paths hard-coded in steps.** They work on one machine only. Use repo-relative paths and an env var for the worktree root.
- **Saved views without grouping or sort.** Save the group-by and sort on every view, and write a useful project README.
- **An empty project README, or one that only lists fields.** Say how to find the next task, what the levels mean, and where the plan source lives.
- **Duplicated truth without a sync rule** (model as label and field, phase as milestone and field, dates copied from milestones). Pick one source and say which. Generate the other.
- **Unused enum options, and a filter tag that matches every item.** Options should be used or removed.
- **Dependencies only in prose, or only in a footer, when native dependencies exist.** Use native links as the source of truth and mirror them in the body.
- **Free-text values in two places** (for example size in the body and in a field) that can drift. Generate the body from the field.
- **No risk field and no per-issue reviewer rationale.** Add both (below).
- **Milestones without due dates.** They give no ordering. An undated backlog milestone is fine only if the roadmap view hides it.
- **Ignoring native issue types** while a `type:` label duplicates them.

---

## Proposed structure for tailrocks

Model IDs below use the Claude names `claude-haiku-5-5`, `claude-sonnet-5-5` and `claude-opus-5-5`. Adjust the option list to what the fleet really runs.

### Hierarchy
Four levels using native sub-issues. The first two live in a central repo (`tailrocks/<planning repo>`). L3 and L4 live in the repo whose code changes.
- **L1 Epic** (one outcome, 1-3 milestones) - title `[E3] ...`
- **L2 Task** (a deliverable, a set of PRs) - `[E3.2] ...`
- **L3 Sub-issue** (one PR, one agent session; the unit an agent executes) - `[E3.2.4] ...`
- **L4 Sub-sub-issue** (optional: one step that needs its own checkable result, such as a spike, a decision, a migration; or a split-off when a weak model fails L3) - `[E3.2.4a] ...`

Rules:
- Only L3 and L4 get assigned to agents. L1 and L2 only track.
- Never go deeper than 4 (GitHub allows 8).
- A leaf that fails twice is split into L4s instead of escalating the model first, unless it is an architecture issue.

### Project fields (exact options)
| Field | Type | Options |
|---|---|---|
| Status | single select | Backlog / Ready / In progress / In review / Blocked / Done / Dropped |
| Level | single select | L1 Epic / L2 Task / L3 Sub-issue / L4 Sub-sub-issue |
| Phase | single select | one option per milestone: `M0 Foundations`, `M1 <plain outcome>`, ..., `Backlog` |
| Kind | single select | feature / test-first / mechanical / design-first / decision / owner-input / docs / spike / release-gate |
| Size | single select | S (up to half a day) / M (1-2 days) / L (several days; split before starting if possible) |
| Risk | single select | low / medium / high (high = security, data loss, migration, CI/release path, public API) |
| Impl agent | single select | one option per fleet role, e.g. mechanic / systems-engineer / frontend-engineer / docs-writer / perf-engineer / human |
| Impl model | single select | claude-haiku-5-5 / claude-sonnet-5-5 / claude-opus-5-5 / human |
| Impl effort | single select | low / medium / high / xhigh / max |
| Review agent | single select | code-reviewer / test-reviewer / security-reviewer / none |
| Review model | single select | claude-haiku-5-5 / claude-sonnet-5-5 / claude-opus-5-5 / human |
| Review effort | single select | low / medium / high / xhigh / max |
| Attempts | number | count of failed agent runs; 2 triggers split-or-escalate |
| Start, Target | date | copied from the milestone (roadmap layout only) |

Built-ins on: Parent issue, Sub-issues progress, Milestone, Labels, Repository, Linked PRs, Assignees.
Iteration field: add `Sprint` only if the fleet runs in time-boxed batches. Otherwise skip it.

### Views (save filter, group, sort on each)
1. **Ready queue** - table, filter `level:L3,L4 is:open -is:blocked -label:blocked status:Ready`, group by Impl model, sort by Phase then Size. This is the dispatcher's input.
2. **Board** - board, filter `is:open`, columns Status, group by Phase.
3. **Roadmap** - roadmap, filter Level in (L1, L2), group by Phase.
4. **By model and effort** - table, group by Impl model, sort by Impl effort, shows Review model and effort.
5. **Blocked and stuck** - table, filter `label:blocked OR Attempts>=2`.
6. **Needs human** - table, filter `Kind:owner-input OR Kind:decision OR Impl model:human`.
7. **In review** - table, `Status:In review`, group by Review model.
8. **High risk** - table, `Risk:high`, sort by Phase.
9. **Epic tree** - table, filter Level L1/L2, columns Sub-issues progress and Milestone.

The project README says: how levels work, how to find the next task (view 1), where the plan source file lives, the escalation rules, and a link to the model routing doc.

### Milestone scheme
- One milestone per **phase gate**, same name in every repo: `M0 Foundations`, `M1 <outcome>` ... `M<n> Release`, and `Backlog` (no date).
- Description: `<one-sentence theme>. Exit: <observable check>; tag <x>.` Add `Starts only when M<k> is done` where ordering matters.
- Due dates are set and spaced about 2-4 weeks apart, so the roadmap layout and the "lowest open milestone" rule work.
- Exit criteria are an executable check on the L1 epic, not only the milestone text.
- The Phase project field mirrors the milestone name (generated, not hand-edited).

### Label scheme (prefix:value, no space; one script syncs them to all repos)
- `level:L1|L2|L3|L4` (redundant with the field on purpose, because label filters work in `gh` and issue search).
- `kind:feature|test-first|mechanical|design-first|decision|owner-input|docs|spike|release-gate`
- `model:haiku|sonnet|opus|human` (implementer model only; the reviewer is in fields and body)
- `size:S|M|L`, `risk:low|medium|high`
- `area:<name>`
- `state:blocked|needs-owner|needs-decision|ready-to-merge|deferred|dropped`
- `source:review|incident|audit` plus a finding ID in the body
- `plan:<plan-key prefix>` instead of a catch-all tag. Do not label everything with one tag.

Use issue types (Task, Bug, Feature) for the nature of the change. Do not duplicate it as a `type:` label.

### Title conventions
`[<code>] <imperative outcome, ~8-12 words>`
- `[E3] Serve every public API route from the new gateway`
- `[E3.2] Rebuild the artifact cache on the v2 storage format`
- `[E3.2.4] Serve cache entries with their expiry metadata`
- `[E3.2.4a] Add the failing test for the empty cache entry`
- Decisions: `[E3.2.D1] Decide: <question> (<option A> or <option B>)`
- Owner inputs: `[E0.1.3] Owner: <action>`

Epic and phase titles state an observable end state.

### Parent-issue template (L1 / L2)
```
## Goal
<one sentence: the observable end state>

## Exit check
1. Run `<command>`. Expected: exit 0 and `<line>`.
2. ...

## Source of truth
<path to the spec file in the repo and section>

## Depends on
- #<n> (<code>)

## Sub-issues
(generated list: code, title, repo, count of open leaves, status)

## Sequencing
<ordered list of codes, with parallel groups marked>

## Done when
Every sub-issue is closed and the exit check passes.

---
Plan key: `E3.2` | Level: L2 | Parent: #<n> | Path: E3 > E3.2 | Blocked by: #<n>
<!-- plan-key: E3.2 -->
```

### Leaf-issue template (L3 / L4), written so a weak model can execute it
Keep it under ~8 KB. Link shared rules instead of pasting them.
```
## Summary
<one sentence end state> Proof: `<command>` exits 0 and prints `<line>`.

## Goal
<why this exists, what the next issue relies on, in 2-4 sentences>

## What exists today
- `<path>:<lines>`: <what is there, name of the function>
- Missing: <what is not there>

## Terms
- <word>: <meaning in this issue>

## Read first
- `<path>` lines <a-b>

## Test first            (omit with "none: <reason>" for mechanical/docs)
- File: `<path>`; test names: `<name>`
- Copy these literals/fixtures: `<...>`
- Command: `<cmd>`
- Expected failing output before the code: `<line>`

## Steps
1. Make the worktree.
   a. Run: `<cmd>`   Expected: exit 0.
2. <one action>.
   a. Run: `<cmd>`   Expected: `<exact output>`.   Reason: <why>
3. Run the gates: `<cmd>`. Expected: exit 0. After three failed runs of one gate go to "If blocked".
4. Commit (signed off, conventional message), push, open the PR with `<exact command and flags>`; the PR body ends with `Refs #<issue>`.

## Decisions already made
- <decision> _(source: <doc>/plan default)_

## Scope
- Files you may change: `<paths>`
## Out of scope
- <thing> (belongs to <code>)

## Acceptance
- Run `<cmd>`. Expected: exit 0 and `<line>`.

## Done when
A ready PR exists, gates pass on its head commit, reviewer reports no blocking finding, the completion comment is posted. The executor does not merge.

## If blocked
Do not ask anyone and do not edit a test to make it pass.
1. Push the branch if it has commits.
2. Comment: the failing command, last 40 output lines, what you tried, the step number.
3. Add label `state:blocked`, increment Attempts, leave the issue open, stop.
Next issue that can start meanwhile: <code>.

## Completion summary (post as the PR/issue comment)
1. Commands run with exit codes. 2. Red and green output. 3. Files changed. 4. PR link and head commit. 5. Three lines: what changed, defaults applied, what the next issue needs.

## Execution profile
| Role | Agent | Model | Effort | Escalate to |
|---|---|---|---|---|
| Implement | mechanic | claude-haiku-5-5 | medium | claude-sonnet-5-5 @medium (relabel, record why) |
| Review | code-reviewer | claude-sonnet-5-5 | high | claude-opus-5-5 @high |
Why: <one line>. Risk: low. Size: S.

## Depends on / Blocks
- #<n> (<code>)

---
Plan key: `E3.2.4` | Level: L3 | Parent: #<n> | Path: E3 > E3.2 > E3.2.4 | Blocked by: #<n>
<!-- plan-key: E3.2.4 -->
```
Writing rules for weak-model leaves:
- One action per step.
- Every step has `Run:` and `Expected:`.
- Use literal paths and names, never "the relevant file".
- No cross-references the agent has to resolve. Paste the three facts it needs.
- A leaf that needs a design choice becomes a `design-first` or `decision` issue first.

### Decision-issue shape
```
## Question
<one question>

## Options
- A: <option> - for / against
- B: <option> - for / against

## Recommendation
<option and one-line reason>

## What changes once decided
Unblocks <codes>. Acceptance: the choice is recorded in <decision log path>.

Needs the owner. Agents prepare the decision only.
```
Exit check for a decision: the log entry exists, the issue is closed with a link to it, and the blocked issues are unblocked.

### Recording "implementation model + effort" and "review model + effort"
**Both, with a defined source of truth.**
- **Project fields are the source of truth** (Impl agent, Impl model, Impl effort, Review agent, Review model, Review effort). They drive views, the dispatcher and cost reports, and survive edits without parsing text.
- **The body "Execution profile" table is generated from the fields** by the plan sync script. The agent reading only the issue text sees its own model and effort, the escalation rung and the reviewer, and a diff of the issue shows when routing changes. Do not hand-edit either side.
- **Labels carry only `model:<tier>`** for fast `gh issue list --label` filters and for the plan-key footer. Effort and reviewer live in fields only.
- **Escalation is recorded, not overwritten silently.** When an issue is re-routed, change the field, bump `Attempts`, and add a comment such as `Escalated implementer sonnet@high -> opus@high: <reason>`.
- **Default review rule:** the reviewer is a different role than the implementer and, where available, a different model family or at least a different tier. Review effort is at least the implementer effort for `Risk:high`.
- **Default mapping to start from:**
  - mechanical, release-gate, docs from a template: haiku @medium
  - feature and test-first: sonnet @high (medium for size S)
  - design-first and high-risk: opus @high, with a design doc reviewed before the sonnet implementation issue is Ready
  - decisions and owner inputs: human

### Escalation ladder
1. Start at the rung in the Execution profile.
2. On a failure, first rule out missing context (a missing path, term or fixture). Fix the issue text and retry at the same rung.
3. After a failure with context ruled out, escalate one rung (same model higher effort, or the next tier). Record the reason in a comment and bump `Attempts`.
4. At `Attempts` = 2, split the leaf into L4 issues before escalating again, unless it is an architecture issue.
5. The top tier needs explicit owner approval, except for design-first issues that produce a design doc.
6. If the top rung fails, follow "If blocked" and route to the "Needs human" view.
