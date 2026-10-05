"""Run the fixed freshness checks in their documented order."""

import sys


def main(argv):
    root, inventory, upstream, advisories, script_dir = argv[1:6]
    sys.path.insert(0, script_dir)
    from freshness_context import FreshnessContext
    from freshness_dependencies import check_effective_identity
    from freshness_evidence import (
        check_advisories,
        check_exceptions,
        check_recorded_freshness,
    )
    from freshness_inventory import (
        check_local_pins,
        check_policy_header,
        check_policy_mirror,
        load_inventory,
        load_policy,
    )
    from freshness_probe_check import check_upstream_probe
    from freshness_validation import check_validation_tool_pins

    ctx = FreshnessContext(root, inventory, upstream == "1", advisories == "1")
    if not load_inventory(ctx):
        return 1
    load_policy(ctx)
    check_policy_header(ctx)
    check_local_pins(ctx)
    check_policy_mirror(ctx)
    check_validation_tool_pins(ctx)
    check_effective_identity(ctx)
    check_recorded_freshness(ctx)
    check_exceptions(ctx)
    check_advisories(ctx)
    if ctx.check_upstream:
        check_upstream_probe(ctx)
    return ctx.finish()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
