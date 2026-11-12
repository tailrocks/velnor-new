# Working on the roadmap

This file is the process for the fast Rust CI roadmap, issue [#574](https://github.com/tailrocks/velnor-new/issues/574). It says where to start, what to follow and when to stop. What the work must achieve is in [the goals](goals.md). What each task builds is in its issue.

## Start here

The roadmap runs unattended. It never stops to ask a person. What only the owner can do is written down as a follow-up issue, and the run goes on.

Start Claude Code in this repository with permission prompts off, on Opus 5.5 at medium effort. Permission prompts are off, so start it inside a sandbox or a container.

```sh
claude --dangerously-skip-permissions --model opus --effort medium
```

The session that runs the loop reads issues, delegates, checks proof and merges. It writes no code, so it needs no stronger model and no higher effort. Each issue names the model that does its work.

Then paste one of these.

Work through the roadmap:

```text
/goal Work through the roadmap of this repository by following docs/roadmap.md until it is finished, without asking anyone. It is finished when the ready-issue query in that file prints nothing. After each issue, print the output of the query and one line for the issue: its number, its pull request, whether it merged, and the command and output that prove each "Done when" box. At the end, print every open issue under #574 that is an [OWNER] follow-up or carries the label blocked. Never merge with a failing or missing required check. Never use --admin or a force push. Never change docs/goals.md. Change docs/hard-decisions.md only where an issue records the owner's approval. Never spend money.
```

Finish one issue (replace the number):

```text
/goal Finish issue #583 of this repository by following docs/roadmap.md, without asking anyone. It is finished when its pull request is merged, the issue is closed, and the command and output that prove each "Done when" box are printed here, or when the issue is marked `blocked` with its evidence and that evidence is printed here. Never merge with a failing or missing required check. Never use --admin or a force push.
```

A session can end before the roadmap does. Start a new one and paste the same text: the state is in GitHub, so it continues where the last one stopped. The first thing a new session does is resume each open issue with the label `in-progress`, because the session that claimed it has ended: find its branch `fast/<issue number>-*` and its pull request, and continue from the step of "One issue" that comes next.

A goal is judged from what is printed in the session, so print the proof. Do not only say that it passed.

## Where the state is

The state of the roadmap is in GitHub. There is no progress file.

| Question | Where the answer is |
|---|---|
| Is an issue done? | It is closed |
| What blocks an issue? | Its "blocked by" relationships. The same list is at the end of its text |
| Which model runs it? | Its labels `exec:haiku`, `exec:sonnet` or `exec:opus`, and `effort:low`, `effort:medium` or `effort:high`. The reviewer is named at the end of its text |
| Does it hold a step only the owner can do? | The label `needs-owner`, and the line "Needs the owner" at the end of its text |
| What waits for the owner? | The open issues with the label `owner-action` |
| What could not be finished? | The open issues with the label `blocked`. The evidence is in their last comment |
| Is somebody working on it? | The label `in-progress` |
| What did an experiment decide? | The table in issue [#586](https://github.com/tailrocks/velnor-new/issues/586) |
| Where does the owner watch it? | The project [Velnor fast Rust CI](https://github.com/orgs/tailrocks/projects/2) and the [milestones](https://github.com/tailrocks/velnor-new/milestones) |

This query prints every issue that can start: open, not a parent, nothing open blocks it, nobody is working on it, and it is not marked `blocked`.

```sh
gh api graphql --paginate -f query='query($endCursor:String){repository(owner:"tailrocks",name:"velnor-new"){issues(first:100,after:$endCursor,states:OPEN,labels:["fast-ci"]){pageInfo{hasNextPage endCursor} nodes{number title blocking(first:1){totalCount} labels(first:20){nodes{name}} subIssuesSummary{total} blockedBy(first:50){nodes{state}}}}}}' --jq '[.data.repository.issues.nodes[] | select(.subIssuesSummary.total==0) | select([.blockedBy.nodes[]|select(.state=="OPEN")]|length==0) | select([.labels.nodes[].name]|any(.=="in-progress" or .=="blocked")|not) ] | sort_by(-.blocking.totalCount)[] | "#\(.number) blocks:\(.blocking.totalCount) \([.labels.nodes[].name|select(test("^(exec|effort):|^needs-owner$"))]|join(" ")) \(.title)"'
```

## The loop

1. Read [the goals](goals.md) and this file.
2. Run the query. The query prints the issues in order of how many issues they block, most first. Pick the first one.
3. Run at most three issues at the same time. Two issues run together only when they change different top-level folders (`generator/`, `ci-helper/`, `local/`, `crates/`) or different repositories.
4. For each issue, follow "One issue" below.
5. Run the query again and continue. The run is finished when the query prints nothing. Then print the open issues with the label `owner-action` and the open issues with the label `blocked`.

## One issue

1. **Read and claim.** Read the issue in full, the issues it depends on, and the rules for pull requests in the root issue #574. Add the label `in-progress` to the issue. Set its card on the project board to "In Progress", as "The project board" below describes.
2. **Delegate.** Start one subagent as the executor, with the model and effort from the issue's labels, in its own git worktree on a branch named `fast/<issue number>-<short name>`. Give it the issue number and this file. Ask it for a short report: what it changed, the proof for each "Done when" box, and anything it could not do.
3. **Build and prove.** The executor makes the change and runs every "Done when" check. A check that names `velnor-lab` follows rule 4 of the root issue. If a check cannot be met, follow "An issue that cannot be finished" below. The executor does not widen the task to make the check pass.
4. **Open the pull request.** One issue is one pull request. The description starts with why, states the measured before and after (the words "no timing effect expected" with the reason are allowed only for a change that touches no executable behavior), shows the command and output for each "Done when" box, and ends with `Closes #<number>`. It ends with `Refs #<number>` instead when a box needs a real run on `velnor-lab` after the merge.
5. **Review.** Start a second subagent as the reviewer, with the model and effort named at the end of the issue. Give it the issue and the diff, not the executor's report. It checks the diff against the issue, against [the goals](goals.md) section "How to review a change", and reruns the "Done when" checks it can.
6. **Answer every comment.** Reviewer findings and review bot comments are fixed, or rejected with a reason, before merging. `AGENTS.md` has the full rule.
7. **Merge and finish.** Squash-merge when the required check is green and no review thread is open. When a box needs a real run on `velnor-lab`, do it as rule 4 of the root issue describes and post the numbers on the issue. Close the issue when every box is proven, and remove the label `in-progress`.
8. **Record.** An experiment writes its decision into issue #586. A changed plan is written into the issues it affects, in the present tense.

The subagent models are `haiku`, `sonnet` and `opus`. Use the model the issue names. Use a stronger one only after a failure, and say so in the pull request.

## The project board

The project [Velnor fast Rust CI](https://github.com/orgs/tailrocks/projects/2) shows the roadmap to the owner. Its lists follow the labels and the "blocked by" links, so they need no upkeep. GitHub adds a new child issue of #574 to the project, moves a card to "In Progress" when a pull request that closes its issue opens, and moves it to "Done" when the issue closes.

One thing is set by hand. When you claim an issue, move its card to "In Progress", so the board shows the work before a pull request exists. Replace `<number>` with the issue number:

```sh
gh project item-edit --project-id PVT_kwDOBLJ97c4BrWyl --field-id PVTSSF_lADOBLJ97c4BrWylzhpp4QY --single-select-option-id 47fc9ee4 --id "$(gh project item-list 2 --owner tailrocks --limit 500 --format json --jq '.items[]|select(.content.number==<number>)|.id')"
```

When you mark an issue `blocked`, move its card back with the same command and the option id `f75ad846` ("Todo").

The board is a display, not the state. If a project command fails, go on.

## Never stop to ask

The run does not wait for a person. These three rules replace every question.

### A step only the owner can do

Some steps need what only the owner has, such as creating a GitHub App, applying organisation settings, or approving a pull request in another repository. An issue names such a step in its line "Needs the owner".

1. Try the step first when it is a command you can run, such as starting a workflow.
2. Do everything else in the issue and prove it.
3. Create a follow-up issue as a child of #574, titled `[OWNER] <what is needed>`, with the label `owner-action` and without the label `fast-ci`. It says what to do, the exact names to use (secrets, variables, files), which "Done when" box stays unproven, and the command that proves it afterwards.
4. Close the original issue with a comment that links the follow-up. The issues that depend on it go on.

### An issue that cannot be finished

1. Retry once with the next stronger model.
2. If it still fails, post the evidence as a comment: what was tried, the command and its output, and a recommendation. A target in [the goals](goals.md) that no option meets is such a case.
3. Add the label `blocked` and remove the label `in-progress`. Leave the issue open.
4. Go on with the issues that do not depend on it.

### What is never done

- A change to [the goals](goals.md). Only the owner changes them.
- A change to [the hard decisions](hard-decisions.md), unless the issue records the owner's approval of that change. Do not write such an approval into an issue yourself.
- Anything that costs money, such as paid storage or larger runners.
- A merge with a failing or missing required check, `--admin`, or a force push.
- A deletion that cannot be undone. Before deleting a branch, save it as a hidden ref `refs/keep/<branch name>`, so it can be restored. Tags, releases, and caches outside `velnor-lab` are never deleted.
- A merge in another repository against that repository's own rules. If they need a person, leave the pull request open and create an `[OWNER]` follow-up.

## What "done" means

The run is finished when the query prints nothing. The roadmap is done when issue #574 is closed: every issue under it is closed, the `[OWNER]` follow-ups included, and the targets in [the goals](goals.md) are shown with measurements on `tailrocks/velnor-lab` and on this repository.
