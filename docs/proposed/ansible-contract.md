# Ansible contract (Ansible stack with uv-locked Python)

Status: partly implemented. Source design: the ChainArgos split stacks design
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
host, vault or SSH field. Paths are repository-relative file paths over
`[A-Za-z0-9._-]` segments.

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

## Known consumer gaps (reported, not fixed here)

- `ansible-configs/requirements.txt` does not exist: the Python closure of
  `ansible-core` and `ansible-lint` must be compiled with hashes.
- `ansible-lint` is not declared anywhere; its version must be pinned in the
  hashed requirements (no `mise.toml` entry is needed or used for it).
- The four collections need declared SHA-256 digests in `[[stacks.ansible.collections]]`.
- The repository mixes Ansible with a Rust workspace (see the ownership note in
  the delivery report); the Rust crates need their own `[stacks.rust]` roots.

## Not yet implemented

- `Stack::Ansible` discovery and the `Ansible / <root>` job (venv, collections,
  parallel syntax and lint steps), plan dispositions and the `python-venv` and
  `ansible-collections` cache families.
- The uv catalog entry check and an `ansible-lint` run with an owned
  `ANSIBLE_CONFIG`/`ANSIBLE_HOME`.
- Named checks reusing the locked-Python path (Selene tests,
  design-system-extraction).
