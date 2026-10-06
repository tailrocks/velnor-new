# tofu-lockfile fixture
Intent: lockfile present/absent/weakened. `absent/` (no lock) is
the control; `present-empty/` (zero-byte lock) proves a neutral
lockfile is ignored; `stale/` (lock naming example.com/a/b,
a provider NOT in config; non-canonical spacing on purpose)
proves validate READS the lock and fails on stale entries;
`weakened/` (unparseable lock HCL) proves a corrupt lock is a
validate error, not a warning. All hashes/addresses fictional.
Expected discovery outcome: fmt selection NEVER includes
`.terraform.lock.hcl` (all four fmt-clean despite odd/corrupt
lock bytes); validate selection always includes it — adapter
must treat stale/corrupt locks as validate-blocking errors with
`failed to read dependency lock file` / `no package … cached`
diagnostics, and must never auto-repair or delete the lock.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- fmt all four roots: exit 0 each (lock never visited)
- validate absent/: exit 0 `Success!`
- validate present-empty/: exit 0 `Success!`
- validate stale/: exit 1 `…no package for example.com/a/b
  1.0.0 cached in .terraform/providers`
- validate weakened/: exit 1 `failed to read dependency lock
  file: 2 problems` (+ non-normalized address note)
