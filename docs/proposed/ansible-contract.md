# Ansible contract (Ansible stack with uv-locked Python)

Status: implemented except for the items under "Not yet implemented". Source design: the ChainArgos split stacks design
(section 6.2 and 6.3). Decision: [host deployment (HD-9)](../host-deployment-decision.md),
adopted per the split brief; owner may revisit. This page states what exists
today and what each remaining piece must satisfy; it does not claim the stack
is complete.

## Consumer shape (`ChainArgos/infrastructure`)

- Playbooks live directly in `ansible-configs/` (47 top-level files plus
  `tasks/`, `config/`, `debian/`); inventory `ansible-configs/hosts.ini`;
  `requirements.yaml` lists four collections by exact version.
- `mise.toml` declares `uv = "0.13.1"` (aqua) and `ansible-core = "2.21.4"`
  (pypi backend, which shells out to uv). There is no `requirements.txt` and no
  `ansible-lint` in the repository today.
- The repository also holds a Selene policy harness (Python and shell tests,
  `scripts/tests`) and a small Rust workspace (`scripts/`); see the ownership
  note below.
- Host rollout (`deploy-*.yml`) stays operator-run (HD-9).

## Configuration

```toml
[stacks.ansible]
roots = ["ansible-configs"]                              # required, sorted, unique
python_requirements = "ansible-configs/requirements.txt" # required; fully --hash pinned
inventory = "ansible-configs/hosts.ini"                  # required
checks = ["syntax", "lint"]                              # closed; default both, sorted
lint_profile = "production"                              # min|basic|moderate|safety|shared|production
timeout_minutes = 10                                     # 1..=360

[[stacks.ansible.collections]]                           # sorted by name; installed offline
name = "community.general"
version = "13.4.0"
sha256 = "<64 lower-case hex>"
```

The table is strict. There is no shell, argv, YAML or `uses:` field, and no
host, vault or SSH field. `ansible.cfg` path values are literal, nonempty
root-relative paths using only ASCII letters, digits, `.`, `_`, `-`, and `/`;
expansion syntax, quotes, backslashes, empty list elements and traversal
outside the root are rejected. `inventory` uses comma-separated paths and
`roles_path` uses colon-separated paths; the other separator is invalid.

## Implemented: `velnor-actions-ansible`

A process-free adapter crate returning typed findings with stable codes.

| Piece | Module | Findings |
|---|---|---|
| Python requirements | `requirements` | `python_requirements_unhashed:<n>`, `python_requirement_unpinned:<spec>`, `python_requirement_hash_malformed:<n>`, `python_requirement_option_forbidden:<text>`, `python_requirement_duplicate:<n>`, `python_requirement_missing:ansible-core`, `conflicting_project_tool_versions:ansible-core` |
| Collections | `collections` | `ansible_collections_mismatch:<name>`, `ansible_collection_unversioned:<name>`, `ansible_requirements_unparsed:<line>` |
| Playbooks | `playbooks` | none (sorted top-level playbook list) |
| Vault | `vault` | `ansible_vault_in_ci:<path>` |
| Affected selection | `select` | none |

Rules:

1. **Python tooling.** The venv is created by
   `uv pip install --require-hashes --no-deps -r <requirements> --python <owned>`
   into `$RUNNER_TEMP/velnor/ansible-venv`. Every requirement is `name==version`
   with at least one `--hash=sha256:<64 hex>`; options, URLs, VCS references,
   editables and includes are refused. With `--no-deps` the file must list the
   whole closure (`uv pip compile --generate-hashes` produces it).
   `ansible-core` must be present and equal the `mise.toml` version (decision 4).
2. **Collections.** Velnor fetches
   `https://galaxy.ansible.com/api/v3/plugin/ansible/content/published/collections/artifacts/<ns>-<name>-<version>.tar.gz`,
   verifies the declared SHA-256 and runs `ansible-galaxy collection install --offline`.
   `requirements.yaml` must list exactly the same set and versions in both
   directions.
3. **Playbooks.** The syntax check covers every top-level `.yml`/`.yaml` file of
   a root whose first item is a play (`hosts:`) or an `import_playbook`, sorted.
4. **Vault.** A file starting with `$ANSIBLE_VAULT;` fails: generated CI holds no
   vault password.
5. **Selection.** A change under a root selects its job; a change to the
   requirements file or the inventory selects every root.

## Decisions

HD-9 (host deployment stays operator-run, outside generated CI/CD) is recorded
in [host deployment decision](../host-deployment-decision.md); adopted per the
split brief; owner may revisit.

## Consumer notes

- The consumer commits `ansible-configs/requirements.txt` (the hash-pinned closure
  of `ansible-core` and `ansible-lint`, compiled with `uv pip compile
  --generate-hashes`), declares `ansible-lint` next to `ansible-core` in
  `mise.toml`, pins every collection of `requirements.yaml` by SHA-256 and
  commits an inert `ansible.cfg` (below).
- The repository mixes Ansible with a Rust workspace; the Rust stack detects the
  root workspace on its own (there is no `roots` key), so the two stacks coexist
  without extra configuration.

## Implemented: the `Ansible / <root>` job

`WorkflowTaskPolicy::Stack` (renderer `stack_jobs`, shared with dbt) carries one
job per configured root; the orchestrator resolves it in `ansible_inventory`
(analysis, fail closed), `ansible_tasks` and `ansible_steps`, and plans it in
`select_ansible`. The job id is `ansible-<slug>` and the display name
`Ansible / <root>`. Findings fail generation as `ansible_stack_findings:`.

- **Shape.** Checkout without credentials, `Setup Mise`, `Resolve project tool
  versions` (defining `VELNOR_TOOL_UV` and `VELNOR_TOOL_PYTHON`, which the job
  references), `Install uv`, `Install Python`, `Create Python environment`,
  `Verify ansible.cfg`, `Install Ansible collections` (when any are declared),
  then `Ansible syntax check` and `Ansible lint` as background steps joined by
  `wait-all`. The job holds `contents: read` only and refuses any `uses:` step or
  token role; there is no vault password and no host connection.
- **uv check and Python.** `uv` and Python install from the `linux-x64` row of
  `mise.lock` (`[tools.uv]`, `[tools.python]`): generation requires a `sha256:`
  checksum and a URL under the upstream release path
  (`ansible_tool_checksum_missing`, `ansible_tool_url_untrusted`,
  `ansible_tool_lock_missing`), the job rebuilds the exact URL from the
  project-declared version, and `sha256sum -c` runs before extraction. Both tools
  must be declared in the Mise config (`uv_not_declared`, `python_not_declared`).
- **Agreement.** `ansible-core` (and `ansible-lint` when `lint` is a check) must
  be declared in the Mise config and equal the pinned version in
  `requirements.txt` (`conflicting_project_tool_versions:<name>`).
- **Environment.** `uv venv`, then `uv pip install --require-hashes --no-deps
  --no-build`, with `UV_NO_CONFIG`, `UV_PYTHON_DOWNLOADS=never` and a job-owned
  `UV_CACHE_DIR`; sdists are never built.
- **Owned configuration.** `ANSIBLE_HOME`, `ANSIBLE_COLLECTIONS_PATH` and
  `UV_CACHE_DIR` live under `$RUNNER_TEMP/velnor`. `ANSIBLE_CONFIG` is the root's
  committed `ansible.cfg`, required and checked in two places: generation admits
  only the inert keys `defaults.{inventory, roles_path, host_key_checking,
  retry_files_enabled, interpreter_python, forks, gathering, timeout, nocows,
  deprecation_warnings, display_skipped_hosts, any_errors_fatal}` and
  `ssh_connection.{pipelining, retries}` with literal path-list grammar and
  every path inside the root
  (`ansible_cfg_missing`, `ansible_cfg_key_forbidden:<section.key>`,
  `ansible_cfg_path_invalid:<key>`,
  `ansible_cfg_path_escapes_root:<key>`), and the job re-checks the file's SHA-256
  against the digest recorded at generation. `ansible-lint` runs from a
  Velnor-written config (`profile`, `offline: true`), never the repository's
  `.ansible-lint`.
- **Checks.** The syntax step runs `ansible-playbook --syntax-check -i <inventory>`
  over every sorted top-level playbook, continues past failures and prints an
  `ok`/`FAIL` report per playbook; the lint step lints the same playbooks.
- **Plan.** A change under a root, to the requirements file or to the inventory
  selects the job; an unknown change set, a global-config path (including the
  Mise config and lock) or a full-run event selects every root. The `ansible-`
  prefix owns no obligations.
- **Caches.** `python-venv` and `ansible-collections` are specified
  ([cache families](cache-families.md)); their restore and save steps are not
  generated yet.

## Not yet implemented

- The `python-venv` and `ansible-collections` restore and save steps.
- Named checks reusing the locked-Python path (Selene tests,
  design-system-extraction).
