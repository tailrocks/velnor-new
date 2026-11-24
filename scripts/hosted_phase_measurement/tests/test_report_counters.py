import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from hosted_phase_measurement import report_counters as R


def _step(counts):
    row = {"counts": counts, "remote_downloaded_bytes": 0, "remote_uploaded_bytes": 0}
    return {"phase": "build", "log_evidence": {"mbx_object_cache_summaries": [row]}}


class ActualCountersTests(unittest.TestCase):
    def test_lookup_totals_sum_incremental(self):
        steps = [_step({"hits": 1, "incremental": 15}), _step({"misses": 2, "incremental": 3})]
        totals = R.build_actual_counters(steps, {"runner_queue_unknown_reason": "none"})["mbx_object_cache_summary_totals"]["lookup_totals"]
        self.assertEqual(totals["incremental"], 18)
        self.assertEqual(totals["hits"], 1)


if __name__ == "__main__":
    unittest.main()
