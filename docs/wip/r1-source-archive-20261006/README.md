# R1 source snapshot archive (2026-10-06)

Preservation-only copies of source variants present in frozen dirty worktrees
but not byte-covered by the R1 checkpoint refs `dd93ee82335013eabc66fef7d7b1376bb8f528ae`
and `508533c8a5b698125e2d7ac057b60df56c968bff` at inventory time.

These files are archived with a `.source` suffix under their original relative
paths so they are not compiled or applied to the current runner source. This
commit does not integrate or endorse either variant. No build, test, review, or
Cargo.lock resolution verification applies to these snapshots. The workspace
Cargo.toml entry is intentionally present in both snapshots; its Git blob is
the same in both and is recorded twice to retain each source worktree context.

Source worktrees at capture:
- bdb1: `/private/tmp/velnor-guest-resource-bdb1`, HEAD `bdb1e14509225d18b4d8ee180731825d0826c16f`, base tree `9e9ce5ed...`.
- e592: `/private/tmp/velnor-r1-guest-resource-e592`, HEAD `0efa83e92d4eca0001795298dcbdf49635bfd905`, base tree `e5920017acf35fe1e85c47232f719d9f4a99706a`.

`BLOBS.tsv` records each original worktree path, archive path, and Git blob
object ID. The matching workspace Cargo.toml blob is deduplicated by Git object
storage but retained in both archive directories.
