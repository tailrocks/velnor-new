## 6. Acceptance gates

| Gate | Required evidence |
|---|---|
| Determinism | Two generation runs from identical inputs produce byte-identical YAML and task IDs |
| Topology | Triggers, permissions, concurrency, action SHAs, plan, matrix, and final jobs match this contract |
| Selection | Fixtures prove reverse-dependency selection, rename/delete handling, and conservative fallback |
| Focused execution | Clippy gates each crate; detected test runner and doctests are visible separate tasks |
| Tool boundary | CI proves locked Mise tools and the detected Cargo/MBX route; direct Cargo/tool installers are rejected |
| Cache ownership | Sources, MBX objects, target directories, and task results have no duplicate owner |
| Cache correctness | Warm reuse works; each changed identity component invalidates the relevant result |
| Trust | PR runs cannot write trusted/release caches or use release credentials |
| Fallback | Disabled, unavailable, corrupt, and incomplete task caches execute normally and report the reason |
| Final status | Missing, failed, cancelled, skipped, and no-work cases produce the exact final result above |
| Dogfood | Velnor's own committed workflow is generated and checked by Velnor |
| Performance | Warm fixture and dogfood runs report setup, cache, compiler, test, and queue time separately |

The implementation MUST add fixtures for a standalone package, multi-crate workspace, shared dependency chain,
build script/shared fixture, feature target, doctest, failing format/Clippy/test, empty suite, renamed
package, warm rerun, and negative cache identities. A green result with missing qualification runs is not
acceptance.
