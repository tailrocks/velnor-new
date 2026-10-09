# Registration target-boundary evidence — 2026-10-10

**Disposition:** This is a record of observed test, gate, and upstream-policy
evidence for PR #125. It identifies no root cause and does not qualify a
release, accept a release, or adopt any newer pin. The root coordinator and
researcher are still validating scope; this note does not settle that review.
No source, version pin, or hold was changed for this record.

## Candidate and hosted evidence

[PR #125](https://github.com/tailrocks/velnor-new/pull/125) was open at the
read-only API snapshot. Its head is
[`6cdb3c60eb6c930d113c6373de01afafad61a831`](https://github.com/tailrocks/velnor-new/commit/6cdb3c60eb6c930d113c6373de01afafad61a831),
tree `c863f0047244757dd83f3c6dc9e2beece32058d4`.

Hosted CI run
[`37988039985`](https://github.com/tailrocks/velnor-new/actions/runs/37988039985)
completed successfully on that head. The check snapshot had no failures and
one skipped `Publish baseline` check. DCO was a separate successful external
check. The [`Rust / velnor-actions-cli` job](https://github.com/tailrocks/velnor-new/actions/runs/37988039985/job/114015641796)
ran 316 tests with 316 passing and 0 skipped, including
`impl_repo_test_registration::cargo_targets_register_every_test_bearing_source`.
The [hosted Alint job](https://github.com/tailrocks/velnor-new/actions/runs/37988039985/job/114014845420)
passed; its four findings were informational notices.

A separate six-gate verification snapshot reported:

| Gate | Command/result |
|---|---|
| Formatting | `cargo fmt --all -- --check` — pass |
| Clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass |
| Workspace tests | Pinned Nextest 0.9.148, `cargo nextest run --locked --workspace` — pass, 3,645 passed and 2 skipped |
| Alint | `alint validate-config` — pass; `alint check --fail-on-warning` — pass, 4 informational findings only |
| Cargo Deny | Pinned `cargo deny check` — pass |
| Freshness | `bash scripts/check-freshness.sh` — fail: stale evidence for 15 non-held subjects |

The 15 stale non-held evidence subjects reported by the check were `mise`,
`gh`, `actionlint`, `shellcheck`, `zizmor`, `nextest`, `opentofu`,
`release-plz`, `actions/cache/restore`, `actions/cache/save`,
`actions/checkout`, `asamarts/alint`,
`aws-actions/configure-aws-credentials`, `jdx/mise-action`, and `runner`.
These gate results are a separate snapshot from the later `verify-local.sh`
recurrence below; they are not a summary of that wrapper's substages.

## Separate `verify-local.sh` recurrence

The recorded wrapper invocation exited 1 with `repo-policy` and `integration`
failed. In the integration log, the strict atomic metadata test
`impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree` failed
after one `NotFound` observation:

```text
observer=2 iteration=247159 writer_phase=rewrite_5_in_progress
path=/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/.tmp2CVLpH/.github
kind=NotFound raw_errno=Some(2)
```

The same diagnostic line records a successful post-error parent probe and a
successful post-error target probe; both reported `is_dir=true` and
`is_symlink=false`. The test summary was 2,119 of 3,645 tests run: 2,118
passed (one slow), 1 failed, and 2 skipped; 1,526 tests were not run after
failure. The registration test also passed in this run, but this incomplete
workspace run is not a clean full-suite result. The later probes do not
identify why the strict observation returned `NotFound`; no root cause is
claimed.

## Upstream deltas and active holds

The copied `check-upstream` output records these source observations at
`2026-10-09T20:47:07Z`. These are detected upstream versions, not proposed
or adopted repository pins.

| Subject | Local pin → reported latest | Source |
|---|---|---|
| Mise | `2026.10.4` → `v2026.10.6` | [Mise latest release](https://api.github.com/repos/jdx/mise/releases/latest) |
| Rust | `1.98.1` → `1.99.0` | [Rust stable channel](https://static.rust-lang.org/dist/channel-rust-stable.toml) |
| MBX (`mr-boxington`) | `1.21.1` → `v1.23.0` | [MBX latest release](https://api.github.com/repos/jdx/mr-boxington/releases/latest) |
| `actions/download-artifact` | `v8.0.1` → `v8.0.2` | [v8.0.2 release notes](https://github.com/actions/download-artifact/releases/tag/v8.0.2) |
| `actions/upload-artifact` | `v7.0.1` → `v7.0.2` | [v7.0.2 release notes](https://github.com/actions/upload-artifact/releases/tag/v7.0.2) |
| `jdx/mr-boxington-action` | `v1.6.0` → `v1.7.1` | [v1.7.1 release notes](https://github.com/jdx/mr-boxington-action/releases/tag/v1.7.1) |

The offline policy output accepts the active holds in the
[freshness inventory](../../.velnor/freshness-inventory.json) for five
subject rows in four grouped scopes: Rust `1.98.1` under #6 through
2026-10-15; MBX `1.21.1`
under #29 through 2026-10-18; the download `v8.0.1` and upload `v7.0.1`
artifact actions together under #6 through 2026-10-21; and
`jdx/mr-boxington-action` `v1.6.0` under #29 through 2026-10-18. This policy
pass records the matching exceptions; it does not mean the reported latest
versions were qualified. The MBX hold rationale still names the earlier
`v1.22.0` observation while this probe reports `v1.23.0`; scope and disposition
remain under review.

The inventory describes the two artifact updates as routine patches observed
during v0.1.1 qualification, with HTTP 429 retry handling and no security
content. The linked upstream release notes describe 429 retry/`Retry-After`
behavior and an `@actions/artifact` dependency update. This preserves the
inventory rationale and cites the upstream summaries; it is not an
independent security assessment or a decision to adopt either patch.

## Preserved log bytes

The copied raw logs remain outside the repository. These paths and SHA-256
values identify the evidence bytes used for this note:

| Log | Lines | SHA-256 |
|---|---:|---|
| `/private/tmp/velnor-registration-target-boundary-20261010/check-upstream.log` | 462 | `d80c7ae0bab03811f554d65d8dd198ac63921da0459cd558bd7fe8395aaa0a0c` |
| `/private/tmp/velnor-registration-target-boundary-20261010/verify-local-recurrence-20261010/verify-local.stdout.log` | 108 | `e36383f7761d2052e57eb746370d186143c90677ff0e6bf02667813a323c05b7` |
| `/private/tmp/velnor-registration-target-boundary-20261010/verify-local-recurrence-20261010/verify-local-repo-policy.log` | 423 | `5b42c149cfbe19afafdde5f1177da5fde96d022d9e07da4f6b68aa57679536cf` |
| `/private/tmp/velnor-registration-target-boundary-20261010/verify-local-recurrence-20261010/verify-local-integration.log` | 2,379 | `05010828a4127ff4e23634b41187d4a2267de911a848d5087f6045a82591ff87` |

No gate was rerun for this documentation record. The recurrence is evidence
of the observed failure only; it does not identify a root cause or establish
release qualification.
