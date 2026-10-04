"""Native governing observations stay fixed across untrusted callbacks."""
import copy
import unittest
from unittest.mock import patch

import release_prepare_semver_test as fixtures

ERROR, NAMESPACE = fixtures.ERROR, fixtures.NAMESPACE


class SemverContextTests(unittest.TestCase):
    setUp = fixtures.SemverOrchestrationTests.setUp
    inventory_hash = fixtures.SemverOrchestrationTests.inventory_hash
    metadata = fixtures.SemverOrchestrationTests.metadata
    docs = fixtures.SemverOrchestrationTests.docs
    checker = fixtures.SemverOrchestrationTests.checker
    prepare = fixtures.SemverOrchestrationTests.prepare

    def test_exact_current_and_baseline_sides_required_before_callback(self):
        original = self.lock_contexts
        cases = ({}, {"current": original["current"]},
                 dict(original, unknown=original["current"]), list(original))
        for value in cases:
            with self.subTest(value=value):
                self.lock_contexts = value
                with patch.dict(NAMESPACE, acquire_registry_baseline=lambda *_:
                                self.fail("context validation must precede callback")):
                    with self.assertRaisesRegex(ERROR, "governing_lock_contexts"):
                        NAMESPACE["prepare_locked_semver"](self.current, "Cargo.toml",
                            "demo", "1.3.0", "1.2.3", self.actual, self.output, self.context,
                            "/sdk/checker", self.lock_contexts, lambda *_: None, self.checker)
                self.assertEqual(self.calls, [])

    def test_both_dtos_validated_before_callback(self):
        self.lock_contexts["baseline"]["unknown"] = "forged authority"
        with self.assertRaises(ERROR):
            self.prepare()
        self.assertEqual(self.calls, [])

    def test_callback_cannot_retarget_copied_contexts(self):
        expected = copy.deepcopy(self.lock_contexts)
        observed = []
        metadata, docs = self.metadata, self.docs

        def acquire(*_):
            self.lock_contexts["current"]["governingLockfile"] = "/different/Cargo.lock"
            self.lock_contexts["baseline"] = dict(expected["current"])
            self.lock_contexts["unknown"] = {"format": 1}
            return self.authenticated

        def read(*args):
            observed.append(dict(args[-2]))
            return metadata(*args)

        def generate(*args):
            observed.append(dict(args[-2]))
            return docs(*args)

        with patch.dict(NAMESPACE, acquire_registry_baseline=acquire,
                        read_locked_package=read, generate_locked_docs=generate):
            result = NAMESPACE["prepare_locked_semver"](self.current, "Cargo.toml", "demo",
                "1.3.0", "1.2.3", self.actual, self.output, self.context, "/sdk/checker",
                self.lock_contexts, lambda *_: self.fail("unexpected real tool"), self.checker)
        self.assertEqual(result["status"], "compatible")
        self.assertEqual(observed, [expected[side] for side in
                                  ("current", "baseline", "current", "baseline")])

    def test_frozen_inner_and_outer_mappings_reject_mutation(self):
        frozen = NAMESPACE["_semver_governing_contexts"](self.lock_contexts)
        with self.assertRaises(TypeError):
            frozen["current"] = self.lock_contexts["baseline"]
        with self.assertRaises(TypeError):
            frozen["baseline"]["governingLockfile"] = "/other/Cargo.lock"


if __name__ == "__main__":
    unittest.main()
