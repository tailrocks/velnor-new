# Scheduler/seeds preservation packet

This packet is a byte-preserving archive of the 19 dirty scheduler/seeds worktrees from the registered 24-worktree inventory. Original worktrees, Git indexes, refs, and commits were read only.

- Source root: `/private/tmp/velnor-*` worktrees listed in `worktrees.tsv`.
- Remote reference snapshot: `/private/tmp/velnor-release-cleanup-ls-remote-20261006.txt`.
- Remote manifest SHA-256: `1050218d04dcd56791869057c9a038a3729eb631fd6a554bcff79a39eff2a0bc`.
- Current main: `ba13b691459310fbc99cbeb857940f60993cd3f7`; tree `bd9a4c9c93f0e7c674b2efa8643054e416f4ab0b`.
- Frozen R1 checkpoint: `dd93ee82335013eabc66fef7d7b1376bb8f528ae`; tree `3ca41fc604c3a11d239973b8baf7e76b35a3e0c6`.
- Guest-R1 work branch in the same manifest: `508533c8a5b698125e2d7ac057b60df56c968bff`; tree `bd91eaeeaf0f77621a82bca47be43ec9dabab3be`.
- The seven detached HEAD commits outside every branch tip in the supplied manifest are listed in `uncovered_heads.tsv`.

## Layout

Each source worktree has a directory named by its basename. `worktree/<original-path>.source` contains current working-tree bytes for every changed or untracked file that exists. `index/stage-N/<original-path>.source` contains each changed path's index blob at its recorded stage. Symlinks are stored as regular `.source` files containing the exact link-target bytes; their original Git mode is recorded in the manifest. Deletions remain manifest rows with no worktree archive path and with any surviving index stages preserved.

`manifest.tsv` has one row per status path. `index_blobs.tsv` has one row per index stage/blob. `worktrees.tsv` records HEAD, tree, parents, detached branch state, merge bases, and upstream. `uncovered_heads.tsv` records all seven unique detached commits not covered by the supplied remote refs.

Only source/test Rust files and Markdown documentation were present in the changed-path inventory. No logs, databases, target directories, credentials, or generated artifacts were copied. Worktree status, HEAD/tree, index, and file identities were checked again after copying. Every archived worktree file and index blob was hash verified.

This packet is evidence for Release to preserve/commit. It does not replace the source worktrees or authorize their deletion.
