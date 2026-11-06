# Freshness evidence — 2026-11-06

Refreshes `checked_at` in `.velnor/freshness-inventory.json` after the 2026-11-05 evidence window expired at `2026-11-06T04:33:43Z`.

## Captures

Two independent authenticated captures (`capture-1` and `capture-2`) are recorded under `docs/implemented/evidence/freshness-2026-11-06/` and agree in `capture-agreement.json` (`agree: true`).

## Temporary holds

Four upstream releases were published on `2026-11-05` within the repository 3-day `default_cooldown_days` window (`.velnor/version-policy.toml`) and require toolchain migration (`mise.toml`/`mise.lock` per VER-3.4 and VER-4.4), controller bootstrap pin updates (`.velnor/generator.lock` and `.velnor/controller-approvals.json`), and workflow/OpenTofu golden regeneration tracked in https://github.com/tailrocks/velnor-new/issues/531:

| Subject | Pinned | Upstream latest | Published |
|---|---|---|---|
| `mise` | `2026.11.1` | `v2026.11.2` | `2026-11-05T12:19:06Z` |
| `jactionlint` | `2.4.0` | `v2.5.0` | `2026-11-05T18:00:01Z` |
| `opentofu` | `1.13.8` | `v1.13.9` | `2026-11-05T19:09:38Z` |
| `asamarts/alint` | `v0.21.0` | `v0.21.1` | `2026-11-05T19:46:05Z` |

## Runner observation

Successful `main` CI run `42182644618` (`Plan` job `127775581179`, source `1168644a27c6a23f78a408c9ae13bd99c04f3ce2`, full log `ci-job.log` SHA-256 `f1e5701ff3c5e4f2a64c39f2f5f5c40ac1a145e1a987cae773218eb7b1afe4ad`, API receipt `ci-job.json` SHA-256 `c1a607edc5eebfd29909fb0d2d1339ccb18a878cc2937846b62d1002bafa9d8c` with `status: "completed"` and `conclusion: "success"`) and the GitHub hosted-runner reference (`runner-doc.html`, SHA-256 `3b68d3e3d160390cc41a24288511f7f44c9a5de3906629f17030e7e0f1959aba`) are recorded under `docs/implemented/evidence/freshness-2026-11-06/runner/`:

- image `ubuntu-26.04`, version `20261101.186.1`
- runner `2.338.2`
