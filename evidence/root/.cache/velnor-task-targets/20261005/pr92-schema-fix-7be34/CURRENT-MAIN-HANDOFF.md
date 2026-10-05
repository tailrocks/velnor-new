# PR92 journal-schema correction — current-main handoff

The defect was found in PR #92 at head `7be34b741e7b5d0e2dbe1a1577240a8717fdfc9c`. That PR merged without changing the affected schema validator. Current `main` was rechecked at `aae1ff63b492d33b7faee218c51799257664b4b1`, tree `8abaab8321b16820415e0dd39c86e2d003c39037`, parent `7a55272ad2a6b829049c6d5ba85d48fb308c018a`. The merged tree matches the reviewed PR head tree. GitHub REST recheck returned the same current main SHA immediately before this packet was frozen.

The validator now checks all nine required `intents` columns by case-insensitive name, rejects extra/missing columns, and compares SQLite `table_info` declared type, nullability, default, and primary-key ordinal before legacy-row updates or version changes. It preserves the existing migration of `dind_id` and `worker_volume`. New tests seed all-name malformed version-0 schemas (missing primary key, wrong ID/state types, nullable state, wrong/missing cleanup default), verify `Journal::open` rejects them without changing table SQL, column metadata, rows, or `user_version`, reject malformed current-version metadata without mutation, and check distinct IDs across reopen.

The patch is `/root/.cache/velnor-task-targets/20261005/pr92-schema-fix-7be34/pr92-schema-metadata-fix.patch`, SHA-256 `3a28217a4b532bdf9f591de2361cd696fe15b2cf3fef6dcf9d3ac7718293ae5b`. `git apply --check` passed against a fresh archive of current `main`; the patch was then applied to that archive and the host-library suite was run there. Postimage source manifest is `current-main-source-files.sha256`, SHA-256 `442436da3386a47ea3d61390ec6ad8ecf2a5186f9443ff50256c99d39dc12d53`.

The reusable patched source archive for follow-on mechanical fixes is `/root/.cache/velnor-task-targets/20261005/pr92-schema-fix-7be34/merged-main-tree`. Its complete manifest covers 1,617 regular files (SHA-256 `3518f82a52a8a1cfae6bc92818f4ac1f22832c842dedb432c4296d71a416a276`) and 7 symlinks (target-list SHA-256 `276149f267286f14e13a746b9873bfb5cde68bcb7934093661be6c5511cbaa22`); all 1,617 regular-file checks pass (`merged-main-tree-manifest-check.log`, SHA-256 `99f70650c1e1f2fc5080826572e447a0c80171cb3f5e6b4cec09b372f10f006a`).

Validation used Rust/Cargo 1.99.0, `CARGO_BUILD_JOBS=1`, a private target directory, and `--locked`:

- `cargo test -p velnor-runner-host --lib`: 164 passed, 0 failed. Raw log SHA-256 `12bafde34477bcfc01b9b6564c061b5147236ab1111d600f33fa3f2ff04674d7`.
- `cargo clippy -p velnor-runner-host --lib -- -D warnings`: passed. Raw log SHA-256 `2da0b2960301457b452fed588332e759f1353820d0c345e3bd36b5adad390409`.
- rustfmt check on all three changed files: passed (empty log SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`).
- Strict all-target Clippy reaches 25 existing `clippy::assert_is_empty` errors in unrelated, unchanged host test files under Clippy 1.99.0. The diagnostics are preserved in `clippy.log` (SHA-256 `d67f53cc38e5757acb6ea5683b6a4f5bfd74dce3be2871920a78d32fb229d282`); none refer to the schema implementation or new tests. This mechanical baseline cleanup is separate from this fix.

No shared repository, Git index, branch, commit, or PR was modified. This packet is ready for independent Sol review and steward integration as a narrow current-main correction.
