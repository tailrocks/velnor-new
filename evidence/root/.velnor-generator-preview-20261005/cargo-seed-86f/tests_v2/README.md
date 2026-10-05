# Offline v2 fixture coverage

`test_seed_candidate_v2.py` builds two tiny crate archives, matching index
rows, a lockfile, and a one-member Git-pinned source workspace in a temporary
directory under the assigned private task directory. The process-tree fixture
is also local and is launched only to prove timeout cleanup. No live registry,
ambient Cargo cache, Rust toolchain, or package build script is used.

The fixtures cover successful creation and offline verification, checksum and
path rejection, FIFO rejection without a blocking open, initial and
streaming-size limits, detection of a file changed during a bounded read,
archive decompression limits, bounded Git output/runtime, combined child-output
limits, wall-time enforcement, and child-process cleanup.

Run with `python3 -B -m unittest discover -s tests_v2 -v` from the task
directory. The recorded output is `test-run.log`.
