#!/bin/bash
# land-risk.sh N "comment": like land.sh, but on a high-risk refusal it uses --accept-risk (owner-approved in session 2026-11-25).
N=$1; C=$2; R=tailrocks/velnor-new
M=~/.claude/plugins/cache/tailrocks/tailrocks-repository-skills/0.5.1/skills/tailrocks-pr-merge/scripts/merge.ts
cd ~/Projects/tailrocks/velnor-new
for i in $(seq 1 80); do
  out=$(bun run $M merge $N --repo $R 2>&1)
  RF=""
  if echo "$out" | grep -q "refused (exit 3)" && echo "$out" | grep -qi "high-risk"; then
    out=$(bun run $M merge $N --repo $R --accept-risk 2>&1); RF="--accept-risk"
  fi
  if echo "$out" | grep -q 'plan (exit 4)'; then
    gh pr comment $N -R $R --body "$C" >/dev/null
    bun run $M merge $N --repo $R --confirm $N $RF 2>&1 | grep -E '^(enqueued|merged|result)'
    bun run $M watch $N --repo $R --confirm $N --interval 180 2>&1 | grep -E '^(merged|result|  \[blocker)'
    exit 0
  fi
  if echo "$out" | grep -q "refused (exit 3)"; then echo "$N REFUSED:"; echo "$out" | grep -E "refused|reason|high-risk"; exit 3; fi
  if echo "$out" | grep -qE '\[blocker\] (conflict|threads_unresolved|dco_missing|github_disagrees|draft|review|stacked)'; then
    echo "$N BLOCKED:"; echo "$out" | grep -E '\[blocker\]'; exit 1
  fi
  sleep 180
done
echo "$N timeout"; echo "$out" | grep -E '\[(blocker|wait)'
