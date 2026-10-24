# GitHub installation token compatibility

Checked against official GitHub documentation on 2026-10-24.

The [Actions token documentation](https://docs.github.com/en/actions/concepts/security/github_token) identifies `GITHUB_TOKEN` as a GitHub App installation access token.
The [completed stateless-token rollout](https://github.blog/changelog/2026-10-02-stateless-github-app-installation-tokens-rolled-out/) states that newly minted installation tokens use the `ghs_APPID_JWT` format and are approximately 520 characters long. Permissions, repository scope, and expiration are unchanged.
The [format guidance](https://github.blog/changelog/2026-05-15-github-app-installation-tokens-per-request-override-header/) explicitly includes Actions `GITHUB_TOKEN`, documents dot separators, and requires opaque handling.

## Native transport boundary

Mise owns structural credential validation for explicit pinned GitHub CLI commands and private-fetch subprocesses. The orchestrator's private-fetch token wrapper uses the same predicate.

- Accept a nonempty opaque value containing ASCII letters, digits, underscore, hyphen, or dot.
- Reject whitespace, control characters, NUL, non-ASCII text, and other punctuation without including the rejected value in an error.
- Do not decode JWTs, validate claims, require a token prefix, count segments, or impose a GitHub-format length limit.
- Keep credentials out of command arguments and debug output. Deliver only the selected credential through the dedicated child environment.
- Preserve pinned-program restrictions and remove ambient credentials from the child environment.

This predicate checks transport safety, not authentication or authorization. GitHub remains the authority for token validity and scope. Existing operation-specific resource budgets, including the evidence collector's 16 KiB context limit, remain separate.

Legacy opaque and modern dotted values must both pass synthetic regression tests. Tests must also preserve rejection, redaction, exact selected-value delivery, and credential isolation. Tests never inspect or mint real credentials.
