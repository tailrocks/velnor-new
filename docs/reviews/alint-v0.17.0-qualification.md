# Alint v0.17.0 action pin qualification

Checked: 2026-10-05. Repository baseline: PR12 worktree at `ac9d833f25bd154788152c21089056b8fa8d6905`.

Scope: compare the current action and binary (`v0.16.1`) with the proposed action and binary (`v0.17.0`) using this repository’s `.alint.yml` and the exact workflow inputs. This proves local configuration and CLI-flag compatibility. It does not prove hosted runner installation or performance.

## Source identity

- GitHub tag `v0.16.1` resolves to action commit `9f9d34ba0eae3888299b9e570f43338b0e7f2cdb`.
- GitHub tag `v0.17.0` resolves to action commit `d93c0283b19dd78afcd8a4b303f1556a7759ba81`.
- Fresh source observation: `GET https://api.github.com/repos/asamarts/alint/releases/latest` returned stable release `v0.17.0` (`published_at` `2026-10-01T05:27:59Z`) at `2026-10-04T17:50:50Z`; raw response SHA256 `80a0eab492dff0471e57cd64f08e8fe947162b2de5aa07ed6f5d87ab0bcb9c5a`.
- Fresh immutable tag check: `GET https://api.github.com/repos/asamarts/alint/git/ref/tags/v0.17.0` returned `refs/tags/v0.17.0` as a direct commit ref to `d93c0283b19dd78afcd8a4b303f1556a7759ba81` at `2026-10-04T17:50:51Z`; raw response SHA256 `d608b31ec61379acf09f6aed4b17fcd97e5c09335bf9583ca48840b67199f5a4`. The inventory row records the release endpoint URL, observation time and raw-response hash, with the pin and qualified commit synchronized to this ref.
- `action.yml` fetched from both immutable commits is byte-identical; SHA256 `2fcafd732f09170f62c17f6d1e1f1a638133c24130873de58f50124974505bad`.
- Official Apple ARM64 release archives matched the corresponding upstream `SHA256SUMS`: v0.16.1 `def639d9581520832096f0318b9f66204181679521b446da93a5d4875d5240e9`; v0.17.0 `99cb9bfbf52c0e33ca3a64b9e14ce6e7413930d8fd39c6e4e29239452cf1fb04`.
- Both downloaded binaries reported their exact expected version and commit prefix.

## Paired compatibility run

The workflow supplies `version`, `config: .alint.yml`, `fail-on-warning: true`, `format: github`, and `path: .`. Since the action pin is a full SHA, the explicit version input is retained. The action metadata maps those inputs to:

```sh
alint --format github --config .alint.yml --fail-on-warning check .
```

Both official binaries were also run with `alint validate-config .alint.yml`.

| Run | v0.16.1 | v0.17.0 |
|---|---:|---:|
| `validate-config .alint.yml` | exit 0; 53 rules | exit 0; 53 rules |
| Workflow-equivalent `check` | exit 0 | exit 0 |
| Check stdout SHA256 | `af37eb29cc4d5bfb9abd7349fd7f1979eee7241c71433e424d2bc7cb095a3075` | same |
| Check stderr | empty | empty |

The stdout bytes match exactly. They contain four informational notices: missing `SECURITY.md`, missing `CODE_OF_CONDUCT.md`, trailing whitespace at line 3 of `velnor-actions-ci-performance-spec.md`, and the `rust-toolchain.toml` pin suggestion. No policy errors were reported.

Config-validation stdout also matches byte-for-byte (SHA256 `e44b23ef5afb87a45ce05156c9f422cddb9a17f00fef0934799eb348438ea309`).

## Limits

Runs used official macOS ARM64 binaries on the local host. The actual GitHub composite action and its hosted Ubuntu install path were not executed; hosted behavior, signatures, and performance remain unqualified. Paired checks first ran at the PR12 baseline before the pin edit, then were repeated on the updated worktree after the action, policy, documentation, and inventory changes. The post-edit run produced the same exit statuses and stdout digests shown above.
