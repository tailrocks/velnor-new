"""MBX object-cache summaries parse with and without the `object cache:` label.

mbx 1.30.0 prints the label-less form; older runs print the labelled form. A
summary-shaped line that does not parse raises instead of being counted as zero.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from hosted_phase_measurement import mbx_summary as M

LABELLED = "mbx[cache]: object cache: 594 hits, 11 misses; 0 B downloaded, 0 B uploaded, 5.8 MiB stored locally"
UNLABELLED = "mbx[cache]: 1 hits, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally"
PINNED_INCREMENTAL = "mbx[cache]: 0 hits, 4 misses, 13 not looked up, 15 incremental; 0 B downloaded, 0 B uploaded, 16.5 KiB stored locally"
UNKNOWN_COUNTER = "mbx[cache]: 3 frobs, 1 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally"
LABELLESS_INCREMENTAL_NO_HITS = "mbx[cache]: 0 misses, 15 incremental; 0 B downloaded, 0 B uploaded, 0 B stored locally"
SINGULAR_HIT = "mbx[cache]: 1 hit, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally"


class ParseMbxSummaryTests(unittest.TestCase):
    def test_labelled_summary_line_parses(self):
        row = M.parse_mbx_summary(LABELLED)
        self.assertEqual(row["counts"], {"hits": 594, "misses": 11})
        self.assertEqual(row["remote_downloaded_bytes"], 0)
        self.assertEqual(row["remote_uploaded_bytes"], 0)
        self.assertAlmostEqual(row["local_store_mib"], 5.8)

    def test_label_less_summary_line_parses(self):
        row = M.parse_mbx_summary(UNLABELLED)
        self.assertEqual(row["counts"], {"hits": 1, "misses": 0})
        self.assertEqual(row["remote_downloaded_bytes"], 0)
        self.assertEqual(row["remote_uploaded_bytes"], 0)
        self.assertEqual(row["local_store_mib"], 0.0)

    def test_pinned_1_30_summary_with_incremental_counter_parses(self):
        row = M.parse_mbx_summary(PINNED_INCREMENTAL)
        self.assertEqual(
            row["counts"],
            {"hits": 0, "misses": 4, "not_looked_up": 13, "incremental": 15},
        )
        self.assertEqual(row["remote_downloaded_bytes"], 0)
        self.assertEqual(row["remote_uploaded_bytes"], 0)
        self.assertAlmostEqual(row["local_store_mib"], 16.5 / 1024)

    def test_label_less_summary_with_unknown_counter_word_raises_naming_the_line(self):
        with self.assertRaises(ValueError) as raised:
            M.parse_mbx_summary(UNKNOWN_COUNTER)
        self.assertIn("frobs", str(raised.exception))
        self.assertIn(UNKNOWN_COUNTER, str(raised.exception))

    def test_label_less_summary_without_hits_keeps_only_printed_counters(self):
        row = M.parse_mbx_summary(LABELLESS_INCREMENTAL_NO_HITS)
        self.assertEqual(row["counts"], {"misses": 0, "incremental": 15})
        self.assertNotIn("hits", row["counts"])

    def test_label_less_summary_with_singular_hit_parses(self):
        row = M.parse_mbx_summary(SINGULAR_HIT)
        self.assertEqual(row["counts"], {"hits": 1, "misses": 0})

    def test_non_summary_mbx_lines_are_ignored(self):
        self.assertIsNone(M.parse_mbx_summary("mbx[cache]: 28.53s session"))
        self.assertIsNone(M.parse_mbx_summary("Compiling serde v1.0.0"))

    def test_malformed_label_less_summary_fails_instead_of_counting_zero(self):
        with self.assertRaises(ValueError):
            M.parse_mbx_summary("mbx[cache]: 1 hits, 0 misses; 0 B downloaded, 0 B uploaded")

    def test_malformed_labelled_summary_still_fails(self):
        with self.assertRaises(ValueError):
            M.parse_mbx_summary("mbx[cache]: object cache: 1 hits, 0 misses; 0 B downloaded")


if __name__ == "__main__":
    unittest.main()
