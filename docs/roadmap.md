# Working on the roadmap

This file is the process for the fast Rust CI roadmap, issue [#574](https://github.com/tailrocks/velnor-new/issues/574). It says where to start, what to follow and when to stop. What the work must achieve is in [the goals](goals.md). What each task builds is in its issue.

## Start here

Open Claude Code in this repository in auto mode and paste one of these.

Work through the roadmap:

```text
/goal Work through the roadmap of this repository by following docs/roadmap.md. Stop when the ready-issue query in that file prints no issue that can be worked on without the owner, or after 40 turns. After each issue, print the output of the query and one line for the issue: its number, its pull request, whether it merged, and the command and output that prove each "Done when" box. Never merge with a failing or missing required check. Never use --admin or a force push. Never change docs/goals.md or docs/hard-decisions.md. Ask before any step that an issue marks "Needs the owner".
```

Finish one issue (replace the number):

```text
/goal Finish issue #583 of this repository by following docs/roadmap.md. It is finished when its pull request is merged, the issue is closed, and the command and output that prove each "Done when" box are printed here. Stop after 25 turns. Never merge with a failing or missing required check. Never use --admin or a force push. Ask before any step that the issue marks "Needs the owner".
```

A goal is judged from what is printed in the session, so print the proof. Do not only say that it passed.

## Where the state is

The state of the roadmap is in GitHub. There is no progress file.

| Question | Where the answer is |
|---|---|
| Is an issue done? | It is closed |
| What blocks an issue? | Its "blocked by" relationships. The same list is at the end of its text |
| Which model runs it? | Its labels `exec:haiku`, `exec:sonnet` or `exec:opus`, and `effort:low`, `effort:medium` or `effort:high`. The reviewer is named at the end of its text |
| Does it need the owner? | The label `needs-owner`, and the line "Needs the owner" at the end of its text |
| Is somebody working on it? | The label `in-progress` |
| What did an experiment decide? | The table in issue [#586](https://github.com/tailrocks/velnor-new/issues/586) |

This query prints every issue that can start: open, not a parent, nothing open blocks it, and nobody is working on it.

```sh
gh api graphql --paginate -f query='query($endCursor:String){repository(owner:"tailrocks",name:"velnor-new"){issues(first:100,after:$endCursor,states:OPEN,labels:["fast-ci"]){pageInfo{hasNextPage endCursor} nodes{number title labels(first:20){nodes{name}} subIssuesSummary{total} blockedBy(first:50){nodes{state}}}}}}' --jq '.data.repository.issues.nodes[] | select(.subIssuesSummary.total==0) | select([.blockedBy.nodes[]|select(.state=="OPEN")]|length==0) | select([.labels.nodes[].name]|index("in-progress")|not) | "#\(.number) \([.labels.nodes[].name|select(test("^(exec|effort):|^needs-owner$"))]|join(" ")) \(.title)"'
```

## The loop

1. Read [the goals](goals.md) and this file.
2. Run the query. From the issues it prints, pick first the one that blocks the most other issues.
3. Run at most three issues at the same time. Two issues run together only when they change different top-level folders (`generator/`, `ci-helper/`, `local/`, `crates/`) or different repositories.
4. For each issue, follow "One issue" below.
5. Run the query again and continue. Stop when it prints nothing that can be worked on without the owner, and say which issues wait for the owner and for what.

## One issue

1. **Read and claim.** Read the issue in full, the issues it depends on, and the rules for pull requests in the root issue #574. Add the label `in-progress` to the issue.
2. **Delegate.** Start one subagent as the executor, with the model and effort from the issue's labels, in its own git worktree on a branch named `fast/<issue number>-<short name>`. Give it the issue number and this file. Ask it for a short report: what it changed, the proof for each "Done when" box, and anything it could not do.
3. **Build and prove.** The executor makes the change and runs every "Done when" check. A check that names `velnor-lab` follows rule 4 of the root issue. If a check cannot be met, the executor stops and posts the evidence as a comment on the issue. It does not widen the task to make the check pass.
4. **Open the pull request.** One issue is one pull request. The description starts with why, states the measured before and after or "no timing effect expected" with the reason, shows the command and output for each "Done when" box, and ends with `Closes #<number>`. It ends with `Refs #<number>` instead when a box needs a real run on `velnor-lab` after the merge.
5. **Review.** Start a second subagent as the reviewer, with the model and effort named at the end of the issue. Give it the issue and the diff, not the executor's report. It checks the diff against the issue, against [the goals](goals.md) section "How to review a change", and reruns the "Done when" checks it can.
6. **Answer every comment.** Reviewer findings and review bot comments are fixed, or rejected with a reason, before merging. `AGENTS.md` has the full rule.
7. **Merge and finish.** Squash-merge when the required check is green and no review thread is open. When a box needs a real run on `velnor-lab`, do it as rule 4 of the root issue describes and post the numbers on the issue. Close the issue when every box is proven, and remove the label `in-progress`.
8. **Record.** An experiment writes its decision into issue #586. A changed plan is written into the issues it affects, in the present tense.

The subagent models are `haiku`, `sonnet` and `opus`. Use the model the issue names. Use a stronger one only after a failure, and say so in the pull request.

## When to stop and ask the owner

- A step that the issue marks "Needs the owner".
- A change to [the goals](goals.md) or to [the hard decisions](hard-decisions.md).
- Anything that costs money, such as paid storage or larger runners.
- Deleting branches, tags, releases, caches outside `velnor-lab`, or anything else that cannot be undone.
- An issue whose "Done when" cannot be met after one honest attempt and one retry with a stronger model.

Ask with the question, the evidence and a recommendation. Then continue with the issues that do not depend on the answer.

## What "done" means

The roadmap is done when issue #574 is closed: every issue under it is closed, and the targets in [the goals](goals.md) are shown with measurements on `tailrocks/velnor-lab` and on this repository.
