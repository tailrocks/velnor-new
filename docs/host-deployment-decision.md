# Host deployment stays operator-run, outside generated CI/CD (HD-9)

**Decision.** Velnor generates CI and artifact-delivery jobs. These jobs
deliver attested images, Maven packages, npm packages, bundles, and Git tags.
Velnor does not generate host-deployment jobs. Operators run
`ansible-configs/deploy-*.yml` on their machines.

Generated Ansible CI checks deployment inputs, syntax, lint rules, pinned
tools, and local collections. It holds no SSH key, vault password, or inventory
credential.

**Status.** The owner adopted this decision through the split brief. The owner
can review it again.

**Rationale.** GitHub-hosted runners cannot reach these private-network hosts.
CI deployment would add persistent SSH keys and vault credentials, contrary
to decision 7. The source repository did not deploy from CI. Imported workflows
must not deploy during import.

Any future `[[deploy.ansible]]` feature requires a separate proposal. It must
specify a temporary runner inside the network, required reviewers, OIDC
authentication, and a secrets service. It must permit deployment only after a
push to the default branch.

**Must not be reintroduced without owner approval:** a generated job that
connects to a managed host, holds an SSH key or vault password, or runs
`ansible-playbook` against a real inventory.
