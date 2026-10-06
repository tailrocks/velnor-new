"""Canonical JSON authority boundaries always report typed reconciliation errors."""
from pathlib import Path
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "src/release_reconcile_common.py"


class CommonJsonBoundaryTest(unittest.TestCase):
    def setUp(self):
        self.ns = {"__name__": "velnor_release_compiled"}
        exec(compile(SOURCE.read_bytes(), str(SOURCE), "exec"), self.ns)
        self.error = self.ns["ReconcileError"]

    def test_preserve_closed_duplicate_and_nonfinite_failures(self):
        for raw, code in (('{"key":1,"key":2}', "duplicate_json_key"),
                          ('{"key":NaN}', "nonfinite_json")):
            with self.subTest(raw=raw), self.assertRaisesRegex(self.error, code):
                self.ns["decode_json"](raw)

    def test_invalid_utf8_syntax_and_huge_int_are_typed(self):
        for raw in (b'{"key":"\xff"}', b'{"key":',
                    "1" * 10000, None):
            with self.subTest(size=len(raw) if raw is not None else None), \
                    self.assertRaisesRegex(self.error, "json_invalid"):
                self.ns["decode_json"](raw)

    def test_comparison_preserves_boolean_integer_distinction(self):
        self.assertFalse(self.ns["same_json"]({"schema": True}, {"schema": 1}))
        self.assertTrue(self.ns["same_json"]({"b": 2, "a": 1}, {"a": 1, "b": 2}))

    def test_invalid_comparison_shapes_and_cycles_are_typed(self):
        cycle = []
        cycle.append(cycle)
        for value in ({"value": float("nan")}, {"value": b"raw"}, cycle):
            with self.subTest(type=type(value).__name__), \
                    self.assertRaisesRegex(self.error, "json_invalid_comparison"):
                self.ns["same_json"](value, {})

    def test_parser_and_encoder_recursion_failures_are_typed(self):
        # Python 3.14 accepts valid JSON deeper than its Python recursion limit.
        # Exercise the documented exception boundary without assuming that limit.
        with patch.object(self.ns["json"], "loads", side_effect=RecursionError):
            with self.assertRaisesRegex(self.error, "json_invalid"):
                self.ns["decode_json"]("[]")
        with patch.object(self.ns["json"], "dumps", side_effect=RecursionError):
            with self.assertRaisesRegex(self.error, "json_invalid_comparison"):
                self.ns["same_json"]([], [])


if __name__ == "__main__":
    unittest.main()
