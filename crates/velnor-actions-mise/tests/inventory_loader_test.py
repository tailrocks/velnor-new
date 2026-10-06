"""Fixed-loader units; the host interpreter is not Foundation qualification."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "src"


def fixed_records():
    mapping = (SOURCE / "source_archive_inventory.rs").read_text()
    pairs = re.findall(r'\(\s*"([^"]+)"\s*,\s*include_str!\("([^"]+)"\),?\s*\)', mapping)
    return [{"name": name, "source": (SOURCE / filename).read_text(),
             "sha256": hashlib.sha256((SOURCE / filename).read_bytes()).hexdigest()}
            for name, filename in pairs]


def framed_digest(records):
    digest = hashlib.sha256()
    values = [b"velnor-source-archive-inventory-template-v1"]
    for value in values:
        digest.update(len(value).to_bytes(8, "big"))
        digest.update(value)
    digest.update(len(records).to_bytes(8, "big"))
    for record in records:
        for value in (record["name"].encode(), record["source"].encode()):
            digest.update(len(value).to_bytes(8, "big"))
            digest.update(value)
    return digest.hexdigest()


def program(records, expected=None, schema=1):
    rust = (SOURCE / "inventory_loader.rs").read_text()
    preload = rust.split('const PRELOAD: &str = r#"', 1)[1].rsplit('"#;', 1)[0]
    order = json.dumps([record["name"] for record in fixed_records()]).encode().hex()
    capsule = json.dumps({"schema": schema, "modules": records}).encode().hex()
    expected = framed_digest(records) if expected is None else expected
    return ("import hashlib,json,sys,types\n"
            f"_INVENTORY_ORDER_HEX='{order}'\n_INVENTORY_CAPSULE_HEX='{capsule}'\n"
            f"_INVENTORY_TEMPLATE_SHA256='{expected}'\n" + preload)


class LoaderTests(unittest.TestCase):
    def run_loader(self, source):
        return subprocess.run([sys.executable, "-I", "-S", "-c", source],
                              capture_output=True, text=True, check=False)

    def canary_records(self):
        records = fixed_records()
        records[0]["source"] = "raise RuntimeError('inventory_payload_executed')\n"
        records[0]["sha256"] = hashlib.sha256(records[0]["source"].encode()).hexdigest()
        return records

    def test_actual_fixed_eight_module_isolated_preload(self):
        records = fixed_records()
        self.assertEqual(len(records), 8)
        result = self.run_loader(program(records) + "\nprint(len(_inventory_order))\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "8\n")

    def test_last_asset_failure_precedes_first_asset_execution(self):
        records = self.canary_records()
        expected = framed_digest(records)
        records[-1]["sha256"] = "0" * 64
        result = self.run_loader(program(records, expected))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("inventory_source_asset_digest", result.stderr)
        self.assertNotIn("inventory_payload_executed", result.stderr)

    def test_rehashed_asset_cannot_change_fixed_ordered_closure(self):
        records = self.canary_records()
        expected = framed_digest(records)
        records[-1]["source"] += "\n# mutated source\n"
        records[-1]["sha256"] = hashlib.sha256(records[-1]["source"].encode()).hexdigest()
        result = self.run_loader(program(records, expected))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("inventory_source_template_digest", result.stderr)
        self.assertNotIn("inventory_payload_executed", result.stderr)

    def test_unknown_order_missing_asset_and_boolean_schema_fail_before_exec(self):
        for change in ("name", "order", "count", "schema"):
            with self.subTest(change=change):
                records = self.canary_records()
                schema = 1
                if change == "name":
                    records[-1]["name"] = "repository_supplied_module"
                elif change == "order":
                    records[-1], records[-2] = records[-2], records[-1]
                elif change == "count":
                    records.pop()
                else:
                    schema = True
                result = self.run_loader(program(records, schema=schema))
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("inventory_payload_executed", result.stderr)


if __name__ == "__main__":
    unittest.main()
