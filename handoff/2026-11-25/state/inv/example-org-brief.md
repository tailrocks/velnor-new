# example-org processor CI fix brief

Goal (owner's request, verbatim constraints):
- Fix the regenerated example-org processor CI so it matches the design of velnor-new and velnor-gen, and land that fix on the PR (branch ci/velnor-gen-tip).
- "The repositories are private, use runner ubuntu-26.04-scale-set, and keep required_check = "Required". Do not hand-edit a generated workflow. A change to generator output has to come from velnor-gen generate and pass velnor-gen check. Do not print tokens or private keys."
- "Do not disable the waste check, and do not paper over a red check, to make the pull requests merge."
- The original Cache failure: ".: tailrocks-type-proto v0.1.1 compiled despite usable restored inputs" — root cause was velnor-ci restoring file mtimes but not directory mtimes (fixed by velnor-new #2774, released in tools-v0.1.113+). Commit 384e86b040 on branch fix/public-sources-before-token was about a generated step referring to a step before it exists — fixed in velnor-gen by #2770 (released in tools-v0.1.113+). Judge that commit: it should become unnecessary after regeneration with the released generator (keep/replace/drop — report which and why; never hand-edit).

Steps:
1. Clone the repo into the session scratchpad (gh repo clone example-org/<repo> <dir> -- --branch ci/velnor-gen-tip). Do not print secrets.
2. Install the released tools: velnor-gen and velnor-ci from tailrocks/velnor-new release tools-v0.1.115 (gh release download tools-v0.1.115 -R tailrocks/velnor-new; verify checksums if published). Use the released velnor-gen binary, not a source build.
3. `velnor-gen --root . pin-helper 0.1.115`, then `velnor-gen --root . generate`, then `velnor-gen --root . check` (must pass). Confirm runner stays ubuntu-26.04-scale-set and required_check = "Required" in .velnor config. Inspect the diff: no hand edits, generated files only from generate.
4. Commit signed-off (git commit -s; follow the repo's commit/PR conventions; read its CONTRIBUTING/AGENTS if present), push to ci/velnor-gen-tip (no force-push unless only for DCO).
5. Watch CI (poll ≤ every 3 min). New helper uses new cache keys, so the first run may be cold. Determine from the workflow how/when the target cache is saved (PR runs vs main) and get a run where restore happens from a base saved by this helper; push an empty signed-off commit ("ci: verify warm cache") if needed to get a second run. Required must be green AND the Cache check (including "Check cache waste") must be green on a warm run, with no "compiled despite usable restored inputs" lines. If Cache still fails, collect the exact failing lines and root-cause them (do NOT disable or weaken checks); report back instead of hacking.
6. Resolve any review threads that your change answers. Run tailrocks-pr-status (pr-status) for the PR.
7. Report: PR, head, run URLs (cold + warm), Cache result with evidence, Required result, verdict on commit 384e86b040, pr-status verdict. Do NOT merge (the orchestrator merges).
