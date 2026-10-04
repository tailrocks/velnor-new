import datetime
import os
import sys


MODULE_DIR = os.path.dirname(os.path.abspath(__file__))
if not sys.path or sys.path[0] != MODULE_DIR:
    sys.path.insert(0, MODULE_DIR)

import advisories
import evidence
import inventory
import lockfile
import pins
import probe
import report


def main():
    root, inv_path, upstream_arg, advisories_arg = sys.argv[1:5]
    report.failures.clear()
    now = datetime.datetime.now(datetime.timezone.utc)
    inv = inventory.load_inventory(inv_path)
    interval, max_days, top_checked = inventory.validate_inventory(inv)
    policy = inventory.load_policy(root, interval, max_days)
    tools, action_pinned, runner, supported = pins.run_local_pins(
        root, inv, policy)
    locked = lockfile.run_lock_checks(root)
    holds = inv.get("temporary_holds", [])
    evidence.run_upstream_freshness(
        tools, action_pinned, runner, interval, top_checked, now, holds)
    evidence.run_exceptions(inv, action_pinned, locked, supported,
                            runner, max_days, now)
    advisories.run_advisories(root, advisories_arg == "1")
    probe.run_upstream_probe(upstream_arg == "1", tools, action_pinned, now)
    return report.finish()


if __name__ == "__main__":
    sys.exit(main())
