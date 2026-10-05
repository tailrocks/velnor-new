# Offline fixture coverage

`test_seed_candidate.py` generates two tiny `fixture-lib` crate archives,
matching crates.io index rows, a version-4 lockfile, and a one-member workspace
inside a temporary directory under the assigned private task directory. A
local Git commit pins the fixture source. No live registry, ambient Cargo
cache, Rust toolchain, or package build script is used.

The tests cover the successful capsule path plus archive/index checksum
mismatches, unsafe tar paths, unexpected extra files, and the four crates.io
index path shapes.
