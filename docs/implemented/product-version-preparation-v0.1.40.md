# Product version preparation for v0.1.40

This prepares the v0.1.40 product release from main
`9a0b9d2f87b34d2f61c6b755cc4275a333566b2e` (#341). The immutable v0.1.39
release was published from `ffe36ad62c30668753df5298e2c3d1afb7f6f6ec` before
#341 merged, so it does not include the native immutable Preview migration.
The existing v0.1.39 release and tag remain unchanged.

v0.1.40 includes the immutable per-source Preview protocol: separate payload
source and workflow authority, bounded source/task/tool admission, complete
artifact acquisition, signed recovery, and current-main-only Homebrew
promotion. It also includes the native empty-only CI inventory retry added
with that migration. Stable publication retains its existing source equality
contract. The retired policy admission route is removed completely.

The root workspace version and exact internal path requirements advance from
0.1.39 to 0.1.40. Cargo controls the lock update: only the fourteen workspace
package versions change; external package records, checksums and dependency
graphs remain unchanged. Repository-generated workflows, parity fixtures and
OpenTofu goldens are regenerated with the documented tools. The independently
versioned runner workspace, tool pins, runner/image graph and historical
release manifests remain unchanged.

This is version preparation, not publication. Exact-source hosted CI,
protected release publication, accepted manifests and public verification
remain separate requirements. Jackin and its Homebrew tap adopt the release
only after its actual published binary and source identity are verified.
