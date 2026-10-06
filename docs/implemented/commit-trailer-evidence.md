# Commit trailer evidence

The audit found three short-name sign-offs in the published PR #68 branch history and one in its protected squash commit. The three branch commits are not ancestors of the squash commit; only the squash commit is part of protected main history.

| Commit | Recorded identity | Cause or evidence limit |
| --- | --- | --- |
| `62efc0985e7cfdfba1c2f7177b0cb6b0ae799b9a` | Author, committer, and sign-off use `Alexey <alexey@zhokhov.com>` | The commit records the abbreviated identity. It does not prove whether that came from local Git configuration or an explicitly supplied message. |
| `790b859842a04c5efb41ec87e80ad5a0e479bdaa` | Author, committer, and sign-off use `Alexey <alexey@zhokhov.com>` | Same evidence limit. |
| `fafaa87430bf9ffa0b454198a2b09aaff835d664` | Author, committer, and sign-off use `Alexey <alexey@zhokhov.com>` | Same evidence limit. |
| `ac3ab6a3d5ba3701c1300bbdd8114390093c5c29` | Full-name author; sign-off uses `Alexey <alexey@zhokhov.com>` | The submitted squash message omitted “Zhokhov”; GitHub preserved that text. |
| `098be4ad61e614fa4ddfb30f7e7148e470974597` (PR #81) | Full-name author; GitHub committer; `Signed-off-by` present without `Co-authored-by` | The protected-main commit has no Codex co-author trailer; preserve the recorded commit. |
| `2497e4004fea00d77c6f319822f4e6b5d07fdac3` (PR #76) | Full-name author; GitHub committer; `Signed-off-by` precedes `Co-authored-by` | The protected-main commit records the canonical trailers in reverse order. This is historical evidence; leave the commit unchanged. |
| `bc06c69815d6e6f5b83f5f8155bff721dafe8ef5` (PR #80) | Full-name author; GitHub committer; `Signed-off-by` precedes `Co-authored-by` | The protected-main commit records the canonical trailers in reverse order. This is historical evidence; leave the commit unchanged. |
| `ccb337ecc66e694bf8cb292af1ff43332389fb97` (PR #79) | Full-name author; GitHub committer; `Signed-off-by` precedes `Co-authored-by` | The protected-main commit records the canonical trailers in reverse order. This is historical evidence; leave the commit unchanged. |
| `540e12a4a225683e78378e63bfdbba2ddba29d86` (PR #89) | Author Alexey Zhokhov; committer GitHub (`web-flow`); `Signed-off-by` precedes `Co-authored-by` | The published squash commit records the reversed order; the available remote commit evidence does not establish who reordered the submitted message. Leave history unchanged. |
| `ba13b691459310fbc99cbeb857940f60993cd3f7` (PR #91) | Author Alexey Zhokhov; committer GitHub (`web-flow`); `Signed-off-by` precedes `Co-authored-by` | Same evidence limit; leave history unchanged. |

The documented repository-local Git identity is `Alexey Zhokhov <alexey@zhokhov.com>`. The private `repo-policy-v1` / `trailer-policy` operation validates the exact terminal trailer block. Its `VELNOR_REPO_POLICY_CHECK_LOCAL_IDENTITIES=1` setting checks the effective Git author and committer identities against that documented identity; include it for every local commit, while API-submitted message validation may omit it. The `validate_commit_trailers` example offers the same rules for manual runs. The published branch and protected history are not rewritten. After publication, inspect the actual remote commit message and identity; validation cannot repair an already published commit.

Run the following from the repository root before submitting a commit or squash message. `MESSAGE_FILE` must resolve to an absolute path to an existing message file:

```sh
MESSAGE_FILE="$(realpath path/to/message.txt)"
env \
  VELNOR_INTERNAL_OP=repo-policy-v1 \
  VELNOR_REPO_POLICY_ACTION=trailer-policy \
  VELNOR_REPO_POLICY_ROOT="$PWD" \
  VELNOR_REPO_POLICY_MESSAGE_PATH="$MESSAGE_FILE" \
  cargo run --quiet --locked -p velnor-actions-cli --bin velnor-actions
```

For every local commit, add `VELNOR_REPO_POLICY_CHECK_LOCAL_IDENTITIES=1` to the `env` assignments to check both configured Git identities. API-submitted message validation may omit this setting. The command reads the exact terminal trailer block from `docs/implemented/codex-agent-configuration.md` and rejects missing, malformed, reversed, duplicated, nonterminal, or body-decoy required trailers. The `validate_commit_trailers` example is the manual equivalent. After an API merge, independently inspect the actual remote commit message and author metadata.
