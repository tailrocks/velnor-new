<!--
PR authoring rules:
- Use one unwrapped paragraph per section. Describe shipped outcomes, not design rationale, a file-by-file changelog, function names, or a full test list.
- Keep Summary and What ships. Add Behavior changes only for changed defaults, validation, errors, or generated workflow behavior; add What this addresses only for a documented gap.
- Add Related pull requests only for coordinated changes across repositories. Add Not included only for a concrete deferred item.
- Add Migration notes only when compatibility changes in `.velnor/` or schemas/contracts under `crates/velnor-actions-contract/`; this repository has no `migrations/` directory.
- Add Documentation only when a change touches `docs/`. This repository has no docs-site server, so do not invent a documentation command or deployed URL.
- Run `./scripts/verify-local.sh` for code, policy, or generated-workflow changes. It is the repository's one-command validation path; no separate smoke command is needed for this CLI generator.
-->

## Summary

<State the change and who benefits in one paragraph.>

## What ships

- <Describe the user or maintainer capability now available.>
- <Describe any configuration or workflow behavior that changes.>

## Behavior changes

<Include this section only when defaults, validation, errors, or generated workflow behavior changes.>

- <Describe the behavior that changes.>

## What this addresses

<Include this section only when the change resolves a documented gap or problem. Name the issue or document by title.>

- <Describe the gap and the user-visible result.>

## Not included

<Include this section only when a concrete deferred item helps reviewers understand the scope.>

- <Name the deferred item and its follow-up.>

## Verify locally

<For changes to Rust crates, repository policy, or generated workflows, run the complete local validation. It prints `verify-local: PASS` when all stages pass.>

```sh
./scripts/verify-local.sh
```
