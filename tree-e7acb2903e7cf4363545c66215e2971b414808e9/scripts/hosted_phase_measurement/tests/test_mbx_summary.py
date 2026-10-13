"""Exercise MBX parsing against every source-bound summary in the retained run."""

from __future__ import annotations

import json
import re
import sys
import unittest
from collections import Counter
from pathlib import Path

PACKAGE_PARENT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PACKAGE_PARENT))
from hosted_phase_measurement import measure_hosted_run as measure  # noqa: E402
from hosted_phase_measurement.mbx_summary import parse_mbx_summary  # noqa: E402
from hosted_phase_measurement.report_counters import build_actual_counters  # noqa: E402


FIXTURE = Path(__file__).parent / "fixtures" / "mbx-summary-exact-joined-v1.jsonl"
EXPECTED_META = {
    "fixture_schema": "velnor.hosted-phase-measurement.mbx-summary-fixture.v1",
    "source_run_id": 37163556069,
    "run_attempt": 1,
    "head_sha": "47815c83b9eeadbaf84b741918fffa7ea550da89",
    "jobs_api_sha256": "1461daae403ed62b3c4b1df2627706d07094f80989dd61af21d548c5ba9dcc01",
    "logs_zip_sha256": "d6d28d04333c48d014c758d7348147c5d4266c6a1ed866d925ee41e21dcb32ae",
    "api_step_rows": 420,
    "active_exact_log_joins": 396,
    "skipped_api_step_rows": 24,
    "summary_count": 60,
}
LOOKUP_TOTALS = {"hits": 1326, "misses": 10, "not_looked_up": 2585, "bypassed": 59}


def read_fixture() -> tuple[dict, list[dict]]:
    rows = [json.loads(line) for line in FIXTURE.read_text(encoding="utf-8").splitlines()]
    return rows[0], rows[1:]


class MbxSummaryTest(unittest.TestCase):
    def test_all_60_exact_joined_source_summaries_and_report_totals(self):
        metadata, source_rows = read_fixture()
        self.assertEqual(metadata, EXPECTED_META)
        self.assertEqual(len(source_rows), 60)
        self.assertEqual(len({(row["job_id"], row["step_number"], row["line_number"]) for row in source_rows}), 60)
        observations = []
        suffixes = Counter()
        report_steps = []
        for source in source_rows:
            self.assertTrue(source["job_name"])
            self.assertTrue(source["step_name"])
            self.assertEqual(len(source["log_sha256"]), 64)
            self.assertGreater(source["line_number"], 0)
            local_unit = re.search(r"\b([A-Za-z]+)\s+stored locally\s*[.!]?\s*$", source["raw_line"], re.I)
            self.assertIsNotNone(local_unit)
            suffixes[local_unit.group(1).lower()] += 1
            evidence = measure.extract_log_evidence((source["raw_line"] + "\n").encode(), "other", source["log_file"])
            self.assertEqual(len(evidence["mbx_object_cache_summaries"]), 1)
            summary = evidence["mbx_object_cache_summaries"][0]
            observations.append(summary)
            report_steps.append({"phase": "other", "conclusion": "success", "log_evidence": evidence})
        self.assertEqual(suffixes, {"mib": 45, "b": 12, "gib": 3})
        counters = build_actual_counters(report_steps, {"runner_queue_unknown_reason": "not measured in this fixture"})
        totals = counters["mbx_object_cache_summary_totals"]
        self.assertEqual(totals["observation_count"], 60)
        self.assertEqual(totals["lookup_totals"], LOOKUP_TOTALS)
        self.assertEqual(totals["remote_downloaded_bytes"], 0)
        self.assertEqual(totals["remote_uploaded_bytes"], 0)
        self.assertEqual(sum(row["remote_downloaded_bytes"] for row in observations), 0)
        self.assertEqual(sum(row["remote_uploaded_bytes"] for row in observations), 0)

    def test_supported_decimal_and_binary_human_units(self):
        row = parse_mbx_summary(
            "mbx[cache]: object cache: 1 hit, 0 misses, 2 not looked up, 3 bypassed; "
            "1.5 KiB downloaded, 2 MiB uploaded, 1.3 GiB stored locally"
        )
        self.assertEqual(row["remote_downloaded_bytes"], 1536)
        self.assertEqual(row["remote_uploaded_bytes"], 2 << 20)
        self.assertAlmostEqual(row["local_store_mib"], 1.3 * 1024)
        self.assertEqual(row["counts"], {"hits": 1, "misses": 0, "not_looked_up": 2, "bypassed": 3})

    def test_supported_decimal_si_and_byte_units(self):
        row = parse_mbx_summary(
            "mbx[cache]: object cache: 1 hit; 0.1 kB downloaded, 1 MB uploaded, 0 B stored locally"
        )
        self.assertEqual(row["remote_downloaded_bytes"], 100)
        self.assertEqual(row["remote_uploaded_bytes"], 1_000_000)
        self.assertEqual(row["local_store_mib"], 0.0)

    def test_non_summary_line_is_ignored(self):
        self.assertIsNone(parse_mbx_summary("ordinary cache log without the summary marker"))

    def test_marked_malformed_rows_fail_instead_of_disappearing(self):
        lines = (
            "mbx[cache]: object cache: 3 lookups; 0 B downloaded, 0 B uploaded, 0 B stored locally",
            "mbx[cache]: object cache: 1 hit; 0 XB downloaded, 0 B uploaded, 0 B stored locally",
            "mbx[cache]: object cache: 1 hit; 0 B downloaded, 0 B uploaded, 0 XB stored locally",
            "mbx[cache]: object cache: 1 hit; 0 B downloaded, 0.5 B uploaded, 0 B stored locally",
            "mbx[cache]: object cache: 1 hit; 0 B downloaded 0 B uploaded, 0 B stored locally",
            "mbx[cache]: object cache: 1 hit, 2 hits; 0 B downloaded, 0 B uploaded, 0 B stored locally",
        )
        for line in lines:
            with self.subTest(line=line), self.assertRaises(ValueError):
                measure.extract_log_evidence((line + "\n").encode(), "other", "fixture.txt")


if __name__ == "__main__":
    unittest.main()
