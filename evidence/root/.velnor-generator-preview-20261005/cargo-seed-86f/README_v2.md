# Exact-lock Cargo seed preparation, v2

This version is pinned to Velnor commit `86f864aabc2192a9f8ccd3f01f2e25b7858c3f20`,
tree `1e16a9270f3ea99b04e68521cfc876fa19db67ac`, and `Cargo.lock` SHA-256
`95d251ccedbcc12095cecb89c9c08b14d72a5f0774d252029a559d45bb557b1a`. It
accepts only the 84 crates.io package instances in that lockfile, their exact
index rows and checksum-matching archives, and source files extracted from
those archives. It does not read Cargo configuration or credentials and does
not invoke Cargo, Rust, package build scripts, or registry clients.

Use `bounded_entry_v2.py` as the command entry point. It starts one operation
with a sanitized environment, a 2 GiB address-space limit, a 1 GiB per-file
write limit, a 64-descriptor limit, a 300-second maximum wall/CPU bound, and a
1 MiB combined output cap. Its timeout path terminates and reaps the child
process group. `seed_candidate_v2.py` also bounds Git capture to 1 MiB and each
Git command to ten seconds.

All input file opens reject non-regular files without blocking on FIFOs. Reads
and hashes use explicit byte caps and compare descriptor metadata before and
after streaming to reject files that change during inspection. The archive
copy checks the compressed-size cap while streaming, before writing an
over-limit chunk. Extracted paths accept only relative regular files and
directories beneath the crate root. Limits are defined near the top of
`seed_candidate_v2.py` and in `bounded_entry_v2.py`.

The CLI is intentionally limited to three explicit modes:

```text
python3 -I -B bounded_entry_v2.py plan --source-root SOURCE --output PRIVATE_PLAN.json
python3 -I -B bounded_entry_v2.py prepare --source-root SOURCE --registry-root REGISTRY --output PRIVATE_CANDIDATE
python3 -I -B bounded_entry_v2.py verify CANDIDATE --source-root SOURCE
```

The plan and capsule formats are versioned `v2`. The capsule is written only
to a new child of this assigned private task directory. Candidate creation is
deliberately withheld until the independent source review and seed-bind gate
are complete.

Run the offline fixtures from this directory with:

```text
python3 -B -m unittest discover -s tests_v2 -v
```

The fixtures create all source, index, archive, and process-tree data under a
temporary directory here. They do not use Cargo, Rust, the ambient registry
cache, or the network.
