"""Cross-language bootstrap policy concordance; belongs to the catalog unit."""

import json
from pathlib import Path
import re
import sys
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts"))
import owned_tool_source as source


class BootstrapAuthorityTests(unittest.TestCase):
    def test_official_bootstrap_rows_match_catalog_authority_for_all_hosts(self):
        file = ROOT / "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs"
        text = file.read_text()
        names = {"LinuxAmd64": "x86_64-unknown-linux-gnu", "LinuxArm64": "aarch64-unknown-linux-gnu",
                 "MacosArm64": "aarch64-apple-darwin"}
        functions = {
            "mise": text.split("const fn mise(", 1)[1].split("const fn mbx(", 1)[0],
            "mbx": text.split("const fn mbx(", 1)[1].split("#[cfg(test)]", 1)[0],
        }
        for tool, function in functions.items():
            rows = re.findall(r'SourceBuildBootstrapHost::(\w+) => \(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)"', function)
            self.assertEqual(len(rows), 3)
            authority = source.official_assets(tool)
            for name, url, archive_sha, binary_sha in rows:
                target = names[name]
                asset = authority[target]
                self.assertEqual((asset["url"], asset["archive_sha256"], asset["binary_sha256"]),
                                 (url, archive_sha, binary_sha))
                for field in ("source_repository", "source_commit", "source_tree", "version", "binary_member"):
                    self.assertIn('"' + asset[field] + '"', text)
                supplied = {"mise": source.official_assets("mise")[target],
                            "mbx": source.official_assets("mbx")[target]}
                self.assertEqual(source.build_bootstrap_descriptor(json.dumps(supplied), target), supplied)


if __name__ == "__main__":
    unittest.main()
