# PR92 journal-schema correction — current-main handoff v2

This packet supersedes `CURRENT-MAIN-HANDOFF.md`. The first version checked `PRAGMA table_info` metadata but missed SQLite's `id INTEGER PRIMARY KEY DESC` exception: it reports the expected declared type and primary-key ordinal while not creating the rowid alias used by `last_insert_rowid`. The v2 validator also inspects `PRAGMA index_list(intents)` and rejects an explicit primary-key index, closing that row-identity gap.

The source base is protected `main` commit `aae1ff63b492d33b7faee218c51799257664b4b1`, tree `8abaab8321b16820415e0dd39c86e2d003c39037`; PR #92 merged as `7be34b741e7b5d0e2dbe1a1577240a8717fdfc9c` without changing the affected validator. The v2 source is `/root/.cache/velnor-task-targets/20261005/pr92-schema-fix-7be34/merged-main-tree-v2`, an exact `git archive` of that current-main tree with the narrow patch applied. It contains no `.git` directory and was not applied to the shared checkout.

The validator checks the nine required `intents` columns by case-insensitive name and validates declared type, nullability, default, and primary-key ordinal from `PRAGMA table_info`. It then rejects explicit primary-key indexes so `id INTEGER PRIMARY KEY DESC` cannot pass as a rowid alias. Validation occurs before legacy row updates or user-version changes. Existing `dind_id` and `worker_volume` legacy migrations remain supported. New regressions check malformed version-0 schemas including the DESC rowid-alias evasion, missing primary key, wrong types, nullable state, and wrong/missing cleanup default; `Journal::open` must reject without changing SQL, metadata, rows, or `user_version`. Additional cases reject malformed current-version metadata without mutation and prove distinct inserted IDs across reopen.

Patch: `/root/.cache/velnor-task-targets/20261005/pr92-schema-fix-7be34/pr92-schema-metadata-fix-v2.patch`, SHA-256 `483b2b831402ab76f5ef7e716a6d785d4a5e06c1b5822971cfdfe5e8295ab5fe`. `git apply --check` passed against a fresh archive of exact `aae1ff63`; the applied source postimages are recorded in `current-main-v2-source-files.sha256` (SHA-256 `9b27d9350c00862269811d69956f7e0a1ee71fae0e00e0b6e45265dfcff6b1c6`):

- `journal/schema.rs`: `1284b155eaf5861526f8955b56e8ec7226259a3caf2e066cab98d9aaedd9804f`
- `journal.rs`: `6982a99601689dedab8bbf75eb5bff1bf14fb57a9fd6953eebec9bde1ce85c57`
- `journal/schema_metadata_tests.rs`: `b495a589d5efadb18df961a336199a9d689a5c8ab36786eee2c49cf67cfdabb2`

The full source manifest `merged-main-tree-v2.files.sha256` (1,617 regular files) has SHA-256 `371d1e8013514092d5b4484c69d1e9a416c7e87812d60eef4d2b6ebc5b38696f`; the seven-link target list `merged-main-tree-v2.symlinks.txt` has SHA-256 `276149f267286f14e13a746b9873bfb5cde68bcb7934093661be6c5511cbaa22`. All regular files verify (`merged-main-tree-v2-manifest-check.log`, SHA-256 `99f70650c1e1f2fc5080826572e447a0c80171cb3f5e6b4cec09b372f10f006a`).

Validation used Rust/Cargo 1.99.0, a private target directory, `CARGO_BUILD_JOBS=1`, and `--locked`:

- Focused schema metadata regressions: 3 passed (`rowid-alias-focused.log`, SHA-256 `0a2201c54e2fda8cb4c8a29950631f0b9229344aff49fd55d5f147650d5ec21b`).
- Full `velnor-runner-host --lib`: 164 passed, 0 failed (`rowid-alias-full-host-tests.log`, SHA-256 `436df6e953ec50d4bc1aef59ffbaf8473c36d5c708a7a8421bdbf8518c011138`).
- Strict library Clippy: passed (`rowid-alias-lib-clippy.log`, SHA-256 `4cf86278c1e6d659943a46a3d7e3da429ac15be90f0f17742e24ef85228e7d3e`).
- rustfmt on the three changed files: passed (empty `rowid-alias-rustfmt.log`, SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`).
- Strict all-target Clippy still reports 25 existing `clippy::assert_is_empty` errors in untouched host test files (`rowid-alias-all-targets-clippy.log`, SHA-256 `ff67d04141ea803f6bcf2767af32978d7bc69aab30eb6835ff491df3ed53800e`). These are separate mechanical cleanup; no lint allowance was added.

No shared checkout, Git index, branch, commit, or PR was modified. V2 is ready for independent review and steward integration against the exact current-main preimages above; refresh the main head and preimages before applying if main has moved.
