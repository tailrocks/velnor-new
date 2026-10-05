# Preserved stale-pyc probe snapshot

This is a preservation-only copy of the bounded synthetic PR88 import-cache probe, captured against source commit `d3ed26f1a47aa67c8296d00607472f5c63116365`. `reproduce.py`, both fixture source trees, both identical 10 KiB synthetic archives, the synthetic CPython 3.9 bytecode fixture, and the original tree manifests are copied byte-for-byte from the task-owned temporary directory.

The harness installs benign bytecode that prints a sentinel and raises, with source timestamp and size fields held to the fixture values. This preserves the reproduction setup; it is not a source fix, product gate, or qualification result. The private execution logs are excluded from this Git snapshot and retained locally with mode 0600.
