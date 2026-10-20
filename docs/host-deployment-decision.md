# Host deployment stays operator-run, outside generated CI/CD (HD-9)

**Decision.** Velnor generates CI and the delivery of artifacts (attested images
pinned by digest, Maven, npm and bundle releases, git tags) but no job that
rolls software onto a host. Rolling out `ansible-configs/deploy-*.yml` onto the
operator-owned machines stays an operator-run Ansible invocation. Generated
Ansible CI only validates deploy inputs (syntax, lint, hash-pinned tooling,
offline collections); it never holds an SSH key, a vault password or an
inventory credential.

**Status.** Adopted per the split brief; owner may revisit.

**Rationale.** The hosts sit on a private network GitHub-hosted runners cannot
reach; a CI deploy would add long-lived SSH keys and vault material to the
trust model, the opposite of decision 7; the source monorepo never deployed
from CI; and imported workflows must not deploy during import. A future
`[[deploy.ansible]]` needs its own proposal: an ephemeral self-hosted runner
inside the network, an environment with required reviewers, OIDC to a secrets
broker, and `push` to the default branch only.

**Must not be reintroduced without owner approval:** a generated job that
connects to a managed host, holds an SSH key or vault password, or runs
`ansible-playbook` against a real inventory.
