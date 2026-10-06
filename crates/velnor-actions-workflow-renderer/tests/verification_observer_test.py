"""Offline tests for the fixed nightly issue observer."""

from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.util
import io
from pathlib import Path
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "src" / "verification_observer.py"
SPEC = importlib.util.spec_from_file_location("verification_observer_under_test", SOURCE)
OBSERVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OBSERVER)

REPOSITORY = "tailrocks/velnor-new"
SHA = "a" * 40
TITLE = "Nightly CI red"
SECRET = "ghp_observer_secret_never_publish"


def environment(result="failure"):
    return {
        "APPROVED_REPOSITORY": REPOSITORY,
        "GITHUB_REPOSITORY": REPOSITORY,
        "EVENT_NAME": "schedule",
        "DEFAULT_BRANCH": "main",
        "REF": "refs/heads/main",
        "REF_PROTECTED": "true",
        "SOURCE_SHA": SHA,
        "RUN_ID": "42",
        "RUN_ATTEMPT": "3",
        "REQUIRED_RESULT": result,
        "GH_TOKEN": SECRET,
    }


def issue(number, title=TITLE, state="open", pull_request=False):
    item = {"number": number, "title": title, "state": state}
    if pull_request:
        item["pull_request"] = {"url": "https://example.invalid/pull"}
    return item


class Api:
    """In-memory GitHub issues endpoint; no network access."""

    def __init__(self, pages=None):
        self.pages = pages or [[]]
        self.calls = []

    def __call__(self, method, path, payload=None):
        self.calls.append((method, path, copy.deepcopy(payload)))
        if method == "GET":
            page = int(path.split("page=")[-1])
            return copy.deepcopy(self.pages[page - 1])
        if method in ("POST", "PATCH"):
            return {"number": 99}
        raise AssertionError(f"unexpected method: {method}")

    def writes(self):
        return [call for call in self.calls if call[0] != "GET"]


class VerificationObserverTests(unittest.TestCase):
    def invoke(self, env, api):
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(output):
            OBSERVER.main(env=env, api=api)
        self.assertNotIn(SECRET, output.getvalue())
        self.assertNotIn(SECRET, repr(api.calls))
        return output.getvalue()

    def assert_rejected(self, env):
        api = Api()
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(output):
            with self.assertRaises((ValueError, RuntimeError, SystemExit)) as raised:
                OBSERVER.main(env=env, api=api)
        self.assertEqual(api.calls, [])
        self.assertNotIn(SECRET, output.getvalue())
        self.assertNotIn(SECRET, str(raised.exception))

    def test_success_never_calls_api(self):
        api = Api()
        self.invoke(environment("success"), api)
        self.assertEqual(api.calls, [])

    def test_allowed_events_create_fixed_issue_for_every_red_result(self):
        for event in ("schedule", "workflow_dispatch"):
            for result in ("failure", "cancelled", "skipped"):
                with self.subTest(event=event, result=result):
                    env = environment(result)
                    env["EVENT_NAME"] = event
                    api = Api()
                    self.invoke(env, api)
                    self.assertEqual(len(api.writes()), 1)
                    method, path, payload = api.writes()[0]
                    self.assertEqual(method, "POST")
                    self.assertEqual(path.lstrip("/"), f"repos/{REPOSITORY}/issues")
                    self.assertEqual(payload["title"], TITLE)
                    self.assertEqual(
                        payload["body"],
                        f"Required result: {result}\n"
                        f"Run: https://github.com/{REPOSITORY}/actions/runs/42/attempts/3\n"
                        f"Source SHA: {SHA}\n",
                    )

    def test_real_and_simulated_failure_have_identical_issue_body(self):
        real, simulated = Api(), Api()
        real_env = environment()
        simulated_env = environment()
        real_env["SIMULATE_FAILURE"] = "false"
        simulated_env["SIMULATE_FAILURE"] = "true"
        self.invoke(real_env, real)
        self.invoke(simulated_env, simulated)
        self.assertEqual(real.calls, simulated.calls)

    def test_updates_lowest_number_open_exact_title_non_pr(self):
        api = Api([[
            issue(1, pull_request=True),
            issue(2, state="closed"),
            issue(3, title="Nightly CI red injected"),
            issue(9),
            issue(4),
        ]])
        self.invoke(environment(), api)
        self.assertEqual(len(api.writes()), 1)
        method, path, payload = api.writes()[0]
        self.assertEqual(method, "PATCH")
        self.assertEqual(path.lstrip("/"), f"repos/{REPOSITORY}/issues/4")
        self.assertIn(SHA, payload["body"])

    def test_full_pages_are_scanned_before_selecting_lowest_number(self):
        first_page = [issue(100 + number, title="Other") for number in range(100)]
        first_page[0] = issue(90)
        api = Api([first_page, [issue(7)]])
        self.invoke(environment(), api)
        reads = [call for call in api.calls if call[0] == "GET"]
        self.assertEqual(len(reads), 2)
        for _, path, payload in reads:
            self.assertIn("per_page=100", path)
            self.assertIn("state=open", path)
            self.assertIsNone(payload)
        self.assertEqual(api.writes()[0][1].lstrip("/"), f"repos/{REPOSITORY}/issues/7")

    def test_five_full_pages_fail_closed_without_writing(self):
        full_page = [issue(number + 1) for number in range(100)]
        api = Api([full_page] * 5)
        with self.assertRaises((ValueError, RuntimeError, SystemExit)):
            self.invoke(environment(), api)
        self.assertEqual(len(api.calls), 5)
        self.assertEqual(api.writes(), [])

    def test_short_fifth_page_completes_scan(self):
        full_page = [issue(number + 100, title="Other") for number in range(100)]
        api = Api([full_page] * 4 + [[issue(8)]])
        self.invoke(environment(), api)
        self.assertEqual(len(api.calls), 6)
        self.assertEqual(api.writes()[0][1].lstrip("/"), f"repos/{REPOSITORY}/issues/8")

    def test_invalid_issue_list_or_matching_number_fails_closed(self):
        malformed_pages = [
            {"message": "API error"},
            [issue(number + 1) for number in range(101)],
            [issue(True)],
            [issue(0)],
            [issue("8")],
        ]
        for page in malformed_pages:
            with self.subTest(page=page):
                api = Api([page])
                with self.assertRaises(RuntimeError):
                    self.invoke(environment(), api)
                self.assertEqual(api.writes(), [])

    def test_source_sha_is_normalized_before_publication(self):
        env = environment()
        env["SOURCE_SHA"] = "A" * 40
        api = Api()
        self.invoke(env, api)
        self.assertIn(f"Source SHA: {SHA}\n", api.writes()[0][2]["body"])

    def test_rejects_untrusted_identity_before_api(self):
        cases = {
            "APPROVED_REPOSITORY": ("", "tailrocks/other", "tailrocks/velnor-new/../other"),
            "GITHUB_REPOSITORY": ("", "Tailrocks/velnor-new", "tailrocks/other"),
            "EVENT_NAME": ("push", "pull_request", "pull_request_target", "schedule\n"),
            "REF": ("refs/heads/other", "refs/tags/main", "refs/heads/main\n"),
            "REF_PROTECTED": ("false", "True", "1", ""),
            "SOURCE_SHA": ("a" * 39, "a" * 41, "z" * 40, "a" * 40 + "\n"),
            "RUN_ID": ("0", "-1", "1.0", "42/../../issues", "42\n"),
            "RUN_ATTEMPT": ("0", "-1", "1.0", "3\n"),
            "REQUIRED_RESULT": ("", "pending", "success\n", "failure\nsecret"),
        }
        for key, values in cases.items():
            for value in values:
                with self.subTest(key=key, value=value):
                    env = environment()
                    env[key] = value
                    self.assert_rejected(env)

    def test_missing_required_fields_rejected_before_api(self):
        for key in environment():
            if key == "GH_TOKEN":
                continue
            with self.subTest(key=key):
                env = environment()
                del env[key]
                self.assert_rejected(env)

    def test_success_still_validates_trusted_context(self):
        env = environment("success")
        env["REF_PROTECTED"] = "false"
        self.assert_rejected(env)

    def test_ambient_logs_and_token_never_enter_issue_body(self):
        env = environment()
        env["LOGS"] = "untrusted workflow log " + SECRET
        env["ERROR_MESSAGE"] = SECRET
        api = Api()
        self.invoke(env, api)
        body = api.writes()[0][2]["body"]
        self.assertNotIn("untrusted workflow log", body)
        self.assertNotIn(SECRET, body)


if __name__ == "__main__":
    unittest.main()
