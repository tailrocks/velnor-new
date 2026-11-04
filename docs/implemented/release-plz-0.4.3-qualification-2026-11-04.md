# release-plz 0.4.3 release-job qualification

The GitHub tag `release-plz-v0.4.3` resolves through annotated tag object
`8375dd600aa8dc28fe6ee77013851d8cf220b79f` to commit
`92e907a14715182c99c26eb734a4b3d7b3775a0b`. GitHub reports that the tag is
unsigned. The crates.io metadata reports crate checksum
`69bbc232e23322cfaf77a60540e69e2dbd83879e843c4fc9d6ad1ead97d257f6`.
This crate checksum is not an executable checksum.

On 2026-11-04, the archive SHA-256 values published by the GitHub release API
matched independently downloaded release assets. The extracted executable
SHA-256 values were measured from those archives:

| Target | Archive SHA-256 | Executable SHA-256 |
| --- | --- | --- |
| Linux x86-64 musl | `4a4cfa92ea2abaee02b40daa596a95ca2725816a6a32cb03a8f48ddead287681` | `678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466` |
| macOS arm64 | `b2c6e636199c7fa9dae8838fbebb49cb99933908eb7dd67f25ec831bb38e1c52` | `d725885957dfe31407a88a5eed28b7e402b47bd0abdff3f5655c896d7a03d723` |

The macOS executable reported `release-plz 0.4.3`. A local Git repository and
mock GitHub API exercised that executable with a release-PR ancestor and with
no associated PR. With `release_always = true` and `pr_branch_prefix = ":"`,
the release dry run retained the checked-out source in both cases. A control
using the normal prefix checked out the ancestor. Git rejects `:` in a branch
name. These tests used `git_only` and did not upload a crate, obtain an OIDC
token, or exercise a GitHub-hosted Linux runner.

The generated Linux acquisition shell was exercised in an isolated container
with no external network. A loopback TLS proxy served the downloaded official
archive. Acquisition accepted the original, rejected a valid gzip with altered
header bytes, and rejected an archive with the wrong executable. It also
rejected archives with duplicate or traversal entries when the test substituted
their archive hashes to exercise the later checks. No downloaded binary was
executed. Those fixture results do not establish publisher identity or source
provenance beyond the recorded GitHub API and asset observations.

Sources: [release tag](https://github.com/release-plz/release-plz/releases/tag/release-plz-v0.4.3),
[GitHub release API](https://api.github.com/repos/release-plz/release-plz/releases/tags/release-plz-v0.4.3),
[crates.io metadata](https://crates.io/api/v1/crates/release-plz/0.4.3).
