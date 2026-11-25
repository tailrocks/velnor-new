# Warm-cache verification brief (ChainArgos processor repos)
Goal: prove the regenerated CI (tools 0.1.115) passes the Cache waste check on a WARM run on main (restoring a base saved by this helper), with no "compiled despite usable restored inputs" lines.
1. Find the CI push run on main for the merge commit given (gh run list -R <repo> --branch main --workflow CI --limit 5). Wait for it to finish (poll ≤ every 3 min, max 60 min). Record: conclusion, Cache job result, whether the target/sources caches were SAVED (look for velnor-private-tgt-/velnor-private-src- save lines in the logs) and the "Check cache waste" line.
2. Then trigger a warm run on main: `gh workflow run ci.yml -R <repo> --ref main` (if workflow_dispatch is allowed by the generated workflow; check `on:` first). If not allowed, report that and stop — do NOT push commits to main.
3. Wait for that run; record whether the target cache RESTORED (hit), the Cache job result, the "Check cache waste" line, and grep the logs for "compiled despite usable restored inputs" (must be 0). Never print secrets.
4. Report ≤6 lines: run URLs (save run, warm run), restore hit yes/no, Cache result, waste line.
Read-only otherwise; do not change any file or push anything.
