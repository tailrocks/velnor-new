# Local cache_prune.rs diff in the main checkout: superseded

Verdict: drop it. origin/main (3797db0eb5) already has the same behavior, done more carefully.

Evidence
- d613fb0007 "keep checkout path libraries in the saved base (#2804)" is an ancestor of origin/main. It touches
  crates/velnor-ci/src/cache_prune.rs and keeps used `path+` library units, their fingerprints and their
  build-script outputs. Its stated purpose is the same as the local diff: no "compiled despite usable
  restored inputs" waste for unchanged path crates.
- `git apply --check` of the local diff against origin/main fails in all three files (cache_prune.rs,
  tests/it/cache_prune.rs, tests/it/cache_prune_layout.rs): the code it edits (`is_dependency`) no longer exists.
- Main is also better than the local diff: it keeps only library units whose manifest is inside the checkout
  (`keep_library`, `manifest_in_checkout`, `outside_path_names`) and still prunes path executables and
  test binaries. The local diff keeps every `PackageIdentity`-parsable unit, executables and path
  units from outside the checkout included, so it would grow the base for no gain.
- Waste check history: 22e4531cdf (#2239) already limits "compiled despite restored inputs" to units the restored
  base held, so a pruned base is judged consistently with what the save kept.

Action: `git -C <main> checkout -- crates/velnor-ci` (user's call; I did not touch the main checkout).

# agent-ada6d9b759ccf8b56 mise.lock (+12 lines): mise auto-lock artifact

The diff adds `unsupported_platforms = ["macos-x64", "macos-x64-baseline"]` to release-plz, hk and
mr-boxington. mise 2026.11.13 writes this when it locks and finds no macos-x64 asset. Main's mise.lock
has no macos-x64 section for those tools and no `unsupported_platforms` key anywhere, so nothing needed is
missing. Discard it (`git checkout -- mise.lock` in that worktree).
