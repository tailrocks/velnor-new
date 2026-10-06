# Preserved WIP source snapshot: renderer compaction (44b2)

This snapshot preserves an isolated Velnor source candidate as-is. It is an artifact under `preservation/`, not an integration into product source.

- Source worktree: `/tmp/velnor-render-compaction-44b2-20261005`
- Source HEAD/base: `44b2c380cdd65b361beab8ced3332319e7f56c73`
- Preservation branch base: `aae1ff63b492d33b7faee218c51799257664b4b1`
- Candidate status before copy: modified `render.rs`; three new shared-script module/test files.
- No tests or builds were run for this preservation checkpoint. Existing review/test packets remain at their original local paths.
- This snapshot predates the current main source and is retained for recovery/reference only; it makes no integration or qualification claim.

Exact copied file hashes (SHA-256):

- `crates/velnor-actions-workflow-renderer/src/render.rs`: `e950bab0300704d710e55a5de383d9a9bddfaed4d33c249a362c7dc1a13acf13`
- `crates/velnor-actions-workflow-renderer/src/document_shared_scripts.rs`: `a9bc39db922b6b0858a46ec9f71981618860f91bb40032a7d8b35975f7ed5793`
- `crates/velnor-actions-workflow-renderer/src/document_shared_scripts_execution_tests.rs`: `216834fa9f38a9fde5659edad740608f94770dcbbecfcf8e93969f7cf9578e34`
- `crates/velnor-actions-workflow-renderer/src/document_shared_scripts_tests.rs`: `22b8042e1a1448f36af5afd21cbf2f4aca3cf42e7f7e6a69e3784035ed6c2b3c`
