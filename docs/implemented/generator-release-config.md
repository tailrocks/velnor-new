# Rust generator release configuration

The Rust generator reads top-level keys from `.velnor/release.toml`.
By default, `roots = ["."]` lists Cargo roots with `release-plz.toml` files and
`after = ["Rust CI"]` lists workflow names and must include every push workflow.

`pr_only_checks = ["DCO"]` explicitly exempts names from release push checks;
it defaults to `[]` and requires pinned `velnor-ci` 0.1.81 or newer. Each name
is a literal repeated `--pr-only-check` argument; Actions expressions are
refused. Each name must exactly match a required check on the default branch,
including case and whitespace. The gate verifies an active ruleset with a
`pull_request` rule. Classic-only branch protection is refused when PR-only
exemptions are configured. Bypass-actor detection is tracked in
[#931](https://github.com/tailrocks/velnor-new/issues/931). At least one required
push check must remain after exemptions. Stale names or removal of that rule refuse
release. Exempted checks may belong to external GitHub Apps. A listed name with
a push job or commit status on the verified commit refuses release because it
is not PR-only. Every other required check still needs a successful GitHub
Actions push job on the default
branch at the exact release commit. Missing, failed, skipped, pull request or
other-commit jobs cannot satisfy it. Missing jobs never create exemptions.
