# Homebrew adoption manifest v2

The first native Jackin tap seed left the consumer release manifest at 0.1.48
while selecting SDK 0.1.54. The ordinary protected planner rejected that tree
with `release_manifest_version_mismatch:0.1.48:0.1.54` in tap PR 514. The seed
renderer must carry its SDK identity through the same fixed output inventory
used by authoring and replay; adding an extra file outside that contract would
leave the two paths inconsistent.

Template v2 requires SDK 0.1.55 and emits exactly five shared outputs:
`.github/workflows/ci.yml`, `.velnor/config.toml`,
`.velnor/release-manifest.json`, `mise.lock`, and `mise.toml`. The manifest is
the supplied UTF-8 byte sequence, without JSON reserialization. Existing native
authoring and replay consume that inventory. The transition's tap base,
deletion inventory, source authority, and inert formula inventory are unchanged.

The reviewed regression plan uses one explicitly registered public integration
target, `homebrew_adoption_seed_public`. It renders the seed from deterministic
input data, asserts exact manifest bytes, applies the other four outputs over a
legacy 0.1.48 manifest, and checks the precise public `prepare` version error.
Applying the rendered manifest must then make public `prepare` succeed. The
fixture uses real temporary files and Git with isolated configuration,
templates, hooks, and signing settings. Fixture release data is not evidence of
a published or authenticated SDK release.

The reset baseline had no Rust test target covering this composition. This
single scenario uses public APIs, restores no deleted private harness, and
needs no network or owned-code mocks. Independent review approved the plan
before implementation and confirmed the observed red/green behavior.

The controlled red omitted only manifest emission while retaining the v2
contract. The public renderer rejected its incomplete inventory in
`/tmp/sdk55-aligned-public-red.log`. Restoring the exact renderer bytes passed
the scenario in `/tmp/sdk55-aligned-public-green.log`. Generated CI registers
the orchestrator's real Nextest task with `--no-tests fail`; local execution
alone is not a claim that hosted CI or SDK publication has completed.
