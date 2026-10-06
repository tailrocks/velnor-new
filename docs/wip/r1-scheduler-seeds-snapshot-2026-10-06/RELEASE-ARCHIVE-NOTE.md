# Release preservation note

This exact packet preserves working-tree bytes, index blobs, and per-worktree
metadata from 19 dirty scheduler/seeds worktrees. It is an archive only; none
of these source variants is merged into the runner implementation or claimed
to pass build/test/review gates.

The packet was captured against the remote-ref manifest whose SHA-256 is
`1050218d04dcd56791869057c9a038a3729eb631fd6a554bcff79a39eff2a0bc` and whose
main tip was `ba13b691459310fbc99cbeb857940f60993cd3f7`. Its statement that seven
HEADs were uncovered is true for that capture. Release later added WIP archive
refs under `refs/tags/wip-preserve/r1-20261006/` for the exact clean history
leaves, including ancestry for the scheduler/seeds heads. One already-remote
null-stats head remains covered by `archive/pr91-original-head-191dce1-20261006`.

Packet aggregate SHA-256: `7a28e66cf9513a80bd9c2b531b193add6231ad10cb409ca77a00126eadc51d1a`.
Packet manifest: `manifest.tsv` (258 status rows across 19 dirty worktrees);
index inventory: `index_blobs.tsv` (238 index stage blobs); source copies carry
a `.source` suffix. The packet has no logs, databases, targets, credentials, or
generated artifacts. The source trees remain unverified for integration.
