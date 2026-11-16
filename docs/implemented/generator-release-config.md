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
actors can. Each exempt required check must also be enforced by such a ruleset
with the same name and App binding. A specific App binding covers a required
check that accepts any App, but an any-App binding cannot cover a required
specific App. Classic-only branch protection cannot authorize exemptions.
GitHub [returns `bypass_actors` only with ruleset write access](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset),
so configured exemptions make the generated gate use the existing release App
from the `release` environment's `RELEASE_APP_ID` and
`RELEASE_APP_PRIVATE_KEY` secrets. The App installation needs repository
`administration: write` and `actions`, `contents`, `statuses`, and `checks`
read permissions. The gate requests these permissions on a token scoped to
the current repository. It checks out nothing and runs only the pinned helper.
The job's own `GITHUB_TOKEN` retains read permissions. Without exemptions,
the gate uses `github.token` and needs neither the App nor the environment.
An App token cannot authorize exemptions when inherited ruleset metadata
remains hidden or unverifiable.
At least one required push check must remain after exemptions. Stale names or
removal of the enforced PR rule refuse release. Exempted checks may belong to
external GitHub Apps. A listed name with a push job, commit status or Checks API
run on the verified commit refuses release, regardless of its App, status or
conclusion. The gate enumerates all commit check-run pages, including historical
runs, and every check-suite page. It separately enumerates runs for suites
missing from the aggregate response, which GitHub limits to 1,000 suites.
An unfinished suite from an App that could produce an exempt check refuses
release. Missing suite status or App metadata also refuses release.
This conservative match also refuses a same-commit pull request check run.
An unreadable or incomplete response refuses release. Every other required
check still needs a successful GitHub Actions push job on the default branch
at the exact release commit. Missing, failed, skipped, pull request or
other-commit jobs cannot satisfy it. External checks cannot replace push jobs,
and missing jobs never create exemptions.
