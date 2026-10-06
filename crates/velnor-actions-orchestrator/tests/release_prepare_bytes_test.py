"""Pure byte proof regressions; no Cargo, Git, or network execution."""
from pathlib import Path
import unittest


class ProofError(Exception):
    pass


def require(condition, reason):
    if not condition:
        raise ProofError(reason)


NS = {"require": require, "__name__": "byte_proof_test"}
SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_prepare_bytes.py"
exec(compile(SOURCE.read_text(), str(SOURCE), "exec"), NS)


class PreparationBytesTests(unittest.TestCase):
    def test_toml_shape_preserves_types_and_datetime_metadata(self):
        before = b'[package]\nname="demo"\nversion="1.0.0"\n[package.metadata]\ncreated=2026-01-02T12:00:00Z\nflag=false\n'
        after = before.replace(b"1.0.0", b"1.1.0")
        NS["preparation_bytes"](before, after, "Cargo.toml", {"demo"})
        for mutation in [after.replace(b"flag=false", b"flag=0"),
                         after.replace(b"2026-01-02", b"2026-01-03")]:
            with self.assertRaisesRegex(ProofError, "proposal_nonversion_edit"):
                NS["preparation_bytes"](before, mutation, "Cargo.toml", {"demo"})
        inherited = b'[package]\nname="demo"\nversion={workspace=true}\n'
        with self.assertRaisesRegex(ProofError, "proposal_nonversion_edit"):
            NS["preparation_bytes"](inherited, inherited.replace(b"true", b"1"), "Cargo.toml", {"demo"})

    def test_strict_semver(self):
        for version in ["0.0.0", "1.2.3-alpha.1", "1.2.3-0", "1.2.3+001.build"]:
            NS["_require_version"](version)
        for version in ["01.0.0", "1.01.0", "1.0.01", "1.0.0-.",
                        "1.0.0-alpha..beta", "1.0.0-01", "1.0.0+", "1.0.0+.",
                        "1.0", "1.0.0\n", 123, {"version": "1.0.0"}]:
            with self.subTest(version=version), self.assertRaises(ProofError):
                NS["_require_version"](version)

    def test_supported_cargo_requirement_grammar(self):
        for version in ["1", "1.2", "1.2.3", "=1.2.3", ">1", ">=1.2",
                        "<2", "<=2.0", "^1.2.3", "~1.2.3", "*", "1.*", "1.2.*",
                        ">=1.2.3, <2.0.0", "=1.2.3-alpha.1", "x", "X", "1.x", "1.2.X",
                        "1.*.*", "^1.*", "=1.*", "=1.*.*", " > 1.2 "]:
            NS["_require_version"](version, True)
        for version in [">>1.0.0", "~~1.0.0", "==1.0.0", "<>1.0.0", "=>1",
                        "1.*.3", "01.2", "1.2.3-01", "", ",1", "1,",
                        "1 || 2", "1.2.3 garbage", "*, ^1", "\t1", "1\t", "1,\n2",
                        "1.2-alpha", "18446744073709551616", ",".join(["1"] * 33)]:
            with self.subTest(version=version), self.assertRaises(ProofError):
                NS["_require_version"](version, True)

    def test_registry_dependencies_with_selected_name_remain_frozen(self):
        for dependency in ['demo="1"', 'demo={version="1"}',
                           'alias={package="demo",version="1"}',
                           'demo={git="https://example.invalid/demo",version="1"}']:
            before = ('[dependencies]\n' + dependency + '\n').encode()
            after = before.replace(b'"1"', b'"2"')
            with self.subTest(dependency=dependency), self.assertRaises(ProofError):
                NS["preparation_bytes"](before, after, "Cargo.toml", {"demo"})

    def test_selected_local_alias_updates_only_requirement(self):
        before = b'[dependencies]\nalias={package="demo",path="../demo",version="1"}\n'
        after = before.replace(b'version="1"', b'version="2"')
        NS["preparation_bytes"](before, after, "Cargo.toml", {"demo"})
        for mutation in [after.replace(b'../demo', b'../other'),
                         after.replace(b'version="2"', b'version={bad="value"}'),
                         after.replace(b'version="2"', b'version=">>2"')]:
            with self.assertRaises(ProofError):
                NS["preparation_bytes"](before, mutation, "Cargo.toml", {"demo"})

    def test_lock_versions_bind_manifest_versions(self):
        raw = b'[[package]]\nname="demo"\nversion="1.2.3"\n'
        NS["preparation_lock_versions"](raw, {"demo": "1.2.3"})
        for mutation in [raw.replace(b'1.2.3', b'9.9.9'), raw + raw,
                         raw.replace(b'1.2.3', b'01.2.3')]:
            with self.assertRaises(ProofError):
                NS["preparation_lock_versions"](mutation, {"demo": "1.2.3"})

    def test_unknown_local_and_registry_records_do_not_bind_workspace_versions(self):
        raw = (b'[[package]]\nname="external-path"\nversion="9.0.0"\n'
               b'[[package]]\nname="demo"\nversion="8.0.0"\n'
               b'source="registry+https://example.invalid"\n')
        NS["preparation_lock_versions"](raw, {"demo": "1.2.3"})

    def test_pinned_requirement_upgrade(self):
        examples = {"1": "2", "1.2": "2.3", "^1.2.3": "^2.3.4", "=1.2.3": "=2.3.4",
                    "~1.2": "~2.3", "1.*": "2.*", "1.2.*": "2.3.*", "*": "*",
                    "^1, ~1.2": "^2, ~2.3", " 1.2.3 ": "2.3.4", "x": "x", " X ": " X ",
                    "1.x": "2.*", "1.2.X": "2.3.*", "1.*.*": "2.*",
                    "=1.*": "=2", "=1.*.*": "=2", "~1.2.*": "~2.3", "^1.*": "^2"}
        for old, expected in examples.items():
            self.assertEqual(NS["_upgrade_requirement"](old, "2.3.4"), expected)
        self.assertEqual(NS["_upgrade_requirement"]("=1.2.3-alpha", "2.3.4-beta.1+build"),
                         "=2.3.4-beta.1")
        with self.assertRaises(ProofError):
            NS["_upgrade_requirement"](">=1", "2.3.4")
        self.assertEqual(NS["_upgrade_requirement"]("1", "2.3.4-beta.1"), "2")
        self.assertEqual(NS["_upgrade_requirement"]("~1.2", "2.3.4-beta.1"), "~2.3")
        self.assertEqual(NS["_upgrade_requirement"]("1.*", "2.3.4-beta.1"), "2.*")
        NS["_require_version"]("18446744073709551615.0.0")
        with self.assertRaises(ProofError):
            NS["_require_version"]("18446744073709551616.0.0")
        NS["_require_version"](",".join(["1"] * 32), True)

    def test_local_requirements_bind_source_path_and_next_version(self):
        consumer = b'[dependencies]\nalias={package="demo",path="../demo",version="1"}\n'
        package = b'[package]\nname="demo"\nversion="2.3.4"\n'
        before = {"crates/consumer/Cargo.toml": consumer, "crates/demo/Cargo.toml": package}
        after = {**before, "crates/consumer/Cargo.toml": consumer.replace(b'version="1"', b'version="2"')}
        NS["preparation_dependency_versions"](before, after, {"demo": "2.3.4"})
        invalid = {**after, "crates/consumer/Cargo.toml": consumer.replace(b'version="1"', b'version="9"')}
        with self.assertRaises(ProofError):
            NS["preparation_dependency_versions"](before, invalid, {"demo": "2.3.4"})
        before["crates/consumer/Cargo.toml"] = consumer.replace(b'../demo', b'../external')
        after["crates/consumer/Cargo.toml"] = after["crates/consumer/Cargo.toml"].replace(b'../demo', b'../external')
        with self.assertRaises(ProofError):
            NS["preparation_dependency_versions"](before, after, {"demo": "2.3.4"})

    def test_lock_local_references_bind_versions_and_registry_references_freeze(self):
        before = (b'[[package]]\nname="consumer"\nversion="1.0.0"\n'
                  b'dependencies=["demo 1.0.0 (registry+https://example.invalid)"]\n')
        after = before.replace(b'demo 1.0.0', b'demo 2.0.0')
        with self.assertRaises(ProofError):
            NS["preparation_bytes"](before, after, "Cargo.lock", {"demo"})
        local = before.replace(b' (registry+https://example.invalid)', b'')
        with self.assertRaises(ProofError):
            NS["preparation_lock_versions"](local, {"demo": "2.0.0"})


if __name__ == "__main__":
    unittest.main()
