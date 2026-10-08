<!--
PR authoring rules:
- Use one paragraph per section; GitHub flows the text.
- Describe shipped outcomes, not a file-by-file changelog or symbol list.
- Keep design rationale in repository documentation.
- Drop optional sections that do not add useful information.
-->

## Summary

<State the change and who benefits.>

## What ships

- <Describe the user or maintainer capability now available.>
- <Describe any configuration or workflow behavior that changes.>

## Verify locally

Run the repository's complete documented validation. It prints `verify-local: PASS` when all checks pass.

```sh
./scripts/verify-local.sh
```
