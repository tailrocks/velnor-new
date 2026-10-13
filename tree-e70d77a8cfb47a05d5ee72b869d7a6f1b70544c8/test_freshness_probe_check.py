"""Offline fixtures for parsing upstream release pages and JSON APIs."""

import unittest

from freshness_probe_check import sniff_latest


class LatestReleaseTests(unittest.TestCase):
    def test_github_release_object(self):
        self.assertEqual(sniff_latest("https://api.github.com/repos/x/y/releases/latest",
                                      '{"tag_name":"v1.2.3"}'), "v1.2.3")

    def test_github_release_list_skips_unstable(self):
        body = '[{"tag_name":"v2.0.0","draft":true},' \
               '{"tag_name":"v1.2.3","prerelease":false}]'
        self.assertEqual(sniff_latest("https://api.github.com/releases", body),
                         "v1.2.3")

    def test_crates_api_uses_max_version(self):
        body = '{"crate":{"max_version":"0.12.23"}}'
        self.assertEqual(sniff_latest("https://crates.io/api/v1/crates/uv", body),
                         "0.12.23")

    def test_python_package_json_version(self):
        self.assertEqual(sniff_latest("https://pypi.org/pypi/python/json",
                                      '{"info":{"version":"3.14.8"}}'),
                         "3.14.8")

    def test_python_release_page(self):
        self.assertEqual(sniff_latest("https://www.python.org/downloads/",
                                      "<a>Download Python 3.14.8</a>"),
                         "3.14.8")

    def test_rust_release_page(self):
        body = '[pkg.rust]\nversion = "1.98.1"'
        self.assertEqual(sniff_latest("https://www.rust-lang.org/", body),
                         "1.98.1")


if __name__ == "__main__":
    unittest.main()
