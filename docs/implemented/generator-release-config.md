# Rust generator release configuration

The Rust generator reads top-level keys from `.velnor/release.toml`.
By default, `roots = ["."]` lists Cargo roots with `release-plz.toml` files and
`after = ["Rust CI"]` lists workflow names and must include every push workflow.

`pr_only_checks = ["DCO"]` explicitly exempts names from release push checks;
it defaults to `[]` and requires pinned `velnor-ci` 0.1.81 or newer. Each name
is a literal repeated `--pr-only-check` argument; Actions expressions are
refused. Each name must exactly match a required check on the default branch,
including case and whitespace. The gate requires at least one applicable,
active branch ruleset with a `pull_request` rule and explicitly empty
`bypass_actors`. Its detail must match the identity and repository,
organization or enterprise source reported by the branch rules API.
Missing, hidden or unverifiable metadata refuses the exemption. A bypassable
ruleset cannot prove enforcement; another applicable ruleset without bypass
actors can. Classic-only branch protection cannot authorize exemptions.
GitHub [returns `bypass_actors` only with ruleset write access](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset),
so read-only credentials may refuse exemptions.
At least one required push check must remain after exemptions. Stale names or
removal of the enforced PR rule refuse release. Exempted checks may belong to
external GitHub Apps. A listed name with a push job, commit status or Checks API
run on the verified commit refuses release, regardless of its App, status or
conclusion. Every check-run page is required, including historical runs.
An unreadable or incomplete response refuses release. Every other required
check still needs a successful GitHub Actions push job on the default branch
at the exact release commit. Missing, failed, skipped, pull request or
other-commit jobs cannot satisfy it. External checks cannot replace push jobs,
and missing jobs never create exemptions.
