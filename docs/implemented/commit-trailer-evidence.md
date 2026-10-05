# Commit trailer evidence

PR #68 was squash-merged as `ac3ab6a3d5ba3701c1300bbdd8114390093c5c29`. Its protected commit has the correct Codex co-author trailer, but its sign-off says `Signed-off-by: Alexey <alexey@zhokhov.com>` instead of the required `Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>`. The merge-message input omitted “Zhokhov”; GitHub preserved that input. The protected history is not rewritten, and this metadata deviation does not change the reviewed source or test result.

Before creating a local commit or submitting a squash message, run `python3 scripts/validate-commit-trailers.py MESSAGE_FILE`; for a local commit, add `--check-local-identities`. The validator reads the exact terminal block from `docs/implemented/codex-agent-configuration.md` and rejects missing, malformed, reversed, duplicated, or nonterminal required trailers. After publication, inspect the actual commit message and author metadata from GitHub; the validator cannot repair a protected commit.
