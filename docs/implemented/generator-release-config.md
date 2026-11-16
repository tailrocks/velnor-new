# Rust generator release configuration

The Rust generator reads top-level keys from `.velnor/release.toml`.
By default, `roots = ["."]` lists Cargo roots with `release-plz.toml` files and
`after = ["Rust CI"]` lists workflow names and must include every push workflow.

`pr_only_checks = ["DCO"]` explicitly exempts names from release push checks;
it defaults to `[]` and requires pinned `velnor-ci` 0.1.81 or newer. Each name
is a literal repeated `--pr-only-check` argument; Actions expressions are
refused. Each name must remain required on the default branch, which must
have a live `pull_request` rule. Listed checks must be valid merge-time admission
evidence, with branch protections enforced without bypass. At least one required
push check must remain after exemptions. Stale names or removal of that rule refuse
release. Exempted checks may belong to external GitHub Apps. Every other
required check still needs a successful GitHub Actions push job on the default
branch at the exact release commit. Missing, failed, skipped, pull request or
other-commit jobs cannot satisfy it. Missing jobs never create exemptions.
