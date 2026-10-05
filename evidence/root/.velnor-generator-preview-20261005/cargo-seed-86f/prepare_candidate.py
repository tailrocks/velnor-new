#!/usr/bin/env python3
"""Acquire the exact pinned crates.io subset into a private capsule."""

import sys

sys.dont_write_bytecode = True

from seed_candidate import main_prepare


if __name__ == "__main__":
    raise SystemExit(main_prepare())
