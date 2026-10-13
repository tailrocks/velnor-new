"""Regression tests for bounded, source-aware compiled pin extraction."""

from contextlib import redirect_stdout
import io
from pathlib import Path
import tempfile
import unittest

from freshness_context import FreshnessContext, RUST_SOURCE_CAP


class RustPinExtractionTests(unittest.TestCase):
    def check_source(self, source, *, raw=False):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pins.rs"
            path.write_bytes(source if raw else source.encode("utf-8"))
            ctx = FreshnessContext(directory, "inventory.json", False, False)
            with redirect_stdout(io.StringIO()):
                value = ctx.rust_const("pins.rs", "PIN")
            return value, ctx.failures

    def test_reads_one_public_literal(self):
        value, failures = self.check_source(
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_ignores_line_and_nested_block_comment_decoys(self):
        value, failures = self.check_source(
            '// pub const PIN: &str = "old";\n'
            '/* outer /* pub const PIN: &str = "older"; */ end */\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_string_contents_cannot_supply_a_declaration(self):
        value, failures = self.check_source(
            'const NOTE: &str = "pub const PIN: &str = \\"decoy\\";";\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_raw_string_contents_cannot_supply_a_declaration(self):
        value, failures = self.check_source(
            'const NOTE: &str = r###"pub const PIN: &str = "decoy";"###;\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_cooked_and_raw_literal_families_cannot_supply_declarations(self):
        value, failures = self.check_source(
            r'const BYTE = b"pub const PIN: &str = \"byte\";";' + "\n"
            r'const RAW_BYTE = br#"pub const PIN: &str = "raw byte";"#;' + "\n"
            r'const C = c"pub const PIN: &str = \"C string\";";' + "\n"
            r'const RAW_C = cr##"pub const PIN: &str = "raw C string";"##;' + "\n"
            r'const C_HEX = c"\xff";' + "\n"
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_macro_body_declaration_does_not_satisfy_pin(self):
        value, failures = self.check_source(
            'macro_rules! fake { () => { pub const PIN: &str = "1.2.3"; } }\n')
        self.assertIsNone(value)
        self.assertEqual(len(failures), 1)
        self.assertIn("missing const PIN", failures[0])

    def test_macro_rules_all_delimiters_hide_body_declarations(self):
        for delimiter in ("(", "[", "{"):
            with self.subTest(delimiter=delimiter):
                closing = {"(": ")", "[": "]", "{": "}"}[delimiter]
                value, failures = self.check_source(
                    f'macro_rules! fake {delimiter} () => '
                    f'{{ pub const PIN: &str = "decoy"; }} {closing}\n')
                self.assertIsNone(value)
                self.assertEqual(len(failures), 1)
                self.assertIn("missing const PIN", failures[0])

    def test_raw_c_braces_inside_macro_body_do_not_end_the_group(self):
        value, failures = self.check_source(
            'macro_rules! fake { () => { '
            'const NOTE = cr##"}} pub const PIN: &str = "decoy"; {{"##; '
            'pub const PIN: &str = "macro decoy"; }; }\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_nested_cfg_module_and_associated_const_are_not_authorities(self):
        nested = (
            '#[cfg(feature = "alternate")]\n'
            'mod alternate { pub const PIN: &str = "module decoy"; }\n'
            '#[cfg_attr(feature = "alternate", cfg(feature = "other"))]\n'
            'mod alternate_again { pub const PIN: &str = "cfg_attr decoy"; }\n'
            'struct Holder;\n'
            'impl Holder { pub const PIN: &str = "associated decoy"; }\n')
        value, failures = self.check_source(nested)
        self.assertIsNone(value)
        self.assertEqual(len(failures), 1)
        self.assertIn("missing const PIN", failures[0])

        value, failures = self.check_source(
            nested + 'pub const PIN: &str = "1.2.3";\n')
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_complete_byte_and_unicode_char_escapes_are_consumed(self):
        value, failures = self.check_source(
            "const BYTE: u8 = b'\\x41';\n"
            "const CHAR: char = '\\u{1f642}';\n"
            "pub const PIN: &str = \"1.2.3\";\n")
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_non_ascii_hex_escape_remains_invalid_in_rust_string(self):
        value, failures = self.check_source(
            r'const NOTE = "\xff";' + "\n"
            'pub const PIN: &str = "1.2.3";\n')
        self.assertIsNone(value)
        self.assertIn("hexadecimal escape", failures[0])

    def test_commented_declaration_does_not_satisfy_pin(self):
        value, failures = self.check_source(
            '// pub const PIN: &str = "1.2.3";\n')
        self.assertIsNone(value)
        self.assertEqual(len(failures), 1)
        self.assertIn("missing const PIN", failures[0])

    def test_duplicate_declarations_fail_closed(self):
        value, failures = self.check_source(
            'pub const PIN: &str = "1.2.3";\n'
            'pub const PIN: &str = "9.9.9";\n')
        self.assertIsNone(value)
        self.assertEqual(len(failures), 1)
        self.assertIn("duplicate const PIN", failures[0])

    def test_conditional_authority_fails_closed(self):
        value, failures = self.check_source(
            '#[cfg(feature = "alternate")]\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertIsNone(value)
        self.assertIn("unsupported authority attribute", failures[0])

    def test_cfg_attr_target_and_conditional_crate_attrs_fail_closed(self):
        target, failures = self.check_source(
            '#[cfg_attr(feature = "alternate", cfg(feature = "other"))]\n'
            'pub const PIN: &str = "1.2.3";\n')
        self.assertIsNone(target)
        self.assertIn("unsupported authority attribute", failures[0])

        for crate_attr in ("#![cfg(feature = \"alternate\")]",
                           "#![cfg_attr(feature = \"alternate\", cfg(test))]",
                           "#![r#cfg(any())]",
                           "#![r#cfg_attr(feature = \"alternate\", r#cfg(test))]"):
            with self.subTest(crate_attr=crate_attr):
                value, failures = self.check_source(
                    f'{crate_attr}\n'
                    'pub const PIN: &str = "1.2.3";\n')
                self.assertIsNone(value)
                self.assertIn("conditional crate attribute", failures[0])

    def test_escaped_pin_literal_fails_closed(self):
        value, failures = self.check_source(
            'pub const PIN: &str = "1\\x2e2.3";\n')
        self.assertIsNone(value)
        self.assertIn("plain string literal", failures[0])

    def test_source_read_is_bounded(self):
        value, failures = self.check_source(
            " " * (RUST_SOURCE_CAP + 1))
        self.assertIsNone(value)
        self.assertIn("source exceeds", failures[0])

    def test_source_at_exact_cap_is_accepted(self):
        declaration = b'pub const PIN: &str = "1.2.3";\n'
        source = declaration + b" " * (RUST_SOURCE_CAP - len(declaration))
        value, failures = self.check_source(source, raw=True)
        self.assertEqual(value, "1.2.3")
        self.assertEqual(failures, [])

    def test_invalid_utf8_fails_closed(self):
        value, failures = self.check_source(b"\xff", raw=True)
        self.assertIsNone(value)
        self.assertIn("unsupported Rust source", failures[0])


if __name__ == "__main__":
    unittest.main()
