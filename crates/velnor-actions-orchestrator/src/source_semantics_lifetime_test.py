"""Source-owned semantic lifetime tests; fixtures never qualify native work."""
import copy
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_sdk as sdk_owner
import source_semantics_lifetime as lifetime_owner
from source_intent_cold_common import ColdSourceIntent


class _SyntheticExecutor:
    def __init__(self, events):
        self.events = events
        self.current = True

    def require_current(self):
        self.events.append("executor-current")
        if not self.current:
            raise ColdSourceIntent("fixture_executor_stale")


class _SyntheticOwner:
    compiled_role = "SourceSemantics"

    def __init__(self, sdk, executor, events):
        self.sdk = sdk
        self.executor = executor
        self.events = events
        self.lifetime = None
        self.closed = False
        self.close_saw_revocation = False
        self.close_error = None

    def require_current(self):
        self.events.append("owner-current")

    def compiler_sdk(self):
        return self.sdk

    def native_executor(self):
        return self.executor

    def close(self):
        self.close_saw_revocation = not lifetime_owner._LIVE_REGISTRY
        self.events.append("owner-close")
        self.closed = True
        if self.close_error is not None:
            raise self.close_error


def _sdk_fixture():
    sdk = object.__new__(sdk_owner.ColdSourceIntentSdk)
    object.__setattr__(sdk, "_seal", sdk_owner._SDK_SEAL)
    return sdk


class SourceSdkLifetimeTests(unittest.TestCase):
    def setUp(self):
        lifetime_owner._LIVE_REGISTRY.clear()
        lifetime_owner._LIVE_LIFETIME = None
        for name in ("_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE",
                     "_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER",
                     "_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE"):
            setattr(lifetime_owner, name, None)
        self.sdk_current = patch.object(
            sdk_owner.ColdSourceIntentSdk, "require_current", return_value=None)
        self.sdk_current.start()
        self.addCleanup(self.sdk_current.stop)

    def _bind(self, sdk=None, executor=None):
        events = []
        sdk = sdk or _sdk_fixture()
        executor = executor or _SyntheticExecutor(events)
        owner = _SyntheticOwner(sdk, executor, events)
        holder = {"owner": owner, "getter_calls": 0}

        def getter():
            holder["getter_calls"] += 1
            return holder["owner"]

        lifetime_owner._COMPILED_SOURCE_SEMANTICS_OWNER_TYPE = _SyntheticOwner
        lifetime_owner._COMPILED_SOURCE_SEMANTICS_OWNER_GETTER = getter
        lifetime_owner._COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE = _SyntheticExecutor
        return owner, executor, holder

    def test_missing_or_foreign_owner_denies_before_issue(self):
        with self.assertRaisesRegex(ColdSourceIntent, "issuer_unavailable"):
            lifetime_owner.load_source_sdk_lifetime()
        owner, _executor, holder = self._bind()
        holder["owner"] = {}
        with self.assertRaisesRegex(ColdSourceIntent, "owner_origin"):
            lifetime_owner.load_source_sdk_lifetime()
        self.assertFalse(owner.closed)

    def test_issues_once_and_rejects_copy_and_clone(self):
        owner, executor, holder = self._bind()
        lifetime = lifetime_owner.load_source_sdk_lifetime()
        owner.lifetime = lifetime
        calls = holder["getter_calls"]
        lifetime.require_current()
        self.assertEqual(holder["getter_calls"], calls + 1)
        self.assertIs(lifetime.native_executor(), executor)
        with self.assertRaisesRegex(ColdSourceIntent, "already_issued"):
            lifetime_owner.load_source_sdk_lifetime()
        with self.assertRaisesRegex(ColdSourceIntent, "copy"):
            copy.copy(lifetime)
        with self.assertRaisesRegex(ColdSourceIntent, "copy"):
            copy.deepcopy(lifetime)
        clone = object.__new__(type(lifetime))
        with self.assertRaisesRegex(ColdSourceIntent, "authority"):
            clone.require_current()
        with self.assertRaisesRegex(ColdSourceIntent, "authority"):
            lifetime_owner.SourceSdkLifetime.close(object())
        self.assertIs(holder["owner"], owner)

    def test_getter_identity_and_rebound_sdk_or_executor_deny(self):
        owner, executor, holder = self._bind()
        lifetime = lifetime_owner.load_source_sdk_lifetime()
        original_getter = lifetime_owner._COMPILED_SOURCE_SEMANTICS_OWNER_GETTER
        lifetime_owner._COMPILED_SOURCE_SEMANTICS_OWNER_GETTER = lambda: owner
        with self.assertRaisesRegex(ColdSourceIntent, "binding_changed"):
            lifetime.require_current()
        lifetime_owner._COMPILED_SOURCE_SEMANTICS_OWNER_GETTER = original_getter
        holder["owner"] = _SyntheticOwner(owner.sdk, executor, owner.events)
        with self.assertRaisesRegex(ColdSourceIntent, "owner_changed"):
            lifetime.require_current()
        holder["owner"] = owner
        owner.sdk = _sdk_fixture()
        with self.assertRaisesRegex(ColdSourceIntent, "sdk_changed"):
            lifetime.require_current()
        owner.sdk = lifetime._sdk
        owner.executor = _SyntheticExecutor(owner.events)
        with self.assertRaisesRegex(ColdSourceIntent, "executor_changed"):
            lifetime.require_current()

    def test_stale_sdk_close_revokes_before_owner_cleanup(self):
        owner, executor, _holder = self._bind()
        lifetime = lifetime_owner.load_source_sdk_lifetime()
        owner.lifetime = lifetime
        owner.sdk = _sdk_fixture()
        lifetime.close()
        self.assertTrue(owner.closed)
        self.assertTrue(owner.close_saw_revocation)
        self.assertEqual(lifetime_owner._LIVE_REGISTRY, {})
        with self.assertRaisesRegex(ColdSourceIntent, "authority"):
            lifetime.require_current()
        with self.assertRaisesRegex(ColdSourceIntent, "authority"):
            lifetime.close()
        with self.assertRaisesRegex(ColdSourceIntent, "already_issued"):
            lifetime_owner.load_source_sdk_lifetime()

    def test_every_mutated_slot_denies_use_but_closes_original_owner(self):
        for slot in ("_owner", "_sdk", "_executor", "_owner_type",
                     "_owner_getter", "_executor_type", "_seal"):
            with self.subTest(slot=slot):
                lifetime_owner._LIVE_REGISTRY.clear()
                lifetime_owner._LIVE_LIFETIME = None
                owner, _executor, _holder = self._bind()
                lifetime = lifetime_owner.load_source_sdk_lifetime()
                object.__setattr__(lifetime, slot, object())
                with self.assertRaisesRegex(ColdSourceIntent,
                                             "authority|binding_changed|owner_changed"):
                    lifetime.require_current()
                with self.assertRaisesRegex(ColdSourceIntent,
                                             "authority|binding_changed|owner_changed"):
                    lifetime.native_executor()
                lifetime.close()
                self.assertTrue(owner.closed)
                self.assertTrue(owner.close_saw_revocation)
                self.assertEqual(lifetime_owner._LIVE_REGISTRY, {})

    def test_cleanup_error_still_revokes_before_original_owner_close(self):
        owner, _executor, _holder = self._bind()
        lifetime = lifetime_owner.load_source_sdk_lifetime()
        owner.close_error = ColdSourceIntent("fixture_cleanup")
        with self.assertRaisesRegex(ColdSourceIntent, "fixture_cleanup"):
            lifetime.close()
        self.assertTrue(owner.closed)
        self.assertTrue(owner.close_saw_revocation)
        self.assertEqual(lifetime_owner._LIVE_REGISTRY, {})
        with self.assertRaisesRegex(ColdSourceIntent, "authority"):
            lifetime.close()

    def test_later_sdk_failure_closes_acquired_owner(self):
        owner, _executor, _holder = self._bind(sdk=object())
        with self.assertRaisesRegex(ColdSourceIntent, "sdk_origin"):
            lifetime_owner.load_source_sdk_lifetime()
        self.assertTrue(owner.closed)

    def test_later_executor_failure_closes_acquired_owner(self):
        owner, _executor, _holder = self._bind(executor=object())
        with self.assertRaisesRegex(ColdSourceIntent, "executor_origin"):
            lifetime_owner.load_source_sdk_lifetime()
        self.assertTrue(owner.closed)

    def test_post_registration_failure_revokes_and_closes(self):
        owner, executor, _holder = self._bind()
        with patch.object(lifetime_owner.SourceSdkLifetime, "require_current",
                          side_effect=ColdSourceIntent("fixture_post_registration")), \
                self.assertRaisesRegex(ColdSourceIntent, "fixture_post_registration"):
            lifetime_owner.load_source_sdk_lifetime()
        self.assertTrue(owner.closed)
        self.assertTrue(owner.close_saw_revocation)
        self.assertEqual(lifetime_owner._LIVE_REGISTRY, {})
        self.assertIsNot(lifetime_owner._LIVE_LIFETIME, lifetime_owner._ISSUING)
        self.assertIsNotNone(executor)


if __name__ == "__main__":
    unittest.main()
