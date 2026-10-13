<!--
PR authoring rules:
- Keep Summary to one unwrapped paragraph. Explain the shipped outcome and who benefits.
- Use concise bullets for What ships; do not include a file-by-file changelog, function inventory, or full test list.
- Keep Summary and What ships. Add Related pull requests only for coordinated changes spanning repositories.
- Add Behavior changes only for changed defaults, validation, errors, or generated workflow behavior.
- Add What this addresses only for a documented gap. Add Not included only for a concrete deferred item.
- Add Migration notes only when `.velnor/` compatibility or schemas/contracts under `crates/velnor-actions-contract/` change.
- Run `./scripts/verify-local.sh` for Rust, policy, or generated-workflow changes; it runs the repository's offline CI subset.
-->

## Related pull requests

<For a coordinated change across repositories, list each related PR link. Drop this section when the PR stands alone.>

- <https://github.com/org/repo/pull/N>

## Summary

<State the change and who benefits in one paragraph.>

## What ships

- <Describe the user or maintainer capability now available.>
- <Describe the configuration or workflow behavior that changes.>

## Behavior changes

<Include this section only when defaults, validation, errors, or generated workflow behavior changes.>

- <Describe the behavior that changes.>

## What this addresses

<Include this section only when the change resolves a documented gap or problem. Name the issue or document by title.>

- <Describe the gap and user-visible result.>

## Not included

<Include this section only when a concrete deferred item helps reviewers understand the scope.>

- <Name the deferred item and its follow-up.>

## Verify locally

<For Rust, policy, or generated-workflow changes, run the complete local verification command.>

```sh
./scripts/verify-local.sh
```

## Migration notes

<Include this section only when `.velnor/` compatibility or a contract/schema change requires operator action.>

<Describe the required action, or state that no migration is needed.>
