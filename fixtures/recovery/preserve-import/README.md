# Preserve-import quarantine

This directory holds source recovered from merged WIP branches that is not
part of the active build contract.

## `native-image-task.rs`

Recovered from `preservation/stop-20261005/heads/codex/native-image-task-20261005`
via integration PR #96. The enclosing workflow-task graph does not yet consume
this native-image declaration, so the file is retained here as provenance and
is excluded from compilation. Reintroduce it only with coherent renderer and
resolver halves.
