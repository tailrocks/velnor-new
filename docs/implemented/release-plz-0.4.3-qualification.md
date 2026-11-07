# release-plz 0.4.3 release-job qualification

The GitHub tag `release-plz-v0.4.3` resolves through annotated tag object
`8375dd600aa8dc28fe6ee77013851d8cf220b79f` to commit
`92e907a14715182c99c26eb734a4b3d7b3775a0b`. GitHub reports that the tag is
unsigned. The crates.io metadata reports crate checksum
`69bbc232e23322cfaf77a60540e69e2dbd83879e843c4fc9d6ad1ead97d257f6`.
This crate checksum is not an executable checksum.

Acquire the pinned upstream archives and verify both the archive and extracted
executable digests. The supported asset identities are:

| Target | Archive SHA-256 | Executable SHA-256 |
| --- | --- | --- |
| Linux x86-64 musl | `4a4cfa92ea2abaee02b40daa596a95ca2725816a6a32cb03a8f48ddead287681` | `678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466` |
| macOS arm64 | `b2c6e636199c7fa9dae8838fbebb49cb99933908eb7dd67f25ec831bb38e1c52` | `d725885957dfe31407a88a5eed28b7e402b47bd0abdff3f5655c896d7a03d723` |

Qualify acquisition in an isolated, network-denied fixture. Serve the original
archive through the controlled TLS endpoint. Require acceptance of the original
and rejection of changed archive bytes, an incorrect executable, duplicate
entries, and traversal entries. For parser-specific cases, supply the changed
archive digest so the fixture reaches the parser rather than stopping at the
outer checksum check.

Qualify source custody with the actual pinned executable, a temporary Git
repository, and a controlled external GitHub API. Cover both an associated
release-PR ancestor and no associated PR. With `release_always = true` and
`pr_branch_prefix = ":"`, require the checked-out source to remain unchanged.
Use the normal prefix as a control that exercises the ancestor checkout. Git
rejects `:` in a branch name.

A `git_only` dry run establishes source-selection behavior. It does not establish
crate publication, OIDC authentication, or native hosted execution. Archive and
executable digests identify selected bytes; they do not establish a reproducible
source build or signed publisher provenance. Record individual execution results
in the pull request or review discussion.

Sources: [release tag](https://github.com/release-plz/release-plz/releases/tag/release-plz-v0.4.3),
[GitHub release API](https://api.github.com/repos/release-plz/release-plz/releases/tags/release-plz-v0.4.3),
[crates.io metadata](https://crates.io/api/v1/crates/release-plz/0.4.3).
