# Reproduction attachment

This attachment preserves three store-level tests added to the upstream MBX
v1.21.1 source at commit `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` in
[`jdx/mr-boxington`](https://github.com/jdx/mr-boxington). The patch adds the
tests to `crates/mbx-cache-store/src/store_tests.rs`. The upstream crate is
MIT-licensed; its license notice is included here.

From the Velnor repository root, clone the exact upstream source, apply this
repository's patch by its absolute path, and run the tests from the upstream
checkout:

```sh
VELNOR_REPO_ROOT="$(git rev-parse --show-toplevel)"
MBX_SOURCE=a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313
MBX_UPSTREAM_CHECKOUT="$(mktemp -d /tmp/mbx-v1.21.1.XXXXXX)"
git clone --no-checkout https://github.com/jdx/mr-boxington "$MBX_UPSTREAM_CHECKOUT"
git -C "$MBX_UPSTREAM_CHECKOUT" checkout --detach "$MBX_SOURCE"
git -C "$MBX_UPSTREAM_CHECKOUT" apply "$VELNOR_REPO_ROOT/docs/reviews/native-mbx-active-run-gc/active-prediction-repro.patch"
(cd "$MBX_UPSTREAM_CHECKOUT" && rustup run 1.98.1 cargo test -p mbx-cache-store a_sweep_ --locked -- --nocapture)
```

The exact 13-line output is stored compressed as
`active-prediction-repro.log.gz`; `gzip -dc` prints it. Its uncompressed
SHA-256 is `6e036590b508f2123fbf798206fdcd3c5f5667846ed6779dc48a9c84ee31ed57`
and the compressed file SHA-256 is
`32b75f686410bc67e2e3c91a80adc1c7ef4e1248faf2efac0af13257840fe4de`. It
records 3 passed tests, 71 filtered. The patch has SHA-256
`defe78fa2f26fe5eeb442ed7ab43e179407f24736728e9df1a327eea9b1e4df8`.

The cases establish three bounded behaviors in the store:

1. Collection before a completed task/group receipt can remove an otherwise
   unrooted action result. This models the persistent store state while a
   prediction is pending, but does not instantiate CacheAgent or its
   in-memory prediction object. A later export reports the synthetic digest
   `800f006f2aaae60a808991d88a45b5c6d3af11e6d2be2868e3301220599e6b02`.
2. Collection after receipt persistence can remove unrelated stale data while
   retaining a small receipt-rooted export closure.
3. Collection can still evict a receipt-rooted closure when that closure
   exceeds the configured budget; a completed receipt is not a capacity
   guarantee.

These are deterministic store-level sequences. They do not run the full
CacheAgent, asynchronous low-disk scheduler, Cargo end to end, or either
production lane. They do not show that the production CI digest
`90517eb497832bc218f615ea90bd90641538fda6617033d85e495a22274a28c1` was one
of the 529 results evicted in run 38024039383, and they do not establish that
delaying collection until a final step is safe at production disk capacity.
