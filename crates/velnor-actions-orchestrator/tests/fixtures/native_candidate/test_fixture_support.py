# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import unittest
from fixture_support import approved_baseline, index_path


class FixtureSupportTest(unittest.TestCase):
    def test_index_paths(self):
        for name, expected in [("a", "/1/a"), ("ab", "/2/ab"), ("abc", "/3/a/abc"), ("abcd", "/ab/cd/abcd"), ("AbCd", "/ab/cd/abcd")]:
            self.assertEqual(index_path(name), expected)

    def test_only_exact_non_yanked_baseline_is_accepted(self):
        item = {"name": "example", "vers": "1.0.0", "cksum": "abc", "yanked": False}
        approved_baseline([item], "example", "1.0.0", "abc")
        for bad in [[], [item, item], [{**item, "yanked": True}], [{**item, "cksum": "wrong"}], [{key: value for key, value in item.items() if key != "yanked"}]]:
            with self.assertRaises(ValueError):
                approved_baseline(bad, "example", "1.0.0", "abc")


if __name__ == "__main__":
    unittest.main()
