#!/usr/bin/env python3
"""Verify a lock-bound seed capsule without network or package execution."""

import sys

sys.dont_write_bytecode = True

from seed_candidate import main_verify


if __name__ == "__main__":
    raise SystemExit(main_verify())
