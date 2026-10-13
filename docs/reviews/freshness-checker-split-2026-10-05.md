# Freshness-checker split — 2026-10-05

## Scope

This note records a mechanical split of the Python already executed by the registered `scripts/check-freshness.sh` gate. The shell entry point and its CLI remain the same. This is the narrowly scoped existing-freshness-Python exception: it adds no Python product, tool dependency, workflow, inventory row, or user-facing command.

The 2026-10-04 PR #12 Python disposition remains a dated path snapshot. This note supersedes only its current-implementation-location statement that the freshness behavior is inline; it does not change any of that snapshot's 79 path dispositions or carry decisions.

## Active implementation map

| Active path | Existing gate responsibility |
|---|---|
| `scripts/check-freshness.sh` | CLI parsing, input-root selection, preflight, and Python stage launch. |
| `scripts/freshness_checks/main.py` | Stage order and final exit status. |
| `scripts/freshness_checks/report.py` | Row formatting, failure accumulation, date/version helpers, and summary. |
| `scripts/freshness_checks/inventory.py` | Inventory shape and version-policy header checks. |
| `scripts/freshness_checks/pins.py` | Local tool/action/runner pins, policy mirrors, and validation-tool pin. |
| `scripts/freshness_checks/rust_pin_parser.py` | Bounded Rust literal parsing used by the existing local-pin stage. |
| `scripts/freshness_checks/lockfile.py` | Dependency forms/scopes, locked identities, graph reachability, and mtime. |
| `scripts/freshness_checks/evidence.py` | Upstream evidence freshness and dated/standing exceptions. |
| `scripts/freshness_checks/advisories.py` | Deny policy and optional live `cargo deny` scan. |
| `scripts/freshness_checks/probe.py` | Existing bounded upstream fetch, gzip decode, and current-catalog parsing. |

The shell derives its helper directory from `BASH_SOURCE[0]` and launches that adjacent `main.py`. The entry point derives its import directory from `__file__` and places it first on `sys.path`. `--root` remains input-tree selection; it cannot redirect helper imports to a fixture.

## Size and preservation evidence

The frozen pre-split source was commit `d22d80c9e4d9d915d12eb1fc101e3b947368de88`. Its checker SHA-256 was `e4b4653ef98c64cd2ff7ca98459bcf0f73f0db10c5d2e77c3e62626998ce59e3`. The offline gate passed on that source: stdout was 310 lines with SHA-256 `7206d5d021656fa819259eeba0778a7feb385289aa6f8a8f71fed9a9710f857b`; stderr was empty.

Static inspection found counterparts for all 30 original Python functions; Rust lexer helpers now live in `rust_pin_parser.py`, and stage helpers were split by responsibility. No original function or check section was skipped. The shell and each of the nine active Python modules are under 400 lines; every function is at most 80 lines. `bash -n`, Python AST parsing/size inspection, and `git diff --check` passed for the split source. Post-split gate output parity and the registered Rust `--root` fixtures must be established on the combined immutable head; this baseline is not claimed as post-split proof.
