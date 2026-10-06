"""Native governing-lock observation regressions; mocked SDK execution only."""
import base64
import unittest

import release_prepare_docs_test as fixtures


class GoverningContextTests(unittest.TestCase):
    setUp = fixtures.LockedDocsTests.setUp
    mock_run = fixtures.LockedDocsTests.mock_run
    generate = fixtures.LockedDocsTests.generate

    def test_closed_exact_versioned_observation_before_first_cargo(self):
        valid = self.lock_context
        cases = [dict(valid, unknown="claim"), dict(valid, format=True),
                 dict(valid, format=1.0), dict(valid, format=2), {},
                 dict(valid, workspaceManifest=123),
                 dict(valid, lockfileBytesBase64=""),
                 dict(valid, lockfileBytesBase64="AB=="),
                 dict(valid, lockfileBytesBase64=valid["lockfileBytesBase64"].rstrip("=")),
                 dict(valid, lockfileBytesBase64=valid["lockfileBytesBase64"] + "\n"),
                 dict(valid, lockfileBytesBase64=chr(0xd800))]
        for observation in cases:
            with self.subTest(observation=observation):
                self.lock_context = observation
                with self.assertRaises(ValueError):
                    self.generate()
        self.assertEqual(self.calls, [])

    def test_lock_full_bytes_must_match_before_first_cargo(self):
        self.lock_context["lockfileBytesBase64"] = base64.b64encode(b"different").decode()
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_lock_bytes"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_paths_are_lexically_canonical_and_within_source(self):
        valid = self.lock_context
        aliases = [str(self.root) + "//Cargo.lock", str(self.root) + "/./Cargo.lock",
                   str(self.root) + "/src/../Cargo.lock", "Cargo.lock", str(self.output)]
        link = self.root / "alias.lock"
        link.symlink_to("Cargo.lock")
        aliases.append(str(link))
        for alias in aliases:
            with self.subTest(alias=alias):
                self.lock_context = dict(valid, governingLockfile=alias)
                with self.assertRaises(ValueError):
                    self.generate()
        self.assertEqual(self.calls, [])

    def test_unrelated_lock_cannot_replace_missing_native_governing_lock(self):
        self.lock_context["governingLockfile"] = str(self.root / "native-missing.lock")
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_lock"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_native_nonconventional_governing_path_used_exactly(self):
        native = self.root / "native.lock"
        (self.root / "Cargo.lock").rename(native)
        self.lock_context["governingLockfile"] = str(native)
        result = self.generate()
        self.assertEqual(result["lock_path"], str(native))
        self.assertEqual(result["input_snapshots"][str(native)], b"version=4\n")

    def test_lock_mutation_after_metadata_stops_before_rustdoc(self):
        def mutate(argv, cwd, environment):
            raw = self.mock_run(argv, cwd, environment)
            (self.root / "Cargo.lock").write_bytes(b"changed")
            return raw

        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate(run=mutate)
        self.assertEqual(len(self.calls), 1)

    def test_lock_mutation_after_failed_metadata_is_checked(self):
        def fail(argv, cwd, environment):
            (self.root / "Cargo.lock").write_bytes(b"changed")
            raise RuntimeError("SDK failure")

        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate(run=fail)

    def test_callback_cannot_mutate_captured_observation(self):
        original = dict(self.lock_context)

        def mutate(argv, cwd, environment):
            self.lock_context["workspaceManifest"] = str(self.root / "foreign/Cargo.toml")
            return self.mock_run(argv, cwd, environment)

        result = self.generate(run=mutate)
        self.assertEqual(dict(result["governing_lock_context"]), original)
        with self.assertRaises(TypeError):
            result["governing_lock_context"]["format"] = 2

    def test_preacquisition_freeze_validates_shape_and_base64(self):
        frozen, raw = fixtures.NAMESPACE["_docs_freeze_lock_observation"](self.lock_context)
        self.assertEqual(raw, b"version=4\n")
        self.lock_context["format"] = 2
        self.assertEqual(frozen["format"], 1)
        again, _ = fixtures.NAMESPACE["_docs_freeze_lock_observation"](frozen)
        self.assertEqual(dict(again), dict(frozen))

    def test_lock_observation_byte_bound(self):
        freeze = fixtures.NAMESPACE["_docs_freeze_lock_observation"]
        observation = dict(self.lock_context,
                           lockfileBytesBase64=base64.b64encode(b"x" * (16 * 1024 * 1024)).decode())
        _, raw = freeze(observation)
        self.assertEqual(len(raw), 16 * 1024 * 1024)
        observation["lockfileBytesBase64"] = base64.b64encode(b"x" * (16 * 1024 * 1024 + 1)).decode()
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_bytes"):
            freeze(observation)

    def test_serialized_observation_size_bound(self):
        oversized = dict(self.lock_context, workspaceManifest="/" + "x" * (16 * 1024 * 1024),
                         governingLockfile="/" + "x" * (16 * 1024 * 1024))
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_size"):
            fixtures.NAMESPACE["_docs_freeze_lock_observation"](oversized)


if __name__ == "__main__":
    unittest.main()
