#!/usr/bin/env python3
"""Write the deterministic source/lock-derived seed plan."""

import sys

sys.dont_write_bytecode = True

from seed_candidate import main_plan


if __name__ == "__main__":
    raise SystemExit(main_plan())
